//! `bwoc-tui` — the full-screen ratatui chat client behind `bwoc chat --tui`.
//!
//! Its own crate (not a `bwoc-cli` module) so the TUI can grow without bloating
//! the CLI and so the ratatui/crossterm surface stays isolated. It drives a
//! `bwoc-harness --chat` subprocess and renders the `bwoc_core::chat_proto`
//! event stream. It compile-depends ONLY on `bwoc-core` (the protocol types +
//! sibling-binary resolution) — never on `bwoc-cli` or `bwoc-harness`. The
//! harness is a runtime subprocess, not a build dependency (the dep-quarantine:
//! nothing on the `bwoc` side pulls in the harness runtime graph).
//!
//! Architecture (no async, std-only):
//!   - The child's stdout is read line-by-line on a dedicated `std::thread`,
//!     each line parsed into a [`ChatEvent`] and forwarded over an
//!     `mpsc::channel` to the UI thread.
//!   - The UI thread runs the ratatui draw loop, polling crossterm for key
//!     events on a short (50ms) timeout, draining the channel between polls,
//!     and writing [`ChatInput`] lines to the child's stdin.
//!
//! Layout (one screen, no mouse; PgUp/PgDn/End scroll the conversation):
//!   ┌ status ──────────────────────────────────────────┐
//!   ┌ conversation ────────────┬ tools / activity ──────┐
//!   │ user + assistant turns   │ ToolCall/ToolResult,    │
//!   │ (streamed tokens inline) │ ⚠ PermissionRequest     │
//!   └──────────────────────────┴─────────────────────────┘
//!   ┌ input ───────────────────────────────────────────┐
//!
//! Keys: Enter sends the input buffer as `ChatInput::User`; on a pending
//! permission request `a`/`d` allow/deny; Ctrl-C (or `q` when input is empty)
//! sends `Quit`, restores the terminal, and exits.

mod session;

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use session::{AgentInfo, Session, SessionConfig, fetch_fleet};

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use bwoc_core::design;
use bwoc_core::manifest::Manifest;
use bwoc_core::trust::Principal;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

/// Default OpenAI-compatible endpoint when the agent's manifest has no
/// `baseUrl` (Ollama). Mirrors the harness's own `DEFAULT_ENDPOINT`; defined
/// locally so `bwoc-cli` need not depend on `bwoc-harness` for the constant.
const DEFAULT_ENDPOINT: &str = "http://localhost:11434/v1";

pub struct TuiArgs {
    pub agent_id: String,
    pub agent_path: PathBuf,
    /// Display name of the agent's backend (e.g. `ollama`), shown in the status
    /// line until the harness's `Ready` event delivers the authoritative value.
    /// A plain `String` so this crate needs no `bwoc-cli` `Backend` dependency.
    pub backend_name: String,
    /// Team chat broadcast log (HV3-3a). `Some(path)` forwards `--team-chat
    /// <path>` to the harness so this session joins a team's shared channel;
    /// `None` keeps it solo. The caller (`bwoc chat --team`) resolves the path.
    pub team_chat: Option<PathBuf>,
}

pub fn run(args: TuiArgs) -> i32 {
    use std::io::IsTerminal;
    if !io::stdout().is_terminal() {
        eprintln!(
            "bwoc chat --tui: stdout is not a TTY. Drop --tui to exec the backend, \
             or run this in an interactive terminal."
        );
        return 2;
    }

    // Resolve the harness binary (sibling of the running `bwoc`, then
    // CARGO_BIN_EXE, then PATH) — same shared rule `bwoc spawn` uses.
    let Some(harness) = bwoc_core::exec::sibling_binary("bwoc-harness") else {
        eprintln!(
            "bwoc chat --tui: bwoc-harness binary not found; install it \
             (`cargo install --path crates/bwoc-harness`) or add it to PATH."
        );
        return 2;
    };

    // Model + endpoint come from the agent's manifest (best-effort). A missing
    // manifest is not fatal — the harness falls back to its own defaults.
    let manifest = Manifest::load_from_path(&args.agent_path.join("config.manifest.json")).ok();
    let model = manifest.as_ref().map(|m| m.primary_model.clone());
    let endpoint = manifest
        .as_ref()
        .and_then(|m| m.base_url.clone())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());

    let argv = harness_argv(
        &args.agent_path,
        model.as_deref(),
        &endpoint,
        &args.backend_name,
        args.team_chat.as_deref(),
    );

    let mut child = match Command::new(&harness)
        .args(&argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Let the child's stderr pass through to ours; on the alt-screen it is
        // mostly invisible but still captured by any redirect the user set.
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "bwoc chat --tui: failed to spawn bwoc-harness ({}): {e}",
                harness.display()
            );
            return 1;
        }
    };

    // Reader thread: child stdout → ChatEvent → channel.
    let stdout = child.stdout.take().expect("stdout piped above");
    let (tx, rx) = mpsc::channel::<ChatEvent>();
    let reader = std::thread::spawn(move || {
        let buf = BufReader::new(stdout);
        for line in buf.lines() {
            let Ok(line) = line else { break };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Skip lines that aren't valid chat_proto events (the harness prints
            // a human banner before the stream begins). Forward-compatible:
            // unparseable lines are dropped, not fatal.
            if let Ok(ev) = serde_json::from_str::<ChatEvent>(line) {
                if tx.send(ev).is_err() {
                    break; // UI hung up.
                }
            }
        }
    });

    let stdin = child.stdin.take().expect("stdin piped above");

    let mut term = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc chat --tui: failed to enter alt screen: {e}");
            let _ = child.kill();
            return 1;
        }
    };

    let mut app = App::new(args.agent_id, &args.backend_name);
    let result = event_loop(&mut term, &mut app, &rx, stdin);

    if let Err(e) = restore_terminal() {
        eprintln!("bwoc chat --tui: warning — failed to restore terminal: {e}");
    }

    // The Quit/EOF path already asked the child to exit; reap it so we don't
    // leave a zombie, killing it if it ignored Quit.
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc chat --tui: {e}");
            1
        }
    }
}

