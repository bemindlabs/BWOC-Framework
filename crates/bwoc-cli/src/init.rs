//! `bwoc init` — create a BWOC workspace at the given path.
//!
//! Phase 1 v2.0. Writes `.bwoc/workspace.toml` + `.bwoc/agents.toml` and the
//! configured `agents_dir`. Refuses if the workspace already exists
//! (idempotency); the caller can pass `--force` to overwrite.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bwoc_core::workspace::{AgentsRegistry, Workspace, WorkspaceDefaults, WorkspaceMeta};

use crate::i18n;
use crate::util::utc_now_iso8601;

pub struct InitArgs {
    pub path: Option<PathBuf>,
    pub force: bool,
    pub lang: String,
    /// Emit JSON `{ workspace, name, version, defaults, files_created }`
    /// instead of the human-readable creation report. Lets scripts chain
    /// init → other commands without parsing the report.
    pub json: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("path does not exist: {0}")]
    PathMissing(PathBuf),
    #[error("workspace already exists at {0} (pass --force to overwrite workspace.toml)")]
    AlreadyExists(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
}

pub fn run(args: InitArgs) -> i32 {
    let bundle = i18n::bundle_for(&args.lang);
    let json = args.json;
    match init(args) {
        Ok(ws_root) => {
            let path = ws_root.display().to_string();
            if json {
                // Load the workspace we just wrote to surface defaults
                // verbatim — gives consumers the agents_dir/backend/lang
                // configured at init time without a follow-up call.
                let ws = Workspace::load(&ws_root).ok();
                let defaults = ws.as_ref().map(|w| {
                    serde_json::json!({
                        "agents_dir": w.defaults.agents_dir,
                        "backend": w.defaults.backend,
                        "lang": w.defaults.lang,
                    })
                });
                let value = serde_json::json!({
                    "workspace": path,
                    "name": ws.as_ref().map(|w| w.workspace.name.clone()),
                    "version": ws.as_ref().map(|w| w.workspace.version.clone()),
                    "defaults": defaults,
                    "files_created": [
                        ".bwoc/workspace.toml",
                        ".bwoc/agents.toml",
                        ".bwoc/memory/README.md",
                        "agents/README.md",
                        "projects/README.md",
                        "notes/README.md",
                        ".gitignore"
                    ],
                });
                println!(
                    "{}",
                    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
                );
                return 0;
            }
            println!();
            println!(
                "{}",
                i18n::t_with(&bundle, "init-success-title", &[("path", &path)])
            );
            println!("{}", i18n::t(&bundle, "init-created-workspace-toml"));
            println!("{}", i18n::t(&bundle, "init-created-agents-toml"));
            println!("{}", i18n::t(&bundle, "init-created-agents-dir"));
            println!("{}", i18n::t(&bundle, "init-created-projects-dir"));
            println!("{}", i18n::t(&bundle, "init-created-notes-dir"));
            println!();
            println!("{}", i18n::t(&bundle, "init-next-steps-header"));
            println!(
                "{}",
                i18n::t_with(&bundle, "init-next-step-validate", &[("path", &path)])
            );
            println!("{}", i18n::t(&bundle, "init-next-step-new"));
            0
        }
        Err(e) => {
            // Error messages stay in English for now — thiserror localization
            // is its own challenge (deferred per Mattaññutā).
            eprintln!("bwoc init: {e}");
            match e {
                InitError::PathMissing(_) | InitError::AlreadyExists(_) => 2,
                _ => 1,
            }
        }
    }
}

