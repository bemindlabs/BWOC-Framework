//! `bwoc dashboard` — Phase 1 TUI.
//!
//! Phase 0 (shipped): bare ratatui shell that draws a title + empty
//! body + footer; quits cleanly on `q`/`Esc`/`Ctrl-C`.
//!
//! Phase 1 (this file): populates the agents pane from
//! `.bwoc/agents.toml` with `↑`/`↓` (or `j`/`k`) navigation, a
//! highlighted selection row, and `r` to refresh.
//!
//! Phase 2 (shipped): detail pane reusing `doctor`-style probes.
//! Phase 3: Fluent i18n.
//! Phase 4 (this slice, issue #45): live agent-activity — an activity
//! column on each row fed by the `sessions` resolution (working/idle/
//! running/—) on the 2s tick, plus session detail (backend/pid/last-
//! activity) and a log tail in the detail pane. Observe-only: the
//! dashboard reads session state, it never drives a session.

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::i18n;
use crate::sessions::{ProcessScanRunner, Session, SessionState, collect_sessions};
use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::{AgentEntry, AgentsRegistry};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use fluent_bundle::{FluentBundle, FluentResource};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

pub struct DashboardArgs {
    pub workspace: Option<PathBuf>,
    pub lang: String,
}

pub fn run(args: DashboardArgs) -> i32 {
    use std::io::IsTerminal;
    if !io::stdout().is_terminal() {
        eprintln!(
            "bwoc dashboard: stdout is not a TTY. Use `bwoc list` / `bwoc status` for non-interactive output."
        );
        return 2;
    }

    let mut app = App::new(args.workspace, args.lang);

    let mut term = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc dashboard: failed to enter alt screen: {e}");
            return 1;
        }
    };

    let result = event_loop(&mut term, &mut app);

    if let Err(e) = restore_terminal() {
        eprintln!("bwoc dashboard: warning — failed to restore terminal: {e}");
    }

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc dashboard: {e}");
            1
        }
    }
}

// --- app state ------------------------------------------------------------

/// How often the dashboard auto-refreshes the agents registry. Live-
/// changing data (runtime indicators, uptime, inbox counts) re-reads
/// on every draw regardless — this only controls how often we re-load
/// agents.toml itself. 2s is fast enough to feel live without thrashing
/// the disk for huge registries.
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Inactivity threshold (seconds) before a live session is reported
/// `idle` rather than `working`. Matches `bwoc sessions`' default so the
/// dashboard and the CLI agree on what "idle" means.
const IDLE_SECS: u64 = 60;

/// Lines of the selected agent's log shown in the detail pane. Capped so
/// the tail never crowds out the manifest/health summary above it
/// (mattaññutā — right amount). The pane clips anything past its height.
const DETAIL_LOG_TAIL_LINES: usize = 10;

struct App {
    workspace: Option<PathBuf>,
    agents: Vec<AgentEntry>,
    table_state: TableState,
    last_refresh_error: Option<String>,
    bundle: FluentBundle<FluentResource>,
    last_refresh_at: Instant,
    /// Snapshot of live sessions from `collect_sessions`, recomputed on
    /// each 2s tick (NOT per-draw — the scan shells out to `pgrep`/`tmux`
    /// and must not run 5×/sec). Feeds the activity column + detail pane.
    sessions: Vec<Session>,
    /// Tail of the selected agent's `.bwoc/agent.log`, refreshed on tick
    /// and on selection change. Empty when no log / nothing selected.
    detail_log: Vec<String>,
    /// One-line note shown in place of the log tail when it is empty —
    /// e.g. "(no log yet)" or a read error. None when `detail_log` has
    /// content.
    detail_log_note: Option<String>,
    /// One-line transient feedback from the last user action (e.g. tmux
    /// open result). Shown in the footer; cleared on next action.
    last_action: Option<String>,
    /// Toggled by `?` — overlay listing every hotkey. Any keypress
    /// dismisses it (including `?`). Doesn't block auto-refresh.
    show_help: bool,
}