/// Build the `bwoc-harness` argv (excluding the program name) for a chat
/// session against `agent_path`. Pure + tested: the wire contract with the
/// harness is `--chat --workdir <p> [--model <m>] --endpoint <url> --backend <b>`.
///
/// `model = None` omits `--model`, letting the harness use its own default
/// (the manifest had no `primaryModel`, which is unusual but not fatal).
/// `backend` selects the provider client: `ollama` / `openai-compatible` both
/// resolve to the OpenAI-compatible client, but `openrouter` is load-bearing —
/// it tells the harness to attach bearer auth, without which every request 401s.
/// `team_chat = Some(path)` appends `--team-chat <path>` (HV3-3a broadcast).
fn harness_argv(
    agent_path: &std::path::Path,
    model: Option<&str>,
    endpoint: &str,
    backend: &str,
    team_chat: Option<&std::path::Path>,
) -> Vec<String> {
    let mut argv = vec![
        "--chat".to_string(),
        "--workdir".to_string(),
        agent_path.to_string_lossy().into_owned(),
    ];
    if let Some(m) = model {
        argv.push("--model".to_string());
        argv.push(m.to_string());
    }
    argv.push("--endpoint".to_string());
    argv.push(endpoint.to_string());
    argv.push("--backend".to_string());
    argv.push(backend.to_string());
    if let Some(log) = team_chat {
        argv.push("--team-chat".to_string());
        argv.push(log.to_string_lossy().into_owned());
    }
    argv
}

// --- app state ------------------------------------------------------------

/// A pending permission request awaiting the operator's `a`/`d` decision.
struct Pending {
    id: String,
    tool: String,
    detail: String,
}

struct App {
    agent_id: String,
    backend: String,
    /// Status-line fields populated from the `Ready` event.
    status: Option<ReadyStatus>,
    /// Conversation scrollback (user + assistant lines).
    conversation: Vec<String>,
    /// Accumulator for the in-flight streamed assistant turn. Flushed to
    /// `conversation` on `Message`/`TurnEnd`.
    streaming: String,
    /// Tools / activity pane lines.
    activity: Vec<String>,
    /// The current input buffer (one line).
    input: String,
    /// A permission request awaiting `a`/`d`. Only one at a time.
    pending: Option<Pending>,
    /// The last `TurnEnd`'s `(prompt_tokens, completion_tokens)`. Because
    /// `prompt_tokens` already counts the whole resent history, the latest value
    /// doubles as an honest *current context size* proxy — no model→window table
    /// needed (which is why the header shows an absolute token count, not a %).
    usage: Option<(u64, u64)>,
    /// Session-cumulative completion (output) tokens — Σ over every `TurnEnd`.
    /// Summing `completion_tokens` is honest (each turn's output is disjoint);
    /// summing `prompt_tokens` would not be (history is recounted every turn).
    total_out: u64,
    /// How many times the harness compacted this session's context.
    compactions: u32,
    /// Set once the harness sends `Bye` (or its stream closes) — the loop exits.
    done: bool,
    /// Conversation scrollback offset: lines up from the live bottom. `0` pins
    /// the view to the newest turn; `PageUp` raises it, `End` returns to live.
    /// New content never yanks the view because the offset is bottom-relative.
    scroll: usize,
    /// Permission mode as last reported by the harness's `ModeChanged` — the
    /// harness is authoritative, so this mirrors whatever it sends (`default` |
    /// `accept_edits` | `bypass`, and `plan` if the harness enters it). `F2`
    /// cycles the first three; `plan` (only ever entered harness-side) falls
    /// back to `default` on the next `F2`. Shown in the status line.
    mode: String,
}

struct ReadyStatus {
    agent: String,
    model: String,
    backend: String,
}

impl App {
    fn new(agent_id: String, backend: &str) -> Self {
        Self {
            agent_id,
            backend: backend.to_string(),
            status: None,
            conversation: vec!["(waiting for harness to become ready…)".to_string()],
            streaming: String::new(),
            activity: Vec::new(),
            input: String::new(),
            pending: None,
            usage: None,
            total_out: 0,
            compactions: 0,
            done: false,
            scroll: 0,
            mode: "default".to_string(),
        }
    }

