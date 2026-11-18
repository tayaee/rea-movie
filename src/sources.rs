//! Issue 8: reusable per-source cache under `<root>/input-details/<source_id>/`.
//!
//! A source (a local video file or a downloaded YouTube video) is analysed
//! once; every artifact that does not depend on the project (ffprobe metadata,
//! scene cuts, subtitles, anchor frames, per-span clips/contact sheets, probe
//! keyframes) is stored here and reused by every later decompile of the same
//! video — another `--range`, another project (`-v2`), or `series-watch`.
//!
//! Layout:
//! ```text
//! <root>/input-details/index.json                  input -> source_id map
//! <root>/input-details/<source_id>/video.<ext>     symlink to the original (when outside)
//! <root>/input-details/<source_id>/probe.json      ffprobe metadata
//! <root>/input-details/<source_id>/scene_cuts.json scene-cut timestamps per scanned segment
//! <root>/input-details/<source_id>/subtitles/      cached / extracted .srt files
//! <root>/input-details/<source_id>/keyframes/      probe_t<ms>.jpg (5s grid), anchor_t<ms>.jpg
//! <root>/input-details/<source_id>/shots_cache/<start_ms>_<end_ms>/  clip, audio, contact sheet
//! ```
//! Pure input videos live in `<root>/input/`. The legacy `<root>/sources/` and
//! `<root>/source/` directories (old `reverse-url.sh` layout) are still
//! recognised when resolving the root from an input path.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SOURCES_DIR_NAME: &str = "input-details";
pub const LEGACY_SOURCE_DIR_NAME: &str = "input";
pub const OLD_SOURCES_DIR_NAME: &str = "sources";
pub const OLD_LEGACY_SOURCE_DIR_NAME: &str = "source";
/// ffmpeg `select='gt(scene,X)'` threshold used by scene detection.
pub const SCENE_THRESHOLD: f64 = 0.14;
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "mov", "webm", "m4v", "avi"];

pub fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn log(msg: &str) {
    println!("{} [sources] {}", crate::log::stamp(), msg);
}

