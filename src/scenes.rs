use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

use crate::decompile::story::SubtitleCue;

/// Split thresholds for scene grouping (calibrated on one-fine-week EP01-v2:
/// boundary histogram distances cluster at 0.04-0.47 within a location and
/// spike to 0.52-0.62 across location changes).
pub const SILENCE_SPLIT_SECS: f64 = 3.0;
pub const HIST_SPLIT: f64 = 0.55;
pub const MAX_SCENE_SECS: f64 = 45.0;
pub const CAP_SILENCE_SECS: f64 = 0.5;
const BOUNDARY_EPS: f64 = 0.15;

#[derive(Debug, Clone)]
pub struct SceneSpan {
    pub scene_id: String,
    pub span_start_idx: usize,
    pub span_end_idx: usize,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// Group consecutive shot spans into scenes. A boundary becomes a scene split
/// when the inter-dialogue silence across it reaches `SILENCE_SPLIT_SECS`,
/// when the visual discontinuity reaches `HIST_SPLIT`, or when the running
/// scene exceeds `MAX_SCENE_SECS` at a boundary with at least `CAP_SILENCE_SECS`
/// of silence. Every span belongs to exactly one scene.
pub fn group_scenes(
    spans: &[(f64, f64)],
    cues: &[SubtitleCue],
    hist_dists: &[f64],
) -> Vec<SceneSpan> {
    let mut scenes = Vec::new();
    if spans.is_empty() {
        return scenes;
    }
    let mut scene_no = 1usize;
    let mut start_idx = 0usize;

    // Silence measured only when it STARTS at this boundary: the last
    // dialogue must have ended inside the immediately preceding span.
    // Otherwise a single long silence would split at every boundary it
    // spans instead of exactly once.
    let gap_at = |idx: usize| -> f64 {
        let boundary = spans[idx].0;
        let prev_start = spans[idx - 1].0;
        let prev_end = cues
            .iter()
            .filter(|c| c.end_sec <= boundary + BOUNDARY_EPS)
            .map(|c| c.end_sec)
            .fold(f64::NEG_INFINITY, f64::max);
        if !(prev_end > prev_start - BOUNDARY_EPS) {
            return 0.0;
        }
        let next_start = cues
            .iter()
            .filter(|c| c.start_sec >= boundary - BOUNDARY_EPS)
            .map(|c| c.start_sec)
            .fold(f64::INFINITY, f64::min);
        if prev_end.is_finite() && next_start.is_finite() {
            (next_start - prev_end).max(0.0)
        } else {
            0.0
        }
    };

    for i in 1..spans.len() {
        let silence = gap_at(i);
        let hist = hist_dists.get(i).copied().unwrap_or(0.0);
        let scene_len = spans[i - 1].1 - spans[start_idx].0;
        let split = silence >= SILENCE_SPLIT_SECS
            || hist >= HIST_SPLIT
            || (scene_len > MAX_SCENE_SECS && silence >= CAP_SILENCE_SECS);
        if split {
            scenes.push(SceneSpan {
                scene_id: format!("scene_{:04}", scene_no),
                span_start_idx: start_idx,
                span_end_idx: i - 1,
                start_sec: spans[start_idx].0,
                end_sec: spans[i - 1].1,
            });
            scene_no += 1;
            start_idx = i;
        }
    }
    scenes.push(SceneSpan {
        scene_id: format!("scene_{:04}", scene_no),
        span_start_idx: start_idx,
        span_end_idx: spans.len() - 1,
        start_sec: spans[start_idx].0,
        end_sec: spans[spans.len() - 1].1,
    });
    scenes
}

/// L1/2 histogram distance (0.0 = identical) between two JPEG stills.
/// 64x36 thumbnail, 4 bits per channel, 64 bins.
pub fn hist_distance_jpeg(a: &Path, b: &Path) -> f64 {
    let hist = |p: &Path| -> Option<[f64; 64]> {
        let img = image::open(p).ok()?.to_rgb8();
        let thumb = image::imageops::resize(&img, 64, 36, image::imageops::FilterType::Triangle);
        let mut h = [0u32; 64];
        for px in thumb.pixels() {
            let idx = ((px[0] as usize / 64) * 16)
                + ((px[1] as usize / 64) * 4)
                + (px[2] as usize / 64);
            h[idx] += 1;
        }
        let total = (64 * 36) as f64;
        Some(h.map(|c| c as f64 / total))
    };
    match (hist(a), hist(b)) {
        (Some(ha), Some(hb)) => ha.iter().zip(hb.iter()).map(|(x, y)| (x - y).abs()).sum::<f64>() / 2.0,
        _ => 0.0,
    }
}

/// M3 sometimes returns an object/array where a string belongs
/// (e.g. dialogue_flow as a list of turns). Accept anything scalar-ish and
/// stringify structured values instead of failing the whole bible.
fn de_flex_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};
    use std::fmt;
    struct Flex;
    impl<'de> Visitor<'de> for Flex {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or stringifiable value")
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<String, A::Error> {
            let mut parts = Vec::new();
            while let Some(v) = seq.next_element::<serde_json::Value>()? {
                parts.push(match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                });
            }
            Ok(parts.join("\n"))
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<String, A::Error> {
            let mut obj = serde_json::Map::new();
            while let Some((k, v)) =
                map.next_entry::<String, serde_json::Value>()?
            {
                obj.insert(k, v);
            }
            Ok(serde_json::Value::Object(obj).to_string())
        }
    }
    deserializer.deserialize_any(Flex)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneCharacter {
    #[serde(default, deserialize_with = "de_flex_string")]
    pub id: String,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub appearance: String,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub wardrobe: String,
}

