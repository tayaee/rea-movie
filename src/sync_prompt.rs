use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::timeline::{ShotMeta, TimelineManifest};

pub struct SyncPromptOptions<'a> {
    pub proj_dir: &'a Path,
    pub shot: Option<String>,
    pub force: bool,
}

/// True when the line carries Hangul outside a `<d>...</d>` dialogue tag.
/// Dialogue tags are preserved verbatim; all other Korean prose must be
/// replaced with English structured direction (issue 9).
pub fn contains_hangul_outside_dialogue(line: &str) -> bool {
    // Strip all <d>...</d> spans before checking for Hangul.
    let mut rest = line;
    let mut plain = String::with_capacity(line.len());
    while let Some(start) = rest.find("<d>") {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        if let Some(end) = after.find("</d>") {
            rest = &after[end + 4..];
        } else {
            rest = after;
            break;
        }
    }
    plain.push_str(rest);
    plain.chars().any(is_hangul_char)
}

fn is_hangul_char(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
        || ('\u{1100}'..='\u{11FF}').contains(&c)
        || ('\u{3130}'..='\u{318F}').contains(&c)
}

/// Remove a prepended `<!-- rea-movie:scene:begin ... -->...<!-- rea-movie:scene:end ... -->`
/// block so the offline compiler never copies Korean scene prose into the H3
/// base (the English scene block is re-attached afterwards by
/// `compile_prompt_file`).
fn strip_scene_block_for_compile(text: &str) -> String {
    let begin_marker = "<!-- rea-movie:scene:begin";
    let end_marker = "<!-- rea-movie:scene:end";
    let mut out = text.to_string();
    loop {
        let Some(start) = out.find(begin_marker) else {
            break;
        };
        let after_start = &out[start..];
        let Some(end_rel) = after_start.find(end_marker) else {
            break;
        };
        let after_end = &after_start[end_rel..];
        let Some(close_rel) = after_end.find("-->") else {
            break;
        };
        let end_abs = start + end_rel + close_rel + 3;
        out.replace_range(start..end_abs, "");
    }
    // Drop stray scene marker lines that survived without a pair.
    out.lines()
        .filter(|l| !l.trim().starts_with("<!-- rea-movie:scene:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Clean section body lines for H3 output: keep dialogue tags verbatim,
/// drop Korean directing prose (replaced once with `fallback_en`).
fn clean_h3_body_lines(lines: &[String], fallback_en: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut dropped_korean = false;
    let mut silent_seen = false;
    for line in lines {
        if line.contains("<d>") {
            out.push(line.clone());
        } else if line.contains("rea-movie:") && line.trim().starts_with("<!--") {
            // Never copy idempotency markers into the H3 base.
            continue;
        } else if contains_hangul_outside_dialogue(line) {
            dropped_korean = true;
            if line.contains("대사 없음") {
                silent_seen = true;
            }
        } else if !line.trim().is_empty() {
            out.push(line.clone());
        }
    }
    if silent_seen {
        out.push(
            "No spoken dialogue in this shot; characters remain silent with natural ambient sound."
                .to_string(),
        );
    } else if dropped_korean {
        out.push(fallback_en.to_string());
    }
    out
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

    let ko_noscene = strip_scene_block_for_compile(ko_prompt);
    let sections = split_sections(&ko_noscene);
    let integrated_raw: Vec<String> = sections
        .iter()
        .filter(|(heading, _)| {
            !heading.contains("음향")
                && !heading.contains("Soundscape")
                && !heading.contains("배경음악")
                && !heading.contains("BGM")
        })
        .flat_map(|(_, body)| body.iter().cloned())
        .collect();

    // Issue 9: never copy Korean directing prose into the H3 base (dialogue
    // <d> tags are the only exception). Dropped Korean is replaced with a
    // single English structured direction so meaning is not silently lost.
    let integrated_lines = clean_h3_body_lines(
        &integrated_raw,
        "Maintain the established camera framing, character blocking, eyelines, and action continuity with smooth realistic motion from <Picture 1> to <Picture 2>.",
    );

    let soundscape_raw: Option<Vec<String>> = sections
        .iter()
        .find(|(heading, _)| {
            heading.contains("음향") || heading.contains("Soundscape") || heading.contains("공간음")
        })
        .map(|(_, body)| body.clone());
    let soundscape = match soundscape_raw {
        Some(body) => {
            let cleaned = clean_h3_body_lines(
                &body,
                "Natural ambience that remains spatially consistent with the described scene.",
            );
            if cleaned.is_empty() {
                "Natural ambience that remains spatially consistent with the described scene."
                    .to_string()
            } else {
                cleaned.join("\n")
            }
        }
        None => "Natural ambience that remains spatially consistent with the described scene."
            .to_string(),
    };

    let music_raw: Option<Vec<String>> = sections
        .iter()
        .find(|(heading, _)| heading.contains("배경음악") || heading.contains("BGM"))
        .map(|(_, body)| body.clone());
    let music = match music_raw {
        Some(body) => {
            let cleaned = clean_h3_body_lines(
                &body,
                "No intrusive non-diegetic music; preserve the scene ambience.",
            );
            if cleaned.is_empty() {
                "No intrusive non-diegetic music; preserve the scene ambience.".to_string()
            } else {
                cleaned.join("\n")
            }
        }
        None => "No intrusive non-diegetic music; preserve the scene ambience.".to_string(),
    };

    let integrated = if integrated_lines.is_empty() {
        "Maintain the established camera framing, character blocking, eyelines, and action continuity with smooth realistic motion from <Picture 1> to <Picture 2>.".to_string()
    } else {
        integrated_lines.join("\n")
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
            delivery_mode: String::new(),
            shot_size: String::new(),
            camera_angle: String::new(),
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
    fn compiler_drops_korean_prose_but_keeps_dialogue() {
        let ko = "# 샷 연출 지시서\n\n## 1. 화면 및 카메라 연출\n- 카메라는 원본 첫 프레임을 따라간다.\n- S1: <d>[Korean] 긴장감 넘치는 장면이다.</d>\n\n## 4. 음향 및 공간음\n- 현장 공간감이 살아있다.\n\n## 5. 배경음악\n- 절제된 시네마틱 배경음.\n";
        let compiled = compile_ko_to_h3(ko, &meta("first_and_last")).unwrap();
        assert!(compiled.contains("<Picture 1> and <Picture 2>"));
        // Dialogue tag is preserved verbatim.
        assert!(compiled.contains("<d>[Korean] 긴장감 넘치는 장면이다.</d>"));
        // Non-dialogue sections must not contain Hangul.
        for section in [
            "integrated_multimodal_description:",
            "overall_soundscape:",
            "non_diegetic_music:",
        ] {
            let start = compiled.find(section).expect("section header");
            let rest = &compiled[start..];
            let end = rest[section.len()..]
                .find("\n\n")
                .map(|i| start + section.len() + i)
                .unwrap_or(compiled.len());
            let body = &compiled[start..end];
            let body_outside_dialogue: String = {
                let mut tmp = body.to_string();
                // Remove <d>...</d> spans before checking.
                while let Some(s) = tmp.find("<d>") {
                    if let Some(e) = tmp[s..].find("</d>") {
                        tmp.replace_range(s..s + e + 4, "");
                    } else {
                        break;
                    }
                }
                tmp
            };
            assert!(
                !body_outside_dialogue
                    .chars()
                    .any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c)),
                "Hangul leaked in {}: {}",
                section,
                body
            );
        }
    }

    #[test]
    fn shot_selector_supports_single_and_range() {
        assert!(selector_matches("0005", "shot_0005"));
        assert!(selector_matches("0001-0005", "shot_0005"));
        assert!(!selector_matches("0001-0004", "shot_0005"));
    }
}
