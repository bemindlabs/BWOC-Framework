//! `bwoc stop <name>` — pause an agent without deleting it.
//!
//! Sets `status = "stopped"` in the workspace's `.bwoc/agents.toml` for
//! the given agent. Files on disk stay intact. The counterpart of
//! `bwoc retire` (which removes the entry + optionally the directory);
//! this is the lighter "pause / deactivate" operation.
//!
//! When Phase 2's control socket lands, this verb will also signal a
//! running `bwoc-agent` process to stop gracefully. For now it's a
//! registry-only mutation.
//!
//! Lookup: matches by full id (`agent-foo`) or bare name (`foo`).
//! TTY confirmation; `--yes` to skip for scripts.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use bwoc_core::workspace::AgentsRegistry;

pub struct StopArgs {
    /// Empty when `--all`. The CLI shim enforces "exactly one of name | all".
    pub name: String,
    pub workspace: Option<PathBuf>,
    pub yes: bool,
    /// Stop every non-stopped agent in the workspace. Mutually exclusive
    /// with `name`. Still honors `--yes` for the mass-action confirm.
    pub all: bool,
    /// Emit JSON `{ workspace, agent, daemon_outcome, registry_updated }`
    /// instead of the human report. Requires `--yes` (scripted destructive
    /// ops without explicit ack → exit 2). Single-agent only — `--all` +
    /// `--json` falls back to the human report.
    pub json: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StopError {
    #[error(
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
    )]
    NoWorkspace,
    #[error("no agent named '{name}' in workspace {workspace}")]
    NotFound { name: String, workspace: PathBuf },
    #[error("agent '{name}' is already stopped")]
    AlreadyStopped { name: String },
    #[error("aborted by user")]
    Aborted,
    #[error(
        "not a TTY and --yes not given. \
         Pass --yes to confirm or run from an interactive shell."
    )]
    NotATerminal,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
}

pub fn run(args: StopArgs) -> i32 {
    let result = if args.all { stop_all(args) } else { stop(args) };
    match result {
        Ok(()) => 0,
        Err(StopError::Aborted) => {
            eprintln!("bwoc stop: aborted — nothing changed");
            2
        }
        Err(StopError::NotATerminal) => {
            eprintln!(
                "bwoc stop: not a TTY and --yes not given. Pass --yes to confirm or run from an interactive shell."
            );
            2
        }
        Err(e) => {
            eprintln!("bwoc stop: {e}");
            match e {
                StopError::NoWorkspace
                | StopError::NotFound { .. }
                | StopError::AlreadyStopped { .. } => 2,
                _ => 1,
            }
        }
    }
}

/// Stop every non-stopped agent in the workspace. Shows the list +
/// count, asks for confirmation (or honors `--yes`), then signal-
/// escalates each in sequence. Registry saves once at the end, so a
/// crash mid-loop still leaves partial intent recorded on subsequent
/// agents.
fn stop_all(args: StopArgs) -> Result<(), StopError> {
    let workspace = resolve_workspace(args.workspace).ok_or(StopError::NoWorkspace)?;
    let mut registry = AgentsRegistry::load(&workspace)?;

    // Candidates: agents not already stopped. Status "active" is the
    // common case; any other non-"stopped" string counts too.
    let candidate_idxs: Vec<usize> = registry
        .agents
        .iter()
        .enumerate()
        .filter(|(_, a)| a.status != "stopped")
        .map(|(i, _)| i)
        .collect();

    if candidate_idxs.is_empty() {
        println!();
        println!("bwoc stop --all: no non-stopped agents in this workspace.");
        println!();
        return Ok(());
    }

    println!();
    println!(
        "About to stop {} agent(s) in {}:",
        candidate_idxs.len(),
        workspace.display()
    );
    for &i in &candidate_idxs {
        let a = &registry.agents[i];
        println!(
            "  - {} ({} → stopped, backend: {})",
            a.id, a.status, a.backend
        );
    }
    println!();
    println!("Files stay on disk. Use `bwoc retire` to remove entirely.");
    println!();

    if !args.yes {
        if !io::stdin().is_terminal() {
            return Err(StopError::Aborted);
        }
        let mut stdout = io::stdout();
        write!(stdout, "Proceed with mass stop? [y/N]: ")?;
        stdout.flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            return Err(StopError::Aborted);
        }
    }

    let mut counts = [0u32; 5]; // NotRunning, SocketOk, Sigterm, Sigkill, CouldNotKill
    for &i in &candidate_idxs {
        let entry = registry.agents[i].clone();
        let agent_path = workspace.join(&entry.path);
        let outcome = escalating_shutdown(&agent_path);
        let label = match outcome {
            StopOutcome::NotRunning => {
                counts[0] += 1;
                "no daemon"
            }
            StopOutcome::SocketOk => {
                counts[1] += 1;
                "STOP via socket"
            }
            StopOutcome::Sigterm => {
                counts[2] += 1;
                "SIGTERM"
            }
            StopOutcome::Sigkill => {
                counts[3] += 1;
                "SIGKILL"
            }
            StopOutcome::CouldNotKill => {
                counts[4] += 1;
                "WARNING still alive"
            }
        };
        println!("  {}: {}", entry.id, label);
        registry.agents[i].status = "stopped".to_string();
    }
    registry.save(&workspace)?;

    println!();
    println!(
        "{} stopped. (no-daemon: {}, socket: {}, SIGTERM: {}, SIGKILL: {}, still-alive: {})",
        candidate_idxs.len(),
        counts[0],
        counts[1],
        counts[2],
        counts[3],
        counts[4],
    );
    println!();
    Ok(())
}

