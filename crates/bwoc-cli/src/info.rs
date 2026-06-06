//! `bwoc info` — a one-card system status: version + release identity + phase +
//! workspace + registered-agent count + update-drift status.
//!
//! Read-only and offline-safe: the update line reads the throttle cache the
//! background check already maintains (see `crate::update`), never the network.

use std::path::{Path, PathBuf};

use bwoc_core::workspace::AgentsRegistry;

/// Implementation phase. Manually bumped on a phase transition (rare); the
/// version numbers below are authoritative for releases.
const CURRENT_PHASE: &str = "Phase 3 — vaya + interconnect";

pub struct InfoArgs {
    pub workspace: Option<PathBuf>,
    pub json: bool,
}

pub fn run(args: InfoArgs) -> i32 {
    let version = env!("CARGO_PKG_VERSION");
    let release = option_env!("BWOC_RELEASE_CALVER"); // Some only on release builds

    let ws = resolve_workspace(args.workspace);
    let agents = ws
        .as_deref()
        .and_then(|w| AgentsRegistry::load(w).ok())
        .map(|r| r.agents.len());

    // Reuse the update check's cached drift status — no network here.
    let update = crate::update::info_status_line()
        .unwrap_or_else(|| "source build (no release identity)".to_string());

    if args.json {
        // Hand-rolled JSON keeps `info` dependency-free of serde derive here.
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let ws_json = match &ws {
            Some(w) => format!("\"{}\"", esc(&w.display().to_string())),
            None => "null".to_string(),
        };
        let agents_json = agents
            .map(|n| n.to_string())
            .unwrap_or_else(|| "null".to_string());
        let release_json = match release {
            Some(r) => format!("\"{}\"", esc(r)),
            None => "null".to_string(),
        };
        println!(
            "{{\"version\":\"{}\",\"release\":{},\"phase\":\"{}\",\"workspace\":{},\"agents\":{},\"update\":\"{}\"}}",
            esc(version),
            release_json,
            esc(CURRENT_PHASE),
            ws_json,
            agents_json,
            esc(&update),
        );
        return 0;
    }

    let release_str = match release {
        Some(r) => format!("release {r}"),
        None => "source build".to_string(),
    };
    println!("BWOC {version}  ({release_str})");
    println!("{CURRENT_PHASE}");
    match (&ws, agents) {
        (Some(w), Some(n)) => println!(
            "Workspace: {}  ({n} agent{})",
            w.display(),
            if n == 1 { "" } else { "s" }
        ),
        _ => println!("Workspace: (none — run `bwoc init`)"),
    }
    println!("Update: {update}   ·   handbook: bwoc handbook");
    0
}

/// Resolve the workspace root: explicit `--workspace`, then `BWOC_WORKSPACE`,
/// then an ancestor walk from the cwd for `.bwoc/workspace.toml`. `None` when no
/// workspace encloses the cwd (info still prints, just without agent counts).
fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Some(env) = std::env::var_os("BWOC_WORKSPACE") {
        return Some(PathBuf::from(env));
    }
    let cwd = std::env::current_dir().ok()?;
    let mut cur: Option<&Path> = Some(cwd.as_path());
    while let Some(dir) = cur {
        if dir.join(".bwoc/workspace.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_explicit() {
        let got = resolve_workspace(Some(PathBuf::from("/explicit/ws")));
        assert_eq!(got, Some(PathBuf::from("/explicit/ws")));
    }

    #[test]
    fn info_runs_without_a_workspace() {
        // A path with no .bwoc still returns 0 and prints the card (no panic).
        let code = run(InfoArgs {
            workspace: Some(PathBuf::from("/nonexistent-xyz")),
            json: true,
        });
        assert_eq!(code, 0);
    }
}
