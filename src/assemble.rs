use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::ProjectConfig;
use crate::timeline::TimelineManifest;

#[allow(dead_code)]
pub struct AssembleOptions<'a> {
    pub proj_dir: &'a Path,
    pub smoke: bool,
}

#[derive(Debug, Clone)]
pub struct AssembleShot {
    pub shot_id: String,
    pub render_video: PathBuf,
    pub orig_clip: PathBuf,
    pub duration: Option<f64>,
}

pub fn run_assemble(opts: AssembleOptions) -> Result<()> {
    let profile = if opts.smoke { "smoke" } else { "prod" };
    println!("{} ==> Running assemble step for profile: {}", crate::log::stamp(), profile);

    let config = load_project_config(opts.proj_dir)?;
    let movie_name = resolve_movie_name(opts.proj_dir, &config);

    let shots = collect_assemble_shots(opts.proj_dir, profile)?;
    if shots.is_empty() {
        bail!("No shots found to assemble in {:?}", opts.proj_dir);
    }
    println!(
        "{} ==> Assembling {} shot(s) for movie: {}",
        crate::log::stamp(),
        shots.len(),
        movie_name
    );

    let media_dir = crate::config::resolve_media_root(opts.proj_dir);
    let output_dir = media_dir.join("outputs").join(profile);
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;

    let final_movie_path = output_dir.join(format!("{}_{}.mp4", movie_name, profile));
    let compare_movie_path = output_dir.join(format!("{}_compare.mp4", movie_name));

    let trim_dir = output_dir.join(".trim");
    let assemble_result = (|| -> Result<()> {
        // 1. Assemble final movie
        assemble_final_movie(&shots, &final_movie_path, &output_dir)?;
        println!("==> Final movie created: {}", final_movie_path.display());

        // 2. Assemble side-by-side compare video
        assemble_compare_movie(&shots, &compare_movie_path, &output_dir)?;
        println!(
            "==> Compare movie created: {}",
            compare_movie_path.display()
        );
        Ok(())
    })();

    // Always clean up temporary .trim directory
    if trim_dir.exists() {
        let _ = fs::remove_dir_all(&trim_dir);
    }

    assemble_result?;

    println!("{} ==> Assemble complete for profile: {}", crate::log::stamp(), profile);
    Ok(())
}

fn load_project_config(proj_dir: &Path) -> Result<ProjectConfig> {
    let config_path = proj_dir.join("rea-movie.toml");
    if config_path.is_file() {
        ProjectConfig::load_from_file(&config_path)
            .with_context(|| format!("Failed to load {}", config_path.display()))
    } else {
        Ok(ProjectConfig::default())
    }
}

fn resolve_movie_name(proj_dir: &Path, config: &ProjectConfig) -> String {
    let cfg_name = config.project.name.trim();
    if !cfg_name.is_empty() && cfg_name != "movie_project" {
        return cfg_name.to_string();
    }
    proj_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("movie")
        .to_string()
}

