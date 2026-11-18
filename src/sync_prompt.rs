use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::timeline::{ShotMeta, TimelineManifest};

pub struct SyncPromptOptions<'a> {
    pub proj_dir: &'a Path,
    pub shot: Option<String>,
    pub force: bool,
}

/// Compile the Korean director note into the structured English prompt consumed
/// by MiniMax-H3. This compiler is intentionally deterministic: rendering must
/// remain reproducible and must not depend on a vision API call.
pub fn compile_ko_to_h3(ko_prompt: &str, meta: &ShotMeta) -> Result<String> {
    if ko_prompt.trim().is_empty() {
        bail!("prompt.ko.md is empty");
    }

    let shot_number = meta
        .shot_id
        .rsplit('_')
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);

    let reference_line = match meta.anchor.as_str() {
        "first_and_last" => format!(
            "For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> (from [Shot {}]) are fully referenced.",
            shot_number
        ),
        "first_only" => format!(
            "For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot {}]) is fully referenced.",
            shot_number
        ),
        "ref_only" => format!(
            "For the target video, at 0.00 seconds into the target video, <Picture 1> (from [Shot {}]) is fully referenced.",
            shot_number
        ),
        "none" => format!(
            "For the target video, at 0.00 seconds into the target video, generate Shot {} from the description below.",
            shot_number
        ),
        other => bail!("Unsupported shot anchor: {other}"),
    };

    let sections = split_sections(ko_prompt);
    let integrated = sections
        .iter()
        .filter(|(heading, _)| {
            !heading.contains("음향")
                && !heading.contains("Soundscape")
                && !heading.contains("배경음악")
                && !heading.contains("BGM")
        })
        .flat_map(|(_, body)| body.iter().cloned())
        .collect::<Vec<_>>();

    let soundscape = sections
        .iter()
        .find(|(heading, _)| {
            heading.contains("음향") || heading.contains("Soundscape") || heading.contains("공간음")
        })
        .map(|(_, body)| body.join("\n"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            "Natural ambience that remains spatially consistent with the described scene."
                .to_string()
        });

    let music = sections
        .iter()
        .find(|(heading, _)| heading.contains("배경음악") || heading.contains("BGM"))
        .map(|(_, body)| body.join("\n"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            "No intrusive non-diegetic music; preserve the scene ambience.".to_string()
        });

    let integrated = if integrated.is_empty() {
        ko_prompt.trim().to_string()
    } else {
        integrated.join("\n")
    };
    // Section 3.7: Removed hardcoded "kbo"/"야구모자" branching.
    // Trait injection is handled uniformly via customization blocks.

    Ok(format!(
        "{reference_line}\n\nintegrated_multimodal_description:\nFrom 0.0s to {:.2}s, maintain the camera, character action, expression, dialogue, and environmental continuity described below.\n{integrated}\n\noverall_soundscape:\n{soundscape}\n\nnon_diegetic_music:\n{music}\n",
        meta.duration
    ))
}

// Section 3.6: Recompile prompt files with customization layer and M3 translation
pub fn compile_prompt_file(shot_dir: &Path, meta: &ShotMeta) -> Result<()> {
    let proj_dir = shot_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(shot_dir);
    let custom = crate::customize::Customization::load(proj_dir);
    let cast = custom.cast_for(&meta.shot_id, &meta.characters);
    let blocks = custom.blocks_for(&meta.shot_id, &cast);

    let ko_path = shot_dir.join("prompt.ko.md");
    let h3_path = shot_dir.join("prompt.h3.txt");

    let base_ko = if ko_path.is_file() {
        let ko_raw = std::fs::read_to_string(&ko_path)
            .with_context(|| format!("Failed to read {}", ko_path.display()))?;
        let base = crate::customize::extract_base(&ko_raw);
        let updated_ko = crate::customize::apply_to_ko(&base, &blocks);
        std::fs::write(&ko_path, updated_ko)
            .with_context(|| format!("Failed to write {}", ko_path.display()))?;
        base
    } else {
        String::new()
    };

    let api_key = std::env::var("MINIMAX_API_KEY").ok();
    let has_api_key = api_key
        .as_deref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);

    let mut compiled_new_h3 = false;
    if has_api_key && !base_ko.is_empty() {
        match crate::decompile::prompt::translate_ko_base_to_h3_m3(
            &base_ko,
            meta,
            api_key.as_deref().unwrap().trim(),
        ) {
            Ok(h3_from_m3) => {
                let base_h3 = crate::customize::extract_base(&h3_from_m3);
                let final_h3 = crate::customize::apply_to_h3(&base_h3, &blocks);
                std::fs::write(&h3_path, final_h3)
                    .with_context(|| format!("Failed to write {}", h3_path.display()))?;
                compiled_new_h3 = true;
            }
            Err(e) => {
                eprintln!(
                    "Warning: MiniMax-M3 API translation failed for shot {}: {:#}; preserving existing base",
                    meta.shot_id, e
                );
            }
        }
    }

    if !compiled_new_h3 {
        // Section 3.6: When API is unavailable, preserve existing prompt.h3.txt base and only refresh custom blocks
        if h3_path.is_file() {
            let existing_h3 = std::fs::read_to_string(&h3_path)
                .with_context(|| format!("Failed to read {}", h3_path.display()))?;
            let base_h3 = crate::customize::extract_base(&existing_h3);
            let final_h3 = crate::customize::apply_to_h3(&base_h3, &blocks);
            std::fs::write(&h3_path, final_h3)
                .with_context(|| format!("Failed to write {}", h3_path.display()))?;
        } else {
            let compiled = compile_ko_to_h3(&base_ko, meta)?;
            let base_h3 = crate::customize::extract_base(&compiled);
            let final_h3 = crate::customize::apply_to_h3(&base_h3, &blocks);
            std::fs::write(&h3_path, final_h3)
                .with_context(|| format!("Failed to write {}", h3_path.display()))?;
        }
    }

    // Re-attach the scene continuity block (idempotent replace).
    if !meta.scene_id.is_empty() {
        if let Some(bible) = crate::scenes::load_bible(proj_dir, &meta.scene_id) {
            for (path, block) in [
                (&ko_path, crate::scenes::format_scene_block_ko(&bible)),
                (&h3_path, crate::scenes::format_scene_block_h3(&bible)),
            ] {
                if path.is_file() {
                    let content = std::fs::read_to_string(path)
                        .with_context(|| format!("Failed to read {}", path.display()))?;
                    let updated = crate::scenes::prepend_scene_block(&content, &block);
                    std::fs::write(path, updated)
                        .with_context(|| format!("Failed to write {}", path.display()))?;
                }
            }
        }
    }

    Ok(())
}

