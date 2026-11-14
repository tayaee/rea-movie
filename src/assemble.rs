use anyhow::{bail, Context, Result};
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
struct AssembleShot {
    shot_id: String,
    render_video: PathBuf,
    orig_clip: PathBuf,
}

pub fn run_assemble(opts: AssembleOptions) -> Result<()> {
    let profile = if opts.smoke { "smoke" } else { "prod" };
    println!("==> Running assemble step for profile: {}", profile);

    let config = load_project_config(opts.proj_dir)?;
    let movie_name = resolve_movie_name(opts.proj_dir, &config);

    let shots = collect_assemble_shots(opts.proj_dir, profile)?;
    if shots.is_empty() {
        bail!("No shots found to assemble in {:?}", opts.proj_dir);
    }
    println!("==> Assembling {} shot(s) for movie: {}", shots.len(), movie_name);

    let media_dir = crate::config::resolve_media_root(opts.proj_dir);
    let output_dir = media_dir.join("outputs").join(profile);
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;

    let final_movie_path = output_dir.join(format!("{}_{}.mp4", movie_name, profile));
    let compare_movie_path = output_dir.join(format!("{}_compare.mp4", movie_name));

    // 1. Assemble final movie
    assemble_final_movie(&shots, &final_movie_path, &output_dir)?;
    println!("==> Final movie created: {}", final_movie_path.display());

    // 2. Assemble side-by-side compare video
    assemble_compare_movie(&shots, &compare_movie_path, &output_dir)?;
    println!("==> Compare movie created: {}", compare_movie_path.display());

    println!("==> Assemble complete for profile: {}", profile);
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
    let proj_dir = proj_dir.canonicalize().unwrap_or_else(|_| proj_dir.to_path_buf());
    let mut shot_ids = Vec::new();

    let timeline_path = proj_dir.join("timeline.json");
    if timeline_path.is_file() {
        if let Ok(manifest) = TimelineManifest::load_from_file(&timeline_path) {
            for item in manifest.shots {
                shot_ids.push(item.shot_id);
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

        assemble_shots.push(AssembleShot {
            shot_id,
            render_video,
            orig_clip,
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

fn assemble_final_movie(
    shots: &[AssembleShot],
    final_path: &Path,
    work_dir: &Path,
) -> Result<()> {
    let work_dir = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
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

    let h3_has_audio = shots.first().map(|s| has_audio_stream(&s.render_video)).unwrap_or(false);
    let orig_has_audio = all_orig_clips_exist
        && shots.first().map(|s| has_audio_stream(&s.orig_clip)).unwrap_or(false);

    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-nostdin", "-v", "error", "-y"]);

    // Input 0: H3 rendered shots
    cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(&h3_list_path);

    // Input 1: Original reference shots (if available)
    if orig_has_audio {
        cmd.args(["-f", "concat", "-safe", "0", "-i"]).arg(&orig_list_path);
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
    let work_dir = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
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
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
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
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
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
}