    /// The mode `F2` cycles to next: default → accept_edits → bypass → default.
    /// Any other value the harness may report (e.g. `plan`) also maps to
    /// `default`, so `F2` always re-enters the cycle from a known point.
    fn next_mode(&self) -> &'static str {
        match self.mode.as_str() {
            "default" => "accept_edits",
            "accept_edits" => "bypass",
            _ => "default",
        }
    }

    /// Apply a scroll key. `PageUp`/`PageDown` move by a fixed 10 lines (the
    /// offset is clamped to the content in `draw_conversation`); `End` returns to
    /// live. Returns true if the key was a scroll key (caller stops processing).
    fn scroll_key(&mut self, code: KeyCode) -> bool {
        const PAGE: usize = 10;
        match code {
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_add(PAGE);
                true
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_sub(PAGE);
                true
            }
            KeyCode::End => {
                self.scroll = 0;
                true
            }
            _ => false,
        }
    }

    /// Fold one harness event into the app state. Pure w.r.t. I/O — returns
    /// nothing; the loop redraws after applying. Factored so the event→state
    /// mapping is unit-testable without a terminal.
    fn apply(&mut self, ev: ChatEvent) {
        match ev {
            ChatEvent::Ready {
                agent,
                model,
                backend,
                ..
            } => {
                self.conversation.clear();
                self.conversation
                    .push(format!("● ready — {agent} · {model} · {backend}"));
                self.status = Some(ReadyStatus {
                    agent,
                    model,
                    backend,
                });
            }
            ChatEvent::Restored { role, text } => {
                // A replayed turn from a persisted session.
                self.conversation.push(format!("{role}: {text}"));
            }
            ChatEvent::Token { text } => {
                self.streaming.push_str(&text);
            }
            ChatEvent::Message { text } => {
                // A complete turn message supersedes any accumulated tokens.
                self.streaming.clear();
                self.conversation.push(format!("assistant: {text}"));
            }
            ChatEvent::ToolCall { id, name, args } => {
                self.activity.push(format!("→ {name}({args})  [{id}]"));
            }
            ChatEvent::ToolResult {
                id,
                name,
                ok,
                output,
            } => {
                let mark = if ok { "✓" } else { "✗" };
                self.activity
                    .push(format!("{mark} {name}: {output}  [{id}]"));
            }
            ChatEvent::PermissionRequest { id, tool, detail } => {
                self.activity
                    .push(format!("⚠ permission: {tool} — {detail}  [{id}]"));
                self.pending = Some(Pending { id, tool, detail });
            }
            ChatEvent::ModeChanged { mode } => {
                self.conversation.push(format!("● permission mode: {mode}"));
                self.mode = mode;
            }
            ChatEvent::Compacted { removed } => {
                self.compactions = self.compactions.saturating_add(1);
                self.conversation.push(format!(
                    "● context compacted — folded {removed} earlier messages"
                ));
            }
            ChatEvent::TurnEnd {
                prompt_tokens,
                completion_tokens,
            } => {
                // Flush any streamed-but-not-Message'd tokens as the turn's text.
                if !self.streaming.is_empty() {
                    let text = std::mem::take(&mut self.streaming);
                    self.conversation.push(format!("assistant: {text}"));
                }
                self.usage = Some((prompt_tokens, completion_tokens));
                self.total_out = self.total_out.saturating_add(completion_tokens);
            }
            ChatEvent::TeamMessage { from, text, .. } => {
                // A teammate's broadcast (HV3-3a) — render distinctly from this
                // agent's own turns so the human can follow the team thread.
                self.conversation.push(format!("📢 {from}: {text}"));
            }
            ChatEvent::Error { message } => {
                self.activity.push(format!("✗ error: {message}"));
            }
            ChatEvent::Bye => {
                self.conversation.push("● session ended".to_string());
                self.done = true;
            }
        }
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

/// Serialize a [`ChatInput`] and write it as one line to the child's stdin.
/// A broken pipe (harness exited) is surfaced so the loop can wind down.
fn send_input(stdin: &mut ChildStdin, input: &ChatInput) -> io::Result<()> {
    let line = input
        .to_line()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}

fn event_loop(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    rx: &Receiver<ChatEvent>,
    mut stdin: ChildStdin,
) -> io::Result<()> {
    loop {
        term.draw(|f| draw_frame(f, app))?;

        // Drain any harness events that arrived since the last draw.
        loop {
            match rx.try_recv() {
                Ok(ev) => app.apply(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Reader thread ended (child stdout closed). One last redraw
                    // happens at the top of the loop; mark done so we exit.
                    app.done = true;
                    break;
                }
            }
        }

        if app.done {
            // Best-effort polite quit; ignore a broken pipe (child already gone).
            let _ = send_input(&mut stdin, &ChatInput::Quit);
            return Ok(());
        }

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && handle_key(app, &mut stdin, key)?
        {
            // Polite quit before we tear down the terminal.
            let _ = send_input(&mut stdin, &ChatInput::Quit);
            return Ok(());
        }
    }
}

/// Process one key event. Returns `Ok(true)` when the user requested quit.
fn handle_key(app: &mut App, stdin: &mut ChildStdin, key: KeyEvent) -> io::Result<bool> {
    let KeyEvent {
        code, modifiers, ..
    } = key;

    // Ctrl-C always quits, regardless of input/pending state.
    if let (KeyCode::Char('c'), KeyModifiers::CONTROL) = (code, modifiers) {
        return Ok(true);
    }

    // A pending permission request captures a/d (and only those); anything else
    // falls through so the user can keep typing while deciding.
    if let Some(p) = &app.pending {
        match code {
            KeyCode::Char('a') => {
                let id = p.id.clone();
                let tool = p.tool.clone();
                app.pending = None;
                app.activity.push(format!("✓ allowed {tool}"));
                send_input(stdin, &ChatInput::Permission { id, allow: true })?;
                return Ok(false);
            }
            KeyCode::Char('d') => {
                let id = p.id.clone();
                let tool = p.tool.clone();
                app.pending = None;
                app.activity.push(format!("✗ denied {tool}"));
                send_input(stdin, &ChatInput::Permission { id, allow: false })?;
                return Ok(false);
            }
            _ => {}
        }
    }

    // Scrollback navigation (PageUp/PageDown/End) — before input editing.
    if app.scroll_key(code) {
        return Ok(false);
    }

    match code {
        // F2 cycles the permission mode. Update `mode` optimistically so a
        // second press advances even before the harness echoes `ModeChanged`
        // (it may not, e.g. while a permission prompt is outstanding); the echo
        // overwrites it authoritatively when it arrives.
        KeyCode::F(2) => {
            let next = app.next_mode().to_string();
            app.mode = next.clone();
            send_input(stdin, &ChatInput::SetMode { mode: next })?;
            Ok(false)
        }
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            if !text.trim().is_empty() {
                app.scroll = 0; // jump to live so the reply is visible
                app.conversation.push(format!("you: {text}"));
                // The local TUI is the trusted operator channel (Phase 5 t1).
                send_input(
                    stdin,
                    &ChatInput::User {
                        text,
                        principal: Principal::LocalOperator,
                    },
                )?;
            }
            Ok(false)
        }
        KeyCode::Backspace => {
            app.input.pop();
            Ok(false)
        }
        // `q` quits only when the input line is empty (otherwise it's a literal
        // character the user is typing).
        KeyCode::Char('q') if app.input.is_empty() => Ok(true),
        KeyCode::Char(c) => {
            app.input.push(c);
            Ok(false)
        }
        KeyCode::Esc => Ok(true),
        _ => Ok(false),
    }
}

// --- drawing --------------------------------------------------------------

fn draw_frame(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status
            Constraint::Min(0),    // body (conversation + activity)
            Constraint::Length(3), // input box
        ])
        .split(area);

    draw_status(f, layout[0], app);
    draw_body(f, layout[1], app);
    draw_input(f, layout[2], app);
}

