//! `bwoc ping <name>` — verify a `bwoc-agent --serve`'d agent is reachable.
//!
//! Phase 0 client for the IPC protocol started in this iter. Opens
//! `<workspace>/<agent>/.bwoc/agent.sock`, writes `PING\n`, reads one
//! line, prints it. Exit code:
//!   0  → got `PONG`
//!   1  → got something else (protocol drift)
//!   2  → workspace / agent / socket not found, or connection refused

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

use bwoc_core::workspace::AgentsRegistry;

pub struct PingArgs {
    /// Empty when `--all`. The CLI shim enforces "exactly one of name | all".
    pub name: String,
    pub workspace: Option<PathBuf>,
    /// Ping every agent in the workspace (skips entries with no live
    /// socket — those aren't running, not an error).
    pub all: bool,
}

pub fn run(args: PingArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc ping: no workspace found. Pass --workspace, set BWOC_WORKSPACE, or run from a workspace dir."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc ping: failed to read agents.toml: {e}");
            return 1;
        }
    };

    if args.all {
        return ping_all(&workspace, &registry);
    }

    let lookup_id = if args.name.starts_with("agent-") {
        args.name.clone()
    } else {
        format!("agent-{}", args.name)
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc ping: no agent named '{}' in workspace {}. Try `bwoc list`.",
            args.name,
            workspace.display()
        );
        return 2;
    };

    let marker = endpoint_marker(&workspace.join(&entry.path));
    if !marker.exists() {
        eprintln!(
            "bwoc ping: agent endpoint missing at {}. Is `bwoc-agent --serve` running in that dir?",
            marker.display()
        );
        return 2;
    }

    match ping_one(&marker, &entry.id) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(code) => code,
    }
}

/// The on-disk marker showing a daemon serves this agent: the Unix domain
/// socket itself, or — Windows, where the pipe lives in a global namespace,
/// not the filesystem — the `.bwoc/agent.pipe` name file the daemon writes on
/// start and removes on clean exit.
fn endpoint_marker(agent_dir: &std::path::Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        agent_dir.join(".bwoc/agent.pipe")
    }
    #[cfg(not(windows))]
    {
        agent_dir.join(".bwoc/agent.sock")
    }
}

/// Single-agent ping. Returns:
///   Ok(true)  → got PONG (exit 0)
///   Ok(false) → got something else (protocol drift, exit 1)
///   Err(n)    → connection failure (exit `n`, usually 2)
#[cfg(unix)]
fn ping_one(sock_path: &std::path::Path, id: &str) -> Result<bool, i32> {
    use std::os::unix::net::UnixStream;
    let mut stream = match UnixStream::connect(sock_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "bwoc ping: failed to connect to {}: {e}",
                sock_path.display()
            );
            return Err(2);
        }
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if let Err(e) = stream.write_all(b"PING\n") {
        eprintln!("bwoc ping: write failed: {e}");
        return Err(1);
    }
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if let Err(e) = reader.read_line(&mut line) {
        eprintln!("bwoc ping: read failed: {e}");
        return Err(1);
    }
    let response = line.trim();
    println!("{id} → {response}");
    Ok(response == "PONG")
}

/// Windows: PING over the agent's named pipe. `sock_path` is the Unix-shaped
/// `<agent>/.bwoc/agent.sock` the caller computed — the agent dir (its
/// grandparent) is what names the pipe.
#[cfg(windows)]
fn ping_one(sock_path: &std::path::Path, id: &str) -> Result<bool, i32> {
    let Some(agent_dir) = sock_path.parent().and_then(|p| p.parent()) else {
        eprintln!(
            "bwoc ping: cannot derive agent dir from {}",
            sock_path.display()
        );
        return Err(1);
    };
    let Some(response) = crate::livecheck::pipe_request(agent_dir, "PING") else {
        eprintln!(
            "bwoc ping: failed to connect to the named pipe for {}",
            agent_dir.display()
        );
        return Err(2);
    };
    println!("{id} → {response}");
    Ok(response == "PONG")
}

#[cfg(not(any(unix, windows)))]
fn ping_one(_sock_path: &std::path::Path, _id: &str) -> Result<bool, i32> {
    eprintln!("bwoc ping: no IPC transport for this platform");
    Err(1)
}

/// Mass ping. Walks the registry, pings every agent with a live
/// socket, summarizes. Agents with no socket (not running) are
/// labeled and counted but don't fail the run — they're just
/// stopped, not broken.
fn ping_all(workspace: &std::path::Path, registry: &AgentsRegistry) -> i32 {
    if registry.agents.is_empty() {
        println!(
            "bwoc ping --all: no agents registered in {}.",
            workspace.display()
        );
        return 0;
    }
    let mut pong = 0u32;
    let mut not_running = 0u32;
    let mut failed = 0u32;
    let mut protocol_drift = 0u32;
    println!();
    for entry in &registry.agents {
        let marker = endpoint_marker(&workspace.join(&entry.path));
        if !marker.exists() {
            println!("{} → not running", entry.id);
            not_running += 1;
            continue;
        }
        match ping_one(&marker, &entry.id) {
            Ok(true) => pong += 1,
            Ok(false) => protocol_drift += 1,
            Err(_) => failed += 1,
        }
    }
    println!();
    println!(
        "{} agent(s): {pong} PONG, {not_running} not running, {protocol_drift} protocol drift, {failed} failed",
        registry.agents.len(),
    );
    println!();
    // Exit 0 if everything that's UP responded correctly; failures
    // (connection errors / drift) → 1. "not running" is not a failure.
    if failed > 0 || protocol_drift > 0 {
        1
    } else {
        0
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
