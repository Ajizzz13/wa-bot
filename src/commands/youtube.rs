use std::io::Read;
use std::process::Command;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Youtube;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

fn extract_with_ytdlp(url: &str) -> Result<String, String> {
    let output = Command::new("yt-dlp")
        .args([
            "--no-warnings",
            "--no-check-certificates",
            "--user-agent", USER_AGENT,
            "-f", "best[height<=720][ext=mp4]/best[height<=720]/best",
            "--get-url",
            url,
        ])
        .output()
        .map_err(|e| format!("gagal menjalankan yt-dlp: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = stdout.trim().lines().next().unwrap_or("").trim().to_string();
    if url.is_empty() || !url.starts_with("http") {
        return Err("yt-dlp tidak mengembalikan URL valid".to_string());
    }
    Ok(url)
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

            {
                let stream_url = match extract_with_ytdlp(url) {
                    Ok(u) => u,
                    Err(e) => {
                        ctx.reply_quoting(format!("❌ Gagal mengekstrak video YouTube: {e}")).await?;
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
                video.caption = Some("🎬 *YouTube Video*".to_string());

                let mut message = wa::Message::default();
                message.video_message = MessageField::some(video);

                ctx.send_message(message).await?;
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
EOF
echo "written"