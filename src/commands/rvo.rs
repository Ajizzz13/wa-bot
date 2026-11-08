use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Rvo;

enum TargetMedia<'a> {
    Image(&'a wa::message::ImageMessage),
    Video(&'a wa::message::VideoMessage),
    Audio(&'a wa::message::AudioMessage),
}

fn find_media_in_message<'a>(msg: &'a wa::Message) -> Option<TargetMedia<'a>> {
    let base = msg.get_base_message();
    if let Some(img) = base.image_message.as_option() {
        return Some(TargetMedia::Image(img));
    }
    if let Some(vid) = base.video_message.as_option() {
        return Some(TargetMedia::Video(vid));
    }
    if let Some(aud) = base.audio_message.as_option() {
        return Some(TargetMedia::Audio(aud));
    }
    None
}

fn resolve_target_media<'a>(
    ctx: &'a MessageContext,
    cached: Option<&'a Arc<wa::Message>>,
) -> Option<TargetMedia<'a>> {
    let base = ctx.message.get_base_message();

    // 1. Cek pesan yang di-quote/reply (extended_text_message)
    if let Some(ext) = base.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if let Some(media) = find_media_in_message(quoted) {
                    return Some(media);
                }
            }
        }
    }

    // 2. Cek pesan yang di-quote dalam pesan media lain
    if let Some(img) = base.image_message.as_option() {
        if let Some(ctx_info) = img.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if let Some(media) = find_media_in_message(quoted) {
                    return Some(media);
                }
            }
        }
    }
    if let Some(vid) = base.video_message.as_option() {
        if let Some(ctx_info) = vid.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if let Some(media) = find_media_in_message(quoted) {
                    return Some(media);
                }
            }
        }
    }

    // 3. Cek pesan saat ini (jika user mengirim media view-once dengan caption /rvo)
    if let Some(media) = find_media_in_message(&ctx.message) {
        return Some(media);
    }

    // 4. Fallback ke pesan view-once terakhir di chat ini yang tersimpan di cache memori
    if let Some(cached_msg) = cached {
        if let Some(media) = find_media_in_message(cached_msg.as_ref()) {
            return Some(media);
        }
    }

    None
}

