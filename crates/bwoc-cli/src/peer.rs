//! `bwoc peer` — read-only cross-workspace view (Phase 3, #20).
//!
//! Implements the read-only "view" slice of the cross-workspace view epic:
//!
//! - `bwoc peer list`            — list peers declared in routes.toml
//! - `bwoc peer <key>`           — show a peer's agents + open team tasks
//! - `bwoc peer <key> status`    — alias for the above
//!
//! Resolution: key → `Routes::resolve` → peer workspace root → peer
//! `AgentsRegistry` + `.bwoc/teams/<id>.toml` + `.bwoc/teams/<id>/tasks.jsonl`.
//!
//! Read-only. No write to peer workspace (learn + give-feedback deferred).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bwoc_core::routing::{RouteKind, Routes};
use bwoc_core::team::{self, Task, TaskState, Team};
use bwoc_core::workspace::AgentsRegistry;

// ── Args ──────────────────────────────────────────────────────────────────────

pub enum PeerAction {
    /// `bwoc peer list` — enumerate declared peers.
    List,
    /// `bwoc peer <key>` or `bwoc peer <key> status` — view a peer.
    View { key: String },
}

pub struct PeerArgs {
    pub action: PeerAction,
    pub workspace: Option<PathBuf>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run(args: PeerArgs) -> i32 {
    let Some(root) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc peer: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };

    let routes = match Routes::load(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc peer: failed to read routes.toml: {e}");
            return 1;
        }
    };

    match args.action {
        PeerAction::List => run_list(&routes),
        PeerAction::View { key } => run_view(&key, &routes, &root),
    }
}

// ── `bwoc peer list` ──────────────────────────────────────────────────────────

fn run_list(routes: &Routes) -> i32 {
    if routes.routes.is_empty() {
        println!();
        println!("No peers declared.");
        println!("  Add a peer in .bwoc/interconnect/routes.toml:");
        println!();
        println!("  [[route]]");
        println!("  agent     = \"agent-name\"");
        println!("  workspace = \"/abs/path/to/peer/workspace\"");
        println!();
        return 0;
    }

    println!();
    println!("{:<32} {:<12} WORKSPACE", "KEY", "KIND");
    println!(
        "{:<32} {:<12} {}",
        "─".repeat(32),
        "─".repeat(12),
        "─".repeat(40)
    );
    for route in &routes.routes {
        let (key, kind) = match &route.kind {
            RouteKind::Agent(id) => (id.as_str(), "agent"),
            RouteKind::Namespace(ns) => (ns.as_str(), "namespace"),
        };
        println!("{:<32} {:<12} {}", key, kind, route.workspace.display());
    }
    println!();
    0
}

// ── `bwoc peer <key> [status]` ───────────────────────────────────────────────

