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
    /// Run MCP (Model Context Protocol) stdio server
    Mcp,
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

    /// Target dubbing language for generated dialogue (e.g. 'ko' or 'en', default: 'en')
    #[arg(long = "dub-lang", visible_alias = "lang", default_value = "en")]
    pub dub_lang: String,

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
}

#[derive(Debug, Args)]
pub struct PoolArgs {
    /// Path to a project directory containing rea-movie.toml (default: current directory)
    #[arg(long = "proj-dir", default_value = ".")]
    pub proj_dir: PathBuf,

    #[command(subcommand)]
    pub command: crate::pool::PoolCommands,
}
