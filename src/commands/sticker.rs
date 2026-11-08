use ab_glyph::{FontRef, PxScale};
use image::codecs::webp::WebPEncoder;
use image::imageops::{resize, FilterType};
use image::{ExtendedColorType, Rgba};
use imageproc::drawing::{draw_text_mut, text_size};

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Sticker;

// Font Roboto Condensed Bold di-embed langsung ke dalam biner (zero dependency ke OS)
const FONT_DATA: &[u8] = include_bytes!("../../assets/RobotoCondensed-Bold.ttf");

fn wrap_text(s: &str, max_chars_per_line: usize) -> String {
    let s = s.to_uppercase();
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in words {
        if current_line.is_empty() {
            current_line.push_str(word);
        } else if current_line.len() + 1 + word.len() <= max_chars_per_line {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines.join("\n")
}

fn parse_text_params(args: &str) -> (Option<String>, Option<String>) {
    let args = args.trim();
    if args.is_empty() {
        return (None, None);
    }

    if let Some((top_raw, bot_raw)) = args.split_once('|') {
        let top = top_raw.trim();
        let bot = bot_raw.trim();
        let top_opt = if top.is_empty() {
            None
        } else {
            Some(wrap_text(top, 16))
        };
        let bot_opt = if bot.is_empty() {
            None
        } else {
            Some(wrap_text(bot, 16))
        };
        (top_opt, bot_opt)
    } else {
        (None, Some(wrap_text(args, 16)))
    }
}

enum MediaTarget<'a> {
    Image(&'a wa::message::ImageMessage),
    Video(&'a wa::message::VideoMessage),
}

fn extract_media_target(msg: &wa::Message) -> Option<MediaTarget<'_>> {
    let base = msg.get_base_message();
    // 1. Direct media message (caption command)
    if let Some(img) = base.image_message.as_option() {
        return Some(MediaTarget::Image(img));
    }
    if let Some(vid) = base.video_message.as_option() {
        return Some(MediaTarget::Video(vid));
    }

    // 2. Quoted message in extended_text_message (reply command)
    if let Some(ext) = base.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                let q_base = quoted.get_base_message();
                if let Some(img) = q_base.image_message.as_option() {
                    return Some(MediaTarget::Image(img));
                }
                if let Some(vid) = q_base.video_message.as_option() {
                    return Some(MediaTarget::Video(vid));
                }
            }
        }
    }

    // 3. Quoted message in other media message
    if let Some(img) = base.image_message.as_option() {
        if let Some(ctx_info) = img.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                let q_base = quoted.get_base_message();
                if let Some(q_img) = q_base.image_message.as_option() {
                    return Some(MediaTarget::Image(q_img));
                }
                if let Some(q_vid) = q_base.video_message.as_option() {
                    return Some(MediaTarget::Video(q_vid));
                }
            }
        }
    }
    if let Some(vid) = base.video_message.as_option() {
        if let Some(ctx_info) = vid.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                let q_base = quoted.get_base_message();
                if let Some(q_img) = q_base.image_message.as_option() {
                    return Some(MediaTarget::Image(q_img));
                }
                if let Some(q_vid) = q_base.video_message.as_option() {
                    return Some(MediaTarget::Video(q_vid));
                }
            }
        }
    }

    None
}

fn render_text_block(
    img: &mut image::RgbaImage,
    font: &FontRef,
    text: &str,
    is_top: bool,
) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return;
    }

    let max_len = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let line_count = lines.len();

    let font_size: f32 = if max_len <= 11 && line_count == 1 {
        62.0
    } else if max_len <= 16 && line_count <= 2 {
        52.0
    } else if max_len <= 20 && line_count <= 2 {
        44.0
    } else {
        36.0
    };

    let scale = PxScale::from(font_size);
    let outline_width = (font_size * 0.08).max(2.0).round() as i32;
    let line_spacing = (font_size * 0.15) as i32;

    let text_color = Rgba([255, 255, 255, 255]);
    let outline_color = Rgba([0, 0, 0, 255]);

    let mut total_block_height = 0;
    let mut line_heights = Vec::new();

    for line in &lines {
        let (_, h) = text_size(scale, font, line);
        line_heights.push(h);
        total_block_height += h as i32;
    }
    if line_count > 1 {
        total_block_height += (line_count as i32 - 1) * line_spacing;
    }

    let start_y = if is_top {
        24
    } else {
        512 - total_block_height - 24
    };

    let mut cur_y = start_y;
    for (idx, line) in lines.iter().enumerate() {
        let (w, _) = text_size(scale, font, line);
        let x = (512 - w as i32) / 2;

        for dy in -outline_width..=outline_width {
            for dx in -outline_width..=outline_width {
                if dx == 0 && dy == 0 {
                    continue;
                }
                if dx * dx + dy * dy <= outline_width * outline_width {
                    draw_text_mut(img, outline_color, x + dx, cur_y + dy, scale, font, line);
                }
            }
        }
        draw_text_mut(img, text_color, x, cur_y, scale, font, line);
        cur_y += line_heights[idx] as i32 + line_spacing;
    }
}

