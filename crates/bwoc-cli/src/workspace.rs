//! `bwoc workspace info` and `bwoc workspace validate`.
//!
//! Read-only inspection of a BWOC workspace. `info` dumps the resolved
//! workspace path, config, and agent count. `validate` runs the five rules
//! from `docs/en/WORKSPACE.en.md` §"Validation Rules" and exits 0 / 2.

use std::path::{Path, PathBuf};

use bwoc_core::workspace::{AgentsRegistry, Workspace};

use crate::i18n;

pub struct InfoArgs {
    pub path: Option<PathBuf>,
    pub lang: String,
    pub json: bool,
    /// Print just the resolved workspace root path, one line, no decoration.
    /// Useful for shell scripts: `cd "$(bwoc workspace info --path-only)"`.
    /// Wins over `--json` if both are set.
    pub path_only: bool,
}

pub struct ValidateArgs {
    pub path: Option<PathBuf>,
    pub lang: String,
    pub json: bool,
}

pub struct ListArgs {
    pub path: Option<PathBuf>,
    pub lang: String,
    pub json: bool,
    /// Filter to agents whose `status` field exactly matches (e.g. "active", "stopped").
    pub status_filter: Option<String>,
    /// Filter to agents whose `backend` field exactly matches (e.g. "claude", "agy").
    pub backend_filter: Option<String>,
    /// Filter to agents whose daemon is actually running (PID file + signal-0 check).
    pub running_only: bool,
    /// Filter to agents with at least one pending inbox envelope.
    /// Pairs with `bwoc inbox <agent>` for the "what needs attention" flow.
    pub inbox_pending_only: bool,
    /// Optional sort key. None = registry insertion order (default).
    /// Accepted: "id" (alphabetical), "inbox" (descending count),
    /// "incarnated" (oldest first), "backend" (alphabetical).
    pub sort: Option<String>,
    /// Print just the count of matching agents (after filters), one
    /// integer to stdout. Useful for shell scripts:
    ///   `if [ $(bwoc list --running --count) -gt 0 ]; then ...`
    pub count_only: bool,
    /// Print just bare agent names (one per line, no `agent-` prefix
    /// stripping — keep the canonical id). Pairs with `--running` /
    /// `--inbox-pending` for iteration:
    ///   `for n in $(bwoc list --running --names-only); do …; done`
    /// Mutually-prioritised: `--count` wins if both set.
    pub names_only: bool,
}

pub struct PruneArgs {
    pub path: Option<PathBuf>,
    pub apply: bool,
    /// Emit JSON instead of the human-readable report. Useful for CI:
    ///   `bwoc workspace prune --json | jq '.phantoms | length'`
    pub json: bool,
}

pub fn run_info(args: InfoArgs) -> i32 {
    let bundle = i18n::bundle_for(&args.lang);
    let Some(root) = find_workspace_root(args.path) else {
        eprintln!(
            "bwoc workspace info: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass a path, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    // --path-only short-circuits: just the resolved root. Wins over --json
    // (most restrictive output mode).
    if args.path_only {
        println!("{}", root.display());
        return 0;
    }
    if args.json {
        return info_json(&root);
    }
    match info(&root, &bundle) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc workspace info: {e}");
            1
        }
    }
}