impl App {
    fn new(workspace_arg: Option<PathBuf>, lang: String) -> Self {
        let workspace = resolve_workspace(workspace_arg);
        let mut app = Self {
            workspace,
            agents: Vec::new(),
            table_state: TableState::default(),
            last_refresh_error: None,
            bundle: i18n::bundle_for(&lang),
            last_refresh_at: Instant::now(),
            last_action: None,
            show_help: false,
            sessions: Vec::new(),
            detail_log: Vec::new(),
            detail_log_note: None,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.last_refresh_at = Instant::now();
        if let Some(root) = &self.workspace {
            match AgentsRegistry::load(root) {
                Ok(r) => {
                    self.agents = r.agents;
                    self.last_refresh_error = None;
                    // Keep selection valid as the registry shrinks/grows.
                    if self.agents.is_empty() {
                        self.table_state.select(None);
                    } else {
                        let cur = self.table_state.selected().unwrap_or(0);
                        self.table_state
                            .select(Some(cur.min(self.agents.len() - 1)));
                    }
                }
                Err(e) => {
                    self.agents.clear();
                    self.last_refresh_error = Some(format!("agents.toml: {e}"));
                    self.table_state.select(None);
                }
            }
            // Recompute the live-session snapshot on the tick (not per-draw).
            // `collect_sessions` is best-effort: missing markers/sessions dir
            // → empty Vec, so the activity column degrades to "—" silently.
            self.sessions = collect_sessions(root, &ProcessScanRunner, IDLE_SECS);
        } else {
            self.agents.clear();
            self.last_refresh_error = None;
            self.table_state.select(None);
            self.sessions.clear();
        }
        self.refresh_detail();
    }

    /// Reload the selected agent's log tail. Called on the 2s tick and on
    /// every selection change so the detail pane never lags more than the
    /// user's own keystroke. Reads only the last `DETAIL_LOG_TAIL_LINES`
    /// lines; a missing/unreadable log is not an error — it sets a note.
    fn refresh_detail(&mut self) {
        self.detail_log.clear();
        self.detail_log_note = None;

        let (Some(root), Some(idx)) = (&self.workspace, self.table_state.selected()) else {
            return;
        };
        let Some(entry) = self.agents.get(idx) else {
            return;
        };
        let log_path = root.join(&entry.path).join(".bwoc/agent.log");
        if !log_path.exists() {
            self.detail_log_note = Some("(no log yet — start the agent to capture output)".into());
            return;
        }
        match crate::log::tail_lines(&log_path, DETAIL_LOG_TAIL_LINES) {
            Ok(lines) if lines.is_empty() => {
                self.detail_log_note = Some("(log is empty)".into());
            }
            Ok(lines) => self.detail_log = lines,
            Err(e) => self.detail_log_note = Some(format!("(log unreadable: {e})")),
        }
    }

    /// The live session for `agent_id`, if any. Only marker-sourced
    /// sessions carry an `agent_id`; scan-only entries (agentId = null)
    /// can't be attributed to a registry row and are skipped here.
    fn session_for(&self, agent_id: &str) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| s.agent_id.as_deref() == Some(agent_id))
    }

    fn next(&mut self) {
        if self.agents.is_empty() {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        self.table_state.select(Some((i + 1) % self.agents.len()));
        self.refresh_detail();
    }

    fn prev(&mut self) {
        if self.agents.is_empty() {
            return;
        }
        let i = self.table_state.selected().unwrap_or(0);
        let new = if i == 0 { self.agents.len() - 1 } else { i - 1 };
        self.table_state.select(Some(new));
        self.refresh_detail();
    }
}

// --- terminal setup / event loop -----------------------------------------

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn event_loop(term: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        term.draw(|f| draw_frame(f, app))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
        {
            // When the help overlay is visible, Esc / q / Ctrl-C still
            // exit; ANY other key dismisses the overlay (without acting).
            // This keeps the help "modal-lite": one keystroke up, one down.
            if app.show_help {
                match (code, modifiers) {
                    (KeyCode::Char('q'), KeyModifiers::NONE) => return Ok(()),
                    (KeyCode::Esc, _) => return Ok(()),
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(()),
                    _ => app.show_help = false,
                }
            } else {
                match (code, modifiers) {
                    (KeyCode::Char('q'), KeyModifiers::NONE) => return Ok(()),
                    (KeyCode::Esc, _) => return Ok(()),
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(()),
                    (KeyCode::Char('r'), _) => app.refresh(),
                    (KeyCode::Char('t'), _) => open_in_tmux(app),
                    (KeyCode::Char('l'), _) => open_log_in_tmux(app),
                    (KeyCode::Char('i'), _) => open_inbox_in_tmux(app),
                    (KeyCode::Char('g'), _) => open_in_ghostty(app),
                    (KeyCode::Char('s'), _) => start_selected_agent(app),
                    (KeyCode::Char('x'), _) => stop_selected_agent(app),
                    (KeyCode::Char('?'), _) => app.show_help = true,
                    (KeyCode::Down | KeyCode::Char('j'), _) => app.next(),
                    (KeyCode::Up | KeyCode::Char('k'), _) => app.prev(),
                    _ => {}
                }
            }
        }

        // Auto-tick: re-read agents.toml every AUTO_REFRESH_INTERVAL.
        // Live data (runtime/uptime/inbox) re-reads on every draw —
        // this only refreshes the agents Vec itself, so additions/
        // removals appear without the user pressing `r`.
        if app.last_refresh_at.elapsed() >= AUTO_REFRESH_INTERVAL {
            app.refresh();
        }
    }
}

