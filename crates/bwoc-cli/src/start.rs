//! `bwoc start <name>` — reactivate an agent and launch its daemon.
//!
//! Two side effects (both idempotent):
//!   1. Sets `status` to `"active"` in the workspace's `.bwoc/agents.toml`
//!   2. Spawns `bwoc-agent --serve` in the agent's directory if no
//!      live daemon is already running there (PID file + signal-0).
//!
//! Counterpart of `bwoc stop` which now both flips status to "stopped"
//! AND sends STOP over the socket if the daemon is alive.
//!
//! Spawn is fire-and-forget — child stdio redirected to /dev/null,
//! parent exits without waiting. On Unix the child is reparented
//! to init/launchd when this process returns.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bwoc_core::workspace::AgentsRegistry;

pub struct StartArgs {
    /// Empty when `--all`. The CLI shim enforces "exactly one of name | all".
    pub name: String,
    pub workspace: Option<PathBuf>,
    pub yes: bool,
    /// Skip spawning `bwoc-agent --serve`; only flip registry status.
    pub no_daemon: bool,
    /// Start every stopped agent in the workspace. Mutually exclusive
    /// with `name`. Honors `--yes` and `--no-daemon`.
    pub all: bool,
    /// Emit JSON `{ workspace, agent, daemon_spawned, daemon_pid, registry_updated }`
    /// instead of the human report. Requires `--yes`. Single-agent only.
    pub json: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
    )]
    NoWorkspace,
    #[error("no agent named '{name}' in workspace {workspace}")]
    NotFound { name: String, workspace: PathBuf },
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

pub fn run(args: StartArgs) -> i32 {
    let result = if args.all {
        start_all(args)
    } else {
        start(args)
    };
    match result {
        Ok(()) => 0,
        Err(StartError::Aborted) => {
            eprintln!("bwoc start: aborted — nothing changed");
            2
        }
        Err(StartError::NotATerminal) => {
            eprintln!(
                "bwoc start: not a TTY and --yes not given. Pass --yes to confirm or run from an interactive shell."
            );
            2
        }
        Err(e) => {
            eprintln!("bwoc start: {e}");
            match e {
                StartError::NoWorkspace | StartError::NotFound { .. } => 2,
                _ => 1,
            }
        }
    }
}

/// Start every stopped agent in the workspace. Mirror of `stop_all`:
/// loads registry, filters candidates, shows the list, single confirm,
/// then iterates. Already-active and already-running agents are skipped
/// (mass-start should target what NEEDS starting; that's what "stopped"
/// means).
fn start_all(args: StartArgs) -> Result<(), StartError> {
    let workspace = resolve_workspace(args.workspace).ok_or(StartError::NoWorkspace)?;
    let mut registry = AgentsRegistry::load(&workspace)?;

    // Candidates: status == "stopped". (Active agents need no action;
    // mass-start's job is to bring stopped agents back up.)
    let candidate_idxs: Vec<usize> = registry
        .agents
        .iter()
        .enumerate()
        .filter(|(_, a)| a.status == "stopped")
        .map(|(i, _)| i)
        .collect();

    if candidate_idxs.is_empty() {
        println!();
        println!("bwoc start --all: no stopped agents in this workspace.");
        println!();
        return Ok(());
    }

    println!();
    println!(
        "About to start {} agent(s) in {}:",
        candidate_idxs.len(),
        workspace.display()
    );
    for &i in &candidate_idxs {
        let a = &registry.agents[i];
        println!("  - {} (stopped → active, backend: {})", a.id, a.backend);
    }
    if args.no_daemon {
        println!();
        println!("(--no-daemon: daemons will NOT be spawned; registry-only)");
    }
    println!();

    if !args.yes {
        if !io::stdin().is_terminal() {
            return Err(StartError::Aborted);
        }
        let mut stdout = io::stdout();
        write!(stdout, "Proceed with mass start? [y/N]: ")?;
        stdout.flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            return Err(StartError::Aborted);
        }
    }

    let mut spawned = 0u32;
    let mut already_running = 0u32;
    let mut skipped_no_daemon = 0u32;
    let mut spawn_errors = 0u32;
    for &i in &candidate_idxs {
        let entry = registry.agents[i].clone();
        let agent_path = workspace.join(&entry.path);
        registry.agents[i].status = "active".to_string();
        let running = daemon_is_alive(&agent_path);
        let label = if args.no_daemon {
            skipped_no_daemon += 1;
            "registry → active (no daemon)".to_string()
        } else if running {
            already_running += 1;
            "registry → active (daemon already running)".to_string()
        } else {
            match spawn_daemon(&agent_path) {
                Ok(pid) => {
                    spawned += 1;
                    format!("registry → active, daemon spawned (pid {pid})")
                }
                Err(e) => {
                    spawn_errors += 1;
                    format!("registry → active, daemon spawn FAILED: {e}")
                }
            }
        };
        println!("  {}: {}", entry.id, label);
    }
    registry.save(&workspace)?;

    println!();
    println!(
        "{} started. (spawned: {}, already-running: {}, skipped --no-daemon: {}, spawn-errors: {})",
        candidate_idxs.len(),
        spawned,
        already_running,
        skipped_no_daemon,
        spawn_errors,
    );
    println!();
    Ok(())
}

