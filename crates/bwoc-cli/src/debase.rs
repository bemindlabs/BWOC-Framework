//! `bwoc debase list | show <agent> | set <agent> <project>` — manage the
//! agent → base-project binding (the "debase" relationship).
//!
//! The binding is functionally carried by `worktreeBase` in each agent's
//! `config.manifest.json`: an agent bound to project `P` has
//! `worktreeBase = <P>/worktrees`, and the framework places task worktrees at
//! `<worktreeBase>/<agentId>/<taskId>`. `debase` makes that implicit
//! relationship a first-class, inspectable surface:
//!
//! - `list` — every agent → its base project + buildable stack (`--json`).
//! - `show <agent>` — the binding detail for one agent.
//! - `set <agent> <project>` — (re)bind an agent to a base project (a gated
//!   write to its manifest's `worktreeBase`).
//!
//! Pairs with `bwoc new --project <path>`, which derives an agent already bound
//! to a project at incarnation time.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;

use crate::new::{ProjectKind, detect_project_kind};

/// The conventional subdirectory of a base project that holds task worktrees.
pub(crate) const WORKTREES_DIR: &str = "worktrees";

/// The `worktreeBase` value an agent bound to `project` should carry.
pub(crate) fn worktree_base_for_project(project: &Path) -> PathBuf {
    project.join(WORKTREES_DIR)
}

