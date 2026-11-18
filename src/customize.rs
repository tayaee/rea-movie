// Section 3.1: Customization module for character casting and trait injection
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

// Section 3.1: Character definition
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Character {
    #[serde(default)]
    pub name_ko: String,
    #[serde(default)]
    pub name_en: String,
    #[serde(default)]
    pub appearance_en: String,
    #[serde(default)]
    pub appearance_ko: String,
    #[serde(default)]
    pub wardrobe_en: String,
    #[serde(default)]
    pub wardrobe_ko: String,
    #[serde(default)]
    pub face_ref: Option<String>,
    #[serde(default)]
    pub voice_ref: Option<String>,
    #[serde(default)]
    pub cap_ref: Option<String>,
    #[serde(default)]
    pub cap_scale: Option<f64>,
    #[serde(default)]
    pub cap_offset: Option<f64>,
}

impl Character {
    pub fn cap_scale(&self) -> f64 {
        self.cap_scale.unwrap_or(1.25)
    }

    pub fn cap_offset(&self) -> f64 {
        self.cap_offset.unwrap_or(-0.08)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharactersFile {
    #[serde(default)]
    pub characters: BTreeMap<String, Character>,
}

// Section 3.1: Trait definition
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Trait {
    pub id: String,
    #[serde(default = "default_global_scope")]
    pub scope: String, // "global" | "shot"
    #[serde(default)]
    pub target: String, // "character:<id>" | "subject:lead" | "scene" | "prop:<id>"
    #[serde(default)]
    pub shots: Vec<String>,
    #[serde(default)]
    pub statement_en: String,
    #[serde(default)]
    pub statement_ko: String,
    #[serde(default)]
    pub negative_en: Option<String>,
    #[serde(default)]
    pub negative_ko: Option<String>,
}

fn default_global_scope() -> String {
    "global".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraitsFile {
    #[serde(default)]
    pub traits: Vec<Trait>,
}

// Section 3.1: Shot override definition
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ShotOverride {
    #[serde(default)]
    pub kind: String, // "action_swap" | "remove" | "preserve"
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ShotCustomization {
    #[serde(default)]
    pub cast: Option<Vec<String>>,
    #[serde(default)]
    pub overrides: Vec<ShotOverride>,
}

// Section 3.1: Injected prompt block
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub en: String,
    pub ko: String,
}

// Section 3.1: Core Customization container
#[derive(Debug, Clone, Default)]
pub struct Customization {
    pub characters: BTreeMap<String, Character>,
    pub traits: Vec<Trait>,
    pub shots: BTreeMap<String, ShotCustomization>,
}

// Section 3.2 & Section 3.3: Markers
pub const BASE_BEGIN: &str = "<!-- rea-movie:base:begin -->";
pub const BASE_END: &str = "<!-- rea-movie:base:end -->";
pub const BLOCK_BEGIN_PREFIX: &str = "<!-- rea-movie:block:begin";
pub const BLOCK_END_PREFIX: &str = "<!-- rea-movie:block:end";
pub const KO_CUSTOM_SECTION: &str = "## 6. 자동 주입 커스텀 지시";

impl Customization {
    // Section 3.1: Load from project directory without failing if files are missing
    pub fn load(proj_dir: &Path) -> Customization {
        let custom_dir = proj_dir.join("customization");
        if !custom_dir.exists() {
            return Customization::default();
        }

        let mut characters = BTreeMap::new();
        let chars_path = custom_dir.join("characters.toml");
        if chars_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&chars_path) {
                if let Ok(parsed) = toml::from_str::<CharactersFile>(&content) {
                    characters = parsed.characters;
                }
            }
        }

        let mut traits = Vec::new();
        let traits_path = custom_dir.join("traits.toml");
        if traits_path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&traits_path) {
                if let Ok(parsed) = toml::from_str::<TraitsFile>(&content) {
                    traits = parsed.traits;
                }
            }
        }