fn init(args: InitArgs) -> Result<PathBuf, InitError> {
    let root = args
        .path
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !root.is_dir() {
        return Err(InitError::PathMissing(root));
    }

    let workspace_toml = root.join(".bwoc/workspace.toml");
    if workspace_toml.exists() && !args.force {
        return Err(InitError::AlreadyExists(root));
    }

    let name = workspace_name(&root);
    let created = utc_now_iso8601();

    let ws = Workspace {
        workspace: WorkspaceMeta {
            name,
            version: "0.1.0".to_string(),
            created,
        },
        defaults: WorkspaceDefaults::default(),
    };
    ws.save(&root)?;

    let registry = AgentsRegistry::default();
    registry.save(&root)?;

    // Create the agents/ directory + its README.
    let agents_dir = root.join(&ws.defaults.agents_dir);
    fs::create_dir_all(&agents_dir)?;
    write_readme_if_missing(&agents_dir, AGENTS_README)?;

    // Scaffold the standard workspace layout: empty dirs the user is
    // expected to populate, each with a README explaining its role.
    // `projects/` for the work agents help build, `notes/` for
    // implementation logs per `NAMING.en.md` §category 10
    // (`YYYY-MM-DD_<title>.md`). Pre-existing dirs and READMEs are
    // kept (idempotent — write_readme_if_missing skips existing files).
    for (dir, readme) in WORKSPACE_EXTRAS {
        let p = root.join(dir);
        fs::create_dir_all(&p)?;
        write_readme_if_missing(&p, readme)?;
    }

    // Write a sensible .gitignore if one doesn't exist. Idempotent —
    // don't clobber user edits.
    write_gitignore_if_missing(&root)?;

    Ok(root)
}

/// Write a `.gitignore` at the workspace root if none exists. Excludes
/// daemon ephemerals (`agent.pid`/`agent.sock`/`inbox.cursor`) that
/// regenerate on every `bwoc start`. `inbox.jsonl` is left tracked by
/// default — users may want message-log history checked in.
fn write_gitignore_if_missing(root: &Path) -> io::Result<()> {
    let path = root.join(".gitignore");
    if path.exists() {
        return Ok(());
    }
    fs::write(path, GITIGNORE_TEMPLATE)
}

/// Write `README.md` into `dir` if it doesn't already exist. Idempotent —
/// repeated `bwoc init --force` calls won't clobber user edits.
fn write_readme_if_missing(dir: &Path, content: &str) -> io::Result<()> {
    let readme = dir.join("README.md");
    if readme.exists() {
        return Ok(());
    }
    fs::write(readme, content)
}

const GITIGNORE_TEMPLATE: &str = "\
# BWOC workspace — daemon ephemerals
#
# These files regenerate on every `bwoc start` and shouldn't be
# committed. `bwoc doctor --auto` sweeps stale ones if a crash leaves
# them behind.
agents/*/.bwoc/agent.pid
agents/*/.bwoc/agent.sock
agents/*/.bwoc/inbox.cursor

# bwoc inbox messages — uncomment if you'd rather not track them.
# Most teams DO want them in history (audit trail / replay), so the
# default is to keep them tracked.
# agents/*/.bwoc/inbox.jsonl

# BWOC workspace — secret store (BWOC-53)
#
# Path convention for plugins that resolve credentials from disk —
# e.g. workflow/gcloud-auth + workflow/gcloud-project read
# `.bwoc/secrets/gcloud-sa.json`. Both the directory and the specific
# file are listed so the intent is explicit at the exact path the
# operator is told to drop credentials at. NEVER commit these files —
# real values live only here; auth.toml in each plugin carries SHAPE
# only (Sila — Adinnaadana).
.bwoc/secrets/
.bwoc/secrets/gcloud-sa.json

# Figma export cache (BWOC-64)
#
# The figma/figma-rest plugin renders Figma nodes into a content-addressable
# cache keyed on SHA-256(file_key + node_id + version + format). The renders
# are reproducible (re-export regenerates them) and binary, so they are never
# committed — committing them would bloat the repo for no durable benefit.
figma/exports/

# Generic local state — match the framework repo's own .gitignore.
.DS_Store
*.swp
*~
";

/// Standard sub-directories scaffolded by `bwoc init` (paired with the
/// README content written into each). The configured `agents_dir`
/// (from `ws.defaults`) is handled separately above.
const WORKSPACE_EXTRAS: &[(&str, &str)] = &[
    ("projects", PROJECTS_README),
    ("notes", NOTES_README),
    (".bwoc/memory", MEMORY_README),
];

const AGENTS_README: &str = "# agents/

Incarnated BWOC agents live here. Each subdirectory is one agent with
its own `AGENTS.md`, `config.manifest.json`, backend symlinks
(`CLAUDE.md` / `AGY.md` / `CODEX.md` / `KIMI.md` → `AGENTS.md`),
and slot dirs (`persona/`, `memories/`, `mindsets/`, `skills/`,
`interconnect/`).

## Commands

- `bwoc new <name>`       — incarnate a new agent here
- `bwoc list`             — see what's registered
- `bwoc check <name>`     — audit backend neutrality
- `bwoc retire <name>`    — remove an agent (registry + files)