fn stop(args: StopArgs) -> Result<(), StopError> {
    let workspace = resolve_workspace(args.workspace).ok_or(StopError::NoWorkspace)?;
    let mut registry = AgentsRegistry::load(&workspace)?;

    let lookup_id = if args.name.starts_with("agent-") {
        args.name.clone()
    } else {
        format!("agent-{}", args.name)
    };
    let idx = registry
        .agents
        .iter()
        .position(|a| a.id == lookup_id)
        .ok_or_else(|| StopError::NotFound {
            name: args.name.clone(),
            workspace: workspace.clone(),
        })?;

    if registry.agents[idx].status == "stopped" {
        return Err(StopError::AlreadyStopped {
            name: args.name.clone(),
        });
    }

    let entry = registry.agents[idx].clone();
    if !args.json {
        println!();
        println!("About to stop agent:");
        println!("  id:       {}", entry.id);
        println!("  path:     {}", entry.path);
        println!("  backend:  {}", entry.backend);
        println!("  status:   {} → stopped", entry.status);
        println!();
        println!("Files stay on disk. Use `bwoc retire` to remove entirely.");
        println!();
    }

    if !args.yes {
        // --json mode requires --yes (no TTY prompt path is meaningful
        // when JSON consumers script this).
        if args.json {
            return Err(StopError::Aborted);
        }
        if !io::stdin().is_terminal() {
            return Err(StopError::NotATerminal);
        }
        let mut stdout = io::stdout();
        write!(stdout, "Proceed? [y/N]: ")?;
        stdout.flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            return Err(StopError::Aborted);
        }
    }

    // Signal-escalation shutdown — STOP → wait → SIGTERM → wait → SIGKILL.
    // Each step is bounded; the chain only escalates if the previous step
    // didn't kill the process. Best-effort throughout: even total failure
    // still flips the registry so the user's intent is recorded.
    let agent_path = workspace.join(&entry.path);
    let stop_outcome = escalating_shutdown(&agent_path);

    registry.agents[idx].status = "stopped".to_string();
    registry.save(&workspace)?;

    let outcome_str = match stop_outcome {
        StopOutcome::NotRunning => "not_running",
        StopOutcome::SocketOk => "socket_ok",
        StopOutcome::Sigterm => "sigterm",
        StopOutcome::Sigkill => "sigkill",
        StopOutcome::CouldNotKill => "could_not_kill",
    };

    if args.json {
        let value = serde_json::json!({
            "workspace": workspace.display().to_string(),
            "agent": entry.id,
            "daemon_outcome": outcome_str,
            "registry_updated": true,
        });
        println!(
            "{}",
            serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(());
    }

    println!();
    println!("Stopped: {}", entry.id);
    match stop_outcome {
        StopOutcome::NotRunning => {
            println!("  Daemon:   was not running (no signal sent)");
        }
        StopOutcome::SocketOk => {
            println!("  Daemon:   STOP via socket → exited cleanly");
        }
        StopOutcome::Sigterm => {
            println!("  Daemon:   socket unresponsive → SIGTERM → exited");
        }
        StopOutcome::Sigkill => {
            println!("  Daemon:   socket + SIGTERM ignored → SIGKILL");
        }
        StopOutcome::CouldNotKill => {
            println!("  Daemon:   WARNING — could not stop process (still alive)");
        }
    }
    println!(
        "  Registry updated: {}/.bwoc/agents.toml",
        workspace.display()
    );
    println!(
        "  Files preserved at: {}",
        workspace.join(&entry.path).display()
    );
    println!();
    Ok(())
}

