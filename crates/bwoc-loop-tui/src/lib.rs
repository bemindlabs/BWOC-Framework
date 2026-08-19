//! `bwoc-loop-tui` — the full-screen ratatui **loop control center** behind
//! `bwoc loop`.
//!
//! Its own crate (not a `bwoc-cli` module) so the loop-engineering TUI can grow
//! without bloating the CLI and so its ratatui/crossterm surface stays isolated.
//! Like [`bwoc-tui`] it compile-depends ONLY on `bwoc-core` (the Saṅgha task
//! model + sibling-binary resolution) — never on `bwoc-cli` or `bwoc-harness`.
//! The harness that actually *drives* a goal-loop is a runtime subprocess
//! (see [`run`]), not a build dependency — the dep-quarantine that keeps the
//! `bwoc` side free of the harness runtime graph.
//!
//! # What it is
//!
//! The operator console for **Loop-Engineering L1** (the Saṅgha goal-loop): pick
//! a team, watch its shared task list drive toward Definition-of-Done, and
//! **start / stop** the loop, streaming the harness's output into a live log
//! pane. Starting a loop spawns `bwoc-harness --lead --loop` over the selected
//! team's `tasks.jsonl`; editing the ticker / budget / tasks is a later slice.
//!
//! # Layout (one screen, no mouse)
//!
//! ```text
//! ┌ BWOC Loop Control · Goal-Loop L1 ─────────────── team: squad · In Progress ┐
//! ├──────────────────────────────────┬─────────────────────────────────────────┤
//! │ Tasks                            │ Goal                                     │
//! │ ▸ t1  ✓ design schema            │ ✓ 1  ⧗ 1  ○ 2  ⛔ 1   (5 total)          │
//! │   t2  ⧗ implement      ⟨t1⟩      │ Ticker  every 5s                         │
//! │   t3  ○ test           ⟨t2⟩ 🔒   │ Budget  20 iters                         │
//! │   …                              │ ── selected ──                           │
//! │                                  │ t3 · test · requires plan                │
//! └──────────────────────────────────┴─────────────────────────────────────────┘
//! │ ↑/↓ move · ⇥ team · r refresh · q quit                                       │
//! ```
//!
//! Keys: `↑/↓` (or `j/k`) move the task selection; `Tab` / `Shift-Tab` cycle
//! teams; `s` starts a goal-loop for the selected team, `x` stops it; `r`
//! refreshes now (state also auto-refreshes on a 2s tick); `q` / `Esc` /
//! `Ctrl-C` quit (killing any running loop) and restore the terminal.

mod run;

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use run::{LaunchSpec, LoopRun};

use bwoc_core::team::{self, Task, TaskState, Team};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

/// Arguments for the loop TUI. The workspace root is resolved by the caller
/// (`bwoc loop`) so this crate carries no resolution logic — it is handed a
/// concrete, existing workspace directory.
pub struct LoopTuiArgs {
    /// Resolved workspace root (holds `.bwoc/teams/`).
    pub workspace: PathBuf,
    /// Team to open first. `None` → the first team alphabetically.
    pub team: Option<String>,
    /// Ticker cadence (seconds) shown for the goal-loop and used as the default
    /// when a run is started in a later slice. Floored at 1s on construction, so
    /// a `0` can never become a zero-delay spin when a loop is launched.
    pub ticker_secs: u64,
    /// Iteration budget shown for the goal-loop (`0` = unbounded).
    pub budget_iters: usize,
    /// Backend forwarded to `bwoc-harness --backend` when a loop starts. `None`
    /// → the harness default (ollama).
    pub backend: Option<String>,
    /// Model forwarded to `bwoc-harness --model`. `None` → harness default.
    pub model: Option<String>,
    /// Endpoint forwarded to `bwoc-harness --endpoint`. `None` → harness default.
    pub endpoint: Option<String>,
}

