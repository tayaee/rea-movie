use anyhow::{Context, Result};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::Path;

// Section 3.3: Bounding box for character head
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl BBox {
    /// Validation criteria from Issue 5:
    /// 0 <= x <= 1, 0 <= y <= 1, 0.02 <= w <= 0.9, 0.02 <= h <= 0.9,
    /// x + w <= 1.02, y + h <= 1.02
    pub fn is_valid(&self) -> bool {
        self.x >= 0.0
            && self.y >= 0.0
            && self.x <= 1.0
            && self.y <= 1.0
            && self.w >= 0.02
            && self.w <= 0.9
            && self.h >= 0.02
            && self.h <= 0.9
            && (self.x + self.w) <= 1.02
            && (self.y + self.h) <= 1.02
    }

    pub fn to_array(&self) -> [f64; 4] {
        [self.x, self.y, self.w, self.h]
    }

    /// Rebuild a bbox from a stored `[x, y, w, h]` array.
    pub fn from_array(values: [f64; 4]) -> Option<BBox> {
        let bbox = BBox {
            x: values[0],
            y: values[1],
            w: values[2],
            h: values[3],
        };
        if bbox.is_valid() {
            Some(bbox)
        } else {
            None
        }
    }
}

// Section 3.2: Record of anchor edit in anchor_edit.json
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnchorEditRecord {
    pub frame: String,
    pub character: String,
    pub bbox: [f64; 4],
    pub prop: String,
    pub applied_at: String,
}

#[derive(Debug, Deserialize)]
struct M3HeadResponse {
    head: Option<BBox>,
}

pub fn load_anchor_edits(path: &Path) -> Vec<AnchorEditRecord> {
    if !path.is_file() {
        return Vec::new();
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    if let Ok(records) = serde_json::from_str::<Vec<AnchorEditRecord>>(&content) {
        records
    } else if let Ok(single) = serde_json::from_str::<AnchorEditRecord>(&content) {
        vec![single]
    } else {
        Vec::new()
    }
}

pub fn save_anchor_edits(path: &Path, records: &[AnchorEditRecord]) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json_str = if records.len() == 1 {
        serde_json::to_string_pretty(&records[0])?
    } else {
        serde_json::to_string_pretty(&records)?
    };
    std::fs::write(path, json_str)?;
    Ok(())
}

