use whatsapp_rust::bot::MessageContext;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Ping;

impl Command for Ping {
    fn name(&self) -> &'static str {
        "ping"
    }

    fn description(&self) -> &'static str {
        "Cek responsivitas bot (balas pong)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        _bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            ctx.reply_quoting("🏓 Pong! Bot aktif.").await?;
            Ok(())
        })
    }
}
