//! `bwoc a2a` — expose a local agent over the A2A protocol (#48 P1-serve).
//!
//! - `bwoc a2a card <agent>`  — print the agent's Agent Card JSON (manifest →
//!   [`bwoc_a2a::card`]). Useful for inspection and to seed a peer's registry.
//! - `bwoc a2a serve <agent>` — run the A2A HTTP listener. **Loopback-only by
//!   default** (no auth yet); a non-loopback `--bind` is allowed but warns.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use bwoc_a2a::card::card_from_manifest;
use bwoc_a2a::serve::{ServeConfig, serve_blocking};
use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;

/// Args for `bwoc a2a card`.
pub struct CardArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
}

/// Args for `bwoc a2a serve`.
pub struct ServeArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
    pub bind: IpAddr,
    pub port: u16,
}

/// Resolve an agent's manifest + inbox path from the workspace registry.
/// Returns `(manifest, inbox_path)` or an error code-bearing message printed to
/// stderr (mirrors `deep_memory_cmd`).
fn resolve_agent(agent: &str, workspace: Option<PathBuf>) -> Result<(Manifest, PathBuf), i32> {
    let Some(workspace) = resolve_workspace(workspace) else {
        eprintln!(
            "bwoc a2a: no workspace found. Pass --workspace, set BWOC_WORKSPACE, \
             or run `bwoc init`."
        );
        return Err(2);
    };
    let registry = AgentsRegistry::load(&workspace).map_err(|e| {
        eprintln!("bwoc a2a: failed to read agents.toml: {e}");
        1
    })?;
    let lookup_id = if agent.starts_with("agent-") {
        agent.to_string()
    } else {
        format!("agent-{agent}")
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc a2a: no agent named '{agent}' in workspace {}. Try `bwoc list`.",
            workspace.display()
        );
        return Err(2);
    };
    let agent_dir = workspace.join(&entry.path);
    let manifest =
        Manifest::load_from_path(&agent_dir.join("config.manifest.json")).map_err(|e| {
            eprintln!("bwoc a2a: failed to read manifest for '{agent}': {e}");
            1
        })?;
    Ok((manifest, agent_dir.join(".bwoc/inbox.jsonl")))
}

pub fn run_card(args: CardArgs) -> i32 {
    let (manifest, _) = match resolve_agent(&args.agent, args.workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };
    // The card's `url` is informational here (no live endpoint); use a loopback
    // placeholder so the printed card is self-consistent.
    let card = card_from_manifest(&manifest, "http://127.0.0.1:0/");
    match serde_json::to_string_pretty(&card) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc a2a card: failed to serialize card: {e}");
            1
        }
    }
}

pub fn run_serve(args: ServeArgs) -> i32 {
    let (manifest, inbox_path) = match resolve_agent(&args.agent, args.workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let addr = SocketAddr::new(args.bind, args.port);
    if !args.bind.is_loopback() {
        eprintln!(
            "bwoc a2a serve: WARNING — binding {addr} is NOT loopback. The A2A \
             listener has no authentication yet (auth lands in a later #48 phase); \
             anyone who can reach this address can write to the agent's inbox. \
             Use 127.0.0.1 unless you front it with an authenticated proxy."
        );
    }
    let card = card_from_manifest(&manifest, &format!("http://{addr}/"));
    let agent_id = manifest.agent_id.clone();
    println!(
        "bwoc a2a serve: agent '{agent_id}' on http://{addr}/ \
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
            eprintln!("bwoc a2a serve: listener error on {addr}: {e}");
            1
        }
    }
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
