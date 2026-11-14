use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::decompile::contact_sheet::generate_contact_sheet;

pub struct ShotExtractPaths<'a> {
    pub shot_dir: &'a Path,
    pub original_clip: &'a Path,
    pub original_audio: &'a Path,
    pub first_frame: &'a Path,
    pub last_frame: &'a Path,
    pub contact_sheet: &'a Path,
}

pub fn extract_shot_references(
    video_path: &str,
    start_sec: f64,
    end_sec: f64,
    duration: f64,
    paths: &ShotExtractPaths,
    force: bool,
) -> Result<()> {
    // 1. Ensure directory layout
    if let Some(parent) = paths.original_clip.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let prod_dir = paths.shot_dir.join("renders").join("prod");
    let smoke_dir = paths.shot_dir.join("renders").join("smoke");
    std::fs::create_dir_all(&prod_dir)?;
    std::fs::create_dir_all(&smoke_dir)?;

    // 2. Extract original_clip.mp4
    if force || !paths.original_clip.exists() {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", &format!("{:.3}", start_sec),
                "-to", &format!("{:.3}", end_sec),
                "-i", video_path,
                "-c:v", "libx264",
                "-pix_fmt", "yuv420p",
                "-c:a", "aac",
                paths.original_clip.to_str().unwrap(),
            ])
            .output()
            .with_context(|| format!("Failed to extract original clip for shot at {:.2}s", start_sec))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            anyhow::bail!("ffmpeg original clip extract failed: {}", stderr);
        }
    }

    // 3. Extract original_audio.wav
    if force || !paths.original_audio.exists() {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", &format!("{:.3}", start_sec),
                "-to", &format!("{:.3}", end_sec),
                "-i", video_path,
                "-vn",
                "-acodec", "pcm_s16le",
                "-ar", "32000",
                "-ac", "2",
                paths.original_audio.to_str().unwrap(),
            ])
            .output()
            .with_context(|| format!("Failed to extract original audio for shot at {:.2}s", start_sec))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            anyhow::bail!("ffmpeg original audio extract failed: {}", stderr);
        }
    }

    // 4. Extract first_frame.jpg (+2 frames or min(duration*0.1, 0.08)s)
    if force || !paths.first_frame.exists() {
        let offset = 0.08_f64.min(duration * 0.1);
        let first_time = start_sec + offset;
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", &format!("{:.3}", first_time),
                "-i", video_path,
                "-vframes", "1",
                "-q:v", "2",
                paths.first_frame.to_str().unwrap(),
            ])
            .output()
            .with_context(|| format!("Failed to extract first frame at {:.3}s", first_time))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            anyhow::bail!("ffmpeg first frame extract failed: {}", stderr);
        }
    }

    // 5. Extract last_frame.jpg (-2 frames or max(start, end - 0.08)s)
    if force || !paths.last_frame.exists() {
        let offset = 0.08_f64.min(duration * 0.1);
        let last_time = (end_sec - offset).max(start_sec);
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss", &format!("{:.3}", last_time),
                "-i", video_path,
                "-vframes", "1",
                "-q:v", "2",
                paths.last_frame.to_str().unwrap(),
            ])
            .output()
            .with_context(|| format!("Failed to extract last frame at {:.3}s", last_time))?;

        if !status.status.success() {
            let stderr = String::from_utf8_lossy(&status.stderr);
            anyhow::bail!("ffmpeg last frame extract failed: {}", stderr);
        }
    }

    // 6. Generate contact_sheet.jpg
    if force || !paths.contact_sheet.exists() {
        generate_contact_sheet(video_path, start_sec, duration, paths.contact_sheet)?;
    }

    Ok(())
}
