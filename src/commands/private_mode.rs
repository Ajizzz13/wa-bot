use whatsapp_rust::bot::MessageContext;

use super::{is_owner, BotContext, BoxFut, Command, CommandResult};

pub struct PrivateMode;

impl Command for PrivateMode {
    fn name(&self) -> &'static str {
        "private"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["mode", "privat", "botmode"]
    }

    fn description(&self) -> &'static str {
        "Atur mode akses bot (hanya chat pribadi atau publik)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            if !is_owner(ctx, bot).await {
                ctx.reply_quoting("❌ Perintah ini khusus untuk *Owner* bot.").await?;
                return Ok(());
            }

            let arg = args.trim().to_lowercase();
            match arg.as_str() {
                "on" | "aktif" | "1" | "enable" => {
                    bot.set_private_mode(true);
                    ctx.reply_quoting(
                        "🔒 *Private Mode: AKTIF*\n\n\
                         Bot sekarang hanya dapat diakses melalui chat pribadi (DM).\n\
                         Perintah di dalam grup dinonaktifkan untuk publik."
                    ).await?;
                }
                "off" | "mati" | "0" | "disable" => {
                    bot.set_private_mode(false);
                    ctx.reply_quoting(
                        "🌐 *Private Mode: NONAKTIF*\n\n\
                         Bot sekarang dapat diakses secara publik (di grup maupun chat pribadi)."
                    ).await?;
                }
                _ => {
                    let status_str = if bot.is_private_mode() {
                        "🔒 *Aktif (ON)* — Hanya Chat Pribadi"
                    } else {
                        "🌐 *Nonaktif (OFF)* — Publik (Grup & Pribadi)"
                    };

                    let text = format!(
                        "⚙️ *PENGATURAN AKSES BOT*\n\n\
                         Status: {status_str}\n\n\
                         *Cara Mengubah:*\n\
                         • */private on* — Kunci akses ke chat pribadi\n\
                         • */private off* — Buka akses ke grup & pribadi"
                    );
                    ctx.reply_quoting(text).await?;
                }
            }
            Ok(())
        })
    }
}