/// Map a design token's ANSI half to ratatui's *named* colour, so the user's
/// terminal theme keeps authority over the exact shade.
fn tone(t: design::ColorToken) -> Color {
    use design::Ansi;
    match t.ansi {
        Ansi::Black => Color::Black,
        Ansi::Red => Color::Red,
        Ansi::Green => Color::Green,
        Ansi::Yellow => Color::Yellow,
        Ansi::Blue => Color::Blue,
        Ansi::Magenta => Color::Magenta,
        Ansi::Cyan => Color::Cyan,
        Ansi::Gray => Color::Gray,
        Ansi::DarkGray => Color::DarkGray,
        Ansi::White => Color::White,
    }
}

fn draw_status(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let text = status_line(app);
    let p = Paragraph::new(Line::from(Span::styled(
        text,
        Style::default()
            .fg(Color::Black)
            .bg(tone(design::color::ACCENT))
            .add_modifier(Modifier::BOLD),
    )));
    f.render_widget(p, area);
}

/// Compact a token count: `512`, `9.1k`, `1.2M`. Keeps the header narrow while
/// staying readable (one decimal above 1k / 1M, dropping a trailing `.0`).
fn fmt_tokens(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
            .replace(".0k", "k")
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
            .replace(".0M", "M")
    }
}

/// Build the one-line status string from the `Ready` event + live usage.
///
/// Token segment is honest-data-only (no model→window/price tables): `ctx` is
/// the last turn's `prompt_tokens` (which already counts the resent history, so
/// it *is* the current context size), `out` is Σ completion tokens this session,
/// and `⟳N` the compaction count. Absolute counts, never a fabricated %/cost.
/// Pure + tested.
fn status_line(app: &App) -> String {
    let base = match &app.status {
        Some(s) => format!(" {} · model {} · backend {} ", s.agent, s.model, s.backend),
        None => format!(
            " {} · backend {} · (connecting…) ",
            app.agent_id, app.backend
        ),
    };
    let usage = match app.usage {
        Some((p, _)) => format!(
            "{base}· ctx {} · out {} ",
            fmt_tokens(p),
            fmt_tokens(app.total_out)
        ),
        None => base,
    };
    let compacted = if app.compactions > 0 {
        format!("· ⟳{} ", app.compactions)
    } else {
        String::new()
    };
    format!("{usage}{compacted}· mode {} (F2) ", app.mode)
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    draw_conversation(f, h[0], app);
    draw_activity(f, h[1], app);
}