fn collect_assemble_shots(proj_dir: &Path, profile: &str) -> Result<Vec<AssembleShot>> {
    let proj_dir = proj_dir
        .canonicalize()
        .unwrap_or_else(|_| proj_dir.to_path_buf());
    let mut shot_ids = Vec::new();
    let mut timeline_durations = HashMap::new();

    let timeline_path = proj_dir.join("timeline.json");
    if timeline_path.is_file() {
        if let Ok(manifest) = TimelineManifest::load_from_file(&timeline_path) {
            for item in manifest.shots {
                shot_ids.push(item.shot_id.clone());
                timeline_durations.insert(item.shot_id, item.duration);
            }
        }
    }

    if shot_ids.is_empty() {
        let shots_dir = proj_dir.join("shots");
        if shots_dir.is_dir() {
            for entry in fs::read_dir(&shots_dir)?.flatten() {
                if entry.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with("shot_") {
                            shot_ids.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    shot_ids.sort_by_key(|id| shot_number(id));
    shot_ids.dedup();

    let media_dir = crate::config::resolve_media_root(&proj_dir);
    let mut assemble_shots = Vec::with_capacity(shot_ids.len());
    for shot_id in shot_ids {
        let media_shot_dir = media_dir.join("shots").join(&shot_id);
        let render_video = media_shot_dir
            .join("renders")
            .join(profile)
            .join(format!("{}.mp4", shot_id));
        let orig_clip = media_shot_dir.join("refs").join("original_clip.mp4");

        if !render_video.is_file() || fs::metadata(&render_video)?.len() == 0 {
            bail!(
                "Shot render video missing for {} at {:?}. Run `rea-movie render` first.",
                shot_id,
                render_video
            );
        }

        let render_video = fs::canonicalize(&render_video).unwrap_or(render_video);
        let orig_clip = fs::canonicalize(&orig_clip).unwrap_or(orig_clip);
        let duration = timeline_durations.get(&shot_id).copied();

        assemble_shots.push(AssembleShot {
            shot_id,
            render_video,
            orig_clip,
            duration,
        });
    }

    Ok(assemble_shots)
}

fn shot_number(shot_id: &str) -> u32 {
    shot_id
        .rsplit('_')
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

fn has_audio_stream(path: &Path) -> bool {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output();
    match output {
        Ok(out) => out.status.success() && !out.stdout.is_empty(),
        Err(_) => false,
    }
}

fn probe_video_height(path: &Path) -> Option<u32> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim().parse::<u32>().ok()
}

fn find_system_font() -> Option<String> {
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf",
        "/usr/share/fonts/truetype/freefont/FreeSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
    ];
    for path in candidates {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    None
}

fn escape_ffmpeg_path(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    s.replace('\\', "/").replace('\'', "'\\''")
}

fn probe_video_duration(path: &Path) -> Option<f64> {
    if let Ok(output) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Ok(dur) = s.trim().parse::<f64>() {
                return Some(dur);
            }
        }
    }

    if let Ok(output) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Ok(dur) = s.trim().parse::<f64>() {
                return Some(dur);
            }
        }
    }

    None
}

fn probe_video_fps(path: &Path) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=r_frame_rate",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    let trimmed = s.trim();
    if let Some((num, den)) = trimmed.split_once('/') {
        let n: f64 = num.parse().ok()?;
        let d: f64 = den.parse().ok()?;
        if d > 0.0 {
            return Some(n / d);
        }
    }
    trimmed.parse::<f64>().ok()
}

fn trim_video_clip(input_path: &Path, out_path: &Path, duration: f64) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {:?}", parent))?;
    }

    let file_stem = out_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("trimmed");
    let temp_name = format!(".{}_{}.tmp.mp4", file_stem, std::process::id());
    let temp_out = out_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(temp_name);

    let fps = probe_video_fps(input_path).unwrap_or(24.0);
    let frames = (duration * fps).round().max(1.0);
    let trim_dur = frames / fps;

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y", "-i"])
        .arg(input_path)
        .args([
            "-t",
            &format!("{:.6}", trim_dur),
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-ar",
            "32000",
            "-ac",
            "2",
            "-f",
            "mp4",
        ])
        .arg(&temp_out);

    let output = cmd
        .output()
        .with_context(|| format!("Failed to run ffmpeg to trim {:?}", input_path))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = fs::remove_file(&temp_out);
        bail!("ffmpeg trim failed for {:?}: {}", input_path, stderr.trim());
    }

    if out_path.exists() {
        fs::remove_file(out_path)?;
    }
    fs::rename(&temp_out, out_path)
        .with_context(|| format!("Failed to rename {:?} to {:?}", temp_out, out_path))?;

    Ok(())
}