/// Outcome of the shutdown ladder, used for the per-step output line.
#[derive(Debug, PartialEq, Eq)]
enum StopOutcome {
    /// No PID file or signal-0 said the pid is dead — nothing to do.
    NotRunning,
    /// Socket `STOP` worked: daemon ack'd and exited within the wait window.
    SocketOk,
    /// Socket was unresponsive but `SIGTERM` made the daemon exit.
    Sigterm,
    /// Both socket + `SIGTERM` ignored; `SIGKILL` ended it.
    Sigkill,
    /// All three failed — process still alive. Caller should warn the user.
    CouldNotKill,
}

/// STOP → SIGTERM → SIGKILL escalation. Each step waits up to ~3s for
/// the daemon to actually exit (poll pid + signal-0 every 100ms).
#[cfg(unix)]
fn escalating_shutdown(agent_path: &std::path::Path) -> StopOutcome {
    use std::time::Duration;

    let pid_path = agent_path.join(".bwoc/agent.pid");
    let Ok(raw) = std::fs::read_to_string(&pid_path) else {
        return StopOutcome::NotRunning;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return StopOutcome::NotRunning;
    };
    if !crate::livecheck::signal_zero_alive(pid) {
        return StopOutcome::NotRunning;
    }

    // Step 1 — socket STOP. Daemon's preferred shutdown path; cleanly
    // removes its own PID + socket files.
    let sock_ok = try_signal_stop(agent_path);
    if sock_ok && wait_for_exit(pid, Duration::from_secs(3)) {
        return StopOutcome::SocketOk;
    }

    // Step 2 — SIGTERM. Daemon ignored STOP or socket was missing. The
    // ctrlc handler in bwoc-agent --serve catches SIGTERM and runs the
    // same cleanup the STOP handler would, so this is "polite escalation."
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if wait_for_exit(pid, Duration::from_secs(3)) {
        return StopOutcome::Sigterm;
    }

    // Step 3 — SIGKILL. No handler runs; daemon leaves debris (stale
    // PID + sock). Doctor's stale-sweep cleans up later.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    // SIGKILL is supposed to be unstoppable; give it 1s to actually
    // reap then check. If still alive, something is very wrong (zombie
    // parent / kernel issue) — surface clearly rather than silently
    // continue.
    if wait_for_exit(pid, Duration::from_secs(1)) {
        return StopOutcome::Sigkill;
    }
    StopOutcome::CouldNotKill
}

/// Windows escalation, mirroring the Unix ladder: pipe STOP (daemon's clean
/// path) → `taskkill /PID` (polite, the WM_CLOSE analogue of SIGTERM) →
/// `taskkill /F /PID` (TerminateProcess, the SIGKILL analogue — daemon leaves
/// debris that doctor sweeps later).
#[cfg(windows)]
fn escalating_shutdown(agent_path: &std::path::Path) -> StopOutcome {
    use std::time::Duration;

    let pid_path = agent_path.join(".bwoc/agent.pid");
    let Ok(raw) = std::fs::read_to_string(&pid_path) else {
        return StopOutcome::NotRunning;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return StopOutcome::NotRunning;
    };
    if !crate::livecheck::signal_zero_alive(pid) {
        return StopOutcome::NotRunning;
    }

    // Step 1 — pipe STOP. Daemon removes its own PID + pipe-name files.
    if try_signal_stop(agent_path) && wait_for_exit(pid, Duration::from_secs(3)) {
        return StopOutcome::SocketOk;
    }

    // Step 2 — polite taskkill (no /F): close-message escalation.
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .output();
    if wait_for_exit(pid, Duration::from_secs(3)) {
        return StopOutcome::Sigterm;
    }

    // Step 3 — forced taskkill: TerminateProcess; no handler runs.
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
    if wait_for_exit(pid, Duration::from_secs(1)) {
        return StopOutcome::Sigkill;
    }
    StopOutcome::CouldNotKill
}

