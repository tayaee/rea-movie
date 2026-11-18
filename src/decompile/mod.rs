pub mod audio;
pub mod contact_sheet;
pub mod extract;
pub mod prompt;
pub mod run_script;
pub mod scene;
pub mod story;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::ProjectConfig;
use crate::timeline::{ShotMeta, TimelineManifest, TimelineShotItem};
use extract::{extract_shot_references, ShotExtractPaths};
use prompt::{generate_prompts, PromptGenContext};
use run_script::generate_run_script;
use scene::{build_shot_spans, build_shot_spans_with, detect_scene_cuts, probe_video_duration};
use story::generate_story_assets;

pub struct DecompileOptions<'a> {
    pub input_path: &'a str,
    pub output_dir: Option<&'a str>,
    pub range: Option<&'a str>,
    pub max_shot_duration: Option<f64>,
    pub min_shot_duration: Option<f64>,
    pub force: bool,
    pub run_after: bool,
    pub smoke: bool,
    pub offline: bool,
    pub dub_lang: Option<&'a str>,
    pub custom_mod: Option<&'a str>,
}

/// Series root for `<root>/h3-projects/<name>` layouts: the grandparent dir.
/// Series-level artifacts (world bible etc.) live at `<root>/story/`.
pub fn series_root_for(proj_dir: &Path) -> Option<PathBuf> {
    let resolved = std::fs::canonicalize(proj_dir).unwrap_or_else(|_| proj_dir.to_path_buf());
    let parent = resolved.parent()?;
    if parent.file_name().and_then(|n| n.to_str()) != Some("h3-projects") {
        return None;
    }
    parent.parent().map(|p| p.to_path_buf())
}

/// Warn when a project has no series context (`<root>/story/world.json`).
/// Non-blocking: partial reverse-engineering proceeds with locally scoped IDs.
pub fn check_series_context(proj_dir: &Path) {
    let has_context = series_root_for(proj_dir)
        .map(|root| root.join("story").join("world.json"))
        .map(|p| p.is_file())
        .unwrap_or(false)
        || proj_dir.join("story").join("world.json").is_file();
    if !has_context {
        eprintln!(
            "{} Warning: no series context (<root>/story/world.json). Proceeding with locally scoped IDs; cross-episode consistency is NOT guaranteed. Ask the user for the full series first (skill section 3-0).",
            crate::log::stamp()
        );
    }
}

/// Load `<root>/story/world.json` (or `<proj>/story/world.json`) and summarize
/// it into a short English string for M3 prompt injection (issue 9).
/// Returns None when no world bible exists.
pub fn load_series_world_summary(proj_dir: &Path) -> Option<String> {
    let candidates = [
        series_root_for(proj_dir).map(|r| r.join("story").join("world.json")),
        Some(proj_dir.join("story").join("world.json")),
    ];
    let path = candidates.into_iter().flatten().find(|p| p.is_file())?;
    let content = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let mut parts = Vec::new();
    for key in ["era", "season", "weather", "color_grade"] {
        if let Some(val) = v.get(key) {
            if val.is_null() {
                continue;
            }
            let s = match val {
                serde_json::Value::String(t) => t.clone(),
                other => other.to_string(),
            };
            if !s.trim().is_empty() && s.trim() != "null" {
                parts.push(format!("{}: {}", key, s.trim()));
            }
        }
    }
    if let Some(locs) = v.get("locations").and_then(|l| l.as_array()) {
        let names: Vec<String> = locs
            .iter()
            .take(6)
            .map(|l| {
                if let Some(s) = l.as_str() {
                    s.to_string()
                } else if let Some(name) = l.get("name").and_then(|n| n.as_str()) {
                    let mut d = name.to_string();
                    if let Some(desc) = l.get("description").and_then(|x| x.as_str()) {
                        if !desc.is_empty() {
                            d.push_str(&format!(" ({})", desc));
                        }
                    }
                    d
                } else {
                    l.to_string()
                }
            })
            .collect();
        if !names.is_empty() {
            parts.push(format!("locations: {}", names.join("; ")));
        }
    }
    if let Some(cine) = v.get("cinematography") {
        let s = cine.to_string();
        if s.len() < 400 {
            parts.push(format!("cinematography: {}", s));
        }
    }
    if parts.is_empty() {
        // Fall back to a truncated raw dump so *some* series context flows.
        let s = content.replace(|c: char| c.is_whitespace(), " ");
        let t: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
        return Some(t.chars().take(800).collect());
    }
    let out = parts.join(" | ");
    Some(out.chars().take(1200).collect())
}

