use std::process::Command as ProcessCommand;

use serde_json::Value;
use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Youtube;

const ANDROID_UA: &str = "com.google.android.youtube/21.26.364 (Linux; U; Android 11) gzip";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

fn extract_video_id(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some(idx) = url.find("v=") {
        let rest = &url[idx + 2..];
        let id = rest.split('&').next().unwrap_or(rest);
        let id = id.split('#').next().unwrap_or(id);
        if id.len() == 11 {
            return Some(id.to_string());
        }
    }

    for prefix in &["/shorts/", "/embed/", "/v/", "youtu.be/"] {
        if let Some(idx) = url.find(prefix) {
            let rest = &url[idx + prefix.len()..];
            let id = rest.split('?').next().unwrap_or(rest);
            let id = id.split('&').next().unwrap_or(id);
            let id = id.split('#').next().unwrap_or(id);
            let id = id.split('/').next().unwrap_or(id);
            if id.len() == 11 {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn download_native_innertube(video_id: &str) -> Result<(String, Vec<u8>), String> {
    let agent: ureq::Agent = ureq::config::Config::builder().build().into();

    let payload = serde_json::json!({
        "videoId": video_id,
        "context": {
            "client": {
                "clientName": "ANDROID",
                "clientVersion": "21.26.364",
                "androidSdkVersion": 30,
                "userAgent": ANDROID_UA,
                "osName": "Android",
                "osVersion": "11"
            }
        }
    });
    let payload_str = payload.to_string();

    let mut resp = agent
        .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
        .header("Content-Type", "application/json")
        .header("User-Agent", ANDROID_UA)
        .send(&payload_str)
        .map_err(|e| format!("request InnerTube gagal: {e}"))?;

    let body_str = resp
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("gagal membaca respons InnerTube: {e}"))?;

    let json_val: Value =
        serde_json::from_str(&body_str).map_err(|e| format!("JSON parse error: {e}"))?;

    let playability = json_val.get("playabilityStatus");
    let status = playability
        .and_then(|p| p.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("UNKNOWN");

    if status != "OK" {
        let reason = playability
            .and_then(|p| p.get("reason"))
            .and_then(|r| r.as_str())
            .unwrap_or("video tidak dapat diputar");
        return Err(format!("{status}: {reason}"));
    }

    let title = json_val
        .get("videoDetails")
        .and_then(|v| v.get("title"))
        .and_then(|t| t.as_str())
        .unwrap_or("YouTube Video")
        .to_string();

    let formats = json_val
        .get("streamingData")
        .and_then(|s| s.get("formats"))
        .and_then(|f| f.as_array())
        .ok_or_else(|| "tidak ada format stream langsung".to_string())?;

    let mut stream_url: Option<String> = None;
    for fmt in formats {
        let mime = fmt.get("mimeType").and_then(|m| m.as_str()).unwrap_or("");
        if mime.contains("mp4") {
            if let Some(u) = fmt.get("url").and_then(|u| u.as_str()) {
                stream_url = Some(u.to_string());
                break;
            }
        }
    }
    if stream_url.is_none() {
        for fmt in formats {
            if let Some(u) = fmt.get("url").and_then(|u| u.as_str()) {
                stream_url = Some(u.to_string());
                break;
            }
        }
    }

    let stream_url = stream_url.ok_or_else(|| "format tidak memiliki URL direct".to_string())?;

    let mut stream_resp = agent
        .get(&stream_url)
        .header("User-Agent", ANDROID_UA)
        .call()
        .map_err(|e| format!("gagal mengunduh stream: {e}"))?;

    let bytes = stream_resp
        .body_mut()
        .with_config()
        .limit(MAX_VIDEO_BYTES)
        .read_to_vec()
        .map_err(|e| format!("gagal membaca buffer video: {e}"))?;

    if bytes.is_empty() {
        return Err("buffer video kosong".to_string());
    }

    Ok((title, bytes))
}

fn download_with_ytdlp_fallback(url: &str) -> Result<(String, Vec<u8>), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp_path = format!("/tmp/yt_wa_{}_{}.mp4", std::process::id(), now);

    struct TempGuard(String);
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = TempGuard(tmp_path.clone());

    let output = ProcessCommand::new("yt-dlp")
        .env("PATH", "/root/.nvm/versions/node/v24.16.0/bin:/usr/local/bin:/usr/bin:/bin")
        .args([
            "--no-warnings",
            "--no-check-certificates",
            "--remote-components", "ejs:github",
            "--js-runtimes", "node",
            "-f", "best[ext=mp4][filesize<60M]/best[height<=720]/best",
            "--merge-output-format", "mp4",
            "--max-filesize", "60M",
            "-o", &tmp_path,
            "--no-simulate",
            "--print", "%(title)s",
            url,
        ])
        .output()
        .map_err(|e| format!("gagal menjalankan yt-dlp fallback: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp error: {}", stderr.trim()));
    }

    let title = String::from_utf8_lossy(&output.stdout)
        .trim()
        .lines()
        .next()
        .unwrap_or("YouTube Video")
        .trim()
        .to_string();

    let bytes = std::fs::read(&tmp_path).map_err(|e| format!("gagal membaca file fallback: {e}"))?;
    if bytes.is_empty() {
        return Err("file fallback kosong".to_string());
    }

    Ok((title, bytes))
}

impl Command for Youtube {
    fn name(&self) -> &'static str {
        "yt"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["youtube", "ytdl"]
    }

    fn description(&self) -> &'static str {
        "Download video YouTube: /yt <link>"
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
                ctx.reply_quoting("❌ Masukkan URL YouTube yang valid.\nContoh: `/yt https://www.youtube.com/watch?v=...`").await?;
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

            let (title, video_bytes) = if let Some(video_id) = extract_video_id(url) {
                match download_native_innertube(&video_id) {
                    Ok(res) => res,
                    Err(_) => match download_with_ytdlp_fallback(url) {
                        Ok(res) => res,
                        Err(e) => {
                            ctx.reply_quoting(format!("❌ Gagal mengunduh video: {e}")).await?;
                            return Ok(());
                        }
                    },
                }
            } else {
                match download_with_ytdlp_fallback(url) {
                    Ok(res) => res,
                    Err(e) => {
                        ctx.reply_quoting(format!("❌ Gagal mengunduh video: {e}")).await?;
                        return Ok(());
                    }
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
            video.caption = Some(format!("🎬 *{}*", title));

            let mut message = wa::Message::default();
            message.video_message = MessageField::some(video);

            ctx.send_message(message).await?;
            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
