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
pub mod owner;
pub mod rvo;

pub type CommandResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
pub struct CommandMeta {
    pub name: &'static str,
    pub description: &'static str,
}

const SETTINGS_FILE: &str = "bot_settings.json";

pub fn normalize_phone_number(raw: &str) -> Option<String> {
    let mut digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }

    if digits.starts_with('0') {
        digits = format!("62{}", &digits[1..]);
    } else if digits.starts_with('8') && digits.len() >= 9 && digits.len() <= 13 {
        digits = format!("62{}", digits);
    }

    if digits.len() < 8 || digits.len() > 16 {
        return None;
    }

    Some(digits)
}

fn load_settings() -> (bool, Vec<String>) {
    if let Ok(content) = std::fs::read_to_string(SETTINGS_FILE) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            let private_mode = val.get("private_mode").and_then(|v| v.as_bool()).unwrap_or(false);
            let owners = val
                .get("owners")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            return (private_mode, owners);
        }
    }
    (false, Vec::new())
}

fn save_settings(private_mode: bool, owners: &[String]) {
    let val = serde_json::json!({
        "private_mode": private_mode,
        "owners": owners,
    });
    if let Ok(j) = serde_json::to_string_pretty(&val) {
        let _ = std::fs::write(SETTINGS_FILE, j);
    }
}

pub struct BotContext {
    pub start_time: Instant,
    pub commands: Vec<CommandMeta>,
    pub media_permits: Arc<Semaphore>,
    pub private_mode: Arc<AtomicBool>,
    pub bot_number: Option<String>,
    pub extra_owners: Arc<std::sync::RwLock<Vec<String>>>,
    pub last_view_once: Arc<std::sync::RwLock<std::collections::HashMap<String, Arc<whatsapp_rust::prelude::wa::Message>>>>,
}

impl BotContext {
    pub fn is_private_mode(&self) -> bool {
        self.private_mode.load(Ordering::Relaxed)
    }

    pub fn set_private_mode(&self, enabled: bool) {
        self.private_mode.store(enabled, Ordering::Relaxed);
        let owners = self.get_owners();
        save_settings(enabled, &owners);
    }

    pub fn get_owners(&self) -> Vec<String> {
        self.extra_owners.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn add_owner(&self, num: String) -> bool {
        if let Ok(mut g) = self.extra_owners.write() {
            if !g.contains(&num) {
                g.push(num);
                let owners = g.clone();
                let priv_mode = self.is_private_mode();
                save_settings(priv_mode, &owners);
                return true;
            }
        }
        false
    }

    pub fn remove_owner(&self, num: &str) -> bool {
        if let Ok(mut g) = self.extra_owners.write() {
            let initial_len = g.len();
            g.retain(|x| x != num);
            if g.len() != initial_len {
                let owners = g.clone();
                let priv_mode = self.is_private_mode();
                save_settings(priv_mode, &owners);
                return true;
            }
        }
        false
    }
    pub fn store_view_once(&self, chat: String, msg: Arc<whatsapp_rust::prelude::wa::Message>) {
        if let Ok(mut g) = self.last_view_once.write() {
            if g.len() > 100 {
                g.clear();
            }
            g.insert(chat, msg);
        }
    }

    pub fn get_last_view_once(&self, chat: &str) -> Option<Arc<whatsapp_rust::prelude::wa::Message>> {
        self.last_view_once.read().ok()?.get(chat).cloned()
    }
}
pub fn is_group_chat(chat: &whatsapp_rust::Jid) -> bool {
    chat.server == whatsapp_rust::Server::Group || chat.to_string().ends_with("@g.us")
}

pub async fn is_owner(ctx: &MessageContext, bot: &BotContext) -> bool {
    // 1. Pesan dikirim dari akun bot itu sendiri (is_from_me)
    if ctx.info.source.is_from_me {
        return true;
    }

    let sender = &ctx.info.source.sender;
    let sender_user = sender.user_base();

    // 2. Sender sama dengan nomor pairing terdaftar di sesi client
    if let Some(bot_pn) = ctx.client.pn() {
        if sender_user == bot_pn.user_base() {
            return true;
        }
    }
    if let Some(bot_lid) = ctx.client.lid() {
        if sender_user == bot_lid.user_base() {
            return true;
        }
    }

    // 3. Sender sama dengan nomor bot yang diberikan saat startup
    if let Some(num) = &bot.bot_number {
        if sender_user == num.as_str() {
            return true;
        }
    }

    // 4. Optional environment variable OWNER_NUMBER
    if let Ok(owner_env) = std::env::var("OWNER_NUMBER") {
        let clean: String = owner_env.chars().filter(|c| c.is_ascii_digit()).collect();
        if !clean.is_empty() && sender_user == clean.as_str() {
            return true;
        }
    }

    // 5. Cek langsung sender_user di daftar extra_owners
    if let Ok(owners) = bot.extra_owners.read() {
        if owners.iter().any(|o| o == sender_user) {
            return true;
        }
    }

    // 6. Jika sender adalah LID (@lid) atau format lain, lookup nomor HP via get_lid_pn_entry
    if let Ok(Some(entry)) = ctx.client.get_lid_pn_entry(sender).await {
        let phone = &entry.phone_number;
        let lid = &entry.lid;

        if let Some(bot_pn) = ctx.client.pn() {
            if phone.as_ref() == bot_pn.user_base() {
                return true;
            }
        }
        if let Some(num) = &bot.bot_number {
            if phone.as_ref() == num.as_str() {
                return true;
            }
        }

        if let Ok(owners) = bot.extra_owners.read() {
            if owners.iter().any(|o| o == phone.as_ref() || o == lid.as_ref()) {
                return true;
            }
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
            Box::new(owner::Owner),
            Box::new(rvo::Rvo),
        ];

        let meta = cmd_list
            .iter()
            .map(|c| CommandMeta {
                name: c.name(),
                description: c.description(),
            })
            .collect();

        let (priv_mode, owners) = load_settings();
        Self {
            commands: cmd_list,
            bot_ctx: BotContext {
                start_time: Instant::now(),
                commands: meta,
                media_permits: Arc::new(Semaphore::new(1)),
                private_mode: Arc::new(AtomicBool::new(priv_mode)),
                bot_number,
                extra_owners: Arc::new(std::sync::RwLock::new(owners)),
                last_view_once: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
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
        if is_group && self.bot_ctx.is_private_mode() && !is_owner(ctx, &self.bot_ctx).await {
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