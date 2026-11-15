mod anchor;
mod assemble;
mod cli;
mod config;
// Section 3.1: Customization module
mod customize;
mod decompile;
mod log;
mod mcp;
mod pool;
mod render;
mod sync_prompt;
mod timeline;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use decompile::{run_decompile, DecompileOptions};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let normalized_args = normalize_cli_args(args);

    let cli = Cli::parse_from(normalized_args);

    match cli.command {
        Commands::Decompile(args) => {
            let opts = DecompileOptions {
                input_path: &args.input,
                output_dir: args.output.as_deref(),
                range: args.range.as_deref(),
                max_shot_duration: Some(args.max_shot_duration),
                force: args.force,
                run_after: args.run,
                smoke: args.smoke,
                offline: args.offline,
                dub_lang: Some(&args.dub_lang),
                custom_mod: args.custom_mod.as_deref(),
            };
            run_decompile(opts)?;
        }
        Commands::Render(args) => {
            let opts = render::RenderOptions {
                proj_dir: &args.proj_dir,
                shot: args.shot,
                range: args.range,
                smoke: args.smoke,
                force: args.force,
                force_video_only: args.force_video_only,
                mock_h3: args.mock_h3,
            };
            render::run_render(opts)?;
        }
        Commands::SyncPrompt(args) => {
            let opts = sync_prompt::SyncPromptOptions {
                proj_dir: &args.proj_dir,
                shot: args.shot,
                force: args.force,
            };
            sync_prompt::run_sync_prompt(opts)?;
        }
        Commands::Assemble(args) => {
            let opts = assemble::AssembleOptions {
                proj_dir: &args.proj_dir,
                smoke: args.smoke,
            };
            assemble::run_assemble(opts)?;
        }
        Commands::Pool(args) => {
            pool::run_pool_in(args.command, &args.proj_dir)?;
        }
        // Section 3.8: Customize CLI command handling
        Commands::Customize(args) => match args.command {
            cli::CustomizeCommands::Apply(apply_args) => {
                let modified =
                    customize::run_apply(&apply_args.proj_dir, apply_args.shot.as_deref())?;
                println!(
                    "{} ==> Applied customization to {} shots in {:?}",
                    crate::log::stamp(),
                    modified.len(),
                    apply_args.proj_dir
                );
            }
            cli::CustomizeCommands::Show(show_args) => {
                let summary = customize::generate_show_summary(&show_args.proj_dir)?;
                print!("{}", summary);
            }
            cli::CustomizeCommands::Anchor(anchor_args) => {
                let edited = anchor::run_anchor_edit(
                    &anchor_args.proj_dir,
                    anchor_args.shot.as_deref(),
                    anchor_args.character.as_deref(),
                    anchor_args.force,
                )?;
                println!(
                    "{} ==> Processed anchor edits for {} shot(s) in {:?}",
                    crate::log::stamp(),
                    edited.len(),
                    anchor_args.proj_dir
                );
            }
        },
        Commands::Mcp => {
            mcp::run_mcp_server()?;
        }
    }

    Ok(())
}

fn normalize_cli_args(args: Vec<String>) -> Vec<String> {
    if args.len() <= 1 {
        return args;
    }

    let first = &args[1];
    let known_subcommands = [
        "decompile",
        "render",
        "sync-prompt",
        "assemble",
        "pool",
        "customize",
        "mcp",
        "help",
        "--help",
        "-h",
        "--version",
        "-V",
    ];

    if known_subcommands.iter().any(|&cmd| cmd == first) {
        return args;
    }

    // If first argument is not a known subcommand and doesn't start with '-',
    // assume it's the input file for `decompile`.
    if !first.starts_with('-') {
        let mut new_args = Vec::with_capacity(args.len() + 1);
        new_args.push(args[0].clone());
        new_args.push("decompile".to_string());
        new_args.extend_from_slice(&args[1..]);
        return new_args;
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_cli_args() {
        let raw = vec![
            "rea-movie".to_string(),
            "robocop.mp4".to_string(),
            "-o".to_string(),
            "./proj".to_string(),
        ];
        let norm = normalize_cli_args(raw);
        assert_eq!(norm[1], "decompile");
        assert_eq!(norm[2], "robocop.mp4");
        assert_eq!(norm[3], "-o");
        assert_eq!(norm[4], "./proj");

        let raw_sub = vec![
            "rea-movie".to_string(),
            "render".to_string(),
            "--smoke".to_string(),
        ];
        let norm_sub = normalize_cli_args(raw_sub.clone());
        assert_eq!(norm_sub, raw_sub);
    }
}
