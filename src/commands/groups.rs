use whatsapp_rust::bot::MessageContext;

use super::{is_owner, BotContext, BoxFut, Command, CommandResult};

/// Diagnostik: daftar semua grup yang diikuti akun bot, lengkap dengan JID.
pub struct Groups;

impl Command for Groups {
    fn name(&self) -> &'static str {
        "groups"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["grup", "listgroup"]
    }

    fn description(&self) -> &'static str {
        "Daftar grup yang diikuti bot (diagnostik)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            if !is_owner(ctx, bot).await {
                return Ok(());
            }

            match ctx.client.groups().list_participating().await {
                Ok(list) => {
                    let mut out = format!("*Daftar Grup ({}):*\n\n", list.len());
                    for g in &list {
                        let subject = g.subject.as_deref().unwrap_or("(tanpa nama)");
                        let members = g
                            .participant_count
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "?".to_string());
                        out.push_str(&format!(
                            "• {subject}\n  `{}`\n  {members} member\n\n",
                            g.id
                        ));
                    }
                    ctx.reply(out).await?;
                }
                Err(e) => {
                    ctx.reply(format!("Gagal ambil daftar grup: {e}")).await?;
                }
            }

            Ok(())
        })
    }
}
