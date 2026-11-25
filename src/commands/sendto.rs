use std::str::FromStr;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::{Freshness, Jid};

use super::{is_owner, BotContext, BoxFut, Command, CommandResult};

/// Diagnostik: kirim pesan ke JID lain (mis. grup yang tidak merespons bot).
pub struct SendTo;

impl Command for SendTo {
    fn name(&self) -> &'static str {
        "sendto"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["kirim"]
    }

    fn description(&self) -> &'static str {
        "Kirim teks ke JID lain (diagnostik, owner only)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            if !is_owner(ctx, bot).await {
                return Ok(());
            }

            let mut parts = args.splitn(2, ' ');
            let jid_str = parts.next().unwrap_or("").trim();
            let text = parts.next().unwrap_or("").trim();

            if jid_str.is_empty() || text.is_empty() {
                ctx.reply("Format: /sendto <jid> <teks>\nContoh: /sendto 120363431048197201@g.us halo")
                    .await?;
                return Ok(());
            }

            let jid = match Jid::from_str(jid_str) {
                Ok(jid) => jid,
                Err(e) => {
                    ctx.reply(format!("JID tidak valid: {e}")).await?;
                    return Ok(());
                }
            };

            // Refresh metadata grup + device list supaya addressing mode (PN/LID)
            // tidak memakai cache lama. Server menolak kiriman LID group dengan
            // NACK 421 (StaleGroupAddressingMode) kalau cache masih PN.
            let request = SendRequest::new(&jid, wa::Message::text(text.to_string())).with_options(
                SendOptions::default()
                    .with_group_metadata_freshness(Freshness::Refresh)
                    .with_device_freshness(Freshness::Refresh),
            );

            match ctx.client.send(request).await {
                Ok(result) => {
                    ctx.reply(format!("Terkirim ke {jid_str} (id: {})", result.message_id))
                        .await?;
                }
                Err(e) => {
                    ctx.reply(format!("Gagal kirim ke {jid_str}: {e}")).await?;
                }
            }

            Ok(())
        })
    }
}
