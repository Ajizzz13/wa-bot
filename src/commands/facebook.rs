use std::io::Read;
use regex::Regex;
use serde_json::Value;

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Facebook;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";
const CRAWLER_UA: &str = "facebookexternalhit/1.1";
const MAX_VIDEO_BYTES: u64 = 60 * 1024 * 1024; // 60 MB batas WhatsApp

struct ExtractedFbVideo {
    title: String,
    stream_url: String,
}

fn fetch_html(url: &str, ua: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::config::Config::builder()
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .header("User-Agent", ua)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.5")
        .call()
        .ok()?;
    let html = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_string().ok()?;
    Some(html)
}

fn clean_fb_url(raw: &str) -> String {
    raw.replace(r"\/", "/")
        .replace("&amp;", "&")
        .replace(r"\u003C", "<")
        .replace(r"\u003E", ">")
        .replace(r"\u0026", "&")
}

fn extract_title(html: &str) -> String {
    Regex::new(r"<title>(.*?)</title>")
        .ok()
        .and_then(|re| re.captures(html))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().replace(" | Facebook", "").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Facebook Video".to_string())
}

/// Cari direct fbcdn.net MP4 URL dari HTML crawler Facebook.
/// Prioritas: m366/m420 (HD) > m412 (SD).
fn extract_fbcdn_url(html: &str) -> Option<String> {
    // Pola broad: video-*.xx.fbcdn.net/.../mXXX/....mp4?...
    let re = Regex::new(r#"video-[a-z0-9-]+\.xx\.fbcdn\.net\\?/o1/v/t2/f2/(m\d+)/[^"\\]+\.mp4[^"\\]*"#).ok()?;
    let mut best: Option<(u8, String)> = None; // (priority, url)
    for caps in re.captures_iter(html) {
        let quality = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let priority: u8 = match quality {
            "m366" | "m420" | "m441" => 0, // HD
            "m412" => 1,                    // SD
            _ => 2,
        };
        let raw = caps.get(0).map(|m| m.as_str())?;
        let url = clean_fb_url(raw);
        match &best {
            Some((p, _)) if *p <= priority => {}
            _ => best = Some((priority, url)),
        }
    }
    best.map(|(_, u)| u)
}

fn try_extract_native(url: &str) -> Option<ExtractedFbVideo> {
    // 1. Crawler UA (bekerja untuk share/r/ dan reel)
    for ua in [CRAWLER_UA, USER_AGENT] {
        if let Some(html) = fetch_html(url, ua) {
            if let Some(stream_url) = extract_fbcdn_url(&html) {
                return Some(ExtractedFbVideo {
                    title: extract_title(&html),
                    stream_url,
                });
            }
            // fallback: regex lama untuk playable_url / hd_src
            let clean = html.replace(r"\u0025", "%").replace(r"\/", "/");
            let hd_re = Regex::new(r#""(?:playable_url_quality_hd|browser_native_hd_url|hd_src)":"(https://[^"]+)""#).ok()?;
            if let Some(caps) = hd_re.captures(&clean) {
                if let Some(m) = caps.get(1) {
                    return Some(ExtractedFbVideo {
                        title: extract_title(&html),
                        stream_url: clean_fb_url(m.as_str()),
                    });
                }
            }
            let sd_re = Regex::new(r#""(?:playable_url|browser_native_sd_url|sd_src)":"(https://[^"]+)""#).ok()?;
            if let Some(caps) = sd_re.captures(&clean) {
                if let Some(m) = caps.get(1) {
                    return Some(ExtractedFbVideo {
                        title: extract_title(&html),
                        stream_url: clean_fb_url(m.as_str()),
                    });
                }
            }
        }
    }
    None
}

fn try_extract_fallback(url: &str) -> Option<ExtractedFbVideo> {
    // Fallback menggunakan SnapSave resolver (vm-decode handled by caller via HTTP)
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
