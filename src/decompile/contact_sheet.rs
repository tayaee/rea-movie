use anyhow::{Context, Result};
use image::{GenericImage, ImageBuffer, Rgb, RgbImage};
use std::path::Path;
use std::process::Command;

// Simple 5x7 bitmap font for characters: '[', ']', '0'-'9', '.', 's', ' '
fn get_glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        's' => [
            0b00000, 0b00000, 0b01110, 0b10000, 0b01110, 0b00001, 0b11110,
        ],
        _ => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
    }
}

fn draw_text_on_image<I: GenericImage<Pixel = Rgb<u8>>>(
    img: &mut I,
    text: &str,
    start_x: u32,
    start_y: u32,
    scale: u32,
) {
    let char_w = 5 * scale;
    let char_h = 7 * scale;
    let spacing = scale;

    let text_len = text.chars().count() as u32;
    let bg_pad_x = 4 * scale;
    let bg_pad_y = 3 * scale;
    let bg_w = text_len * (char_w + spacing) + bg_pad_x * 2;
    let bg_h = char_h + bg_pad_y * 2;

    // Draw dark background box
    for py in 0..bg_h {
        for px in 0..bg_w {
            let x = start_x + px;
            let y = start_y + py;
            if x < img.width() && y < img.height() {
                let orig = img.get_pixel(x, y);
                // Blend with black (opacity ~ 0.8)
                let r = (orig[0] as f32 * 0.2) as u8;
                let g = (orig[1] as f32 * 0.2) as u8;
                let b = (orig[2] as f32 * 0.2) as u8;
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
    }

    // Draw characters
    let mut cursor_x = start_x + bg_pad_x;
    let text_y = start_y + bg_pad_y;

    for c in text.chars() {
        let glyph = get_glyph(c);
        for (row, &row_bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (row_bits & (1 << (4 - col))) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let x = cursor_x + col as u32 * scale + sx;
                            let y = text_y + row as u32 * scale + sy;
                            if x < img.width() && y < img.height() {
                                img.put_pixel(x, y, Rgb([255, 255, 255]));
                            }
                        }
                    }
                }
            }
        }
        cursor_x += char_w + spacing;
    }
}

pub fn generate_contact_sheet(
    video_path: &str,
    start_sec: f64,
    duration: f64,
    output_path: &Path,
) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Determine 1-second sample intervals
    let mut sample_rel_times = Vec::new();
    let mut cur = 0.0;
    while cur < duration {
        sample_rel_times.push(cur);
        cur += 1.0;
    }
    if sample_rel_times.is_empty() {
        sample_rel_times.push(0.0);
    }

    let temp_dir = tempfile::tempdir()?;
    let tile_w = 320u32;
    let tile_h = 180u32;

    let mut frames = Vec::new();

    for (idx, rel_time) in sample_rel_times.iter().enumerate() {
        let abs_time = (start_sec + rel_time).min(start_sec + duration - 0.04);
        let frame_path = temp_dir.path().join(format!("frame_{:04}.jpg", idx));

        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss",
                &format!("{:.3}", abs_time),
                "-i",
                video_path,
                "-vframes",
                "1",
                "-s",
                &format!("{}x{}", tile_w, tile_h),
                "-q:v",
                "3",
                frame_path.to_str().unwrap(),
            ])
            .output()
            .with_context(|| format!("Failed to extract sample frame at {:.2}s", abs_time))?;

        if !status.status.success() || !frame_path.exists() {
            // If ffmpeg failed on this frame, create fallback placeholder image
            let img = ImageBuffer::from_pixel(tile_w, tile_h, Rgb([40, 40, 40]));
            frames.push((img, *rel_time));
        } else {
            let img = image::open(&frame_path)
                .map(|i| i.to_rgb8())
                .unwrap_or_else(|_| ImageBuffer::from_pixel(tile_w, tile_h, Rgb([40, 40, 40])));
            frames.push((img, *rel_time));
        }
    }

    let n = frames.len() as u32;
    let sheet_w = tile_w * n;
    let sheet_h = tile_h;

    let mut sheet: RgbImage = ImageBuffer::new(sheet_w, sheet_h);

    for (idx, (mut frame_img, rel_time)) in frames.into_iter().enumerate() {
        let label = format!("[{:.1}s]", rel_time);
        draw_text_on_image(&mut frame_img, &label, 8, 8, 2);

        let offset_x = idx as u32 * tile_w;
        for y in 0..tile_h {
            for x in 0..tile_w {
                let p = frame_img.get_pixel(x, y);
                sheet.put_pixel(offset_x + x, y, *p);
            }
        }
    }

    sheet
        .save(output_path)
        .with_context(|| format!("Failed to save contact sheet to {:?}", output_path))?;

    Ok(())
}