fn run_view(key: &str, routes: &Routes, local_root: &Path) -> i32 {
    let peer_ws = match routes.resolve(key) {
        Some(p) => p.to_path_buf(),
        None => {
            // Collect available keys to guide the user.
            let available: Vec<String> = routes
                .routes
                .iter()
                .map(|r| match &r.kind {
                    RouteKind::Agent(id) => id.clone(),
                    RouteKind::Namespace(ns) => ns.clone(),
                })
                .collect();
            if available.is_empty() {
                eprintln!(
                    "bwoc peer: unknown peer key '{key}'. No peers are declared in routes.toml."
                );
            } else {
                eprintln!(
                    "bwoc peer: unknown peer key '{key}'. Available: {}",
                    available.join(", ")
                );
            }
            return 2;
        }
    };

    // Sanity check: the peer workspace root must be a reachable directory.
    if !peer_ws.is_dir() {
        eprintln!(
            "bwoc peer: peer workspace '{}' is not reachable (directory missing or not mounted).",
            peer_ws.display()
        );
        return 1;
    }

    let peer_registry = match AgentsRegistry::load(&peer_ws) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "bwoc peer: failed to read agents.toml from '{}': {e}",
                peer_ws.display()
            );
            return 1;
        }
    };

    println!();
    println!("Peer: {key}");
    println!("{}=", "=".repeat(key.len() + 5));
    println!("  workspace: {}", peer_ws.display());
    println!();

    // --- Agents ---------------------------------------------------------------
    println!("  Agents ({}):", peer_registry.agents.len());
    if peer_registry.agents.is_empty() {
        println!("    (none registered)");
    } else {
        println!(
            "    {:<28} {:<10} {:<10} STATUS",
            "ID", "BACKEND", "INCARNATED"
        );
        println!(
            "    {:<28} {:<10} {:<10} {}",
            "─".repeat(28),
            "─".repeat(10),
            "─".repeat(10),
            "─".repeat(8)
        );
        for a in &peer_registry.agents {
            // Trim incarnated to date part only for compact display.
            let date = a.incarnated.get(..10).unwrap_or(&a.incarnated);
            println!(
                "    {:<28} {:<10} {:<10} {}",
                a.id, a.backend, date, a.status
            );
        }
    }
    println!();

    // --- Teams + open tasks ---------------------------------------------------
    let open_tasks = collect_open_tasks(&peer_ws);
    if open_tasks.is_empty() {
        println!("  Teams: no open tasks found.");
    } else {
        println!(
            "  Open tasks ({} total across all teams):",
            open_tasks.len()
        );
        println!();
        for (team_id, tasks) in &open_tasks {
            println!("  Team: {team_id}");
            println!(
                "    {:<16} {:<12} {:<24} TITLE",
                "ID", "STATE", "CLAIMED BY"
            );
            println!(
                "    {:<16} {:<12} {:<24} {}",
                "─".repeat(16),
                "─".repeat(12),
                "─".repeat(24),
                "─".repeat(30)
            );
            for t in tasks {
                let state = t.state.as_str();
                let claimed = t.claimed_by.as_deref().unwrap_or("—");
                println!("    {:<16} {:<12} {:<24} {}", t.id, state, claimed, t.title);
            }
            println!();
        }
    }

    // Suppress unused-variable warning — local_root is intentionally kept in
    // the signature for future "learn" extension (write path, deferred #20).
    let _ = local_root;
    0
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Discover all teams in the peer workspace and return their open tasks
/// (Pending or InProgress). Returns empty vec if no teams directory exists.
///
/// Reuses `bwoc_core::team::parse_tasks` pointed at the peer root — no new
/// parsing logic.
fn collect_open_tasks(peer_ws: &Path) -> Vec<(String, Vec<Task>)> {
    let teams_dir = peer_ws.join(".bwoc/teams");
    if !teams_dir.is_dir() {
        return Vec::new();
    }

    let entries = match fs::read_dir(&teams_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut result: Vec<(String, Vec<Task>)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        // Team membership files are `<team-id>.toml` at the top level.
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let team_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Validate the team file is parseable (non-fatal on malformed).
        let team_body = match fs::read_to_string(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if Team::from_toml(&team_body).is_err() {
            continue;
        }

        // Load tasks — absent tasks.jsonl ≡ empty list.
        let tasks_path = teams_dir.join(&team_id).join("tasks.jsonl");
        let tasks = match load_tasks_from(&tasks_path) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let open: Vec<Task> = tasks
            .into_iter()
            .filter(|t| matches!(t.state, TaskState::Pending | TaskState::InProgress))
            .collect();

        if !open.is_empty() {
            result.push((team_id, open));
        }
    }

    // Stable order for deterministic output.
    result.sort_by(|(a, _), (b, _)| a.cmp(b));
    result
}

/// Load and parse a `tasks.jsonl` file. Absent file → empty list.
fn load_tasks_from(path: &Path) -> Result<Vec<Task>, String> {
    match fs::read_to_string(path) {
        Ok(body) => team::parse_tasks(&body).map_err(|e| format!("tasks.jsonl malformed: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("failed to read {}: {e}", path.display())),
    }
}

// ── Workspace resolution (mirrors send.rs / sangha.rs) ───────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bwoc_core::team::{Task, TaskState, Team};
    use bwoc_core::workspace::{
        AgentEntry, AgentsRegistry, Workspace, WorkspaceDefaults, WorkspaceMeta,
    };
    use std::fs;

    // ── Fixture builders ──────────────────────────────────────────────────────

    fn make_local_ws(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("bwoc-peer-local-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();
        Workspace {
            workspace: WorkspaceMeta {
                name: label.into(),
                version: "0.1.0".into(),
                created: "2026-05-24T00:00:00Z".into(),
            },
            defaults: WorkspaceDefaults::default(),
        }
        .save(&root)
        .unwrap();
        AgentsRegistry::default().save(&root).unwrap();
        root
    }

    fn make_peer_ws(label: &str, agent_ids: &[&str]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("bwoc-peer-peer-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        Workspace {
            workspace: WorkspaceMeta {
                name: label.into(),
                version: "0.1.0".into(),
                created: "2026-05-24T00:00:00Z".into(),
            },
            defaults: WorkspaceDefaults::default(),
        }
        .save(&root)
        .unwrap();
        let mut reg = AgentsRegistry::default();
        for id in agent_ids {
            fs::create_dir_all(root.join("agents").join(id)).unwrap();
            reg.agents.push(AgentEntry {
                id: (*id).into(),
                path: format!("agents/{id}"),
                backend: "claude".into(),
                incarnated: "2026-05-24T00:00:00Z".into(),
                status: "active".into(),
            });
        }
        reg.save(&root).unwrap();
        root
    }

    fn write_routes(local: &Path, content: &str) {
        fs::write(local.join(".bwoc/interconnect/routes.toml"), content).unwrap();
    }

    fn seed_team_tasks(peer: &Path, team_id: &str, tasks: &[Task]) {
        let teams_dir = peer.join(".bwoc/teams");
        let task_dir = teams_dir.join(team_id);
        fs::create_dir_all(&task_dir).unwrap();
        // Write a minimal team membership file.
        let team = Team::new(team_id, vec!["agent-alpha".into()]);
        fs::write(
            teams_dir.join(format!("{team_id}.toml")),
            team.to_toml().unwrap(),
        )
        .unwrap();
        // Write tasks.jsonl.
        let body = bwoc_core::team::render_tasks(tasks).unwrap();
        fs::write(task_dir.join("tasks.jsonl"), body).unwrap();
    }

    // ── peer list ─────────────────────────────────────────────────────────────

    #[test]
    fn peer_list_no_peers() {
        let local = make_local_ws("list-empty");
        // No routes.toml written → empty routes.
        let routes = Routes::load(&local).unwrap();
        assert!(routes.routes.is_empty());
        let code = run(PeerArgs {
            action: PeerAction::List,
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&local);
    }

    #[test]
    fn peer_list_shows_declared_peer() {
        let peer = make_peer_ws("list-show-peer", &["agent-omega"]);
        let local = make_local_ws("list-show-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"agent-omega\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );
        let code = run(PeerArgs {
            action: PeerAction::List,
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    // ── peer <key> ────────────────────────────────────────────────────────────

    #[test]
    fn peer_view_unknown_key_returns_2() {
        let local = make_local_ws("view-unknown-local");
        let peer = make_peer_ws("view-unknown-peer", &["agent-alpha"]);
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"agent-alpha\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );
        let code = run(PeerArgs {
            action: PeerAction::View {
                key: "agent-ghost".into(),
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 2);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn peer_view_known_key_shows_agents() {
        let peer = make_peer_ws("view-known-peer", &["agent-alpha", "agent-beta"]);
        let local = make_local_ws("view-known-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nnamespace = \"agent-\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );
        // Use the namespace key itself.
        let code = run(PeerArgs {
            action: PeerAction::View {
                key: "agent-alpha".into(),
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn peer_view_lists_open_tasks() {
        let peer = make_peer_ws("view-tasks-peer", &["agent-alpha"]);
        let local = make_local_ws("view-tasks-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"agent-alpha\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );

        // Seed a team with one pending and one completed task.
        let mut pending = Task::new("t1", "open task", vec![]);
        pending.state = TaskState::Pending;
        let mut done = Task::new("t2", "done task", vec![]);
        done.state = TaskState::Completed;
        seed_team_tasks(&peer, "squad", &[pending, done]);

        let code = run(PeerArgs {
            action: PeerAction::View {
                key: "agent-alpha".into(),
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn peer_view_unreachable_workspace_returns_1() {
        let local = make_local_ws("view-unreachable-local");
        write_routes(
            &local,
            "[[route]]\nagent = \"agent-ghost\"\nworkspace = '/tmp/bwoc-nonexistent-workspace-xyz'\n",
        );
        let code = run(PeerArgs {
            action: PeerAction::View {
                key: "agent-ghost".into(),
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 1);
        let _ = fs::remove_dir_all(&local);
    }

    #[test]
    fn collect_open_tasks_filters_completed() {
        let peer = make_peer_ws("collect-tasks", &[]);
        let mut t1 = Task::new("t1", "pending", vec![]);
        t1.state = TaskState::Pending;
        let mut t2 = Task::new("t2", "in progress", vec![]);
        t2.state = TaskState::InProgress;
        t2.claimed_by = Some("agent-alpha".into());
        let mut t3 = Task::new("t3", "done", vec![]);
        t3.state = TaskState::Completed;
        seed_team_tasks(&peer, "team-a", &[t1, t2, t3]);

        let open = collect_open_tasks(&peer);
        assert_eq!(open.len(), 1);
        let (team_id, tasks) = &open[0];
        assert_eq!(team_id, "team-a");
        assert_eq!(tasks.len(), 2); // t1 + t2; t3 filtered out
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn collect_open_tasks_empty_when_no_teams_dir() {
        let peer = make_peer_ws("no-teams", &[]);
        let open = collect_open_tasks(&peer);
        assert!(open.is_empty());
        let _ = fs::remove_dir_all(&peer);
    }
}
