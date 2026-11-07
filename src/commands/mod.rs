use std::future::Future;
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
pub mod ytmp3;

pub type CommandResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
pub struct CommandMeta {
    pub name: &'static str,
    pub description: &'static str,
}

pub struct BotContext {
    pub start_time: Instant,
    pub commands: Vec<CommandMeta>,
    pub media_permits: Arc<Semaphore>,
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
    pub fn new() -> Self {
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