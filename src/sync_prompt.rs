use anyhow::Result;
use std::path::Path;

#[allow(dead_code)]
pub struct SyncPromptOptions<'a> {
    pub proj_dir: &'a Path,
    pub shot: Option<String>,
    pub force: bool,
}

pub fn run_sync_prompt(opts: SyncPromptOptions) -> Result<()> {
    println!("==> sync-prompt called (shot: {:?}, force: {})", opts.shot, opts.force);
    Ok(())
}
