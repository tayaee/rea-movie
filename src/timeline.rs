use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineManifest {
    pub shots: Vec<TimelineShotItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineShotItem {
    pub shot_id: String,
    pub start_sec: f64,
    pub end_sec: f64,
    pub duration: f64,
    pub is_chained_longtake: bool,
    pub anchor: String,
    #[serde(default)]
    pub assignee: String,
    // Section 3.4: Character casting stored in timeline.json
    #[serde(default)]
    pub characters: Vec<String>,
    // Scene grouping: which scene this shot belongs to (scene_XXXX).
    #[serde(default)]
    pub scene_id: String,
    // Issue 10: on-screen vs off-screen delivery + camera grammar (back-compat defaults).
    #[serde(default)]
    pub delivery_mode: String,
    #[serde(default)]
    pub shot_size: String,
    #[serde(default)]
    pub camera_angle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotMeta {
    pub shot_id: String,
    pub start_time: f64,
    pub end_time: f64,
    pub duration: f64,
    #[serde(default = "default_anchor")]
    pub anchor: String,
    #[serde(default = "default_seed")]
    pub seed: u64,
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub audio_peaks: Vec<f64>,
    // Scene grouping: which scene this shot belongs to (scene_XXXX).
    #[serde(default)]
    pub scene_id: String,
    // Issue 10: on-screen vs off-screen delivery + camera grammar (back-compat defaults).
    #[serde(default)]
    pub delivery_mode: String,
    #[serde(default)]
    pub shot_size: String,
    #[serde(default)]
    pub camera_angle: String,
}

fn default_anchor() -> String {
    "first_and_last".to_string()
}

fn default_seed() -> u64 {
    42
}

impl TimelineManifest {
    #[allow(dead_code)]
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read timeline from {:?}", path.as_ref()))?;
        let manifest: TimelineManifest = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse timeline JSON from {:?}", path.as_ref()))?;
        Ok(manifest)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize timeline to JSON")?;
        std::fs::write(path.as_ref(), content)
            .with_context(|| format!("Failed to write timeline to {:?}", path.as_ref()))?;
        Ok(())
    }
}

impl ShotMeta {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read shot.toml from {:?}", path.as_ref()))?;
        let meta: ShotMeta = toml::from_str(&content)
            .with_context(|| format!("Failed to parse shot.toml from {:?}", path.as_ref()))?;
        Ok(meta)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content =
            toml::to_string_pretty(self).context("Failed to serialize shot.toml to TOML")?;
        std::fs::write(path.as_ref(), content)
            .with_context(|| format!("Failed to write shot.toml to {:?}", path.as_ref()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shot_meta_serialization() {
        let meta = ShotMeta {
            shot_id: "shot_0001".to_string(),
            start_time: 0.0,
            end_time: 4.0,
            duration: 4.0,
            anchor: "first_and_last".to_string(),
            seed: 42,
            characters: vec!["S1".to_string()],
            audio_peaks: vec![1.2, 3.5],
            scene_id: "scene_0001".to_string(),
            delivery_mode: String::new(),
            shot_size: String::new(),
            camera_angle: String::new(),
        };
        let s = toml::to_string(&meta).unwrap();
        assert!(s.contains("first_and_last"));
        let parsed: ShotMeta = toml::from_str(&s).unwrap();
        assert_eq!(parsed.shot_id, "shot_0001");
        assert_eq!(parsed.audio_peaks, vec![1.2, 3.5]);
        // Issue 10: missing new keys default to empty (back-compat).
        let legacy = "shot_id = \"shot_0001\"\nstart_time = 0.0\nend_time = 4.0\nduration = 4.0\n";
        let parsed_legacy: ShotMeta = toml::from_str(legacy).unwrap();
        assert!(parsed_legacy.delivery_mode.is_empty());
        assert!(parsed_legacy.shot_size.is_empty());
        assert!(parsed_legacy.camera_angle.is_empty());
    }
}