        let mut shots = BTreeMap::new();
        let shots_dir = custom_dir.join("shots");
        if shots_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&shots_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                if let Ok(parsed) = toml::from_str::<ShotCustomization>(&content) {
                                    shots.insert(stem.to_string(), parsed);
                                }
                            }
                        }
                    }
                }
            }
        }

        Customization {
            characters,
            traits,
            shots,
        }
    }

    // Section 3.1: Check if any customization files or entries exist
    #[allow(dead_code)]
    pub fn has_any(&self) -> bool {
        !self.characters.is_empty() || !self.traits.is_empty() || !self.shots.is_empty()
    }

    // Section 3.1 & Section 3.4: Cast resolution with shot override precedence
    pub fn cast_for(&self, shot_id: &str, fallback: &[String]) -> Vec<String> {
        let norm = normalize_shot_key(shot_id);
        if let Some(shot_custom) = self.shots.get(&norm).or_else(|| self.shots.get(shot_id)) {
            if let Some(ref cast) = shot_custom.cast {
                return cast.clone();
            }
        }
        fallback.to_vec()
    }

    // Section 3.1 & Section 3.5: Block resolution (Character Bible -> Traits -> Shot overrides)
    pub fn blocks_for(&self, shot_id: &str, cast: &[String]) -> Vec<Block> {
        let mut blocks = Vec::new();

        // 1. Character Bible in cast order
        for char_id in cast {
            if let Some(ch) = self.characters.get(char_id) {
                let name_en = if ch.name_en.is_empty() {
                    char_id.clone()
                } else {
                    ch.name_en.clone()
                };
                let name_ko = if ch.name_ko.is_empty() {
                    char_id.clone()
                } else {
                    ch.name_ko.clone()
                };

                let mut en_parts = Vec::new();
                en_parts.push(name_en.clone());
                if !ch.appearance_en.is_empty() {
                    en_parts.push(ch.appearance_en.clone());
                }
                if !ch.wardrobe_en.is_empty() {
                    en_parts.push(format!("Wardrobe: {}", ch.wardrobe_en));
                }

                let mut ko_parts = Vec::new();
                ko_parts.push(name_ko.clone());
                if !ch.appearance_ko.is_empty() {
                    ko_parts.push(ch.appearance_ko.clone());
                }
                if !ch.wardrobe_ko.is_empty() {
                    ko_parts.push(format!("복장: {}", ch.wardrobe_ko));
                }

                blocks.push(Block {
                    id: format!("character:{}", char_id),
                    en: en_parts.join(". ") + ".",
                    ko: ko_parts.join(". ") + ".",
                });
            }
        }

        // 2. Traits (global and shot-scoped)
        let norm_shot = normalize_shot_key(shot_id);
        for tr in &self.traits {
            let applies = if tr.scope == "shot" {
                let shot_matched = tr
                    .shots
                    .iter()
                    .any(|s| normalize_shot_key(s) == norm_shot || s == shot_id);
                if !shot_matched {
                    false
                } else if let Some(target_char) = tr.target.strip_prefix("character:") {
                    cast.iter().any(|c| c == target_char)
                } else {
                    true
                }
            } else {
                // global scope
                if let Some(target_char) = tr.target.strip_prefix("character:") {
                    cast.iter().any(|c| c == target_char)
                } else {
                    true
                }
            };

            if applies {
                let mut en = tr.statement_en.clone();
                if let Some(ref neg_en) = tr.negative_en {
                    if !neg_en.is_empty() {
                        en.push_str(&format!(" (Negative constraint: {})", neg_en));
                    }
                }

                let mut ko = tr.statement_ko.clone();
                if let Some(ref neg_ko) = tr.negative_ko {
                    if !neg_ko.is_empty() {
                        ko.push_str(&format!(" (부정 제약: {})", neg_ko));
                    }
                }

                blocks.push(Block {
                    id: tr.id.clone(),
                    en,
                    ko,
                });
            }
        }

        // 3. Shot overrides
        if let Some(shot_custom) = self
            .shots
            .get(&norm_shot)
            .or_else(|| self.shots.get(shot_id))
        {
            for (idx, ov) in shot_custom.overrides.iter().enumerate() {
                let (en, ko) = match ov.kind.as_str() {
                    "action_swap" => {
                        let from = ov.from.as_deref().unwrap_or("");
                        let to = ov.to.as_deref().unwrap_or("");
                        (
                            format!("Action swap: change '{}' to '{}'.", from, to),
                            format!("동작 변경: '{}' -> '{}'.", from, to),
                        )
                    }
                    "remove" => {
                        let target = ov.target.as_deref().unwrap_or("");
                        (
                            format!("Remove element: {}.", target),
                            format!("요소 제거: {}.", target),
                        )
                    }
                    "preserve" => {
                        let target = ov.target.as_deref().unwrap_or("");
                        (
                            format!("Preserve element: {}.", target),
                            format!("요소 보존: {}.", target),
                        )
                    }
                    _ => (
                        format!("Custom override: {}.", ov.kind),
                        format!("커스텀 오버라이드: {}.", ov.kind),
                    ),
                };

                blocks.push(Block {
                    id: format!("override:{}", idx + 1),
                    en,
                    ko,
                });
            }
        }

        blocks
    }
}