/// Persistent directing context shared by every shot of one scene.
/// IDs are scene-scoped (S1, S2, ...) unless matched to the global
/// customization bible by a director.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneBible {
    #[serde(default, deserialize_with = "de_flex_string")]
    pub scene_id: String,
    #[serde(default)]
    pub start_sec: f64,
    #[serde(default)]
    pub end_sec: f64,
    #[serde(default)]
    pub shot_count: usize,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub location: String,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub time_of_day: String,
    #[serde(default)]
    pub characters: Vec<SceneCharacter>,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub spatial_map: String,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub eyeline_notes: String,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub dialogue_flow: String,
    #[serde(default, deserialize_with = "de_flex_string")]
    pub lighting: String,
}

impl SceneBible {
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize scene bible to JSON")?;
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path.as_ref(), content)
            .with_context(|| format!("Failed to write scene bible to {:?}", path.as_ref()))?;
        Ok(())
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read scene bible from {:?}", path.as_ref()))?;
        let bible: SceneBible = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse scene bible from {:?}", path.as_ref()))?;
        Ok(bible)
    }
}

pub fn scenes_dir(proj_dir: &Path) -> PathBuf {
    proj_dir.join("story").join("scenes")
}

pub fn load_bible(proj_dir: &Path, scene_id: &str) -> Option<SceneBible> {
    let path = scenes_dir(proj_dir).join(format!("{scene_id}.json"));
    if path.is_file() {
        SceneBible::load_from_file(path).ok()
    } else {
        None
    }
}

/// Deterministic offline bible: no vision API, built from timing, cast and
/// cue text. Keeps the pipeline reproducible without network.
pub fn offline_bible(
    scene: &SceneSpan,
    cast: &[String],
    cues: &[SubtitleCue],
) -> SceneBible {
    let mut ids: Vec<String> = cast.to_vec();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        ids.push("S1".to_string());
    }
    let dialogue_flow = cues
        .iter()
        .filter(|c| c.end_sec > scene.start_sec && c.start_sec < scene.end_sec)
        .map(|c| format!("[{:.1}s] {}", c.start_sec, c.text))
        .collect::<Vec<_>>()
        .join("\n");
    SceneBible {
        scene_id: scene.scene_id.clone(),
        start_sec: scene.start_sec,
        end_sec: scene.end_sec,
        shot_count: scene.span_end_idx - scene.span_start_idx + 1,
        location: "미확인 (offline)".to_string(),
        time_of_day: String::new(),
        characters: ids
            .into_iter()
            .map(|id| SceneCharacter {
                id,
                appearance: String::new(),
                wardrobe: String::new(),
            })
            .collect(),
        spatial_map: String::new(),
        eyeline_notes: String::new(),
        dialogue_flow,
        lighting: String::new(),
    }
}

const SCENE_BEGIN_PREFIX: &str = "<!-- rea-movie:scene:begin";
const SCENE_END_PREFIX: &str = "<!-- rea-movie:scene:end";