See [`docs/en/INCARNATION.en.md`](../docs/en/INCARNATION.en.md) for
the full walkthrough.
";

const PROJECTS_README: &str = "# projects/

Your work — apps, repos, libraries the BWOC agents help you build.

This directory is yours. The framework doesn't enforce structure
here; populate it however your project conventions require (one
project per sub-directory is the obvious pattern).

Agents access projects via their `worktreeBase` setting or by being
spawned from a project directory: `bwoc spawn --path <project-dir>`.
";

const MEMORY_README: &str = "# .bwoc/memory/

Workspace-level memory — knowledge shared across all agents in this
workspace. Per the WORKSPACE.en.md spec §\"Central Memory\".

## Scope hierarchy

Memory in BWOC is layered:

1. **Per-agent**       — `agents/<name>/memories/` (one agent's recall)
2. **Per-workspace**   — `.bwoc/memory/` ← *this directory*
3. **Per-user**        — `~/.bwoc/memory/` (cross-workspace personal)
4. **Tier 2 backend**  — pluggable (vector store, etc.) — Phase 2+

Files in this directory are accessible by any agent in the workspace
and should encode shared context — coding standards, project glossary,
deployment recipes, common gotchas.

## Naming convention

Plain Markdown files. Use kebab-case filenames per
[`docs/en/NAMING.en.md`](../../docs/en/NAMING.en.md) categories 5–7
(slot-level READMEs, reference docs).

## What goes here

- Cross-agent conventions (\"all agents in this workspace use 2-space indent\")
- Shared glossary terms
- Deployment recipes, release procedures
- Cross-cutting gotchas surfaced by one agent that others should know

## What does NOT go here

- Per-agent specific knowledge → use `agents/<name>/memories/` instead
- Personal preferences → use `~/.bwoc/memory/` instead
- Secrets → never commit
";

const NOTES_README: &str = "# notes/

Implementation notes, decisions, and design logs.

## Naming convention

Files follow `YYYY-MM-DD_<title>.md` per
[`docs/en/NAMING.en.md`](../docs/en/NAMING.en.md) category 10.
Example: `2026-05-22_workspace-design.md`.

## What goes here

Development-oriented context — what changed, *why*, decisions made,
alternatives considered, bugs surfaced and fixed. Distinct from
`CHANGELOG.md` (release-oriented) and per-agent `memories/`
(scoped to one agent's perspective).
";

fn workspace_name(root: &Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("workspace")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn fresh_dir(label: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("bwoc-init-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn args(dir: &Path, force: bool) -> InitArgs {
        InitArgs {
            path: Some(dir.to_path_buf()),
            force,
            lang: "en".to_string(),
            json: false,
        }
    }

    #[test]
    fn init_creates_workspace_files() {
        let dir = fresh_dir("creates");
        let code = run(args(&dir, false));
        assert_eq!(code, 0);
        assert!(dir.join(".bwoc/workspace.toml").exists());
        assert!(dir.join(".bwoc/agents.toml").exists());
        assert!(dir.join("agents").is_dir());
        assert!(dir.join("projects").is_dir());
        assert!(dir.join("notes").is_dir());
        // Each scaffold dir now ships with a README explaining its role.
        assert!(dir.join("agents/README.md").is_file());
        assert!(dir.join("projects/README.md").is_file());
        assert!(dir.join("notes/README.md").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_preserves_existing_readme() {
        // If a README already exists (user edited it), `bwoc init --force`
        // must not clobber it.
        let dir = fresh_dir("readme-keep");
        // Pre-create one of the scaffold dirs with a custom README.
        fs::create_dir_all(dir.join("notes")).unwrap();
        fs::write(dir.join("notes/README.md"), "# my custom notes readme").unwrap();
        let code = run(args(&dir, false));
        assert_eq!(code, 0);
        let content = fs::read_to_string(dir.join("notes/README.md")).unwrap();
        assert_eq!(content, "# my custom notes readme");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_refuses_existing_workspace_without_force() {
        let dir = fresh_dir("refuses");
        assert_eq!(run(args(&dir, false)), 0);
        assert_eq!(run(args(&dir, false)), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_force_overwrites() {
        let dir = fresh_dir("force");
        assert_eq!(run(args(&dir, false)), 0);
        assert_eq!(run(args(&dir, true)), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