// Section 3.2 & Section 3.3: Remove existing block sections
pub fn strip_blocks(content: &str) -> String {
    let mut out_lines = Vec::new();
    let mut in_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(BLOCK_BEGIN_PREFIX) {
            in_block = true;
            continue;
        }
        if trimmed.starts_with(BLOCK_END_PREFIX) {
            in_block = false;
            continue;
        }
        if !in_block {
            out_lines.push(line);
        }
    }

    out_lines.join("\n")
}

// Section 3.3: Extract base area preserving user edits
pub fn extract_base(content: &str) -> String {
    let stripped = strip_blocks(content);

    let base_inner = if let Some(start_idx) = stripped.find(BASE_BEGIN) {
        let after_begin = &stripped[start_idx + BASE_BEGIN.len()..];
        if let Some(end_idx) = after_begin.find(BASE_END) {
            after_begin[..end_idx].trim().to_string()
        } else {
            after_begin.trim().to_string()
        }
    } else {
        stripped.trim().to_string()
    };

    // Remove any trailing ## 6. section if present in ko content
    let final_clean = if let Some(ko_sec_idx) = base_inner.find(KO_CUSTOM_SECTION) {
        base_inner[..ko_sec_idx].trim().to_string()
    } else {
        base_inner
    };

    final_clean
}

// Section 3.3: Wrap content in base markers
pub fn wrap_base(base_text: &str) -> String {
    let trimmed = base_text.trim();
    if trimmed.is_empty() {
        format!("{}\n{}", BASE_BEGIN, BASE_END)
    } else {
        format!("{}\n{}\n{}", BASE_BEGIN, trimmed, BASE_END)
    }
}

// Section 3.2: Format block for English H3 prompt
fn format_block_h3(b: &Block) -> String {
    format!(
        "<!-- rea-movie:block:begin id={} -->\n{}\n<!-- rea-movie:block:end id={} -->",
        b.id,
        b.en.trim(),
        b.id
    )
}

// Section 3.2: Format block for Korean director note
fn format_block_ko(b: &Block) -> String {
    format!(
        "<!-- rea-movie:block:begin id={} -->\n- {}\n<!-- rea-movie:block:end id={} -->",
        b.id,
        b.ko.trim(),
        b.id
    )
}

// Section 3.2: Apply custom blocks to H3 prompt (idempotent, English only)
pub fn apply_to_h3(base: &str, blocks: &[Block]) -> String {
    let clean_base = extract_base(base);

    if blocks.is_empty() {
        return wrap_base(&clean_base);
    }

    let blocks_text = blocks
        .iter()
        .map(format_block_h3)
        .collect::<Vec<_>>()
        .join("\n");

    let header = "integrated_multimodal_description:";
    let mut modified = String::new();

    if let Some(header_pos) = clean_base.find(header) {
        let after_header = header_pos + header.len();
        let prefix = &clean_base[..after_header];
        let suffix = &clean_base[after_header..];

        modified.push_str(prefix);
        modified.push('\n');
        modified.push_str(&blocks_text);
        if !suffix.starts_with('\n') {
            modified.push('\n');
        }
        modified.push_str(suffix);
    } else {
        modified.push_str(&clean_base);
        if !modified.is_empty() && !modified.ends_with('\n') {
            modified.push_str("\n\n");
        }
        modified.push_str(header);
        modified.push('\n');
        modified.push_str(&blocks_text);
        modified.push('\n');
    }

    wrap_base(&modified)
}

// Section 3.2: Apply custom blocks to Korean director note (idempotent)
pub fn apply_to_ko(base: &str, blocks: &[Block]) -> String {
    let clean_base = extract_base(base);
    let wrapped = wrap_base(&clean_base);

    if blocks.is_empty() {
        return wrapped;
    }

    let blocks_text = blocks
        .iter()
        .map(format_block_ko)
        .collect::<Vec<_>>()
        .join("\n");

    format!("{}\n\n{}\n{}", wrapped, KO_CUSTOM_SECTION, blocks_text)
}