fn info_json(root: &Path) -> i32 {
    if !root.is_dir() {
        eprintln!("bwoc workspace info: not a directory: {}", root.display());
        return 1;
    }
    if !root.join(".bwoc/workspace.toml").exists() {
        eprintln!(
            "bwoc workspace info: not a BWOC workspace (no .bwoc/workspace.toml): {}",
            root.display()
        );
        return 2;
    }
    let ws = match Workspace::load(root) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("bwoc workspace info: {e}");
            return 1;
        }
    };
    let registry = AgentsRegistry::load(root).unwrap_or_default();

    let value = serde_json::json!({
        "workspace": root.display().to_string(),
        "name": ws.workspace.name,
        "version": ws.workspace.version,
        "created": ws.workspace.created,
        "defaults": {
            "agents_dir": ws.defaults.agents_dir,
            "backend": ws.defaults.backend,
            "lang": ws.defaults.lang,
        },
        "agents_count": registry.agents.len(),
        "resources": {
            "projects": crate::livecheck::count_subdirs(&root.join("projects")),
            "notes": crate::livecheck::count_user_md_files(&root.join("notes")),
            "memory": crate::livecheck::count_user_md_files(&root.join(".bwoc/memory")),
        },
        "attention": {
            // Always present so consumers can branch on .attention.total_pending == 0.
            "agents_needing_attention": registry.agents.iter()
                .filter(|a| crate::livecheck::inbox_count(root, a) > 0)
                .count(),
            "total_pending": registry.agents.iter()
                .map(|a| crate::livecheck::inbox_count(root, a) as u64)
                .sum::<u64>(),
        },
        "agents": registry.agents.iter().map(|a| serde_json::json!({
            "id": a.id,
            "path": a.path,
            "backend": a.backend,
            "status": a.status,
            "incarnated": a.incarnated,
        })).collect::<Vec<_>>(),
    });
    match serde_json::to_string_pretty(&value) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc workspace info: failed to serialize JSON: {e}");
            1
        }
    }
}

pub fn run_validate(args: ValidateArgs) -> i32 {
    let bundle = i18n::bundle_for(&args.lang);
    let Some(root) = find_workspace_root(args.path) else {
        eprintln!(
            "bwoc workspace validate: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass a path, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    let report = validate(&root);
    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "passes": report.passes,
            "violations": report.violations,
            "summary": {
                "passes": report.passes.len(),
                "violations": report.violations.len(),
            },
        });
        match serde_json::to_string_pretty(&value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("bwoc workspace validate: failed to serialize JSON: {e}");
                return 1;
            }
        }
    } else {
        print_validation_report(&root, &report, &bundle);
    }
    if report.violations.is_empty() { 0 } else { 2 }
}

/// Validate that a resolved workspace root is a real BWOC workspace before use.
///
/// `find_workspace_root` / `resolve_workspace` return an **explicit** `--workspace`
/// (or `BWOC_WORKSPACE`) value verbatim, without checking it exists — only the
/// ancestor-walk fallback validates `.bwoc/workspace.toml`. A bogus explicit path
/// then loads an empty registry (`AgentsRegistry::load` treats a missing
/// `agents.toml` as empty) and the command reports a false "empty, healthy"
/// result with exit 0 (issue #339). This makes that case fail loud instead.
///
/// On failure prints a clear error under `cmd` and returns the exit code to
/// propagate (2 — a usage/argument error).
pub(crate) fn ensure_workspace(root: &Path, cmd: &str) -> Result<(), i32> {
    if !root.exists() {
        eprintln!("{cmd}: workspace path not found: {}", root.display());
        return Err(2);
    }
    if !root.is_dir() {
        eprintln!(
            "{cmd}: workspace path is not a directory: {}",
            root.display()
        );
        return Err(2);
    }
    // `.is_file()` (not `.exists()`) so a *directory* named `workspace.toml`
    // doesn't slip through as a valid workspace.
    if !root.join(".bwoc/workspace.toml").is_file() {
        eprintln!(
            "{cmd}: not a BWOC workspace (no .bwoc/workspace.toml): {}. \
             Pass a valid --workspace path or run `bwoc init` first.",
            root.display()
        );
        return Err(2);
    }
    Ok(())
}

