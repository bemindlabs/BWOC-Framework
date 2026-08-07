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
    /// Emit JSON `{ workspace, name, version, defaults, runtime, profile,
    /// files_created }` instead of the human-readable creation report. Lets
    /// scripts chain init → other commands without parsing the report.
    pub json: bool,
    /// Scaffold without agent runtime/daemon provisioning (CI / read-only /
    /// inspection workspaces). Omits the daemon-ephemeral `.gitignore`
    /// patterns; the workspace stays valid.
    pub no_runtime: bool,
    /// Scaffold a single-agent workspace (one agent slot) instead of the
    /// multi-agent fleet default.
    pub single_agent: bool,
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
    // Capture the mode flags before `args` is moved into `init` so the JSON
    // shape + human report can echo the chosen profile back to the caller.
    let runtime_enabled = !args.no_runtime;
    let single_agent = args.single_agent;
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
                    "runtime": runtime_enabled,
                    "profile": if single_agent { "single-agent" } else { "fleet" },
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
            // Mode notes — printed in English (like the error path) only when
            // a non-default flag is set, so default output is unchanged.
            if !runtime_enabled {
                println!("  • runtime/daemon provisioning skipped (--no-runtime)");
            }
            if single_agent {
                println!("  • single-agent workspace (--single-agent)");
            }
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

    // Create the agents/ directory + its README. `--single-agent` swaps in
    // single-agent-oriented guidance instead of the fleet default.
    let agents_dir = root.join(&ws.defaults.agents_dir);
    fs::create_dir_all(&agents_dir)?;
    let agents_readme = if args.single_agent {
        AGENTS_README_SINGLE
    } else {
        AGENTS_README
    };
    write_readme_if_missing(&agents_dir, agents_readme)?;

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
    // don't clobber user edits. `--no-runtime` drops the daemon-ephemeral
    // block (the workspace never spawns agents, so those files never appear).
    write_gitignore_if_missing(&root, args.no_runtime)?;

    Ok(root)
}