// Section 3.8: Apply customization to project files in place
pub fn run_apply(proj_dir: &Path, shot_filter: Option<&str>) -> Result<Vec<String>> {
    let custom = Customization::load(proj_dir);
    let timeline_path = proj_dir.join("timeline.json");
    let shots_dir = proj_dir.join("shots");

    let mut applied_shots = Vec::new();
    let norm_filter = shot_filter.map(normalize_shot_key);

    if timeline_path.is_file() {
        let manifest = crate::timeline::TimelineManifest::load_from_file(&timeline_path)?;
        let mut updated_manifest = manifest.clone();
        let mut cast_changed = false;
        for (idx, shot) in manifest.shots.iter().enumerate() {
            let norm_id = normalize_shot_key(&shot.shot_id);
            if let Some(ref filt) = norm_filter {
                if filt != &norm_id && filt != &shot.shot_id {
                    continue;
                }
            }

            let shot_dir = shots_dir.join(&shot.shot_id);
            apply_to_shot_dir(&shot_dir, &shot.shot_id, &custom, &shot.characters)?;
            // Keep timeline.json in sync with the resolved cast so that later
            // stages (render / anchor edit) use the same character assignment.
            let resolved = custom.cast_for(&shot.shot_id, &shot.characters);
            if updated_manifest.shots[idx].characters != resolved {
                updated_manifest.shots[idx].characters = resolved;
                cast_changed = true;
            }
            applied_shots.push(shot.shot_id.clone());
        }
        if cast_changed {
            updated_manifest.save_to_file(&timeline_path)?;
        }
    } else if shots_dir.is_dir() {
        for entry in std::fs::read_dir(&shots_dir)?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(shot_id) = path.file_name().and_then(|s| s.to_str()) {
                    let norm_id = normalize_shot_key(shot_id);
                    if let Some(ref filt) = norm_filter {
                        if filt != &norm_id && filt != shot_id {
                            continue;
                        }
                    }

                    let shot_meta_path = path.join("shot.toml");
                    let fallback_cast = if shot_meta_path.is_file() {
                        crate::timeline::ShotMeta::load_from_file(&shot_meta_path)
                            .map(|m| m.characters)
                            .unwrap_or_else(|_| vec!["S1".to_string()])
                    } else {
                        vec!["S1".to_string()]
                    };

                    apply_to_shot_dir(&path, shot_id, &custom, &fallback_cast)?;
                    applied_shots.push(shot_id.to_string());
                }
            }
        }
    }

    Ok(applied_shots)
}

fn apply_to_shot_dir(
    shot_dir: &Path,
    shot_id: &str,
    custom: &Customization,
    fallback_cast: &[String],
) -> Result<()> {
    let cast = custom.cast_for(shot_id, fallback_cast);
    let blocks = custom.blocks_for(shot_id, &cast);

    let ko_path = shot_dir.join("prompt.ko.md");
    if ko_path.is_file() {
        let old_ko = std::fs::read_to_string(&ko_path)?;
        let new_ko = apply_to_ko(&old_ko, &blocks);
        std::fs::write(&ko_path, new_ko)
            .with_context(|| format!("Failed to update {}", ko_path.display()))?;
    }

    let h3_path = shot_dir.join("prompt.h3.txt");
    if h3_path.is_file() {
        let old_h3 = std::fs::read_to_string(&h3_path)?;
        let new_h3 = apply_to_h3(&old_h3, &blocks);
        std::fs::write(&h3_path, new_h3)
            .with_context(|| format!("Failed to update {}", h3_path.display()))?;
    }

    // Re-attach the scene continuity block (idempotent replace).
    if let Ok(meta) = crate::timeline::ShotMeta::load_from_file(&shot_dir.join("shot.toml")) {
        if !meta.scene_id.is_empty() {
            let proj_dir = shot_dir
                .parent()
                .and_then(|p| p.parent())
                .unwrap_or(shot_dir);
            if let Some(bible) = crate::scenes::load_bible(proj_dir, &meta.scene_id) {
                for (path, block) in [
                    (shot_dir.join("prompt.ko.md"), crate::scenes::format_scene_block_ko(&bible)),
                    (shot_dir.join("prompt.h3.txt"), crate::scenes::format_scene_block_h3(&bible)),
                ] {
                    if path.is_file() {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let updated = crate::scenes::prepend_scene_block(&content, &block);
                            let _ = std::fs::write(&path, updated);
                        }
                    }
                }
            }
        }
    }

    // Update shot.toml characters if changed
    let meta_path = shot_dir.join("shot.toml");
    if meta_path.is_file() {
        if let Ok(mut meta) = crate::timeline::ShotMeta::load_from_file(&meta_path) {
            if meta.characters != cast {
                meta.characters = cast;
                let _ = meta.save_to_file(&meta_path);
            }
        }
    }

    Ok(())
}