/// `t` hotkey — spawn `bwoc spawn` for the selected agent in a new tmux
/// window. Auto-selects the LLM/model via the agent's manifest (`bwoc
/// spawn` already does this — backend lookup from registry, model from
/// manifest.primaryModel).
///
/// Requires `$TMUX` (the dashboard must itself be running inside a
/// tmux session). Failure modes:
///   - no `$TMUX`: updates `app.last_action` with a hint; no-op
///   - no agent selected: no-op (silent)
///   - tmux exec error: updates `app.last_action` with the error
fn open_in_tmux(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        app.last_action = Some("(no agent selected — ↑↓ to pick first)".to_string());
        return;
    };
    let Some(entry) = app.agents.get(idx).cloned() else {
        return;
    };
    let Some(root) = &app.workspace else {
        app.last_action = Some("(no workspace — nothing to talk to)".to_string());
        return;
    };

    if std::env::var_os("TMUX").is_none() {
        app.last_action = Some(
            "(not inside tmux — run `tmux new-session` first, then re-launch dashboard)"
                .to_string(),
        );
        return;
    }

    let agent_path = root.join(&entry.path);
    let agent_path_str = agent_path.to_string_lossy().to_string();
    // `tmux new-window -n <name> -- bwoc spawn --path <p> --backend <b>`
    // The trailing args after `--` are run as the new window's command.
    let result = std::process::Command::new("tmux")
        .args([
            "new-window",
            "-n",
            entry.id.as_str(),
            "--",
            "bwoc",
            "spawn",
            "--path",
            agent_path_str.as_str(),
            "--backend",
            entry.backend.as_str(),
        ])
        .status();
    match result {
        Ok(s) if s.success() => {
            app.last_action = Some(format!(
                "→ tmux window '{}' opened (backend: {})",
                entry.id, entry.backend
            ));
        }
        Ok(s) => {
            app.last_action = Some(format!("tmux new-window exited {s}"));
        }
        Err(e) => {
            app.last_action = Some(format!("tmux exec failed: {e}"));
        }
    }
}

/// `g` hotkey — spawn `bwoc spawn` for the selected agent in a new
/// Ghostty terminal window. Mirrors `open_in_tmux` (chat mode) but
/// targets a fresh Ghostty window instead of a tmux pane. macOS-only
/// because Ghostty's CLI launcher on macOS is `open -na Ghostty.app`.
fn open_in_ghostty(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        app.last_action = Some("(no agent selected — ↑↓ to pick first)".to_string());
        return;
    };
    let Some(entry) = app.agents.get(idx).cloned() else {
        return;
    };
    let Some(root) = &app.workspace else {
        app.last_action = Some("(no workspace — nothing to talk to)".to_string());
        return;
    };

    if !cfg!(target_os = "macos") {
        app.last_action =
            Some("(ghostty: macOS-only — run `ghostty -e bwoc spawn …` manually)".to_string());
        return;
    }

    let agent_path = root.join(&entry.path);
    let agent_path_str = agent_path.to_string_lossy().to_string();
    let wd_arg = format!("--working-directory={agent_path_str}");
    // `open -na Ghostty.app --args --working-directory=<p> -e bwoc spawn --path <p> --backend <b>`
    let result = std::process::Command::new("open")
        .args([
            "-na",
            "Ghostty.app",
            "--args",
            wd_arg.as_str(),
            "-e",
            "bwoc",
            "spawn",
            "--path",
            agent_path_str.as_str(),
            "--backend",
            entry.backend.as_str(),
        ])
        .status();
    match result {
        Ok(s) if s.success() => {
            app.last_action = Some(format!(
                "→ Ghostty window for '{}' opened (backend: {})",
                entry.id, entry.backend
            ));
        }
        Ok(s) => {
            app.last_action = Some(format!(
                "ghostty: `open -na Ghostty.app` exited {s} (installed in /Applications?)"
            ));
        }
        Err(e) => {
            app.last_action = Some(format!("ghostty: `open` exec failed: {e}"));
        }
    }
}

/// `s` hotkey — start (run) the selected agent: flip its registry status
/// to active and spawn `bwoc-agent --serve` if not already alive. Shells
/// out to `bwoc start <id> --yes --json` with output **captured** (not
/// inherited) so the daemon's stdout/stderr never corrupts the TUI. The
/// daemon is spawned detached by `bwoc start`, so it survives this short-
/// lived child. Refreshes the registry on success so the row's status +
/// the ●/○ runtime indicator flip immediately.
fn start_selected_agent(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        app.last_action = Some("(no agent selected — ↑↓ to pick first)".to_string());
        return;
    };
    let Some(entry) = app.agents.get(idx).cloned() else {
        return;
    };
    let Some(root) = &app.workspace else {
        app.last_action = Some("(no workspace — nothing to start)".to_string());
        return;
    };
    let root_str = root.to_string_lossy().to_string();

    // `--json` requires `--yes` (no TTY prompt path in a captured child).
    let result = std::process::Command::new("bwoc")
        .args([
            "start",
            entry.id.as_str(),
            "--yes",
            "--json",
            "--workspace",
            root_str.as_str(),
        ])
        .output();
    match result {
        Ok(out) if out.status.success() => {
            // Parse the JSON shape `{ daemon_spawned, daemon_pid, already_running, ... }`
            // for a precise message; fall back to a generic one if parsing fails.
            let stdout = String::from_utf8_lossy(&out.stdout);
            let parsed = serde_json::from_str::<serde_json::Value>(stdout.trim()).ok();
            let already = parsed
                .as_ref()
                .and_then(|v| v.get("already_running").and_then(|b| b.as_bool()))
                .unwrap_or(false);
            let pid = parsed
                .as_ref()
                .and_then(|v| v.get("daemon_pid").and_then(|p| p.as_u64()));
            app.last_action = Some(match (already, pid) {
                (true, _) => format!("→ '{}' already running", entry.id),
                (false, Some(p)) => format!("→ started '{}' (daemon pid {p})", entry.id),
                (false, None) => format!("→ started '{}' (status active)", entry.id),
            });
            app.refresh();
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let first = err.trim().lines().next().unwrap_or("unknown error");
            app.last_action = Some(format!("start '{}' failed: {first}", entry.id));
        }
        Err(e) => {
            app.last_action = Some(format!("start exec failed: {e} (is bwoc on PATH?)"));
        }
    }
}