fn draw_conversation(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = app
        .conversation
        .iter()
        .map(|l| Line::from(l.clone()))
        .collect();
    // Show the in-flight streamed turn live, below the committed history.
    if !app.streaming.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("assistant: {}", app.streaming),
            Style::default().add_modifier(Modifier::DIM),
        )));
    }

    // The view is anchored to the tail; `app.scroll` (lines up from the bottom)
    // lets the operator page back through history. `skip` drops that many lines
    // from the front, scaled up by the scroll offset; both are clamped, so the
    // window can never run past either end.
    let inner_height = area.height.saturating_sub(2) as usize; // borders
    let total = lines.len();
    let scroll = app.scroll.min(total.saturating_sub(inner_height.max(1)));
    let skip = total.saturating_sub(inner_height.max(1) + scroll);
    let visible: Vec<Line> = lines
        .into_iter()
        .skip(skip)
        .take(inner_height.max(1))
        .collect();

    let title = if scroll > 0 {
        format!(" conversation ↑{scroll} (End=live) ")
    } else {
        " conversation ".to_string()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(tone(design::color::ACCENT)));
    let p = Paragraph::new(visible)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn draw_activity(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let skip = app.activity.len().saturating_sub(inner_height.max(1));
    let lines: Vec<Line> = app
        .activity
        .iter()
        .skip(skip)
        .map(|l| {
            let style = if l.starts_with('⚠') {
                Style::default()
                    .fg(tone(design::color::WARNING))
                    .add_modifier(Modifier::BOLD)
            } else if l.starts_with('✗') {
                Style::default().fg(tone(design::color::DANGER))
            } else if l.starts_with('✓') {
                Style::default().fg(tone(design::color::SUCCESS))
            } else {
                Style::default()
            };
            Line::from(Span::styled(l.clone(), style))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" tools / activity ")
        .border_style(Style::default().add_modifier(Modifier::DIM));
    let p = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn draw_input(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let (title, border) = match &app.pending {
        Some(p) => (
            format!(" permission: {} ({}) — [a]llow / [d]eny ", p.tool, p.detail),
            Style::default()
                .fg(tone(design::color::WARNING))
                .add_modifier(Modifier::BOLD),
        ),
        None => (
            " input — Enter send · q/Esc/Ctrl-C quit ".to_string(),
            Style::default().fg(tone(design::color::ACCENT)),
        ),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border);
    let p = Paragraph::new(Line::from(format!("> {}", app.input))).block(block);
    f.render_widget(p, area);
}

// ===========================================================================
// Fleet mode (multi-agent) — a left fleet sidebar + one live session per agent
// ===========================================================================
//
// Additive over the single-agent path: it reuses the very same per-agent [`App`]
// (event mapping + draw helpers), one instance per fleet member, and drains
// **every** live [`session::Session`] each tick so a background agent keeps
// streaming into its pane (and its sidebar dot) while you read another. `Tab`
// switches which pane fills the chat area.

/// Args for the multi-agent fleet TUI (`bwoc chat <agent> --tui --fleet`).
/// Backend/model/endpoint are chosen once for the session and applied to every
/// agent (per-agent manifest resolution is a later slice).
pub struct FleetArgs {
    pub workdir: PathBuf,
    pub backend: String,
    pub model: String,
    pub endpoint: String,
}

struct Fleet {
    cfg: SessionConfig,
    agents: Vec<AgentInfo>,
    panes: HashMap<String, App>,
    sessions: HashMap<String, Session>,
    selected: usize,
    /// The `Ctrl-P` command palette when open (`None` = closed).
    palette: Option<Palette>,
}

/// Command-palette state: the filter text + the highlighted row.
#[derive(Default)]
struct Palette {
    query: String,
    sel: usize,
}

/// What a chosen palette row does.
enum PaletteAction {
    /// Switch the active pane to `agents[i]`.
    Switch(usize),
    /// Forget the active agent's persisted conversation.
    Forget,
    /// Quit the TUI.
    Quit,
}

struct PaletteItem {
    label: String,
    action: PaletteAction,
}

impl Fleet {
    fn new(agents: Vec<AgentInfo>, cfg: SessionConfig) -> Self {
        let mut f = Self {
            cfg,
            agents,
            panes: HashMap::new(),
            sessions: HashMap::new(),
            selected: 0,
            palette: None,
        };
        if let Some(id) = f.agents.first().map(|a| a.id.clone()) {
            f.open(&id);
        }
        f
    }

    fn active_id(&self) -> Option<String> {
        self.agents.get(self.selected).map(|a| a.id.clone())
    }

    /// The command set, filtered by the palette query (case-insensitive
    /// substring). Order: switch-to-each-agent, forget, quit.
    fn commands(&self) -> Vec<PaletteItem> {
        let mut items: Vec<PaletteItem> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, a)| PaletteItem {
                label: format!("→ switch to {}", a.id),
                action: PaletteAction::Switch(i),
            })
            .collect();
        items.push(PaletteItem {
            label: "⊘ forget this agent's conversation".to_string(),
            action: PaletteAction::Forget,
        });
        items.push(PaletteItem {
            label: "✕ quit".to_string(),
            action: PaletteAction::Quit,
        });
        let q = self
            .palette
            .as_ref()
            .map(|p| p.query.to_lowercase())
            .unwrap_or_default();
        if q.is_empty() {
            items
        } else {
            items
                .into_iter()
                .filter(|it| it.label.to_lowercase().contains(&q))
                .collect()
        }
    }

    /// Execute the highlighted command. Returns true iff it was Quit.
    fn palette_execute(&mut self) -> bool {
        let Some(p) = self.palette.as_ref() else {
            return false;
        };
        let items = self.commands();
        let Some(item) = items.get(p.sel) else {
            self.palette = None;
            return false;
        };
        let quit = match &item.action {
            &PaletteAction::Switch(i) => {
                self.selected = i;
                if let Some(id) = self.active_id() {
                    self.open(&id);
                }
                false
            }
            PaletteAction::Forget => {
                if let Some(id) = self.active_id() {
                    if let Some(s) = self.sessions.get_mut(&id) {
                        s.send(&ChatInput::Forget);
                    }
                    if let Some(pane) = self.panes.get_mut(&id) {
                        pane.conversation.push("● forgot conversation".to_string());
                    }
                }
                false
            }
            PaletteAction::Quit => true,
        };
        self.palette = None;
        quit
    }

    /// Ensure a pane + session exist for `agent` (lazy, idempotent). Each agent
    /// is driven with its **own** backend + manifest-resolved model/endpoint
    /// (see [`SessionConfig::for_agent`]); a vendor-CLI backend the harness can't
    /// drive gets a pane with a hint but no session.
    fn open(&mut self, agent: &str) {
        let Some(info) = self.agents.iter().find(|a| a.id == agent).cloned() else {
            return;
        };
        self.panes
            .entry(agent.to_string())
            .or_insert_with(|| App::new(agent.to_string(), &info.backend));
        if self.sessions.contains_key(agent) {
            return;
        }
        if !session::is_harness_drivable(&info.backend) {
            if let Some(p) = self.panes.get_mut(agent) {
                // Only note it once — `open` is called on every pane switch.
                let hint = format!(
                    "● {} backend — not harness-drivable; open with `bwoc chat {}` directly",
                    info.backend, agent
                );
                if p.conversation.last() != Some(&hint) {
                    p.conversation.push(hint);
                }
            }
            return;
        }
        let cfg = self.cfg.for_agent(&info);
        match Session::spawn(agent, &cfg) {
            Ok(s) => {
                self.sessions.insert(agent.to_string(), s);
            }
            Err(e) => {
                if let Some(p) = self.panes.get_mut(agent) {
                    p.activity.push(format!("✗ failed to start session: {e}"));
                }
            }
        }
    }

    /// Fold every live session's pending events into its pane. Collect each
    /// session's currently-available events first (immutable borrow), then apply
    /// them to the pane (mutable) — two phases so `sessions` and `panes` never
    /// alias, and no manual `try_recv` loop for clippy to grumble about.
    fn drain(&mut self) {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        let mut dead = Vec::new();
        for id in ids {
            let events: Vec<ChatEvent> = match self.sessions.get(&id) {
                Some(s) => s.rx.try_iter().collect(),
                None => continue,
            };
            for ev in events {
                if matches!(ev, ChatEvent::Bye) {
                    dead.push(id.clone());
                }
                if let Some(p) = self.panes.get_mut(&id) {
                    p.apply(ev);
                }
            }
            // A harness that exited *without* a Bye leaves the channel merely
            // disconnected (indistinguishable from "empty" via try_iter), so
            // check the child directly and reap it — otherwise the Session +
            // its Child handle would leak.
            if self.sessions.get_mut(&id).is_some_and(|s| !s.is_alive()) {
                dead.push(id.clone());
            }
        }
        for id in dead {
            self.sessions.remove(&id);
        }
    }

    fn switch(&mut self, delta: i32) {
        if self.agents.is_empty() {
            return;
        }
        let n = self.agents.len() as i32;
        self.selected = (((self.selected as i32 + delta) % n + n) % n) as usize;
        if let Some(id) = self.active_id() {
            self.open(&id);
        }
    }
}

