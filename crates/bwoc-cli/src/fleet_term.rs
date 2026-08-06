//! `bwoc fleet term` — open a terminal for every agent in the fleet and arrange
//! the panes with a chosen layout.
//!
//! Portable across **macOS + Linux**: tmux is the one terminal multiplexer both
//! ship (or install identically), and its `select-layout` gives the layout
//! arrangements for free — equal grid, columns, rows, or a main + stack. Each
//! agent gets its own pane running `bwoc spawn` in the agent's directory, with
//! the pane border titled by the agent id so the grid stays legible.
//!
//! OS-native window tiling (separate Ghostty / Terminal.app windows positioned
//! on the desktop) is a mac-only follow-up — deliberately not this command.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use bwoc_core::workspace::AgentsRegistry;

use crate::chat::resolve_workspace;
use crate::spawn;

/// The pane arrangement. Each maps to a built-in tmux layout so the choice is
/// applied with a single `select-layout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum TmuxLayout {
    /// Equal tiles in a grid — tmux `tiled`. The default; scales best past ~4 agents.
    Grid,
    /// One column per agent, side by side — tmux `even-horizontal`.
    Columns,
    /// One row per agent, stacked top to bottom — tmux `even-vertical`.
    Rows,
    /// A large main pane on the left, the rest stacked on the right — tmux `main-vertical`.
    MainVertical,
    /// A large main pane on top, the rest in a row below — tmux `main-horizontal`.
    MainHorizontal,
}

impl TmuxLayout {
    fn tmux_name(self) -> &'static str {
        match self {
            TmuxLayout::Grid => "tiled",
            TmuxLayout::Columns => "even-horizontal",
            TmuxLayout::Rows => "even-vertical",
            TmuxLayout::MainVertical => "main-vertical",
            TmuxLayout::MainHorizontal => "main-horizontal",
        }
    }
}

pub struct FleetTermArgs {
    pub workspace: Option<PathBuf>,
    pub layout: TmuxLayout,
    pub session: String,
    /// Build the session but do not attach — just print the attach command.
    /// Implied when stdout is not a TTY (e.g. a script / the control center).
    pub print: bool,
}

/// Build the ordered tmux command sequence (each inner `Vec` is one `tmux`
/// invocation's argv, program name excluded) that opens one pane per agent in
/// `session` and arranges them by `layout`. Pure + tested.
///
/// `agents` is `(agent_id, abs_path, backend)`. The first agent seeds the
/// session window; each subsequent agent adds a `split-window`. The grid is
/// rebalanced with `tiled` *after every split* so tmux never runs out of room
/// ("no space for new pane") on a fleet of many agents; a final `select-layout`
/// applies the requested arrangement. Each pane's border is titled with the
/// agent id (`pane-border-status top` makes those visible).
pub(crate) fn tmux_fleet_commands(
    session: &str,
    agents: &[(String, String, String)],
    layout: TmuxLayout,
    bwoc_exe: &str,
) -> Vec<Vec<String>> {
    let spawn_argv = |path: &str, backend: &str| -> Vec<String> {
        vec![
            "--".into(),
            bwoc_exe.into(),
            "spawn".into(),
            "--path".into(),
            path.into(),
            "--backend".into(),
            backend.into(),
        ]
    };
    let mut cmds: Vec<Vec<String>> = Vec::new();
    let Some(((id0, p0, b0), rest)) = agents.split_first() else {
        return cmds; // no agents — the caller refuses before running
    };

    // Detached session with the first agent in window 0.
    let mut first = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        session.into(),
        "-n".into(),
        "fleet".into(),
    ];
    first.extend(spawn_argv(p0, b0));
    cmds.push(first);
    // Keep a pane visible after its agent exits (dead panes read "[exited]")
    // instead of closing — so one finished/crashed agent can't collapse the
    // layout (or the whole session, if it were the last pane). Set immediately
    // after the session exists, before any split.
    cmds.push(vec![
        "set-option".into(),
        "-t".into(),
        session.into(),
        "remain-on-exit".into(),
        "on".into(),
    ]);
    cmds.push(title_cmd(session, id0));

    // One pane per remaining agent; rebalance to `tiled` between splits so the
    // next split always has room, then title the freshly-created (active) pane.
    for (id, path, backend) in rest {
        let mut split = vec!["split-window".into(), "-t".into(), session.into()];
        split.extend(spawn_argv(path, backend));
        cmds.push(split);
        cmds.push(vec![
            "select-layout".into(),
            "-t".into(),
            session.into(),
            "tiled".into(),
        ]);
        cmds.push(title_cmd(session, id));
    }

    // Show the pane-border titles, then apply the requested layout last.
    cmds.push(vec![
        "set-option".into(),
        "-t".into(),
        session.into(),
        "pane-border-status".into(),
        "top".into(),
    ]);
    cmds.push(vec![
        "select-layout".into(),
        "-t".into(),
        session.into(),
        layout.tmux_name().into(),
    ]);
    cmds
}

/// Title the *active* pane of `session` with `id`.
fn title_cmd(session: &str, id: &str) -> Vec<String> {
    vec![
        "select-pane".into(),
        "-t".into(),
        session.into(),
        "-T".into(),
        id.into(),
    ]
}

fn tmux_missing() -> bool {
    Command::new("tmux")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
}