pub fn run_sync_prompt(opts: SyncPromptOptions) -> Result<()> {
    let timeline_path = opts.proj_dir.join("timeline.json");
    let timeline = TimelineManifest::load_from_file(&timeline_path)
        .with_context(|| format!("Failed to load {}", timeline_path.display()))?;

    let selected = timeline
        .shots
        .iter()
        .filter(|item| {
            opts.shot
                .as_ref()
                .map(|selector| selector_matches(selector, &item.shot_id))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    if selected.is_empty() {
        bail!("No shot matched {:?}", opts.shot);
    }

    for item in selected {
        let shot_dir = opts.proj_dir.join("shots").join(&item.shot_id);
        let meta_path = shot_dir.join("shot.toml");
        let meta = ShotMeta::load_from_file(&meta_path)
            .with_context(|| format!("Failed to load {}", meta_path.display()))?;
        let h3_path = shot_dir.join("prompt.h3.txt");

        if h3_path.is_file() && !opts.force {
            println!(
                "{} ==> Skipping {} (prompt.h3.txt already exists)",
                crate::log::stamp(),
                item.shot_id
            );
            continue;
        }
        compile_prompt_file(&shot_dir, &meta)?;
        println!("{} ==> Compiled {}", crate::log::stamp(), item.shot_id);
    }
    Ok(())
}

fn split_sections(markdown: &str) -> Vec<(String, Vec<String>)> {
    let mut sections = Vec::new();
    let mut heading = String::new();
    let mut body = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            if !heading.is_empty() || !body.is_empty() {
                sections.push((heading.clone(), body.clone()));
            }
            heading = trimmed.trim_start_matches('#').trim().to_string();
            body.clear();
        } else if trimmed.starts_with('#') {
            // The title is not a useful H3 instruction.
            continue;
        } else if !trimmed.is_empty() {
            body.push(trimmed.to_string());
        }
    }

    if !heading.is_empty() || !body.is_empty() {
        sections.push((heading, body));
    }
    sections
}

fn selector_matches(selector: &str, shot_id: &str) -> bool {
    let shot_number = shot_id
        .rsplit('_')
        .next()
        .and_then(|value| value.parse::<u32>().ok());
    let selector = selector.trim().trim_start_matches("shot_");
    if let Some((start, end)) = selector.split_once('-') {
        let start = start.parse::<u32>().ok();
        let end = end.parse::<u32>().ok();
        match (start, end, shot_number) {
            (Some(start), Some(end), Some(number)) => number >= start && number <= end,
            _ => false,
        }
    } else {
        selector.parse::<u32>().ok() == shot_number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(anchor: &str) -> ShotMeta {
        ShotMeta {
            shot_id: "shot_0005".to_string(),
            start_time: 0.0,
            end_time: 4.0,
            duration: 4.0,
            anchor: anchor.to_string(),
            seed: 42,
            characters: vec!["S1".to_string()],
            audio_peaks: vec![],
            scene_id: String::new(),
        }
    }

    #[test]
    fn compiler_keeps_director_edits_and_required_sections() {
        let ko = "# Shot\n\n## 1. 화면\n- CHARACTER_EDIT_MARKER walks slowly.\n\n## 4. 음향\n- Quiet room tone.\n\n## 5. 배경음악\n- Low synth pad.\n";
        let compiled = compile_ko_to_h3(ko, &meta("first_and_last")).unwrap();
        assert!(compiled.contains("CHARACTER_EDIT_MARKER"));
        assert!(compiled.contains("integrated_multimodal_description:"));
        assert!(compiled.contains("overall_soundscape:"));
        assert!(compiled.contains("non_diegetic_music:"));
        assert!(compiled.contains("<Picture 1> and <Picture 2>"));
    }

    #[test]
    fn shot_selector_supports_single_and_range() {
        assert!(selector_matches("0005", "shot_0005"));
        assert!(selector_matches("0001-0005", "shot_0005"));
        assert!(!selector_matches("0001-0004", "shot_0005"));
    }
}
