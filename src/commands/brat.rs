use ab_glyph::{FontRef, PxScale};
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};

use whatsapp_rust::bot::MessageContext;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;

use super::{BotContext, BoxFut, Command, CommandResult};

pub struct Brat;

const FONT_DATA: &[u8] = include_bytes!("../../assets/RobotoCondensed-Bold.ttf");

// Brat aesthetic: white background, black text
const BG_COLOR: Rgba<u8> = Rgba([255, 255, 255, 255]);
const TEXT_COLOR: Rgba<u8> = Rgba([0, 0, 0, 255]);

fn render_brat_image(text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let size = 512u32;
    let mut img = RgbaImage::from_pixel(size, size, BG_COLOR);

    let font = FontRef::try_from_slice(FONT_DATA)?;
    let text = text.trim();
    if text.is_empty() {
        return Ok(encode_webp(&img)?);
    }

    let max_width = size as i32 - 40;
    let max_lines = 6usize;
    let words: Vec<&str> = text.split_whitespace().collect();

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in &words {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current, word)
        };
        let scale = PxScale::from(48.0);
        let (w, _) = text_size(scale, &font, &candidate);
        if w as i32 > max_width && !current.is_empty() {
            lines.push(current.clone());
            current = word.to_string();
        } else {
            current = candidate;
        }
        if lines.len() >= max_lines {
            break;
        }
    }
    if !current.is_empty() && lines.len() < max_lines {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(text.to_string());
    }

    let mut font_size: f32 = 64.0;
    for _ in 0..20 {
        let scale = PxScale::from(font_size);
        let total_h: i32 = lines
            .iter()
            .map(|l| text_size(scale, &font, l).1 as i32)
            .sum::<i32>()
            + (lines.len().saturating_sub(1) as i32 * 8);
        let max_line_w: i32 = lines
            .iter()
            .map(|l| text_size(scale, &font, l).0 as i32)
            .max()
            .unwrap_or(0);
        if total_h <= size as i32 - 40 && max_line_w <= max_width {
            break;
        }
        font_size *= 0.9;
        if font_size < 12.0 {
            break;
        }
    }

    let scale = PxScale::from(font_size);
    let line_heights: Vec<u32> = lines
        .iter()
        .map(|l| text_size(scale, &font, l).1)
        .collect();
    let total_h: i32 = line_heights.iter().sum::<u32>() as i32
        + (lines.len().saturating_sub(1) as i32 * 8);
    let mut y = (size as i32 - total_h) / 2;

    for (i, line) in lines.iter().enumerate() {
        let (w, _) = text_size(scale, &font, line);
        let x = (size as i32 - w as i32) / 2;
        draw_text_mut(&mut img, TEXT_COLOR, x, y, scale, &font, line);
        y += line_heights[i] as i32 + 8;
    }

    encode_webp(&img)
}

fn encode_webp(img: &RgbaImage) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let mut webp_bytes = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut webp_bytes);
    encoder.encode(
        img.as_raw(),
        img.width(),
        img.height(),
        ExtendedColorType::Rgba8,
    )?;
    Ok(webp_bytes)
}

impl Command for Brat {
    fn name(&self) -> &'static str {
        "brat"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["bratsticker", "bratstiker"]
    }

    fn description(&self) -> &'static str {
        "Buat stiker bergaya Brat (teks di background putih): /brat <teks>"
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a MessageContext,
        args: &'a str,
        bot: &'a BotContext,
    ) -> BoxFut<'a, CommandResult> {
        Box::pin(async move {
            let text = args.trim();
            if text.is_empty() {
                ctx.reply_quoting("❌ Masukkan teks.\nContoh: `/brat aku keren`").await?;
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
                let webp_bytes = render_brat_image(text)?;

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