pub fn run_list(args: ListArgs) -> i32 {
    let bundle = i18n::bundle_for(&args.lang);

    let Some(root) = find_workspace_root(args.path) else {
        // Error path stays English (thiserror localization deferred).
        eprintln!(
            "bwoc list: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace <path>, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };

    if let Err(code) = ensure_workspace(&root, "bwoc list") {
        return code;
    }

    let registry = match AgentsRegistry::load(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "bwoc list: failed to read agents.toml at {}: {e}",
                root.display()
            );
            return 1;
        }
    };

    // Apply filters BEFORE serialisation — both human and JSON output
    // honour them. Filter values are exact string match (case-sensitive)
    // so users can also filter on custom registry status fields later.
    let matches = |a: &bwoc_core::workspace::AgentEntry| -> bool {
        if let Some(s) = &args.status_filter {
            if a.status != *s {
                return false;
            }
        }
        if let Some(b) = &args.backend_filter {
            if a.backend != *b {
                return false;
            }
        }
        if args.running_only && crate::livecheck::running_pid(&root, a).is_none() {
            return false;
        }
        if args.inbox_pending_only && crate::livecheck::inbox_count(&root, a) == 0 {
            return false;
        }
        true
    };
    let mut filtered: Vec<&bwoc_core::workspace::AgentEntry> =
        registry.agents.iter().filter(|a| matches(a)).collect();

    // Optional --sort. Stable sort so secondary order = registry order.
    if let Some(field) = args.sort.as_deref() {
        match field {
            "id" => filtered.sort_by(|a, b| a.id.cmp(&b.id)),
            "backend" => filtered.sort_by(|a, b| a.backend.cmp(&b.backend)),
            "incarnated" => filtered.sort_by(|a, b| a.incarnated.cmp(&b.incarnated)),
            "inbox" => {
                // Descending — biggest backlog first; that's what the user
                // wants when scanning for "what needs attention next".
                filtered.sort_by(|a, b| {
                    let ca = crate::livecheck::inbox_count(&root, a);
                    let cb = crate::livecheck::inbox_count(&root, b);
                    cb.cmp(&ca)
                });
            }
            other => {
                eprintln!(
                    "bwoc list --sort: unknown field '{other}'. \
                     Accepted: id | inbox | incarnated | backend"
                );
                return 2;
            }
        }
    }

    // --count: short-circuit before any formatting. With --json, emit
    // `{ "count": N }`; without, just the integer + newline. Filters and
    // sort already applied, so the count reflects the user's view.
    // Wins over --names-only if both are set (count is the more
    // restrictive view).
    if args.count_only {
        if args.json {
            let value = serde_json::json!({ "count": filtered.len() });
            match serde_json::to_string(&value) {
                Ok(s) => {
                    println!("{s}");
                    return 0;
                }
                Err(e) => {
                    eprintln!("bwoc list --count --json: serialize failed: {e}");
                    return 1;
                }
            }
        }
        println!("{}", filtered.len());
        return 0;
    }

    // --names-only: bare agent ids, one per line, no decorative text.
    // Designed for shell-script iteration with filters/sort:
    //   `for n in $(bwoc list --running --names-only); do …; done`
    // Honors --json: `{ "names": ["agent-foo", ...] }`.
    if args.names_only {
        if args.json {
            let value = serde_json::json!({
                "names": filtered.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            });
            match serde_json::to_string(&value) {
                Ok(s) => {
                    println!("{s}");
                    return 0;
                }
                Err(e) => {
                    eprintln!("bwoc list --names-only --json: serialize failed: {e}");
                    return 1;
                }
            }
        }
        for a in &filtered {
            println!("{}", a.id);
        }
        return 0;
    }

    // JSON branch — stable machine-readable output, no decorative text,
    // no Fluent (locale doesn't affect machine consumers).
    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "agents": filtered.iter().map(|a| {
                let running = crate::livecheck::running_pid(&root, a).is_some();
                // Null when not running OR when running but the daemon
                // didn't answer the STATUS probe (older daemons may not).
                // Consumers can branch on (running, uptime_seconds).
                let uptime_seconds = if running {
                    crate::livecheck::query_uptime(&root, a)
                } else {
                    None
                };
                serde_json::json!({
                    "id": a.id,
                    "path": a.path,
                    "backend": a.backend,
                    // Confinement tier (t30): "ambient" backends (e.g. `cli`)
                    // run their tools outside the harness — no Phase 5 gate/jail
                    // and no #271 read-only guarantee.
                    "trust_tier": if bwoc_core::trust::backend_trust_tier(&a.backend).is_ambient() {
                        "ambient"
                    } else {
                        "confined"
                    },
                    "status": a.status,
                    "incarnated": a.incarnated,
                    "running": running,
                    "uptime_seconds": uptime_seconds,
                    "inbox_count": crate::livecheck::inbox_count(&root, a),
                })
            }).collect::<Vec<_>>(),
        });
        match serde_json::to_string_pretty(&value) {
            Ok(s) => {
                println!("{s}");
                return 0;
            }
            Err(e) => {
                eprintln!("bwoc list: failed to serialize JSON: {e}");
                return 1;
            }
        }
    }

    let root_display = root.display().to_string();
    if filtered.is_empty() {
        if registry.agents.is_empty() {
            // No agents at all.
            println!(
                "{}",
                i18n::t_with(&bundle, "list-empty", &[("path", &root_display)])
            );
        } else {
            // Agents exist but none matched the filter — give an
            // actionable message instead of just empty output.
            let filter_desc = describe_filters(
                args.status_filter.as_deref(),
                args.backend_filter.as_deref(),
            );
            println!(
                "(no agents match {filter_desc} in workspace {root_display} — {} total)",
                registry.agents.len()
            );
        }
        return 0;
    }

    // pad_visual handles Unicode column-width so TH labels align right
    // (Rust's `{:<N}` pads by byte count; CJK + Thai byte-count diverges
    // from display width).
    use crate::livecheck::pad_visual;
    println!(
        "{} {} {} {} {} {}",
        pad_visual(&i18n::t(&bundle, "list-col-id"), 32),
        pad_visual(&i18n::t(&bundle, "list-col-status"), 10),
        pad_visual(&i18n::t(&bundle, "list-col-backend"), 10),
        pad_visual("UPTIME", 9),
        pad_visual("INBOX", 7),
        i18n::t(&bundle, "list-col-path"),
    );
    println!(
        "{} {} {} {} {} {}",
        "─".repeat(32),
        "─".repeat(10),
        "─".repeat(10),
        "─".repeat(9),
        "─".repeat(7),
        "─".repeat(20),
    );
    for a in &filtered {
        // Daemon liveness + uptime. Running daemons get "● 5m12s",
        // stopped agents get "○      —". Uptime probe goes through
        // the same STATUS socket query `bwoc status` uses, so the
        // two surfaces stay aligned.
        let (mark, uptime) = match crate::livecheck::running_pid(&root, a) {
            Some(_) => match crate::livecheck::query_uptime(&root, a) {
                Some(secs) => ("●", crate::livecheck::format_uptime(secs)),
                None => ("●", "?".to_string()),
            },
            None => ("○", "—".to_string()),
        };
        let count = crate::livecheck::inbox_count(&root, a);
        let inbox_cell = if count == 0 {
            "—".to_string()
        } else {
            count.to_string()
        };
        println!(
            "{mark} {} {} {} {} {} {}",
            pad_visual(&a.id, 30),
            pad_visual(&a.status, 10),
            pad_visual(&a.backend, 10),
            pad_visual(&uptime, 9),
            pad_visual(&inbox_cell, 7),
            a.path,
        );
    }
    // Footer warning (t30): flag any AMBIENT backend so the operator sees the
    // confinement gap the fixed-width BACKEND column can't convey.
    let ambient = filtered
        .iter()
        .filter(|a| bwoc_core::trust::backend_trust_tier(&a.backend).is_ambient())
        .count();
    if ambient > 0 {
        println!(
            "⚠ {ambient} agent(s) on an AMBIENT backend — tools run outside the harness; \
             confinement & the #271 read-only guarantee do NOT apply (gateway auto-process \
             refuses them)."
        );
    }
    0
}

