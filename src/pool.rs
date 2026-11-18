use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::config::{PoolConfig, ProjectConfig};

pub const H3_IMAGE: &str = "vllm/vllm-omni:minimax-h3";
pub const H3_REQUIRED_OPTIONS: &[&str] = &[
    "--omni",
    "--trust-remote-code",
    "--num-gpus 1",
    "--tensor-parallel-size 1",
    "--text-encoder-tp-size 1",
    "--usp 1",
    "--ring 1",
    "--vae-patch-parallel-size 1",
    "--vae-parallel-mode tile",
    "--vae-use-tiling",
    "--quantization fp8",
    "--enforce-eager",
    "--diffusion-attention-backend CUDNN_ATTN",
];

const MIN_AVAILABLE_MEMORY_KB: u64 = 64 * 1024 * 1024;
const PORT_SCAN_LIMIT: u16 = 32;

#[derive(Debug, Subcommand)]
pub enum PoolCommands {
    /// Show current DGX Spark node pool status
    Status,
    /// Generate and deploy launcher scripts for DGX Spark nodes
    GenerateScripts,
    /// First-time setup: configure your 1-4 MiniMax-H3 nodes (interactive unless flags given)
    Setup(PoolSetupArgs),
}

#[derive(Debug, Clone, clap::Args)]
pub struct PoolSetupArgs {
    /// Comma-separated H3 node hostnames (1-4, e.g. "h3node1.local,h3node2.local").
    /// Omit for interactive prompt.
    #[arg(long)]
    pub nodes: Option<String>,
    /// Base port for dynamic endpoint scanning (default: 8000)
    #[arg(long)]
    pub base_port: Option<u16>,
    /// Shared workspace path on the nodes (default: ~/rea-movie-artifacts)
    #[arg(long)]
    pub workspace: Option<String>,
    /// Skip SSH reachability check
    #[arg(long, default_value_t = false)]
    pub no_check: bool,
}

/// Parse + validate a comma-separated node list (1-4 entries).
pub fn parse_node_list(raw: &str) -> Result<Vec<String>> {
    let nodes: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if nodes.is_empty() || nodes.len() > 4 {
        bail!(
            "Pool supports 1-4 H3 nodes, got {}: {raw}",
            nodes.len()
        );
    }
    for node in &nodes {
        validate_hostname(node)?;
    }
    Ok(nodes)
}

/// The H3 model partition that must be resident on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineMode {
    /// Handles both `fl2va` and `t2va` requests.
    Fl2va,
    /// Handles `ref2va` requests.
    Ref2va,
}