// Section 3.8: Summary display of customization layer
pub fn generate_show_summary(proj_dir: &Path) -> Result<String> {
    let custom = Customization::load(proj_dir);
    let mut out = String::new();

    out.push_str("=== Customization Layer Summary ===\n\n");

    out.push_str(&format!(
        "Characters Defined: {}\n",
        custom.characters.len()
    ));
    for (id, ch) in &custom.characters {
        out.push_str(&format!(
            "  - [{}]: {} ({}) | {}\n",
            id, ch.name_ko, ch.name_en, ch.appearance_en
        ));
    }
    out.push('\n');

    out.push_str(&format!("Traits Defined: {}\n", custom.traits.len()));
    for tr in &custom.traits {
        out.push_str(&format!(
            "  - [{}] scope: {}, target: {}\n    statement_ko: {}\n    statement_en: {}\n",
            tr.id, tr.scope, tr.target, tr.statement_ko, tr.statement_en
        ));
    }
    out.push('\n');

    out.push_str(&format!("Shot Overrides: {}\n", custom.shots.len()));
    for (shot_id, s) in &custom.shots {
        let cast_str = s
            .cast
            .as_ref()
            .map(|c| c.join(", "))
            .unwrap_or_else(|| "default".to_string());
        out.push_str(&format!(
            "  - [{}] cast: [{}], overrides: {}\n",
            shot_id,
            cast_str,
            s.overrides.len()
        ));
    }

    let timeline_path = proj_dir.join("timeline.json");
    if timeline_path.is_file() {
        if let Ok(manifest) = crate::timeline::TimelineManifest::load_from_file(&timeline_path) {
            out.push_str("\nTimeline Shots Resolved Cast:\n");
            for shot in manifest.shots {
                let cast = custom.cast_for(&shot.shot_id, &shot.characters);
                out.push_str(&format!("  - {}: [{}]\n", shot.shot_id, cast.join(", ")));
            }
        }
    }

    Ok(out)
}

