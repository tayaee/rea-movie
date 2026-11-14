use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Resolves the workspace media root for heavy video/audio files (`original_clip.mp4`,
/// `original_audio.wav`, `renders/`, `outputs/`).
///
/// When a project lives under `.../rea-movie-projects/<project_name>`, its media and
/// rendered video outputs are separated into `../rea-movie-workspace/<project_name>`
/// (`.../rea-movie-workspace/<project_name>`) so that the project directory itself remains
/// lightweight and directly committable to a Git repository without `.mkv`/`.mp4`/`.wav` binaries.
pub fn resolve_media_root(proj_dir: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(proj_dir).unwrap_or_else(|_| {
        if proj_dir.is_absolute() {
            proj_dir.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(proj_dir))
                .unwrap_or_else(|_| proj_dir.to_path_buf())
        }
    });
    if let Some(proj_name) = resolved.file_name() {
        if let Some(parent) = resolved.parent() {
            if parent.file_name().and_then(|n| n.to_str()) == Some("rea-movie-projects") {
                if let Some(grandparent) = parent.parent() {
                    return grandparent.join("rea-movie-workspace").join(proj_name);
                }
            }
        }
    }
    proj_dir.to_path_buf()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub project: ProjectMeta,
    pub pool: PoolConfig,
    pub profiles: ProfilesConfig,
    pub minimax: MinimaxConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub source_video: String,
    pub duration: f64,
    #[serde(default = "default_max_shot_duration")]
    pub max_shot_duration: f64,
}

fn default_max_shot_duration() -> f64 {
    8.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    #[serde(default = "default_nodes")]
    pub nodes: Vec<String>,
    #[serde(default = "default_base_port")]
    pub base_port: u16,
    #[serde(default = "default_remote_workspace")]
    pub remote_workspace: String,
}

fn default_nodes() -> Vec<String> {
    vec!["spark1.local".to_string(), "spark2.local".to_string()]
}

fn default_base_port() -> u16 {
    8000
}

fn default_remote_workspace() -> String {
    "/rosenas/data/rea-movie-workspace".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesConfig {
    pub prod: ProfileSettings,
    pub smoke: ProfileSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub steps: u32,
    pub flow_shift: f64,
    pub audio_flow_shift: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinimaxConfig {
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
}

fn default_api_url() -> String {
    "https://api.minimax.io/v1/chat/completions".to_string()
}

fn default_model() -> String {
    "MiniMax-M3".to_string()
}

fn default_api_key_env() -> String {
    "MINIMAX_API_KEY".to_string()
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            project: ProjectMeta {
                name: "movie_project".to_string(),
                source_video: "".to_string(),
                duration: 0.0,
                max_shot_duration: 8.0,
            },
            pool: PoolConfig {
                nodes: default_nodes(),
                base_port: default_base_port(),
                remote_workspace: default_remote_workspace(),
            },
            profiles: ProfilesConfig {
                prod: ProfileSettings {
                    width: 960,
                    height: 576,
                    fps: 24,
                    steps: 50,
                    flow_shift: 12.0,
                    audio_flow_shift: 3.0,
                },
                smoke: ProfileSettings {
                    width: 608,
                    height: 352,
                    fps: 24,
                    steps: 10,
                    flow_shift: 12.0,
                    audio_flow_shift: 3.0,
                },
            },
            minimax: MinimaxConfig {
                api_url: default_api_url(),
                model: default_model(),
                api_key_env: default_api_key_env(),
            },
        }
    }
}

impl ProjectConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config from {:?}", path.as_ref()))?;
        let config: ProjectConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {:?}", path.as_ref()))?;
        Ok(config)
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content =
            toml::to_string_pretty(self).context("Failed to serialize ProjectConfig to TOML")?;
        std::fs::write(path.as_ref(), content)
            .with_context(|| format!("Failed to write config to {:?}", path.as_ref()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_config_serialization() {
        let cfg = ProjectConfig::default();
        let serialized = toml::to_string(&cfg).unwrap();
        assert!(serialized.contains("spark1.local"));
        assert!(serialized.contains("spark2.local"));
        assert!(serialized.contains("rea-movie-workspace"));
        let deserialized: ProjectConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.pool.nodes.len(), 2);
        assert_eq!(
            deserialized.pool.remote_workspace,
            "/rosenas/data/rea-movie-workspace"
        );
    }

    #[test]
    fn test_resolve_media_root_separates_projects_and_workspace() {
        let proj = Path::new("/rosenas/data/rea-movie-projects/breakingbad-s5e7");
        let media = resolve_media_root(proj);
        assert_eq!(
            media,
            PathBuf::from("/rosenas/data/rea-movie-workspace/breakingbad-s5e7")
        );

        let standalone = Path::new("/tmp/test_movie_proj");
        assert_eq!(resolve_media_root(standalone), standalone.to_path_buf());
    }
}
