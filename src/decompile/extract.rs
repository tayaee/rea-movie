use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::decompile::contact_sheet::generate_contact_sheet;
use crate::sources::{materialize_asset, SourceCache};

pub struct ShotExtractPaths<'a> {
    pub media_shot_dir: &'a Path,
    pub original_clip: &'a Path,
    pub original_audio: &'a Path,
    pub first_frame: &'a Path,
    pub last_frame: &'a Path,
    pub contact_sheet: &'a Path,
}

fn run_ffmpeg(args: &[String], what: &str) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run ffmpeg ({what})"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg {} failed: {}", what, stderr);
    }
    Ok(())
}

fn path_arg(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

/// Extract (or reuse) the per-shot reference assets.
///
/// Issue 8: when a [`SourceCache`] is given, every asset is looked up in
/// `<root>/sources/<id>/` first — anchor frames by timestamp under
/// `keyframes/`, clip/audio/contact sheet by span under `shots_cache/` — and
/// copied instead of re-running ffmpeg. Fresh extractions are stored back
/// into the cache. Existing project files are kept unless `force`.
pub fn extract_shot_references(
    video_path: &str,
    start_sec: f64,
    end_sec: f64,
    duration: f64,
    paths: &ShotExtractPaths,
    force: bool,
    cache: Option<&SourceCache>,
) -> Result<()> {
    // 1. Ensure directory layout
    if let Some(parent) = paths.original_clip.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.first_frame.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let prod_dir = paths.media_shot_dir.join("renders").join("prod");
    let smoke_dir = paths.media_shot_dir.join("renders").join("smoke");
    std::fs::create_dir_all(&prod_dir)?;
    std::fs::create_dir_all(&smoke_dir)?;

    let span_dir = cache.map(|c| c.shot_cache_dir(start_sec, end_sec));
    let span_cached = |name: &str| span_dir.as_ref().map(|d| d.join(name));
    let video = video_path.to_string();
    let start_s = format!("{:.3}", start_sec);
    let end_s = format!("{:.3}", end_sec);

    // 2. original_clip.mp4
    let cached = span_cached("original_clip.mp4");
    materialize_asset(
        cache,
        paths.original_clip,
        cached.as_deref(),
        force,
        |out| {
            run_ffmpeg(
                &[
                    "-y".into(),
                    "-ss".into(),
                    start_s.clone(),
                    "-to".into(),
                    end_s.clone(),
                    "-i".into(),
                    video.clone(),
                    "-c:v".into(),
                    "libx264".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                    "-c:a".into(),
                    "aac".into(),
                    path_arg(out),
                ],
                "original clip extract",
            )
            .with_context(|| {
                format!(
                    "Failed to extract original clip for shot at {:.2}s",
                    start_sec
                )
            })
        },
    )?;

    // 3. original_audio.wav
    let cached = span_cached("original_audio.wav");
    materialize_asset(
        cache,
        paths.original_audio,
        cached.as_deref(),
        force,
        |out| {
            run_ffmpeg(
                &[
                    "-y".into(),
                    "-ss".into(),
                    start_s.clone(),
                    "-to".into(),
                    end_s.clone(),
                    "-i".into(),
                    video.clone(),
                    "-vn".into(),
                    "-acodec".into(),
                    "pcm_s16le".into(),
                    "-ar".into(),
                    "32000".into(),
                    "-ac".into(),
                    "2".into(),
                    path_arg(out),
                ],
                "original audio extract",
            )
            .with_context(|| {
                format!(
                    "Failed to extract original audio for shot at {:.2}s",
                    start_sec
                )
            })
        },
    )?;

    // 4/5. Anchor frames: first (+2 frames or min(duration*0.1, 0.08)s) and
    // last (-2 frames). Cached by absolute timestamp under keyframes/.
    // ffmpeg 8 + container-duration > video-duration edge: last_time can land
    // past the video stream (audio extends further). Try t, then earlier
    // fallbacks so one boundary frame never fails the whole decompile.
    let offset = 0.08_f64.min(duration * 0.1);
    let first_time = start_sec + offset;
    let last_time = (end_sec - offset).max(start_sec);
    for (target, t, what) in [
        (paths.first_frame, first_time, "first frame extract"),
        (paths.last_frame, last_time, "last frame extract"),
    ] {
        let cached = cache.map(|c| c.anchor_frame_path(t));
        materialize_asset(cache, target, cached.as_deref(), force, |out| {
            let candidates = [t, t - 0.2, t - 0.4, start_sec + 0.05];
            let mut last_err = String::new();
            for ct in candidates {
                let ct = ct
                    .max(start_sec)
                    .min((start_sec + duration - 0.05).max(start_sec));
                let res = run_ffmpeg(
                    &[
                        "-y".into(),
                        "-ss".into(),
                        format!("{:.3}", ct),
                        "-i".into(),
                        video.clone(),
                        "-vframes".into(),
                        "1".into(),
                        "-pix_fmt".into(),
                        "yuvj420p".into(),
                        "-q:v".into(),
                        "2".into(),
                        path_arg(out),
                    ],
                    what,
                );
                match res {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = format!("{e:#}"),
                }
            }
            anyhow::bail!("ffmpeg {what} failed (t={t:.3}): {last_err}")
        })?;
    }

    // 6. contact_sheet.jpg
    let cached = span_cached("contact_sheet.jpg");
    materialize_asset(
        cache,
        paths.contact_sheet,
        cached.as_deref(),
        force,
        |out| generate_contact_sheet(video_path, start_sec, duration, out),
    )?;

    Ok(())
}