/// Auto-refresh cadence — team task lists change rarely, so a slow tick keeps
/// the view live without hammering the filesystem.
const AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Entry point for `bwoc loop`. Returns a process exit code.
pub fn run(args: LoopTuiArgs) -> i32 {
    let mut app = App::new(args);
    app.reload();
    if app.teams.is_empty() {
        eprintln!(
            "bwoc loop: no Saṅgha teams in this workspace ({}). \
             Create one with `bwoc team create <id> --members …` first.",
            app.workspace.display()
        );
        return 2;
    }

    let mut term = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc loop: could not initialise the terminal: {e}");
            return 1;
        }
    };
    let loop_result = event_loop(&mut term, &mut app);
    // Always restore the terminal, even if the loop errored.
    let restore_result = restore_terminal();
    if let Err(e) = loop_result {
        eprintln!("bwoc loop: terminal error: {e}");
        return 1;
    }
    if let Err(e) = restore_result {
        eprintln!("bwoc loop: could not restore the terminal: {e}");
        return 1;
    }
    0
}

// ── App state ───────────────────────────────────────────────────────────────

/// The derived Definition-of-Done state of a team's task list, computed purely
/// from task states + dependencies (the same shape the harness goal-loop reports
/// at runtime, surfaced here statically before/while a loop runs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoalStatus {
    /// No tasks on the list.
    Empty,
    /// Every task is `Completed`.
    Done,
    /// At least one task is in progress or claimable (deps satisfied).
    InProgress,
    /// Pending tasks remain but every one is blocked by an incomplete
    /// dependency — no forward progress without new/finished work.
    Blocked,
}

impl GoalStatus {
    fn label(self) -> &'static str {
        match self {
            GoalStatus::Empty => "Empty",
            GoalStatus::Done => "Done",
            GoalStatus::InProgress => "In Progress",
            GoalStatus::Blocked => "Blocked",
        }
    }

    fn color(self) -> Color {
        match self {
            GoalStatus::Empty => Color::DarkGray,
            GoalStatus::Done => Color::Green,
            GoalStatus::InProgress => Color::Yellow,
            GoalStatus::Blocked => Color::Red,
        }
    }
}

/// Tally of task states for the goal summary line.
#[derive(Default, Clone, Copy)]
struct Counts {
    completed: usize,
    in_progress: usize,
    pending: usize,
    blocked: usize,
}

struct App {
    workspace: PathBuf,
    /// Team ids present in the workspace, sorted.
    teams: Vec<String>,
    sel_team: usize,
    /// Tasks of the selected team.
    tasks: Vec<Task>,
    sel_task: usize,
    ticker_secs: u64,
    budget_iters: usize,
    last_refresh: Instant,
    /// Non-fatal load error surfaced in the footer (e.g. a malformed task file).
    error: Option<String>,
    /// Team requested on the command line, honoured on the first load.
    initial_team: Option<String>,
    /// The running (or just-finished) goal-loop subprocess, if one was started.
    /// `reload()` never touches this field — only teams/tasks are re-read.
    run: Option<LoopRun>,
    /// Backend / model / endpoint forwarded to the harness on start.
    backend: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
}

impl App {
    fn new(args: LoopTuiArgs) -> Self {
        Self {
            workspace: args.workspace,
            teams: Vec::new(),
            sel_team: 0,
            tasks: Vec::new(),
            sel_task: 0,
            // Floor at 1s so the flag's "floored at 1s" contract holds no matter
            // the caller — a 0 must never reach a loop launch as a zero delay.
            ticker_secs: args.ticker_secs.max(1),
            budget_iters: args.budget_iters,
            last_refresh: Instant::now(),
            error: None,
            initial_team: args.team,
            run: None,
            backend: args.backend,
            model: args.model,
            endpoint: args.endpoint,
        }
    }

    fn teams_dir(&self) -> PathBuf {
        self.workspace.join(".bwoc").join("teams")
    }

