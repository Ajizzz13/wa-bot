use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::youtube::{download_native_innertube, extract_video_id};
use super::{BotContext, BoxFut, Command, CommandResult};

pub struct YtMp3;

impl Command for YtMp3 {
    fn name(&self) -> &'static str {
        "ytmp3"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["yta", "ytaudio"]
    }

    fn description(&self) -> &'static str {
        "Download audio YouTube: /ytmp3 <link>"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let url = args.trim();
            if !url.starts_with("http") {
                ctx.reply_quoting("❌ Masukkan URL YouTube yang valid.\nContoh: `/ytmp3 https://www.youtube.com/watch?v=...`").await?;
                return Ok(());
            }

            let video_id = match extract_video_id(url) {
                Some(id) => id,
                None => {
                    ctx.reply_quoting("❌ Tidak dapat mengekstrak video ID dari link YouTube.").await?;
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

            let (title, audio_bytes) = match download_native_innertube(&video_id) {
                Ok(res) => res,
                Err(e) => {
                    ctx.reply_quoting(format!("❌ Gagal mengunduh audio: {e}")).await?;
                    return Ok(());
                }
            };

            let upload = ctx
                .client
                .upload(audio_bytes, MediaType::Audio, Default::default())
                .await?;

            let mut audio = wa::message::AudioMessage::default();
            audio.url = Some(upload.url);
            audio.direct_path = Some(upload.direct_path);
            audio.media_key = Some(upload.media_key.to_vec());
            audio.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
            audio.file_sha256 = Some(upload.file_sha256.to_vec());
            audio.file_length = Some(upload.file_length);
            audio.media_key_timestamp = Some(upload.media_key_timestamp);
            audio.mimetype = Some("audio/mp4".to_string());
            audio.ptt = Some(false);

            let mut message = wa::Message::default();
            message.audio_message = MessageField::some(audio);

            ctx.reply_quoting(format!("🎵 *{}*", title)).await?;
            ctx.send_message(message).await?;
            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
