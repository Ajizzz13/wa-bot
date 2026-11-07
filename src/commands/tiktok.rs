use regex::Regex;
use serde_json::Value;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct TikTok;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

struct ExtractedVideo {
    title: String,
    stream_url: String,
}

fn try_extract_native(url: &str) -> Option<(ExtractedVideo, ureq::Agent)> {
    let agent: ureq::Agent = ureq::config::Config::builder()
        .build()
        .into();

    let mut resp = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.5")
        .call()
        .ok()?;

    let html = resp.body_mut().with_config().limit(2 * 1024 * 1024).read_to_string().ok()?;
    let re = Regex::new(r#"<script id="__UNIVERSAL_DATA_FOR_REHYDRATION__"[^>]*>(.*?)</script>"#).ok()?;
    let captures = re.captures(&html)?;
    let json_str = captures.get(1)?.as_str();

    let v: Value = serde_json::from_str(json_str).ok()?;
    let item = v.get("__DEFAULT_SCOPE__")?
        .get("webapp.video-detail")?
        .get("itemInfo")?
        .get("itemStruct")?;

    let title = item.get("desc")
        .and_then(|t| t.as_str())
        .unwrap_or("TikTok Video")
        .to_string();

    let play_addr = item.get("video")?
        .get("playAddr")?
        .as_str()?
        .to_string();

    Some((ExtractedVideo { title, stream_url: play_addr }, agent))
}

fn try_extract_tikwm(url: &str) -> Option<(ExtractedVideo, ureq::Agent)> {
    let agent: ureq::Agent = ureq::config::Config::builder()
        .build()
        .into();

    let api_url = format!("https://www.tikwm.com/api/?url={}", url);
    let mut resp = agent
        .get(&api_url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?;

    let json_str = resp.body_mut().with_config().limit(1024 * 1024).read_to_string().ok()?;
    let v: Value = serde_json::from_str(&json_str).ok()?;

    if v.get("code").and_then(|c| c.as_i64()) != Some(0) {
        return None;
    }

    let data = v.get("data")?;
    let stream_url = data.get("play")?.as_str()?.to_string();
    let title = data.get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("TikTok Video")
        .to_string();

    Some((ExtractedVideo { title, stream_url }, agent))
}

impl Command for TikTok {
    fn name(&self) -> &'static str {
        "tt"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["tiktok"]
    }

    fn description(&self) -> &'static str {
        "Download video TikTok tanpa watermark: /tt <link>"
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
                ctx.reply_quoting("❌ Masukkan URL TikTok yang valid.\nContoh: `/tt https://www.tiktok.com/@user/video/123` atau `/tt https://vt.tiktok.com/xxx`").await?;
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
                let (extracted, agent) = match try_extract_native(url) {
                    Some(res) => res,
                    None => match try_extract_tikwm(url) {
                        Some(res) => res,
                        None => {
                            ctx.reply_quoting("❌ Gagal mengekstrak video TikTok. Pastikan video publik dan link dapat diakses.").await?;
                            return Ok(());
                        }
                    },
                };
                crate::media_memory_checkpoint("tt extracted");

                let mut stream_resp = match agent
                    .get(&extracted.stream_url)
                    .header("User-Agent", USER_AGENT)
                    .header("Referer", "https://www.tiktok.com/")
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
                crate::media_memory_checkpoint("tt downloaded");

                let upload = ctx
                    .client
                    .upload(video_bytes, MediaType::Video, Default::default())
                    .await?;
                crate::media_memory_checkpoint("tt uploaded");

                let mut video = wa::message::VideoMessage::default();
                video.url = Some(upload.url);
                video.direct_path = Some(upload.direct_path);
                video.media_key = Some(upload.media_key.to_vec());
                video.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
                video.file_sha256 = Some(upload.file_sha256.to_vec());
                video.file_length = Some(upload.file_length);
                video.media_key_timestamp = Some(upload.media_key_timestamp);
                video.mimetype = Some("video/mp4".to_string());
                video.caption = Some(format!("🎬 *{}*", extracted.title));

                let mut message = wa::Message::default();
                message.video_message = MessageField::some(video);

                ctx.send_message(message).await?;
            }
            crate::media_memory_checkpoint("tt sent");

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