    /// Reload the team list and the selected team's tasks from disk.
    fn reload(&mut self) {
        self.error = None;
        let prev_team = self.teams.get(self.sel_team).cloned();
        self.teams = self.scan_teams();

        // Resolve which team stays selected: the CLI-requested one on first
        // load, else keep the previously-selected id if it still exists, else
        // clamp into range.
        if let Some(want) = self.initial_team.take() {
            if let Some(i) = self.teams.iter().position(|t| *t == want) {
                self.sel_team = i;
            }
        } else if let Some(prev) = prev_team {
            if let Some(i) = self.teams.iter().position(|t| *t == prev) {
                self.sel_team = i;
            }
        }
        if self.sel_team >= self.teams.len() {
            self.sel_team = self.teams.len().saturating_sub(1);
        }

        self.reload_tasks();
        self.last_refresh = Instant::now();
    }

    /// Scan `.bwoc/teams/*.toml` for team ids. Silently skips unreadable /
    /// malformed team files (a broken team never wedges the whole view) and any
    /// team whose `id` is not a safe single path segment — the id is later joined
    /// as `.bwoc/teams/<id>/tasks.jsonl`, so a hostile `id` like `../secrets`
    /// must never escape the teams directory (path-traversal defense; this is a
    /// security framework).
    fn scan_teams(&self) -> Vec<String> {
        let dir = self.teams_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            if let Ok(body) = std::fs::read_to_string(&p)
                && let Ok(t) = Team::from_toml(&body)
                && is_safe_team_id(&t.id)
            {
                ids.push(t.id);
            }
        }
        ids.sort();
        ids
    }

    /// Load the selected team's `tasks.jsonl`. A parse error is surfaced in the
    /// footer rather than emptying the view silently.
    fn reload_tasks(&mut self) {
        self.tasks.clear();
        let Some(team_id) = self.teams.get(self.sel_team) else {
            return;
        };
        let path = self.teams_dir().join(team_id).join("tasks.jsonl");
        match std::fs::read_to_string(&path) {
            Ok(body) => match team::parse_tasks(&body) {
                Ok(tasks) => self.tasks = tasks,
                Err(e) => self.error = Some(format!("tasks.jsonl parse error: {e}")),
            },
            // Missing file = a team with no tasks yet; not an error.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => self.error = Some(format!("cannot read tasks.jsonl: {e}")),
        }
        if self.sel_task >= self.tasks.len() {
            self.sel_task = self.tasks.len().saturating_sub(1);
        }
    }

    fn current_team(&self) -> Option<&str> {
        self.teams.get(self.sel_team).map(|s| s.as_str())
    }

    fn selected_task(&self) -> Option<&Task> {
        self.tasks.get(self.sel_task)
    }

    /// Set of ids that are `Completed` — used for dep-satisfaction checks.
    fn completed_ids(&self) -> std::collections::HashSet<&str> {
        self.tasks
            .iter()
            .filter(|t| t.state == TaskState::Completed)
            .map(|t| t.id.as_str())
            .collect()
    }

    /// True when every dependency of `task` is completed.
    fn deps_satisfied(&self, task: &Task, completed: &std::collections::HashSet<&str>) -> bool {
        task.deps.iter().all(|d| completed.contains(d.as_str()))
    }

    fn counts(&self) -> Counts {
        let completed = self.completed_ids();
        let mut c = Counts::default();
        for t in &self.tasks {
            match t.state {
                TaskState::Completed => c.completed += 1,
                TaskState::InProgress => c.in_progress += 1,
                TaskState::Pending => {
                    if self.deps_satisfied(t, &completed) {
                        c.pending += 1;
                    } else {
                        c.blocked += 1;
                    }
                }
            }
        }
        c
    }

    fn goal_status(&self) -> GoalStatus {
        if self.tasks.is_empty() {
            return GoalStatus::Empty;
        }
        let c = self.counts();
        if c.completed == self.tasks.len() {
            return GoalStatus::Done;
        }
        if c.in_progress > 0 || c.pending > 0 {
            return GoalStatus::InProgress;
        }
        // Only completed + dependency-blocked pending remain.
        GoalStatus::Blocked
    }

    // ── navigation ──

    fn task_down(&mut self) {
        if !self.tasks.is_empty() {
            self.sel_task = (self.sel_task + 1).min(self.tasks.len() - 1);
        }
    }

    fn task_up(&mut self) {
        self.sel_task = self.sel_task.saturating_sub(1);
    }

    /// True while a goal-loop subprocess is live.
    fn loop_running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.is_running())
    }

    fn team_next(&mut self) {
        // A running loop is bound to the team it started on; don't switch the
        // view out from under it.
        if self.loop_running() {
            self.error = Some("stop the loop (x) before switching teams".into());
            return;
        }
        if self.teams.len() > 1 {
            self.sel_team = (self.sel_team + 1) % self.teams.len();
            self.sel_task = 0;
            self.reload_tasks();
        }
    }

    fn team_prev(&mut self) {
        if self.loop_running() {
            self.error = Some("stop the loop (x) before switching teams".into());
            return;
        }
        if self.teams.len() > 1 {
            self.sel_team = (self.sel_team + self.teams.len() - 1) % self.teams.len();
            self.sel_task = 0;
            self.reload_tasks();
        }
    }

    // ── goal-loop control (PR2) ──

    /// Start a goal-loop for the selected team: spawn `bwoc-harness --lead
    /// --loop` over its `tasks.jsonl`, capturing output into the log pane. A
    /// no-op if one is already running. Failures (harness not found, spawn
    /// error) surface in the footer rather than crashing the TUI.
    fn start_loop(&mut self) {
        if self.loop_running() {
            return;
        }
        let Some(team) = self.current_team().map(|s| s.to_string()) else {
            self.error = Some("no team selected".into());
            return;
        };
        // Defense in depth: the id came from scan_teams (already gated), but it
        // is about to cross an exec/path boundary — re-validate.
        if !is_safe_team_id(&team) {
            self.error = Some(format!("unsafe team id '{team}' — refusing to launch"));
            return;
        }
        let Some(harness) = bwoc_core::exec::sibling_binary("bwoc-harness") else {
            self.error = Some(
                "bwoc-harness not found (looked next to bwoc, in CARGO_BIN_EXE, and on PATH)"
                    .into(),
            );
            return;
        };
        let spec = LaunchSpec {
            harness,
            tasks_path: self.teams_dir().join(&team).join("tasks.jsonl"),
            workdir: self.workspace.clone(),
            interval_secs: self.ticker_secs,
            max_iters: self.budget_iters,
            backend: self.backend.clone(),
            model: self.model.clone(),
            endpoint: self.endpoint.clone(),
        };
        match LoopRun::spawn(spec, team) {
            Ok(r) => {
                self.run = Some(r);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Stop a running goal-loop (kill + reap). The finished run stays visible in
    /// the log pane until another is started.
    fn stop_loop(&mut self) {
        if let Some(r) = &mut self.run {
            r.stop();
        }
    }

    /// Drain output + poll the child for exit — called once per UI tick so the
    /// log pane and status update on the poll cadence, not only on keypress.
    fn tick_loop(&mut self) {
        if let Some(r) = &mut self.run {
            r.drain();
            r.poll();
        }
    }
}

// ── terminal setup / event loop ──────────────────────────────────────────────

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // If either step fails, raw mode is already on — undo it before returning so
    // the caller's terminal isn't left wedged in raw mode on an init error.
    if let Err(e) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(t) => Ok(t),
        Err(e) => {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            let _ = disable_raw_mode();
            Err(e)
        }
    }
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn event_loop(term: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        // Pull any new harness output + refresh the child's liveness before
        // drawing, so the log pane and status track the 200ms poll cadence even
        // when the operator isn't pressing keys.
        app.tick_loop();

        term.draw(|f| draw(f, app))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
        {
            match (code, modifiers) {
                (KeyCode::Char('q'), KeyModifiers::NONE) | (KeyCode::Esc, _) => return Ok(()),
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(()),
                (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => app.task_down(),
                (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => app.task_up(),
                (KeyCode::Tab, _) => app.team_next(),
                (KeyCode::BackTab, _) => app.team_prev(),
                (KeyCode::Char('r'), KeyModifiers::NONE) => app.reload(),
                (KeyCode::Char('s'), KeyModifiers::NONE) => app.start_loop(),
                (KeyCode::Char('x'), KeyModifiers::NONE) => app.stop_loop(),
                _ => {}
            }
        }

        if app.last_refresh.elapsed() >= AUTO_REFRESH_INTERVAL {
            app.reload();
        }
    }
}

// ── rendering ────────────────────────────────────────────────────────────────

fn draw(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(3),    // body
            Constraint::Length(1), // footer
        ])
        .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);
}