fn session_exists(session: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", session])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn run(args: FleetTermArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc fleet term: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc fleet term: failed to read agents registry: {e}");
            return 1;
        }
    };
    if registry.agents.is_empty() {
        eprintln!(
            "bwoc fleet term: no agents in {} — incarnate one with `bwoc new`.",
            workspace.display()
        );
        return 2;
    }
    if tmux_missing() {
        eprintln!(
            "bwoc fleet term: tmux not found on PATH. Install it (macOS: `brew install tmux`, \
             Linux: `apt install tmux`) — this command lays the fleet out in tmux panes."
        );
        return 2;
    }
    if session_exists(&args.session) {
        eprintln!(
            "bwoc fleet term: tmux session '{0}' already exists — attach with `tmux attach -t {0}`, \
             kill it with `tmux kill-session -t {0}`, or pass a different --session.",
            args.session
        );
        return 2; // user/input error — consistent with the refusals above
    }

    let bwoc = spawn::bwoc_exe();
    let agents: Vec<(String, String, String)> = registry
        .agents
        .iter()
        .map(|a| {
            (
                a.id.clone(),
                workspace.join(&a.path).to_string_lossy().into_owned(),
                a.backend.clone(),
            )
        })
        .collect();

    for cmd in tmux_fleet_commands(&args.session, &agents, args.layout, &bwoc) {
        match Command::new("tmux")
            .args(&cmd)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!(
                    "bwoc fleet term: `tmux {}` exited {s} — the session may be partially built; \
                     `tmux kill-session -t {}` to clear it.",
                    cmd.first().map(String::as_str).unwrap_or("?"),
                    args.session
                );
                return 1;
            }
            Err(e) => {
                eprintln!("bwoc fleet term: failed to run tmux: {e}");
                return 1;
            }
        }
    }

    println!(
        "Opened {} agent panes in tmux session '{}' (layout: {}). \
         Cycle layouts live with `<prefix> Space`.",
        agents.len(),
        args.session,
        args.layout.tmux_name()
    );

    // Attach unless asked not to (or no TTY to attach to).
    if args.print || !std::io::stdout().is_terminal() {
        println!("Attach with:  tmux attach -t {}", args.session);
        return 0;
    }
    match Command::new("tmux")
        .args(["attach", "-t", &args.session])
        .status()
    {
        Ok(_) => 0,
        Err(e) => {
            eprintln!(
                "bwoc fleet term: attach failed: {e} — run `tmux attach -t {}` manually.",
                args.session
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agents() -> Vec<(String, String, String)> {
        vec![
            (
                "agent-pi".into(),
                "/ws/agents/agent-pi".into(),
                "claude".into(),
            ),
            (
                "agent-ji".into(),
                "/ws/agents/agent-ji".into(),
                "ollama".into(),
            ),
            (
                "agent-mu".into(),
                "/ws/agents/agent-mu".into(),
                "codex".into(),
            ),
        ]
    }

    #[test]
    fn first_agent_seeds_the_session_and_spawns() {
        let c = tmux_fleet_commands("bwoc-fleet", &agents(), TmuxLayout::Grid, "/bin/bwoc");
        let first = &c[0];
        assert_eq!(first[0], "new-session");
        assert!(first.contains(&"-d".to_string()), "detached: {first:?}");
        assert!(first.windows(2).any(|w| w == ["-s", "bwoc-fleet"]));
        // spawns the first agent in its dir with its backend
        assert!(
            first
                .windows(2)
                .any(|w| w == ["--path", "/ws/agents/agent-pi"])
        );
        assert!(first.windows(2).any(|w| w == ["--backend", "claude"]));
        // remain-on-exit is set right after the session is created so an exiting
        // agent can't collapse the layout.
        assert!(
            c.iter()
                .any(|cmd| cmd.contains(&"remain-on-exit".to_string()))
        );
    }

    #[test]
    fn one_split_per_additional_agent_and_final_layout() {
        let c = tmux_fleet_commands("s", &agents(), TmuxLayout::Columns, "bwoc");
        let splits = c
            .iter()
            .filter(|cmd| cmd.first().map(String::as_str) == Some("split-window"))
            .count();
        assert_eq!(splits, 2, "3 agents → 2 splits");
        // rebalanced to tiled between splits so tmux never runs out of room
        assert!(c.iter().any(
            |cmd| cmd.first().map(String::as_str) == Some("select-layout")
                && cmd.contains(&"tiled".to_string())
        ));
        // the LAST command applies the requested layout
        let last = c.last().unwrap();
        assert_eq!(last[0], "select-layout");
        assert_eq!(last.last().unwrap(), "even-horizontal");
    }

    #[test]
    fn every_agent_pane_is_titled() {
        let c = tmux_fleet_commands("s", &agents(), TmuxLayout::Grid, "bwoc");
        for id in ["agent-pi", "agent-ji", "agent-mu"] {
            assert!(
                c.iter()
                    .any(|cmd| cmd.first().map(String::as_str) == Some("select-pane")
                        && cmd.contains(&id.to_string())),
                "pane titled for {id}"
            );
        }
        assert!(
            c.iter()
                .any(|cmd| cmd.contains(&"pane-border-status".to_string()))
        );
    }

    #[test]
    fn no_agents_yields_no_commands() {
        assert!(tmux_fleet_commands("s", &[], TmuxLayout::Grid, "bwoc").is_empty());
    }

    #[test]
    fn layout_names_map_to_tmux() {
        assert_eq!(TmuxLayout::Grid.tmux_name(), "tiled");
        assert_eq!(TmuxLayout::Rows.tmux_name(), "even-vertical");
        assert_eq!(TmuxLayout::MainVertical.tmux_name(), "main-vertical");
        assert_eq!(TmuxLayout::MainHorizontal.tmux_name(), "main-horizontal");
    }
}