/// Recover the base-project root from a `worktreeBase` by stripping a trailing
/// `worktrees` component. Returns `None` when the path doesn't follow the
/// convention — callers then show the raw value rather than guessing.
pub(crate) fn project_root_of(worktree_base: &str) -> Option<String> {
    let p = Path::new(worktree_base);
    if p.file_name().and_then(|n| n.to_str()) == Some(WORKTREES_DIR) {
        p.parent().map(|r| r.to_string_lossy().into_owned())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// CLI surface (consumed by main.rs)
// ---------------------------------------------------------------------------

pub struct DebaseArgs {
    pub action: DebaseAction,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE > ancestor walk.
    pub workspace: Option<PathBuf>,
    pub json: bool,
}

pub enum DebaseAction {
    /// `bwoc debase list` — every agent's binding.
    List,
    /// `bwoc debase show <agent>` — one agent's binding.
    Show { agent: String },
    /// `bwoc debase set <agent> <project>` — (re)bind to a base project.
    Set {
        agent: String,
        project: PathBuf,
        yes: bool,
    },
}

/// One agent's resolved binding.
struct Binding {
    agent: String,
    /// Raw `worktreeBase` from the manifest (`None` = unbound).
    worktree_base: Option<String>,
    /// Base project root derived from `worktree_base` (convention-stripped).
    project_root: Option<String>,
    /// Buildable stack at the project root, or `None` when unbound/unknown.
    buildable: Option<ProjectKind>,
}

impl Binding {
    fn for_manifest(agent: &str, m: &Manifest) -> Self {
        let worktree_base = m.worktree_base.clone();
        let project_root = worktree_base.as_deref().and_then(project_root_of);
        let buildable =
            project_root
                .as_deref()
                .and_then(|root| match detect_project_kind(Path::new(root)) {
                    ProjectKind::Unknown => None,
                    k => Some(k),
                });
        Self {
            agent: agent.to_string(),
            worktree_base,
            project_root,
            buildable,
        }
    }

    fn buildable_label(&self) -> &str {
        self.buildable.map(|k| k.display_name()).unwrap_or("—")
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(args: DebaseArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc debase: no workspace found. Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc debase: failed to read agents.toml: {e}");
            return 1;
        }
    };

    match args.action {
        DebaseAction::List => list(&workspace, &registry, args.json),
        DebaseAction::Show { agent } => show(&workspace, &registry, &agent, args.json),
        DebaseAction::Set {
            agent,
            project,
            yes,
        } => set(&workspace, &registry, &agent, &project, yes, args.json),
    }
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

fn list(workspace: &Path, registry: &AgentsRegistry, json: bool) -> i32 {
    let mut bindings = Vec::new();
    for entry in &registry.agents {
        let manifest_path = workspace.join(&entry.path).join("config.manifest.json");
        match Manifest::load_from_path(&manifest_path) {
            Ok(m) => bindings.push(Binding::for_manifest(&entry.id, &m)),
            Err(e) => eprintln!("bwoc debase: skipping {} ({e})", entry.id),
        }
    }

    if json {
        let arr: Vec<_> = bindings
            .iter()
            .map(|b| {
                serde_json::json!({
                    "agent": b.agent,
                    "worktree_base": b.worktree_base,
                    "project_root": b.project_root,
                    "buildable": b.buildable.map(|k| k.display_name()),
                    "bound": b.worktree_base.is_some(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        return 0;
    }

    if bindings.is_empty() {
        println!("(no agents registered in {})", workspace.display());
        return 0;
    }

    let agent_w = bindings
        .iter()
        .map(|b| b.agent.len())
        .max()
        .unwrap_or(5)
        .max(5);
    println!("{:<agent_w$}  {:<28}  BUILDABLE", "AGENT", "BASE PROJECT");
    for b in &bindings {
        let proj = match &b.project_root {
            Some(root) => relativize(workspace, root),
            None => b.worktree_base.clone().unwrap_or_else(|| "—".to_string()),
        };
        println!(
            "{:<agent_w$}  {:<28}  {}",
            b.agent,
            proj,
            b.buildable_label()
        );
    }
    0
}

// ---------------------------------------------------------------------------
// show
// ---------------------------------------------------------------------------

fn show(workspace: &Path, registry: &AgentsRegistry, agent: &str, json: bool) -> i32 {
    let Some(entry) = find_agent(registry, agent) else {
        eprintln!(
            "bwoc debase: no agent named '{agent}' in {}. Try `bwoc list`.",
            workspace.display()
        );
        return 2;
    };
    let manifest_path = workspace.join(&entry.path).join("config.manifest.json");
    let m = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "bwoc debase: failed to read {}: {e}",
                manifest_path.display()
            );
            return 1;
        }
    };
    let b = Binding::for_manifest(&entry.id, &m);

    if json {
        let worktree_pattern = b
            .worktree_base
            .as_ref()
            .map(|wb| format!("{wb}/{}/<taskId>", entry.id));
        let v = serde_json::json!({
            "agent": b.agent,
            "worktree_base": b.worktree_base,
            "project_root": b.project_root,
            "buildable": b.buildable.map(|k| k.display_name()),
            "bound": b.worktree_base.is_some(),
            "worktree_pattern": worktree_pattern,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }

    println!("Agent:        {}", b.agent);
    match &b.worktree_base {
        Some(wb) => {
            println!("worktreeBase: {wb}");
            match &b.project_root {
                Some(root) => println!("Base project: {root}"),
                None => println!(
                    "Base project: (worktreeBase doesn't follow the <project>/worktrees convention)"
                ),
            }
            println!("Buildable:    {}", b.buildable_label());
            println!("Worktrees at: {wb}/{}/<taskId>", entry.id);
        }
        None => {
            println!("worktreeBase: (unset — agent is unbound)");
            println!("Base project: —");
            println!(
                "Hint:         bind it with `bwoc debase set {} <project-path>`",
                b.agent
            );
        }
    }
    0
}

// ---------------------------------------------------------------------------
// set
// ---------------------------------------------------------------------------

fn set(
    workspace: &Path,
    registry: &AgentsRegistry,
    agent: &str,
    project: &Path,
    yes: bool,
    json: bool,
) -> i32 {
    let Some(entry) = find_agent(registry, agent) else {
        eprintln!(
            "bwoc debase: no agent named '{agent}' in {}. Try `bwoc list`.",
            workspace.display()
        );
        return 2;
    };

    // The project must exist — worktreeBase has to be an absolute, real path
    // because `bwoc retire` matches it against `git worktree list`.
    let abs = match std::fs::canonicalize(project) {
        Ok(p) if p.is_dir() => p,
        _ => {
            eprintln!(
                "bwoc debase: project path does not exist or is not a directory: {}",
                project.display()
            );
            return 2;
        }
    };
    let new_wb = worktree_base_for_project(&abs)
        .to_string_lossy()
        .into_owned();

    let manifest_path = workspace.join(&entry.path).join("config.manifest.json");
    let mut m = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "bwoc debase: failed to read {}: {e}",
                manifest_path.display()
            );
            return 1;
        }
    };
    let old_wb = m.worktree_base.clone();

    if old_wb.as_deref() == Some(new_wb.as_str()) {
        // Idempotent — nothing to do.
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "agent": entry.id,
                    "worktree_base": new_wb,
                    "changed": false,
                }))
                .unwrap_or_default()
            );
        } else {
            println!(
                "{} already bound to {} (no change).",
                entry.id,
                abs.display()
            );
        }
        return 0;
    }

    // Confirm on a TTY unless --yes; non-TTY proceeds (same UX as other writes).
    if !yes && !json && std::io::stdin().is_terminal() {
        let from = old_wb.as_deref().unwrap_or("(unset)");
        eprint!(
            "Rebind {} worktreeBase\n  from: {from}\n  to:   {new_wb}\nProceed? [y/N] ",
            entry.id
        );
        use std::io::Write;
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        if std::io::stdin().read_line(&mut answer).is_err() {
            return 1;
        }
        let answer = answer.trim().to_lowercase();
        if answer != "y" && answer != "yes" {
            eprintln!("Aborted.");
            return 1;
        }
    }

    m.worktree_base = Some(new_wb.clone());
    if let Err(e) = m.save_to_path(&manifest_path) {
        eprintln!(
            "bwoc debase: failed to write {}: {e}",
            manifest_path.display()
        );
        return 1;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "agent": entry.id,
                "previous_worktree_base": old_wb,
                "worktree_base": new_wb,
                "project_root": abs.to_string_lossy(),
                "changed": true,
            }))
            .unwrap_or_default()
        );
    } else {
        println!("✓ {} → {}", entry.id, abs.display());
        println!("  worktreeBase = {new_wb}");
    }
    0
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Find an agent by id, tolerating a missing `agent-` prefix.
fn find_agent<'a>(
    registry: &'a AgentsRegistry,
    name: &str,
) -> Option<&'a bwoc_core::workspace::AgentEntry> {
    let lookup = if name.starts_with("agent-") {
        name.to_string()
    } else {
        format!("agent-{name}")
    };
    registry.agents.iter().find(|a| a.id == lookup)
}