/// Entry point for the fleet TUI. Discovers agents via `bwoc list --json`,
/// opens the first, and runs the sidebar loop.
pub fn run_fleet(args: FleetArgs) -> i32 {
    use std::io::IsTerminal;
    if !io::stdout().is_terminal() {
        eprintln!("bwoc chat --tui: stdout is not a TTY — run in an interactive terminal.");
        return 2;
    }
    let Some(harness) = bwoc_core::exec::sibling_binary("bwoc-harness") else {
        eprintln!("bwoc chat --tui: bwoc-harness binary not found (install it or add to PATH).");
        return 2;
    };
    let bwoc_bin = bwoc_core::exec::sibling_binary("bwoc")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bwoc".to_string());
    let cfg = SessionConfig {
        harness_bin: harness.to_string_lossy().into_owned(),
        workdir: args.workdir.to_string_lossy().into_owned(),
        backend: args.backend,
        model: args.model,
        endpoint: args.endpoint,
    };
    let agents = fetch_fleet(&bwoc_bin, &cfg.workdir);
    if agents.is_empty() {
        eprintln!(
            "bwoc chat --tui: no agents found in {} (bwoc list --json).",
            cfg.workdir
        );
        // Exit 2 — a user/input error (no fleet to show), consistent with the
        // CLI's "no workspace" / "no agent" codes.
        return 2;
    }

    let mut term = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc chat --tui: failed to enter alt screen: {e}");
            return 1;
        }
    };
    let mut fleet = Fleet::new(agents, cfg);
    let result = fleet_event_loop(&mut term, &mut fleet);
    if let Err(e) = restore_terminal() {
        eprintln!("bwoc chat --tui: warning — failed to restore terminal: {e}");
    }
    // Dropping `fleet` drops each Session → Quit + kill + reap every child.
    drop(fleet);
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc chat --tui: {e}");
            1
        }
    }
}

fn fleet_event_loop(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    fleet: &mut Fleet,
) -> io::Result<()> {
    loop {
        term.draw(|f| draw_fleet(f, fleet))?;
        fleet.drain();
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && fleet_handle_key(fleet, key)?
        {
            return Ok(());
        }
    }
}

/// Returns `Ok(true)` on quit. Routes input to the **active** pane's session.
fn fleet_handle_key(fleet: &mut Fleet, key: KeyEvent) -> io::Result<bool> {
    let KeyEvent {
        code, modifiers, ..
    } = key;
    if let (KeyCode::Char('c'), KeyModifiers::CONTROL) = (code, modifiers) {
        return Ok(true);
    }

    // Command palette captures all input while open.
    if fleet.palette.is_some() {
        let n = fleet.commands().len();
        match code {
            KeyCode::Esc => fleet.palette = None,
            KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => fleet.palette = None,
            KeyCode::Enter => return Ok(fleet.palette_execute()),
            KeyCode::Up => {
                if let Some(p) = fleet.palette.as_mut() {
                    p.sel = p.sel.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(p) = fleet.palette.as_mut() {
                    p.sel = (p.sel + 1).min(n.saturating_sub(1));
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = fleet.palette.as_mut() {
                    p.query.pop();
                    p.sel = 0;
                }
            }
            KeyCode::Char(c) => {
                if let Some(p) = fleet.palette.as_mut() {
                    p.query.push(c);
                    p.sel = 0;
                }
            }
            _ => {}
        }
        return Ok(false);
    }
    if let (KeyCode::Char('p'), KeyModifiers::CONTROL) = (code, modifiers) {
        fleet.palette = Some(Palette::default());
        return Ok(false);
    }

    match code {
        KeyCode::Tab => {
            fleet.switch(1);
            return Ok(false);
        }
        KeyCode::BackTab => {
            fleet.switch(-1);
            return Ok(false);
        }
        _ => {}
    }
    let Some(id) = fleet.active_id() else {
        return Ok(false);
    };

    // A pending approval on the active pane captures a/d.
    let pending_id = fleet
        .panes
        .get(&id)
        .and_then(|p| p.pending.as_ref().map(|pd| pd.id.clone()));
    if let Some(pid) = pending_id {
        match code {
            KeyCode::Char('a') | KeyCode::Char('d') => {
                let allow = code == KeyCode::Char('a');
                if let Some(s) = fleet.sessions.get_mut(&id) {
                    s.send(&ChatInput::Permission { id: pid, allow });
                }
                if let Some(p) = fleet.panes.get_mut(&id) {
                    p.pending = None;
                    p.activity
                        .push(if allow { "✓ allowed" } else { "✗ denied" }.to_string());
                }
                return Ok(false);
            }
            _ => {}
        }
    }

    // Scrollback on the active pane (PageUp/PageDown/End).
    if let Some(p) = fleet.panes.get_mut(&id) {
        if p.scroll_key(code) {
            return Ok(false);
        }
    }

    match code {
        // F2 cycles the active pane's permission mode. Update the pane's `mode`
        // optimistically (see the single-agent F2 handler) so repeated presses
        // advance even if the harness defers its `ModeChanged` echo.
        KeyCode::F(2) => {
            if let Some(next) = fleet.panes.get(&id).map(|p| p.next_mode().to_string()) {
                if let Some(p) = fleet.panes.get_mut(&id) {
                    p.mode = next.clone();
                }
                if let Some(s) = fleet.sessions.get_mut(&id) {
                    s.send(&ChatInput::SetMode { mode: next });
                }
            }
            Ok(false)
        }
        KeyCode::Enter => {
            let text = fleet
                .panes
                .get_mut(&id)
                .map(|p| std::mem::take(&mut p.input))
                .unwrap_or_default();
            if !text.trim().is_empty() {
                if let Some(p) = fleet.panes.get_mut(&id) {
                    p.conversation.push(format!("you: {text}"));
                    p.scroll = 0; // jump to live
                }
                if let Some(s) = fleet.sessions.get_mut(&id) {
                    s.send(&ChatInput::User {
                        text,
                        principal: Principal::LocalOperator,
                    });
                }
            }
            Ok(false)
        }
        KeyCode::Backspace => {
            if let Some(p) = fleet.panes.get_mut(&id) {
                p.input.pop();
            }
            Ok(false)
        }
        KeyCode::Char('q')
            if fleet
                .panes
                .get(&id)
                .map(|p| p.input.is_empty())
                .unwrap_or(true) =>
        {
            Ok(true)
        }
        KeyCode::Char(c) => {
            if let Some(p) = fleet.panes.get_mut(&id) {
                p.input.push(c);
            }
            Ok(false)
        }
        KeyCode::Esc => Ok(true),
        _ => Ok(false),
    }
}

fn draw_fleet(f: &mut ratatui::Frame, fleet: &Fleet) {
    let area = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(20)])
        .split(rows[1]);

    draw_fleet_sidebar(f, cols[0], fleet);
    if let Some(id) = fleet.active_id() {
        if let Some(app) = fleet.panes.get(&id) {
            draw_status(f, rows[0], app);
            draw_body(f, cols[1], app);
            draw_input(f, rows[2], app);
        }
    }
    if fleet.palette.is_some() {
        draw_palette(f, area, fleet);
    }
}