fn prepare_effective_shots(shots: &[AssembleShot], output_dir: &Path) -> Result<Vec<AssembleShot>> {
    let trim_dir = output_dir.join(".trim");
    let mut effective_shots = Vec::with_capacity(shots.len());

    for shot in shots {
        let mut eff = shot.clone();

        if let Some(target_dur) = shot.duration {
            // 1. Render clip trimming if actual duration > target_dur + 0.08
            if let Some(render_dur) = probe_video_duration(&shot.render_video) {
                if render_dur > target_dur + 0.08 {
                    let trim_render_path = trim_dir.join(format!("{}.mp4", shot.shot_id));
                    let is_valid = trim_render_path.is_file()
                        && fs::metadata(&trim_render_path)
                            .map(|m| m.len() > 0)
                            .unwrap_or(false);
                    if !is_valid {
                        trim_video_clip(&shot.render_video, &trim_render_path, target_dur)?;
                    }
                    eff.render_video = trim_render_path;
                }
            }

            // 2. Original clip trimming (for compare movie and dual audio sync)
            if shot.orig_clip.is_file() {
                if let Some(orig_dur) = probe_video_duration(&shot.orig_clip) {
                    if orig_dur > target_dur + 0.08 {
                        let trim_orig_path = trim_dir.join(format!("{}_orig.mp4", shot.shot_id));
                        let is_valid = trim_orig_path.is_file()
                            && fs::metadata(&trim_orig_path)
                                .map(|m| m.len() > 0)
                                .unwrap_or(false);
                        if !is_valid {
                            trim_video_clip(&shot.orig_clip, &trim_orig_path, target_dur)?;
                        }
                        eff.orig_clip = trim_orig_path;
                    }
                }
            }
        }

        effective_shots.push(eff);
    }

    Ok(effective_shots)
}

fn assemble_final_movie(shots: &[AssembleShot], final_path: &Path, work_dir: &Path) -> Result<()> {
    let work_dir = work_dir
        .canonicalize()
        .unwrap_or_else(|_| work_dir.to_path_buf());
    let effective_shots = prepare_effective_shots(shots, &work_dir)?;
    let shots = &effective_shots;

    let h3_list_path = work_dir.join(format!(".h3_concat_{}.txt", std::process::id()));
    let orig_list_path = work_dir.join(format!(".orig_concat_{}.txt", std::process::id()));
    let temp_final_path = work_dir.join(format!(".temp_final_{}.mp4", std::process::id()));

    let mut h3_list_content = String::new();
    let mut orig_list_content = String::new();
    let mut all_orig_clips_exist = true;

    for shot in shots {
        let h3_esc = escape_ffmpeg_path(&shot.render_video);
        h3_list_content.push_str(&format!("file '{}'\n", h3_esc));

        if shot.orig_clip.is_file() {
            let orig_esc = escape_ffmpeg_path(&shot.orig_clip);
            orig_list_content.push_str(&format!("file '{}'\n", orig_esc));
        } else {
            all_orig_clips_exist = false;
        }
    }

    fs::write(&h3_list_path, &h3_list_content)?;
    if all_orig_clips_exist {
        fs::write(&orig_list_path, &orig_list_content)?;
    }

    let h3_has_audio = shots
        .first()
        .map(|s| has_audio_stream(&s.render_video))
        .unwrap_or(false);
    let orig_has_audio = all_orig_clips_exist
        && shots
            .first()
            .map(|s| has_audio_stream(&s.orig_clip))
            .unwrap_or(false);

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y"]);

    // Input 0: H3 rendered shots
    cmd.args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&h3_list_path);

    // Input 1: Original reference shots (if available)
    if orig_has_audio {
        cmd.args(["-f", "concat", "-safe", "0", "-i"])
            .arg(&orig_list_path);
    }

    // Map video from H3
    cmd.args(["-map", "0:v:0"]);

    // Audio stream 0 (Track 1): H3 generated audio
    if h3_has_audio {
        cmd.args([
            "-map",
            "0:a:0",
            "-metadata:s:a:0",
            "title=H3 Generated Audio",
        ]);
    }

    // Audio stream 1 (Track 2): Original movie audio
    if orig_has_audio {
        let audio_idx = if h3_has_audio { "1" } else { "0" };
        cmd.args([
            "-map",
            "1:a:0",
            &format!("-metadata:s:a:{}", audio_idx),
            "title=Original Movie Audio",
        ]);
    }

    cmd.args([
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-ar",
        "32000",
        "-ac",
        "2",
        "-movflags",
        "+faststart",
    ]);
    cmd.arg(&temp_final_path);

    let output = cmd
        .output()
        .context("Failed to run ffmpeg for final movie assembly")?;

    let _ = fs::remove_file(&h3_list_path);
    let _ = fs::remove_file(&orig_list_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = fs::remove_file(&temp_final_path);
        bail!("ffmpeg final movie assembly failed: {}", stderr.trim());
    }

    if final_path.exists() {
        fs::remove_file(final_path)?;
    }
    fs::rename(&temp_final_path, final_path)
        .with_context(|| format!("Failed to move assembled video to {:?}", final_path))?;

    Ok(())
}