fn convert_to_webp(
    image_bytes: &[u8],
    top_text: Option<&str>,
    bot_text: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let dynamic_img = image::load_from_memory(image_bytes)?;
    let mut img_rgba = resize(
        &dynamic_img,
        512,
        512,
        FilterType::CatmullRom,
    );

    if top_text.is_some() || bot_text.is_some() {
        let font = FontRef::try_from_slice(FONT_DATA)?;
        if let Some(top) = top_text {
            render_text_block(&mut img_rgba, &font, top, true);
        }
        if let Some(bot) = bot_text {
            render_text_block(&mut img_rgba, &font, bot, false);
        }
    }

    let mut webp_bytes = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut webp_bytes);
    encoder.encode(
        img_rgba.as_raw(),
        512,
        512,
        ExtendedColorType::Rgba8,
    )?;

    Ok(webp_bytes)
}

fn convert_video_to_animated_webp(
    video_bytes: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let in_path = format!("/tmp/wa_vid_in_{}_{}.mp4", std::process::id(), now);
    let out_path = format!("/tmp/wa_vid_out_{}_{}.webp", std::process::id(), now);

    struct FileCleaner(String, String);
    impl Drop for FileCleaner {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(&self.1);
        }
    }
    let _cleaner = FileCleaner(in_path.clone(), out_path.clone());
    std::fs::write(&in_path, video_bytes)?;

    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            &in_path,
            "-t",
            "4",
            "-vf",
            "fps=12,scale=512:512:force_original_aspect_ratio=decrease,pad=512:512:(ow-iw)/2:(oh-ih)/2:color=0x00000000",
            "-vcodec",
            "libwebp_anim",
            "-lossless",
            "0",
            "-q:v",
            "45",
            "-loop",
            "0",
            "-an",
            &out_path,
        ])
        .output()
        .map_err(|e| format!("gagal menjalankan ffmpeg: {e}"))?;

    if !status.status.success() {
        let err = String::from_utf8_lossy(&status.stderr);
        let last_err = err.lines().last().unwrap_or("unknown error");
        return Err(format!("ffmpeg error: {last_err}").into());
    }

    let webp_bytes = std::fs::read(&out_path)?;
    if webp_bytes.is_empty() {
        return Err("hasil stiker video kosong".into());
    }
    if webp_bytes.len() > 1024 * 1024 {
        return Err("ukuran stiker video melebihi batas 1 MB WhatsApp".into());
    }
    Ok(webp_bytes)
}

async fn send_sticker(
    ctx: &MessageContext,
    webp_bytes: Vec<u8>,
    is_animated: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let upload = ctx
        .client
        .upload(webp_bytes, MediaType::Sticker, Default::default())
        .await?;

    let mut sticker = wa::message::StickerMessage::default();
    sticker.url = Some(upload.url);
    sticker.direct_path = Some(upload.direct_path);
    sticker.media_key = Some(upload.media_key.to_vec());
    sticker.file_enc_sha256 = Some(upload.file_enc_sha256.to_vec());
    sticker.file_sha256 = Some(upload.file_sha256.to_vec());
    sticker.file_length = Some(upload.file_length);
    sticker.media_key_timestamp = Some(upload.media_key_timestamp);
    sticker.mimetype = Some("image/webp".to_string());
    sticker.height = Some(512);
    sticker.width = Some(512);
    if is_animated {
        sticker.is_animated = Some(true);
    }

    let mut message = wa::Message::default();
    message.sticker_message = MessageField::some(sticker);

    ctx.send_message(message).await?;
    Ok(())
}

impl Command for Sticker {
    fn name(&self) -> &'static str {
        "s"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["sticker", "stiker"]
    }

    fn description(&self) -> &'static str {
        "Buat stiker foto/video (opsional teks: /s atas | bawah atau /s | bawah)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let media = match extract_media_target(&ctx.message) {
                Some(m) => m,
                None => {
                    ctx.reply_quoting("❌ Kirim foto/video dengan caption `/s` atau balas/reply foto/video dengan `/s`.").await?;
                    return Ok(());
                }
            };

            let (top_opt, bot_opt) = parse_text_params(args);

            let _ = ctx.react("⏳").await;
            let _media_permit = bot
                .media_permits
                .clone()
                .acquire_owned()
                .await
                .expect("media semaphore must remain open");
            let _media_heap = super::MediaHeapGuard;

            match media {
                MediaTarget::Image(img) => {
                    let image_bytes = ctx.client.download(img).await?;
                    let webp_bytes = convert_to_webp(
                        &image_bytes,
                        top_opt.as_deref(),
                        bot_opt.as_deref(),
                    )?;
                    send_sticker(ctx, webp_bytes, false).await?;
                }
                MediaTarget::Video(vid) => {
                    if vid.seconds.unwrap_or(0) > 10 {
                        ctx.reply_quoting("❌ Durasi video maksimal 10 detik untuk stiker.").await?;
                        return Ok(());
                    }
                    let video_bytes = ctx.client.download(vid).await?;
                    let webp_bytes = convert_video_to_animated_webp(&video_bytes)?;
                    send_sticker(ctx, webp_bytes, true).await?;
                }
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
