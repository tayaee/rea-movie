//! Issue 8: `rea-movie series-watch` — lightweight whole-series pre-analysis.
//!
//! "Watching" the whole series is not a full decompile. For every episode it
//! runs ffprobe, samples a keyframe every 5 seconds and loads the subtitles —
//! all cached under `<root>/sources/<source_id>/` — and writes the series
//! context that `decompile` expects:
//!
//! - `<root>/story/world.json`
//! - `<root>/story/characters.json`
//! - `<root>/story/timeline-days.md`
//!
//! Online with `MINIMAX_API_KEY`: one MiniMax-M3 call summarises the series
//! (sample frames + subtitles). Offline or on failure: deterministic JSON/MD.
//! Existing files are kept unless `--force`.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::decompile::story::{load_subtitle_cues_from, SubtitleCue};
use crate::sources::{self, SourceCache};

pub struct SeriesWatchOptions<'a> {
    pub inputs: &'a [String],
    pub source_dir: Option<&'a Path>,
    pub out_root: Option<&'a Path>,
    pub offline: bool,
    pub force: bool,
    pub interval: f64,
}

#[derive(Debug, Default)]
pub struct SeriesWatchReport {
    pub story_dir: PathBuf,
    pub episodes: usize,
    pub mode: String,
    pub written: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

struct Episode {
    number: usize,
    cache: SourceCache,
    probe: sources::ProbeInfo,
    keyframes: Vec<PathBuf>,
    subtitle: Option<PathBuf>,
    cues: Vec<SubtitleCue>,
    palette: Option<Palette>,
    avg_shot_len: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct Palette {
    rgb: [f64; 3],
    luma: f64,
}

impl Palette {
    fn tone(&self) -> &'static str {
        if self.luma < 70.0 {
            "dark / low-key"
        } else if self.luma > 160.0 {
            "bright / high-key"
        } else {
            "neutral / mid-key"
        }
    }

    fn temperature(&self) -> &'static str {
        let diff = self.rgb[0] - self.rgb[2];
        if diff > 12.0 {
            "warm"
        } else if diff < -12.0 {
            "cool"
        } else {
            "neutral"
        }
    }

    fn to_json(self) -> Value {
        json!({
            "mean_rgb": [self.rgb[0].round(), self.rgb[1].round(), self.rgb[2].round()],
            "mean_luma": (self.luma * 10.0).round() / 10.0,
            "tone": self.tone(),
            "temperature": self.temperature(),
        })
    }
}

fn log(msg: &str) {
    println!("{} [series-watch] {}", crate::log::stamp(), msg);
}

/// Natural sort key so `EP2` sorts before `EP10`.
fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut num = String::new();
    let mut text = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !text.is_empty() {
                key.push((1, 0, std::mem::take(&mut text).to_lowercase()));
            }
            num.push(ch);
        } else {
            if !num.is_empty() {
                key.push((
                    0,
                    std::mem::take(&mut num).parse().unwrap_or(u64::MAX),
                    String::new(),
                ));
            }
            text.push(ch);
        }
    }
    if !num.is_empty() {
        key.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !text.is_empty() {
        key.push((1, 0, text.to_lowercase()));
    }
    key
}

fn scan_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if depth > 0 && !matches!(name.as_str(), "shots_cache" | "keyframes" | "subtitles") {
                scan_dir(&path, depth - 1, out);
            }
        } else if sources::is_video_file(&path) && path.exists() {
            out.push(path);
        }
    }
}

/// Resolve `<root>`: explicit `--out-root`, else the parent of a
/// `sources/`/`source/` dir, else the root of the first input.
fn resolve_root(opts: &SeriesWatchOptions) -> Result<PathBuf> {
    if let Some(root) = opts.out_root {
        return Ok(root.to_path_buf());
    }
    if let Some(dir) = opts.source_dir {
        let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
        let name = abs.file_name().and_then(|n| n.to_str());
        if matches!(
            name,
            Some(sources::SOURCES_DIR_NAME) | Some(sources::LEGACY_SOURCE_DIR_NAME)
        ) {
            if let Some(parent) = abs.parent() {
                return Ok(parent.to_path_buf());
            }
        }
        if let Some(root) = sources::root_from_input(&abs.join("x")) {
            return Ok(root);
        }
    }
    for input in opts.inputs {
        if let Some(root) = sources::root_from_input(Path::new(input)) {
            return Ok(root);
        }
    }
    anyhow::bail!("Cannot determine series root; pass --out-root <root>")
}

