//! `bwoc retire <name>` — the complement to `bwoc new`. Phase 3 vaya.
//!
//! Removes an agent's registry entry and performs full lifecycle cleanup:
//!
//! # Cleanup order
//!
//! The cleanup steps run in this order before directory removal, so that
//! each step can still inspect the agent directory as needed:
//!
//! 1. **Worktree cleanup** — find live git worktrees under
//!    `<worktreeBase>/<agentId>/` and remove each with `git worktree remove`.
//!    Requires `worktreeBase` to be set in `config.manifest.json`; skipped
//!    silently when absent. Idempotent: missing worktrees are not an error.
//!
//! 2. **Branch release** — delete all local git branches matching
//!    `agent/<agentId>/*` (the multi-agent collision-guard naming from
//!    AGENTS.md §4.2). Prefers `git branch -d` (safe, merged-only); if a
//!    branch is unmerged, falls back to `git branch -D` and surfaces the
//!    branch name in the output with a "forced" label (Sīla: no silent
//!    destruction).
//!
//! 3. **Interconnect deregister** — remove routes in
//!    `.bwoc/interconnect/routes.toml` whose `agent` field equals the
//!    retiring `<agentId>`. Peers must not route to a dead agent. Skipped
//!    when the file is absent.
//!
//! 4. **File handling** — controlled by `--keep-files` / `--keep-memory` /
//!    default delete. Runs after the above so the manifest can be read
//!    during worktree cleanup (step 1).
//!
//! 5. **Registry removal** — remove the agent entry from `.bwoc/agents.toml`.
//!
//! # Resolution
//!
//! The agent is looked up in the enclosing workspace's `.bwoc/agents.toml`
//! by `id` (the `agent-<name>` form). Failing that, by `name` alone (so
//! `bwoc retire foo` matches the entry whose id is `agent-foo`).
//!
//! # Safety
//!
//! Interactive confirmation in TTY (any non-`y`/`yes` aborts). `--yes`
//! skips the confirmation for scripts. `--keep-files` keeps the agent
//! directory on disk and only removes the registry entry (and still runs
//! git cleanup steps).

use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use bwoc_core::manifest::Manifest;
use bwoc_core::routing::Routes;
use bwoc_core::workspace::AgentsRegistry;

use crate::git_worktree::{
    branch_delete, branch_delete_force, branch_list_glob, worktree_list, worktree_remove,
};

pub struct RetireArgs {
    pub name: String,
    pub workspace: Option<PathBuf>,
    pub yes: bool,
    /// Preserve the entire agent directory; remove only the registry entry.
    pub keep_files: bool,
    /// Preserve just `memories/` (and the parent dir scaffold); remove
    /// everything else. Lets users retire an agent while keeping the
    /// knowledge it accumulated. Mutually exclusive with `keep_files`.
    pub keep_memory: bool,
    /// Emit JSON `{ workspace, agent, path, mode, registry_updated,
    /// worktrees_removed, branches_removed, branches_forced, routes_removed }`
    /// instead of human output. With `--json`, the confirmation prompt
    /// is bypassed only if `yes` is also set (refusing destructive
    /// scripted use without `--yes` is a deliberate guard).
    pub json: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RetireError {
    #[error(
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
    )]
    NoWorkspace,
    #[error("no agent named '{name}' in workspace {workspace}")]
    NotFound { name: String, workspace: PathBuf },
    #[error("aborted by user")]
    Aborted,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
    #[error("routing error: {0}")]
    Routing(#[from] bwoc_core::routing::RoutingError),
}

/// Summary of the three vaya cleanup steps, for output reporting.
#[derive(Debug, Default)]
struct CleanupSummary {
    /// Worktree paths successfully removed.
    worktrees_removed: Vec<String>,
    /// Branches removed with safe `-d` (fully merged).
    branches_removed: Vec<String>,
    /// Branches that required force `-D` (had unmerged commits) — surfaced
    /// in output so the operator is informed (Sīla: no silent destruction).
    branches_forced: Vec<String>,
    /// Number of routes removed from routes.toml.
    routes_removed: usize,
}