pub fn format_scene_block_ko(bible: &SceneBible) -> String {
    let mut out = format!(
        "<!-- rea-movie:scene:begin id={} -->\n## 0. 씬 컨텍스트 ({}: {:.1}s ~ {:.1}s, {}샷)\n",
        bible.scene_id, bible.scene_id, bible.start_sec, bible.end_sec, bible.shot_count
    );
    if !bible.location.is_empty() {
        out.push_str(&format!("- 장소: {}\n", bible.location));
    }
    if !bible.time_of_day.is_empty() {
        out.push_str(&format!("- 시간대: {}\n", bible.time_of_day));
    }
    for ch in &bible.characters {
        let mut desc = ch.id.clone();
        if !ch.appearance.is_empty() {
            desc.push_str(&format!(" ({})", ch.appearance));
        }
        if !ch.wardrobe.is_empty() {
            desc.push_str(&format!(" / {}", ch.wardrobe));
        }
        out.push_str(&format!("- 인물 {}: 씬 전체에서 동일한 외모·의상 유지\n", desc));
    }
    if !bible.spatial_map.is_empty() {
        out.push_str(&format!("- 공간 배치: {}\n", bible.spatial_map));
    }
    if !bible.eyeline_notes.is_empty() {
        out.push_str(&format!("- 시선: {}\n", bible.eyeline_notes));
    }
    if !bible.dialogue_flow.is_empty() {
        out.push_str(&format!("- 대사 흐름:\n{}\n", bible.dialogue_flow));
    }
    if !bible.lighting.is_empty() {
        out.push_str(&format!("- 조명: {}\n", bible.lighting));
    }
    out.push_str(&format!(
        "<!-- rea-movie:scene:end id={} -->",
        bible.scene_id
    ));
    out
}

pub fn format_scene_block_h3(bible: &SceneBible) -> String {
    let mut parts = vec![format!(
        "<!-- rea-movie:scene:begin id={} -->\nScene continuity context ({}):",
        bible.scene_id, bible.scene_id
    )];
    if !bible.location.is_empty() {
        parts.push(format!("Location: {}.", bible.location));
    }
    for ch in &bible.characters {
        let mut desc = format!("Character {} must keep the exact same face", ch.id);
        if !ch.appearance.is_empty() {
            desc.push_str(&format!(" ({})", ch.appearance));
        }
        if !ch.wardrobe.is_empty() {
            desc.push_str(&format!(" and wardrobe ({})", ch.wardrobe));
        }
        desc.push_str(" in every shot of this scene; do not redesign the person between angle changes.");
        parts.push(desc);
    }
    if !bible.spatial_map.is_empty() {
        parts.push(format!("Spatial layout: {}.", bible.spatial_map));
    }
    if !bible.eyeline_notes.is_empty() {
        parts.push(format!("Eyelines: {}.", bible.eyeline_notes));
    }
    if !bible.dialogue_flow.is_empty() {
        parts.push(format!("Dialogue flow: {}.", bible.dialogue_flow.replace('\n', " | ")));
    }
    if !bible.lighting.is_empty() {
        parts.push(format!("Lighting: keep {} constant across shots.", bible.lighting));
    }
    parts.push(format!(
        "<!-- rea-movie:scene:end id={} -->",
        bible.scene_id
    ));
    parts.join("\n")
}

/// Prepend the scene block to prompt content, replacing any previous scene
/// block first so regeneration never duplicates it.
pub fn prepend_scene_block(content: &str, block: &str) -> String {
    let stripped = strip_scene_block(content);
    if block.trim().is_empty() {
        return stripped;
    }
    if stripped.trim().is_empty() {
        return block.trim().to_string();
    }
    format!("{}\n\n{}", block.trim(), stripped.trim())
}

