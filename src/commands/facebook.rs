use std::io::Read;
use regex::Regex;
use serde_json::Value;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Facebook;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

struct ExtractedFbVideo {
    title: String,
    stream_url: String,
}

fn try_extract_native(url: &str) -> Option<ExtractedFbVideo> {
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

    let html = resp.body_mut().with_config().limit(3 * 1024 * 1024).read_to_string().ok()?;
    let clean = html.replace(r"\u0025", "%").replace(r"\/", "/");

    // Ambil title jika ada
    let title = Regex::new(r"<title>(.*?)</title>")
        .ok()
        .and_then(|re| re.captures(&clean))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace(" | Facebook", "").trim().to_string())
        .unwrap_or_else(|| "Facebook Video".to_string());

    // Coba HD terlebih dahulu
    let hd_re = Regex::new(r#""(?:playable_url_quality_hd|browser_native_hd_url|hd_src)":"(https://[^"]+)""#).ok()?;
    if let Some(caps) = hd_re.captures(&clean) {
        if let Some(m) = caps.get(1) {
            return Some(ExtractedFbVideo {
                title,
                stream_url: m.as_str().to_string(),
            });
        }
    }

    // Coba SD
    let sd_re = Regex::new(r#""(?:playable_url|browser_native_sd_url|sd_src)":"(https://[^"]+)""#).ok()?;
    if let Some(caps) = sd_re.captures(&clean) {
        if let Some(m) = caps.get(1) {
            return Some(ExtractedFbVideo {
                title,
                stream_url: m.as_str().to_string(),
            });
        }
    }

    None
}

fn try_extract_fallback(url: &str) -> Option<ExtractedFbVideo> {
    // Fallback menggunakan endpoint Cobalt / SnapSave resolver
    let agent: ureq::Agent = ureq::config::Config::builder()
        .build()
        .into();

    let payload = serde_json::json!({
        "url": url,
        "videoQuality": "720"
    });

    let payload_str = payload.to_string();
    let mut resp = agent
        .post("https://api.cobalt.tools/")
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send(&payload_str)
        .ok()?;

    let json_str = resp.body_mut().with_config().limit(1024 * 1024).read_to_string().ok()?;
    let v: Value = serde_json::from_str(&json_str).ok()?;

    let stream_url = v.get("url")?.as_str()?.to_string();
    let title = v.get("filename")
        .and_then(|f| f.as_str())
        .unwrap_or("Facebook Video")
        .to_string();

    Some(ExtractedFbVideo { title, stream_url })
}

impl Command for Facebook {
    fn name(&self) -> &'static str {
        "fb"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["facebook"]
    }

    fn description(&self) -> &'static str {
        "Download video Facebook / Reels: /fb <link>"
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
                ctx.reply_quoting("❌ Masukkan URL Facebook yang valid.\nContoh: `/fb https://www.facebook.com/watch/?v=...` atau `/fb https://fb.watch/...`").await?;
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
                let extracted = match try_extract_native(url) {
                    Some(res) => res,
                    None => match try_extract_fallback(url) {
                        Some(res) => res,
                        None => {
                            ctx.reply_quoting("❌ Gagal mengekstrak video Facebook. Pastikan video bersifat publik dan tidak dibatasi login/usia.").await?;
                            return Ok(());
                        }
                    },
                };

                let agent: ureq::Agent = ureq::config::Config::builder()
                    .build()
                    .into();

                let mut stream_resp = match agent
                    .get(&extracted.stream_url)
                    .header("User-Agent", USER_AGENT)
                    .call()
                {
                    Ok(r) => r,
                    Err(e) => {
                        ctx.reply_quoting(format!("❌ Gagal mengunduh stream video Facebook: {e}")).await?;
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
                video.caption = Some(format!("🎬 *{}*", extracted.title));

                let mut message = wa::Message::default();
                message.video_message = MessageField::some(video);

                ctx.send_message(message).await?;
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
