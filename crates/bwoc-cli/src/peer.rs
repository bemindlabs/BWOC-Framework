//! `bwoc peer` — read-only cross-workspace view (Phase 3, #20).
//!
//! Implements the read-only "view" + "learn" slices of the cross-workspace view
//! epic:
//!
//! - `bwoc peer list`               — list peers declared in routes.toml
//! - `bwoc peer <key>`              — show a peer's agents + open team tasks
//! - `bwoc peer <key> status`       — alias for the above
//! - `bwoc peer learn <key>`        — list shared docs from the peer's allowlist
//! - `bwoc peer learn <key> <doc>`  — print one shared doc (allowlist-gated)
//!
//! Allowlist: a peer declares what it exposes in
//! `<peer>/.bwoc/interconnect/shared.toml`:
//!
//! ```toml
//! share = ["research", "retrospectives"]   # doc-kind names
//! ```
//!
//! Absent/empty file → nothing shared (safe default).
//!
//! Enforcement: every file path that `learn` resolves is checked to be
//! path-contained inside the allowlisted kind directory (see
//! `is_path_contained`). Any attempt to read outside that boundary is refused.
//!
//! Read-only. No write to peer workspace; auto-ingest into local memory is
//! deferred to a later phase.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bwoc_core::doc_kind;
use bwoc_core::routing::{RouteKind, Routes};
use bwoc_core::team::{self, Task, TaskState, Team};
use bwoc_core::workspace::AgentsRegistry;

// ── Args ──────────────────────────────────────────────────────────────────────

pub enum PeerAction {
    /// `bwoc peer list` — enumerate declared peers.
    List,
    /// `bwoc peer <key>` or `bwoc peer <key> status` — view a peer.
    View { key: String },
    /// `bwoc peer learn <key> [doc]` — list or view shared docs.
    Learn {
        key: String,
        /// When `Some`, print the named document; when `None`, list all shared docs.
        doc: Option<String>,
    },
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
        PeerAction::Learn { key, doc } => run_learn(&key, doc.as_deref(), &routes),
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

// ── `bwoc peer learn <key> [doc]` ────────────────────────────────────────────

// The peer's shared-allowlist (`shared.toml`) is parsed in `bwoc-core`
// (`routing::SharedAllowlist`) — keeps TOML/serde parsing in core, cli dep-lean.

/// Return `true` iff `candidate` is strictly inside (or equal to) `base`.
///
/// Uses `Path::canonicalize`-style prefix checking on the *normalised*
/// component sequence so that `../` escapes are caught without requiring the
/// path to exist on disk.  Both paths must be absolute.
fn is_path_contained(base: &Path, candidate: &Path) -> bool {
    // Normalise by resolving `.` and `..` in the component sequence without
    // touching the filesystem (canonicalize would require the path to exist).
    let norm = |p: &Path| -> PathBuf {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other),
            }
        }
        out
    };
    let base_n = norm(base);
    let cand_n = norm(candidate);
    cand_n.starts_with(&base_n)
}

/// Resolve a peer key to its workspace root, or print an error and return
/// `None`.  Shared by `run_view` and `run_learn` to avoid duplication.
fn resolve_peer_ws<'a>(key: &str, routes: &'a Routes) -> Option<&'a Path> {
    let peer_ws = routes.resolve(key)?;
    Some(peer_ws)
}

fn run_learn(key: &str, doc: Option<&str>, routes: &Routes) -> i32 {
    // 1. Resolve the peer workspace root.
    let peer_ws = match resolve_peer_ws(key, routes) {
        Some(p) => p.to_path_buf(),
        None => {
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
                    "bwoc peer learn: unknown peer key '{key}'. \
                     No peers are declared in routes.toml."
                );
            } else {
                eprintln!(
                    "bwoc peer learn: unknown peer key '{key}'. Available: {}",
                    available.join(", ")
                );
            }
            return 2;
        }
    };

    if !peer_ws.is_dir() {
        eprintln!(
            "bwoc peer learn: peer workspace '{}' is not reachable \
             (directory missing or not mounted).",
            peer_ws.display()
        );
        return 1;
    }

    // 2. Load the peer's allowlist.
    let shared = bwoc_core::routing::SharedAllowlist::load(&peer_ws);
    if shared.share.is_empty() {
        println!();
        println!("Peer '{key}' shares nothing (no shared.toml or empty allowlist).");
        println!();
        return 0;
    }

    match doc {
        None => run_learn_list(key, &peer_ws, &shared.share),
        Some(name) => run_learn_view(key, name, &peer_ws, &shared.share),
    }
}