pub fn run_decompile(opts: DecompileOptions) -> Result<PathBuf> {
    let input_path = opts.input_path;
    let input_meta = std::fs::metadata(input_path).with_context(|| {
        format!(
            "Input video file '{}' does not exist or cannot be accessed",
            input_path
        )
    })?;
    if !input_meta.is_file() {
        anyhow::bail!("Input path '{}' is not a regular file", input_path);
    }

    let input_abs = std::fs::canonicalize(input_path)?;
    let input_str = input_abs.to_string_lossy().to_string();

    // 1. Determine project directory (default: ~/rea-movie-artifacts/h3-projects/<stem>)
    let proj_dir: PathBuf = match opts.output_dir {
        Some(out) => PathBuf::from(out),
        None => {
            let stem = Path::new(input_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("movie_project");
            let projects_root = std::env::var("REA_MOVIE_PROJECTS").unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|home| format!("{home}/rea-movie-artifacts/h3-projects"))
                    .unwrap_or_else(|_| "/rosenas/data/rea-movie-artifacts/h3-projects".to_string())
            });
            let projects_dir = PathBuf::from(&projects_root);
            if std::fs::create_dir_all(&projects_dir).is_ok() {
                projects_dir.join(stem)
            } else {
                PathBuf::from(format!("./{}", stem))
            }
        }
    };

    std::fs::create_dir_all(&proj_dir)
        .with_context(|| format!("Failed to create project directory at {:?}", proj_dir))?;

    // Series-first default: a project under <root>/h3-projects/<name> is expected
    // to have series context at <root>/story/{world,characters}.json (see skill §3-0).
    // Without it, IDs stay locally scoped and cross-episode consistency is
    // not guaranteed. Warn and proceed; the agent asks the user for the full
    // series first and only continues partial on explicit confirmation.
    check_series_context(&proj_dir);

    let media_dir = crate::config::resolve_media_root(&proj_dir);
    std::fs::create_dir_all(&media_dir).with_context(|| {
        format!(
            "Failed to create media workspace directory at {:?}",
            media_dir
        )
    })?;

    if media_dir != proj_dir {
        let gitignore_path = proj_dir.join(".gitignore");
        if !gitignore_path.exists() {
            let _ = std::fs::write(
                &gitignore_path,
                "# Heavy video/audio assets & renders are stored in ../h3-results/\n*.mp4\n*.mkv\n*.wav\n/outputs/\n**/renders/\n",
            );
        }
    }

    // Issue 8: per-source cache at <root>/sources/<source_id>/ (probe, scene
    // cuts, subtitles, anchor frames, per-span clips). Reused across ranges
    // and projects; disabled for standalone projects outside any root.
    let source_cache = crate::sources::resolve_sources_dir(Some(&proj_dir), Path::new(input_path))
        .and_then(
            |dir| match crate::sources::SourceCache::open(&dir, Path::new(input_path)) {
                Ok(cache) => Some(cache),
                Err(e) => {
                    eprintln!(
                        "{} Warning: source cache unavailable at {:?} ({:#}); extracting without cache",
                        crate::log::stamp(),
                        dir,
                        e
                    );
                    None
                }
            },
        );
    if let Some(cache) = &source_cache {
        println!(
            "{} ==> Source cache: {} (source_id: {})",
            crate::log::stamp(),
            cache.dir.display(),
            cache.source_id
        );
    }

    let max_dur = opts.max_shot_duration.unwrap_or(8.0);
    let min_dur = opts.min_shot_duration.unwrap_or(0.0);
    let total_dur = source_cache
        .as_ref()
        .and_then(|c| c.probe(opts.force).ok())
        .map(|p| p.duration)
        .filter(|d| *d > 0.0)
        .or_else(|| probe_video_duration(&input_str).ok())
        .unwrap_or(0.0);

    // 2. rea-movie.toml
    let config_path = proj_dir.join("rea-movie.toml");
    let mut config = if config_path.exists() && !opts.force {
        ProjectConfig::load_from_file(&config_path).unwrap_or_default()
    } else {
        let mut cfg = ProjectConfig::default();
        let stem = Path::new(input_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("movie_project");
        cfg.project.name = stem.to_string();
        cfg.project.source_video = input_str.clone();
        cfg.project.duration = total_dur;
        cfg.project.max_shot_duration = max_dur;
        cfg
    };
    // Ensure nodes include spark1.local and spark2.local
    if config.pool.nodes.is_empty() {
        config.pool.nodes = vec!["spark1.local".to_string(), "spark2.local".to_string()];
    }
    config.save_to_file(&config_path)?;

    // 3. Detect scenes and shot spans
    println!(
        "{} ==> Detecting scene cuts and building shot spans (max duration: {:.1}s)...",
        crate::log::stamp(),
        max_dur
    );
    let spans =
        if total_dur > 0.0 {
            build_shot_spans_with(total_dur, opts.range, max_dur, min_dur, |s, e| {
                match &source_cache {
                    Some(cache) => cache
                        .scene_cuts(s, e, opts.force, |s, e| detect_scene_cuts(&input_str, s, e))
                        .unwrap_or_default(),
                    None => detect_scene_cuts(&input_str, s, e).unwrap_or_default(),
                }
            })?
        } else {
            build_shot_spans(&input_str, opts.range, max_dur, min_dur)?
        };
    println!(
        "{} ==> Total shots identified: {}",
        crate::log::stamp(),
        spans.len()
    );

    // 4. timeline.json setup & customization loading
    let timeline_path = proj_dir.join("timeline.json");
    let mut timeline_items = Vec::new();

    // Section 3.1 & Section 3.7: Load customization and integrate --mod as global trait
    let mut custom = crate::customize::Customization::load(&proj_dir);
    let api_key = std::env::var("MINIMAX_API_KEY").ok();
    let has_api_key = api_key
        .as_deref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);

    if let Some(m) = opts.custom_mod {
        if !m.trim().is_empty() {
            let stmt_en = if !opts.offline && has_api_key {
                match prompt::translate_mod_to_english_m3(
                    m.trim(),
                    api_key.as_deref().unwrap().trim(),
                ) {
                    Ok(en) => en,
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to translate --mod via M3 API: {:#}; using raw text",
                            e
                        );
                        m.trim().to_string()
                    }
                }
            } else {
                m.trim().to_string()
            };

            custom.traits.push(crate::customize::Trait {
                id: "custom_mod".to_string(),
                scope: "global".to_string(),
                target: "scene".to_string(),
                shots: vec![],
                statement_en: stmt_en,
                statement_ko: m.trim().to_string(),
                negative_en: None,
                negative_ko: None,
            });
        }
    }

    // 5. Process each shot
    let shots_dir = proj_dir.join("shots");
    std::fs::create_dir_all(&shots_dir)?;
    let media_shots_dir = media_dir.join("shots");
    std::fs::create_dir_all(&media_shots_dir)?;

    let subtitle_srt: Option<PathBuf> = match &source_cache {
        Some(cache) => cache.subtitle_path(opts.force),
        None => story::find_sibling_srt(&input_str),
    };
    let subtitle_cues = subtitle_srt
        .as_deref()
        .map(story::load_subtitle_cues_from)
        .unwrap_or_default();
    let has_subtitle_track = !subtitle_cues.is_empty();
    let dub_lang = opts.dub_lang.unwrap_or("ko");
    let want_ko_dub = dub_lang.trim().to_ascii_lowercase().starts_with("ko");
    let m3_key = std::env::var("MINIMAX_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());

    // Exactly-once cue assignment: each cue belongs to the single span
    // containing its start time, so rendered dialogue never repeats across
    // micro-shots after assembly.
    let span_bounds: Vec<(f64, f64)> = spans.iter().map(|sp| (sp.start_sec, sp.end_sec)).collect();
    let cue_assignment = story::assign_cues_to_spans(&subtitle_cues, &span_bounds);

    // Loop A collects per-span media + cast; loop B writes shot.toml,
    // timeline and prompts after scene grouping. SpanWork carries loop A
    // results into loop B.
    struct SpanWork {
        span_idx: usize,
        shot_id: String,
        index: usize,
        start_sec: f64,
        end_sec: f64,
        duration: f64,
        is_chained_longtake: bool,
        shot_dir: std::path::PathBuf,
        contact_sheet: std::path::PathBuf,
        peaks: Vec<f64>,
        characters: Vec<String>,
    }
    let mut work_items: Vec<SpanWork> = Vec::new();

    for (span_idx, span) in spans.iter().enumerate() {
        let shot_dir = shots_dir.join(&span.shot_id);
        std::fs::create_dir_all(&shot_dir)?;

        let refs_dir = shot_dir.join("refs");
        std::fs::create_dir_all(&refs_dir)?;

        let media_shot_dir = media_shots_dir.join(&span.shot_id);
        let media_refs_dir = media_shot_dir.join("refs");
        std::fs::create_dir_all(&media_refs_dir)?;

        let original_clip = media_refs_dir.join("original_clip.mp4");
        let original_audio = media_refs_dir.join("original_audio.wav");
        let first_frame = refs_dir.join("first_frame.jpg");
        let last_frame = refs_dir.join("last_frame.jpg");
        let contact_sheet = refs_dir.join("contact_sheet.jpg");

        let extract_paths = ShotExtractPaths {
            media_shot_dir: &media_shot_dir,
            original_clip: &original_clip,
            original_audio: &original_audio,
            first_frame: &first_frame,
            last_frame: &last_frame,
            contact_sheet: &contact_sheet,
        };

        // Extract media assets
        extract_shot_references(
            &input_str,
            span.start_sec,
            span.end_sec,
            span.duration,
            &extract_paths,
            opts.force,
            source_cache.as_ref(),
        )?;

        // Analyze audio peaks
        let peaks = audio::analyze_audio_peaks(&original_audio, span.duration).unwrap_or_default();

        // Section 3.4: Character detection via MiniMax-M3 Vision API
        let detected = if !opts.offline
            && has_api_key
            && !custom.characters.is_empty()
            && contact_sheet.exists()
        {
            match prompt::detect_characters_with_m3(
                &contact_sheet,
                &custom.characters,
                api_key.as_deref().unwrap().trim(),
            ) {
                Ok(chars) if !chars.is_empty() => chars,
                Ok(_) => vec!["S1".to_string()],
                Err(e) => {
                    eprintln!(
                        "Warning: Character detection failed for {}: {:#}; falling back to ['S1']",
                        span.shot_id, e
                    );
                    vec!["S1".to_string()]
                }
            }
        } else {
            vec!["S1".to_string()]
        };

        // Section 3.4: customization/shots/<shot_id>.toml cast takes precedence
        let final_cast = custom.cast_for(&span.shot_id, &detected);

        // Collect per-span work; shot.toml, timeline entries and prompts are
        // written in loop B after scene grouping assigns every span to a scene.
        work_items.push(SpanWork {
            span_idx,
            shot_id: span.shot_id.clone(),
            index: span.index,
            start_sec: span.start_sec,
            end_sec: span.end_sec,
            duration: span.duration,
            is_chained_longtake: span.is_chained_longtake,
            shot_dir,
            contact_sheet,
            peaks,
            characters: final_cast,
        });
    }

    if let Some(cache) = &source_cache {
        println!(
            "{} ==> Source cache assets: {} reused, {} extracted ({})",
            crate::log::stamp(),
            cache.reused_count(),
            cache.extracted_count(),
            cache.source_id
        );
    }

    // 5b. Scene grouping from visual discontinuity + dialogue silence + cap.
    let hist_dists: Vec<f64> = spans
        .iter()
        .enumerate()
        .map(|(i, span)| {
            if i == 0 {
                return 0.0;
            }
            let prev = shots_dir
                .join(&spans[i - 1].shot_id)
                .join("refs")
                .join("contact_sheet.jpg");
            let cur = shots_dir
                .join(&span.shot_id)
                .join("refs")
                .join("contact_sheet.jpg");
            crate::scenes::hist_distance_jpeg(&prev, &cur)
        })
        .collect();
    let scenes = crate::scenes::group_scenes(&span_bounds, &subtitle_cues, &hist_dists);
    println!(
        "{} ==> Grouped {} shot(s) into {} scene(s)",
        crate::log::stamp(),
        spans.len(),
        scenes.len()
    );
    let mut span_scene_id = vec![String::new(); spans.len()];
    for scene in &scenes {
        for idx in scene.span_start_idx..=scene.span_end_idx {
            if let Some(slot) = span_scene_id.get_mut(idx) {
                *slot = scene.scene_id.clone();
            }
        }
    }

    // 5c. Scene bibles: reuse when present, otherwise M3 online or
    // deterministic offline. One vision call per scene (not per shot).
    let mut bibles: std::collections::HashMap<String, crate::scenes::SceneBible> =
        std::collections::HashMap::new();
    for scene in &scenes {
        let path = crate::scenes::scenes_dir(&proj_dir).join(format!("{}.json", scene.scene_id));
        if path.is_file() && !opts.force {
            if let Ok(bible) = crate::scenes::SceneBible::load_from_file(&path) {
                bibles.insert(scene.scene_id.clone(), bible);
                continue;
            }
        }
        let mut cast: Vec<String> = work_items
            .iter()
            .filter(|w| w.span_idx >= scene.span_start_idx && w.span_idx <= scene.span_end_idx)
            .flat_map(|w| w.characters.clone())
            .collect();
        cast.sort();
        cast.dedup();
        let mut scene_cues: Vec<crate::decompile::story::SubtitleCue> = subtitle_cues
            .iter()
            .filter(|c| c.end_sec > scene.start_sec && c.start_sec < scene.end_sec)
            .cloned()
            .collect();
        // Translate the scene's own cue lines (small request like the per-shot
        // path) so the bible's dialogue_flow matches a Korean dub.
        if want_ko_dub
            && !opts.offline
            && scene_cues.iter().any(|c| !prompt::is_korean_text(&c.text))
        {
            if let Some(key) = m3_key.as_deref() {
                let texts: Vec<String> = scene_cues.iter().map(|c| c.text.clone()).collect();
                match prompt::translate_lines_to_korean_m3(&texts, key.trim()) {
                    Ok(translated) => {
                        for (cue, line) in scene_cues.iter_mut().zip(translated.into_iter()) {
                            cue.text = line;
                        }
                    }
                    Err(e) => eprintln!(
                        "Warning: scene dialogue translation failed for {} ({:#})",
                        scene.scene_id, e
                    ),
                }
            }
        }
        let bible = if !opts.offline
            && m3_key
                .as_deref()
                .map(|k| !k.trim().is_empty())
                .unwrap_or(false)
        {
            let sheets: Vec<std::path::PathBuf> = work_items
                .iter()
                .filter(|w| w.span_idx >= scene.span_start_idx && w.span_idx <= scene.span_end_idx)
                .map(|w| w.shot_dir.join("refs").join("contact_sheet.jpg"))
                .collect();
            let dialogue: Vec<(f64, String)> = scene_cues
                .iter()
                .map(|c| (c.start_sec, c.text.clone()))
                .collect();
            match prompt::generate_scene_bible_m3(
                &scene.scene_id,
                scene.start_sec,
                scene.end_sec,
                scene.span_end_idx - scene.span_start_idx + 1,
                &sheets,
                &dialogue,
                m3_key.as_deref().unwrap().trim(),
            ) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!(
                        "Warning: scene bible M3 failed for {} ({:#}); using offline bible",
                        scene.scene_id, e
                    );
                    crate::scenes::offline_bible(scene, &cast, &subtitle_cues)
                }
            }
        } else {
            crate::scenes::offline_bible(scene, &cast, &subtitle_cues)
        };
        if let Err(e) = bible.save_to_file(&path) {
            eprintln!("Warning: failed to save scene bible ({e:#})");
        }
        bibles.insert(scene.scene_id.clone(), bible);
    }

    // 5d. Loop B: shot.toml + timeline + prompts, all carrying scene context.
    // Issue 9: series world summary is loaded once and injected into every M3 call.
    let series_world_summary = load_series_world_summary(&proj_dir);
    for work in &work_items {
        let scene_id = span_scene_id
            .get(work.span_idx)
            .cloned()
            .unwrap_or_default();
        let scene_bible = bibles.get(&scene_id);

        // shot.toml
        let shot_toml_path = work.shot_dir.join("shot.toml");
        let shot_meta = if shot_toml_path.exists() && !opts.force {
            let mut meta = ShotMeta::load_from_file(&shot_toml_path).unwrap_or_else(|_| ShotMeta {
                shot_id: work.shot_id.clone(),
                start_time: work.start_sec,
                end_time: work.end_sec,
                duration: work.duration,
                anchor: "first_and_last".to_string(),
                seed: 42,
                characters: work.characters.clone(),
                audio_peaks: work.peaks.clone(),
                scene_id: scene_id.clone(),
            });
            if meta.characters.is_empty() {
                meta.characters = work.characters.clone();
                let _ = meta.save_to_file(&shot_toml_path);
            }
            if meta.scene_id.is_empty() {
                meta.scene_id = scene_id.clone();
                let _ = meta.save_to_file(&shot_toml_path);
            }
            meta
        } else {
            let meta = ShotMeta {
                shot_id: work.shot_id.clone(),
                start_time: work.start_sec,
                end_time: work.end_sec,
                duration: work.duration,
                anchor: "first_and_last".to_string(),
                seed: 42 + work.index as u64,
                characters: work.characters.clone(),
                audio_peaks: work.peaks.clone(),
                scene_id: scene_id.clone(),
            };
            meta.save_to_file(&shot_toml_path)?;
            meta
        };

        // Section 3.4: Record character cast in timeline item
        timeline_items.push(TimelineShotItem {
            shot_id: work.shot_id.clone(),
            start_sec: work.start_sec,
            end_sec: work.end_sec,
            duration: work.duration,
            is_chained_longtake: work.is_chained_longtake,
            anchor: shot_meta.anchor.clone(),
            assignee: "".to_string(),
            characters: shot_meta.characters.clone(),
            scene_id: scene_id.clone(),
        });

        // Prompts
        let ko_path = work.shot_dir.join("prompt.ko.md");
        let h3_path = work.shot_dir.join("prompt.h3.txt");
        let mut span_subtitles = cue_assignment
            .get(work.span_idx)
            .cloned()
            .unwrap_or_default();

        // Per-shot EN->KO translation when dubbing in Korean (online + M3 key).
        // Tiny per-shot requests survive cluster load spikes far better than one
        // giant project-level call. Offline keeps the deterministic table behavior.
        if want_ko_dub
            && !opts.offline
            && !span_subtitles.is_empty()
            && span_subtitles.iter().any(|l| !prompt::is_korean_text(l))
        {
            match m3_key.as_deref() {
                Some(key) => match prompt::translate_lines_to_korean_m3(
                    &span_subtitles,
                    key.trim(),
                ) {
                    Ok(translated) => span_subtitles = translated,
                    Err(e) => eprintln!(
                        "Warning: subtitle translation failed for {} ({:#}); keeping original text",
                        work.shot_id, e
                    ),
                },
                None => eprintln!(
                    "Warning: MINIMAX_API_KEY not set; keeping original subtitle text for Korean dub in {}",
                    work.shot_id
                ),
            }
        }

        // Issue 9: anchor frames for <Picture 1>/<Picture 2> injection.
        let first_frame_path = work.shot_dir.join("refs").join("first_frame.jpg");
        let last_frame_path = work.shot_dir.join("refs").join("last_frame.jpg");
        let prompt_ctx = PromptGenContext {
            shot_id: &work.shot_id,
            shot_index: work.index,
            start_sec: work.start_sec,
            end_sec: work.end_sec,
            duration: work.duration,
            is_chained_longtake: work.is_chained_longtake,
            audio_peaks: &shot_meta.audio_peaks,
            speaker: shot_meta
                .characters
                .first()
                .map(|s| s.as_str())
                .unwrap_or("S1"),
            contact_sheet_path: &work.contact_sheet,
            offline: opts.offline,
            force: opts.force,
            dub_lang,
            subtitle_lines: &span_subtitles,
            has_subtitle_track,
            custom_mod: opts.custom_mod,
            first_frame_path: if first_frame_path.is_file() {
                Some(&first_frame_path)
            } else {
                None
            },
            last_frame_path: if last_frame_path.is_file() {
                Some(&last_frame_path)
            } else {
                None
            },
            scene_bible,
            series_world_summary: series_world_summary.as_deref(),
            anchor: shot_meta.anchor.as_str(),
        };

        // Section 3.2 & Section 3.5: Resolve blocks and inject into prompts
        let blocks = custom.blocks_for(&work.shot_id, &shot_meta.characters);
        generate_prompts(&prompt_ctx, &blocks, scene_bible, &ko_path, &h3_path)?;
    }

    // Section 3.4: Save final timeline manifest with resolved character casts
    let manifest = TimelineManifest {
        shots: timeline_items,
    };
    manifest.save_to_file(&timeline_path)?;

    // 6. Story & Character assets
    generate_story_assets(
        &proj_dir,
        &media_dir,
        spans.len(),
        total_dur,
        subtitle_srt.as_deref(),
    )?;

    // 7. run.sh
    generate_run_script(&proj_dir)?;

    println!(
        "{} ==> Project decompile complete at: {:?}",
        crate::log::stamp(),
        proj_dir
    );
    if media_dir != proj_dir {
        println!(
            "{} ==> Media workspace (clips & render outputs) separated at: {:?}",
            crate::log::stamp(),
            media_dir
        );
    }

    // 8. If --run flag, invoke render and assemble
    if opts.run_after {
        println!(
            "{} ==> Running automated render and assembly (--run)...",
            crate::log::stamp()
        );
        crate::render::run_render(crate::render::RenderOptions {
            proj_dir: &proj_dir,
            shot: None,
            range: None,
            smoke: opts.smoke,
            force: opts.force,
            force_video_only: false,
            mock_h3: false,
        })?;
        crate::assemble::run_assemble(crate::assemble::AssembleOptions {
            proj_dir: &proj_dir,
            smoke: opts.smoke,
            force: opts.force,
        })?;
    }

    Ok(proj_dir)
}

#[cfg(test)]
mod series_tests {
    use super::*;

    #[test]
    fn test_series_root_for_h3_projects_layout() {
        // Canonical layout resolves to the artifacts root.
        let dir = std::env::temp_dir().join("rea-movie-series-test");
        let proj = dir.join("h3-projects").join("some_proj");
        std::fs::create_dir_all(&proj).unwrap();
        let root = series_root_for(&proj).unwrap();
        assert_eq!(root, dir);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_series_root_none_for_standalone() {
        assert!(series_root_for(Path::new("/tmp/standalone_proj")).is_none());
    }
}
