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

    let mut integrated = if integrated.is_empty() {
        ko_prompt.trim().to_string()
    } else {
        integrated.join("\n")
    };
    if (ko_prompt.to_ascii_lowercase().contains("kbo") || ko_prompt.contains("야구모자"))
        && !integrated.contains("Korean Baseball Organization")
    {
        integrated.push_str(
            "\nIMPORTANT VISUAL MODIFICATION: The main protagonist prominently wears a navy-and-red KBO (Korean Baseball Organization) baseball cap with a bold white embroidered 'KBO' emblem on his head throughout the entire shot.",
        );
    }

    Ok(format!(
        "{reference_line}\n\nintegrated_multimodal_description:\nFrom 0.0s to {:.2}s, maintain the camera, character action, expression, dialogue, and environmental continuity described below.\n{integrated}\n\noverall_soundscape:\n{soundscape}\n\nnon_diegetic_music:\n{music}\n",
        meta.duration
    ))
}

/// Compile `prompt.ko.md` in place into `prompt.h3.txt`.
pub fn compile_prompt_file(shot_dir: &Path, meta: &ShotMeta) -> Result<()> {
    let ko_path = shot_dir.join("prompt.ko.md");
    let h3_path = shot_dir.join("prompt.h3.txt");
    let ko_prompt = std::fs::read_to_string(&ko_path)
        .with_context(|| format!("Failed to read {}", ko_path.display()))?;
    let h3_prompt = compile_ko_to_h3(&ko_prompt, meta)?;
    std::fs::write(&h3_path, h3_prompt)
        .with_context(|| format!("Failed to write {}", h3_path.display()))?;
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
                "==> Skipping {} (prompt.h3.txt already exists)",
                item.shot_id
            );
            continue;
        }
        compile_prompt_file(&shot_dir, &meta)?;
        println!("==> Compiled {}", item.shot_id);
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