fn assemble_compare_movie(
    shots: &[AssembleShot],
    compare_path: &Path,
    work_dir: &Path,
) -> Result<()> {
    let work_dir = work_dir
        .canonicalize()
        .unwrap_or_else(|_| work_dir.to_path_buf());
    let effective_shots = prepare_effective_shots(shots, &work_dir)?;
    let shots = &effective_shots;

    let pid = std::process::id();
    let temp_compare_dir = work_dir.join(format!(".tmp_compare_{}", pid));
    fs::create_dir_all(&temp_compare_dir)?;

    let system_font = find_system_font();
    let mut compare_shot_files = Vec::with_capacity(shots.len());

    for (idx, shot) in shots.iter().enumerate() {
        let out_shot_clip = temp_compare_dir.join(format!("compare_{:04}.mp4", idx + 1));
        build_single_shot_compare(shot, &out_shot_clip, system_font.as_deref())?;
        compare_shot_files.push(out_shot_clip);
    }

    // Concat all shot compare clips together
    let concat_list_path = temp_compare_dir.join("compare_list.txt");
    let mut concat_content = String::new();
    for clip in &compare_shot_files {
        concat_content.push_str(&format!("file '{}'\n", escape_ffmpeg_path(clip)));
    }
    fs::write(&concat_list_path, &concat_content)?;

    let temp_compare_final = work_dir.join(format!(".temp_compare_final_{}.mp4", pid));

    // Try fast stream copy concat first
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i",
        ])
        .arg(&concat_list_path)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(&temp_compare_final)
        .output();

    let success = match output {
        Ok(out) => out.status.success(),
        Err(_) => false,
    };

    if !success {
        // Fallback to re-encoding concat if copy failed
        let reencode_output = Command::new("ffmpeg")
            .args([
                "-nostdin", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i",
            ])
            .arg(&concat_list_path)
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
            ])
            .arg(&temp_compare_final)
            .output()
            .context("Failed to run ffmpeg concat for compare movie")?;

        if !reencode_output.status.success() {
            let stderr = String::from_utf8_lossy(&reencode_output.stderr);
            let _ = fs::remove_dir_all(&temp_compare_dir);
            let _ = fs::remove_file(&temp_compare_final);
            bail!("ffmpeg compare concat failed: {}", stderr.trim());
        }
    }

    let _ = fs::remove_dir_all(&temp_compare_dir);

    if compare_path.exists() {
        fs::remove_file(compare_path)?;
    }
    fs::rename(&temp_compare_final, compare_path)
        .with_context(|| format!("Failed to move compare movie to {:?}", compare_path))?;

    Ok(())
}

