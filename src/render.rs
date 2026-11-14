use anyhow::{bail, Context, Result};
use base64::Engine;
use reqwest::blocking::multipart::{Form, Part};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::config::{ProfileSettings, ProjectConfig};
use crate::pool::{self, EngineMode, PoolEndpoint};
use crate::sync_prompt::compile_prompt_file;
use crate::timeline::{ShotMeta, TimelineManifest};

#[allow(dead_code)]
pub struct RenderOptions<'a> {
    pub proj_dir: &'a Path,
    pub shot: Option<String>,
    pub range: Option<String>,
    pub smoke: bool,
    pub force: bool,
    pub force_video_only: bool,
    pub mock_h3: bool,
}

#[derive(Debug, Clone)]
struct RenderShot {
    shot_id: String,
    start_sec: f64,
    end_sec: f64,
    duration: f64,
    anchor: String,
    seed: u64,
    characters: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct RenderBehavior {
    force: bool,
    compile_prompt: bool,
    preserve_prompt: bool,
}

pub fn run_render(opts: RenderOptions) -> Result<()> {
    let profile_name = if opts.smoke { "smoke" } else { "prod" };
    let config = load_project_config(opts.proj_dir)?;
    let profile = if opts.smoke {
        config.profiles.smoke.clone()
    } else {
        config.profiles.prod.clone()
    };

    let shots = load_render_shots(opts.proj_dir)?;
    let selected = select_shots(shots, opts.shot.as_deref(), opts.range.as_deref())?;
    if selected.is_empty() {
        println!("==> No shots matched the requested filter");
        return Ok(());
    }

    let mock_h3 = opts.mock_h3 || mock_h3_from_environment();
    let behavior = RenderBehavior {
        force: opts.force || opts.force_video_only,
        compile_prompt: opts.force,
        preserve_prompt: opts.force_video_only && !opts.force,
    };
    println!(
        "==> Rendering {} shot(s) with {} profile (mock_h3={})",
        selected.len(),
        profile_name,
        mock_h3
    );

    // The FL2VA partition owns both fl2va and t2va. Ref2VA requests are kept
    // in a separate, final group so an engine switch can happen only once.
    for (engine, group) in group_shots_by_engine(selected) {
        if group.is_empty() {
            continue;
        }

        let endpoints = if mock_h3 {
            Vec::new()
        } else {
            resolve_endpoints(&config, engine)?
        };

        if !endpoints.is_empty() {
            let urls = endpoints
                .iter()
                .map(PoolEndpoint::base_url)
                .collect::<Vec<_>>()
                .join(", ");
            println!("==> Engine group {} -> {}", engine.as_str(), urls);
        }

        if mock_h3 || endpoints.len() <= 1 {
            let endpoint = endpoints.first();
            for shot in group {
                render_shot(
                    opts.proj_dir,
                    profile_name,
                    &profile,
                    &shot,
                    endpoint,
                    mock_h3,
                    behavior,
                )?;
            }
        } else {
            // Parallel multi-node dispatch: spawn exactly 1 worker thread per
            // DGX Spark endpoint so each GB10 node stays single-tenant (no OOM)
            // while all available nodes render simultaneously in parallel.
            let queue = std::sync::Mutex::new(std::collections::VecDeque::from(group));
            let first_err: std::sync::Mutex<Option<anyhow::Error>> = std::sync::Mutex::new(None);
            std::thread::scope(|s| {
                for endpoint in &endpoints {
                    let queue_ref = &queue;
                    let err_ref = &first_err;
                    let proj_dir = opts.proj_dir;
                    let profile_ref = &profile;
                    s.spawn(move || loop {
                        if err_ref.lock().unwrap().is_some() {
                            break;
                        }
                        let next_shot = {
                            let mut q = queue_ref.lock().unwrap();
                            q.pop_front()
                        };
                        let Some(shot) = next_shot else {
                            break;
                        };
                        println!(
                            "==> [Worker {}:{}] Rendering {}...",
                            endpoint.host, endpoint.port, shot.shot_id
                        );
                        if let Err(e) = render_shot(
                            proj_dir,
                            profile_name,
                            profile_ref,
                            &shot,
                            Some(endpoint),
                            mock_h3,
                            behavior,
                        ) {
                            let mut slot = err_ref.lock().unwrap();
                            if slot.is_none() {
                                *slot = Some(e);
                            }
                            break;
                        }
                    });
                }
            });
            if let Some(err) = first_err.into_inner().unwrap() {
                return Err(err);
            }
        }
    }

    println!("==> Render complete (profile: {})", profile_name);
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

fn load_render_shots(proj_dir: &Path) -> Result<Vec<RenderShot>> {
    let timeline_path = proj_dir.join("timeline.json");
    if timeline_path.is_file() {
        let timeline = TimelineManifest::load_from_file(&timeline_path)
            .with_context(|| format!("Failed to load {}", timeline_path.display()))?;
        let mut shots = Vec::new();
        for item in timeline.shots {
            let shot_dir = proj_dir.join("shots").join(&item.shot_id);
            let meta_path = shot_dir.join("shot.toml");
            let meta = ShotMeta::load_from_file(&meta_path).ok();
            shots.push(RenderShot {
                shot_id: item.shot_id.clone(),
                start_sec: item.start_sec,
                end_sec: item.end_sec,
                duration: item.duration,
                anchor: meta
                    .as_ref()
                    .map(|meta| meta.anchor.clone())
                    .unwrap_or(item.anchor),
                seed: meta.as_ref().map(|meta| meta.seed).unwrap_or(42),
                characters: meta
                    .as_ref()
                    .map(|meta| meta.characters.clone())
                    .unwrap_or_default(),
            });
        }
        return Ok(shots);
    }

    // A timeline is preferred, but a project assembled by hand can still be
    // rendered from shot.toml files.
    let shots_dir = proj_dir.join("shots");
    let mut shots = Vec::new();
    let entries = fs::read_dir(&shots_dir)
        .with_context(|| format!("Failed to read {}", shots_dir.display()))?;
    for entry in entries.flatten() {
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let meta_path = entry.path().join("shot.toml");
        if !meta_path.is_file() {
            continue;
        }
        let meta = ShotMeta::load_from_file(&meta_path)?;
        shots.push(RenderShot {
            shot_id: meta.shot_id,
            start_sec: meta.start_time,
            end_sec: meta.end_time,
            duration: meta.duration,
            anchor: meta.anchor,
            seed: meta.seed,
            characters: meta.characters,
        });
    }
    shots.sort_by_key(|shot| shot_number(&shot.shot_id));
    Ok(shots)
}

fn select_shots(
    shots: Vec<RenderShot>,
    shot_selector: Option<&str>,
    time_range: Option<&str>,
) -> Result<Vec<RenderShot>> {
    let total_duration = shots
        .iter()
        .map(|shot| shot.end_sec)
        .fold(0.0_f64, f64::max);
    let requested_range = match time_range {
        Some(value) => Some(crate::decompile::scene::parse_time_range(
            value,
            total_duration,
        )?),
        None => None,
    };

    let mut selected = shots
        .into_iter()
        .filter(|shot| {
            let shot_ok = shot_selector
                .map(|selector| selector_matches(selector, &shot.shot_id))
                .unwrap_or(true);
            let range_ok = requested_range
                .map(|(start, end)| shot.start_sec < end && shot.end_sec > start)
                .unwrap_or(true);
            shot_ok && range_ok
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|shot| shot_number(&shot.shot_id));

    if let Some(selector) = shot_selector {
        validate_shot_selector(selector)?;
    }
    Ok(selected)
}

fn validate_shot_selector(selector: &str) -> Result<()> {
    let value = selector.trim().trim_start_matches("shot_");
    if let Some((start, end)) = value.split_once('-') {
        let start = start
            .parse::<u32>()
            .with_context(|| format!("Invalid shot range start: {start}"))?;
        let end = end
            .parse::<u32>()
            .with_context(|| format!("Invalid shot range end: {end}"))?;
        if start == 0 || end == 0 || start > end {
            bail!("Invalid shot range: {selector}");
        }
    } else {
        let number = value
            .parse::<u32>()
            .with_context(|| format!("Invalid shot selector: {selector}"))?;
        if number == 0 {
            bail!("Invalid shot selector: {selector}");
        }
    }
    Ok(())
}

fn selector_matches(selector: &str, shot_id: &str) -> bool {
    let number = shot_number(shot_id);
    let value = selector.trim().trim_start_matches("shot_");
    if let Some((start, end)) = value.split_once('-') {
        match (start.parse::<u32>(), end.parse::<u32>()) {
            (Ok(start), Ok(end)) => number >= start && number <= end,
            _ => false,
        }
    } else {
        value.parse::<u32>().ok() == Some(number)
    }
}

fn shot_number(shot_id: &str) -> u32 {
    shot_id
        .rsplit('_')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

fn group_shots_by_engine(shots: Vec<RenderShot>) -> Vec<(EngineMode, Vec<RenderShot>)> {
    let mut fl2va = Vec::new();
    let mut ref2va = Vec::new();
    for shot in shots {
        if shot.anchor == "ref_only" {
            ref2va.push(shot);
        } else {
            fl2va.push(shot);
        }
    }
    vec![(EngineMode::Fl2va, fl2va), (EngineMode::Ref2va, ref2va)]
}

fn resolve_endpoints(config: &ProjectConfig, engine: EngineMode) -> Result<Vec<PoolEndpoint>> {
    if let Ok(value) = std::env::var("REA_MOVIE_H3_ENDPOINT") {
        if !value.trim().is_empty() {
            let url = reqwest::Url::parse(value.trim())
                .with_context(|| format!("Invalid REA_MOVIE_H3_ENDPOINT: {value}"))?;
            let host = url
                .host_str()
                .with_context(|| "REA_MOVIE_H3_ENDPOINT must include a host")?
                .to_string();
            let port = url.port().unwrap_or(80);
            let configured_engine = std::env::var("REA_MOVIE_H3_ENGINE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|value| EngineMode::parse(&value))
                .transpose()?;
            let endpoint = PoolEndpoint {
                host,
                port,
                engine: configured_engine,
            };
            if !endpoint.supports(engine) {
                bail!(
                    "REA_MOVIE_H3_ENDPOINT is configured for {}, not {}",
                    endpoint
                        .engine
                        .map(|mode| mode.as_str())
                        .unwrap_or("unknown"),
                    engine.as_str()
                );
            }
            return Ok(vec![endpoint]);
        }
    }

    let endpoints = pool::discover_endpoints(&config.pool);
    let matching = endpoints
        .iter()
        .filter(|endpoint| endpoint.supports(engine))
        .cloned()
        .collect::<Vec<_>>();
    if !matching.is_empty() {
        return Ok(matching);
    }

    // If a stateful node currently hosts the other partition, request exactly
    // one switch and wait for the launcher to publish its new dynamic port.
    if let Some(endpoint) = endpoints.first() {
        pool::request_engine_switch(
            &endpoint.host,
            &config.pool.remote_workspace,
            config.pool.base_port,
            engine,
        )
        .context("Failed to request H3 engine switch")?;
        let wait_seconds = std::env::var("REA_MOVIE_ENGINE_SWITCH_WAIT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(120);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(wait_seconds) {
            std::thread::sleep(Duration::from_secs(2));
            let updated = pool::discover_endpoints(&config.pool)
                .into_iter()
                .filter(|candidate| candidate.supports(engine))
                .collect::<Vec<_>>();
            if !updated.is_empty() {
                return Ok(updated);
            }
        }
        bail!(
            "H3 engine switch to {} was requested, but no matching endpoint became ready",
            engine.as_str()
        );
    }

    bail!(
        "No H3 endpoint is available for engine {}; run `rea-movie pool generate-scripts` first",
        engine.as_str()
    );
}

fn render_shot(
    proj_dir: &Path,
    profile_name: &str,
    profile: &ProfileSettings,
    shot: &RenderShot,
    endpoint: Option<&PoolEndpoint>,
    mock_h3: bool,
    behavior: RenderBehavior,
) -> Result<()> {
    let shot_dir = proj_dir.join("shots").join(&shot.shot_id);
    let target = shot_dir
        .join("renders")
        .join(profile_name)
        .join(format!("{}.mp4", shot.shot_id));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    if !behavior.force && valid_nonempty_file(&target) {
        println!("==> Skipping {} (target already exists)", shot.shot_id);
        return Ok(());
    }

    let h3_path = shot_dir.join("prompt.h3.txt");
    if behavior.compile_prompt {
        let meta = shot_meta(proj_dir, shot)?;
        compile_prompt_file(&shot_dir, &meta)
            .with_context(|| format!("Failed to compile prompt for {}", shot.shot_id))?;
        println!(
            "==> Compiled prompt.ko.md -> prompt.h3.txt for {}",
            shot.shot_id
        );
    } else if behavior.preserve_prompt {
        if !h3_path.is_file() {
            bail!(
                "Cannot use --force-video-only for {}: prompt.h3.txt does not exist",
                shot.shot_id
            );
        }
        println!("==> Preserving existing prompt.h3.txt for {}", shot.shot_id);
    } else if !h3_path.is_file() {
        let meta = shot_meta(proj_dir, shot)?;
        compile_prompt_file(&shot_dir, &meta)?;
    }

    if behavior.force {
        if let Some(backup) = backup_to_history(&target)? {
            println!("==> Backed up {} to {}", shot.shot_id, backup.display());
        }
    }

    let prompt = fs::read_to_string(&h3_path)
        .with_context(|| format!("Failed to read {}", h3_path.display()))?;
    if mock_h3 {
        synthesize_mock_video(&target, profile, shot.duration)?;
    } else {
        let endpoint = endpoint.context("H3 endpoint is required for a real render")?;
        let bytes = request_h3(endpoint, proj_dir, shot, profile, &prompt)?;
        write_bytes_atomic(&target, &bytes)?;
    }

    if !valid_nonempty_file(&target) {
        bail!(
            "Renderer did not create a valid non-empty target: {}",
            target.display()
        );
    }
    println!("==> Rendered {}", target.display());
    Ok(())
}

fn shot_meta(proj_dir: &Path, shot: &RenderShot) -> Result<ShotMeta> {
    let path = proj_dir.join("shots").join(&shot.shot_id).join("shot.toml");
    ShotMeta::load_from_file(&path).with_context(|| format!("Failed to load {}", path.display()))
}

fn valid_nonempty_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn backup_to_history(target: &Path) -> Result<Option<PathBuf>> {
    if !target.exists() {
        return Ok(None);
    }
    let history_dir = target
        .parent()
        .context("Render target has no parent directory")?
        .join("history");
    fs::create_dir_all(&history_dir)?;

    let mut take = 1_u32;
    let backup = loop {
        let candidate = history_dir.join(format!("take_{take:03}.mp4"));
        if !candidate.exists() {
            break candidate;
        }
        take += 1;
        if take > 99_999 {
            bail!("History take limit exceeded in {}", history_dir.display());
        }
    };
    fs::copy(target, &backup).with_context(|| {
        format!(
            "Failed to back up {} to {}",
            target.display(),
            backup.display()
        )
    })?;
    Ok(Some(backup))
}

fn synthesize_mock_video(target: &Path, profile: &ProfileSettings, duration: f64) -> Result<()> {
    let duration = duration.max(0.1);
    let temp = target.with_file_name(format!(
        ".{}.{}.mock.mp4",
        target
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("render"),
        std::process::id()
    ));
    if temp.exists() {
        fs::remove_file(&temp)?;
    }

    let video_source = format!(
        "color=c=0x142033:s={}x{}:r={}:d={:.3}",
        profile.width, profile.height, profile.fps, duration
    );
    let audio_source = format!("sine=frequency=440:sample_rate=32000:duration={duration:.3}");
    let output = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &video_source,
            "-f",
            "lavfi",
            "-i",
            &audio_source,
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-t",
            &format!("{duration:.3}"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-r",
            &profile.fps.to_string(),
            "-c:a",
            "aac",
            "-ar",
            "32000",
            "-ac",
            "2",
            "-movflags",
            "+faststart",
        ])
        .arg(&temp)
        .output()
        .context("Failed to run ffmpeg for mock H3 synthesis")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Mock H3 ffmpeg synthesis failed: {}", stderr.trim());
    }
    if !valid_nonempty_file(&temp) {
        bail!("Mock H3 synthesis produced no output");
    }
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(&temp, target)?;
    Ok(())
}

fn request_h3(
    endpoint: &PoolEndpoint,
    proj_dir: &Path,
    shot: &RenderShot,
    profile: &ProfileSettings,
    prompt: &str,
) -> Result<Vec<u8>> {
    let task = match shot.anchor.as_str() {
        "first_and_last" | "first_only" => "fl2va",
        "ref_only" => "ref2va",
        "none" => "t2va",
        other => bail!("Unsupported shot anchor: {other}"),
    };

    let h3_duration = shot.duration.clamp(4.0, 15.0);
    let mut extra_params = json!({
        "task": task,
        "duration": h3_duration,
        "audio_flow_shift": profile.audio_flow_shift,
    });
    if shot.anchor == "first_and_last" {
        extra_params["frame_indices"] = json!([0, -1]);
    } else if shot.anchor == "first_only" {
        extra_params["frame_indices"] = json!([0]);
    }

    let mut form = Form::new()
        .text("prompt", prompt.to_string())
        .text("width", profile.width.to_string())
        .text("height", profile.height.to_string())
        .text("fps", profile.fps.to_string())
        .text("num_inference_steps", profile.steps.to_string())
        .text("flow_shift", profile.flow_shift.to_string())
        .text("seed", shot.seed.to_string())
        .text("extra_params", serde_json::to_string(&extra_params)?);

    match shot.anchor.as_str() {
        "first_and_last" => {
            let refs = shot_dir(proj_dir, &shot.shot_id).join("refs");
            form = form
                .part(
                    "input_references",
                    image_part(&refs.join("first_frame.jpg"), "first_frame.jpg")?,
                )
                .part(
                    "input_references",
                    image_part(&refs.join("last_frame.jpg"), "last_frame.jpg")?,
                );
        }
        "first_only" => {
            let refs = shot_dir(proj_dir, &shot.shot_id).join("refs");
            form = form.part(
                "input_reference",
                image_part(&refs.join("first_frame.jpg"), "first_frame.jpg")?,
            );
        }
        "ref_only" => {
            let (face, voice) = character_reference_paths(proj_dir, &shot.characters)?;
            form = form
                .part("input_references", image_part(&face, "face.jpg")?)
                .part("input_references", audio_part(&voice, "voice.wav")?);
        }
        "none" => {
            form = form.text("aspect_ratio", "16:9");
        }
        other => bail!("Unsupported shot anchor: {other}"),
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(14_400))
        .build()?;
    let url = format!(
        "{}/v1/videos/sync",
        endpoint.base_url().trim_end_matches('/')
    );
    let response = client
        .post(&url)
        .multipart(form)
        .send()
        .with_context(|| format!("H3 request failed: {url}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        bail!("H3 server returned {status}: {}", body.trim());
    }
    let bytes = response.bytes()?.to_vec();
    decode_h3_response(bytes)
}

fn image_part(path: &Path, file_name: &str) -> Result<Part> {
    let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(Part::bytes(bytes)
        .file_name(file_name.to_string())
        .mime_str("image/jpeg")?)
}

fn audio_part(path: &Path, file_name: &str) -> Result<Part> {
    let bytes = fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(Part::bytes(bytes)
        .file_name(file_name.to_string())
        .mime_str("audio/wav")?)
}

fn character_reference_paths(proj_dir: &Path, characters: &[String]) -> Result<(PathBuf, PathBuf)> {
    let root = proj_dir.join("assets").join("characters");
    let fallback = "S1".to_string();
    let requested = if characters.is_empty() {
        vec![&fallback]
    } else {
        characters.iter().collect::<Vec<_>>()
    };

    for character in requested {
        let direct = root.join(character);
        let matching = fs::read_dir(&root)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name == character || name.starts_with(&format!("{character}_")))
                    .unwrap_or(false)
            })
            .map(|entry| entry.path())
            .unwrap_or(direct);

        let face = matching.join("face.jpg");
        let voice = matching.join("voice.wav");
        if face.is_file() && voice.is_file() {
            return Ok((face, voice));
        }
    }
    bail!(
        "No character face.jpg/voice.wav pair found in {}",
        root.display()
    )
}

fn decode_h3_response(bytes: Vec<u8>) -> Result<Vec<u8>> {
    let trimmed = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|index| &bytes[index..])
        .unwrap_or(&bytes);
    if !trimmed.starts_with(b"{") {
        if bytes.is_empty() {
            bail!("H3 server returned an empty response");
        }
        return Ok(bytes);
    }