/// Show `path` relative to `workspace` when it's underneath it, else as-is.
fn relativize(workspace: &Path, path: &str) -> String {
    Path::new(path)
        .strip_prefix(workspace)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Resolution: explicit > BWOC_WORKSPACE env > ancestor walk for
/// `.bwoc/workspace.toml`. Mirrors the other command modules.
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

    #[test]
    fn worktree_base_appends_worktrees() {
        let wb = worktree_base_for_project(Path::new("/p/bwoc-framwork"));
        assert_eq!(wb, PathBuf::from("/p/bwoc-framwork/worktrees"));
    }

    #[test]
    fn project_root_strips_worktrees() {
        assert_eq!(
            project_root_of("/p/bwoc-framwork/worktrees").as_deref(),
            Some("/p/bwoc-framwork")
        );
    }

    #[test]
    fn project_root_rejects_non_convention() {
        assert_eq!(project_root_of("/tmp"), None);
        assert_eq!(project_root_of("/p/worktrees/extra"), None);
    }

    #[test]
    fn roundtrip_project_to_worktree_base_and_back() {
        let project = "/Users/x/projects/bwoc-chat";
        let wb = worktree_base_for_project(Path::new(project));
        assert_eq!(
            project_root_of(&wb.to_string_lossy()).as_deref(),
            Some(project)
        );
    }

    #[test]
    fn relativize_under_workspace() {
        let ws = Path::new("/ws");
        assert_eq!(relativize(ws, "/ws/projects/p"), "projects/p");
        assert_eq!(relativize(ws, "/elsewhere/p"), "/elsewhere/p");
    }
}
