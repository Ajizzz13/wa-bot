use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;

use super::{is_owner, normalize_phone_number, BotContext, BoxFut, Command, CommandResult};

pub struct Owner;

async fn extract_target_phone(ctx: &MessageContext, arg: &str) -> Option<String> {
    let arg_trimmed = arg.trim();
    if !arg_trimmed.is_empty() {
        if let Some(norm) = normalize_phone_number(arg_trimmed) {
            return Some(norm);
        }
    }

    let base = ctx.message.get_base_message();
    if let Some(ext) = base.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(part) = &ctx_info.participant {
                if let Ok(jid) = part.parse::<whatsapp_rust::Jid>() {
                    if let Ok(Some(entry)) = ctx.client.get_lid_pn_entry(&jid).await {
                        return Some(entry.phone_number.to_string());
                    }
                }
                if let Some(norm) = normalize_phone_number(part) {
                    return Some(norm);
                }
            }
            if let Some(first_mention) = ctx_info.mentioned_jid.first() {
                if let Ok(jid) = first_mention.parse::<whatsapp_rust::Jid>() {
                    if let Ok(Some(entry)) = ctx.client.get_lid_pn_entry(&jid).await {
                        return Some(entry.phone_number.to_string());
                    }
                }
                if let Some(norm) = normalize_phone_number(first_mention) {
                    return Some(norm);
                }
            }
        }
    }

    None
}

impl Command for Owner {
    fn name(&self) -> &'static str {
        "owner"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["owners", "manageowner"]
    }

    fn description(&self) -> &'static str {
        "Kelola nomor owner bot: /owner add|delete|list <nomor>"
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

            let (subcmd, rest) = match args.trim().split_once(' ') {
                Some((s, r)) => (s.trim().to_lowercase(), r.trim()),
                None => (args.trim().to_lowercase(), ""),
            };

            match subcmd.as_str() {
                "add" | "tambah" => {
                    let target = extract_target_phone(ctx, rest).await;
                    match target {
                        Some(num) => {
                            if bot.add_owner(num.clone()) {
                                ctx.reply_quoting(format!("✅ Berhasil menambahkan @{num} ke daftar Owner.")).await?;
                            } else {
                                ctx.reply_quoting(format!("⚠️ Nomor @{num} sudah terdaftar sebagai Owner.")).await?;
                            }
                        }
                        None => {
                            ctx.reply_quoting(
                                "❌ Masukkan nomor yang valid atau reply pesan user.\n\
                                 Format didukung: 08..., +62..., 62..., spasi, atau tanda (-).\n\
                                 Contoh: `/owner add 0812-3456-7890`"
                            ).await?;
                        }
                    }
                }
                "delete" | "del" | "remove" | "rm" | "hapus" => {
                    let target = extract_target_phone(ctx, rest).await;
                    match target {
                        Some(num) => {
                            if bot.remove_owner(&num) {
                                ctx.reply_quoting(format!("🗑️ Berhasil menghapus @{num} dari daftar Owner.")).await?;
                            } else {
                                ctx.reply_quoting(format!("❌ Nomor @{num} tidak ditemukan di daftar Owner tambahan.")).await?;
                            }
                        }
                        None => {
                            ctx.reply_quoting(
                                "❌ Masukkan nomor yang valid atau reply pesan user.\n\
                                 Contoh: `/owner delete 0812-3456-7890`"
                            ).await?;
                        }
                    }
                }
                "list" | "daftar" => {
                    let mut list_text = String::new();
                    list_text.push_str("╭───[ 👑 *DAFTAR OWNER BOT* ]───\n");

                    // Owner Utama (Nomor Bot / Client PN)
                    if let Some(bot_pn) = ctx.client.pn() {
                        list_text.push_str(&format!("│ • @{} (Bot / Utama)\n", bot_pn.user_base()));
                    } else if let Some(num) = &bot.bot_number {
                        list_text.push_str(&format!("│ • @{num} (Bot / Utama)\n"));
                    }

                    // Owner Tambahan
                    let extras = bot.get_owners();
                    if extras.is_empty() {
                        list_text.push_str("│ _Tidak ada owner tambahan_\n");
                    } else {
                        for num in &extras {
                            list_text.push_str(&format!("│ • @{num} (Owner Tambahan)\n"));
                        }
                    }
                    list_text.push_str("╰─────────────────────────────┈");
                    ctx.reply_quoting(list_text).await?;
                }
                _ => {
                    let guide =
                        "👑 *MANAJEMEN OWNER BOT*\n\n\
                         • */owner add <nomor>*\n\
                           Tambah owner baru (08/62/+62/spasi/-)\n\
                         • */owner delete <nomor>*\n\
                           Hapus nomor dari daftar owner\n\
                         • */owner list*\n\
                           Lihat seluruh owner yang terdaftar\n\n\
                         _Tips: Anda juga bisa mereply pesan seseorang lalu ketik `/owner add`._";
                    ctx.reply_quoting(guide).await?;
                }
            }
            Ok(())
        })
    }
}
