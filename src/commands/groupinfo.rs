use std::str::FromStr;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::Jid;

use super::{is_owner, BotContext, BoxFut, Command, CommandResult};

/// Diagnostik: tampilkan metadata lengkap satu grup, termasuk status keanggotaan bot.
pub struct GroupInfo;

impl Command for GroupInfo {
    fn name(&self) -> &'static str {
        "groupinfo"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["infogrup"]
    }

    fn description(&self) -> &'static str {
        "Info metadata satu grup (diagnostik, owner only)"
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

            let jid_str = args.trim();
            if jid_str.is_empty() {
                ctx.reply("Format: /groupinfo <jid>\nContoh: /groupinfo 120363431048197201@g.us")
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

            match ctx.client.groups().fetch_metadata(&jid).await {
                Ok(meta) => {
                    let mut out = format!(
                        "*Group Info*\n\
                         • ID: `{}`\n\
                         • Subject: {}\n\
                         • Participants: {}\n\
                         • Addressing: {:?}\n\
                         • Announcement: {}\n\
                         • Membership approval: {}\n\
                         • Locked: {}\n\
                         • Created: {:?}\n\n\
                         *Participants:*\n",
                        meta.id,
                        meta.subject.as_deref().unwrap_or("(tanpa nama)"),
                        meta.participants.len(),
                        meta.addressing_mode,
                        meta.is_announcement,
                        meta.membership_approval,
                        meta.is_locked,
                        meta.creation_time,
                    );

                    for p in meta.participants.iter().take(30) {
                        let phone = p
                            .phone_number
                            .as_ref()
                            .map(|j| j.to_string())
                            .unwrap_or_else(|| "-".to_string());
                        out.push_str(&format!(
                            "• {} | {} | {:?}\n",
                            p.jid, phone, p.participant_type
                        ));
                    }
                    if meta.participants.len() > 30 {
                        out.push_str(&format!(
                            "… dan {} participant lainnya\n",
                            meta.participants.len() - 30
                        ));
                    }

                    ctx.reply(out).await?;
                }
                Err(e) => {
                    ctx.reply(format!("Gagal ambil metadata {jid_str}: {e}"))
                        .await?;
                }
            }

            Ok(())
        })
    }
}
