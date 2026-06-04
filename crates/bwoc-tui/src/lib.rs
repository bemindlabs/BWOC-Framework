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
//! Layout (one screen, no mouse, no scrollbar — v1 is deliberately lean):
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

use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use bwoc_core::design;
use bwoc_core::manifest::Manifest;
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
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

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

    let argv = harness_argv(&args.agent_path, model.as_deref(), &endpoint);

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
/// harness is `--chat --workdir <p> [--model <m>] --endpoint <url>`.
///
/// `model = None` omits `--model`, letting the harness use its own default
/// (the manifest had no `primaryModel`, which is unusual but not fatal).
fn harness_argv(agent_path: &std::path::Path, model: Option<&str>, endpoint: &str) -> Vec<String> {
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
    /// Cumulative token usage from the last `TurnEnd`.
    usage: Option<(u64, u64)>,
    /// Set once the harness sends `Bye` (or its stream closes) — the loop exits.
    done: bool,
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
            done: false,
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
            }
            ChatEvent::Compacted { removed } => {
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

    match code {
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            if !text.trim().is_empty() {
                app.conversation.push(format!("you: {text}"));
                send_input(stdin, &ChatInput::User { text })?;
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

/// Build the one-line status string from the `Ready` event + live usage.
/// Pure + tested.
fn status_line(app: &App) -> String {
    let base = match &app.status {
        Some(s) => format!(" {} · model {} · backend {} ", s.agent, s.model, s.backend),
        None => format!(
            " {} · backend {} · (connecting…) ",
            app.agent_id, app.backend
        ),
    };
    match app.usage {
        Some((p, c)) => format!("{base}· tokens {p}+{c} "),
        None => base,
    }
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

    // Pin the view to the tail: keep only the last `height` lines so the most
    // recent turn is always visible (no scrollbar in v1).
    let inner_height = area.height.saturating_sub(2) as usize; // borders
    let skip = lines.len().saturating_sub(inner_height.max(1));
    let visible: Vec<Line> = lines.into_iter().skip(skip).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" conversation ")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_argv_includes_chat_workdir_model_endpoint() {
        let argv = harness_argv(
            std::path::Path::new("/ws/agent-pi"),
            Some("gpt-5.5"),
            "https://api.openai.com/v1",
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
            ]
        );
    }

    #[test]
    fn harness_argv_omits_model_when_none() {
        let argv = harness_argv(std::path::Path::new("/ws/agent-pi"), None, DEFAULT_ENDPOINT);
        assert_eq!(
            argv,
            [
                "--chat",
                "--workdir",
                "/ws/agent-pi",
                "--endpoint",
                DEFAULT_ENDPOINT,
            ]
        );
        assert!(!argv.iter().any(|a| a == "--model"));
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
}