fn build_single_shot_compare(
    shot: &AssembleShot,
    out_path: &Path,
    system_font: Option<&str>,
) -> Result<()> {
    let render_height = probe_video_height(&shot.render_video).unwrap_or(576);
    let orig_input = if shot.orig_clip.is_file() {
        &shot.orig_clip
    } else {
        &shot.render_video
    };

    let shot_num_u32 = shot_number(&shot.shot_id);
    let label_text = format!("Shot #{:04}", shot_num_u32);

    let h3_has_audio = has_audio_stream(&shot.render_video);
    let orig_has_audio = shot.orig_clip.is_file() && has_audio_stream(&shot.orig_clip);

    // 1. Try with drawtext filter
    let drawtext_part = if let Some(font) = system_font {
        format!(
            "drawtext=fontfile='{}':text='{}':fontsize=28:fontcolor=white:box=1:boxcolor=black@0.6:boxborderw=6:x=w-tw-20:y=20",
            font, label_text
        )
    } else {
        format!(
            "drawtext=text='{}':fontsize=28:fontcolor=white:box=1:boxcolor=black@0.6:boxborderw=6:x=w-tw-20:y=20",
            label_text
        )
    };

    let filter_with_label = format!(
        "[0:v]scale=-2:{h}[left];[1:v]{drawtext}[right];[left][right]hstack=inputs=2[v]",
        h = render_height,
        drawtext = drawtext_part
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y"]);
    cmd.args(["-i"]).arg(orig_input);
    cmd.args(["-i"]).arg(&shot.render_video);
    cmd.args(["-filter_complex", &filter_with_label, "-map", "[v]"]);

    if h3_has_audio {
        cmd.args(["-map", "1:a:0"]);
    } else if orig_has_audio {
        cmd.args(["-map", "0:a:0"]);
    }

    cmd.args([
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-ar",
        "32000",
        "-ac",
        "2",
        "-movflags",
        "+faststart",
    ]);
    cmd.arg(out_path);

    let output = cmd.output();
    let success = match output {
        Ok(ref out) => out.status.success(),
        Err(_) => false,
    };

    if success {
        return Ok(());
    }

    // 2. Fallback without drawtext if font/drawtext failed
    let filter_fallback = format!(
        "[0:v]scale=-2:{h}[left];[1:v]null[right];[left][right]hstack=inputs=2[v]",
        h = render_height
    );

    let mut fallback_cmd = Command::new("ffmpeg");
    fallback_cmd.args(["-nostdin", "-v", "error", "-y"]);
    fallback_cmd.args(["-i"]).arg(orig_input);
    fallback_cmd.args(["-i"]).arg(&shot.render_video);
    fallback_cmd.args(["-filter_complex", &filter_fallback, "-map", "[v]"]);

    if h3_has_audio {
        fallback_cmd.args(["-map", "1:a:0"]);
    } else if orig_has_audio {
        fallback_cmd.args(["-map", "0:a:0"]);
    }

    fallback_cmd.args([
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-ar",
        "32000",
        "-ac",
        "2",
        "-movflags",
        "+faststart",
    ]);
    fallback_cmd.arg(out_path);

    let fb_output = fallback_cmd
        .output()
        .context("Failed to run ffmpeg fallback for compare shot")?;

    if !fb_output.status.success() {
        let stderr = String::from_utf8_lossy(&fb_output.stderr);
        bail!(
            "ffmpeg failed to build compare shot for {}: {}",
            shot.shot_id,
            stderr.trim()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shot_number_extraction() {
        assert_eq!(shot_number("shot_0001"), 1);
        assert_eq!(shot_number("shot_0025"), 25);
        assert_eq!(shot_number("0005"), 5);
        assert_eq!(shot_number("invalid"), u32::MAX);
    }

    #[test]
    fn test_escape_ffmpeg_path() {
        let p = Path::new("/path/with 'quotes'/and space");
        let esc = escape_ffmpeg_path(p);
        assert_eq!(esc, "/path/with '\\''quotes'\\''/and space");
    }

    fn create_test_video(path: &Path, duration_sec: f64) -> bool {
        let output = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=blue:s=160x120:d={:.3}", duration_sec),
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .output();
        output.map(|o| o.status.success()).unwrap_or(false)
    }

    #[test]
    fn test_probe_video_duration_and_trim_video_clip() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let raw_video = tmp_dir.path().join("raw.mp4");
        if !create_test_video(&raw_video, 3.0) {
            eprintln!("Skipping test: ffmpeg lavfi not available");
            return;
        }

        let dur = probe_video_duration(&raw_video).expect("probe duration of raw video");
        assert!(
            (dur - 3.0).abs() < 0.2,
            "Raw duration should be ~3.0s, got {}",
            dur
        );

        let trimmed_video = tmp_dir.path().join("trimmed.mp4");
        trim_video_clip(&raw_video, &trimmed_video, 1.5).expect("trim video clip");
        assert!(trimmed_video.is_file());

        let trimmed_dur =
            probe_video_duration(&trimmed_video).expect("probe duration of trimmed video");
        assert!(
            (trimmed_dur - 1.5).abs() < 0.2,
            "Trimmed duration should be ~1.5s, got {}",
            trimmed_dur
        );
    }

    #[test]
    fn test_prepare_effective_shots_trim_threshold_and_reuse() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let work_dir = tmp_dir.path().join("outputs").join("smoke");
        fs::create_dir_all(&work_dir).unwrap();

        let v3_path = tmp_dir.path().join("video_3s.mp4");
        let v1_path = tmp_dir.path().join("video_1s.mp4");
        if !create_test_video(&v3_path, 3.0) || !create_test_video(&v1_path, 1.0) {
            eprintln!("Skipping test: ffmpeg lavfi not available");
            return;
        }

        let shots = vec![
            // Case 1: Needs trim (actual 3.0s > target 1.5s + 0.08)
            AssembleShot {
                shot_id: "shot_0001".to_string(),
                render_video: v3_path.clone(),
                orig_clip: v3_path.clone(),
                duration: Some(1.5),
            },
            // Case 2: No trim needed (actual 3.0s <= target 3.0s + 0.08)
            AssembleShot {
                shot_id: "shot_0002".to_string(),
                render_video: v3_path.clone(),
                orig_clip: v3_path.clone(),
                duration: Some(3.0),
            },
            // Case 3: No duration in timeline -> no trim
            AssembleShot {
                shot_id: "shot_0003".to_string(),
                render_video: v3_path.clone(),
                orig_clip: v3_path.clone(),
                duration: None,
            },
            // Case 4: No orig clip file exists -> only render video trimmed
            AssembleShot {
                shot_id: "shot_0004".to_string(),
                render_video: v3_path.clone(),
                orig_clip: tmp_dir.path().join("non_existent.mp4"),
                duration: Some(1.0),
            },
        ];

        let eff = prepare_effective_shots(&shots, &work_dir).expect("prepare effective shots");
        assert_eq!(eff.len(), 4);

        // Shot 1 should be trimmed to .trim/shot_0001.mp4 and .trim/shot_0001_orig.mp4
        assert_eq!(
            eff[0].render_video,
            work_dir.join(".trim").join("shot_0001.mp4")
        );
        assert_eq!(
            eff[0].orig_clip,
            work_dir.join(".trim").join("shot_0001_orig.mp4")
        );
        assert!(eff[0].render_video.is_file());
        assert!(eff[0].orig_clip.is_file());
        let dur_trimmed = probe_video_duration(&eff[0].render_video).unwrap();
        assert!((dur_trimmed - 1.5).abs() < 0.2);

        // Shot 2 should keep original files
        assert_eq!(eff[1].render_video, v3_path);
        assert_eq!(eff[1].orig_clip, v3_path);

        // Shot 3 should keep original files
        assert_eq!(eff[2].render_video, v3_path);
        assert_eq!(eff[2].orig_clip, v3_path);

        // Shot 4: render trimmed, orig_clip kept as nonexistent path
        assert_eq!(
            eff[3].render_video,
            work_dir.join(".trim").join("shot_0004.mp4")
        );
        assert_eq!(eff[3].orig_clip, tmp_dir.path().join("non_existent.mp4"));

        // Reuse test: calling again should reuse existing files without error
        let eff2 =
            prepare_effective_shots(&shots, &work_dir).expect("prepare effective shots reuse");
        assert_eq!(eff2[0].render_video, eff[0].render_video);
    }

    #[test]
    fn test_collect_assemble_shots_reads_timeline_duration() {
        let tmp_dir = tempfile::tempdir().expect("create temp dir");
        let proj_dir = tmp_dir.path();
        let shot_render_dir = proj_dir
            .join("shots")
            .join("shot_0001")
            .join("renders")
            .join("smoke");
        fs::create_dir_all(&shot_render_dir).unwrap();
        let shot_file = shot_render_dir.join("shot_0001.mp4");
        fs::write(&shot_file, b"dummy video content").unwrap();

        // 1. With timeline.json
        let timeline_json = r#"{
            "shots": [
                {
                    "shot_id": "shot_0001",
                    "start_sec": 0.0,
                    "end_sec": 2.45,
                    "duration": 2.45,
                    "is_chained_longtake": false,
                    "anchor": "first_and_last",
                    "assignee": ""
                }
            ]
        }"#;
        fs::write(proj_dir.join("timeline.json"), timeline_json).unwrap();

        let shots = collect_assemble_shots(proj_dir, "smoke").expect("collect assemble shots");
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].shot_id, "shot_0001");
        assert_eq!(shots[0].duration, Some(2.45));

        // 2. Without timeline.json -> duration should be None
        fs::remove_file(proj_dir.join("timeline.json")).unwrap();
        let shots_no_tl =
            collect_assemble_shots(proj_dir, "smoke").expect("collect without timeline");
        assert_eq!(shots_no_tl.len(), 1);
        assert_eq!(shots_no_tl[0].shot_id, "shot_0001");
        assert_eq!(shots_no_tl[0].duration, None);
    }
}
