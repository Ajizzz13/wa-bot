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

            let menu = format!(
                "╭───「 ✦ *Z-BOT ASSISTANT* ✦ 」\n\
                 │\n\
                 │ 👤 *User:* @{phone}\n\
                 │ ⏱️ *Uptime:* {uptime}\n\
                 │ ⚡ *Engine:* Rust 1.94 (Async Native)\n\
                 │ 📦 *Commands:* {total_cmd} Fitur Siap Pakai\n\
                 │ 📡 *Status:* Online & Responsive\n\
                 │\n\
                 ╰──────────────────────┈⊷\n\n\
                 ╭──「 📥 *MEDIA DOWNLOADER* 」\n\
                 │ ✦ */yt* `<link>`\n\
                 │   └ _Download video/shorts YouTube (MP4)_\n\
                 │ ✦ */tt* `<link>`\n\
                 │   └ _Download video TikTok tanpa watermark_\n\
                 │ ✦ */ig* `<link>`\n\
                 │   └ _Download reel / post Instagram (MP4)_\n\
                 │ ✦ */fb* `<link>`\n\
                 │   └ _Download video publik Facebook_\n\
                 ╰──────────────────────┈⊷\n\n\
                 ╭──「 🎨 *CREATIVE & STICKER* 」\n\
                 │ ✦ */s* `[teks | teks]`\n\
                 │   └ _Konversi gambar/video ke WebP stiker_\n\
                 │ ✦ */brat* `<teks>`\n\
                 │   └ _Generate stiker teks gaya Brat aesthetic_\n\
                 ╰──────────────────────┈⊷\n\n\
                 ╭──「 ⚙️ *SYSTEM & UTILITY* 」\n\
                 │ ✦ */ping*\n\
                 │   └ _Cek kecepatan respon & latensi bot_\n\
                 │ ✦ */uptime*\n\
                 │   └ _Lihat runtime aktif & pemakaian RAM_\n\
                 │ ✦ */menu* / */help*\n\
                 │   └ _Tampilkan dashboard panduan ini_\n\
                 ╰──────────────────────┈⊷\n\n\
                 ╭──「 💡 *TIPS PENGGUNAAN* 」\n\
                 │ • Kirim/reply foto atau video dengan */s*\n\
                 │ • Buat meme: */s teks atas | teks bawah*\n\
                 │ • Pastikan tautan media bersifat publik\n\
                 ╰──────────────────────┈⊷"
            );

            ctx.reply_quoting(menu).await?;
            Ok(())
        })
    }
}
