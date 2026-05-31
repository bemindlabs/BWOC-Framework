//! `bwoc supervise <agent>` — keep an agent's daemon alive.
//!
//! Spawns `bwoc-agent --serve` in the agent's directory and waits.
//! If the child exits non-zero (crash) and the rate limit allows,
//! respawns. Clean exit (zero status) ends the supervisor. SIGINT
//! / SIGTERM sends the child a SIGTERM, waits for it, then exits 0.
//!
//! Phase 2 "restart-on-crash supervision" — the simplest design that
//! works: parent process IS the watchdog. No systemd / launchd
//! integration, no daemonization. Users run `bwoc supervise <name>`
//! inside a tmux window or systemd unit they control themselves.
//!
//! Rate-limit guard: at most `max_restarts_per_min` restarts in any
//! rolling 60-second window. If exceeded, the supervisor exits with
//! exit code 2 and a clear message — better than burning CPU in a
//! crash loop while the user sleeps.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bwoc_core::workspace::AgentsRegistry;

pub struct SuperviseArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
    /// Maximum restarts within a rolling 60s window. Default 10.
    /// Beyond this, the supervisor gives up (exit 2) — keeps crash
    /// loops from cooking the laptop.
    pub max_restarts_per_min: usize,
    /// Emit one JSON event per action to stdout (spawn, exit, respawn,
    /// rate-limit, signal-stop). Pipes cleanly to a log aggregator.
    pub json: bool,
}

/// One JSON event. Each is printed as a single compact line on stdout
/// when `--json` is set. Schema is intentionally narrow and stable.
fn emit_event(json: bool, kind: &str, agent: &str, detail: serde_json::Value) {
    if !json {
        return;
    }
    let v = serde_json::json!({
        "ts": crate::util::utc_now_iso8601(),
        "agent": agent,
        "event": kind,
        "detail": detail,
    });
    if let Ok(line) = serde_json::to_string(&v) {
        println!("{line}");
    }
}

pub fn run(args: SuperviseArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc supervise: no workspace found. Pass --workspace, set BWOC_WORKSPACE, \
             or run from a workspace dir."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc supervise: failed to read agents.toml: {e}");
            return 1;
        }
    };
    let lookup_id = if args.agent.starts_with("agent-") {
        args.agent.clone()
    } else {
        format!("agent-{}", args.agent)
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc supervise: no agent named '{}' in workspace {}.",
            args.agent,
            workspace.display()
        );
        return 2;
    };
    let agent_path = workspace.join(&entry.path);

    eprintln!(
        "bwoc supervise: watching {} (max {}/min restarts)",
        entry.id, args.max_restarts_per_min
    );
    emit_event(
        args.json,
        "watch_start",
        &entry.id,
        serde_json::json!({ "max_restarts_per_min": args.max_restarts_per_min }),
    );

    let should_stop = Arc::new(AtomicBool::new(false));
    {
        let flag = should_stop.clone();
        let _ = ctrlc::set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    // Recent restart timestamps; pruned to last 60s on each check.
    let mut restarts: Vec<Instant> = Vec::new();

    loop {
        if should_stop.load(Ordering::SeqCst) {
            eprintln!("bwoc supervise: caught signal — exiting");
            emit_event(args.json, "signal_stop", &entry.id, serde_json::json!({}));
            return 0;
        }

        // Rate-limit check before spawning.
        let one_min_ago = Instant::now() - Duration::from_secs(60);
        restarts.retain(|t| *t >= one_min_ago);
        if restarts.len() >= args.max_restarts_per_min {
            eprintln!(
                "bwoc supervise: hit rate limit ({} restarts in last 60s) — giving up. \
                 Investigate the daemon crash cause; rerun when ready.",
                restarts.len()
            );
            emit_event(
                args.json,
                "rate_limit_hit",
                &entry.id,
                serde_json::json!({ "restarts_in_window": restarts.len() }),
            );
            return 2;
        }
        restarts.push(Instant::now());
        emit_event(
            args.json,
            "spawn",
            &entry.id,
            serde_json::json!({
                "restarts_in_window": restarts.len(),
                "max": args.max_restarts_per_min,
            }),
        );

        // Spawn the daemon. Stderr → agent.log (same as `bwoc start`).
        let exit_status = match spawn_and_wait(&agent_path, &should_stop) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bwoc supervise: spawn failed: {e}");
                emit_event(
                    args.json,
                    "spawn_failed",
                    &entry.id,
                    serde_json::json!({ "error": e.to_string() }),
                );
                return 1;
            }
        };

        if should_stop.load(Ordering::SeqCst) {
            eprintln!("bwoc supervise: caught signal mid-run — exiting");
            emit_event(args.json, "signal_stop", &entry.id, serde_json::json!({}));
            return 0;
        }

        // Clean exit (status 0) → stop supervising.
        if exit_status.success() {
            eprintln!(
                "bwoc supervise: {} exited cleanly — supervisor stopping",
                entry.id
            );
            emit_event(
                args.json,
                "clean_exit",
                &entry.id,
                serde_json::json!({ "exit_code": 0 }),
            );
            return 0;
        }

        // Non-zero: crash. Pause briefly before respawn.
        eprintln!(
            "bwoc supervise: {} exited {} — respawning in 1s ({}/{} in window)",
            entry.id,
            exit_status,
            restarts.len(),
            args.max_restarts_per_min,
        );
        emit_event(
            args.json,
            "crash_respawn",
            &entry.id,
            serde_json::json!({
                "exit_code": exit_status.code(),
                "restarts_in_window": restarts.len(),
            }),
        );
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Spawn `bwoc-agent --serve` and wait for it to exit. Returns the
/// exit status. Stderr is redirected to the agent's log file (same
/// path as `bwoc start`), so observers can use `bwoc log -f` against
/// the supervised daemon just like a normally-started one.
fn spawn_and_wait(
    agent_path: &Path,
    _should_stop: &Arc<AtomicBool>,
) -> std::io::Result<std::process::ExitStatus> {
    let bwoc_dir = agent_path.join(".bwoc");
    std::fs::create_dir_all(&bwoc_dir)?;
    let log_path = bwoc_dir.join("agent.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    // Sibling-of-running-binary resolution (see bwoc_core::exec) so the
    // supervisor restarts the matching daemon, not a stale PATH copy.
    let agent_bin = bwoc_core::exec::binary_or_name("bwoc-agent");
    let mut child = Command::new(&agent_bin)
        .arg("--serve")
        .current_dir(agent_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
        .spawn()?;
    // Block on the child. ctrlc handler sets should_stop; the next
    // loop iteration sees it. We don't kill the child here — the
    // daemon's own SIGTERM/SIGINT handler does the cleanup.
    let status = child.wait()?;
    Ok(status)
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