impl Command for Rvo {
    fn name(&self) -> &'static str {
        "rvo"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["viewonce", "vo", "readviewonce", "antiviewonce"]
    }

    fn description(&self) -> &'static str {
        "Buka & simpan foto/video 1x lihat: reply media dengan /rvo"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        _args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let chat_key = ctx.info.source.chat.to_string();
            let cached = bot.get_last_view_once(&chat_key);

            let target = match resolve_target_media(ctx, cached.as_ref()) {
                Some(m) => m,
                None => {
                    ctx.reply_quoting(
                        "❌ Tidak ditemukan foto atau video 1x lihat.\n\n\
                         *Cara Pakai:*\n\
                         • Reply/balas pesan 1x lihat dengan */rvo*\n\
                         • Atau ketik */rvo* langsung setelah ada yang mengirim media 1x lihat di chat ini."
                    ).await?;
                    return Ok(());
                }
            };

            let _ = ctx.react("⏳").await;
            let _media_permit = bot
                .media_permits
                .clone()
                .acquire_owned()
                .await
                .expect("media semaphore must remain open");
            let _media_heap = super::MediaHeapGuard;

            match target {
                TargetMedia::Image(img) => {
                    let bytes = match ctx.client.download(img).await {
                        Ok(b) => b,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal mengunduh foto 1x lihat: {e}")).await?;
                            return Ok(());
                        }
                    };

                    let upload = match ctx.client.upload(bytes, MediaType::Image, Default::default()).await {
                        Ok(u) => u,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal memproses ulang foto: {e}")).await?;
                            return Ok(());
                        }
                    };

                    let mut new_img = wa::message::ImageMessage::default();
                    new_img.url = Some(upload.url);
                    new_img.direct_path = Some(upload.direct_path);
                    new_img.media_key = Some(upload.media_key.to_vec());
                    new_img.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    new_img.file_sha256 = Some(upload.file_sha256.to_vec());
                    new_img.file_length = Some(upload.file_length);
                    new_img.media_key_timestamp = Some(upload.media_key_timestamp);
                    new_img.mimetype = img.mimetype.clone().or_else(|| Some("image/jpeg".to_string()));

                    let caption = match &img.caption {
                        Some(cap) if !cap.trim().is_empty() => {
                            format!("🔓 *Foto 1x Lihat Terbuka*\n\n{cap}")
                        }
                        _ => "🔓 *Foto 1x Lihat Terbuka*".to_string(),
                    };
                    new_img.caption = Some(caption);
                    new_img.view_once = Some(false);

                    let mut message = wa::Message::default();
                    message.image_message = MessageField::some(new_img);
                    ctx.send_message(message).await?;
                }
                TargetMedia::Video(vid) => {
                    let bytes = match ctx.client.download(vid).await {
                        Ok(b) => b,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal mengunduh video 1x lihat: {e}")).await?;
                            return Ok(());
                        }
                    };

                    let upload = match ctx.client.upload(bytes, MediaType::Video, Default::default()).await {
                        Ok(u) => u,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal memproses ulang video: {e}")).await?;
                            return Ok(());
                        }
                    };

                    let mut new_vid = wa::message::VideoMessage::default();
                    new_vid.url = Some(upload.url);
                    new_vid.direct_path = Some(upload.direct_path);
                    new_vid.media_key = Some(upload.media_key.to_vec());
                    new_vid.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    new_vid.file_sha256 = Some(upload.file_sha256.to_vec());
                    new_vid.file_length = Some(upload.file_length);
                    new_vid.media_key_timestamp = Some(upload.media_key_timestamp);
                    new_vid.mimetype = vid.mimetype.clone().or_else(|| Some("video/mp4".to_string()));

                    let caption = match &vid.caption {
                        Some(cap) if !cap.trim().is_empty() => {
                            format!("🔓 *Video 1x Lihat Terbuka*\n\n{cap}")
                        }
                        _ => "🔓 *Video 1x Lihat Terbuka*".to_string(),
                    };
                    new_vid.caption = Some(caption);
                    new_vid.view_once = Some(false);

                    let mut message = wa::Message::default();
                    message.video_message = MessageField::some(new_vid);
                    ctx.send_message(message).await?;
                }
                TargetMedia::Audio(aud) => {
                    let bytes = match ctx.client.download(aud).await {
                        Ok(b) => b,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal mengunduh voice note 1x lihat: {e}")).await?;
                            return Ok(());
                        }
                    };

                    let upload = match ctx.client.upload(bytes, MediaType::Audio, Default::default()).await {
                        Ok(u) => u,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal memproses ulang audio: {e}")).await?;
                            return Ok(());
                        }
                    };

                    let mut new_aud = wa::message::AudioMessage::default();
                    new_aud.url = Some(upload.url);
                    new_aud.direct_path = Some(upload.direct_path);
                    new_aud.media_key = Some(upload.media_key.to_vec());
                    new_aud.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                    new_aud.file_sha256 = Some(upload.file_sha256.to_vec());
                    new_aud.file_length = Some(upload.file_length);
                    new_aud.media_key_timestamp = Some(upload.media_key_timestamp);
                    new_aud.mimetype = aud.mimetype.clone().or_else(|| Some("audio/ogg; codecs=opus".to_string()));
                    new_aud.ptt = aud.ptt.or(Some(true));
                    new_aud.view_once = Some(false);

                    let mut message = wa::Message::default();
                    message.audio_message = MessageField::some(new_aud);
                    ctx.reply_quoting("🔓 *Voice Note 1x Lihat Terbuka*").await?;
                    ctx.send_message(message).await?;
                }
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