fn strip_scene_block(content: &str) -> String {
    let Some(start) = content.find(SCENE_BEGIN_PREFIX) else {
        return content.to_string();
    };
    let after_start = &content[start..];
    let Some(end_rel) = after_start.find(SCENE_END_PREFIX) else {
        return content.to_string();
    };
    let after_end = &after_start[end_rel..];
    let tail = after_end
        .find("-->")
        .map(|i| &after_end[i + 3..])
        .unwrap_or("");
    format!("{}{}", &content[..start], tail).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decompile::story::SubtitleCue;

    fn cue(start_sec: f64, end_sec: f64, text: &str) -> SubtitleCue {
        SubtitleCue {
            start_sec,
            end_sec,
            text: text.to_string(),
        }
    }

    #[test]
    fn test_group_scenes_silence_split() {
        // Continuous dialogue 0-10s, then 5s silence, then dialogue again.
        let spans: Vec<(f64, f64)> = (0..12).map(|i| (i as f64 * 2.0, i as f64 * 2.0 + 2.0)).collect();
        let cues = vec![
            cue(0.5, 9.5, "continuous talk"),
            cue(15.0, 17.0, "after silence"),
            cue(18.0, 23.0, "more talk"),
        ];
        let hist = vec![0.1; 12];
        let scenes = group_scenes(&spans, &cues, &hist);
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].span_end_idx, 4);
        assert_eq!(scenes[1].span_start_idx, 5);
    }

    #[test]
    fn test_group_scenes_hist_split() {
        // No dialogue at all: visual cut alone must split.
        let spans: Vec<(f64, f64)> = (0..4).map(|i| (i as f64 * 2.0, i as f64 * 2.0 + 2.0)).collect();
        let hist = vec![0.0, 0.2, 0.7, 0.15];
        let scenes = group_scenes(&spans, &[], &hist);
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].span_end_idx, 1);
        assert_eq!(scenes[1].span_start_idx, 2);
    }

    #[test]
    fn test_group_scenes_covers_every_span_once() {
        let spans: Vec<(f64, f64)> = (0..20).map(|i| (i as f64 * 1.5, i as f64 * 1.5 + 1.5)).collect();
        let cues = vec![
            cue(0.5, 12.0, "long talk"),
            cue(20.0, 29.0, "second talk"),
        ];
        let hist = vec![0.1; 20];
        let scenes = group_scenes(&spans, &cues, &hist);
        let mut covered = vec![false; 20];
        for sc in &scenes {
            for i in sc.span_start_idx..=sc.span_end_idx {
                assert!(!covered[i], "span {i} in two scenes");
                covered[i] = true;
            }
        }
        assert!(covered.iter().all(|&c| c));
    }

    #[test]
    fn test_prepend_scene_block_idempotent() {
        let bible = SceneBible {
            scene_id: "scene_0001".to_string(),
            start_sec: 0.0,
            end_sec: 10.0,
            shot_count: 3,
            location: "카페".to_string(),
            time_of_day: String::new(),
            characters: vec![SceneCharacter {
                id: "S1".to_string(),
                appearance: "긴 머리".to_string(),
                wardrobe: String::new(),
            }],
            spatial_map: String::new(),
            eyeline_notes: String::new(),
            dialogue_flow: String::new(),
            lighting: String::new(),
        };
        let block = format_scene_block_ko(&bible);
        let once = prepend_scene_block("본문", &block);
        let twice = prepend_scene_block(&once, &block);
        assert_eq!(once, twice);
        assert!(once.contains("씬 컨텍스트"));
        assert!(once.contains("본문"));
        // New bible replaces the old block instead of stacking.
        let mut bible2 = bible.clone();
        bible2.location = "사무실".to_string();
        let updated = prepend_scene_block(&once, &format_scene_block_ko(&bible2));
        assert_eq!(updated.matches("rea-movie:scene:begin").count(), 1);
        assert!(updated.contains("사무실"));
        assert!(!updated.contains("카페"));
    }

    #[test]
    fn test_bible_tolerates_structured_strings() {
        // M3 sometimes returns dialogue_flow as an array of turns or a map.
        let raw = r#"{
            "scene_id": "scene_0001",
            "location": "cafe",
            "characters": [{"id": "S1", "appearance": "long hair", "wardrobe": "blazer"}],
            "dialogue_flow": [
                {"speaker": "S2", "line": "Her!", "context": "points at portrait"},
                "S1 says nothing."
            ],
            "spatial_map": {"S1": "center-left", "S2": "off-camera"}
        }"#;
        let bible: SceneBible = serde_json::from_str(raw).unwrap();
        assert_eq!(bible.scene_id, "scene_0001");
        assert!(bible.dialogue_flow.contains("Her!"));
        assert!(bible.dialogue_flow.contains("S1 says nothing."));
        assert!(bible.spatial_map.contains("center-left"));
    }

    #[test]
    fn test_bible_serde_roundtrip() {        let bible = offline_bible(
            &SceneSpan {
                scene_id: "scene_0001".to_string(),
                span_start_idx: 0,
                span_end_idx: 2,
                start_sec: 0.0,
                end_sec: 6.0,
            },
            &["S1".to_string()],
            &[cue(0.5, 2.0, "hello")],
        );
        let json = serde_json::to_string(&bible).unwrap();
        let parsed: SceneBible = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.scene_id, "scene_0001");
        assert_eq!(parsed.characters.len(), 1);
        assert!(parsed.dialogue_flow.contains("hello"));
    }
}
