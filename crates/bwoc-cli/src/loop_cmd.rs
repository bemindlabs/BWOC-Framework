//! `bwoc loop` — launch the Loop-Engineering control-center TUI.
//!
//! Thin CLI seam over the [`bwoc_loop_tui`] crate: it resolves the workspace
//! root (the standard `--workspace` > `BWOC_WORKSPACE` > ancestor-walk chain)
//! and hands a concrete, existing directory to the TUI, which owns all the
//! ratatui rendering. Keeping resolution here mirrors `bwoc dashboard` and keeps
//! the TUI crate free of CLI-resolution logic (dep-quarantine: `bwoc-loop-tui`
//! depends only on `bwoc-core`).

use std::path::PathBuf;

/// Runtime args for `bwoc loop` (already-parsed from the clap layer).
pub struct LoopArgs {
    /// Workspace root. `None` → BWOC_WORKSPACE env, then ancestor walk.
    pub workspace: Option<PathBuf>,
    /// Team to open first. `None` → the first team alphabetically.
    pub team: Option<String>,
    /// Ticker cadence (seconds) the goal-loop shows / starts with.
    pub ticker_secs: u64,
    /// Iteration budget the goal-loop shows / starts with (`0` = unbounded).
    pub budget_iters: usize,
    /// Backend forwarded to `bwoc-harness --backend` when a loop starts.
    pub backend: Option<String>,
    /// Model forwarded to `bwoc-harness --model`.
    pub model: Option<String>,
    /// Endpoint forwarded to `bwoc-harness --endpoint`.
    pub endpoint: Option<String>,
}

/// Resolve the workspace, then run the loop TUI. Returns a process exit code.
pub fn run(args: LoopArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc loop: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    bwoc_loop_tui::run(bwoc_loop_tui::LoopTuiArgs {
        workspace,
        team: args.team,
        ticker_secs: args.ticker_secs,
        budget_iters: args.budget_iters,
        backend: args.backend,
        model: args.model,
        endpoint: args.endpoint,
    })
}

/// Standard workspace resolution: explicit flag > `BWOC_WORKSPACE` > walk
/// ancestors for `.bwoc/workspace.toml`. Mirrors `dashboard::resolve_workspace`.
fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE")
        && !env_path.is_empty()
    {
        return Some(PathBuf::from(env_path));
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