    let value: serde_json::Value =
        serde_json::from_slice(trimmed).context("H3 server returned invalid JSON")?;
    if let Some(data) = value.get("data").and_then(|value| value.as_str()) {
        if let Some(encoded) = data.strip_prefix("data:video/mp4;base64,") {
            return base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .context("H3 server returned invalid base64 video data");
        }
        return base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("H3 server returned invalid base64 video data");
    }

    let url = value
        .get("video_url")
        .and_then(|value| value.as_str())
        .or_else(|| value.get("url").and_then(|value| value.as_str()))
        .context("H3 JSON response does not contain video data or a URL")?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(14_400))
        .build()?;
    let response = client.get(url).send()?.error_for_status()?;
    Ok(response.bytes()?.to_vec())
}

fn write_bytes_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let temp = target.with_file_name(format!(
        ".{}.{}.response.mp4",
        target
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("render"),
        std::process::id()
    ));
    fs::write(&temp, bytes)
        .with_context(|| format!("Failed to write temporary render {}", temp.display()))?;
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(&temp, target)?;
    Ok(())
}

fn shot_dir(proj_dir: &Path, shot_id: &str) -> PathBuf {
    proj_dir.join("shots").join(shot_id)
}

fn mock_h3_from_environment() -> bool {
    std::env::var("REA_MOVIE_MOCK_H3")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(id: &str, start: f64, end: f64, anchor: &str) -> RenderShot {
        RenderShot {
            shot_id: id.to_string(),
            start_sec: start,
            end_sec: end,
            duration: end - start,
            anchor: anchor.to_string(),
            seed: 42,
            characters: vec!["S1".to_string()],
        }
    }

    #[test]
    fn shot_filter_supports_single_range_and_time_overlap() {
        let shots = vec![
            shot("shot_0001", 0.0, 4.0, "first_and_last"),
            shot("shot_0002", 4.0, 8.0, "first_only"),
            shot("shot_0003", 8.0, 12.0, "ref_only"),
        ];
        let selected = select_shots(shots.clone(), Some("0002"), None).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].shot_id, "shot_0002");

        let selected = select_shots(shots.clone(), Some("0001-0002"), None).unwrap();
        assert_eq!(selected.len(), 2);

        let selected = select_shots(shots, None, Some("00:00:03-00:00:09")).unwrap();
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].shot_id, "shot_0001");
        assert_eq!(selected[1].shot_id, "shot_0002");
        assert_eq!(selected[2].shot_id, "shot_0003");
    }

    #[test]
    fn engine_grouping_keeps_ref2va_last() {
        let shots = vec![
            shot("shot_0001", 0.0, 1.0, "first_and_last"),
            shot("shot_0002", 1.0, 2.0, "ref_only"),
            shot("shot_0003", 2.0, 3.0, "none"),
        ];
        let groups = group_shots_by_engine(shots);
        assert_eq!(groups[0].0, EngineMode::Fl2va);
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, EngineMode::Ref2va);
        assert_eq!(groups[1].1.len(), 1);
    }

    #[test]
    fn history_backup_is_sequential_and_preserves_old_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("shot_0001.mp4");
        fs::write(&target, b"old").unwrap();
        let first = backup_to_history(&target).unwrap().unwrap();
        assert_eq!(first.file_name().unwrap(), "take_001.mp4");
        assert_eq!(fs::read(&first).unwrap(), b"old");

        fs::write(&target, b"new").unwrap();
        let second = backup_to_history(&target).unwrap().unwrap();
        assert_eq!(second.file_name().unwrap(), "take_002.mp4");
        assert_eq!(fs::read(&second).unwrap(), b"new");
    }
}