// Liveness (signal_zero_alive, running_pid) + inbox_count moved to
// crate::livecheck. `is_running` was just `running_pid().is_some()` —
// call sites updated to use livecheck directly.

/// Format a one-phrase summary of the active filters for the empty-set
/// hint message. Returns e.g. `--status=active`, `--backend=claude`,
/// or `--status=stopped --backend=agy`.
fn describe_filters(status: Option<&str>, backend: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(s) = status {
        parts.push(format!("--status={s}"));
    }
    if let Some(b) = backend {
        parts.push(format!("--backend={b}"));
    }
    if parts.is_empty() {
        "(no filter)".to_string()
    } else {
        parts.join(" ")
    }
}

pub fn run_prune(args: PruneArgs) -> i32 {
    let Some(root) = find_workspace_root(args.path) else {
        eprintln!(
            "bwoc workspace prune: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass a path, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };

    let mut registry = match AgentsRegistry::load(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc workspace prune: failed to read agents.toml: {e}");
            return 1;
        }
    };

    let agents_dir_name = Workspace::load(&root)
        .map(|w| w.defaults.agents_dir)
        .unwrap_or_else(|_| "agents".to_string());
    let agents_dir = root.join(&agents_dir_name);

    // Phantom entries: in registry, but the directory is gone.
    let phantom_indices: Vec<usize> = registry
        .agents
        .iter()
        .enumerate()
        .filter_map(|(i, a)| (!root.join(&a.path).is_dir()).then_some(i))
        .collect();

    // Orphan dirs: subdirs of agents_dir that look like agents (have AGENTS.md)
    // but aren't in the registry.
    let mut orphans: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() || !p.join("AGENTS.md").is_file() {
                continue;
            }
            let rel = p
                .strip_prefix(&root)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| p.display().to_string());
            if !registry.agents.iter().any(|a| a.path == rel) {
                orphans.push(p);
            }
        }
    }

    // JSON branch: emit structured shape, optionally apply phantom removal,
    // and exit early. Same logic as the human path but no decorative text.
    if args.json {
        let phantoms: Vec<serde_json::Value> = phantom_indices
            .iter()
            .map(|&i| {
                let a = &registry.agents[i];
                serde_json::json!({
                    "id": a.id,
                    "path": a.path,
                    "backend": a.backend,
                    "status": a.status,
                })
            })
            .collect();
        let orphans_json: Vec<String> = orphans
            .iter()
            .map(|o| {
                o.strip_prefix(&root)
                    .map(|r| r.display().to_string())
                    .unwrap_or_else(|_| o.display().to_string())
            })
            .collect();
        let mut removed: Vec<String> = Vec::new();
        if args.apply {
            for &i in phantom_indices.iter().rev() {
                removed.push(registry.agents.remove(i).id);
            }
            if let Err(e) = registry.save(&root) {
                eprintln!("bwoc workspace prune --json: failed to save agents.toml: {e}");
                return 1;
            }
        }
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "phantoms": phantoms,
            "orphans": orphans_json,
            "applied": args.apply,
            "removed": removed,
        });
        match serde_json::to_string_pretty(&value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("bwoc workspace prune --json: serialize failed: {e}");
                return 1;
            }
        }
        // Exit 0 if clean OR fully applied; 2 if orphans remain (matches human path).
        return if !orphans.is_empty() && args.apply {
            2
        } else {
            0
        };
    }

    println!();
    println!("Workspace prune: {}", root.display());
    println!("=================");
    if phantom_indices.is_empty() && orphans.is_empty() {
        println!("Clean — no phantom registry entries, no orphan agent directories.");
        println!();
        return 0;
    }

    if !phantom_indices.is_empty() {
        println!();
        println!("Phantom registry entries (in agents.toml but directory is missing):");
        for &i in &phantom_indices {
            let a = &registry.agents[i];
            println!(
                "  - {} @ {}  (backend: {}, status: {})",
                a.id, a.path, a.backend, a.status
            );
        }
    }
    if !orphans.is_empty() {
        println!();
        println!(
            "Orphan agent directories (under {}/ but not in agents.toml):",
            agents_dir_name
        );
        for o in &orphans {
            let rel = o
                .strip_prefix(&root)
                .map(|r| r.display().to_string())
                .unwrap_or_else(|_| o.display().to_string());
            println!("  - {rel}");
        }
        println!();
        println!("Orphan dirs are NOT auto-removed (they may hold real work). To clean an orphan:");
        println!("  bwoc retire <name>          # if it should be deleted");
        println!("  # OR add it back to agents.toml manually if it should stay.");
    }

    if args.apply {
        // Remove phantom entries from registry, reverse-index order.
        let mut removed = Vec::new();
        for &i in phantom_indices.iter().rev() {
            removed.push(registry.agents.remove(i).id);
        }
        if let Err(e) = registry.save(&root) {
            eprintln!();
            eprintln!("bwoc workspace prune: failed to save updated agents.toml: {e}");
            return 1;
        }
        println!();
        if !removed.is_empty() {
            println!(
                "Applied: removed {} phantom entries from agents.toml: {}",
                removed.len(),
                removed.join(", ")
            );
        }
        if !orphans.is_empty() {
            println!(
                "Skipped: {} orphan directories (use `bwoc retire <name>` per directory).",
                orphans.len()
            );
        }
        println!();
        return if orphans.is_empty() { 0 } else { 2 };
    }

    println!();
    println!(
        "Dry run — nothing changed. Rerun with `--apply` to remove {} phantom registry entries.",
        phantom_indices.len()
    );
    println!();
    2
}