impl EngineMode {
    pub fn as_str(self) -> &'static str {
        match self {
            EngineMode::Fl2va => "fl2va",
            EngineMode::Ref2va => "ref2va",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "fl2va" | "t2va" => Ok(Self::Fl2va),
            "ref2va" => Ok(Self::Ref2va),
            other => bail!("Unsupported H3 engine mode: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub node: String,
    pub host: String,
    pub port: u16,
    pub engine: EngineMode,
    pub status: String,
    pub workspace: String,
    #[serde(default)]
    pub mem_available_kb: Option<u64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolEndpoint {
    pub host: String,
    pub port: u16,
    /// `None` means that the endpoint was discovered without node-state and may
    /// be a combined H3 service.
    pub engine: Option<EngineMode>,
}

impl PoolEndpoint {
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn supports(&self, mode: EngineMode) -> bool {
        self.engine.map(|engine| engine == mode).unwrap_or(true)
    }
}

/// Backwards-compatible entry point for callers that do not have a project
/// directory. The CLI uses `run_pool_in` so `rea-movie.toml` can be discovered.
#[allow(dead_code)]
pub fn run_pool(cmd: PoolCommands) -> Result<()> {
    run_pool_in(cmd, Path::new("."))
}

pub fn run_pool_in(cmd: PoolCommands, proj_dir: &Path) -> Result<()> {
    match cmd {
        PoolCommands::Status => {
            let config = load_pool_config(proj_dir);
            print_pool_status(&config)
        }
        PoolCommands::GenerateScripts => {
            let config = load_pool_config(proj_dir);
            generate_and_deploy_scripts(&config)
        }
        PoolCommands::Setup(args) => run_pool_setup(proj_dir, &args),
    }
}

fn load_pool_config(proj_dir: &Path) -> ProjectConfig {
    let config_path = proj_dir.join("rea-movie.toml");
    if config_path.is_file() {
        ProjectConfig::load_from_file(config_path).unwrap_or_default()
    } else {
        ProjectConfig::default()
    }
}

fn print_pool_status(config: &ProjectConfig) -> Result<()> {
    println!("{} ==> DGX Spark Pool Status", crate::log::stamp());
    println!(
        "{}   Nodes: {}",
        crate::log::stamp(),
        config.pool.nodes.join(", ")
    );
    println!(
        "{}   Default port: {} (dynamic scanning: {}..{})",
        crate::log::stamp(),
        config.pool.base_port,
        config.pool.base_port,
        config.pool.base_port.saturating_add(PORT_SCAN_LIMIT)
    );
    println!(
        "{}   Remote workspace: {}",
        crate::log::stamp(),
        config.pool.remote_workspace
    );
    println!("{}   H3 image: {}", crate::log::stamp(), H3_IMAGE);

    let control_workspace = expand_home_path(&config.pool.remote_workspace);
    let mut reachable = 0usize;
    for node in &config.pool.nodes {
        let state = read_remote_node_state(node, &config.pool.remote_workspace)
            .ok()
            .flatten()
            .or_else(|| cached_node_state(&control_workspace, node));

        match state {
            Some(state) => {
                println!(
                    "{}   - {}: {} (engine={}, port={}, mem_available_kb={})",
                    crate::log::stamp(),
                    node,
                    state.status,
                    state.engine.as_str(),
                    state.port,
                    state
                        .mem_available_kb
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                );
            }
            None => {
                let open_port = discover_open_port(node, config.pool.base_port);
                if let Some(port) = open_port {
                    println!(
                        "{}   - {}: reachable (detected port={})",
                        crate::log::stamp(),
                        node,
                        port
                    );
                    reachable += 1;
                } else {
                    println!(
                        "{}   - {}: unreachable (no node-state found)",
                        crate::log::stamp(),
                        node
                    );
                }
            }
        }
    }
    if reachable == 0 && !config.pool.nodes.is_empty() {
        println!(
            "{}   Hint: no H3 node reachable. First-time setup? Run `rea-movie pool setup --proj-dir <proj>` to configure your 1-4 H3 nodes.",
            crate::log::stamp()
        );
    }
    Ok(())
}

fn prompt_line(prompt: &str) -> Result<String> {
    use std::io::{BufRead, Write};
    print!("{prompt}");
    std::io::stdout().flush()?;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("Failed to read stdin (use --nodes/--base-port flags for non-interactive setup)")?;
    Ok(line.trim().to_string())
}

fn stdin_is_tty() -> bool {
    #[cfg(unix)]
    {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn ssh_check(node: &str) -> bool {
    Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ConnectionAttempts=1",
        ])
        .arg(node)
        .arg("true")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_pool_setup(proj_dir: &Path, args: &PoolSetupArgs) -> Result<()> {
    let mut config = load_pool_config(proj_dir);

    let nodes = match &args.nodes {
        Some(raw) => parse_node_list(raw)?,
        None => {
            if !stdin_is_tty() {
                bail!("No --nodes given and stdin is not a terminal. Example: rea-movie pool setup --nodes h3node1.local,h3node2.local");
            }
            println!("{} ==> MiniMax-H3 pool setup (1-4 nodes)", crate::log::stamp());
            let count_raw = prompt_line("How many H3 nodes [1-4] (default 1)? ")?;
            let count: usize = if count_raw.trim().is_empty() {
                1
            } else {
                count_raw
                    .trim()
                    .parse()
                    .with_context(|| format!("Invalid node count: {count_raw}"))?
            };
            if count == 0 || count > 4 {
                bail!("Pool supports 1-4 H3 nodes, got {count}");
            }
            let mut hosts = Vec::with_capacity(count);
            for i in 1..=count {
                let host = prompt_line(&format!("Host {i}/{count} (e.g. h3node{i}.local): "))?;
                if host.trim().is_empty() {
                    bail!("Host {i} is empty");
                }
                validate_hostname(host.trim())?;
                hosts.push(host.trim().to_string());
            }
            hosts
        }
    };

    if let Some(port) = args.base_port {
        config.pool.base_port = port;
    }
    if let Some(ws) = &args.workspace {
        validate_remote_workspace(ws)?;
        config.pool.remote_workspace = ws.clone();
    }
    config.pool.nodes = nodes;

    if !args.no_check {
        for node in &config.pool.nodes {
            if ssh_check(node) {
                println!("{}   - {node}: SSH OK", crate::log::stamp());
            } else {
                println!(
                    "{}   - {node}: SSH FAILED (passwordless `ssh {node}` must work; continuing anyway, run `pool generate-scripts` after fixing)",
                    crate::log::stamp()
                );
            }
        }
    }

    let config_path = proj_dir.join("rea-movie.toml");
    config.save_to_file(&config_path)?;
    println!(
        "{} ==> Pool configured: {} (base_port={}, workspace={}) -> {}",
        crate::log::stamp(),
        config.pool.nodes.join(", "),
        config.pool.base_port,
        config.pool.remote_workspace,
        config_path.display()
    );
    Ok(())
}

fn generate_and_deploy_scripts(config: &ProjectConfig) -> Result<()> {
    let workspace = expand_home_path(&config.pool.remote_workspace);
    fs::create_dir_all(&workspace)
        .with_context(|| format!("Failed to create control workspace {}", workspace.display()))?;

    let script_path = workspace.join("serve-h3.sh");
    write_script(&script_path, config.pool.base_port)?;
    make_executable(&script_path)?;

    let control_state = NodeState {
        node: "control-node".to_string(),
        host: hostname().unwrap_or_else(|| "localhost".to_string()),
        port: config.pool.base_port,
        engine: EngineMode::Fl2va,
        status: "configured".to_string(),
        workspace: config.pool.remote_workspace.clone(),
        mem_available_kb: None,
        updated_at: current_timestamp(),
    };
    write_node_state(&workspace.join("node-state.json"), &control_state)?;

    println!(
        "{} ==> DGX Spark launcher scripts generated",
        crate::log::stamp()
    );
    println!(
        "{}   Control workspace: {}",
        crate::log::stamp(),
        workspace.display()
    );
    println!(
        "{}   Script: {}",
        crate::log::stamp(),
        script_path.display()
    );

    for node in &config.pool.nodes {
        validate_hostname(node)?;
        let node_cache = workspace.join("nodes").join(node);
        fs::create_dir_all(&node_cache)?;

        let state = NodeState {
            node: node.clone(),
            host: node.clone(),
            port: config.pool.base_port,
            engine: EngineMode::Fl2va,
            status: "configured".to_string(),
            workspace: config.pool.remote_workspace.clone(),
            mem_available_kb: None,
            updated_at: current_timestamp(),
        };
        let state_path = node_cache.join("node-state.json");
        write_node_state(&state_path, &state)?;

        match deploy_to_node(
            node,
            &config.pool.remote_workspace,
            &script_path,
            &state_path,
        ) {
            Ok(()) => println!("{}   - {}: deployed", crate::log::stamp(), node),
            Err(error) => {
                println!(
                    "{}   - {}: local cache ready, deployment skipped ({error:#})",
                    crate::log::stamp(),
                    node
                );
            }
        }
    }
    Ok(())
}

fn write_script(path: &Path, base_port: u16) -> Result<()> {
    let content = serve_script_template(base_port);
    fs::write(path, content)
        .with_context(|| format!("Failed to write H3 launcher script {}", path.display()))
}

fn make_executable(path: &Path) -> Result<()> {
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn write_node_state(path: &Path, state: &NodeState) -> Result<()> {
    let content = serde_json::to_string_pretty(state)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
        .with_context(|| format!("Failed to write node state {}", path.display()))
}

fn deploy_to_node(
    node: &str,
    remote_workspace: &str,
    local_script: &Path,
    local_state: &Path,
) -> Result<()> {
    validate_remote_workspace(remote_workspace)?;
    let remote_dir = remote_workspace.trim_end_matches('/');
    if remote_dir.starts_with("/rosenas/") {
        let verify_command = format!(
            "mkdir -p -- {dir}/nodes/{node} {dir}/logs {dir}/projects && chmod 755 -- {dir}/serve-h3.sh",
            dir = shell_expanded_home_path(remote_dir),
            node = shell_quote(node),
        );
        return run_ssh(node, &verify_command);
    }

    let remote_script = format!("{remote_dir}/serve-h3.sh");
    let remote_state = format!("{remote_dir}/node-state.json");

    let mkdir_command = format!("mkdir -p -- {}", shell_expanded_home_path(remote_workspace));
    run_ssh(node, &mkdir_command)?;

    run_scp(node, local_script, &remote_script)?;
    run_scp(node, local_state, &remote_state)?;

    let chmod_command = format!(
        "chmod 755 -- {} {}",
        shell_expanded_home_path(&remote_script),
        shell_expanded_home_path(&remote_state)
    );
    run_ssh(node, &chmod_command)?;
    Ok(())
}

fn run_ssh(node: &str, command: &str) -> Result<()> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ConnectionAttempts=1",
        ])
        .arg(node)
        .arg(command)
        .output()
        .with_context(|| format!("Failed to execute ssh on {node}"))?;
    if !output.status.success() {
        bail!(
            "ssh {node} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn run_scp(node: &str, source: &Path, destination: &str) -> Result<()> {
    let target = format!("{node}:{destination}");
    let output = Command::new("scp")
        .args([
            "-q",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ConnectionAttempts=1",
        ])
        .arg(source)
        .arg(&target)
        .output()
        .with_context(|| format!("Failed to execute scp to {node}"))?;
    if !output.status.success() {
        bail!(
            "scp to {node} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn cached_node_state(workspace: &Path, node: &str) -> Option<NodeState> {
    let path = workspace.join("nodes").join(node).join("node-state.json");
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn read_remote_node_state(node: &str, remote_workspace: &str) -> Result<Option<NodeState>> {
    validate_remote_workspace(remote_workspace)?;
    let trimmed = remote_workspace.trim_end_matches('/');
    if trimmed.starts_with("/rosenas/") {
        let shared_node_path = PathBuf::from(trimmed)
            .join("nodes")
            .join(node)
            .join("node-state.json");
        if let Ok(content) = fs::read_to_string(&shared_node_path) {
            if let Ok(state) = serde_json::from_str(&content) {
                return Ok(Some(state));
            }
        }
    }
    let state_path = format!("{trimmed}/node-state.json");
    if is_local_host(node) {
        let local_path = expand_home_path(&state_path);
        let content = fs::read_to_string(local_path).ok();
        return Ok(content.and_then(|value| serde_json::from_str(&value).ok()));
    }

    let command = format!("cat -- {}", shell_expanded_home_path(&state_path));
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=3",
            "-o",
            "ConnectionAttempts=1",
        ])
        .arg(node)
        .arg(command)
        .output()
        .with_context(|| format!("Failed to read node state from {node}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let state = serde_json::from_slice(&output.stdout).ok();
    Ok(state)
}

/// Discover endpoints from cached/remote node state and, as a fallback, open
/// TCP ports starting at the configured base port.
pub fn discover_endpoints(config: &PoolConfig) -> Vec<PoolEndpoint> {
    let workspace = expand_home_path(&config.remote_workspace);
    let mut endpoints = Vec::new();

    for node in &config.nodes {
        let state = read_remote_node_state(node, &config.remote_workspace)
            .ok()
            .flatten()
            .or_else(|| cached_node_state(&workspace, node));

        if let Some(state) = state {
            if state.status != "configured" && endpoint_ready(&state.host, state.port) {
                endpoints.push(PoolEndpoint {
                    host: state.host.clone(),
                    port: state.port,
                    engine: Some(state.engine),
                });
                continue;
            }
        }

        for port in config.base_port..config.base_port.saturating_add(PORT_SCAN_LIMIT) {
            if endpoint_ready(node, port) {
                endpoints.push(PoolEndpoint {
                    host: node.clone(),
                    port,
                    engine: None,
                });
                break;
            }
        }
    }
    endpoints
}

/// Request a single resident-partition switch on a node. The launcher writes
/// the selected dynamic port to `node-state.json` before starting the server.
pub fn request_engine_switch(
    node: &str,
    remote_workspace: &str,
    base_port: u16,
    target_mode: EngineMode,
) -> Result<()> {
    validate_hostname(node)?;
    validate_remote_workspace(remote_workspace)?;
    let workspace = remote_workspace.trim_end_matches('/');
    let script = format!("{workspace}/serve-h3.sh");
    let log = format!("{workspace}/serve-h3.log");

    if is_local_host(node) {
        let local_script = expand_home_path(&script);
        if !local_script.is_file() {
            bail!("H3 launcher script not found: {}", local_script.display());
        }
        let mut command = Command::new("nohup");
        command
            .arg(&local_script)
            .arg(target_mode.as_str())
            .env("REA_MOVIE_NODE_NAME", node)
            .env("REA_MOVIE_WORKSPACE", expand_home_path(remote_workspace))
            .env("REA_MOVIE_BASE_PORT", base_port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to start local H3 engine switch on {node}"))?;
        return Ok(());
    }

    let command = format!(
        "docker rm -f rea-movie-h3-{name} >/dev/null 2>&1 || true; nohup env REA_MOVIE_NODE_NAME={node} REA_MOVIE_WORKSPACE={workspace} REA_MOVIE_BASE_PORT={base_port} {script} {mode} >>{log} 2>&1 </dev/null &",
        name = shell_quote(node),
        node = shell_quote(node),
        workspace = shell_expanded_home_path(workspace),
        base_port = base_port,
        script = shell_expanded_home_path(&script),
        mode = target_mode.as_str(),
        log = shell_expanded_home_path(&log),
    );
    run_ssh(node, &command)
}

fn discover_open_port(node: &str, base_port: u16) -> Option<u16> {
    (base_port..base_port.saturating_add(PORT_SCAN_LIMIT)).find(|&port| tcp_port_open(node, port))
}

fn endpoint_ready(host: &str, port: u16) -> bool {
    if !tcp_port_open(host, port) {
        return false;
    }
    if is_local_host(host) {
        return true;
    }
    let url = format!("http://{}:{}/health", host, port);
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .ok()
        .and_then(|client| client.get(&url).send().ok())
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
}

fn tcp_port_open(host: &str, port: u16) -> bool {
    let addresses: Vec<SocketAddr> = match (host, port).to_socket_addrs() {
        Ok(iter) => iter.collect(),
        Err(_) => return false,
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, Duration::from_millis(120)).is_ok())
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn expand_home_path(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn current_timestamp() -> String {
    chrono::Local::now().to_rfc3339()
}

fn validate_hostname(host: &str) -> Result<()> {
    if host.is_empty() || host.starts_with('-') {
        bail!("Invalid DGX Spark host: {host}");
    }
    if !host
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':'))
    {
        bail!("Invalid characters in DGX Spark host: {host}");
    }
    Ok(())
}

fn validate_remote_workspace(workspace: &str) -> Result<()> {
    if workspace.is_empty() {
        bail!("Remote workspace must not be empty");
    }
    let rest = workspace
        .strip_prefix("~/")
        .unwrap_or_else(|| workspace.strip_prefix('/').unwrap_or(workspace));
    if rest.split('/').any(|part| part == "..") {
        bail!("Remote workspace must not contain '..': {workspace}");
    }
    if !workspace
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | '~'))
    {
        bail!("Invalid characters in remote workspace: {workspace}");
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn shell_expanded_home_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        format!("\"$HOME/{}\"", rest.replace('"', "\\\""))
    } else {
        shell_quote(path)
    }
}

fn serve_script_template(base_port: u16) -> String {
    let required_options = H3_REQUIRED_OPTIONS
        .iter()
        .map(|option| format!("    {option} \\\n"))
        .collect::<String>();

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

ENGINE="${{1:-fl2va}}"
case "$ENGINE" in
  fl2va|ref2va) ;;
  *) echo "Invalid H3 engine: $ENGINE (expected fl2va or ref2va)" >&2; exit 2 ;;
esac

WORKSPACE="${{REA_MOVIE_WORKSPACE:-$HOME/rea-movie-artifacts}}"
NODE_NAME="${{REA_MOVIE_NODE_NAME:-$(hostname)}}"
BASE_PORT="${{REA_MOVIE_BASE_PORT:-{base_port}}}"
MIN_AVAILABLE_KB="${{REA_MOVIE_MIN_AVAILABLE_KB:-{min_available_kb}}}"
H3_IMAGE="${{REA_MOVIE_H3_IMAGE:-{image}}}"

mkdir -p -- "$WORKSPACE" "$WORKSPACE/nodes/$NODE_NAME" "$WORKSPACE/logs" "$WORKSPACE/projects"

MEM_AVAILABLE_KB="$(awk '/^MemAvailable:/ {{print $2}}' /proc/meminfo)"
if [[ -z "$MEM_AVAILABLE_KB" ]]; then
  echo "Unable to determine MemAvailable" >&2
  exit 1
fi
if (( MEM_AVAILABLE_KB < MIN_AVAILABLE_KB )); then
  echo "Insufficient available memory: ${{MEM_AVAILABLE_KB}} KiB (minimum ${{MIN_AVAILABLE_KB}} KiB)" >&2
  exit 1
fi

port_is_open() {{
  local candidate="$1"
  if command -v ss >/dev/null 2>&1 && ss -H -ltn "sport = :$candidate" 2>/dev/null | grep -q .; then
    return 0
  fi
  if (echo >"/dev/tcp/127.0.0.1/$candidate") >/dev/null 2>&1; then
    return 0
  fi
  return 1
}}

PORT=""
for candidate in $(seq "$BASE_PORT" $((BASE_PORT + 100))); do
  if ! port_is_open "$candidate"; then
    PORT="$candidate"
    break
  fi
done
if [[ -z "$PORT" ]]; then
  echo "No free H3 port found from $BASE_PORT" >&2
  exit 1
fi

cat >"$WORKSPACE/nodes/$NODE_NAME/node-state.json" <<EOF
{{
  "node": "$NODE_NAME",
  "host": "$NODE_NAME",
  "port": $PORT,
  "engine": "$ENGINE",
  "status": "starting",
  "workspace": "$WORKSPACE",
  "mem_available_kb": $MEM_AVAILABLE_KB,
  "updated_at": "$(date --iso-8601=seconds)"
}}
EOF
cp -f "$WORKSPACE/nodes/$NODE_NAME/node-state.json" "$WORKSPACE/node-state.json" 2>/dev/null || true

export CUDA_VISIBLE_DEVICES="${{CUDA_VISIBLE_DEVICES:-0}}"
export FLASHINFER_DISABLE_VERSION_CHECK=1
export VLLM_WORKER_MULTIPROC_METHOD=spawn
export VLLM_OMNI_VIDEO_SYNC_TIMEOUT=14400

# One resident partition per GB10 node. Unified memory keeps the weights in
# place without creating a second staging copy.
docker rm -f "rea-movie-h3-$NODE_NAME" >/dev/null 2>&1 || true
exec docker run --rm --gpus all --shm-size=8g \
  --name "rea-movie-h3-$NODE_NAME" \
  -p "${{PORT}}:8000" \
  -v "$WORKSPACE:$WORKSPACE" \
  -v "$HOME/.cache/huggingface:/root/.cache/huggingface" \
  -v "$HOME/models/MiniMax-H3:/model:ro" \
  -v "$HOME/tools/vllm-omni:/opt/vllm-omni:ro" \
  -e PYTHONPATH=/opt/vllm-omni \
  "$H3_IMAGE" \
  vllm serve /model \
  --host 0.0.0.0 \
  --port 8000 \
  --task-type "$ENGINE" \
{required_options}  --init-timeout 3600
"#,
        min_available_kb = MIN_AVAILABLE_MEMORY_KB,
        base_port = base_port,
        image = H3_IMAGE,
        required_options = required_options,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_contains_required_gb10_recipe() {
        let script = serve_script_template(8000);
        assert!(script.contains("vllm/vllm-omni:minimax-h3"));
        assert!(script.contains("-v \"$HOME/.cache/huggingface:/root/.cache/huggingface\""));
        assert!(script.contains("-v \"$HOME/models/MiniMax-H3:/model:ro\""));
        assert!(script.contains("-v \"$HOME/tools/vllm-omni:/opt/vllm-omni:ro\""));
        for option in H3_REQUIRED_OPTIONS {
            assert!(script.contains(option), "missing required option {option}");
        }
        assert!(!script.to_ascii_lowercase().contains("offload"));
        assert!(script.contains("MemAvailable"));
        assert!(script.contains("seq \"$BASE_PORT\""));

        let custom_port_script = serve_script_template(9100);
        assert!(custom_port_script.contains("REA_MOVIE_BASE_PORT:-9100"));
    }

    #[test]
    fn engine_mode_parsing() {
        assert_eq!(EngineMode::parse("fl2va").unwrap(), EngineMode::Fl2va);
        assert_eq!(EngineMode::parse("t2va").unwrap(), EngineMode::Fl2va);
        assert_eq!(EngineMode::parse("ref2va").unwrap(), EngineMode::Ref2va);
        assert!(EngineMode::parse("unknown").is_err());
    }

    #[test]
    fn remote_workspace_validation_rejects_traversal() {
        assert!(validate_remote_workspace("~/rea-movie-artifacts").is_ok());
        assert!(validate_remote_workspace("../other").is_err());
    }

    #[test]
    fn parse_node_list_accepts_one_to_four() {
        assert_eq!(parse_node_list("h3node1.local").unwrap(), vec!["h3node1.local"]);
        assert_eq!(
            parse_node_list("a.local, b.local ,c.local,d.local").unwrap(),
            vec!["a.local", "b.local", "c.local", "d.local"]
        );
        assert!(parse_node_list("").is_err());
        assert!(parse_node_list("a,b,c,d,e").is_err());
        assert!(parse_node_list("bad host!").is_err());
    }
}