/// `x` hotkey — stop the selected agent: send STOP to its daemon (if
/// alive) and flip its registry status to stopped. Mirror of
/// `start_selected_agent` — shells out to `bwoc stop <id> --yes --json`
/// with output captured so the TUI is not corrupted, then refreshes so
/// the row status + ●/○ runtime indicator flip immediately.
fn stop_selected_agent(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        app.last_action = Some("(no agent selected — ↑↓ to pick first)".to_string());
        return;
    };
    let Some(entry) = app.agents.get(idx).cloned() else {
        return;
    };
    let Some(root) = &app.workspace else {
        app.last_action = Some("(no workspace — nothing to stop)".to_string());
        return;
    };
    let root_str = root.to_string_lossy().to_string();

    let result = std::process::Command::new("bwoc")
        .args([
            "stop",
            entry.id.as_str(),
            "--yes",
            "--json",
            "--workspace",
            root_str.as_str(),
        ])
        .output();
    match result {
        Ok(out) if out.status.success() => {
            // `bwoc stop --json` shape: `{ workspace, agent, daemon_outcome, registry_updated }`.
            // daemon_outcome ∈ not_running | socket_ok | sigterm | sigkill | could_not_kill.
            let stdout = String::from_utf8_lossy(&out.stdout);
            let outcome = serde_json::from_str::<serde_json::Value>(stdout.trim())
                .ok()
                .and_then(|v| {
                    v.get("daemon_outcome")
                        .and_then(|o| o.as_str())
                        .map(str::to_string)
                });
            app.last_action = Some(match outcome.as_deref() {
                Some("not_running") => {
                    format!("→ '{}' marked stopped (no daemon running)", entry.id)
                }
                Some("socket_ok") => format!("→ stopped '{}' (graceful STOP)", entry.id),
                Some("sigterm") => format!("→ stopped '{}' (SIGTERM)", entry.id),
                Some("sigkill") => format!(
                    "→ stopped '{}' (SIGKILL — daemon was unresponsive)",
                    entry.id
                ),
                Some("could_not_kill") => {
                    format!("⚠ '{}' marked stopped but daemon would not die", entry.id)
                }
                _ => format!("→ stopped '{}'", entry.id),
            });
            app.refresh();
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            let first = err.trim().lines().next().unwrap_or("unknown error");
            app.last_action = Some(format!("stop '{}' failed: {first}", entry.id));
        }
        Err(e) => {
            app.last_action = Some(format!("stop exec failed: {e} (is bwoc on PATH?)"));
        }
    }
}

/// `l` hotkey — open `bwoc log -f <agent>` in a new tmux window for the
/// selected agent. Live tail of daemon stderr. Mirrors `open_in_tmux`
/// (chat) — same workspace/tmux/selection guards, same feedback shape.
fn open_log_in_tmux(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        app.last_action = Some("(no agent selected — ↑↓ to pick first)".to_string());
        return;
    };
    let Some(entry) = app.agents.get(idx).cloned() else {
        return;
    };
    let Some(root) = &app.workspace else {
        app.last_action = Some("(no workspace — nothing to tail)".to_string());
        return;
    };

    if std::env::var_os("TMUX").is_none() {
        app.last_action = Some(
            "(not inside tmux — run `tmux new-session` first, then re-launch dashboard)"
                .to_string(),
        );
        return;
    }

    let root_str = root.to_string_lossy().to_string();
    let bare = entry.id.strip_prefix("agent-").unwrap_or(&entry.id);
    let window_name = format!("{}-log", entry.id);
    let result = std::process::Command::new("tmux")
        .args([
            "new-window",
            "-n",
            window_name.as_str(),
            "--",
            "bwoc",
            "log",
            bare,
            "--workspace",
            root_str.as_str(),
            "--follow",
        ])
        .status();
    match result {
        Ok(s) if s.success() => {
            app.last_action = Some(format!("→ tmux window '{window_name}' tailing log -f"));
        }
        Ok(s) => {
            app.last_action = Some(format!("tmux new-window exited {s}"));
        }
        Err(e) => {
            app.last_action = Some(format!("tmux exec failed: {e}"));
        }
    }
}