/// Write a `.gitignore` at the workspace root if none exists. The default
/// excludes daemon ephemerals (`agent.pid`/`agent.sock`/`inbox.cursor`) that
/// regenerate on every `bwoc start`; `inbox.jsonl` is left tracked by default
/// — users may want message-log history checked in. When `no_runtime` is set,
/// the daemon-ephemeral block is replaced by a short note, since a runtime-less
/// workspace never produces those files.
fn write_gitignore_if_missing(root: &Path, no_runtime: bool) -> io::Result<()> {
    let path = root.join(".gitignore");
    if path.exists() {
        return Ok(());
    }
    let header = if no_runtime {
        GITIGNORE_NO_RUNTIME_NOTE
    } else {
        GITIGNORE_DAEMON_BLOCK
    };
    fs::write(path, format!("{header}{GITIGNORE_REST}"))
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

/// Default `.gitignore` head: the daemon-ephemeral block. Concatenated with
/// [`GITIGNORE_REST`] this reproduces the historical full template byte-for-byte,
/// so the default (no-flag) `bwoc init` output is unchanged.
const GITIGNORE_DAEMON_BLOCK: &str = "\
# BWOC workspace — daemon ephemerals
#
# These files regenerate on every `bwoc start` and shouldn't be
# committed. `bwoc doctor --auto` sweeps stale ones if a crash leaves
# them behind.
agents/*/.bwoc/agent.pid
agents/*/.bwoc/agent.sock
agents/*/.bwoc/inbox.cursor

# bwoc outbox — sender-side durable retry queue (messages spooled because a
# remote peer was offline; drained by `bwoc outbox flush`). Transient per-machine
# delivery state, not an audit trail, so it is not tracked.
.bwoc/outbox/

# bwoc inbox messages — uncomment if you'd rather not track them.
# Most teams DO want them in history (audit trail / replay), so the
# default is to keep them tracked.
# agents/*/.bwoc/inbox.jsonl

";

/// `--no-runtime` `.gitignore` head: replaces the daemon-ephemeral block with
/// a note explaining why those patterns are absent. A runtime-less workspace
/// never spawns agents, so the daemon ephemerals never appear.
const GITIGNORE_NO_RUNTIME_NOTE: &str = "\
# BWOC workspace — runtime provisioning skipped (--no-runtime)
#
# This workspace was initialized without agent runtime/daemon setup, so
# the daemon-ephemeral ignore patterns (agent.pid / agent.sock /
# inbox.cursor) are intentionally omitted — a runtime-less workspace
# never produces them. Re-run `bwoc init --force` without --no-runtime
# to add them if you later decide to spawn agents.

";

/// Shared `.gitignore` tail (secret store, figma cache, generic local state).
/// Appended after either head above.
const GITIGNORE_REST: &str = "\
# BWOC workspace — secret store (BWOC-53)
#
# Path convention for plugins that resolve credentials from disk —
# e.g. workflow/gcloud-auth + workflow/gcloud-project read
# `.bwoc/secrets/gcloud-sa.json`, and gws/gws-auth reads
# `.bwoc/secrets/gws-token.json`. Both the directory and the specific
# files are listed so the intent is explicit at the exact path the
# operator is told to drop credentials at. NEVER commit these files —
# real values live only here; auth.toml in each plugin carries SHAPE
# only (Sila — Adinnaadana).
.bwoc/secrets/
.bwoc/secrets/gcloud-sa.json
.bwoc/secrets/gws-token.json

# BWOC agent signing keys (Trust v2 — SIGNING.en.md)
#
# `bwoc trust --keygen` writes an ed25519 PRIVATE key to
# agents/<id>/.bwoc/agent.key (mode 0600 on Unix). The matching PUBLIC key lives
# in the agent's manifest (trust.signingPublicKey) and IS tracked. NEVER
# commit the private key — it is the agent's identity (Sila — Adinnaadana).
agents/*/.bwoc/agent.key

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

const AGENTS_README_SINGLE: &str = "# agents/ (single-agent workspace)

This workspace was initialized with `--single-agent`: it holds a single
incarnated BWOC agent rather than the multi-agent fleet. The directory
layout is identical — one subdirectory with the agent's `AGENTS.md`,
`config.manifest.json`, backend symlinks (`CLAUDE.md` / `AGY.md` /
`CODEX.md` / `KIMI.md` → `AGENTS.md`), and slot dirs (`persona/`,
`memories/`, `mindsets/`, `skills/`, `interconnect/`).

## Commands

- `bwoc new <name>`       — incarnate the agent here
- `bwoc status <name>`    — health + identity snapshot for the one agent
- `bwoc check <name>`     — audit backend neutrality
- `bwoc retire <name>`    — remove the agent (registry + files)

You can grow into a fleet at any time — `bwoc new` a second agent and the
multi-agent commands (`bwoc list`, `bwoc fleet`) apply unchanged.

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
            no_runtime: false,
            single_agent: false,
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

    // --- BWOC-71: --no-runtime / --single-agent ---------------------------

    /// Default (no flags): the .gitignore carries the daemon-ephemeral block
    /// and the agents/ README is the fleet variant. Guards the AC "existing
    /// behavior unchanged when flags absent".
    #[test]
    fn init_default_runtime_and_fleet() {
        let dir = fresh_dir("default-mode");
        assert_eq!(run(args(&dir, false)), 0);
        let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("agents/*/.bwoc/agent.pid"));
        assert!(gitignore.contains("daemon ephemerals"));
        // Trust v2 private signing key must never be committed (Adinnaadana).
        assert!(gitignore.contains("agents/*/.bwoc/agent.key"));
        // Default head + shared tail reproduce the historical full template.
        assert_eq!(
            gitignore,
            format!("{GITIGNORE_DAEMON_BLOCK}{GITIGNORE_REST}")
        );
        let readme = fs::read_to_string(dir.join("agents/README.md")).unwrap();
        assert!(!readme.contains("single-agent workspace"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// `--no-runtime` omits the daemon-ephemeral patterns but still writes a
    /// valid workspace (workspace.toml + agents.toml present).
    #[test]
    fn init_no_runtime_omits_daemon_gitignore() {
        let dir = fresh_dir("no-runtime");
        let mut a = args(&dir, false);
        a.no_runtime = true;
        assert_eq!(run(a), 0);
        let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
        // The ignore *patterns* are gone (the prose note may still name them).
        assert!(!gitignore.contains("agents/*/.bwoc/agent.pid"));
        assert!(!gitignore.contains("agents/*/.bwoc/agent.sock"));
        assert!(gitignore.contains("runtime provisioning skipped"));
        // Shared tail (secret store etc.) is still present.
        assert!(gitignore.contains(".bwoc/secrets/"));
        // The private signing key is a secret in the shared tail — ignored
        // even when the daemon block is omitted.
        assert!(gitignore.contains("agents/*/.bwoc/agent.key"));
        // Workspace stays valid.
        assert!(dir.join(".bwoc/workspace.toml").exists());
        assert!(dir.join(".bwoc/agents.toml").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// `--single-agent` scaffolds the single-agent README variant.
    #[test]
    fn init_single_agent_readme() {
        let dir = fresh_dir("single-agent");
        let mut a = args(&dir, false);
        a.single_agent = true;
        assert_eq!(run(a), 0);
        let readme = fs::read_to_string(dir.join("agents/README.md")).unwrap();
        assert!(readme.contains("single-agent workspace"));
        // Daemon gitignore block is untouched by --single-agent.
        let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains("agents/*/.bwoc/agent.pid"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// The two flags compose: no daemon block AND single-agent README.
    #[test]
    fn init_flags_compose() {
        let dir = fresh_dir("compose");
        let mut a = args(&dir, false);
        a.no_runtime = true;
        a.single_agent = true;
        assert_eq!(run(a), 0);
        let gitignore = fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert!(!gitignore.contains("agents/*/.bwoc/agent.pid"));
        assert!(gitignore.contains("runtime provisioning skipped"));
        let readme = fs::read_to_string(dir.join("agents/README.md")).unwrap();
        assert!(readme.contains("single-agent workspace"));
        let _ = fs::remove_dir_all(&dir);
    }
}