/// List shared documents across all allowlisted kinds.
fn run_learn_list(key: &str, peer_ws: &Path, share: &[String]) -> i32 {
    println!();
    println!("Shared documents from peer '{key}':");
    println!();

    let mut any = false;

    for kind_name in share {
        // Resolve the doc-kind to get its directory.
        let kind = match doc_kind::kind(kind_name) {
            Some(k) => k,
            None => {
                eprintln!(
                    "  [warn] kind '{kind_name}' in shared.toml is not a known doc-kind — skipping"
                );
                continue;
            }
        };

        let kind_dir = peer_ws.join(&kind.dir);
        if !kind_dir.is_dir() {
            // Kind dir missing in peer → no docs for this kind, not an error.
            continue;
        }

        let entries = match fs::read_dir(&kind_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut docs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
            .collect();
        docs.sort();

        if docs.is_empty() {
            continue;
        }

        println!("  [{kind_name}]");
        for doc_path in &docs {
            // Enforce containment — belt-and-suspenders.
            if !is_path_contained(&kind_dir, doc_path) {
                continue;
            }
            let filename = doc_path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            let title = first_heading(doc_path).unwrap_or_else(|| filename.to_string());
            println!("    {filename}  —  {title}");
            any = true;
        }
        println!();
    }

    if !any {
        println!("  (no documents found in shared kinds)");
        println!();
    }
    0
}

/// View one shared document — enforces allowlist + path-containment.
fn run_learn_view(key: &str, doc_name: &str, peer_ws: &Path, share: &[String]) -> i32 {
    // Walk each allowlisted kind and look for a match.
    for kind_name in share {
        let kind = match doc_kind::kind(kind_name) {
            Some(k) => k,
            None => {
                // Unknown kind — silently skip (warning already emitted in list).
                continue;
            }
        };

        let kind_dir = peer_ws.join(&kind.dir);
        if !kind_dir.is_dir() {
            continue;
        }

        // Accept exact filename or filename-without-extension.
        let candidate_exact = kind_dir.join(doc_name);
        let candidate_md = kind_dir.join(format!("{doc_name}.md"));

        for candidate in [&candidate_exact, &candidate_md] {
            if !candidate.is_file() {
                continue;
            }

            // Path-containment check — never read outside the kind dir.
            if !is_path_contained(&kind_dir, candidate) {
                eprintln!(
                    "bwoc peer learn: path '{}' is outside the allowlisted \
                     kind directory — refused.",
                    candidate.display()
                );
                return 1;
            }

            match fs::read_to_string(candidate) {
                Ok(content) => {
                    println!("{content}");
                    return 0;
                }
                Err(e) => {
                    eprintln!(
                        "bwoc peer learn: failed to read '{}': {e}",
                        candidate.display()
                    );
                    return 1;
                }
            }
        }
    }

    // Not found in any allowlisted kind.
    eprintln!(
        "bwoc peer learn: document '{doc_name}' not found in any allowlisted \
         kind for peer '{key}'. \
         Use `bwoc peer learn {key}` to list available documents."
    );
    1
}

/// Extract the first Markdown heading (`# ...`) from a file as a plain string,
/// or return `None` when no heading is present.
fn first_heading(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim_start_matches('#').trim();
        if line.starts_with('#') && !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
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

    // ── peer learn ────────────────────────────────────────────────────────────

    /// Seed a `shared.toml` and a research doc in the peer workspace.
    fn seed_shared_doc(peer: &Path, kinds: &[&str], kind_dir: &str, filename: &str, body: &str) {
        // Write shared.toml.
        let ic_dir = peer.join(".bwoc/interconnect");
        fs::create_dir_all(&ic_dir).unwrap();
        let share_list: Vec<String> = kinds.iter().map(|s| format!("\"{s}\"")).collect();
        fs::write(
            ic_dir.join("shared.toml"),
            format!("share = [{}]\n", share_list.join(", ")),
        )
        .unwrap();
        // Write the doc.
        let doc_dir = peer.join(kind_dir);
        fs::create_dir_all(&doc_dir).unwrap();
        fs::write(doc_dir.join(filename), body).unwrap();
    }

    #[test]
    fn learn_lists_shared_research_but_not_retro() {
        // Peer shares only "research". A retrospectives doc is present but NOT
        // in the allowlist → must not appear in the listing.
        let peer = make_peer_ws("learn-list", &[]);
        let local = make_local_ws("learn-list-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"peer-alpha\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );

        // Shared: research doc.
        seed_shared_doc(
            &peer,
            &["research"],
            "research/",
            "2026-05-24_test-topic.md",
            "# Test Topic\n\nSome content.\n",
        );
        // Non-shared: retrospectives doc — must be invisible.
        let retro_dir = peer.join("retrospectives/");
        fs::create_dir_all(&retro_dir).unwrap();
        fs::write(
            retro_dir.join("2026-05-24_session.md"),
            "# Session Retro\n\nSecret.\n",
        )
        .unwrap();

        // List: only research should appear.
        let code = run(PeerArgs {
            action: PeerAction::Learn {
                key: "peer-alpha".into(),
                doc: None,
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn learn_view_allowlisted_doc_succeeds() {
        let peer = make_peer_ws("learn-view-ok", &[]);
        let local = make_local_ws("learn-view-ok-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"peer-beta\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );
        seed_shared_doc(
            &peer,
            &["research"],
            "research/",
            "2026-05-24_my-research.md",
            "# My Research\n\nHello from peer.\n",
        );

        let code = run(PeerArgs {
            action: PeerAction::Learn {
                key: "peer-beta".into(),
                doc: Some("2026-05-24_my-research.md".into()),
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn learn_view_non_allowlisted_doc_refused() {
        // "retrospectives" NOT in allowlist → viewing it must fail.
        let peer = make_peer_ws("learn-refuse", &[]);
        let local = make_local_ws("learn-refuse-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"peer-gamma\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );
        // Only "research" shared.
        seed_shared_doc(
            &peer,
            &["research"],
            "research/",
            "2026-05-24_allowed.md",
            "# Allowed\n",
        );
        // Retro doc exists but is NOT in the allowlist.
        let retro_dir = peer.join("retrospectives/");
        fs::create_dir_all(&retro_dir).unwrap();
        fs::write(retro_dir.join("2026-05-24_secret.md"), "# Secret\n").unwrap();

        let code = run(PeerArgs {
            action: PeerAction::Learn {
                key: "peer-gamma".into(),
                doc: Some("2026-05-24_secret.md".into()),
            },
            workspace: Some(local.clone()),
        });
        // Must not succeed (doc not found in any allowlisted kind).
        assert_ne!(code, 0);
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    #[test]
    fn learn_absent_shared_toml_returns_nothing_shared() {
        // No shared.toml at all → nothing shared, exit 0.
        let peer = make_peer_ws("learn-absent-shared", &[]);
        let local = make_local_ws("learn-absent-local");
        write_routes(
            &local,
            &format!(
                "[[route]]\nagent = \"peer-delta\"\nworkspace = '{}'\n",
                peer.display()
            ),
        );

        let code = run(PeerArgs {
            action: PeerAction::Learn {
                key: "peer-delta".into(),
                doc: None,
            },
            workspace: Some(local.clone()),
        });
        assert_eq!(code, 0); // Nothing shared — but not an error.
        let _ = fs::remove_dir_all(&local);
        let _ = fs::remove_dir_all(&peer);
    }

    // ── is_path_contained ─────────────────────────────────────────────────────

    #[test]
    fn path_containment_accepts_child() {
        assert!(is_path_contained(
            Path::new("/peer/research"),
            Path::new("/peer/research/2026-05-24_foo.md")
        ));
    }

    #[test]
    fn path_containment_accepts_same() {
        assert!(is_path_contained(
            Path::new("/peer/research"),
            Path::new("/peer/research")
        ));
    }

    #[test]
    fn path_containment_rejects_sibling() {
        assert!(!is_path_contained(
            Path::new("/peer/research"),
            Path::new("/peer/retrospectives/secret.md")
        ));
    }

    #[test]
    fn path_containment_rejects_parent_escape() {
        assert!(!is_path_contained(
            Path::new("/peer/research"),
            Path::new("/peer/research/../retrospectives/secret.md")
        ));
    }
}
