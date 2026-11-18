use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "rea-movie",
    about = "Reverse-engineer any movie into reproducible MiniMax-H3 multi-shot project",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Reverse-engineer video file into shot-by-shot MiniMax-H3 project
    Decompile(DecompileArgs),
    /// Render shots using DGX Spark MiniMax-H3 server pool
    Render(RenderArgs),
    /// Recompile Korean director prompt (prompt.ko.md) into MiniMax-H3 prompt (prompt.h3.txt)
    SyncPrompt(SyncPromptArgs),
    /// Assemble rendered shots into full movie and side-by-side compare video
    Assemble(AssembleArgs),
    /// Manage DGX Spark server pool
    Pool(PoolArgs),
    // Section 3.8: Customize subcommand
    /// Manage prompt customization layer (characters, traits, and shot overrides)
    Customize(CustomizeArgs),
    /// Lightweight whole-series pre-analysis (probe + 5s keyframes + subtitles) into <root>/story/
    SeriesWatch(SeriesWatchArgs),
    /// Run MCP (Model Context Protocol) stdio server
    Mcp,
}

#[derive(Debug, Args)]
pub struct SeriesWatchArgs {
    /// Episode video files or directories (default: <out-root>/input-details/)
    pub inputs: Vec<String>,

    /// Directory to scan for episode videos (e.g. <root>/input-details)
    #[arg(long = "source-dir")]
    pub source_dir: Option<PathBuf>,

    /// Series root; writes <root>/story/{world.json,characters.json,timeline-days.md}
    #[arg(long = "out-root")]
    pub out_root: Option<PathBuf>,

    /// Do not call MiniMax-M3; write deterministic structured JSON/MD
    #[arg(long = "offline")]
    pub offline: bool,

    /// Rebuild story files and re-sample cached probe data
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Probe keyframe interval in seconds (default: 5.0)
    #[arg(long = "interval", default_value = "5.0")]
    pub interval: f64,
}

#[derive(Debug, Args)]
pub struct DecompileArgs {
    /// Path to input video file (.mp4, .mkv, etc.)
    pub input: String,

    /// Target project output directory (default: ./<filename_timestamp>/)
    #[arg(short = 'o', long = "output")]
    pub output: Option<String>,

    /// Specific time range to decompile (format: START-END, e.g. 00:01:00-00:02:30 or 10-25)
    #[arg(long = "range")]
    pub range: Option<String>,

    /// Maximum duration (in seconds) for a single shot before splitting (default: 8.0)
    #[arg(long = "max-shot-duration", default_value = "8.0")]
    pub max_shot_duration: f64,

    /// Minimum shot duration (in seconds); shorter micro-cuts are merged with
    /// an adjacent shot within max-shot-duration (default: 0.0 = no merging)
    #[arg(long = "min-shot-duration", default_value = "0.0")]
    pub min_shot_duration: f64,

    /// Force overwrite existing references and prompts
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Automatically trigger render and assemble after decompiling
    #[arg(long = "run")]
    pub run: bool,

    /// Use smoke profile (608x352, 10 steps) instead of prod
    #[arg(long = "smoke")]
    pub smoke: bool,

    /// Run in offline mode without calling MiniMax vision API
    #[arg(long = "offline")]
    pub offline: bool,

    /// Target dubbing language for generated dialogue (e.g. 'ko' or 'en', default: 'ko')
    #[arg(long = "dub-lang", visible_alias = "lang", default_value = "ko")]
    pub dub_lang: String,

    /// Translate original dialogue into the dub language (legacy KO dubbing).
    /// Default keeps subtitle originals verbatim (Issue 11 §3-2).
    #[arg(long = "translate-dub")]
    pub translate_dub: bool,

    /// Custom visual/character modification applied to all shots (e.g. "주인공에게 KBO 야구모자를 씌워")
    #[arg(long = "mod", visible_alias = "custom-prompt", visible_alias = "style")]
    pub custom_mod: Option<String>,
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Path to project directory (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    /// Target shot ID or range (e.g. 0005 or 0001-0005)
    #[arg(long = "shot")]
    pub shot: Option<String>,

    /// Target time range (format: START-END)
    #[arg(long = "range")]
    pub range: Option<String>,

    /// Use smoke profile (608x352, 10 steps) instead of prod
    #[arg(long = "smoke")]
    pub smoke: bool,

    /// Force overwrite and regenerate video
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Regenerate video using existing prompt.h3.txt without recompiling prompt.ko.md
    #[arg(long = "force-video-only")]
    pub force_video_only: bool,

    /// Use local mock H3 synthesis (for testing without remote GPU pool)
    #[arg(long = "mock-h3")]
    pub mock_h3: bool,

    /// Do not use remote analysis services (accepted for run-script compatibility)
    #[arg(long = "offline")]
    pub offline: bool,
}

#[derive(Debug, Args)]
pub struct SyncPromptArgs {
    /// Path to project directory (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    /// Target shot ID (e.g. 0005)
    #[arg(long = "shot")]
    pub shot: Option<String>,

    /// Force overwrite existing prompt.h3.txt
    #[arg(short = 'f', long = "force")]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct AssembleArgs {
    /// Path to project directory (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    /// Use smoke profile (608x352, 10 steps) instead of prod
    #[arg(long = "smoke")]
    pub smoke: bool,

    /// Force rebuild even when outputs are newer than all shot videos
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Also build a proof video with burned-in speaker-tagged subtitles (Issue 11 §3-3)
    #[arg(long = "proof")]
    pub proof: bool,

    /// Skip MiniMax-M3 subtitle translation (deterministic fallback only)
    #[arg(long = "offline")]
    pub offline: bool,
}

#[derive(Debug, Args)]
pub struct PoolArgs {
    /// Path to a project directory containing rea-movie.toml (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    #[command(subcommand)]
    pub command: crate::pool::PoolCommands,
}

// Section 3.8: Customize subcommand definitions
#[derive(Debug, Args)]
pub struct CustomizeArgs {
    #[command(subcommand)]
    pub command: CustomizeCommands,
}

#[derive(Debug, Subcommand)]
pub enum CustomizeCommands {
    /// Re-apply customization blocks to prompt.ko.md and prompt.h3.txt (idempotent)
    Apply(CustomizeApplyArgs),
    /// Show summary of characters, traits, and shot casts
    Show(CustomizeShowArgs),
    /// Composite character props (e.g. cap_ref) onto anchor frames
    Anchor(CustomizeAnchorArgs),
}

#[derive(Debug, Args)]
pub struct CustomizeApplyArgs {
    /// Path to project directory (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    /// Target shot ID (e.g. 0015 or shot_0015)
    #[arg(long = "shot")]
    pub shot: Option<String>,
}

#[derive(Debug, Args)]
pub struct CustomizeShowArgs {
    /// Path to project directory (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct CustomizeAnchorArgs {
    /// Path to project directory (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    /// Target shot ID (e.g. 0015 or shot_0015)
    #[arg(long = "shot")]
    pub shot: Option<String>,

    /// Target character ID (e.g. walter)
    #[arg(long = "character")]
    pub character: Option<String>,

    /// Force re-edit already edited frames
    #[arg(long = "force", default_value_t = false)]
    pub force: bool,
}