/// Detect the character's head bounding box using MiniMax-M3 Vision API (or mock during testing)
pub fn detect_head_bbox(
    image_path: &Path,
    appearance_en: &str,
    api_key_opt: Option<&str>,
) -> Result<Option<BBox>> {
    // 1. Check for mock bbox environment variable (for offline/regression test isolation)
    if let Ok(mock_val) = std::env::var("REA_MOVIE_MOCK_BBOX") {
        let trimmed = mock_val.trim();
        if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("null") {
            return Ok(None);
        }
        let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
        if parts.len() == 4 {
            if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                parts[0].parse::<f64>(),
                parts[1].parse::<f64>(),
                parts[2].parse::<f64>(),
                parts[3].parse::<f64>(),
            ) {
                let bbox = BBox { x, y, w, h };
                if bbox.is_valid() {
                    return Ok(Some(bbox));
                }
            }
        }
        // Fallback default mock bbox
        return Ok(Some(BBox {
            x: 0.263,
            y: 0.079,
            w: 0.206,
            h: 0.519,
        }));
    }

    // 2. Real MiniMax-M3 API call
    let api_key = match api_key_opt {
        Some(k) if !k.trim().is_empty() => k.trim(),
        _ => {
            eprintln!("Warning: MINIMAX_API_KEY is not set; skipping head detection");
            return Ok(None);
        }
    };

    let img_bytes = std::fs::read(image_path)
        .with_context(|| format!("Failed to read image at {:?}", image_path))?;
    let base64_img = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &img_bytes);
    let data_uri = format!("data:image/jpeg;base64,{}", base64_img);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let system_prompt =
        "You are a precise computer vision annotation system. Output ONLY valid JSON.";
    let prompt_text = format!(
        "Locate the head of {}. Output ONLY JSON {{\"head\": {{\"x\": <0-1 left>, \"y\": <0-1 top>, \"w\": <0-1 width>, \"h\": <0-1 height>}}}} describing the bounding box of the HEAD ONLY (skull, not shoulders). If the character is not present, output {{\"head\": null}}.",
        appearance_en
    );

    let user_msg = serde_json::json!({
        "model": "MiniMax-M3",
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt_text
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": data_uri
                        }
                    }
                ]
            }
        ]
    });

    let resp = client
        .post("https://api.minimax.io/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&user_msg)
        .send()?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "MiniMax M3 head detection API returned status: {}",
            resp.status()
        );
    }

    let json_val: serde_json::Value = resp.json()?;
    let text = json_val["choices"][0]["message"]["content"]
        .as_str()
        .context("Missing content in MiniMax M3 head detection response")?;

    let parsed: M3HeadResponse = crate::decompile::prompt::parse_llm_json(text)?;
    if let Some(bbox) = parsed.head {
        if bbox.is_valid() {
            Ok(Some(bbox))
        } else {
            eprintln!(
                "Warning: Detected bbox {:?} failed validation criteria",
                bbox
            );
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

/// Calculate transparent bounding box of an RGBA image, trim transparent padding, and return cropped image.
pub fn trim_transparent(img: &RgbaImage) -> Option<RgbaImage> {
    let (w, h) = img.dimensions();
    let mut min_x = w;
    let mut max_x = 0;
    let mut min_y = h;
    let mut max_y = 0;
    let mut found = false;

    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x, y);
            if p[3] > 0 {
                found = true;
                if x < min_x {
                    min_x = x;
                }
                if x > max_x {
                    max_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
    }

    if !found {
        return None;
    }

    let crop_w = max_x - min_x + 1;
    let crop_h = max_y - min_y + 1;
    Some(image::imageops::crop_imm(img, min_x, min_y, crop_w, crop_h).to_image())
}

/// Composite prop PNG onto base frame JPEG according to Issue 5 specifications:
/// 1. Decode base frame JPEG to RGBA8
/// 2. Decode prop PNG to RGBA8 and trim transparent bounding box
/// 3. cap_w = head_w_px * cap_scale, keeping aspect ratio for cap_h
/// 4. cap center x = head bbox center x; cap top y = head_top + head_h * cap_offset
/// 5. Manual alpha blending: out = cap.rgb * a + base.rgb * (1 - a)
/// 6. Save as high-quality JPEG
pub fn composite_prop(
    base_frame_path: &Path,
    prop_path: &Path,
    out_frame_path: &Path,
    bbox: &BBox,
    cap_scale: f64,
    cap_offset: f64,
) -> Result<bool> {
    let mut base_img = image::open(base_frame_path)
        .with_context(|| format!("Failed to open base frame {:?}", base_frame_path))?
        .to_rgba8();
    let (frame_w, frame_h) = base_img.dimensions();

    let prop_img = image::open(prop_path)
        .with_context(|| format!("Failed to open prop image {:?}", prop_path))?
        .to_rgba8();

    let cropped_prop = match trim_transparent(&prop_img) {
        Some(c) => c,
        None => {
            eprintln!("Warning: Prop image {:?} is fully transparent", prop_path);
            return Ok(false);
        }
    };

    let (crop_w, crop_h) = cropped_prop.dimensions();

    // Scale calculation
    let head_w_px = bbox.w * (frame_w as f64);
    let head_h_px = bbox.h * (frame_h as f64);
    let head_x_px = bbox.x * (frame_w as f64);
    let head_y_px = bbox.y * (frame_h as f64);

    let target_cap_w = (head_w_px * cap_scale).round().max(1.0) as u32;
    let target_cap_h = ((target_cap_w as f64) * (crop_h as f64 / crop_w as f64))
        .round()
        .max(1.0) as u32;

    let resized_prop = image::imageops::resize(
        &cropped_prop,
        target_cap_w,
        target_cap_h,
        image::imageops::FilterType::Lanczos3,
    );

    // Placement coordinates
    let head_center_x = head_x_px + head_w_px / 2.0;
    let cap_start_x = (head_center_x - (target_cap_w as f64) / 2.0).round() as i64;
    let cap_start_y = (head_y_px + head_h_px * cap_offset).round() as i64;

    // Section 3.4: Manual alpha compositing
    for cy in 0..target_cap_h {
        let py = cap_start_y + cy as i64;
        if py < 0 || py >= frame_h as i64 {
            continue;
        }
        for cx in 0..target_cap_w {
            let px = cap_start_x + cx as i64;
            if px < 0 || px >= frame_w as i64 {
                continue;
            }

            let cap_p = resized_prop.get_pixel(cx, cy);
            let a = (cap_p[3] as f64) / 255.0;
            if a <= 0.0 {
                continue;
            }

            let base_p = base_img.get_pixel(px as u32, py as u32);
            let out_r = ((cap_p[0] as f64) * a + (base_p[0] as f64) * (1.0 - a)).round() as u8;
            let out_g = ((cap_p[1] as f64) * a + (base_p[1] as f64) * (1.0 - a)).round() as u8;
            let out_b = ((cap_p[2] as f64) * a + (base_p[2] as f64) * (1.0 - a)).round() as u8;

            base_img.put_pixel(px as u32, py as u32, Rgba([out_r, out_g, out_b, 255]));
        }
    }

    // Save as JPEG with highest quality (q=98 ~ q=2)
    let rgb_img = image::DynamicImage::ImageRgba8(base_img).to_rgb8();
    if let Some(parent) = out_frame_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out_file = std::fs::File::create(out_frame_path)
        .with_context(|| format!("Failed to create output file {:?}", out_frame_path))?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out_file, 98);
    encoder.encode(
        rgb_img.as_raw(),
        frame_w,
        frame_h,
        image::ExtendedColorType::Rgb8,
    )?;

    Ok(true)
}

fn normalize_shot_key(key: &str) -> String {
    let trimmed = key.trim();
    if let Some(stripped) = trimmed.strip_prefix("shot_") {
        format!("shot_{:0>4}", stripped)
    } else if let Ok(num) = trimmed.parse::<usize>() {
        format!("shot_{:0>4}", num)
    } else {
        trimmed.to_string()
    }
}

/// Main entry point for `rea-movie customize anchor` CLI and MCP tool.
/// Processes shots and composites props onto anchor frames for characters defined in `customization/characters.toml` with `cap_ref`.
pub fn run_anchor_edit(
    proj_dir: &Path,
    shot_filter: Option<&str>,
    character_filter: Option<&str>,
    force: bool,
) -> Result<Vec<String>> {
    let custom = crate::customize::Customization::load(proj_dir);
    if custom.characters.is_empty() {
        return Ok(Vec::new());
    }

    // Filter characters with cap_ref
    let target_characters: Vec<(&String, &crate::customize::Character)> = custom
        .characters
        .iter()
        .filter(|(id, ch)| {
            if ch.cap_ref.is_none() {
                return false;
            }
            if let Some(cf) = character_filter {
                if cf != id.as_str() {
                    return false;
                }
            }
            true
        })
        .collect();

    if target_characters.is_empty() {
        return Ok(Vec::new());
    }

    // Check API key / Mock
    let api_key = std::env::var("MINIMAX_API_KEY").ok();
    let has_mock = std::env::var("REA_MOVIE_MOCK_BBOX").is_ok();
    if !has_mock
        && api_key
            .as_deref()
            .map(|k| k.trim().is_empty())
            .unwrap_or(true)
    {
        eprintln!("Warning: MINIMAX_API_KEY is not set; skipping anchor frame editing");
        return Ok(Vec::new());
    }

    // Collect shots
    let timeline_path = proj_dir.join("timeline.json");
    let shots_dir = proj_dir.join("shots");
    let mut shot_entries: Vec<(String, Vec<String>)> = Vec::new();

    let norm_filter = shot_filter.map(normalize_shot_key);

    if timeline_path.is_file() {
        if let Ok(manifest) = crate::timeline::TimelineManifest::load_from_file(&timeline_path) {
            for shot in manifest.shots {
                let norm_id = normalize_shot_key(&shot.shot_id);
                if let Some(ref filt) = norm_filter {
                    if filt != &norm_id && filt != &shot.shot_id {
                        continue;
                    }
                }
                shot_entries.push((shot.shot_id, shot.characters));
            }
        }
    } else if shots_dir.is_dir() {
        for entry in std::fs::read_dir(&shots_dir)?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(shot_id) = path.file_name().and_then(|s| s.to_str()) {
                    let norm_id = normalize_shot_key(shot_id);
                    if let Some(ref filt) = norm_filter {
                        if filt != &norm_id && filt != shot_id {
                            continue;
                        }
                    }
                    let meta_path = path.join("shot.toml");
                    let cast = if meta_path.is_file() {
                        crate::timeline::ShotMeta::load_from_file(&meta_path)
                            .map(|m| m.characters)
                            .unwrap_or_else(|_| vec!["S1".to_string()])
                    } else {
                        vec!["S1".to_string()]
                    };
                    shot_entries.push((shot_id.to_string(), cast));
                }
            }
        }
    }

    shot_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut edited_shots = Vec::new();

    for (shot_id, base_cast) in shot_entries {
        let cast = custom.cast_for(&shot_id, &base_cast);
        let shot_dir = shots_dir.join(&shot_id);
        let refs_dir = shot_dir.join("refs");
        if !refs_dir.is_dir() {
            continue;
        }

        let anchor_edit_path = refs_dir.join("anchor_edit.json");
        let mut edits = load_anchor_edits(&anchor_edit_path);
        let mut shot_modified = false;

        let frame_names = ["first_frame.jpg", "last_frame.jpg"];

        for (char_id, char_def) in &target_characters {
            if !cast.contains(char_id) {
                continue;
            }

            let prop_rel = match char_def.cap_ref.as_ref() {
                Some(p) => p,
                None => continue,
            };
            let prop_path = proj_dir.join(prop_rel);
            if !prop_path.is_file() {
                eprintln!(
                    "Warning: Prop image not found at {:?}; skipping character '{}'",
                    prop_path, char_id
                );
                continue;
            }

            for frame_name in &frame_names {
                let frame_path = refs_dir.join(frame_name);
                let orig_name = match frame_name.strip_suffix(".jpg") {
                    Some(stem) => format!("{}.orig.jpg", stem),
                    None => format!("{}.orig", frame_name),
                };
                let orig_path = refs_dir.join(&orig_name);

                if !frame_path.is_file() && !orig_path.is_file() {
                    continue;
                }

                // Check idempotency: skip if already edited and !force
                let already_edited = edits.iter().any(|r| r.frame == *frame_name);
                if already_edited && !force {
                    continue;
                }

                // Preserve original frame
                if !orig_path.is_file() {
                    std::fs::copy(&frame_path, &orig_path).with_context(|| {
                        format!("Failed to preserve original frame to {:?}", orig_path)
                    })?;
                }

                // Detect head bbox on original frame.
                // A previously recorded bbox is reused when the frame was already
                // edited, so that re-compositing with a new prop asset does not
                // pay for another vision call.
                let appearance = if !char_def.appearance_en.is_empty() {
                    &char_def.appearance_en
                } else {
                    &char_def.name_en
                };

                let recorded_bbox = edits
                    .iter()
                    .find(|r| r.frame == *frame_name && r.character == **char_id)
                    .and_then(|r| BBox::from_array(r.bbox));

                let bbox_opt = match recorded_bbox {
                    Some(b) => Some(b),
                    None => match detect_head_bbox(&orig_path, appearance, api_key.as_deref()) {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!(
                                "Warning: Head detection failed for shot {} {}: {:#}",
                                shot_id, frame_name, e
                            );
                            None
                        }
                    },
                };

                let bbox = match bbox_opt {
                    Some(b) => b,
                    None => {
                        eprintln!(
                            "Warning: Character '{}' head not detected in {}/{}; skipping edit",
                            char_id, shot_id, frame_name
                        );
                        continue;
                    }
                };

                // Composite prop onto frame_path
                match composite_prop(
                    &orig_path,
                    &prop_path,
                    &frame_path,
                    &bbox,
                    char_def.cap_scale(),
                    char_def.cap_offset(),
                ) {
                    Ok(true) => {
                        let record = AnchorEditRecord {
                            frame: frame_name.to_string(),
                            character: char_id.to_string(),
                            bbox: bbox.to_array(),
                            prop: prop_rel.clone(),
                            applied_at: chrono::Utc::now().to_rfc3339(),
                        };

                        // Update edit record
                        if let Some(pos) = edits.iter().position(|r| r.frame == *frame_name) {
                            edits[pos] = record;
                        } else {
                            edits.push(record);
                        }
                        shot_modified = true;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to composite prop for shot {} {}: {:#}",
                            shot_id, frame_name, e
                        );
                    }
                }
            }
        }

        if shot_modified {
            save_anchor_edits(&anchor_edit_path, &edits)?;
            edited_shots.push(shot_id);
        }
    }

    Ok(edited_shots)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbox_validation() {
        let valid = BBox {
            x: 0.263,
            y: 0.079,
            w: 0.206,
            h: 0.519,
        };
        assert!(valid.is_valid());

        // Negative coordinate
        let invalid_neg = BBox {
            x: -0.01,
            y: 0.1,
            w: 0.2,
            h: 0.2,
        };
        assert!(!invalid_neg.is_valid());

        // Too small width
        let invalid_small_w = BBox {
            x: 0.1,
            y: 0.1,
            w: 0.01,
            h: 0.2,
        };
        assert!(!invalid_small_w.is_valid());

        // Out of bounds (x + w > 1.02)
        let invalid_oob = BBox {
            x: 0.9,
            y: 0.1,
            w: 0.2,
            h: 0.2,
        };
        assert!(!invalid_oob.is_valid());
    }

    #[test]
    fn test_trim_transparent() {
        // 10x10 image with a 4x4 non-transparent square in the middle
        let mut img = RgbaImage::new(10, 10);
        for y in 3..7 {
            for x in 4..8 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }

        let cropped = trim_transparent(&img).expect("should find non-transparent area");
        assert_eq!(cropped.dimensions(), (4, 4));
        assert_eq!(cropped.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));

        // Fully transparent image
        let empty_img = RgbaImage::new(5, 5);
        assert!(trim_transparent(&empty_img).is_none());
    }

    #[test]
    fn test_manual_alpha_blending() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base_path = temp_dir.path().join("base.jpg");
        let prop_path = temp_dir.path().join("prop.png");
        let out_path = temp_dir.path().join("out.jpg");

        // 100x100 white base image
        let base_img = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        base_img.save(&base_path).unwrap();

        // 50x50 semi-transparent red prop (alpha = 128 / 255 ≈ 0.502)
        let prop_img = image::RgbaImage::from_pixel(50, 50, Rgba([200, 0, 0, 128]));
        prop_img.save(&prop_path).unwrap();

        let bbox = BBox {
            x: 0.2,
            y: 0.2,
            w: 0.4,
            h: 0.4,
        };
        let res = composite_prop(&base_path, &prop_path, &out_path, &bbox, 1.0, 0.0)
            .expect("compositing should succeed");
        assert!(res);
        assert!(out_path.is_file());

        let out_img = image::open(&out_path).unwrap().to_rgb8();
        assert_eq!(out_img.dimensions(), (100, 100));

        // Center pixel should be blended (not pure white 255, 255, 255)
        let center_p = out_img.get_pixel(40, 40);
        assert!(center_p[0] < 255); // Red blended with white
        assert!(center_p[1] < 255); // Green lowered
        assert!(center_p[2] < 255); // Blue lowered
    }

    #[test]
    fn test_anchor_edit_record_serialization() {
        let rec = AnchorEditRecord {
            frame: "first_frame.jpg".to_string(),
            character: "walter".to_string(),
            bbox: [0.263, 0.079, 0.206, 0.519],
            prop: "assets/props/kbo_cap.png".to_string(),
            applied_at: "2026-11-14T21:00:00Z".to_string(),
        };

        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("anchor_edit.json");

        // Save single record
        save_anchor_edits(&path, std::slice::from_ref(&rec)).unwrap();
        let loaded = load_anchor_edits(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], rec);

        // Save multiple records
        let rec2 = AnchorEditRecord {
            frame: "last_frame.jpg".to_string(),
            character: "walter".to_string(),
            bbox: [0.270, 0.080, 0.210, 0.520],
            prop: "assets/props/kbo_cap.png".to_string(),
            applied_at: "2026-11-14T21:05:00Z".to_string(),
        };
        save_anchor_edits(&path, &[rec.clone(), rec2.clone()]).unwrap();
        let loaded_multi = load_anchor_edits(&path);
        assert_eq!(loaded_multi.len(), 2);
        assert_eq!(loaded_multi[0], rec);
        assert_eq!(loaded_multi[1], rec2);
    }

    #[test]
    fn test_anchor_edit_idempotency_and_orig_preservation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let proj = temp_dir.path();

        // 1. Setup characters.toml
        let custom_dir = proj.join("customization");
        std::fs::create_dir_all(&custom_dir).unwrap();
        std::fs::write(
            custom_dir.join("characters.toml"),
            r#"
[characters.walter]
name_en = "Walter White"
appearance_en = "bald man"
cap_ref = "assets/props/kbo_cap.png"
cap_scale = 1.30
cap_offset = -0.05
"#,
        )
        .unwrap();

        // 2. Setup prop image
        let props_dir = proj.join("assets/props");
        std::fs::create_dir_all(&props_dir).unwrap();
        let prop_img = RgbaImage::from_pixel(40, 40, Rgba([200, 16, 46, 255]));
        prop_img.save(props_dir.join("kbo_cap.png")).unwrap();

        // 3. Setup shot directory and frames
        let refs_dir = proj.join("shots/shot_0001/refs");
        std::fs::create_dir_all(&refs_dir).unwrap();
        let base_img = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        base_img.save(refs_dir.join("first_frame.jpg")).unwrap();

        // 4. Setup timeline.json
        std::fs::write(
            proj.join("timeline.json"),
            r#"{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 3.0,
      "duration": 3.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    }
  ]
}"#,
        )
        .unwrap();

        // Set mock bbox
        std::env::set_var("REA_MOVIE_MOCK_BBOX", "0.2,0.1,0.3,0.3");

        // First run
        let edited = run_anchor_edit(proj, None, None, false).expect("first run should succeed");
        assert_eq!(edited, vec!["shot_0001"]);

        // Verify original frame preserved
        let orig_path = refs_dir.join("first_frame.orig.jpg");
        assert!(orig_path.is_file());
        let orig_bytes = std::fs::read(&orig_path).unwrap();

        // Verify edited frame differs from original
        let edited_path = refs_dir.join("first_frame.jpg");
        let edited_bytes = std::fs::read(&edited_path).unwrap();
        assert_ne!(orig_bytes, edited_bytes);

        // Verify anchor_edit.json
        let edit_log_path = refs_dir.join("anchor_edit.json");
        assert!(edit_log_path.is_file());
        let edits = load_anchor_edits(&edit_log_path);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].frame, "first_frame.jpg");
        assert_eq!(edits[0].character, "walter");

        // Second run without force (idempotency: bytes must remain unchanged)
        let edited_second =
            run_anchor_edit(proj, None, None, false).expect("second run should succeed");
        assert!(edited_second.is_empty());
        let second_bytes = std::fs::read(&edited_path).unwrap();
        assert_eq!(edited_bytes, second_bytes);

        std::env::remove_var("REA_MOVIE_MOCK_BBOX");
    }

    #[test]
    fn test_skip_when_no_cap_ref() {
        let temp_dir = tempfile::tempdir().unwrap();
        let proj = temp_dir.path();

        let custom_dir = proj.join("customization");
        std::fs::create_dir_all(&custom_dir).unwrap();
        std::fs::write(
            custom_dir.join("characters.toml"),
            r#"
[characters.jesse]
name_en = "Jesse Pinkman"
appearance_en = "young man"
"#,
        )
        .unwrap();

        let refs_dir = proj.join("shots/shot_0001/refs");
        std::fs::create_dir_all(&refs_dir).unwrap();
        let base_img = image::RgbImage::from_pixel(100, 100, image::Rgb([255, 255, 255]));
        base_img.save(refs_dir.join("first_frame.jpg")).unwrap();

        std::fs::write(
            proj.join("timeline.json"),
            r#"{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 3.0,
      "duration": 3.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["jesse"]
    }
  ]
}"#,
        )
        .unwrap();

        let edited = run_anchor_edit(proj, None, None, false).unwrap();
        assert!(edited.is_empty());
        assert!(!refs_dir.join("first_frame.orig.jpg").exists());
        assert!(!refs_dir.join("anchor_edit.json").exists());
    }
}