fn normalize_shot_key(shot_id: &str) -> String {
    let clean = shot_id.trim();
    if clean.starts_with("shot_") {
        clean.to_string()
    } else if let Ok(num) = clean.parse::<u32>() {
        format!("shot_{:04}", num)
    } else {
        clean.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_to_h3_idempotency_and_preservation() {
        let base_content = r#"For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> are fully referenced.

integrated_multimodal_description:
From 0.0s to 4.0s, camera pans right.

overall_soundscape:
Ambient room tone.

non_diegetic_music:
None."#;

        let blocks = vec![
            Block {
                id: "character:walter".to_string(),
                en: "Walter White: bald head, goatee. Wardrobe: black jacket.".to_string(),
                ko: "월터 화이트: 대머리, 콧수염. 복장: 검정 재킷.".to_string(),
            },
            Block {
                id: "kbo_cap".to_string(),
                en: "Walter wears a navy-and-red KBO baseball cap in every shot.".to_string(),
                ko: "월터는 모든 샷에서 KBO 야구모자를 착용한다.".to_string(),
            },
        ];

        let first = apply_to_h3(base_content, &blocks);
        let second = apply_to_h3(&first, &blocks);

        assert_eq!(first, second, "apply_to_h3 must be idempotent");
        assert!(first.contains("<!-- rea-movie:base:begin -->"));
        assert!(first.contains("<!-- rea-movie:base:end -->"));
        assert!(first.contains("<!-- rea-movie:block:begin id=character:walter -->"));
        assert!(first.contains("<!-- rea-movie:block:begin id=kbo_cap -->"));
        assert!(first.contains("From 0.0s to 4.0s, camera pans right."));

        // Must not contain Korean text in H3
        for c in first.chars() {
            assert!(
                !('\u{AC00}'..='\u{D7A3}').contains(&c),
                "H3 prompt must not contain Korean characters"
            );
        }
    }

    #[test]
    fn test_apply_to_ko_idempotency_and_preservation() {
        let base_ko = r#"# 샷 연출 지시서: shot_0001 (0.00s ~ 4.00s)

## 1. 화면 및 카메라 연출
- 카메라: 원본 구도를 잇는 샷
- 연출 지시: 디렉터의 고유 수정사항 보존

## 5. 배경음악
- 조용한 앰비언스"#;

        let blocks = vec![Block {
            id: "kbo_cap".to_string(),
            en: "Walter wears KBO cap.".to_string(),
            ko: "월터는 모든 샷에서 KBO 야구모자를 착용한다.".to_string(),
        }];

        let first = apply_to_ko(base_ko, &blocks);
        let second = apply_to_ko(&first, &blocks);

        assert_eq!(first, second, "apply_to_ko must be idempotent");
        assert!(first.contains("디렉터의 고유 수정사항 보존"));
        assert!(first.contains("## 6. 자동 주입 커스텀 지시"));
        assert!(first.contains("<!-- rea-movie:block:begin id=kbo_cap -->"));
        assert_eq!(
            first.matches("## 6. 자동 주입 커스텀 지시").count(),
            1,
            "Section ## 6 must not be duplicated"
        );
        assert_eq!(
            first
                .matches("<!-- rea-movie:block:begin id=kbo_cap -->")
                .count(),
            1,
            "Block must not be duplicated"
        );
    }

    #[test]
    fn test_cast_resolution_precedence() {
        let mut custom = Customization::default();
        assert!(!custom.has_any());

        let shot15 = ShotCustomization {
            cast: Some(vec!["walter".to_string()]),
            ..Default::default()
        };
        custom.shots.insert("shot_0015".to_string(), shot15);
        assert!(custom.has_any());

        let fallback = vec!["S1".to_string()];
        // Shot 15 should return override cast
        assert_eq!(custom.cast_for("shot_0015", &fallback), vec!["walter"]);
        assert_eq!(custom.cast_for("0015", &fallback), vec!["walter"]);

        // Other shots should return fallback
        assert_eq!(custom.cast_for("shot_0001", &fallback), vec!["S1"]);
    }

    #[test]
    fn test_trait_filtering_by_scope_and_target() {
        let mut custom = Customization::default();
        custom.traits.push(Trait {
            id: "walter_trait".to_string(),
            scope: "global".to_string(),
            target: "character:walter".to_string(),
            shots: vec![],
            statement_en: "Walter wears cap.".to_string(),
            statement_ko: "월터는 모자를 쓴다.".to_string(),
            negative_en: None,
            negative_ko: None,
        });
        custom.traits.push(Trait {
            id: "scene_trait".to_string(),
            scope: "global".to_string(),
            target: "scene".to_string(),
            shots: vec![],
            statement_en: "High contrast lighting.".to_string(),
            statement_ko: "강한 대비 조명.".to_string(),
            negative_en: None,
            negative_ko: None,
        });

        // Shot with walter in cast
        let blocks_walter = custom.blocks_for("shot_0001", &["walter".to_string()]);
        assert_eq!(blocks_walter.len(), 2);
        assert!(blocks_walter.iter().any(|b| b.id == "walter_trait"));
        assert!(blocks_walter.iter().any(|b| b.id == "scene_trait"));

        // Shot with mike only in cast
        let blocks_mike = custom.blocks_for("shot_0001", &["mike".to_string()]);
        assert_eq!(blocks_mike.len(), 1);
        assert!(!blocks_mike.iter().any(|b| b.id == "walter_trait"));
        assert!(blocks_mike.iter().any(|b| b.id == "scene_trait"));
    }

    #[test]
    fn test_character_prop_fields_parsing_and_defaults() {
        let toml_str = r#"
[characters.walter]
name_en = "Walter White"
appearance_en = "a completely bald middle-aged man with a goatee"
cap_ref = "assets/props/kbo_cap.png"
cap_scale = 1.30
cap_offset = -0.05

[characters.jesse]
name_en = "Jesse Pinkman"
"#;
        let file: CharactersFile = toml::from_str(toml_str).unwrap();
        let walter = &file.characters["walter"];
        assert_eq!(walter.cap_ref.as_deref(), Some("assets/props/kbo_cap.png"));
        assert_eq!(walter.cap_scale(), 1.30);
        assert_eq!(walter.cap_offset(), -0.05);

        let jesse = &file.characters["jesse"];
        assert_eq!(jesse.cap_ref, None);
        assert_eq!(jesse.cap_scale(), 1.25);
        assert_eq!(jesse.cap_offset(), -0.08);
    }
}
