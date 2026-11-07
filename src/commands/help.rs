use whatsapp_rust::bot::MessageContext;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Help;

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let rem_secs = secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}h"));
    }
    if hours > 0 {
        parts.push(format!("{hours}j"));
    }
    if mins > 0 {
        parts.push(format!("{mins}m"));
    }
    parts.push(format!("{rem_secs}d"));
    parts.join(" ")
}

impl Command for Help {
    fn name(&self) -> &'static str {
        "help"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["menu", "bantuan"]
    }

    fn description(&self) -> &'static str {
        "Menampilkan dashboard panduan dan daftar menu bot"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let sender_raw = ctx.info.source.sender.to_string();
            let phone = sender_raw
                .split('@')
                .next()
                .unwrap_or(&sender_raw)
                .split(':')
                .next()
                .unwrap_or(&sender_raw);

            let uptime = format_uptime(bot.start_time.elapsed().as_secs());
            let total_cmd = bot.commands.len();
            let is_user_owner = super::is_owner(ctx, bot);
            let role_label = if is_user_owner { "Owner" } else { "User" };

            let menu = format!(
                "╭─[ *Z-BOT ASSISTANT* ]─╮\n\
                 │ 👤 *User:* @{phone}\n\
                 │ 👑 *Role:* {role_label}\n\
                 │ ⏱️ *Uptime:* {uptime}\n\
                 │ ⚡ *Engine:* Rust 1.94\n\
                 │ 📦 *Total:* {total_cmd} Fitur\n\
                 ╰───────────────────────╯\n\n\
                 ───[ *📥 DOWNLOADER* ]───\n\
                 • */yt* `<link>`\n\
                   Unduh video YouTube (MP4)\n\
                 • */ytmp3* `<link>`\n\
                   Unduh audio YouTube (MP4/MP3)\n\
                 • */tt* `<link>`\n\
                   Unduh TikTok tanpa watermark\n\
                 • */ig* `<link>`\n\
                   Unduh Instagram Reel / Post\n\
                 • */fb* `<link>`\n\
                   Unduh video publik Facebook\n\n\
                 ───[ *🎨 CREATIVE* ]───\n\
                 • */s* `[teks | teks]`\n\
                   Konversi foto/video ke stiker\n\
                 • */brat* `<teks>`\n\
                   Buat stiker teks gaya Brat\n\n\
                 ───[ *⚙️ SYSTEM* ]───\n\
                 • */ping*\n\
                   Cek kecepatan respon & RAM\n\
                 • */uptime*\n\
                   Lihat waktu aktif server\n\
                 • */menu* / */help*\n\
                   Tampilkan panduan ini\n\n\
                 ───[ 👑 *OWNER* ]───\n\
                 • */private* `[on/off]`\n\
                   Kunci akses grup / privat\n\n\
                 ───[ 💡 *TIPS* ]───\n\
                 • Reply media dengan command\n\
                 • Meme: */s teks atas | bawah*"
            );

            ctx.reply_quoting(menu).await?;
            Ok(())
        })
    }
}