/// Centered command-palette overlay: a filter line + the matching commands with
/// the selected row highlighted.
fn draw_palette(f: &mut ratatui::Frame, area: Rect, fleet: &Fleet) {
    let items = fleet.commands();
    let sel = fleet.palette.as_ref().map(|p| p.sel).unwrap_or(0);
    let query = fleet
        .palette
        .as_ref()
        .map(|p| p.query.as_str())
        .unwrap_or("");

    // A modest centered box (guarding the caps so clamp can't be inverted on a
    // tiny terminal).
    let w = area.width.saturating_sub(8).clamp(20, 60);
    let h_cap = area.height.saturating_sub(4).max(4);
    let h = (items.len() as u16 + 4).clamp(4, h_cap);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let rect = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(ratatui::widgets::Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" commands (Esc close) ")
        .border_style(Style::default().fg(tone(design::color::ACCENT)));
    f.render_widget(block, rect);

    let inner = Rect {
        x: rect.x + 1,
        y: rect.y + 1,
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    };
    let irows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(Color::DarkGray)),
            Span::raw(query.to_string()),
            Span::styled("▏", Style::default().fg(Color::Gray)),
        ])),
        irows[0],
    );
    let list: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let style = if i == sel {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let marker = if i == sel { "▶ " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{marker}{}", it.label),
                style,
            )))
        })
        .collect();
    f.render_widget(List::new(list), irows[1]);
}

