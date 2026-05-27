//! `bwoc-a2a` binary — the A2A transport runner (#48 P1-serve).
//!
//! Split from `bwoc-cli` on purpose: this binary links the HTTP stack
//! (axum/tokio), which the CLI must not (the dep-quarantine invariant in
//! `bwoc-cli/src/spawn.rs`). `bwoc a2a card`/`serve` in the CLI resolve this
//! binary as a sibling and exec it — exactly how `bwoc spawn` runs the Ollama
//! `bwoc-harness` sibling.
//!
//! - `bwoc-a2a card <agent>`  — print the agent's Agent Card JSON.
//! - `bwoc-a2a serve <agent>` — run the listener (loopback-only by default).

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;
use clap::{Parser, Subcommand};

use bwoc_a2a::card::card_from_manifest;
use bwoc_a2a::serve::{ServeConfig, serve_blocking};

#[derive(Parser, Debug)]
#[command(
    name = "bwoc-a2a",
    about = "A2A protocol transport runner for BWOC agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print the agent's A2A Agent Card (JSON) derived from its manifest.
    Card {
        /// Agent name or id (the `agent-` prefix is optional).
        agent: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Advertised endpoint URL. Defaults to `http://<bind>:<port>/`.
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "127.0.0.1")]
        bind: IpAddr,
        #[arg(long, default_value_t = 41241)]
        port: u16,
    },
    /// Run the A2A HTTP listener: Agent Card at the well-known path + a
    /// JSON-RPC endpoint that drops inbound messages into the agent's inbox.
    Serve {
        /// Agent name or id (the `agent-` prefix is optional).
        agent: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Address to bind. Defaults to loopback (`127.0.0.1`); a non-loopback
        /// value warns, since the listener has no authentication yet.
        #[arg(long, default_value = "127.0.0.1")]
        bind: IpAddr,
        #[arg(long, default_value_t = 41241)]
        port: u16,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Card {
            agent,
            workspace,
            url,
            bind,
            port,
        } => run_card(&agent, workspace, url, bind, port),
        Command::Serve {
            agent,
            workspace,
            bind,
            port,
        } => run_serve(&agent, workspace, bind, port),
    };
    ExitCode::from(code)
}

fn run_card(
    agent: &str,
    workspace: Option<PathBuf>,
    url: Option<String>,
    bind: IpAddr,
    port: u16,
) -> u8 {
    let (manifest, _) = match resolve_agent(agent, workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let url = url.unwrap_or_else(|| format!("http://{}/", SocketAddr::new(bind, port)));
    let card = card_from_manifest(&manifest, &url);
    match serde_json::to_string_pretty(&card) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc-a2a card: failed to serialize card: {e}");
            1
        }
    }
}

fn run_serve(agent: &str, workspace: Option<PathBuf>, bind: IpAddr, port: u16) -> u8 {
    let (manifest, inbox_path) = match resolve_agent(agent, workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let addr = SocketAddr::new(bind, port);
    if !bind.is_loopback() {
        eprintln!(
            "bwoc-a2a serve: WARNING — binding {addr} is NOT loopback. The A2A \
             listener has no authentication yet (auth lands in a later #48 phase); \
             anyone who can reach this address can write to the agent's inbox. \
             Use 127.0.0.1 unless you front it with an authenticated proxy."
        );
    }
    let card = card_from_manifest(&manifest, &format!("http://{addr}/"));
    let agent_id = manifest.agent_id.clone();
    println!(
        "bwoc-a2a serve: agent '{agent_id}' on http://{addr}/ \
         (Agent Card at http://{addr}/.well-known/agent-card.json). Ctrl-C to stop."
    );
    match serve_blocking(ServeConfig {
        agent_id,
        inbox_path,
        card,
        addr,
    }) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc-a2a serve: listener error on {addr}: {e}");
            1
        }
    }
}

/// Resolve an agent's manifest + inbox path from the workspace registry.
/// `Err(code)` carries the process exit code after printing to stderr.
fn resolve_agent(agent: &str, workspace: Option<PathBuf>) -> Result<(Manifest, PathBuf), u8> {
    let Some(workspace) = resolve_workspace(workspace) else {
        eprintln!(
            "bwoc-a2a: no workspace found. Pass --workspace, set BWOC_WORKSPACE, \
             or run `bwoc init`."
        );
        return Err(2);
    };
    let registry = AgentsRegistry::load(&workspace).map_err(|e| {
        eprintln!("bwoc-a2a: failed to read agents.toml: {e}");
        1u8
    })?;
    let lookup_id = if agent.starts_with("agent-") {
        agent.to_string()
    } else {
        format!("agent-{agent}")
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc-a2a: no agent named '{agent}' in workspace {}. Try `bwoc list`.",
            workspace.display()
        );
        return Err(2);
    };
    let agent_dir = workspace.join(&entry.path);
    let manifest =
        Manifest::load_from_path(&agent_dir.join("config.manifest.json")).map_err(|e| {
            eprintln!("bwoc-a2a: failed to read manifest for '{agent}': {e}");
            1u8
        })?;
    Ok((manifest, agent_dir.join(".bwoc/inbox.jsonl")))
}

fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return Some(PathBuf::from(env_path));
        }
    }
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}
