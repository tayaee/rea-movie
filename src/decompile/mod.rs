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
use scene::{build_shot_spans, probe_video_duration};
use story::generate_story_assets;

pub struct DecompileOptions<'a> {
    pub input_path: &'a str,
    pub output_dir: Option<&'a str>,
    pub range: Option<&'a str>,
    pub max_shot_duration: Option<f64>,
    pub force: bool,
    pub run_after: bool,
    pub smoke: bool,
    pub offline: bool,
    pub dub_lang: Option<&'a str>,
    pub custom_mod: Option<&'a str>,
}

pub fn run_decompile(opts: DecompileOptions) -> Result<PathBuf> {
    let input_path = opts.input_path;
    let input_meta = std::fs::metadata(input_path)
        .with_context(|| format!("Input video file '{}' does not exist or cannot be accessed", input_path))?;
    if !input_meta.is_file() {
        anyhow::bail!("Input path '{}' is not a regular file", input_path);
    }

    let input_abs = std::fs::canonicalize(input_path)?;
    let input_str = input_abs.to_string_lossy().to_string();

    // 1. Determine project directory (default: /rosenas/data/rea-movie-projects/<stem>)
    let proj_dir: PathBuf = match opts.output_dir {
        Some(out) => PathBuf::from(out),
        None => {
            let stem = Path::new(input_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("movie_project");
            let projects_root = std::env::var("REA_MOVIE_PROJECTS")
                .unwrap_or_else(|_| "/rosenas/data/rea-movie-projects".to_string());
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

    let media_dir = crate::config::resolve_media_root(&proj_dir);
    std::fs::create_dir_all(&media_dir)
        .with_context(|| format!("Failed to create media workspace directory at {:?}", media_dir))?;

    if media_dir != proj_dir {
        let gitignore_path = proj_dir.join(".gitignore");
        if !gitignore_path.exists() {
            let _ = std::fs::write(
                &gitignore_path,
                "# Heavy video/audio assets & renders are stored in ../rea-movie-workspace/\n*.mp4\n*.mkv\n*.wav\n/outputs/\n**/renders/\n",
            );
        }
    }

    let max_dur = opts.max_shot_duration.unwrap_or(8.0);
    let total_dur = probe_video_duration(&input_str).unwrap_or(0.0);

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
    println!("==> Detecting scene cuts and building shot spans (max duration: {:.1}s)...", max_dur);
    let spans = build_shot_spans(&input_str, opts.range, max_dur)?;
    println!("==> Total shots identified: {}", spans.len());

    // 4. timeline.json
    let timeline_path = proj_dir.join("timeline.json");
    let mut timeline_items = Vec::new();

    for span in &spans {
        timeline_items.push(TimelineShotItem {
            shot_id: span.shot_id.clone(),
            start_sec: span.start_sec,
            end_sec: span.end_sec,
            duration: span.duration,
            is_chained_longtake: span.is_chained_longtake,
            anchor: "first_and_last".to_string(),
            assignee: "".to_string(),
        });
    }

    let manifest = TimelineManifest {
        shots: timeline_items,
    };
    manifest.save_to_file(&timeline_path)?;

    // 5. Process each shot
    let shots_dir = proj_dir.join("shots");
    std::fs::create_dir_all(&shots_dir)?;
    let media_shots_dir = media_dir.join("shots");
    std::fs::create_dir_all(&media_shots_dir)?;

    let subtitle_cues = story::load_subtitle_cues(&input_str);
    let has_subtitle_track = !subtitle_cues.is_empty();
    let dub_lang = opts.dub_lang.unwrap_or("en");

    for span in &spans {
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
        )?;

        // Analyze audio peaks
        let peaks = audio::analyze_audio_peaks(&original_audio, span.duration).unwrap_or_default();

        // shot.toml
        let shot_toml_path = shot_dir.join("shot.toml");
        let shot_meta = if shot_toml_path.exists() && !opts.force {
            ShotMeta::load_from_file(&shot_toml_path).unwrap_or_else(|_| ShotMeta {
                shot_id: span.shot_id.clone(),
                start_time: span.start_sec,
                end_time: span.end_sec,
                duration: span.duration,
                anchor: "first_and_last".to_string(),
                seed: 42,
                characters: vec!["S1".to_string()],
                audio_peaks: peaks.clone(),
            })
        } else {
            let meta = ShotMeta {
                shot_id: span.shot_id.clone(),
                start_time: span.start_sec,
                end_time: span.end_sec,
                duration: span.duration,
                anchor: "first_and_last".to_string(),
                seed: 42 + span.index as u64,
                characters: vec!["S1".to_string()],
                audio_peaks: peaks.clone(),
            };
            meta.save_to_file(&shot_toml_path)?;
            meta
        };

        // Prompts
        let ko_path = shot_dir.join("prompt.ko.md");
        let h3_path = shot_dir.join("prompt.h3.txt");
        let span_subtitles = story::cues_for_span(&subtitle_cues, span.start_sec, span.end_sec);

        let prompt_ctx = PromptGenContext {
            shot_id: &span.shot_id,
            shot_index: span.index,
            start_sec: span.start_sec,
            end_sec: span.end_sec,
            duration: span.duration,
            is_chained_longtake: span.is_chained_longtake,
            audio_peaks: &shot_meta.audio_peaks,
            speaker: shot_meta.characters.first().map(|s| s.as_str()).unwrap_or("S1"),
            contact_sheet_path: &contact_sheet,
            offline: opts.offline,
            force: opts.force,
            dub_lang,
            subtitle_lines: &span_subtitles,
            has_subtitle_track,
            custom_mod: opts.custom_mod,
        };

        generate_prompts(&prompt_ctx, &ko_path, &h3_path)?;
    }

    // 6. Story & Character assets
    generate_story_assets(&proj_dir, &media_dir, spans.len(), total_dur, Some(&input_str))?;

    // 7. run.sh
    generate_run_script(&proj_dir)?;

    println!("==> Project decompile complete at: {:?}", proj_dir);
    if media_dir != proj_dir {
        println!("==> Media workspace (clips & render outputs) separated at: {:?}", media_dir);
    }

    // 8. If --run flag, invoke render and assemble
    if opts.run_after {
        println!("==> Running automated render and assembly (--run)...");
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
        })?;
    }

    Ok(proj_dir)
}