fn draw_fleet_sidebar(f: &mut ratatui::Frame, area: Rect, fleet: &Fleet) {
    let items: Vec<ListItem> = fleet
        .agents
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let pane = fleet.panes.get(&a.id);
            let busy = pane
                .map(|p| !p.streaming.is_empty() || p.pending.is_some())
                .unwrap_or(false);
            let (dot, color) = if busy {
                ("●", Color::Green)
            } else if a.inbox_count > 0 {
                ("◍", Color::Yellow)
            } else if a.running {
                ("●", Color::DarkGray)
            } else {
                ("○", Color::DarkGray)
            };
            let name = a.id.strip_prefix("agent-").unwrap_or(&a.id).to_string();
            let unread = if a.inbox_count > 0 {
                format!(" ({})", a.inbox_count)
            } else {
                String::new()
            };
            let name_style = if i == fleet.selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{dot} "), Style::default().fg(color)),
                Span::styled(format!("{name}{unread}"), name_style),
            ]))
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" fleet ")
        .border_style(Style::default().fg(tone(design::color::ACCENT)));
    f.render_widget(List::new(items).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_argv_includes_chat_workdir_model_endpoint() {
        let argv = harness_argv(
            std::path::Path::new("/ws/agent-pi"),
            Some("gpt-5.5"),
            "https://api.openai.com/v1",
            "openai-compatible",
            None,
        );
        assert_eq!(
            argv,
            [
                "--chat",
                "--workdir",
                "/ws/agent-pi",
                "--model",
                "gpt-5.5",
                "--endpoint",
                "https://api.openai.com/v1",
                "--backend",
                "openai-compatible",
            ]
        );
    }

    #[test]
    fn harness_argv_omits_model_when_none() {
        let argv = harness_argv(
            std::path::Path::new("/ws/agent-pi"),
            None,
            DEFAULT_ENDPOINT,
            "ollama",
            None,
        );
        assert_eq!(
            argv,
            [
                "--chat",
                "--workdir",
                "/ws/agent-pi",
                "--endpoint",
                DEFAULT_ENDPOINT,
                "--backend",
                "ollama",
            ]
        );
        assert!(!argv.iter().any(|a| a == "--model"));
    }

    #[test]
    fn harness_argv_passes_openrouter_backend() {
        // The TUI path must forward `--backend openrouter` so the harness
        // attaches bearer auth; otherwise OpenRouter requests 401.
        let argv = harness_argv(
            std::path::Path::new("/ws/agent-or"),
            Some("anthropic/claude-opus-4-8"),
            DEFAULT_ENDPOINT,
            "openrouter",
            None,
        );
        let i = argv
            .iter()
            .position(|a| a == "--backend")
            .expect("flag present");
        assert_eq!(argv.get(i + 1).map(String::as_str), Some("openrouter"));
    }

    #[test]
    fn harness_argv_appends_team_chat_when_set() {
        let argv = harness_argv(
            std::path::Path::new("/ws/agent-pi"),
            None,
            DEFAULT_ENDPOINT,
            "ollama",
            Some(std::path::Path::new("/ws/.bwoc/teams/squad/chat.jsonl")),
        );
        // `--team-chat <path>` is appended as a trailing pair.
        let i = argv
            .iter()
            .position(|a| a == "--team-chat")
            .expect("flag present");
        assert_eq!(
            argv.get(i + 1).map(String::as_str),
            Some("/ws/.bwoc/teams/squad/chat.jsonl")
        );
    }

    #[test]
    fn status_line_uses_ready_fields_when_present() {
        let mut app = App::new("agent-pi".into(), "ollama");
        app.apply(ChatEvent::Ready {
            agent: "agent-pi".into(),
            model: "llama3".into(),
            backend: "ollama".into(),
            tools: vec![],
        });
        let s = status_line(&app);
        assert!(s.contains("agent-pi"));
        assert!(s.contains("llama3"));
        assert!(s.contains("ollama"));
    }

    #[test]
    fn status_line_falls_back_before_ready() {
        let app = App::new("agent-pi".into(), "openai-compatible");
        let s = status_line(&app);
        assert!(s.contains("agent-pi"));
        assert!(s.contains("openai-compatible"));
        assert!(s.contains("connecting"));
    }

    #[test]
    fn apply_message_appends_assistant_line_and_clears_stream() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::Token {
            text: "partial".into(),
        });
        assert_eq!(app.streaming, "partial");
        app.apply(ChatEvent::Message {
            text: "final answer".into(),
        });
        assert!(app.streaming.is_empty());
        assert!(
            app.conversation
                .iter()
                .any(|l| l == "assistant: final answer")
        );
    }

    #[test]
    fn apply_team_message_renders_with_broadcast_marker() {
        let mut app = App::new("agent-pi".into(), "ollama");
        app.apply(ChatEvent::TeamMessage {
            from: "agent-a".into(),
            text: "found the root cause".into(),
            ts: "2026-06-06T00:00:00Z".into(),
        });
        assert!(
            app.conversation
                .iter()
                .any(|l| l == "📢 agent-a: found the root cause"),
            "team message renders distinctly: {:?}",
            app.conversation
        );
    }

    #[test]
    fn apply_turn_end_flushes_streamed_tokens_and_records_usage() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::Token {
            text: "streamed".into(),
        });
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 10,
            completion_tokens: 20,
        });
        assert_eq!(app.usage, Some((10, 20)));
        assert!(app.streaming.is_empty());
        assert!(app.conversation.iter().any(|l| l == "assistant: streamed"));
    }

    #[test]
    fn total_out_accumulates_but_ctx_tracks_latest_prompt() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 1_000,
            completion_tokens: 200,
        });
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 1_500, // grew as history was resent
            completion_tokens: 300,
        });
        // out is Σ completion; ctx is the *latest* prompt (not summed).
        assert_eq!(app.total_out, 500);
        assert_eq!(app.usage, Some((1_500, 300)));
        let s = status_line(&app);
        assert!(s.contains("ctx 1.5k"), "got: {s}");
        assert!(s.contains("out 500"), "got: {s}");
    }

    #[test]
    fn compaction_count_shown_only_after_a_compaction() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::Ready {
            agent: "a".into(),
            model: "m".into(),
            backend: "ollama".into(),
            tools: vec![],
        });
        assert!(!status_line(&app).contains('⟳'));
        app.apply(ChatEvent::Compacted { removed: 4 });
        app.apply(ChatEvent::Compacted { removed: 2 });
        assert_eq!(app.compactions, 2);
        assert!(status_line(&app).contains("⟳2"));
    }

    #[test]
    fn fmt_tokens_is_compact_and_readable() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(512), "512");
        assert_eq!(fmt_tokens(1_000), "1k");
        assert_eq!(fmt_tokens(9_100), "9.1k");
        assert_eq!(fmt_tokens(1_200_000), "1.2M");
        assert_eq!(fmt_tokens(2_000_000), "2M");
    }

    #[test]
    fn permission_request_sets_pending_and_marks_activity() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::PermissionRequest {
            id: "p1".into(),
            tool: "run_command".into(),
            detail: "rm -rf build/".into(),
        });
        assert!(app.pending.is_some());
        assert_eq!(app.pending.as_ref().unwrap().id, "p1");
        assert!(app.activity.iter().any(|l| l.starts_with('⚠')));
    }

    #[test]
    fn bye_marks_session_done() {
        let mut app = App::new("a".into(), "ollama");
        assert!(!app.done);
        app.apply(ChatEvent::Bye);
        assert!(app.done);
    }

    fn fleet_for_test(ids: &[&str]) -> Fleet {
        Fleet {
            cfg: SessionConfig {
                harness_bin: "true".into(),
                workdir: "/tmp".into(),
                backend: "ollama".into(),
                model: String::new(),
                endpoint: "x".into(),
            },
            agents: ids
                .iter()
                .map(|id| AgentInfo {
                    id: (*id).to_string(),
                    backend: "ollama".into(),
                    path: format!("agents/{id}"),
                    running: false,
                    inbox_count: 0,
                })
                .collect(),
            panes: HashMap::new(),
            sessions: HashMap::new(),
            selected: 0,
            palette: Some(Palette::default()),
        }
    }

    #[test]
    fn palette_lists_and_filters_commands() {
        let mut fleet = fleet_for_test(&["agent-pi", "agent-jisoo"]);
        // 2 switch entries + forget + quit.
        let all = fleet.commands();
        assert_eq!(all.len(), 4);
        assert!(all[0].label.contains("agent-pi"));
        assert!(matches!(all[3].action, PaletteAction::Quit));
        // Case-insensitive substring filter.
        fleet.palette = Some(Palette {
            query: "JISOO".into(),
            sel: 0,
        });
        let filtered = fleet.commands();
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].label.contains("jisoo"));
        // Filtering to quit, then executing it, returns true.
        fleet.palette = Some(Palette {
            query: "quit".into(),
            sel: 0,
        });
        assert!(fleet.palette_execute());
        assert!(fleet.palette.is_none()); // closes after execute
    }

    #[test]
    fn mode_cycles_and_reflects_mode_changed() {
        let mut app = App::new("a".into(), "ollama");
        assert_eq!(app.mode, "default");
        assert_eq!(app.next_mode(), "accept_edits");
        app.apply(ChatEvent::ModeChanged {
            mode: "accept_edits".into(),
        });
        assert_eq!(app.mode, "accept_edits");
        assert_eq!(app.next_mode(), "bypass");
        app.apply(ChatEvent::ModeChanged {
            mode: "bypass".into(),
        });
        assert_eq!(app.next_mode(), "default");
        assert!(status_line(&app).contains("mode bypass"));
    }

    #[test]
    fn scroll_key_pages_and_end_returns_to_live() {
        let mut app = App::new("a".into(), "ollama");
        assert_eq!(app.scroll, 0);
        assert!(app.scroll_key(KeyCode::PageUp));
        assert_eq!(app.scroll, 10);
        assert!(app.scroll_key(KeyCode::PageUp));
        assert_eq!(app.scroll, 20);
        assert!(app.scroll_key(KeyCode::PageDown));
        assert_eq!(app.scroll, 10);
        assert!(app.scroll_key(KeyCode::End));
        assert_eq!(app.scroll, 0);
        // PageDown at the bottom saturates at 0 (never negative).
        assert!(app.scroll_key(KeyCode::PageDown));
        assert_eq!(app.scroll, 0);
        // A non-scroll key is not consumed.
        assert!(!app.scroll_key(KeyCode::Enter));
    }
}