/// `i` hotkey — open `bwoc inbox <agent> --watch` in a new tmux window
/// for the selected agent. Live tail of pending + new envelopes.
/// Third of the t/l/i triad (chat / log / inbox), all sharing the same
/// guards + feedback shape.
fn open_inbox_in_tmux(app: &mut App) {
    let Some(idx) = app.table_state.selected() else {
        app.last_action = Some("(no agent selected — ↑↓ to pick first)".to_string());
        return;
    };
    let Some(entry) = app.agents.get(idx).cloned() else {
        return;
    };
    let Some(root) = &app.workspace else {
        app.last_action = Some("(no workspace — no inbox to watch)".to_string());
        return;
    };

    if std::env::var_os("TMUX").is_none() {
        app.last_action = Some(
            "(not inside tmux — run `tmux new-session` first, then re-launch dashboard)"
                .to_string(),
        );
        return;
    }

    let root_str = root.to_string_lossy().to_string();
    let bare = entry.id.strip_prefix("agent-").unwrap_or(&entry.id);
    let window_name = format!("{}-inbox", entry.id);
    let result = std::process::Command::new("tmux")
        .args([
            "new-window",
            "-n",
            window_name.as_str(),
            "--",
            "bwoc",
            "inbox",
            bare,
            "--workspace",
            root_str.as_str(),
            "--watch",
        ])
        .status();
    match result {
        Ok(s) if s.success() => {
            app.last_action = Some(format!("→ tmux window '{window_name}' watching inbox"));
        }
        Ok(s) => {
            app.last_action = Some(format!("tmux new-window exited {s}"));
        }
        Err(e) => {
            app.last_action = Some(format!("tmux exec failed: {e}"));
        }
    }
}

// --- workspace resolution -------------------------------------------------

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

// --- drawing --------------------------------------------------------------

fn draw_frame(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // banner
            Constraint::Min(0),    // body (agents + detail)
            Constraint::Length(1), // footer
        ])
        .split(area);

    draw_banner(f, layout[0], app);
    draw_body(f, layout[1], app);
    draw_footer(f, layout[2], app);

    // Help overlay drawn last so it sits above everything. Centered,
    // ~50% width × ~70% height. Any keypress dismisses it (handled in
    // the input loop), so the overlay just renders here.
    if app.show_help {
        draw_help_overlay(f, area);
    }
}

/// Centered modal listing every keybinding. Cleared on any keypress.
fn draw_help_overlay(f: &mut ratatui::Frame, area: Rect) {
    use ratatui::style::{Modifier, Style};
    use ratatui::widgets::{Clear, Paragraph};

    // Center: 60% wide, 60% tall.
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(area);
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60),
            Constraint::Percentage(20),
        ])
        .split(v[1]);
    let popup = h[1];

    let body = "\
  ?           toggle this help overlay
  q / Esc     quit the dashboard
  Ctrl-C      quit the dashboard
  r           refresh now (auto-refresh: 2s)

  ↑ / k       previous agent in the list
  ↓ / j       next agent in the list

  t           open `bwoc spawn` in a new tmux window
              (chat with the selected agent)
  l           open `bwoc log -f` in a new tmux window
              (live tail the daemon log)
  i           open `bwoc inbox --watch` in a new tmux window
              (live tail the inbox)
  g           open `bwoc spawn` in a new Ghostty window
              (chat with the selected agent; macOS-only)
  s           start (run) the selected agent
              (flip status active + spawn bwoc-agent --serve)
  x           stop the selected agent
              (signal the daemon + flip status stopped)

Press any key to dismiss.";

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Hotkeys ")
        .border_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(body).block(block), popup);
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    draw_agents(f, h[0], app);
    draw_detail(f, h[1], app);
}

