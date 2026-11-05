use whatsapp_rust::bot::MessageContext;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Help;

impl Command for Help {
    fn name(&self) -> &'static str {
        "help"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["menu"]
    }

    fn description(&self) -> &'static str {
        "Menampilkan daftar perintah bot yang tersedia"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let mut list = String::new();
            for meta in &bot.commands {
                list.push_str(&format!("• */{}* — {}\n", meta.name, meta.description));
            }

            let text = format!(
                "🤖 *Daftar Perintah WhatsApp Bot*\n\n\
                 {list}\n\
                 _Tips: `/s teks atas | teks bawah` atau `/s | teks bawah` untuk stiker bermeme._"
            );

            ctx.reply_quoting(text).await?;
            Ok(())
        })
    }
}