/// Series root derived from an input path located in `<root>/input-details/<id>/`,
/// `<root>/input-details/`, `<root>/input/`, or the legacy `<root>/sources/<id>/`
/// and `<root>/source/` layouts.
pub fn root_from_input(input: &Path) -> Option<PathBuf> {
    let abs = absolute(input);
    let mut cur = abs.parent();
    // Look at most two levels up: input-details/<id>/file or input/file
    // (plus legacy sources/<id>/file and source/file).
    for _ in 0..2 {
        let dir = cur?;
        let name = dir.file_name().and_then(|n| n.to_str());
        if name == Some(SOURCES_DIR_NAME)
            || name == Some(LEGACY_SOURCE_DIR_NAME)
            || name == Some(OLD_SOURCES_DIR_NAME)
            || name == Some(OLD_LEGACY_SOURCE_DIR_NAME)
        {
            return dir.parent().map(|p| p.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Resolve `<root>/input-details/` for a decompile.
///
/// Priority: `REA_MOVIE_SOURCES_DIR` env → series root of a
/// `<root>/h3-projects/<name>` project → root derived from the input path.
/// Standalone projects with inputs outside any root get no cache (`None`),
/// which keeps the pre-issue-8 behavior.
pub fn resolve_sources_dir(proj_dir: Option<&Path>, input: &Path) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("REA_MOVIE_SOURCES_DIR") {
        if !dir.trim().is_empty() {
            return Some(PathBuf::from(dir.trim()));
        }
    }
    if let Some(root) = proj_dir.and_then(crate::decompile::series_root_for) {
        return Some(root.join(SOURCES_DIR_NAME));
    }
    root_from_input(input).map(|r| r.join(SOURCES_DIR_NAME))
}

/// YouTube video id embedded by yt-dlp as `... [<id>].ext`.
pub fn youtube_id_from_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let re = regex::Regex::new(r"\[([A-Za-z0-9_-]{11})\]\s*$").ok()?;
    re.captures(stem).map(|c| c[1].to_string())
}

fn file_size_mtime(path: &Path) -> Result<(u64, i64)> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok((meta.len(), mtime))
}

/// Content fingerprint: FNV-1a 64 over size + first/last 1 MiB. Path
/// independent, so the same file copied elsewhere maps to the same source.
pub fn content_fingerprint(path: &Path) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    const CHUNK: u64 = 1 << 20;
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let size = file.metadata()?.len();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(&size.to_le_bytes());
    let mut buf = Vec::new();
    (&mut file).take(CHUNK).read_to_end(&mut buf)?;
    feed(&buf);
    if size > CHUNK {
        buf.clear();
        file.seek(SeekFrom::Start(size.saturating_sub(CHUNK).max(CHUNK)))?;
        file.read_to_end(&mut buf)?;
        feed(&buf);
    }
    Ok(format!("{hash:016x}"))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn copy_atomic(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = to.with_file_name(format!(
        ".{}.tmp",
        to.file_name().and_then(|n| n.to_str()).unwrap_or("cache")
    ));
    std::fs::copy(from, &tmp)
        .with_context(|| format!("copy {} -> {}", from.display(), tmp.display()))?;
    std::fs::rename(&tmp, to)?;
    Ok(())
}

fn is_nonempty_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

fn ms_key(t: f64) -> u64 {
    (t.max(0.0) * 1000.0).round() as u64
}

// ---------------------------------------------------------------------------
// index.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexedInput {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub source_id: String,
    /// `youtube` | `file`
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub inputs: Vec<IndexedInput>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceIndex {
    pub version: u32,
    #[serde(default)]
    pub sources: BTreeMap<String, IndexEntry>,
}

impl Default for SourceIndex {
    fn default() -> Self {
        Self {
            version: 1,
            sources: BTreeMap::new(),
        }
    }
}

impl SourceIndex {
    pub fn load(sources_dir: &Path) -> Self {
        std::fs::read_to_string(sources_dir.join("index.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, sources_dir: &Path) -> Result<()> {
        write_json_atomic(&sources_dir.join("index.json"), self)
    }

    /// Fast path: an input already indexed with identical size + mtime.
    pub fn lookup(&self, path: &str, size: u64, mtime: i64) -> Option<String> {
        self.sources.values().find_map(|e| {
            e.inputs
                .iter()
                .any(|i| i.path == path && i.size == size && i.mtime == mtime)
                .then(|| e.source_id.clone())
        })
    }
}

// ---------------------------------------------------------------------------
// probe.json / scene_cuts.json
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProbeInfo {
    pub duration: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub video_codec: String,
    #[serde(default)]
    pub has_audio: bool,
    #[serde(default)]
    pub subtitle_streams: u32,
    pub size: u64,
    pub mtime: i64,
}

fn parse_rate(rate: &str) -> f64 {
    let mut parts = rate.split('/');
    let num: f64 = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0.0);
    let den: f64 = parts.next().and_then(|d| d.parse().ok()).unwrap_or(1.0);
    if den > 0.0 {
        num / den
    } else {
        0.0
    }
}

pub fn run_ffprobe(video: &Path) -> Result<ProbeInfo> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format"])
        .arg("-show_streams")
        .arg(video)
        .output()
        .with_context(|| format!("Failed to run ffprobe on {}", video.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let video_stream = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .cloned()
        .unwrap_or_default();
    let num = |v: &serde_json::Value| -> f64 {
        v.as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| v.as_f64())
            .unwrap_or(0.0)
    };
    let mut duration = num(&json["format"]["duration"]);
    if duration <= 0.0 {
        duration = num(&video_stream["duration"]);
    }
    let mut fps = parse_rate(video_stream["avg_frame_rate"].as_str().unwrap_or("0/1"));
    if fps <= 0.0 {
        fps = parse_rate(video_stream["r_frame_rate"].as_str().unwrap_or("0/1"));
    }
    let (size, mtime) = file_size_mtime(video)?;
    Ok(ProbeInfo {
        duration,
        fps,
        width: video_stream["width"].as_u64().unwrap_or(0) as u32,
        height: video_stream["height"].as_u64().unwrap_or(0) as u32,
        video_codec: video_stream["codec_name"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        subtitle_streams: streams
            .iter()
            .filter(|s| s["codec_type"] == "subtitle")
            .count() as u32,
        size,
        mtime,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CutSegment {
    pub start: f64,
    pub end: f64,
    pub cuts: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SceneCutsCache {
    pub version: u32,
    pub threshold: f64,
    pub size: u64,
    pub mtime: i64,
    /// Scanned segments (merged when overlapping). Cuts are absolute seconds.
    pub segments: Vec<CutSegment>,
}

impl SceneCutsCache {
    /// Cuts strictly inside `(start, end)` if a cached segment covers the range.
    pub fn lookup(&self, start: f64, end: f64) -> Option<Vec<f64>> {
        const EPS: f64 = 0.01;
        let seg = self
            .segments
            .iter()
            .find(|s| s.start <= start + EPS && s.end >= end - EPS)?;
        Some(
            seg.cuts
                .iter()
                .copied()
                .filter(|c| *c > start + 0.1 && *c < end - 0.1)
                .collect(),
        )
    }

    /// Insert a scanned segment, merging every overlapping segment into it.
    pub fn insert(&mut self, start: f64, end: f64, cuts: &[f64]) {
        let mut merged = CutSegment {
            start,
            end,
            cuts: cuts.to_vec(),
        };
        let mut rest = Vec::new();
        for seg in self.segments.drain(..) {
            if seg.start <= merged.end && merged.start <= seg.end {
                merged.start = merged.start.min(seg.start);
                merged.end = merged.end.max(seg.end);
                merged.cuts.extend(seg.cuts);
            } else {
                rest.push(seg);
            }
        }
        merged
            .cuts
            .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        merged.cuts.dedup_by(|a, b| (*a - *b).abs() < 0.1);
        rest.push(merged);
        rest.sort_by(|a, b| {
            a.start
                .partial_cmp(&b.start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.segments = rest;
    }
}

// ---------------------------------------------------------------------------
// SourceCache
// ---------------------------------------------------------------------------

/// Outcome of materialising one cached asset into a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetOutcome {
    /// Target already present in the project (skip-by-default).
    Existing,
    /// Copied from `<root>/input-details/<id>/`.
    Reused,
    /// Freshly extracted (and stored into the cache).
    Extracted,
}

pub struct SourceCache {
    pub source_id: String,
    pub dir: PathBuf,
    /// Canonical path of the actual video file.
    pub video: PathBuf,
    reused: Cell<usize>,
    extracted: Cell<usize>,
}

impl SourceCache {
    /// Register `input` in `<sources_dir>/index.json` and prepare its cache dir.
    pub fn open(sources_dir: &Path, input: &Path) -> Result<Self> {
        std::fs::create_dir_all(sources_dir)
            .with_context(|| format!("create {}", sources_dir.display()))?;
        let sources_dir = std::fs::canonicalize(sources_dir)?;
        let input_abs = absolute(input);
        let video = std::fs::canonicalize(input)
            .with_context(|| format!("resolve input {}", input.display()))?;
        let video_str = video.to_string_lossy().to_string();
        let (size, mtime) = file_size_mtime(&video)?;
        let mut index = SourceIndex::load(&sources_dir);

        // 1. Input already lives in <sources>/<id>/ -> that directory is authoritative.
        let inside_id = input_abs
            .parent()
            .filter(|p| {
                p.parent()
                    .and_then(|pp| std::fs::canonicalize(pp).ok())
                    .map(|pp| pp == sources_dir)
                    .unwrap_or(false)
            })
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
        let video_id = youtube_id_from_name(&video).or_else(|| youtube_id_from_name(&input_abs));
        let indexed = index.lookup(&video_str, size, mtime);
        let mut fingerprint = String::new();
        let source_id = if let Some(id) = inside_id {
            id
        } else if let Some(id) = indexed {
            id
        } else if let Some(vid) = &video_id {
            format!("yt-{vid}")
        } else {
            fingerprint = content_fingerprint(&video)?;
            format!("file-{fingerprint}")
        };

        let dir = sources_dir.join(&source_id);
        std::fs::create_dir_all(&dir)?;

        // Original video link when the input lives outside the cache dir.
        let video_in_dir = video
            .parent()
            .map(|p| std::fs::canonicalize(p).ok() == std::fs::canonicalize(&dir).ok())
            .unwrap_or(false);
        if !video_in_dir {
            let ext = video.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
            let link = dir.join(format!("video.{ext}"));
            // Keep a live link (or a real cached file); replace missing/broken links.
            if !link.exists() {
                let _ = std::fs::remove_file(&link);
                #[cfg(unix)]
                {
                    if let Err(e) = std::os::unix::fs::symlink(&video, &link) {
                        eprintln!("Warning: failed to link source video ({e})");
                    }
                }
            }
        }

        // Update index.json.
        let now = chrono::Local::now().to_rfc3339();
        let title = video
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let entry = index
            .sources
            .entry(source_id.clone())
            .or_insert_with(|| IndexEntry {
                source_id: source_id.clone(),
                kind: if video_id.is_some() {
                    "youtube"
                } else {
                    "file"
                }
                .to_string(),
                video_id: video_id.clone(),
                url: video_id
                    .as_ref()
                    .map(|v| format!("https://www.youtube.com/watch?v={v}")),
                title: title.clone(),
                fingerprint: fingerprint.clone(),
                inputs: Vec::new(),
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        if entry.fingerprint.is_empty() && !fingerprint.is_empty() {
            entry.fingerprint = fingerprint;
        }
        let new_input = IndexedInput {
            path: video_str.clone(),
            size,
            mtime,
        };
        if !entry.inputs.contains(&new_input) {
            entry.inputs.retain(|i| i.path != video_str);
            entry.inputs.push(new_input);
            entry.updated_at = now;
        }
        index.save(&sources_dir)?;

        Ok(Self {
            source_id,
            dir,
            video,
            reused: Cell::new(0),
            extracted: Cell::new(0),
        })
    }

    pub fn video_str(&self) -> String {
        self.video.to_string_lossy().to_string()
    }

    pub fn reused_count(&self) -> usize {
        self.reused.get()
    }

    pub fn extracted_count(&self) -> usize {
        self.extracted.get()
    }

    pub fn keyframes_dir(&self) -> PathBuf {
        self.dir.join("keyframes")
    }

    pub fn subtitles_dir(&self) -> PathBuf {
        self.dir.join("subtitles")
    }

    /// Full-resolution anchor frame keyed by timestamp.
    pub fn anchor_frame_path(&self, t: f64) -> PathBuf {
        self.keyframes_dir()
            .join(format!("anchor_t{:09}.jpg", ms_key(t)))
    }

    /// Down-scaled probe keyframe (series-watch grid) keyed by timestamp.
    pub fn probe_frame_path(&self, t: f64) -> PathBuf {
        self.keyframes_dir()
            .join(format!("probe_t{:09}.jpg", ms_key(t)))
    }

    /// Span-keyed cache dir for clip / audio / contact sheet.
    pub fn shot_cache_dir(&self, start: f64, end: f64) -> PathBuf {
        self.dir
            .join("shots_cache")
            .join(format!("{:09}_{:09}", ms_key(start), ms_key(end)))
    }

    /// ffprobe metadata, cached in `probe.json` (invalidated on size/mtime change).
    pub fn probe(&self, force: bool) -> Result<ProbeInfo> {
        let path = self.dir.join("probe.json");
        let (size, mtime) = file_size_mtime(&self.video)?;
        if !force {
            if let Some(info) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<ProbeInfo>(&s).ok())
                .filter(|p| p.size == size && p.mtime == mtime && p.duration > 0.0)
            {
                log(&format!("reuse probe.json ({})", self.source_id));
                return Ok(info);
            }
        }
        let info = run_ffprobe(&self.video)?;
        write_json_atomic(&path, &info)?;
        log(&format!(
            "wrote probe.json ({}: {:.2}s, {}x{} @ {:.2}fps)",
            self.source_id, info.duration, info.width, info.height, info.fps
        ));
        Ok(info)
    }

    fn load_scene_cuts(&self) -> Option<SceneCutsCache> {
        let (size, mtime) = file_size_mtime(&self.video).ok()?;
        std::fs::read_to_string(self.dir.join("scene_cuts.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<SceneCutsCache>(&s).ok())
            .filter(|c| {
                c.size == size && c.mtime == mtime && (c.threshold - SCENE_THRESHOLD).abs() < 1e-9
            })
    }

    /// Cached scene cuts covering `[start, end]` (no ffmpeg run).
    pub fn cached_scene_cuts(&self, start: f64, end: f64) -> Option<Vec<f64>> {
        self.load_scene_cuts()?.lookup(start, end)
    }

    /// Scene cuts for `[start, end]`: reuse `scene_cuts.json` when a cached
    /// segment covers the range, otherwise run `detect` and store the result.
    pub fn scene_cuts<F>(&self, start: f64, end: f64, force: bool, detect: F) -> Result<Vec<f64>>
    where
        F: FnOnce(f64, f64) -> Result<Vec<f64>>,
    {
        if !force {
            if let Some(cuts) = self.cached_scene_cuts(start, end) {
                log(&format!(
                    "reuse scene_cuts.json ({}: {:.2}-{:.2}s, {} cut(s))",
                    self.source_id,
                    start,
                    end,
                    cuts.len()
                ));
                return Ok(cuts);
            }
        }
        let cuts = detect(start, end)?;
        let (size, mtime) = file_size_mtime(&self.video)?;
        let mut cache = self
            .load_scene_cuts()
            .filter(|_| !force)
            .unwrap_or(SceneCutsCache {
                version: 1,
                threshold: SCENE_THRESHOLD,
                size,
                mtime,
                segments: Vec::new(),
            });
        cache.insert(start, end, &cuts);
        write_json_atomic(&self.dir.join("scene_cuts.json"), &cache)?;
        log(&format!(
            "wrote scene_cuts.json ({}: {:.2}-{:.2}s, {} cut(s))",
            self.source_id,
            start,
            end,
            cuts.len()
        ));
        Ok(cuts)
    }

    /// Best subtitle for this source.
    ///
    /// Priority: sibling `.srt` of the original (copied into `subtitles/` for
    /// later reuse) → `<dir>/<stem>.*.srt` → `subtitles/*.srt` → one-time
    /// extraction of the first embedded subtitle stream (a `.no-embedded`
    /// marker prevents re-trying on every run; `force` retries).
    pub fn subtitle_path(&self, force: bool) -> Option<PathBuf> {
        let subs_dir = self.subtitles_dir();
        if let Some(sibling) =
            crate::decompile::story::find_sibling_srt(&self.video.to_string_lossy())
        {
            if let Some(name) = sibling.file_name() {
                let cached = subs_dir.join(name);
                if force || !cached.exists() {
                    let _ = copy_atomic(&sibling, &cached);
                }
            }
            return Some(sibling);
        }
        if let Some(found) = pick_srt(&self.dir).or_else(|| pick_srt(&subs_dir)) {
            log(&format!(
                "reuse subtitle {} ({})",
                found.display(),
                self.source_id
            ));
            return Some(found);
        }
        let marker = subs_dir.join(".no-embedded");
        if marker.exists() && !force {
            return None;
        }
        let _ = std::fs::create_dir_all(&subs_dir);
        let out = subs_dir.join("embedded.srt");
        let ok = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-i"])
            .arg(&self.video)
            .args(["-map", "0:s:0", "-c:s", "srt"])
            .arg(&out)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok && is_nonempty_file(&out) {
            log(&format!(
                "extracted embedded subtitle -> {} ({})",
                out.display(),
                self.source_id
            ));
            Some(out)
        } else {
            let _ = std::fs::remove_file(&out);
            let _ = std::fs::write(&marker, "no embedded subtitle stream\n");
            None
        }
    }

    /// 5-second-grid probe keyframes (down-scaled to 480px wide) for
    /// series-watch. Existing frames are reused unless `force`.
    pub fn probe_keyframes(&self, duration: f64, interval: f64, force: bool) -> Vec<PathBuf> {
        let interval = if interval > 0.0 { interval } else { 5.0 };
        let mut frames = Vec::new();
        let mut reused = 0usize;
        let mut t = 0.0;
        while t < duration - 0.05 {
            let path = self.probe_frame_path(t);
            if !force && is_nonempty_file(&path) {
                reused += 1;
                frames.push(path);
            } else {
                let _ = std::fs::create_dir_all(self.keyframes_dir());
                let ok = Command::new("ffmpeg")
                    .args(["-nostdin", "-v", "error", "-y", "-ss"])
                    .arg(format!("{t:.3}"))
                    .arg("-i")
                    .arg(&self.video)
                    .args(["-vframes", "1", "-vf", "scale=480:-2", "-q:v", "4"])
                    .arg(&path)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if ok && is_nonempty_file(&path) {
                    frames.push(path);
                }
            }
            t += interval;
        }
        log(&format!(
            "probe keyframes {}: {} total ({} reused) every {:.1}s",
            self.source_id,
            frames.len(),
            reused,
            interval
        ));
        frames
    }
}

/// Materialise one asset at `target`: keep an existing target (unless
/// `force`), else copy from `cached`, else run `produce` and store the result
/// into `cached`. Works without a cache (`cached = None`).
pub fn materialize_asset<F>(
    cache: Option<&SourceCache>,
    target: &Path,
    cached: Option<&Path>,
    force: bool,
    produce: F,
) -> Result<AssetOutcome>
where
    F: FnOnce(&Path) -> Result<()>,
{
    if !force && target.exists() {
        return Ok(AssetOutcome::Existing);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(cached) = cached {
        if !force && is_nonempty_file(cached) {
            copy_atomic(cached, target)?;
            if let Some(c) = cache {
                c.reused.set(c.reused.get() + 1);
            }
            return Ok(AssetOutcome::Reused);
        }
    }
    produce(target)?;
    if let Some(cached) = cached {
        if is_nonempty_file(target) {
            if let Err(e) = copy_atomic(target, cached) {
                eprintln!("Warning: failed to store source cache asset ({e:#})");
            }
        }
    }
    if let Some(c) = cache {
        c.extracted.set(c.extracted.get() + 1);
    }
    Ok(AssetOutcome::Extracted)
}

/// Pick the preferred `.srt` in `dir`: `*.ko.srt` > plain > `*.en.srt` >
/// `*.diarized.srt` > `embedded.srt`.
pub fn pick_srt(dir: &Path) -> Option<PathBuf> {
    let mut srts: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("srt"))
        .collect();
    srts.sort();
    let rank = |p: &PathBuf| -> u8 {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if name.ends_with(".ko.srt") {
            0
        } else if name == "embedded.srt" {
            4
        } else if name.ends_with(".diarized.srt") {
            3
        } else if name.ends_with(".en.srt") {
            2
        } else {
            1
        }
    };
    srts.into_iter().min_by_key(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_youtube_id_from_name() {
        assert_eq!(
            youtube_id_from_name(Path::new("/x/01_Title [dQw4w9WgXcQ].mp4")).as_deref(),
            Some("dQw4w9WgXcQ")
        );
        assert!(youtube_id_from_name(Path::new("/x/movie.mp4")).is_none());
    }

    #[test]
    fn test_root_from_input_new_and_legacy() {
        assert_eq!(
            root_from_input(Path::new("/r/input-details/yt-abc/ep.mp4")),
            Some(PathBuf::from("/r"))
        );
        assert_eq!(
            root_from_input(Path::new("/r/input/ep.mp4")),
            Some(PathBuf::from("/r"))
        );
        assert_eq!(
            root_from_input(Path::new("/r/sources/yt-abc/ep.mp4")),
            Some(PathBuf::from("/r"))
        );
        assert_eq!(
            root_from_input(Path::new("/r/source/ep.mp4")),
            Some(PathBuf::from("/r"))
        );
        assert!(root_from_input(Path::new("/tmp/x/ep.mp4")).is_none());
    }

    #[test]
    fn test_scene_cuts_cache_lookup_and_merge() {
        let mut cache = SceneCutsCache {
            version: 1,
            threshold: SCENE_THRESHOLD,
            size: 1,
            mtime: 1,
            segments: Vec::new(),
        };
        cache.insert(0.0, 60.0, &[5.0, 30.0, 59.0]);
        // Sub-range of a cached segment: filtered without ffmpeg.
        assert_eq!(cache.lookup(10.0, 40.0), Some(vec![30.0]));
        assert_eq!(cache.lookup(0.0, 60.0), Some(vec![5.0, 30.0, 59.0]));
        // Not covered.
        assert!(cache.lookup(50.0, 90.0).is_none());
        // Overlapping insert merges coverage.
        cache.insert(50.0, 90.0, &[59.02, 70.0]);
        assert_eq!(cache.segments.len(), 1);
        assert_eq!(cache.lookup(0.0, 90.0), Some(vec![5.0, 30.0, 59.0, 70.0]));
        // Disjoint insert keeps a separate segment.
        cache.insert(200.0, 210.0, &[205.0]);
        assert_eq!(cache.segments.len(), 2);
        assert_eq!(cache.lookup(200.0, 210.0), Some(vec![205.0]));
    }

    #[test]
    fn test_pick_srt_priority() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.en.srt", "embedded.srt", "a.ko.srt"] {
            std::fs::write(dir.path().join(name), "1\n").unwrap();
        }
        assert_eq!(
            pick_srt(dir.path()).unwrap().file_name().unwrap(),
            "a.ko.srt"
        );
        std::fs::remove_file(dir.path().join("a.ko.srt")).unwrap();
        assert_eq!(
            pick_srt(dir.path()).unwrap().file_name().unwrap(),
            "a.en.srt"
        );
    }

    #[test]
    fn test_open_index_and_materialize_reuse() {
        let root = tempfile::tempdir().unwrap();
        let video = root.path().join("clip.mp4");
        std::fs::write(&video, b"not really a video but stable bytes").unwrap();
        let sources = root.path().join("input-details");

        let c1 = SourceCache::open(&sources, &video).unwrap();
        assert!(c1.source_id.starts_with("file-"));
        assert!(sources.join("index.json").is_file());
        assert!(c1.dir.join("video.mp4").exists());

        // Same file again -> same id via index.json.
        let c2 = SourceCache::open(&sources, &video).unwrap();
        assert_eq!(c1.source_id, c2.source_id);
        let index = SourceIndex::load(&sources);
        assert_eq!(index.sources.len(), 1);

        // Copy elsewhere -> same id via content fingerprint.
        let copy = root.path().join("other").join("clip-copy.mp4");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(&video, &copy).unwrap();
        let c3 = SourceCache::open(&sources, &copy).unwrap();
        assert_eq!(c1.source_id, c3.source_id);

        // Materialize: first extract, then reuse from cache into another target.
        let cached = c1.shot_cache_dir(1.0, 2.0).join("asset.bin");
        let t1 = root.path().join("p1").join("asset.bin");
        let out = materialize_asset(Some(&c1), &t1, Some(&cached), false, |p| {
            std::fs::write(p, b"payload")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(out, AssetOutcome::Extracted);
        assert!(cached.is_file());
        let t2 = root.path().join("p2").join("asset.bin");
        let out2 = materialize_asset(Some(&c1), &t2, Some(&cached), false, |_| {
            anyhow::bail!("must not extract")
        })
        .unwrap();
        assert_eq!(out2, AssetOutcome::Reused);
        assert_eq!(std::fs::read(&t2).unwrap(), b"payload");
        let out3 = materialize_asset(Some(&c1), &t2, Some(&cached), false, |_| {
            anyhow::bail!("must not extract")
        })
        .unwrap();
        assert_eq!(out3, AssetOutcome::Existing);
        assert_eq!(c1.reused_count(), 1);
        assert_eq!(c1.extracted_count(), 1);
    }
}