/// Collect episode videos in playback order (deduplicated by real file).
fn collect_videos(opts: &SeriesWatchOptions, root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut targets: Vec<PathBuf> = opts.inputs.iter().map(PathBuf::from).collect();
    if let Some(dir) = opts.source_dir {
        targets.push(dir.to_path_buf());
    }
    if targets.is_empty() {
        targets.push(root.join(sources::SOURCES_DIR_NAME));
        targets.push(root.join(sources::LEGACY_SOURCE_DIR_NAME));
    }
    for target in targets {
        if target.is_dir() {
            scan_dir(&target, 2, &mut found);
        } else if target.is_file() {
            found.push(target);
        } else {
            eprintln!(
                "Warning: series-watch input not found: {}",
                target.display()
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut videos: Vec<(PathBuf, PathBuf)> = found
        .into_iter()
        .filter_map(|p| {
            let real = std::fs::canonicalize(&p).ok()?;
            seen.insert(real.clone()).then_some((p, real))
        })
        .collect();
    videos.sort_by(|a, b| {
        let na = a.1.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let nb = b.1.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        natural_key(na).cmp(&natural_key(nb))
    });
    videos.into_iter().map(|(p, _)| p).collect()
}

fn average_palette(frames: &[PathBuf]) -> Option<Palette> {
    if frames.is_empty() {
        return None;
    }
    let step = (frames.len() / 24).max(1);
    let (mut sum, mut n) = ([0.0f64; 3], 0u64);
    for frame in frames.iter().step_by(step) {
        let Ok(img) = image::open(frame) else {
            continue;
        };
        let rgb = img.to_rgb8();
        for px in rgb.pixels().step_by(7) {
            sum[0] += px[0] as f64;
            sum[1] += px[1] as f64;
            sum[2] += px[2] as f64;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let rgb = [sum[0] / n as f64, sum[1] / n as f64, sum[2] / n as f64];
    let luma = 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2];
    Some(Palette { rgb, luma })
}

/// Speaker labels from subtitle conventions: `NAME: line`, `[S1] line`,
/// `(이름) line`. Sound tags like `[Music]` are ignored.
pub fn speaker_tag(text: &str) -> Option<String> {
    let re =
        regex::Regex::new(r"^(?:\[([^\]]{1,24})\]|\(([^)]{1,16})\)|([A-Z][A-Z0-9 .'\-]{1,23}):)")
            .ok()?;
    let cap = re.captures(text.trim())?;
    let raw = cap
        .get(1)
        .or_else(|| cap.get(2))
        .or_else(|| cap.get(3))?
        .as_str()
        .trim()
        .to_string();
    const NOISE: &[&str] = &[
        "music",
        "applause",
        "laughter",
        "laughs",
        "silence",
        "noise",
        "음악",
        "박수",
        "웃음",
        "효과음",
        "♪",
    ];
    let lower = raw.to_lowercase();
    if raw.is_empty() || NOISE.iter().any(|n| lower.contains(n)) {
        return None;
    }
    Some(raw)
}

fn fmt_hms(sec: f64) -> String {
    let s = sec.max(0.0).round() as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn md_cell(text: &str, max: usize) -> String {
    let clean: String = text.replace('|', "/").replace('\n', " ");
    let mut out: String = clean.chars().take(max).collect();
    if clean.chars().count() > max {
        out.push('…');
    }
    out
}

fn deterministic_world(root: &Path, episodes: &[Episode], interval: f64) -> Value {
    let total: f64 = episodes.iter().map(|e| e.probe.duration).sum();
    let palettes: Vec<Palette> = episodes.iter().filter_map(|e| e.palette).collect();
    let series_palette = if palettes.is_empty() {
        None
    } else {
        let n = palettes.len() as f64;
        let rgb = [0, 1, 2].map(|i| palettes.iter().map(|p| p.rgb[i]).sum::<f64>() / n);
        let luma = palettes.iter().map(|p| p.luma).sum::<f64>() / n;
        Some(Palette { rgb, luma })
    };
    let first = episodes.first().map(|e| &e.probe);
    let aspect = first
        .filter(|p| p.height > 0)
        .map(|p| format!("{:.2}:1", p.width as f64 / p.height as f64));
    let shot_lens: Vec<f64> = episodes.iter().filter_map(|e| e.avg_shot_len).collect();
    let avg_shot = (!shot_lens.is_empty())
        .then(|| (shot_lens.iter().sum::<f64>() / shot_lens.len() as f64 * 100.0).round() / 100.0);
    let eps: Vec<Value> = episodes
        .iter()
        .map(|e| {
            json!({
                "episode": e.number,
                "source_id": e.cache.source_id,
                "title": e.cache.video.file_stem().and_then(|s| s.to_str()).unwrap_or_default(),
                "video": e.cache.video_str(),
                "duration_sec": (e.probe.duration * 1000.0).round() / 1000.0,
                "fps": (e.probe.fps * 1000.0).round() / 1000.0,
                "width": e.probe.width,
                "height": e.probe.height,
                "subtitle": e.subtitle.as_ref().map(|p| p.to_string_lossy().to_string()),
                "cue_count": e.cues.len(),
                "keyframes_dir": e.cache.keyframes_dir().to_string_lossy(),
                "keyframe_count": e.keyframes.len(),
                "palette": e.palette.map(Palette::to_json),
                "avg_shot_length_sec": e.avg_shot_len.map(|v| (v * 100.0).round() / 100.0),
                "first_line": e.cues.first().map(|c| c.text.clone()),
                "last_line": e.cues.last().map(|c| c.text.clone()),
            })
        })
        .collect();
    json!({
        "version": 1,
        "generated_by": "rea-movie series-watch",
        "generated_at": chrono::Local::now().to_rfc3339(),
        "mode": "offline",
        "status": "draft — human confirmation required",
        "series_root": root.to_string_lossy(),
        "episode_count": episodes.len(),
        "total_duration_sec": (total * 1000.0).round() / 1000.0,
        "probe_interval_sec": interval,
        "era": Value::Null,
        "season": Value::Null,
        "weather": Value::Null,
        "locations": [],
        "color_grade": series_palette.map(|p| format!("{} {}", p.temperature(), p.tone())),
        "palette": series_palette.map(Palette::to_json),
        "cinematography": {
            "resolution": first.map(|p| format!("{}x{}", p.width, p.height)),
            "aspect_ratio": aspect,
            "fps": first.map(|p| (p.fps * 1000.0).round() / 1000.0),
            "avg_shot_length_sec": avg_shot,
            "handheld_ratio": Value::Null,
        },
        "episodes": eps,
    })
}

fn deterministic_characters(episodes: &[Episode]) -> Value {
    // speaker -> (line count, episodes)
    let mut speakers: BTreeMap<String, (usize, Vec<usize>)> = BTreeMap::new();
    for ep in episodes {
        for cue in &ep.cues {
            if let Some(tag) = speaker_tag(&cue.text) {
                let entry = speakers.entry(tag).or_default();
                entry.0 += 1;
                if !entry.1.contains(&ep.number) {
                    entry.1.push(ep.number);
                }
            }
        }
    }
    let mut ranked: Vec<(String, (usize, Vec<usize>))> = speakers.into_iter().collect();
    ranked.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    let characters: Vec<Value> = ranked
        .iter()
        .enumerate()
        .map(|(i, (name, (lines, eps)))| {
            json!({
                "id": format!("P{}", i + 1),
                "name": name,
                "source": "subtitle_speaker_tag",
                "line_count": lines,
                "episodes": eps,
                "appearance": "",
                "wardrobe_by_episode": {},
                "relationships": [],
                "speech_style": "",
            })
        })
        .collect();
    json!({
        "version": 1,
        "generated_by": "rea-movie series-watch",
        "generated_at": chrono::Local::now().to_rfc3339(),
        "mode": "offline",
        "status": "draft — human confirmation required",
        "episode_count": episodes.len(),
        "characters": characters,
        "notes": "Global character registry. Scene bibles reference these IDs (S1@scene = P3). Offline mode only derives speakers from subtitle tags; fill appearance/wardrobe/relationships by review or rerun online.",
    })
}

fn deterministic_timeline_md(episodes: &[Episode], mode: &str) -> String {
    let mut md = String::new();
    md.push_str("# Series Timeline (Days) — draft\n\n");
    md.push_str(&format!(
        "> Generated by `rea-movie series-watch` (mode: {mode}). Day mapping must be confirmed by a human.\n\n"
    ));
    md.push_str("| EP | Title | Duration | Source ID | Day | First line | Last line |\n");
    md.push_str("|---|---|---|---|---|---|---|\n");
    for e in episodes {
        let title = e
            .cache
            .video
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        md.push_str(&format!(
            "| {:02} | {} | {} | {} | TBD | {} | {} |\n",
            e.number,
            md_cell(title, 48),
            fmt_hms(e.probe.duration),
            e.cache.source_id,
            md_cell(e.cues.first().map(|c| c.text.as_str()).unwrap_or("-"), 40),
            md_cell(e.cues.last().map(|c| c.text.as_str()).unwrap_or("-"), 40),
        ));
    }
    md.push_str("\n## Episode continuity\n\n");
    if episodes.len() < 2 {
        md.push_str("- Single episode: no cross-episode continuity to map.\n");
    }
    for pair in episodes.windows(2) {
        md.push_str(&format!(
            "- EP{:02} → EP{:02}: TBD (e.g. night end → morning start; confirm wardrobe/props carry-over)\n",
            pair[0].number, pair[1].number
        ));
    }
    md
}

fn subtitle_excerpt(cues: &[SubtitleCue], max_chars: usize) -> String {
    let full: Vec<String> = cues
        .iter()
        .map(|c| format!("[{}] {}", fmt_hms(c.start_sec), c.text))
        .collect();
    let total: usize = full.iter().map(|l| l.len() + 1).sum();
    if total <= max_chars {
        return full.join("\n");
    }
    // Evenly thin out lines to stay within budget while covering the episode.
    let keep = (full.len() * max_chars / total.max(1)).max(1);
    let step = full.len() as f64 / keep as f64;
    (0..keep)
        .map(|i| full[((i as f64) * step) as usize].clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// One MiniMax-M3 call for the whole series.
fn summarize_with_m3(episodes: &[Episode], api_key: &str) -> Result<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let system_prompt = "You are a TV series script supervisor and continuity director. From per-episode metadata, subtitles and sample frames of the WHOLE series, output valid JSON only (no markdown) with keys: world (object: era, season, weather, locations (array of {name, description, palette, lighting}), color_grade, cinematography), characters (array of {id, name, appearance, wardrobe_by_episode (object EP->string), relationships (array of strings, include honorific/banmal usage), speech_style, episodes (array of numbers)}), timeline_days_md (string: markdown mapping each EP to story day(s) and cross-episode continuity). Use stable global IDs P1, P2, ... for characters.";
    let budget = (40_000 / episodes.len().max(1)).clamp(1_500, 8_000);
    let mut text = String::from("Series episodes:\n");
    for e in episodes {
        text.push_str(&format!(
            "\n=== EP{:02} {} ({}, {}x{}) ===\n{}\n",
            e.number,
            e.cache
                .video
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default(),
            fmt_hms(e.probe.duration),
            e.probe.width,
            e.probe.height,
            subtitle_excerpt(&e.cues, budget)
        ));
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    let per_ep = (10 / episodes.len().max(1)).clamp(1, 3);
    let mut images = 0;
    'outer: for e in episodes {
        let n = e.keyframes.len();
        for k in 1..=per_ep {
            if images >= 10 {
                break 'outer;
            }
            let Some(frame) = e.keyframes.get(n * k / (per_ep + 1)) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(frame) else {
                continue;
            };
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
            content.push(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:image/jpeg;base64,{b64}")},
            }));
            images += 1;
        }
    }
    let body = json!({
        "model": "MiniMax-M3",
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": content},
        ]
    });
    let mut last_err = None;
    for attempt in 0..2u64 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
        let result: Result<Value> = (|| {
            let resp = client
                .post("https://api.minimax.io/v1/chat/completions")
                .header("Authorization", format!("Bearer {api_key}"))
                .json(&body)
                .send()?;
            if !resp.status().is_success() {
                anyhow::bail!(
                    "MiniMax series-watch API returned status: {}",
                    resp.status()
                );
            }
            let val: Value = resp.json()?;
            let raw = val["choices"][0]["message"]["content"]
                .as_str()
                .context("Missing content in series-watch response")?;
            let parsed: Value = crate::decompile::prompt::parse_llm_json(
                crate::decompile::prompt::strip_think_blocks(raw).trim(),
            )?;
            if !parsed.is_object() || parsed.get("world").is_none() {
                anyhow::bail!("series-watch response has no 'world' object");
            }
            Ok(parsed)
        })();
        match result {
            Ok(v) => return Ok(v),
            Err(e) => {
                eprintln!(
                    "Warning: series-watch M3 attempt {} failed: {:#}",
                    attempt + 1,
                    e
                );
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("series-watch M3 failed")))
}

fn has_m3_mode(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let Ok(data) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(val): Result<Value, _> = serde_json::from_str(&data) else {
        return false;
    };
    val.get("mode").and_then(|m| m.as_str()) == Some("m3")
}

fn write_if_needed(
    path: &Path,
    content: &str,
    force: bool,
    report: &mut SeriesWatchReport,
) -> Result<()> {
    if path.exists() && !force {
        log(&format!("skip (exists): {}", path.display()));
        report.skipped.push(path.to_path_buf());
        return Ok(());
    }
    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))?;
    log(&format!("wrote {}", path.display()));
    report.written.push(path.to_path_buf());
    Ok(())
}

pub fn run_series_watch(opts: SeriesWatchOptions) -> Result<SeriesWatchReport> {
    let root = resolve_root(&opts)?;
    std::fs::create_dir_all(&root)?;
    let story_dir = root.join("story");
    std::fs::create_dir_all(&story_dir)?;
    let world_path = story_dir.join("world.json");
    let chars_path = story_dir.join("characters.json");
    let days_path = story_dir.join("timeline-days.md");

    let mut report = SeriesWatchReport {
        story_dir: story_dir.clone(),
        ..Default::default()
    };
    if !opts.force && world_path.exists() && chars_path.exists() && days_path.exists() {
        log(&format!(
            "series context already present at {} (use --force to rebuild); skipping",
            story_dir.display()
        ));
        report.mode = "skipped".to_string();
        report.skipped = vec![world_path, chars_path, days_path];
        return Ok(report);
    }

    let videos = collect_videos(&opts, &root);
    if videos.is_empty() {
        anyhow::bail!(
            "No episode videos found (looked in inputs/--source-dir or {}/sources/)",
            root.display()
        );
    }
    let sources_dir = root.join(sources::SOURCES_DIR_NAME);
    let mut episodes: Vec<Episode> = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for video in videos {
        let cache = match SourceCache::open(&sources_dir, &video) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: skipping {} ({:#})", video.display(), e);
                continue;
            }
        };
        if !seen_ids.insert(cache.source_id.clone()) {
            continue;
        }
        let probe = match cache.probe(opts.force) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: ffprobe failed for {} ({:#})", video.display(), e);
                continue;
            }
        };
        let number = episodes.len() + 1;
        log(&format!(
            "EP{:02} {} ({:.1}s) -> {}",
            number,
            cache
                .video
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default(),
            probe.duration,
            cache.dir.display()
        ));
        let keyframes = cache.probe_keyframes(probe.duration, opts.interval, opts.force);
        let subtitle = cache.subtitle_path(opts.force);
        let cues = subtitle
            .as_deref()
            .map(load_subtitle_cues_from)
            .unwrap_or_default();
        let palette = average_palette(&keyframes);
        let avg_shot_len = cache
            .cached_scene_cuts(0.0, probe.duration)
            .map(|cuts| probe.duration / (cuts.len() as f64 + 1.0));
        episodes.push(Episode {
            number,
            cache,
            probe,
            keyframes,
            subtitle,
            cues,
            palette,
            avg_shot_len,
        });
    }
    if episodes.is_empty() {
        anyhow::bail!("No episode could be probed");
    }
    report.episodes = episodes.len();

    let mut world = deterministic_world(&root, &episodes, opts.interval);
    let mut characters = deterministic_characters(&episodes);
    let mut mode = "offline".to_string();
    let mut m3_days: Option<String> = None;

    let api_key = std::env::var("MINIMAX_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    if !opts.offline {
        match api_key.as_deref() {
            Some(key) => match summarize_with_m3(&episodes, key.trim()) {
                Ok(summary) => {
                    mode = "m3".to_string();
                    world["mode"] = json!("m3");
                    if let Some(w) = summary.get("world").and_then(|w| w.as_object()) {
                        for key in ["era", "season", "weather", "locations", "color_grade"] {
                            if let Some(v) = w.get(key) {
                                world[key] = v.clone();
                            }
                        }
                        world["m3_world"] = Value::Object(w.clone());
                    }
                    characters["mode"] = json!("m3");
                    if let Some(list) = summary.get("characters").and_then(|c| c.as_array()) {
                        if !list.is_empty() {
                            characters["subtitle_speakers"] = characters["characters"].clone();
                            characters["characters"] = Value::Array(list.clone());
                        }
                    }
                    m3_days = summary
                        .get("timeline_days_md")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                }
                Err(e) => eprintln!(
                    "{} Warning: series-watch M3 summary failed ({:#}); writing deterministic offline context",
                    crate::log::stamp(),
                    e
                ),
            },
            None => eprintln!(
                "{} Warning: MINIMAX_API_KEY not set; writing deterministic offline series context",
                crate::log::stamp()
            ),
        }
    }

    let mut days_md = deterministic_timeline_md(&episodes, &mode);
    if let Some(extra) = m3_days {
        days_md.push_str("\n## M3 draft\n\n");
        days_md.push_str(&extra);
        days_md.push('\n');
    }

    let existing_world_is_m3 = has_m3_mode(&world_path);
    let existing_chars_is_m3 = has_m3_mode(&chars_path);

    if mode == "offline" && existing_world_is_m3 {
        log(&format!(
            "skip (preserve existing m3): {}",
            world_path.display()
        ));
        report.skipped.push(world_path.clone());
    } else {
        write_if_needed(
            &world_path,
            &serde_json::to_string_pretty(&world)?,
            opts.force,
            &mut report,
        )?;
    }

    if mode == "offline" && existing_chars_is_m3 {
        log(&format!(
            "skip (preserve existing m3): {}",
            chars_path.display()
        ));
        report.skipped.push(chars_path.clone());
    } else {
        write_if_needed(
            &chars_path,
            &serde_json::to_string_pretty(&characters)?,
            opts.force,
            &mut report,
        )?;
    }

    if mode == "offline" && existing_world_is_m3 && days_path.exists() {
        log(&format!(
            "skip (preserve existing m3): {}",
            days_path.display()
        ));
        report.skipped.push(days_path.clone());
    } else {
        write_if_needed(&days_path, &days_md, opts.force, &mut report)?;
    }

    report.mode = mode;
    log(&format!(
        "done: {} episode(s), mode={}, story at {}",
        report.episodes,
        report.mode,
        story_dir.display()
    ));
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_natural_key_orders_episode_numbers() {
        let mut names = vec!["EP10.mp4", "EP2.mp4", "EP1.mp4"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, vec!["EP1.mp4", "EP2.mp4", "EP10.mp4"]);
    }

    #[test]
    fn test_speaker_tag_detection() {
        assert_eq!(
            speaker_tag("WALTER: Say my name.").as_deref(),
            Some("WALTER")
        );
        assert_eq!(speaker_tag("[S1] hello").as_deref(), Some("S1"));
        assert_eq!(speaker_tag("(지수) 안녕").as_deref(), Some("지수"));
        assert!(speaker_tag("[Music]").is_none());
        assert!(speaker_tag("just a line").is_none());
    }

    #[test]
    fn test_subtitle_excerpt_respects_budget() {
        let cues: Vec<SubtitleCue> = (0..500)
            .map(|i| SubtitleCue {
                start_sec: i as f64,
                end_sec: i as f64 + 1.0,
                text: format!("line number {i} with some words"),
            })
            .collect();
        let ex = subtitle_excerpt(&cues, 2_000);
        assert!(ex.len() <= 2_200);
        assert!(ex.contains("line number 0 "));
    }

    #[test]
    fn test_has_m3_mode_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let p_m3 = dir.path().join("m3.json");
        std::fs::write(&p_m3, r#"{"mode": "m3", "val": 1}"#).unwrap();
        assert!(has_m3_mode(&p_m3));

        let p_offline = dir.path().join("offline.json");
        std::fs::write(&p_offline, r#"{"mode": "offline", "val": 1}"#).unwrap();
        assert!(!has_m3_mode(&p_offline));

        let p_invalid = dir.path().join("invalid.json");
        std::fs::write(&p_invalid, "not json").unwrap();
        assert!(!has_m3_mode(&p_invalid));

        let p_missing = dir.path().join("missing.json");
        assert!(!has_m3_mode(&p_missing));
    }

    #[test]
    fn test_preserve_existing_m3_context_on_offline_force() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let story = root.join("story");
        std::fs::create_dir_all(&story).unwrap();

        let world_path = story.join("world.json");
        let chars_path = story.join("characters.json");
        let days_path = story.join("timeline-days.md");

        std::fs::write(
            &world_path,
            r#"{"mode": "m3", "locations": [{"name": "M3-Castle"}]}"#,
        )
        .unwrap();
        std::fs::write(
            &chars_path,
            r#"{"mode": "m3", "characters": [{"id": "P1", "name": "M3-King"}]}"#,
        )
        .unwrap();
        std::fs::write(&days_path, "# M3 Timeline\nDay 1 in Castle\n").unwrap();

        let vid_path = root.join("ep01.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:d=0.5",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                vid_path.to_str().unwrap(),
            ])
            .status();
        if status.is_err() || !status.unwrap().success() {
            return;
        }

        let input_str = vid_path.to_string_lossy().to_string();
        let inputs = vec![input_str];
        let opts = SeriesWatchOptions {
            inputs: &inputs,
            source_dir: None,
            out_root: Some(root),
            offline: true,
            force: true,
            interval: 5.0,
        };

        let report = run_series_watch(opts).unwrap();
        assert_eq!(report.mode, "offline");
        assert!(report.skipped.contains(&world_path));
        assert!(report.skipped.contains(&chars_path));
        assert!(report.skipped.contains(&days_path));

        let world_after: Value =
            serde_json::from_str(&std::fs::read_to_string(&world_path).unwrap()).unwrap();
        assert_eq!(world_after["mode"], "m3");
        assert_eq!(world_after["locations"][0]["name"], "M3-Castle");

        let chars_after: Value =
            serde_json::from_str(&std::fs::read_to_string(&chars_path).unwrap()).unwrap();
        assert_eq!(chars_after["mode"], "m3");
        assert_eq!(chars_after["characters"][0]["name"], "M3-King");

        let days_after = std::fs::read_to_string(&days_path).unwrap();
        assert!(days_after.contains("# M3 Timeline"));
    }
}
