use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Semaphore;
use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::MessageExt;

pub mod brat;
pub mod facebook;
pub mod help;
pub mod instagram;
pub mod ping;
pub mod sticker;
pub mod tiktok;
pub mod uptime;
pub mod youtube;
pub mod private_mode;
pub mod ytmp3;

pub type CommandResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
pub struct CommandMeta {
    pub name: &'static str,
    pub description: &'static str,
}

const SETTINGS_FILE: &str = "bot_settings.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SettingsFile {
    private_mode: bool,
}

fn load_settings() -> bool {
    if let Ok(content) = std::fs::read_to_string(SETTINGS_FILE) {
        if let Ok(val) = serde_json::from_str::<SettingsFile>(&content) {
            return val.private_mode;
        }
    }
    false
}

fn save_settings(private_mode: bool) {
    let s = SettingsFile { private_mode };
    if let Ok(j) = serde_json::to_string_pretty(&s) {
        let _ = std::fs::write(SETTINGS_FILE, j);
    }
}

pub struct BotContext {
    pub start_time: Instant,
    pub commands: Vec<CommandMeta>,
    pub media_permits: Arc<Semaphore>,
    pub private_mode: Arc<AtomicBool>,
    pub bot_number: Option<String>,
}

impl BotContext {
    pub fn is_private_mode(&self) -> bool {
        self.private_mode.load(Ordering::Relaxed)
    }

    pub fn set_private_mode(&self, enabled: bool) {
        self.private_mode.store(enabled, Ordering::Relaxed);
        save_settings(enabled);
    }
}

pub fn is_group_chat(chat: &whatsapp_rust::Jid) -> bool {
    chat.server == whatsapp_rust::Server::Group || chat.to_string().ends_with("@g.us")
}

pub fn is_owner(ctx: &MessageContext, bot: &BotContext) -> bool {
    // 1. Pesan dikirim dari akun bot itu sendiri (is_from_me)
    if ctx.info.source.is_from_me {
        return true;
    }

    // 2. Sender sama dengan nomor pairing terdaftar di sesi client
    if let Some(bot_pn) = ctx.client.pn() {
        if ctx.info.source.sender.user_base() == bot_pn.user_base() {
            return true;
        }
    }

    // 3. Sender sama dengan nomor bot yang diberikan saat startup
    if let Some(num) = &bot.bot_number {
        if ctx.info.source.sender.user_base() == num.as_str() {
            return true;
        }
    }

    // 4. Optional environment variable OWNER_NUMBER
    if let Ok(owner_env) = std::env::var("OWNER_NUMBER") {
        let clean: String = owner_env.chars().filter(|c| c.is_ascii_digit()).collect();
        if !clean.is_empty() && ctx.info.source.sender.user_base() == clean.as_str() {
            return true;
        }
    }

    false
}
pub trait Command: Send + Sync {
    fn name(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    fn description(&self) -> &'static str;
    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult>;
}

/// Mengekstrak teks dari pesan biasa atau caption media (gambar/video/dokumen)
pub fn extract_message_text(msg: &whatsapp_rust::prelude::wa::Message) -> Option<&str> {
    if let Some(text) = msg.text_content() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if let Some(caption) = msg.get_caption() {
        let trimmed = caption.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

pub struct MediaHeapGuard;

impl Drop for MediaHeapGuard {
    fn drop(&mut self) {
        #[cfg(target_env = "gnu")]
        unsafe {
            libc::malloc_trim(0);
        }
    }
}

pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
    pub bot_ctx: BotContext,
}

impl CommandRegistry {
    pub fn new(bot_number: Option<String>) -> Self {
        let cmd_list: Vec<Box<dyn Command>> = vec![
            Box::new(ping::Ping),
            Box::new(uptime::Uptime),
            Box::new(sticker::Sticker),
            Box::new(tiktok::TikTok),
            Box::new(facebook::Facebook),
            Box::new(help::Help),
            Box::new(brat::Brat),
            Box::new(youtube::Youtube),
            Box::new(instagram::Instagram),
            Box::new(ytmp3::YtMp3),
            Box::new(private_mode::PrivateMode),
        ];

        let meta = cmd_list
            .iter()
            .map(|c| CommandMeta {
                name: c.name(),
                description: c.description(),
            })
            .collect();

        Self {
            commands: cmd_list,
            bot_ctx: BotContext {
                start_time: Instant::now(),
                commands: meta,
                media_permits: Arc::new(Semaphore::new(1)),
                private_mode: Arc::new(AtomicBool::new(load_settings())),
                bot_number,
            },
        }
    }

    pub async fn dispatch(&self, ctx: &MessageContext) -> bool {
        // Ambil teks baik dari percakapan maupun dari caption media
        let text = match extract_message_text(&ctx.message) {
            Some(t) => t,
            None => return false,
        };

        // WAJIB berawalan "/" agar tidak merespons chat random
        if !text.starts_with('/') {
            return false;
        }
        let is_group = is_group_chat(&ctx.info.source.chat);
        if is_group && self.bot_ctx.is_private_mode() && !is_owner(ctx, &self.bot_ctx) {
            return false;
        }


        let (cmd_trigger, args) = match text.split_once(' ') {
            Some((c, a)) => (c.trim(), a.trim()),
            None => {
                if let Some(pos) = text.find('|') {
                    (text[..pos].trim(), text[pos..].trim())
                } else {
                    (text, "")
                }
            }
        };

        let normalized = cmd_trigger.trim_start_matches('/').to_lowercase();
        if normalized.is_empty() {
            return false;
        }

        for cmd in &self.commands {
            let matches = cmd.name().eq_ignore_ascii_case(&normalized)
                || cmd
                    .aliases()
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&normalized));

            if matches {
                if let Err(e) = cmd.execute(ctx, args, &self.bot_ctx).await {
                    eprintln!("command {} failed: {e}", cmd.name());
                    let _ = ctx.reply(format!("⚠️ Terjadi kesalahan: {e}")).await;
                }
                return true;
            }
        }
        false
    }
}