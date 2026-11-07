use std::io::Read;
use serde_json::Value;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Instagram;

const COBALT_API: &str = "https://api.cobalt.tools/";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

fn try_extract_cobalt(url: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::config::Config::builder()
        .build()
        .into();
    let payload = serde_json::json!({
        "url": url,
        "videoQuality": "720",
        "downloadMode": "auto"
    });
    let payload_str = payload.to_string();
    let mut resp = agent
        .post(COBALT_API)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send(&payload_str)
        .ok()?;
    let json_str = resp.body_mut().with_config().limit(1024 * 1024).read_to_string().ok()?;
    let v: Value = serde_json::from_str(&json_str).ok()?;
    let status = v.get("status")?.as_str()?;
    if status == "error" {
        return None;
    }
    v.get("url")?.as_str().map(|s| s.to_string())
}

impl Command for Instagram {
    fn name(&self) -> &'static str {
        "ig"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["instagram", "reels", "reel"]
    }

    fn description(&self) -> &'static str {
        "Download video/reel Instagram: /ig <link>"
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
                ctx.reply_quoting("❌ Masukkan URL Instagram yang valid.\nContoh: `/ig https://www.instagram.com/reel/...`").await?;
                return Ok(());
            }

            let _ = ctx.react("⏳").await;
            let _media_permit = bot
                .media_permits
                .clone()
                .acquire_owned()
                .await
                .expect("media semaphore must remain open");
            let _media_heap = super::MediaHeapGuard;

            {
                let stream_url = match try_extract_cobalt(url) {
                    Some(u) => u,
                    None => {
                        ctx.reply_quoting("❌ Gagal mengekstrak video Instagram. Pastikan video/reel bersifat publik.").await?;
                        return Ok(());
                    }
                };

                let agent: ureq::Agent = ureq::config::Config::builder()
                    .build()
                    .into();

                let mut stream_resp = match agent
                    .get(&stream_url)
                    .header("User-Agent", USER_AGENT)
                    .call()
                {
                    Ok(r) => r,
                    Err(e) => {
                        ctx.reply_quoting(format!("❌ Gagal mengunduh stream video: {e}")).await?;
                        return Ok(());
                    }
                };

                let video_bytes = match stream_resp
                    .body_mut()
                    .with_config()
                    .limit(MAX_VIDEO_BYTES)
                    .read_to_vec()
                {
                    Ok(b) => b,
                    Err(e) => {
                        ctx.reply_quoting(format!("❌ Video terlalu besar (>60 MB) atau koneksi terputus: {e}")).await?;
                        return Ok(());
                    }
                };

                let upload = ctx
                    .client
                    .upload(video_bytes, MediaType::Video, Default::default())
                    .await?;

                let mut video = wa::message::VideoMessage::default();
                video.url = Some(upload.url);
                video.direct_path = Some(upload.direct_path);
                video.media_key = Some(upload.media_key.to_vec());
                video.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                video.file_sha256 = Some(upload.file_sha256.to_vec());
                video.file_length = Some(upload.file_length);
                video.media_key_timestamp = Some(upload.media_key_timestamp);
                video.mimetype = Some("video/mp4".to_string());
                video.caption = Some("🎬 *Instagram Video*".to_string());

                let mut message = wa::Message::default();
                message.video_message = MessageField::some(video);

                ctx.send_message(message).await?;
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}