fn start(args: StartArgs) -> Result<(), StartError> {
    let workspace = resolve_workspace(args.workspace).ok_or(StartError::NoWorkspace)?;
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
        .ok_or_else(|| StartError::NotFound {
            name: args.name.clone(),
            workspace: workspace.clone(),
        })?;

    let entry = registry.agents[idx].clone();
    let agent_path = workspace.join(&entry.path);
    let already_active = entry.status == "active";
    let already_running = daemon_is_alive(&agent_path);

    if !args.json {
        println!();
        println!("About to start agent:");
        println!("  id:       {}", entry.id);
        println!("  path:     {}", entry.path);
        println!("  backend:  {}", entry.backend);
        if already_active {
            println!("  status:   active (no change)");
        } else {
            println!("  status:   {} → active", entry.status);
        }
        if already_running {
            println!("  daemon:   already running");
        } else if args.no_daemon {
            println!("  daemon:   --no-daemon (will NOT spawn)");
        } else {
            println!("  daemon:   will spawn `bwoc-agent --serve`");
        }
        println!();
    }

    if !args.yes {
        if args.json {
            return Err(StartError::Aborted);
        }
        if !io::stdin().is_terminal() {
            return Err(StartError::NotATerminal);
        }
        let mut stdout = io::stdout();
        write!(stdout, "Proceed? [y/N]: ")?;
        stdout.flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            return Err(StartError::Aborted);
        }
    }

    // Idempotent: flip status if needed.
    if !already_active {
        registry.agents[idx].status = "active".to_string();
        registry.save(&workspace)?;
    }

    // Spawn the daemon unless told not to or it's already running.
    let daemon_spawned = if args.no_daemon || already_running {
        None
    } else {
        Some(spawn_daemon(&agent_path)?)
    };

    if args.json {
        let value = serde_json::json!({
            "workspace": workspace.display().to_string(),
            "agent": entry.id,
            "daemon_spawned": daemon_spawned.is_some(),
            "daemon_pid": daemon_spawned,
            "already_running": already_running,
            "registry_updated": !already_active,
        });
        println!(
            "{}",
            serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(());
    }

    println!();
    println!("Started: {}", entry.id);
    if already_active {
        println!("  Registry: already active (no change)");
    } else {
        println!(
            "  Registry: updated to active at {}/.bwoc/agents.toml",
            workspace.display()
        );
    }
    match daemon_spawned {
        Some(pid) => println!("  Daemon:   spawned (pid {pid})"),
        None if already_running => println!("  Daemon:   already running"),
        None => println!("  Daemon:   not spawned (--no-daemon)"),
    }
    println!();
    Ok(())
}