/// Resolve the workspace per `WORKSPACE.en.md` §"Workspace Resolution":
/// explicit path → BWOC_WORKSPACE env → ancestor walk → cwd-self → None.
fn find_workspace_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        let p = PathBuf::from(env_path);
        if !p.as_os_str().is_empty() {
            return Some(p);
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

#[derive(Debug, thiserror::Error)]
pub enum InfoError {
    #[error("path does not exist or is not a directory: {0}")]
    PathMissing(PathBuf),
    #[error("not a BWOC workspace (no .bwoc/workspace.toml): {0}")]
    NotAWorkspace(PathBuf),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
}

fn info(
    root: &Path,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) -> Result<(), InfoError> {
    if !root.is_dir() {
        return Err(InfoError::PathMissing(root.to_path_buf()));
    }
    if !root.join(".bwoc/workspace.toml").exists() {
        return Err(InfoError::NotAWorkspace(root.to_path_buf()));
    }
    let ws = Workspace::load(root)?;
    let registry = AgentsRegistry::load(root)?;

    let root_display = root.display().to_string();
    let agents_count = registry.agents.len().to_string();

    println!();
    println!(
        "{}",
        i18n::t_with(bundle, "info-header", &[("path", &root_display)])
    );
    let lbl = |key: &str| i18n::t(bundle, key);
    println!("  {}:        {}", lbl("info-label-name"), ws.workspace.name);
    println!(
        "  {}:        {}",
        lbl("info-label-version"),
        ws.workspace.version
    );
    println!(
        "  {}:        {}",
        lbl("info-label-created"),
        ws.workspace.created
    );
    if let Some(ref b) = ws.defaults.backend {
        println!("  {}:        {b}", lbl("info-label-backend"));
    }
    if let Some(ref l) = ws.defaults.lang {
        println!("  {}:        {l}", lbl("info-label-lang"));
    }
    println!(
        "  {}:  {}",
        lbl("info-label-agents-dir"),
        ws.defaults.agents_dir
    );
    println!("  {}:      {}", lbl("info-label-agents"), agents_count);
    for a in &registry.agents {
        println!(
            "    - {}",
            i18n::t_with(
                bundle,
                "info-agent-row",
                &[("id", &a.id), ("status", &a.status), ("path", &a.path)],
            )
        );
    }
    // Workspace-level resource counts (matching the dashboard banner).
    // Only print when there's something to show — silent on empty so
    // the human output stays uncluttered for fresh `bwoc init` workspaces.
    let projects = crate::livecheck::count_subdirs(&root.join("projects"));
    let notes = crate::livecheck::count_user_md_files(&root.join("notes"));
    let memory = crate::livecheck::count_user_md_files(&root.join(".bwoc/memory"));
    if projects > 0 || notes > 0 || memory > 0 {
        println!();
        println!("  Resources:");
        if projects > 0 {
            println!("    projects:    {projects}");
        }
        if notes > 0 {
            println!("    notes:       {notes}");
        }
        if memory > 0 {
            println!("    memory:      {memory}");
        }
    }

    // Attention summary: agents with pending inbox envelopes. Surfaces
    // "what needs reading?" at the workspace overview without forcing
    // the user to scan the per-agent rows. Silent when nothing is
    // pending (fresh / idle workspaces stay quiet).
    let mut agents_needing_attention = 0u32;
    let mut total_pending = 0u64;
    for a in &registry.agents {
        let c = crate::livecheck::inbox_count(root, a) as u64;
        if c > 0 {
            agents_needing_attention += 1;
            total_pending += c;
        }
    }
    if agents_needing_attention > 0 {
        println!();
        println!(
            "  Attention: {agents_needing_attention} agent(s) have {total_pending} \
             pending message(s) total. Run `bwoc list --inbox-pending` to see them.",
        );
    }

    println!();
    Ok(())
}

/// Structured validation findings.
pub struct ValidationReport {
    pub passes: Vec<String>,
    pub violations: Vec<String>,
}

/// Run the 5 rules from `WORKSPACE.en.md` §Validation Rules.
pub fn validate(root: &Path) -> ValidationReport {
    let mut report = ValidationReport {
        passes: Vec::new(),
        violations: Vec::new(),
    };

    // Rule 1: .bwoc/ directory exists
    let dot_bwoc = root.join(".bwoc");
    if dot_bwoc.is_dir() {
        report.passes.push(".bwoc/ exists".into());
    } else {
        report.violations.push(".bwoc/ directory missing".into());
        return report; // Can't continue — short-circuit.
    }

    // Rule 2: .bwoc/workspace.toml exists, parses, has required fields
    let ws_path = dot_bwoc.join("workspace.toml");
    if !ws_path.is_file() {
        report
            .violations
            .push(".bwoc/workspace.toml missing".into());
        return report;
    }
    let ws = match Workspace::load(root) {
        Ok(w) => {
            report.passes.push(".bwoc/workspace.toml parses".into());
            w
        }
        Err(e) => {
            report
                .violations
                .push(format!(".bwoc/workspace.toml invalid: {e}"));
            return report;
        }
    };

    if ws.workspace.name.is_empty() {
        report.violations.push("workspace.name is empty".into());
    } else {
        report
            .passes
            .push(format!("workspace.name = {:?}", ws.workspace.name));
    }
    if ws.workspace.created.is_empty() {
        report.violations.push("workspace.created is empty".into());
    } else {
        report
            .passes
            .push(format!("workspace.created = {:?}", ws.workspace.created));
    }

    // Rule 5: version is parseable SemVer (checked here since we have `ws` in hand)
    if is_semver(&ws.workspace.version) {
        report.passes.push(format!(
            "workspace.version {} is valid SemVer",
            ws.workspace.version
        ));
    } else {
        report.violations.push(format!(
            "workspace.version {:?} is not parseable SemVer (expect X.Y.Z)",
            ws.workspace.version
        ));
    }

    // Rule 3: .bwoc/agents.toml exists and parses
    let agents_path = dot_bwoc.join("agents.toml");
    if !agents_path.is_file() {
        report.violations.push(".bwoc/agents.toml missing".into());
    } else {
        match AgentsRegistry::load(root) {
            Ok(reg) => {
                report.passes.push(format!(
                    ".bwoc/agents.toml parses ({} agent(s))",
                    reg.agents.len()
                ));
            }
            Err(e) => {
                report
                    .violations
                    .push(format!(".bwoc/agents.toml invalid: {e}"));
            }
        }
    }

    // Rule 4: agents_dir exists (relative to root)
    let agents_dir = root.join(&ws.defaults.agents_dir);
    if agents_dir.is_dir() {
        report
            .passes
            .push(format!("agents_dir {:?} exists", ws.defaults.agents_dir));
    } else {
        report.violations.push(format!(
            "agents_dir {:?} (from workspace.toml [defaults]) does not exist",
            ws.defaults.agents_dir
        ));
    }

    report
}

fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.splitn(3, '.').collect();
    if parts.len() != 3 {
        return false;
    }
    // Each part must be a non-empty run of ASCII digits with no leading zero
    // except "0" itself. SemVer also allows pre-release / build metadata after
    // X.Y.Z, but for workspace versions we keep it strict.
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn print_validation_report(
    root: &Path,
    report: &ValidationReport,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) {
    let root_display = root.display().to_string();
    let pass_label = i18n::t(bundle, "validate-label-pass");
    let fail_label = i18n::t(bundle, "validate-label-fail");
    let passes_count = report.passes.len().to_string();
    let violations_count = report.violations.len().to_string();

    println!();
    println!(
        "{}",
        i18n::t_with(bundle, "validate-header", &[("path", &root_display)])
    );
    println!("===========================================");
    for p in &report.passes {
        println!("{pass_label}  {p}");
    }
    for v in &report.violations {
        println!("{fail_label}  {v}");
    }
    println!("===========================================");
    if report.violations.is_empty() {
        println!(
            "{}",
            i18n::t_with(
                bundle,
                "validate-summary-success",
                &[("passes", &passes_count)],
            )
        );
    } else {
        println!(
            "{}",
            i18n::t_with(
                bundle,
                "validate-summary-failure",
                &[("passes", &passes_count), ("violations", &violations_count)],
            )
        );
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    fn fresh_dir(label: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("bwoc-ws-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn semver_validation() {
        assert!(is_semver("0.1.0"));
        assert!(is_semver("1.2.3"));
        assert!(is_semver("0.0.0"));
        assert!(!is_semver(""));
        assert!(!is_semver("1.2"));
        assert!(!is_semver("1.2.3.4"));
        assert!(!is_semver("v1.2.3"));
        assert!(!is_semver("1.2.x"));
    }

    #[test]
    fn validate_missing_dot_bwoc() {
        let dir = fresh_dir("missing");
        let report = validate(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains(".bwoc/ directory missing"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_clean_workspace_passes() {
        let dir = fresh_dir("clean");
        // Set up a minimal valid workspace by hand.
        let dot = dir.join(".bwoc");
        fs::create_dir_all(&dot).unwrap();
        fs::write(
            dot.join("workspace.toml"),
            r#"[workspace]
name = "demo"
version = "0.1.0"
created = "2026-05-22T06:00:00Z"

[defaults]
agents_dir = "agents"
"#,
        )
        .unwrap();
        fs::write(dot.join("agents.toml"), "").unwrap();
        fs::create_dir_all(dir.join("agents")).unwrap();
        let report = validate(&dir);
        assert!(
            report.violations.is_empty(),
            "violations: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_workspace_root_walks_ancestors() {
        let root = fresh_dir("walk");
        let dot = root.join(".bwoc");
        fs::create_dir_all(&dot).unwrap();
        fs::write(
            dot.join("workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\ncreated=\"x\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("nested/sub/dir")).unwrap();

        // Explicit path takes precedence.
        let explicit = find_workspace_root(Some(root.join("explicit")));
        assert_eq!(explicit, Some(root.join("explicit")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_workspace_rejects_missing_and_non_workspace_paths() {
        // A path that does not exist → Err(2).
        let missing_base = fresh_dir("ensure-missing");
        assert_eq!(
            ensure_workspace(&missing_base.join("gone"), "bwoc list"),
            Err(2)
        );

        // A path that exists but is a file (not a dir) → Err(2).
        let file_base = fresh_dir("ensure-file");
        let as_file = file_base.join("afile");
        fs::write(&as_file, "x").unwrap();
        assert_eq!(ensure_workspace(&as_file, "bwoc list"), Err(2));

        // A real dir with no .bwoc/workspace.toml → Err(2) (the #339 case: a
        // bogus explicit --workspace must not masquerade as an empty workspace).
        let bare = fresh_dir("ensure-bare");
        assert_eq!(ensure_workspace(&bare, "bwoc list"), Err(2));

        // A real workspace → Ok.
        let ws = fresh_dir("ensure-ok");
        let dot = ws.join(".bwoc");
        fs::create_dir_all(&dot).unwrap();
        fs::write(
            dot.join("workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\ncreated=\"x\"\n",
        )
        .unwrap();
        assert_eq!(ensure_workspace(&ws, "bwoc list"), Ok(()));

        let _ = fs::remove_dir_all(&missing_base);
        let _ = fs::remove_dir_all(&file_base);
        let _ = fs::remove_dir_all(&bare);
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn validate_bad_semver_fails() {
        let dir = fresh_dir("bad-semver");
        let dot = dir.join(".bwoc");
        fs::create_dir_all(&dot).unwrap();
        fs::write(
            dot.join("workspace.toml"),
            r#"[workspace]
name = "demo"
version = "0.1"
created = "2026-05-22T06:00:00Z"
"#,
        )
        .unwrap();
        fs::write(dot.join("agents.toml"), "").unwrap();
        fs::create_dir_all(dir.join("agents")).unwrap();
        let report = validate(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("not parseable SemVer"))
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