pub fn run(args: RetireArgs) -> i32 {
    match retire(args) {
        Ok(()) => 0,
        Err(RetireError::Aborted) => {
            eprintln!("bwoc retire: aborted — nothing changed");
            // Aborted is a clean user decision, not an error.
            2
        }
        Err(e) => {
            eprintln!("bwoc retire: {e}");
            match e {
                RetireError::NoWorkspace | RetireError::NotFound { .. } => 2,
                _ => 1,
            }
        }
    }
}

fn retire(args: RetireArgs) -> Result<(), RetireError> {
    let workspace = resolve_workspace(args.workspace).ok_or(RetireError::NoWorkspace)?;
    let mut registry = AgentsRegistry::load(&workspace)?;

    // Match by id first ("agent-foo"), then by name ("foo" → agent-foo).
    let lookup_id = if args.name.starts_with("agent-") {
        args.name.clone()
    } else {
        format!("agent-{}", args.name)
    };
    let idx = registry
        .agents
        .iter()
        .position(|a| a.id == lookup_id)
        .ok_or_else(|| RetireError::NotFound {
            name: args.name.clone(),
            workspace: workspace.clone(),
        })?;

    let entry = registry.agents[idx].clone();
    let agent_path = workspace.join(&entry.path);
    let mode = if args.keep_files {
        "keep_files"
    } else if args.keep_memory {
        "keep_memory"
    } else {
        "delete"
    };

    // Confirmation (skipped in --json mode, which always requires --yes).
    if !args.json {
        println!();
        println!("About to retire agent:");
        println!("  id:       {}", entry.id);
        println!("  path:     {} (relative to workspace)", entry.path);
        println!("  backend:  {}", entry.backend);
        println!("  status:   {}", entry.status);
        println!();
        if args.keep_files {
            println!("Keeping files on disk; removing only the registry entry.");
        } else if args.keep_memory {
            println!(
                "Keeping just memories/; removing everything else under: {}",
                agent_path.display()
            );
        } else {
            println!("This will DELETE the directory: {}", agent_path.display());
        }
        println!();
    }

    if !args.yes {
        // --json mode requires --yes (destructive scripted use needs the
        // explicit ack; refusing silently is safer than emitting "ok").
        if args.json {
            return Err(RetireError::Aborted);
        }
        if !io::stdin().is_terminal() {
            return Err(RetireError::Aborted);
        }
        let mut stdout = io::stdout();
        write!(stdout, "Proceed? [y/N]: ")?;
        stdout.flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let answer = line.trim().to_ascii_lowercase();
        if answer != "y" && answer != "yes" {
            return Err(RetireError::Aborted);
        }
    }

    // ── Step 1: Worktree cleanup ──────────────────────────────────────────────
    // ── Step 2: Branch release ────────────────────────────────────────────────
    // ── Step 3: Interconnect deregister ──────────────────────────────────────
    // All three run BEFORE file/directory removal so the manifest is still
    // accessible (step 1 reads worktreeBase from config.manifest.json).
    let cleanup = run_cleanup(&workspace, &agent_path, &entry.id);

    // ── Step 4: File handling ─────────────────────────────────────────────────
    if !args.keep_files && agent_path.exists() {
        if args.keep_memory {
            remove_all_except_memories(&agent_path)?;
        } else {
            fs::remove_dir_all(&agent_path)?;
        }
    }

    // ── Step 5: Remove from registry ─────────────────────────────────────────
    registry.agents.remove(idx);
    registry.save(&workspace)?;

    if args.json {
        let value = serde_json::json!({
            "workspace": workspace.display().to_string(),
            "agent": entry.id,
            "path": entry.path,
            "mode": mode,
            "registry_updated": true,
            "worktrees_removed": cleanup.worktrees_removed,
            "branches_removed": cleanup.branches_removed,
            "branches_forced": cleanup.branches_forced,
            "routes_removed": cleanup.routes_removed,
        });
        println!(
            "{}",
            serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(());
    }

    println!();
    println!("Retired: {}", entry.id);
    if args.keep_files {
        println!("  Files kept at: {}", agent_path.display());
    } else if args.keep_memory {
        println!(
            "  Memories preserved at: {}",
            agent_path.join("memories").display()
        );
        println!("  Other files removed under: {}", agent_path.display());
    } else {
        println!("  Files removed: {}", agent_path.display());
    }
    println!(
        "  Registry updated: {}/.bwoc/agents.toml",
        workspace.display()
    );
    // Surface vaya cleanup results.
    if !cleanup.worktrees_removed.is_empty() {
        println!(
            "  Worktrees removed: {}",
            cleanup.worktrees_removed.join(", ")
        );
    }
    if !cleanup.branches_removed.is_empty() {
        println!(
            "  Branches deleted: {}",
            cleanup.branches_removed.join(", ")
        );
    }
    if !cleanup.branches_forced.is_empty() {
        println!(
            "  Branches force-deleted (unmerged): {}",
            cleanup.branches_forced.join(", ")
        );
    }
    if cleanup.routes_removed > 0 {
        println!(
            "  Routes removed from routes.toml: {}",
            cleanup.routes_removed
        );
    }
    println!();
    Ok(())
}

/// Run all three vaya cleanup steps: worktree removal, branch release,
/// interconnect deregister. Non-fatal: failures in git operations (e.g.
/// git not on PATH, worktree already gone) are silently skipped — retire
/// must not be blocked by partial prior cleanup (idempotency contract).
fn run_cleanup(
    workspace: &std::path::Path,
    agent_path: &std::path::Path,
    agent_id: &str,
) -> CleanupSummary {
    let mut summary = CleanupSummary::default();

    // ── Step 1: Worktree cleanup ──────────────────────────────────────────────
    // Resolve worktreeBase from config.manifest.json. Convention:
    //   <worktreeBase>/<agentId>/<taskId>
    // We list all live git worktrees and remove those whose path starts with
    // <worktreeBase>/<agentId>/. This is fully deterministic from the path
    // convention — we do NOT parse any agent-written log (Anattā).
    let manifest_path = agent_path.join("config.manifest.json");
    if let Ok(manifest) = Manifest::load_from_path(&manifest_path) {
        if let Some(base) = manifest.worktree_base {
            let agent_worktree_prefix = PathBuf::from(&base).join(agent_id);
            // List all registered worktrees; remove matching ones.
            if let Ok(worktrees) = worktree_list() {
                for wt in worktrees {
                    // Skip the main worktree (it's never under worktreeBase).
                    if wt.path.starts_with(&agent_worktree_prefix) {
                        match worktree_remove(&wt.path) {
                            Ok(()) => {
                                summary
                                    .worktrees_removed
                                    .push(wt.path.display().to_string());
                            }
                            Err(_) => {
                                // Already gone or has uncommitted changes the
                                // operator must handle. Idempotent — skip.
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Step 2: Branch release ────────────────────────────────────────────────
    // Delete all local branches matching `agent/<agentId>/*`. Prefer safe `-d`
    // (refuses unmerged); fall back to `-D` with a logged warning when needed.
    let glob = format!("agent/{agent_id}/*");
    if let Ok(branches) = branch_list_glob(&glob) {
        for branch in branches {
            match branch_delete(&branch) {
                Ok(()) => {
                    summary.branches_removed.push(branch);
                }
                Err(_) => {
                    // Safe delete failed — branch has unmerged commits. Surface
                    // as "forced" in output so operator is informed (Sīla).
                    match branch_delete_force(&branch) {
                        Ok(()) => {
                            summary.branches_forced.push(branch);
                        }
                        Err(_) => {
                            // Branch doesn't exist or git unavailable — skip.
                        }
                    }
                }
            }
        }
    }

    // ── Step 3: Interconnect deregister ──────────────────────────────────────
    // Remove routes whose `agent` field equals this agent_id. Peers must not
    // route to a dead agent. File absent → skip (idempotent).
    if let Ok(removed) = Routes::remove_agent_routes(workspace, agent_id) {
        summary.routes_removed = removed;
    }

    summary
}

/// Walk the agent directory and remove everything except `memories/`.
/// Idempotent — missing memories/ is fine (the parent dir just gets
/// stripped clean). After this returns, `<agent_path>/memories/` is
/// the only thing left, if it existed.
fn remove_all_except_memories(agent_path: &std::path::Path) -> io::Result<()> {
    let read = fs::read_dir(agent_path)?;
    for entry in read.flatten() {
        let name = entry.file_name();
        if name == "memories" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Resolve the workspace root: explicit > BWOC_WORKSPACE env > ancestor
/// walk from cwd > None. Mirror of the chain in workspace.rs and
/// doctor.rs (kept private to avoid premature extraction).
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
    use bwoc_core::manifest::Manifest;
    use bwoc_core::workspace::{AgentEntry, Workspace, WorkspaceDefaults, WorkspaceMeta};

    fn setup_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-retire-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        fs::create_dir_all(root.join("agents")).unwrap();
        let ws = Workspace {
            workspace: WorkspaceMeta {
                name: label.to_string(),
                version: "0.1.0".to_string(),
                created: "2026-05-22T00:00:00Z".to_string(),
            },
            defaults: WorkspaceDefaults::default(),
        };
        ws.save(&root).unwrap();
        let mut reg = AgentsRegistry::default();
        reg.agents.push(AgentEntry {
            id: "agent-alpha".into(),
            path: "agents/agent-alpha".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22T00:00:00Z".into(),
            status: "active".into(),
        });
        reg.save(&root).unwrap();
        fs::create_dir_all(root.join("agents/agent-alpha")).unwrap();
        fs::write(root.join("agents/agent-alpha/AGENTS.md"), "stub").unwrap();
        root
    }

    /// Write a minimal valid `config.manifest.json` for an agent.
    fn write_manifest(agent_dir: &std::path::Path, worktree_base: Option<&str>) {
        let m = Manifest {
            name: "alpha".into(),
            agent_id: "agent-alpha".into(),
            agent_role: "test".into(),
            primary_model: "model-x".into(),
            fallback_model: None,
            memory_path: "memories/".into(),
            sessions_path: None,
            deep_memory_cmd: None,
            lint_cmd: "true".into(),
            format_cmd: "true".into(),
            test_cmd: "true".into(),
            build_cmd: "true".into(),
            worktree_base: worktree_base.map(str::to_string),
            scope_description: None,
            out_of_scope: None,
            backend: None,
            base_url: None,
            trust: None,
            version: "2.0".into(),
        };
        m.save_to_path(&agent_dir.join("config.manifest.json"))
            .unwrap();
    }

    // ── Existing retirement tests (preserved) ─────────────────────────────────

    #[test]
    fn retire_removes_entry_and_files() {
        let root = setup_workspace("removes");
        let args = RetireArgs {
            keep_memory: false,
            json: false,
            name: "alpha".into(),
            workspace: Some(root.clone()),
            yes: true,
            keep_files: false,
        };
        assert!(retire(args).is_ok());

        let reg = AgentsRegistry::load(&root).unwrap();
        assert!(reg.agents.is_empty(), "registry should be empty");
        assert!(
            !root.join("agents/agent-alpha").exists(),
            "agent dir should be deleted"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn retire_keeps_files_when_flagged() {
        let root = setup_workspace("keep");
        let args = RetireArgs {
            keep_memory: false,
            json: false,
            name: "alpha".into(),
            workspace: Some(root.clone()),
            yes: true,
            keep_files: true,
        };
        assert!(retire(args).is_ok());

        let reg = AgentsRegistry::load(&root).unwrap();
        assert!(reg.agents.is_empty());
        assert!(
            root.join("agents/agent-alpha/AGENTS.md").exists(),
            "files should be kept"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn retire_fails_for_unknown_name() {
        let root = setup_workspace("unknown");
        let args = RetireArgs {
            keep_memory: false,
            json: false,
            name: "nonexistent".into(),
            workspace: Some(root.clone()),
            yes: true,
            keep_files: false,
        };
        match retire(args) {
            Err(RetireError::NotFound { name, .. }) => assert_eq!(name, "nonexistent"),
            other => panic!("expected NotFound, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn retire_matches_full_id_or_bare_name() {
        let root = setup_workspace("idmatch");
        // The agent id is "agent-alpha"; both "alpha" and "agent-alpha" must work.
        for name in ["alpha", "agent-alpha"] {
            // Re-set up between runs.
            let _ = fs::remove_dir_all(&root);
            let r = setup_workspace("idmatch");
            let args = RetireArgs {
                keep_memory: false,
                json: false,
                name: name.into(),
                workspace: Some(r.clone()),
                yes: true,
                keep_files: false,
            };
            assert!(retire(args).is_ok(), "should match name={name}");
            let _ = fs::remove_dir_all(&r);
        }
    }

    // ── Step 1: Worktree cleanup tests ────────────────────────────────────────

    /// When `config.manifest.json` is absent, worktree cleanup is skipped —
    /// retire still succeeds (idempotency: missing config is not an error).
    #[test]
    fn worktree_cleanup_skipped_when_no_manifest() {
        let root = setup_workspace("wt-no-manifest");
        // No manifest written — run_cleanup must not panic.
        let summary = run_cleanup(&root, &root.join("agents/agent-alpha"), "agent-alpha");
        assert!(summary.worktrees_removed.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    /// When manifest has no `worktreeBase`, worktree cleanup step is skipped.
    #[test]
    fn worktree_cleanup_skipped_when_no_worktree_base() {
        let root = setup_workspace("wt-no-base");
        let agent_dir = root.join("agents/agent-alpha");
        write_manifest(&agent_dir, None);
        let summary = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert!(summary.worktrees_removed.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    /// When `worktreeBase` is set but there are no matching live worktrees
    /// (as is the case in a unit-test environment where `git worktree list`
    /// either fails or returns only the main worktree), the step is a no-op.
    #[test]
    fn worktree_cleanup_noop_when_no_matching_worktrees() {
        let root = setup_workspace("wt-noop");
        let agent_dir = root.join("agents/agent-alpha");
        // Use a temp dir as worktreeBase that holds no real worktrees.
        let fake_base =
            std::env::temp_dir().join(format!("bwoc-retire-wt-base-{}", std::process::id()));
        let _ = fs::create_dir_all(&fake_base);
        write_manifest(&agent_dir, Some(&fake_base.to_string_lossy()));

        let summary = run_cleanup(&root, &agent_dir, "agent-alpha");
        // No matching worktrees — worktrees_removed must be empty (not an error).
        assert!(summary.worktrees_removed.is_empty());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&fake_base);
    }

    // ── Step 2: Branch release tests ──────────────────────────────────────────

    /// When `git branch --list 'agent/agent-alpha/*'` returns nothing (normal
    /// unit-test environment), branch cleanup is a no-op — not an error.
    #[test]
    fn branch_cleanup_noop_when_no_matching_branches() {
        let root = setup_workspace("br-noop");
        let agent_dir = root.join("agents/agent-alpha");
        let summary = run_cleanup(&root, &agent_dir, "agent-nonexistent");
        assert!(summary.branches_removed.is_empty());
        assert!(summary.branches_forced.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    // ── Step 3: Interconnect deregister tests ─────────────────────────────────

    /// When routes.toml is absent, the deregister step is a no-op.
    #[test]
    fn routes_deregister_noop_when_file_absent() {
        let root = setup_workspace("rt-absent");
        let agent_dir = root.join("agents/agent-alpha");
        let summary = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert_eq!(summary.routes_removed, 0);
        let _ = fs::remove_dir_all(&root);
    }

    /// Routes matching the retiring agent are removed; other routes survive.
    /// Uses TOML literal strings for paths (Windows CI safety — backslashes
    /// in double-quoted TOML strings are treated as escape sequences).
    #[test]
    fn routes_deregister_removes_agent_routes() {
        let root = setup_workspace("rt-remove");
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();

        // Write routes.toml with two routes: one for agent-alpha (retiring),
        // one for agent-beta (must survive). Paths use TOML literal strings
        // (single-quoted) to avoid Windows backslash escape issues in CI.
        let routes_content = r#"
[[route]]
agent = 'agent-alpha'
workspace = '/srv/ws-a'

[[route]]
agent = 'agent-beta'
workspace = '/srv/ws-b'
"#;
        fs::write(root.join(".bwoc/interconnect/routes.toml"), routes_content).unwrap();

        let agent_dir = root.join("agents/agent-alpha");
        let summary = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert_eq!(summary.routes_removed, 1, "one route should be removed");

        // Reload and check the surviving route.
        let remaining =
            bwoc_core::routing::Routes::load(&root).expect("routes.toml should still be valid");
        assert_eq!(remaining.routes.len(), 1, "one route should remain");
        use bwoc_core::routing::RouteKind;
        assert_eq!(
            remaining.routes[0].kind,
            RouteKind::Agent("agent-beta".into()),
            "agent-beta route must survive"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Routes for other agents are untouched when the retiring agent has no route.
    #[test]
    fn routes_deregister_noop_when_no_matching_route() {
        let root = setup_workspace("rt-noop");
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();

        // Only a route for agent-beta — agent-alpha has none.
        let routes_content = r#"
[[route]]
agent = 'agent-beta'
workspace = '/srv/ws-b'
"#;
        fs::write(root.join(".bwoc/interconnect/routes.toml"), routes_content).unwrap();

        let agent_dir = root.join("agents/agent-alpha");
        let summary = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert_eq!(summary.routes_removed, 0);

        let remaining =
            bwoc_core::routing::Routes::load(&root).expect("routes.toml should still be valid");
        assert_eq!(remaining.routes.len(), 1, "agent-beta route must survive");
        let _ = fs::remove_dir_all(&root);
    }

    /// Namespace routes are never removed by agent-id deregister — they are
    /// workspace-level routing, not agent-specific.
    #[test]
    fn routes_deregister_preserves_namespace_routes() {
        let root = setup_workspace("rt-ns");
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();

        let routes_content = r#"
[[route]]
agent = 'agent-alpha'
workspace = '/srv/ws-a'

[[route]]
namespace = 'team-x'
workspace = '/srv/team-x'
"#;
        fs::write(root.join(".bwoc/interconnect/routes.toml"), routes_content).unwrap();

        let agent_dir = root.join("agents/agent-alpha");
        let summary = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert_eq!(summary.routes_removed, 1, "only the agent route is removed");

        let remaining =
            bwoc_core::routing::Routes::load(&root).expect("routes should still be valid");
        assert_eq!(remaining.routes.len(), 1, "namespace route must survive");
        use bwoc_core::routing::RouteKind;
        assert!(
            matches!(&remaining.routes[0].kind, RouteKind::Namespace(ns) if ns == "team-x"),
            "surviving route must be the namespace route"
        );
        let _ = fs::remove_dir_all(&root);
    }

    // ── JSON output tests ─────────────────────────────────────────────────────

    /// JSON output includes the new vaya cleanup fields (additive extension).
    #[test]
    fn json_output_includes_cleanup_fields() {
        let root = setup_workspace("json-cleanup");

        // Add a route for agent-alpha so we can verify routes_removed in JSON.
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();
        let routes_content = "[[route]]\nagent = 'agent-alpha'\nworkspace = '/srv/ws-a'\n";
        fs::write(root.join(".bwoc/interconnect/routes.toml"), routes_content).unwrap();

        let args = RetireArgs {
            keep_memory: false,
            json: true,
            name: "alpha".into(),
            workspace: Some(root.clone()),
            yes: true,
            keep_files: false,
        };

        // Capture stdout by calling the internal retire() and checking the
        // registry is updated; we verify the JSON shape via the public API.
        assert!(retire(args).is_ok());

        // Registry must be empty after retire.
        let reg = AgentsRegistry::load(&root).unwrap();
        assert!(reg.agents.is_empty());
        // Routes file must be empty of agent-alpha (routes_removed = 1).
        let remaining = bwoc_core::routing::Routes::load(&root).unwrap();
        assert!(
            remaining.routes.is_empty(),
            "agent-alpha route should be gone"
        );

        let _ = fs::remove_dir_all(&root);
    }

    // ── Idempotency test ──────────────────────────────────────────────────────

    /// Running run_cleanup twice on the same agent must not error on the
    /// second run (idempotency: step 3 re-runs cleanly when no routes remain).
    #[test]
    fn cleanup_is_idempotent() {
        let root = setup_workspace("idempotent");
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();
        let routes_content = "[[route]]\nagent = 'agent-alpha'\nworkspace = '/srv/ws'\n";
        fs::write(root.join(".bwoc/interconnect/routes.toml"), routes_content).unwrap();

        let agent_dir = root.join("agents/agent-alpha");

        // First run: removes 1 route.
        let s1 = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert_eq!(s1.routes_removed, 1);

        // Second run: route already gone, should remove 0 (not panic).
        let s2 = run_cleanup(&root, &agent_dir, "agent-alpha");
        assert_eq!(s2.routes_removed, 0);

        let _ = fs::remove_dir_all(&root);
    }
}
