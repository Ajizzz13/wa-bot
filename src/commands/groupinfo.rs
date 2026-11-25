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

            // Cache routing yang dipakai jalur send (bisa beda dari metadata fresh).
            let routing_line = match ctx.client.groups().routing_info(&jid).await {
                Ok(info) => format!(
                    "{:?} ({} participants)",
                    info.addressing_mode,
                    info.participants.len()
                ),
                Err(e) => format!("gagal: {e}"),
            };

            match ctx.client.groups().fetch_metadata(&jid).await {
                Ok(meta) => {
                    let mut out = format!(
                        "*Group Info*\n\
                         • ID: `{}`\n\
                         • Subject: {}\n\
                         • Participants: {}\n\
                         • Addressing: {:?}\n\
                         • Routing cache: {}\n\
                         • Announcement: {}\n\
                         • Membership approval: {}\n\
                         • Locked: {}\n\
                         • Created: {:?}\n\
                         • Flags: count={:?} missing_id={} incognito={} suspended={} hidden={} capi={} limit_share={} evo={:?} parent={:?} default_sub={} general={}\n\n\
                         *Participants:*\n",
                        meta.id,
                        meta.subject.as_deref().unwrap_or("(tanpa nama)"),
                        meta.participants.len(),
                        meta.addressing_mode,
                        routing_line,
                        meta.is_announcement,
                        meta.membership_approval,
                        meta.is_locked,
                        meta.creation_time,
                        meta.participant_count,
                        meta.has_missing_participant_identification,
                        meta.is_incognito,
                        meta.is_suspended,
                        meta.is_hidden_group,
                        meta.has_capi,
                        meta.is_limit_sharing_enabled,
                        meta.evolution_version,
                        meta.parent_group_jid,
                        meta.is_default_sub_group,
                        meta.is_general_chat,
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