#[cfg(not(any(unix, windows)))]
fn escalating_shutdown(_agent_path: &std::path::Path) -> StopOutcome {
    StopOutcome::NotRunning
}

/// Poll liveness every 100ms for up to `deadline`. Returns true once
/// the process is gone, false if it's still alive past the window.
#[cfg(any(unix, windows))]
fn wait_for_exit(pid: u32, deadline: std::time::Duration) -> bool {
    use std::time::{Duration, Instant};
    let until = Instant::now() + deadline;
    while Instant::now() < until {
        if !crate::livecheck::signal_zero_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !crate::livecheck::signal_zero_alive(pid)
}

/// Try to send `STOP\n` over the agent's Unix socket. Returns true if
/// the agent acknowledged. Silent on missing socket / connection errors
/// — `bwoc stop` is allowed to update the registry even when no live
/// process exists to signal.
#[cfg(unix)]
fn try_signal_stop(agent_path: &std::path::Path) -> bool {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let sock_path = agent_path.join(".bwoc/agent.sock");
    if !sock_path.exists() {
        return false;
    }
    let Ok(mut stream) = UnixStream::connect(&sock_path) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if stream.write_all(b"STOP\n").is_err() {
        return false;
    }
    let mut response = String::new();
    if BufReader::new(&stream).read_line(&mut response).is_err() {
        return false;
    }
    response.trim().starts_with("OK")
}

/// Windows: STOP over the agent's named pipe (see `livecheck::pipe_request`).
#[cfg(windows)]
fn try_signal_stop(agent_path: &std::path::Path) -> bool {
    crate::livecheck::pipe_request(agent_path, "STOP")
        .map(|r| r.starts_with("OK"))
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn try_signal_stop(_agent_path: &std::path::Path) -> bool {
    false
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

#[cfg(test)]
mod tests {
    use super::*;
    use bwoc_core::workspace::{
        AgentEntry, AgentsRegistry, Workspace, WorkspaceDefaults, WorkspaceMeta,
    };
    use std::fs;

    fn setup_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-stop-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        fs::create_dir_all(root.join("agents")).unwrap();
        Workspace {
            workspace: WorkspaceMeta {
                name: label.to_string(),
                version: "0.1.0".to_string(),
                created: "2026-05-22T00:00:00Z".to_string(),
            },
            defaults: WorkspaceDefaults::default(),
        }
        .save(&root)
        .unwrap();
        let mut reg = AgentsRegistry::default();
        reg.agents.push(AgentEntry {
            id: "agent-alpha".into(),
            path: "agents/agent-alpha".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22T00:00:00Z".into(),
            status: "active".into(),
        });
        reg.save(&root).unwrap();
        root
    }

    #[test]
    fn stop_sets_status_to_stopped() {
        let root = setup_workspace("ok");
        assert!(
            stop(StopArgs {
                name: "alpha".into(),
                workspace: Some(root.clone()),
                yes: true,
                all: false,
                json: false,
            })
            .is_ok()
        );
        let reg = AgentsRegistry::load(&root).unwrap();
        assert_eq!(reg.agents[0].status, "stopped");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stop_refuses_already_stopped() {
        let root = setup_workspace("already");
        stop(StopArgs {
            name: "alpha".into(),
            workspace: Some(root.clone()),
            yes: true,
            all: false,
            json: false,
        })
        .unwrap();
        let err = stop(StopArgs {
            name: "alpha".into(),
            workspace: Some(root.clone()),
            yes: true,
            all: false,
            json: false,
        });
        assert!(matches!(err, Err(StopError::AlreadyStopped { .. })));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stop_fails_for_unknown_name() {
        let root = setup_workspace("missing");
        let err = stop(StopArgs {
            name: "zzz".into(),
            workspace: Some(root.clone()),
            yes: true,
            all: false,
            json: false,
        });
        assert!(matches!(err, Err(StopError::NotFound { .. })));
        let _ = fs::remove_dir_all(&root);
    }
}
