use std::path::Path;

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
    top_text: Option<&str>,
    bot_text: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let tmp_path = format!("/tmp/wa_vid_sticker_{}_{}.mp4", std::process::id(), now);

    struct TempGuard(String);
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = TempGuard(tmp_path.clone());
    std::fs::write(&tmp_path, video_bytes)?;

    let mut reader = yscv_video::Mp4VideoReader::open(Path::new(&tmp_path))
        .map_err(|e| format!("gagal membaca format video MP4: {e}"))?;

    let font = if top_text.is_some() || bot_text.is_some() {
        Some(FontRef::try_from_slice(FONT_DATA)?)
    } else {
        None
    };

    let mut raw_frames = Vec::new();
    let mut total_decoded = 0usize;

    // Decode frame secara streaming (maksimal 75 frame / ~3 detik untuk stiker WA)
    while let Some(frame) = reader.next_frame().map_err(|e| format!("error decode frame: {e}"))? {
        total_decoded += 1;
        raw_frames.push(frame);
        if total_decoded >= 75 {
            break;
        }
    }

    if raw_frames.is_empty() {
        return Err("tidak ada frame video yang berhasil didecode".into());
    }

    // Sampling agar stiker video memiliki motion mulus di 10–12 FPS (maksimal 20–24 frame)
    let target_frames = 20usize;
    let stride = if raw_frames.len() > target_frames {
        (raw_frames.len() + target_frames - 1) / target_frames
    } else {
        1
    };

    let sampled_frames: Vec<_> = raw_frames.into_iter().step_by(stride).take(target_frames).collect();
    let frame_duration_ms: u32 = 100; // 10 FPS

    let mut anim_frames = Vec::new();
    for frame in sampled_frames {
        let rgb_img = match image::RgbImage::from_raw(frame.width as u32, frame.height as u32, frame.rgb8_data) {
            Some(img) => img,
            None => continue,
        };
        let dyn_img = image::DynamicImage::ImageRgb8(rgb_img);
        let scaled = dyn_img.resize(512, 512, FilterType::Triangle).to_rgba8();

        let mut canvas = image::RgbaImage::new(512, 512);
        let offset_x = (512 - scaled.width()) / 2;
        let offset_y = (512 - scaled.height()) / 2;
        image::imageops::overlay(&mut canvas, &scaled, offset_x as i64, offset_y as i64);

        if let Some(ref f) = font {
            if let Some(top) = top_text {
                render_text_block(&mut canvas, f, top, true);
            }
            if let Some(bot) = bot_text {
                render_text_block(&mut canvas, f, bot, false);
            }
        }

        let anim_frame = oxideav_webp::AnimFrame::new(
            512,
            512,
            canvas.into_raw(),
            frame_duration_ms,
        );
        anim_frames.push(anim_frame);
    }

    if anim_frames.is_empty() {
        return Err("gagal memproses frame stiker animasi".into());
    }

    let webp_bytes = oxideav_webp::build_animated_webp(&anim_frames)
        .map_err(|e| format!("gagal mengompilasi stiker animasi WebP: {e}"))?;

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
                    let webp_bytes = convert_video_to_animated_webp(
                        &video_bytes,
                        top_opt.as_deref(),
                        bot_opt.as_deref(),
                    )?;
                    send_sticker(ctx, webp_bytes, true).await?;
                }
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