fn draw_header(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let status = app.goal_status();
    let team = app.current_team().unwrap_or("—");
    let line = Line::from(vec![
        Span::styled(
            " BWOC Loop Control ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Goal-Loop L1 ", Style::default().fg(Color::Cyan)),
        Span::raw("· team "),
        Span::styled(team, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" · "),
        Span::styled(
            status.label(),
            Style::default()
                .fg(status.color())
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_body(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);
    draw_tasks(f, app, cols[0]);
    // Right column: Goal detail on top, the live loop / log pane below.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(cols[1]);
    draw_detail(f, app, right[0]);
    draw_loop(f, app, right[1]);
}

/// The loop control / live-log pane: a status header line plus the tail of the
/// harness's captured output. Idle when no loop has been started this session.
fn draw_loop(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    match &app.run {
        None => {
            lines.push(Line::from(Span::styled(
                "idle · press s to start",
                Style::default().fg(Color::DarkGray),
            )));
        }
        Some(r) => {
            let (text, color) = r.status_label();
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            // Fill the remaining rows with the log tail.
            let room = log_rows(area.height);
            let width = log_line_width(area.width);
            if r.log.is_empty() {
                lines.push(Line::from(Span::styled(
                    "(waiting for output…)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                for l in r.log.tail(room) {
                    lines.push(Line::from(Span::styled(
                        truncate(l, width),
                        Style::default().fg(Color::Gray),
                    )));
                }
            }
        }
    }
    let para = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Loop "));
    f.render_widget(para, area);
}

/// Icon + colour for a task's effective state (pending splits into claimable vs
/// dependency-blocked).
fn task_glyph(
    app: &App,
    task: &Task,
    completed: &std::collections::HashSet<&str>,
) -> (&'static str, Color) {
    match task.state {
        TaskState::Completed => ("✓", Color::Green),
        TaskState::InProgress => ("⧗", Color::Yellow),
        TaskState::Pending => {
            if app.deps_satisfied(task, completed) {
                ("○", Color::Gray)
            } else {
                ("⛔", Color::Red)
            }
        }
    }
}

fn draw_tasks(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let completed = app.completed_ids();
    let items: Vec<ListItem> = app
        .tasks
        .iter()
        .map(|t| {
            let (glyph, color) = task_glyph(app, t, &completed);
            let mut spans = vec![
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::styled(format!("{:<6}", t.id), Style::default().fg(Color::DarkGray)),
                Span::raw(truncate(&t.title, 34)),
            ];
            if !t.deps.is_empty() {
                spans.push(Span::styled(
                    format!("  ⟨{}⟩", t.deps.join(",")),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            if t.requires_plan {
                let plan_color = match t.plan_approved {
                    Some(true) => Color::Green,
                    Some(false) => Color::Red,
                    None => Color::Magenta,
                };
                spans.push(Span::styled(" 🔒", Style::default().fg(plan_color)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Tasks ({}) ", app.tasks.len())),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !app.tasks.is_empty() {
        state.select(Some(app.sel_task));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let c = app.counts();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("✓ ", Style::default().fg(Color::Green)),
            Span::raw(format!("{}   ", c.completed)),
            Span::styled("⧗ ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}   ", c.in_progress)),
            Span::styled("○ ", Style::default().fg(Color::Gray)),
            Span::raw(format!("{}   ", c.pending)),
            Span::styled("⛔ ", Style::default().fg(Color::Red)),
            Span::raw(format!("{}", c.blocked)),
        ]),
        Line::from(Span::styled(
            format!("{} task(s) total", app.tasks.len()),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Ticker  ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("every {}s", app.ticker_secs)),
        ]),
        Line::from(vec![
            Span::styled("Budget  ", Style::default().fg(Color::Cyan)),
            Span::raw(if app.budget_iters == 0 {
                "unbounded (DoD/blocked still halt)".to_string()
            } else {
                format!("{} iteration(s)", app.budget_iters)
            }),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "── selected ──",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    if let Some(t) = app.selected_task() {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", t.id),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(t.title.clone()),
        ]));
        lines.push(Line::from(Span::styled(
            format!("state: {}", t.state.as_str()),
            Style::default().fg(Color::DarkGray),
        )));
        if let Some(who) = &t.claimed_by {
            lines.push(Line::from(format!("claimed by: {who}")));
        }
        if !t.deps.is_empty() {
            let completed = app.completed_ids();
            let unmet: Vec<&String> = t
                .deps
                .iter()
                .filter(|d| !completed.contains(d.as_str()))
                .collect();
            let deps_line = if unmet.is_empty() {
                format!("deps: {} (all met)", t.deps.join(", "))
            } else {
                format!(
                    "deps: {} (waiting on {})",
                    t.deps.join(", "),
                    unmet
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let color = if unmet.is_empty() {
                Color::Green
            } else {
                Color::Red
            };
            lines.push(Line::from(Span::styled(
                deps_line,
                Style::default().fg(color),
            )));
        }
        if t.requires_plan {
            let (txt, color) = match t.plan_approved {
                Some(true) => ("plan: approved", Color::Green),
                Some(false) => ("plan: rejected — resubmit", Color::Red),
                None if t.plan.is_some() => ("plan: submitted, awaiting review", Color::Yellow),
                None => ("plan: required, not submitted", Color::Magenta),
            };
            lines.push(Line::from(Span::styled(txt, Style::default().fg(color))));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "no task selected",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let para = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Goal "));
    f.render_widget(para, area);
}

fn draw_footer(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let line = if let Some(err) = &app.error {
        Line::from(Span::styled(
            format!(" ⚠ {err}"),
            Style::default().fg(Color::Red),
        ))
    } else {
        // The loop key flips between start and stop depending on run state.
        let loop_hint = if app.loop_running() {
            "x stop"
        } else {
            "s start"
        };
        Line::from(vec![Span::styled(
            format!(" ↑/↓ move · ⇥ team · {loop_hint} · r refresh · q quit"),
            Style::default().fg(Color::DarkGray),
        )])
    };
    f.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
}

/// True when `id` is safe to use as a single path segment under
/// `.bwoc/teams/`. The team id is joined into `<teams>/<id>/tasks.jsonl`, so a
/// value containing a path separator, a `..`/`.` component, a NUL, or an
/// absolute-path prefix could read outside the teams directory — reject those.
/// Accepts a non-empty id made only of ASCII alphanumerics, `-`, `_`, and `.`
/// (the kebab/snake convention), while still forbidding `.`/`..` as the whole
/// id. Mirrors the same-segment discipline the CLI applies on `bwoc team create`.
fn is_safe_team_id(id: &str) -> bool {
    if id.is_empty() || id == "." || id == ".." {
        return false;
    }
    id.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Number of log rows the loop pane can show: the pane height minus the two
/// border rows minus the one status-header line. Saturating so a pane too short
/// to hold anything yields `0` (rather than underflowing) — the tail iterator
/// then simply renders nothing.
fn log_rows(area_height: u16) -> usize {
    (area_height as usize).saturating_sub(2).saturating_sub(1)
}

/// Width available for a log line inside the pane (minus the two border columns),
/// floored at a small minimum so `truncate` always has room for at least a few
/// chars + the ellipsis even in a very narrow pane.
fn log_line_width(area_width: u16) -> usize {
    (area_width as usize).saturating_sub(2).max(8)
}

/// Truncate a title to `max` chars, appending `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, state: TaskState, deps: &[&str]) -> Task {
        let mut t = Task::new(
            id,
            format!("task {id}"),
            deps.iter().map(|s| s.to_string()).collect(),
        );
        t.state = state;
        t
    }

    fn app_with(tasks: Vec<Task>) -> App {
        App {
            workspace: PathBuf::from("/nonexistent"),
            teams: vec!["squad".into()],
            sel_team: 0,
            tasks,
            sel_task: 0,
            ticker_secs: 5,
            budget_iters: 20,
            last_refresh: Instant::now(),
            error: None,
            initial_team: None,
            run: None,
            backend: None,
            model: None,
            endpoint: None,
        }
    }

    #[test]
    fn goal_status_empty_when_no_tasks() {
        assert_eq!(app_with(vec![]).goal_status(), GoalStatus::Empty);
    }

    #[test]
    fn goal_status_done_when_all_completed() {
        let a = app_with(vec![
            task("t1", TaskState::Completed, &[]),
            task("t2", TaskState::Completed, &[]),
        ]);
        assert_eq!(a.goal_status(), GoalStatus::Done);
    }

    #[test]
    fn goal_status_in_progress_with_claimable_pending() {
        let a = app_with(vec![
            task("t1", TaskState::Completed, &[]),
            task("t2", TaskState::Pending, &["t1"]), // deps met → claimable
        ]);
        assert_eq!(a.goal_status(), GoalStatus::InProgress);
    }

    #[test]
    fn goal_status_blocked_when_all_pending_are_dep_blocked() {
        // t2 pending, depends on t1 which is also pending (not completed) →
        // nothing claimable, nothing in progress → Blocked.
        let a = app_with(vec![
            task("t1", TaskState::InProgress, &[]),
            task("t2", TaskState::Pending, &["t3"]), // t3 doesn't exist / not done
        ]);
        // t1 in progress → still InProgress, not Blocked.
        assert_eq!(a.goal_status(), GoalStatus::InProgress);

        let b = app_with(vec![
            task("t1", TaskState::Completed, &[]),
            task("t2", TaskState::Pending, &["t9"]), // dep never satisfiable here
        ]);
        assert_eq!(b.goal_status(), GoalStatus::Blocked);
    }

    #[test]
    fn counts_split_pending_into_claimable_and_blocked() {
        let a = app_with(vec![
            task("t1", TaskState::Completed, &[]),
            task("t2", TaskState::InProgress, &[]),
            task("t3", TaskState::Pending, &["t1"]), // claimable
            task("t4", TaskState::Pending, &["t2"]), // blocked (t2 not completed)
        ]);
        let c = a.counts();
        assert_eq!(c.completed, 1);
        assert_eq!(c.in_progress, 1);
        assert_eq!(c.pending, 1);
        assert_eq!(c.blocked, 1);
    }

    #[test]
    fn task_navigation_clamps() {
        let mut a = app_with(vec![
            task("t1", TaskState::Pending, &[]),
            task("t2", TaskState::Pending, &[]),
        ]);
        a.task_up(); // already at 0
        assert_eq!(a.sel_task, 0);
        a.task_down();
        a.task_down(); // clamps at last
        assert_eq!(a.sel_task, 1);
    }

    #[test]
    fn truncate_appends_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn log_pane_math_saturates_at_tiny_sizes() {
        // Heights too small for any log row → 0 (no underflow/panic).
        assert_eq!(log_rows(0), 0);
        assert_eq!(log_rows(2), 0); // just the two borders
        assert_eq!(log_rows(3), 0); // borders + status line, no room
        assert_eq!(log_rows(4), 1);
        assert_eq!(log_rows(12), 9);
        // Widths floor at 8 so truncate always has room.
        assert_eq!(log_line_width(0), 8);
        assert_eq!(log_line_width(2), 8);
        assert_eq!(log_line_width(40), 38);
        // truncate with the floored width never panics on a long line.
        let _ = truncate(&"x".repeat(200), log_line_width(0));
    }

    #[test]
    fn is_safe_team_id_rejects_traversal_and_separators() {
        // Safe ids.
        assert!(is_safe_team_id("squad"));
        assert!(is_safe_team_id("team-1"));
        assert!(is_safe_team_id("tian_ting.v2"));
        // Path-traversal / separators / absolute / control — all rejected.
        assert!(!is_safe_team_id(""));
        assert!(!is_safe_team_id("."));
        assert!(!is_safe_team_id(".."));
        assert!(!is_safe_team_id("../secrets"));
        assert!(!is_safe_team_id("a/b"));
        assert!(!is_safe_team_id("/etc/passwd"));
        assert!(!is_safe_team_id("a\\b"));
        assert!(!is_safe_team_id("a\0b"));
    }

    #[test]
    fn ticker_secs_floored_at_one_on_construction() {
        let app = App::new(LoopTuiArgs {
            workspace: PathBuf::from("/nonexistent"),
            team: None,
            ticker_secs: 0, // must floor to 1
            budget_iters: 0,
            backend: None,
            model: None,
            endpoint: None,
        });
        assert_eq!(app.ticker_secs, 1);
    }

    /// Render one frame into an off-screen backend and assert the key chrome is
    /// present — a headless smoke test that `draw()` lays out without panicking
    /// (no real TTY needed, so it runs in CI).
    #[test]
    fn draw_renders_without_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let app = app_with(vec![
            task("t1", TaskState::Completed, &[]),
            task("t2", TaskState::InProgress, &[]),
            task("t3", TaskState::Pending, &["t1"]),
        ]);
        let backend = TestBackend::new(90, 20);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();

        let text = buffer_text(term.backend().buffer());
        assert!(
            text.contains("BWOC Loop Control"),
            "header missing:\n{text}"
        );
        assert!(text.contains("Tasks"), "tasks pane missing");
        assert!(text.contains("Goal"), "goal pane missing");
        assert!(text.contains("In Progress"), "goal status missing");
        // PR2: the loop pane renders (idle, since no run was started).
        assert!(text.contains("Loop"), "loop pane missing");
        assert!(text.contains("idle"), "idle hint missing:\n{text}");
    }

    #[test]
    fn footer_hint_shows_start_when_idle() {
        // No run → footer offers 's start' (loop_running() is false).
        let app = app_with(vec![task("t1", TaskState::Pending, &[])]);
        assert!(!app.loop_running());
    }

    #[test]
    fn team_switch_blocked_hint_when_no_run_is_noop() {
        // Guard path is exercised via loop_running(); with no run, switching is
        // allowed (this documents the guard's default-open behaviour).
        let mut app = app_with(vec![task("t1", TaskState::Pending, &[])]);
        app.teams = vec!["a".into(), "b".into()];
        app.sel_team = 0;
        app.team_next(); // no run → switches freely
        assert_eq!(app.sel_team, 1);
        assert!(app.error.is_none());
    }

    /// Flatten a ratatui test buffer into a single string for substring asserts.
    fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
        let area = buf.area;
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }
}
