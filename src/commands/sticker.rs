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
    // Otomatis ubah teks menjadi KAPITAL (uppercase)
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
        // Jika tanpa tanda "|", teks ditempatkan di bagian bawah stiker
        (None, Some(wrap_text(args, 16)))
    }
}

fn extract_image_message(msg: &wa::Message) -> Option<&wa::message::ImageMessage> {
    let base = msg.get_base_message();
    // 1. Direct image message (caption command)
    if let Some(img) = base.image_message.as_option() {
        return Some(img);
    }

    // 2. Quoted message in extended_text_message (reply command)
    if let Some(ext) = base.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if let Some(img) = quoted.get_base_message().image_message.as_option() {
                    return Some(img);
                }
            }
        }
    }

    // 3. Quoted message in other media message
    if let Some(img) = base.image_message.as_option() {
        if let Some(ctx_info) = img.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if let Some(q_img) = quoted.get_base_message().image_message.as_option() {
                    return Some(q_img);
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

    let border_w: i32 = if font_size >= 50.0 { 4 } else { 3 };
    let scale = PxScale::from(font_size);
    let line_spacing: i32 = 4;

    let line_heights: Vec<u32> = lines
        .iter()
        .map(|l| text_size(scale, font, l).1)
        .collect();

    let total_h: i32 = line_heights.iter().sum::<u32>() as i32
        + (lines.len().saturating_sub(1) as i32 * line_spacing);

    let start_y = if is_top {
        18
    } else {
        512 - total_h - 18
    };

    let mut cur_y = start_y;
    for (idx, line) in lines.iter().enumerate() {
        let (w, _h) = text_size(scale, font, line);
        let x = (512 - w as i32) / 2;

        // Outline border warna hitam
        for dx in -border_w..=border_w {
            for dy in -border_w..=border_w {
                if dx != 0 || dy != 0 {
                    draw_text_mut(
                        img,
                        Rgba([0, 0, 0, 255]),
                        x + dx,
                        cur_y + dy,
                        scale,
                        font,
                        line,
                    );
                }
            }
        }

        // Teks utama warna putih
        draw_text_mut(
            img,
            Rgba([255, 255, 255, 255]),
            x,
            cur_y,
            scale,
            font,
            line,
        );

        cur_y += line_heights[idx] as i32 + line_spacing;
    }
}

fn convert_to_webp(
    image_bytes: &[u8],
    top_text: Option<&str>,
    bot_text: Option<&str>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Decode gambar langsung dari memori RAM (JPEG/PNG/WebP/GIF)
    let dynamic_img = image::load_from_memory(image_bytes)?;

    // 2. Resize 1:1 (512x512) tanpa border/crop
    let mut img_rgba = resize(
        &dynamic_img,
        512,
        512,
        FilterType::CatmullRom,
    );

    // 3. Render teks font jika ada
    if top_text.is_some() || bot_text.is_some() {
        let font = FontRef::try_from_slice(FONT_DATA)?;
        if let Some(top) = top_text {
            render_text_block(&mut img_rgba, &font, top, true);
        }
        if let Some(bot) = bot_text {
            render_text_block(&mut img_rgba, &font, bot, false);
        }
    }

    // 4. Encode ke WebP murni di memori
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

impl Command for Sticker {
    fn name(&self) -> &'static str {
        "s"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["sticker", "stiker"]
    }

    fn description(&self) -> &'static str {
        "Buat stiker 1:1 (opsional teks: /s atas | bawah atau /s | bawah)"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let img = match extract_image_message(&ctx.message) {
                Some(i) => i,
                None => {
                    ctx.reply_quoting("❌ Kirim gambar dengan caption `/s` atau balas/reply gambar dengan `/s`.").await?;
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

            {
                let image_bytes = ctx.client.download(img).await?;
                let webp_bytes = convert_to_webp(
                    &image_bytes,
                    top_opt.as_deref(),
                    bot_opt.as_deref(),
                )?;

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

                let mut message = wa::Message::default();
                message.sticker_message = MessageField::some(sticker);

                ctx.send_message(message).await?;
            }

            let _ = ctx.react("✅").await;
            Ok(())
        })
    }
}