/// Spawn `bwoc-agent --serve` in `agent_path`. Stderr is redirected to
/// `<agent_path>/.bwoc/agent.log` (append) so `bwoc log <agent>` has
/// something to tail. Stdin + stdout still go to /dev/null — the daemon
/// prints all useful output to stderr and never reads stdin in --serve
/// mode. Returns the child PID.
fn spawn_daemon(agent_path: &Path) -> Result<u32, StartError> {
    let bwoc_dir = agent_path.join(".bwoc");
    std::fs::create_dir_all(&bwoc_dir)?;
    let log_path = bwoc_dir.join("agent.log");
    // Open append-mode so multiple start/stop cycles accumulate history
    // rather than truncating prior runs. Truncate via `bwoc log --clear`
    // (future) if it ever grows unwieldy.
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    // Resolve `bwoc-agent` relative to the running `bwoc` (sibling), so a dev
    // build or non-PATH install starts its *own* daemon, not a stale copy.
    let agent_bin = bwoc_core::exec::binary_or_name("bwoc-agent");
    let child = Command::new(&agent_bin)
        .arg("--serve")
        .current_dir(agent_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file))
        .spawn()
        .map_err(|e| {
            io::Error::other(format!(
                "failed to spawn `{} --serve` in {}: {e} \
                 (install bwoc-agent next to bwoc, or `cargo install --path crates/bwoc-agent`)",
                agent_bin.to_string_lossy(),
                agent_path.display()
            ))
        })?;
    let pid = child.id();

    // Close the start-up race: the daemon writes `.bwoc/agent.pid` only
    // once its `--serve` startup reaches that line, which can lag the
    // `spawn()` return by milliseconds. A second `bwoc start` arriving in
    // that window reads no pid file, decides "not running", and spawns a
    // duplicate daemon. Writing the pid file from the parent *now* — with
    // the child's pid, which equals the daemon's own `std::process::id()` —
    // closes the window. The daemon's later write is idempotent (same
    // value). Best-effort: a write failure here just reopens the race, it
    // doesn't break the spawn, so we don't propagate the error.
    let pid_path = bwoc_dir.join("agent.pid");
    let _ = std::fs::write(&pid_path, format!("{pid}\n"));

    Ok(pid)
}

/// True iff the agent has a PID file AND the pid is alive (signal-0).
/// Thin wrapper over `crate::livecheck::signal_zero_alive` — the
/// livecheck `running_pid` helper takes `(root, AgentEntry)`, but here
/// we already have an `agent_path`, so reading the pid file directly
/// is cleaner than reconstructing the relative path.
fn daemon_is_alive(agent_path: &Path) -> bool {
    let pid_path = agent_path.join(".bwoc/agent.pid");
    let Ok(raw) = std::fs::read_to_string(&pid_path) else {
        return false;
    };
    let Ok(pid) = raw.trim().parse::<u32>() else {
        return false;
    };
    crate::livecheck::signal_zero_alive(pid)
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

    fn setup_workspace(label: &str, initial_status: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-start-{label}-{}", std::process::id()));
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
            status: initial_status.into(),
        });
        reg.save(&root).unwrap();
        root
    }

    #[test]
    fn start_with_no_daemon_sets_status_to_active() {
        let root = setup_workspace("ok", "stopped");
        assert!(
            start(StartArgs {
                all: false,
                json: false,
                name: "alpha".into(),
                workspace: Some(root.clone()),
                yes: true,
                no_daemon: true,
            })
            .is_ok()
        );
        let reg = AgentsRegistry::load(&root).unwrap();
        assert_eq!(reg.agents[0].status, "active");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn start_already_active_is_idempotent() {
        // Was previously rejected with AlreadyActive — now allowed because
        // it still does useful work (idempotent registry + potential
        // daemon spawn).
        let root = setup_workspace("already", "active");
        let result = start(StartArgs {
            all: false,
            json: false,
            name: "alpha".into(),
            workspace: Some(root.clone()),
            yes: true,
            no_daemon: true,
        });
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn start_fails_for_unknown_name() {
        let root = setup_workspace("missing", "stopped");
        let err = start(StartArgs {
            all: false,
            json: false,
            name: "zzz".into(),
            workspace: Some(root.clone()),
            yes: true,
            no_daemon: true,
        });
        assert!(matches!(err, Err(StartError::NotFound { .. })));
        let _ = fs::remove_dir_all(&root);
    }
}
