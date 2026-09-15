//! `bwoc memory wake-up | search <query> | mine <path> [--mode <mode>]`
//!
//! Tier 2 deep-memory CLI surface.  Resolves the agent's `deepMemoryCmd` from
//! its `config.manifest.json` via the workspace registry, then dispatches to
//! [`bwoc_core::deep_memory`].
//!
//! **Non-fatal by design.** When Tier 2 is not configured (no `deepMemoryCmd`
//! or the placeholder `# (Tier 2 not configured)`), every subcommand prints a
//! one-line status message and exits 0.  The agent continues to function
//! normally on Tier 1 alone.
//!
//! **Tier 1 is unchanged.** The existing `bwoc memory list | show | put |
//! search | rm` subcommands live in `memory.rs` and are not touched here.

use std::path::PathBuf;

use bwoc_core::deep_memory::{self, DeepMemoryStatus};
use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;

// ---------------------------------------------------------------------------
// Public types consumed by main.rs
// ---------------------------------------------------------------------------

pub struct Tier2Args {
    pub action: Tier2Action,
    /// Agent name (with or without `agent-` prefix).
    pub agent: String,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk.
    pub workspace: Option<PathBuf>,
}

pub enum Tier2Action {
    /// `bwoc memory wake-up` — emit prior context at session start.
    WakeUp,
    /// `bwoc memory search --tier 2 <query> <agent>` — find relevant past decisions.
    Search { query: String },
    /// `bwoc memory mine <path> [--mode <mode>]` — persist session learnings.
    Mine { path: PathBuf, mode: String },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(args: Tier2Args) -> i32 {
    // --- resolve workspace --------------------------------------------------
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc memory (Tier 2): no workspace found. \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`."
        );
        return 2;
    };

    // --- resolve agent manifest --------------------------------------------
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc memory (Tier 2): failed to read agents.toml: {e}");
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
            "bwoc memory (Tier 2): no agent named '{}' in workspace {}. \
             Try `bwoc list` to see registered agents.",
            args.agent,
            workspace.display()
        );
        return 2;
    };
    let manifest_path = workspace.join(&entry.path).join("config.manifest.json");
    let manifest = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "bwoc memory (Tier 2): failed to read {}: {e}",
                manifest_path.display()
            );
            return 1;
        }
    };

    // --- build backend from manifest ---------------------------------------
    let backend = deep_memory::from_manifest_cmd(manifest.deep_memory_cmd.as_deref());

    if backend.status() == DeepMemoryStatus::Disabled {
        println!(
            "Tier 2 deep memory is not configured for agent '{}'. \
             Set deepMemoryCmd in {} to enable it.",
            args.agent,
            manifest_path.display()
        );
        return 0; // non-fatal — Tier 2 absence must not break anything
    }

    // --- dispatch -----------------------------------------------------------
    match args.action {
        Tier2Action::WakeUp => match backend.wake_up() {
            Ok(output) => {
                print!("{output}");
                0
            }
            Err(e) => {
                eprintln!("bwoc memory wake-up: {e}");
                1
            }
        },
        Tier2Action::Search { query } => match backend.search(&query) {
            Ok(output) => {
                print!("{output}");
                0
            }
            Err(e) => {
                eprintln!("bwoc memory search --tier 2: {e}");
                1
            }
        },
        Tier2Action::Mine { path, mode } => match backend.mine(&path, &mode) {
            Ok(()) => {
                println!("Tier 2 mine complete: {} (mode={})", path.display(), mode);
                0
            }
            Err(e) => {
                eprintln!("bwoc memory mine: {e}");
                1
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution (mirrors memory.rs / trust.rs pattern)
// ---------------------------------------------------------------------------

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
