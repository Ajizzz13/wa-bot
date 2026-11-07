use std::process::Command as ProcessCommand;

use serde_json::Value;
use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Instagram;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

fn extract_shortcode(url: &str) -> Option<&str> {
    let patterns = ["/reel/", "/reels/", "/p/", "/tv/"];
    for p in patterns {
        if let Some(pos) = url.find(p) {
            let rest = &url[pos + p.len()..];
            let code = rest.split('?').next().unwrap_or(rest);
            let code = code.split('&').next().unwrap_or(code);
            let code = code.split('#').next().unwrap_or(code);
            let code = code.trim_matches('/');
            if !code.is_empty() {
                return Some(code);
            }
        }
    }
    None
}

fn id_to_pk(shortcode: &str) -> Option<u64> {
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let code = if shortcode.len() > 28 {
        &shortcode[..shortcode.len() - 28]
    } else {
        shortcode
    };
    let mut pk: u64 = 0;
    for &b in code.as_bytes() {
        let idx = table.iter().position(|&c| c == b)? as u64;
        pk = pk.checked_mul(64)?.checked_add(idx)?;
    }
    Some(pk)
}

fn try_native_info_api(pk: u64) -> Option<String> {
    let agent: ureq::Agent = ureq::config::Config::builder().build().into();
    let url = format!("https://i.instagram.com/api/v1/media/{pk}/info/");
    let mut resp = agent
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("X-IG-App-ID", "936619743392459")
        .header("Accept", "*/*")
        .call()
        .ok()?;

    let mut buf = String::new();
    resp.body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_to_string(&mut buf)
        .ok()?;

    let val: Value = serde_json::from_str(&buf).ok()?;
    let items = val.get("items")?.as_array()?;
    let first = items.first()?;
    let versions = first.get("video_versions")?.as_array()?;
    let best = versions.first()?;
    best.get("url")?.as_str().map(|s| s.to_string())
}

fn download_with_ytdlp(url: &str) -> Result<(String, Vec<u8>), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp_path = format!("/tmp/ig_wa_{}_{}.mp4", std::process::id(), now);

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
            "-f", "best[ext=mp4][filesize<60M]/best[ext=mp4]/best",
            "--merge-output-format", "mp4",
            "--max-filesize", "60M",
            "-o", &tmp_path,
            "--no-simulate",
            "--print", "%(title)s",
            url,
        ])
        .output()
        .map_err(|e| format!("gagal menjalankan yt-dlp: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let trimmed = stderr.trim();
        if trimmed.contains("There is no video in this post") {
            return Err("postingan ini bukan video/reel (hanya foto/teks)".to_string());
        }
        if trimmed.contains("Instagram sent an empty media response") || trimmed.contains("login") {
            return Err("Instagram membatasi akses tanpa login untuk postingan ini".to_string());
        }
        return Err(format!("yt-dlp error: {trimmed}"));
    }

    let title = String::from_utf8_lossy(&output.stdout)
        .trim()
        .lines()
        .next()
        .unwrap_or("Instagram Reel")
        .trim()
        .to_string();

    let bytes = std::fs::read(&tmp_path).map_err(|e| format!("gagal membaca file Instagram: {e}"))?;
    if bytes.is_empty() {
        return Err("file unduhan kosong".to_string());
    }

    Ok((title, bytes))
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

            let mut video_data: Option<(String, Vec<u8>)> = None;

            if let Some(shortcode) = extract_shortcode(url) {
                if let Some(pk) = id_to_pk(shortcode) {
                    if let Some(direct_url) = try_native_info_api(pk) {
                        let agent: ureq::Agent = ureq::config::Config::builder().build().into();
                        if let Ok(mut resp) = agent.get(&direct_url).header("User-Agent", USER_AGENT).call() {
                            if let Ok(b) = resp.body_mut().with_config().limit(MAX_VIDEO_BYTES).read_to_vec() {
                                if !b.is_empty() {
                                    video_data = Some(("Instagram Reel".to_string(), b));
                                }
                            }
                        }
                    }
                }
            }

            let (title, video_bytes) = match video_data {
                Some(data) => data,
                None => match download_with_ytdlp(url) {
                    Ok(data) => data,
                    Err(e) => {
                        ctx.reply_quoting(format!("❌ Gagal mengekstrak video Instagram: {e}")).await?;
                        return Ok(());
                    }
                },
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