fn draw_detail(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let title = format!(" {} ", i18n::t(&app.bundle, "dash-pane-detail"));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().add_modifier(Modifier::DIM));

    let Some(idx) = app.table_state.selected() else {
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                i18n::t(&app.bundle, "dash-empty-select"),
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .alignment(Alignment::Center)
        .block(block);
        f.render_widget(p, area);
        return;
    };
    let Some(entry) = app.agents.get(idx) else {
        f.render_widget(Paragraph::new("").block(block), area);
        return;
    };
    let Some(root) = &app.workspace else {
        f.render_widget(Paragraph::new("").block(block), area);
        return;
    };

    let agent_path = root.join(&entry.path);
    let mut lines: Vec<Line> = Vec::new();

    let key_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    lines.push(Line::from(vec![
        Span::styled("id          ", key_style),
        Span::raw(entry.id.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("path        ", key_style),
        Span::raw(entry.path.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("backend     ", key_style),
        Span::raw(entry.backend.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("incarnated  ", key_style),
        Span::raw(entry.incarnated.clone()),
    ]));

    // Runtime (live process state) + inbox count — surfaces Phase 2/3
    // data inside the dashboard. Best-effort: missing files / no daemon
    // render as "○ not running" / "0 messages".
    let (runtime_mark, runtime_color, runtime_text) =
        match crate::livecheck::running_pid(root, entry) {
            Some(pid) => match crate::livecheck::query_uptime(root, entry) {
                Some(secs) => (
                    "●",
                    Color::Green,
                    format!(
                        "running (pid {pid}, uptime {})",
                        crate::livecheck::format_uptime(secs)
                    ),
                ),
                None => ("●", Color::Green, format!("running (pid {pid})")),
            },
            None => ("○", Color::DarkGray, "not running".to_string()),
        };
    lines.push(Line::from(vec![
        Span::styled("runtime     ", key_style),
        Span::styled(
            runtime_mark,
            Style::default()
                .fg(runtime_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(runtime_text, Style::default().fg(runtime_color)),
    ]));

    // Backend session — from the `sessions` resolution (marker + scan +
    // idle-secs). Distinct from `runtime` above: that tracks the daemon
    // (`bwoc-agent --serve`); this tracks the interactive backend process
    // (claude/agy/codex/…). No live session → dim em-dash.
    match app.session_for(&entry.id) {
        Some(s) => {
            let (glyph, color, label) = activity_display(Some(&s.state));
            lines.push(Line::from(vec![
                Span::styled("session     ", key_style),
                Span::styled(
                    glyph,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(label, Style::default().fg(color)),
                Span::styled(
                    format!("  ({}, pid {})", s.backend, s.pid),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("last seen   ", key_style),
                Span::raw(format_activity_age(s.last_activity)),
            ]));
        }
        None => {
            lines.push(Line::from(vec![
                Span::styled("session     ", key_style),
                Span::styled(
                    "— no live backend session",
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }

    let count = crate::livecheck::inbox_count(root, entry);
    let inbox_color = if count == 0 {
        Color::DarkGray
    } else {
        Color::Yellow
    };
    lines.push(Line::from(vec![
        Span::styled("inbox       ", key_style),
        Span::styled(
            format!("{count} message(s)"),
            Style::default().fg(inbox_color),
        ),
    ]));
    let (refused_count, refused_detail) = crate::livecheck::refusal_summary(root, entry);
    if refused_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("refused     ", key_style),
            Span::styled(
                format!("{refused_count} refusal(s)"),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        if let Some((reason, from)) = refused_detail {
            lines.push(Line::from(vec![
                Span::styled("last refused", key_style),
                Span::raw(format!(" {reason} from {from}")),
            ]));
        }
    }

    // Saṅgha teams the selected agent belongs to + its task standing.
    // Read-only view of the shared task lists (`.bwoc/teams/`); shown
    // only when the agent is on at least one team.
    let teams = crate::livecheck::agent_team_summaries(root, &entry.id);
    for ts in &teams {
        let color = if ts.available > 0 {
            Color::Green
        } else {
            Color::DarkGray
        };
        lines.push(Line::from(vec![
            Span::styled("team        ", key_style),
            Span::raw(format!("{}: ", ts.team)),
            Span::styled(
                format!("{} mine", ts.claimed_by_me),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(", "),
            Span::styled(
                format!("{} avail", ts.available),
                Style::default().fg(color),
            ),
            Span::raw(format!(" / {} total", ts.total)),
        ]));
    }

    lines.push(Line::from(""));

    // Manifest fields (load on demand; failures shown gracefully).
    match Manifest::load_from_path(&agent_path.join("config.manifest.json")) {
        Ok(m) => {
            lines.push(Line::from(Span::styled(
                "manifest:",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled("  role        ", key_style),
                Span::raw(m.agent_role),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  model       ", key_style),
                Span::raw(m.primary_model),
            ]));
            if let Some(fb) = m.fallback_model {
                lines.push(Line::from(vec![
                    Span::styled("  fallback    ", key_style),
                    Span::raw(fb),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("  memory      ", key_style),
                Span::raw(m.memory_path),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  version     ", key_style),
                Span::raw(m.version),
            ]));
            // Persona scope (from the new manifest fields populated by
            // `bwoc new --scope` / `--out-of-scope`). Shown only when
            // set — otherwise the {{placeholder}} fallback is just
            // noise here.
            if let Some(scope) = &m.scope_description {
                lines.push(Line::from(vec![
                    Span::styled("  scope       ", key_style),
                    Span::raw(scope.clone()),
                ]));
            }
            if let Some(out) = &m.out_of_scope {
                lines.push(Line::from(vec![
                    Span::styled("  out-of-scope", key_style),
                    Span::raw(format!(" {out}")),
                ]));
            }
        }
        Err(e) => {
            lines.push(Line::from(Span::styled(
                format!("manifest: failed ({e})"),
                Style::default().fg(Color::Red),
            )));
        }
    }

    // Resource counts — mindsets / skills / memories. Each is the
    // count of `.md` files in the directory excluding template
    // scaffolding (SPEC.md / README.md). 0 renders as "—".
    let counts_block = [
        (
            "mindsets",
            crate::livecheck::count_user_md_files(&agent_path.join("mindsets")),
        ),
        (
            "skills",
            crate::livecheck::count_user_md_files(&agent_path.join("skills")),
        ),
        (
            "memories",
            crate::livecheck::count_user_md_files(&agent_path.join("memories")),
        ),
    ];
    if counts_block.iter().any(|(_, n)| *n > 0) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "resources:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for (label, n) in &counts_block {
            let value = if *n == 0 {
                "—".to_string()
            } else {
                n.to_string()
            };
            let value_color = if *n == 0 {
                Color::DarkGray
            } else {
                Color::Reset
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {label:<12}"), key_style),
                Span::styled(value, Style::default().fg(value_color)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // Health probe — same shape as `bwoc doctor` / `bwoc status` per-agent
    // checks, returning one summarised verdict.
    let (mark, color, msg) = match probe(root, entry) {
        Health::Ok => ("✓", Color::Green, "all probes passed".to_string()),
        Health::Warn(m) => ("⚠", Color::Yellow, m),
        Health::Fail(m) => ("✗", Color::Red, m),
    };
    lines.push(Line::from(vec![
        Span::styled("health      ", key_style),
        Span::styled(
            mark,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(msg, Style::default().fg(color)),
    ]));

    // Log tail — the last N lines of the selected agent's daemon log,
    // refreshed on the tick + on selection change (see `refresh_detail`).
    // Reuses `log::tail_lines` (the same read the `--follow` path uses) so
    // the dashboard and `bwoc log` show identical content. Missing/empty
    // logs degrade to a dim note rather than a blank gap.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("log (last {DETAIL_LOG_TAIL_LINES}):"),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    if let Some(note) = &app.detail_log_note {
        lines.push(Line::from(Span::styled(
            format!("  {note}"),
            Style::default().add_modifier(Modifier::DIM),
        )));
    } else {
        for log_line in &app.detail_log {
            lines.push(Line::from(Span::styled(
                format!("  {log_line}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

// Liveness + inbox helpers (signal_zero_alive, running_pid,
// query_uptime, format_uptime, inbox_count) moved to crate::livecheck.

/// Map a session state (or its absence) to (glyph, colour, label) for the
/// activity column. Pure + single-sourced so the column and the detail
/// pane render the same vocabulary. `None` = no live backend session for
/// this agent → a dim em-dash. Note this is the *backend session* (claude
/// /agy/codex/…), distinct from the daemon "runtime ●" line, which tracks
/// `bwoc-agent --serve`.
fn activity_display(state: Option<&SessionState>) -> (&'static str, Color, &'static str) {
    match state {
        Some(SessionState::Working) => ("●", Color::Green, "working"),
        Some(SessionState::Idle) => ("◑", Color::Yellow, "idle"),
        Some(SessionState::Running) => ("●", Color::Cyan, "running"),
        Some(SessionState::Stale) => ("○", Color::DarkGray, "stale"),
        None => ("—", Color::DarkGray, ""),
    }
}

/// Render `last_activity` (epoch-seconds) as a human "N ago" age relative
/// to now, reusing `livecheck::format_uptime` for the unit formatting.
/// `None` → "unknown" (scan-sourced sessions have no activity signal).
fn format_activity_age(last_activity: Option<u64>) -> String {
    let Some(ts) = last_activity else {
        return "unknown".to_string();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{} ago",
        crate::livecheck::format_uptime(now.saturating_sub(ts))
    )
}

const BACKEND_SYMLINKS: &[&str] = &["CLAUDE.md", "AGY.md", "CODEX.md", "KIMI.md"];

#[derive(Debug)]
enum Health {
    Ok,
    Warn(String),
    Fail(String),
}

fn probe(root: &std::path::Path, a: &AgentEntry) -> Health {
    let p = root.join(&a.path);
    if !p.is_dir() {
        return Health::Fail(format!("directory missing: {}", p.display()));
    }
    if !p.join("AGENTS.md").is_file() {
        return Health::Fail("missing AGENTS.md".to_string());
    }
    let missing: Vec<&str> = BACKEND_SYMLINKS
        .iter()
        .copied()
        .filter(|link| !p.join(link).exists())
        .collect();
    if !missing.is_empty() {
        return Health::Warn(format!(
            "missing symlinks: {} (run `bwoc doctor --auto`)",
            missing.join(", ")
        ));
    }
    if !p.join("config.manifest.json").is_file() {
        return Health::Warn("config.manifest.json missing".to_string());
    }
    Health::Ok
}

fn draw_banner(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let title_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let workspace_line = match &app.workspace {
        Some(p) => {
            // Workspace-level counts surfacing the scaffolded dirs
            // (`projects/` subdirs, `notes/` user .md files, `.bwoc/memory/`
            // entries) + the attention sum (agents with pending inbox).
            // 0 renders as "—" for visual quiet; attention is OMITTED
            // entirely when no agent has pending — surfaces only when
            // it matters.
            let projects = crate::livecheck::count_subdirs(&p.join("projects"));
            let notes = crate::livecheck::count_user_md_files(&p.join("notes"));
            let memory = crate::livecheck::count_user_md_files(&p.join(".bwoc/memory"));
            let total_pending: u64 = app
                .agents
                .iter()
                .map(|a| crate::livecheck::inbox_count(p, a) as u64)
                .sum();
            let p_str = if projects == 0 {
                "—".to_string()
            } else {
                projects.to_string()
            };
            let n_str = if notes == 0 {
                "—".to_string()
            } else {
                notes.to_string()
            };
            let m_str = if memory == 0 {
                "—".to_string()
            } else {
                memory.to_string()
            };
            let attn_suffix = if total_pending > 0 {
                format!("  ·  attention: {total_pending} pending")
            } else {
                String::new()
            };
            format!(
                "Workspace: {}  ·  projects: {}  ·  notes: {}  ·  memory: {}{}",
                p.display(),
                p_str,
                n_str,
                m_str,
                attn_suffix
            )
        }
        None => "Workspace: (none — pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`)"
            .to_string(),
    };
    let lines = vec![
        Line::from(Span::styled("BWOC Framework", title_style)),
        Line::from(Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().add_modifier(Modifier::DIM),
        )),
        Line::from(workspace_line),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" dashboard ")
        .border_style(Style::default().fg(Color::Cyan));
    let p = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(block);
    f.render_widget(p, area);
}

fn draw_agents(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" agents ")
        .border_style(Style::default().add_modifier(Modifier::DIM));

    if let Some(err) = &app.last_refresh_error {
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("failed to read agents: {err}"),
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "press `r` to retry",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .alignment(Alignment::Center)
        .block(block);
        f.render_widget(p, area);
        return;
    }

    if app.agents.is_empty() {
        let msg = if app.workspace.is_some() {
            "(no agents registered — `bwoc new <name>` to incarnate the first)"
        } else {
            "(no workspace found — exit and run `bwoc init` first)"
        };
        let p = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                msg,
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .alignment(Alignment::Center)
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from("ID"),
        Cell::from("ACTIVITY"),
        Cell::from("STATUS"),
        Cell::from("BACKEND"),
        Cell::from("PATH"),
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .agents
        .iter()
        .map(|a| {
            // Live activity from the session snapshot (marker/scan resolved
            // on the tick). No session for this id → "—".
            let (glyph, color, label) = activity_display(app.session_for(&a.id).map(|s| &s.state));
            let activity = Line::from(vec![
                Span::styled(glyph, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(label, Style::default().fg(color)),
            ]);
            Row::new(vec![
                Cell::from(a.id.clone()),
                Cell::from(activity),
                Cell::from(a.status.clone()),
                Cell::from(a.backend.clone()),
                Cell::from(a.path.clone()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Percentage(26),
        Constraint::Percentage(16),
        Constraint::Percentage(13),
        Constraint::Percentage(13),
        Constraint::Percentage(32),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    // When we have transient feedback from the last action (tmux open
    // result, "not in tmux" hint, etc.), show that instead of the
    // hotkey legend — the user just performed an action and wants to
    // know what happened. The hotkey legend re-appears on next refresh.
    if let Some(msg) = &app.last_action {
        let p = Paragraph::new(Line::from(Span::styled(
            msg.clone(),
            Style::default().fg(Color::Cyan),
        )))
        .alignment(Alignment::Center);
        f.render_widget(p, area);
        return;
    }

    let bold = Style::default().add_modifier(Modifier::BOLD);
    let count = if app.agents.is_empty() {
        "0 agents".to_string()
    } else {
        let cur = app.table_state.selected().unwrap_or(0) + 1;
        format!("{}/{}", cur, app.agents.len())
    };
    let nav = i18n::t(&app.bundle, "dash-footer-navigate");
    let refresh = i18n::t(&app.bundle, "dash-footer-refresh");
    let quit = i18n::t(&app.bundle, "dash-footer-quit");
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(count, Style::default().fg(Color::Cyan)),
        Span::raw("    "),
        Span::styled("↑↓/jk", bold),
        Span::raw(format!(" {nav}    ")),
        Span::styled("t", bold),
        Span::raw(" talk    "),
        Span::styled("l", bold),
        Span::raw(" log    "),
        Span::styled("i", bold),
        Span::raw(" inbox    "),
        Span::styled("s", bold),
        Span::raw(" start    "),
        Span::styled("x", bold),
        Span::raw(" stop    "),
        Span::styled("r", bold),
        Span::raw(format!(" {refresh}    ")),
        Span::styled("?", bold),
        Span::raw(" help    "),
        Span::styled("q/Esc", bold),
        Span::raw(format!(" {quit}")),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(footer, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_display_covers_every_state_and_absence() {
        assert_eq!(
            activity_display(Some(&SessionState::Working)),
            ("●", Color::Green, "working")
        );
        assert_eq!(
            activity_display(Some(&SessionState::Idle)),
            ("◑", Color::Yellow, "idle")
        );
        assert_eq!(
            activity_display(Some(&SessionState::Running)),
            ("●", Color::Cyan, "running")
        );
        assert_eq!(
            activity_display(Some(&SessionState::Stale)),
            ("○", Color::DarkGray, "stale")
        );
        // No live session → dim em-dash, empty label.
        assert_eq!(activity_display(None), ("—", Color::DarkGray, ""));
    }

    #[test]
    fn format_activity_age_none_is_unknown() {
        assert_eq!(format_activity_age(None), "unknown");
    }

    #[test]
    fn format_activity_age_recent_reads_as_ago() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // ~5s ago → "5s ago" (or a tick more if the clock advances).
        let s = format_activity_age(Some(now.saturating_sub(5)));
        assert!(s.ends_with(" ago"), "expected a relative age, got {s:?}");
    }
}
