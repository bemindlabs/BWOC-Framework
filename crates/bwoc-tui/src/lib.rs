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
//! Layout (one screen; arrows/PgUp/PgDn/End scroll the conversation):
//!   ┌ status ──────────────────────────────────────────┐
//!   ┌ conversation ────────────────────────────────────┐
//!   │ user + assistant turns (streamed tokens inline),  │
//!   │ → ToolCall, ✓/✗ ToolResult, ⚠ PermissionRequest   │
//!   │ all interleaved into one transcript               │
//!   └───────────────────────────────────────────────────┘
//!   ┌ input ───────────────────────────────────────────┐
//!   ↑/↓ scroll · PgUp/PgDn page · End live · select/copy · Ctrl-C exit
//!
//! Keys: Enter sends the input buffer as `ChatInput::User`; on a pending
//! permission request `a`/`d` allow/deny (only on an empty input line, once the
//! prompt has been up for a moment, so a prompt that appears mid-sentence never
//! consumes typed letters); Ctrl-C sends `Quit`, restores the terminal, and
//! exits. In a single session, `/` opens a command menu and `@` completes
//! project files (see the `complete` module). In the fleet, a line that
//! begins with `@<agent>` routes the rest of the message to that fleet member's
//! live session (opening its pane and switching to it).

mod complete;
mod markdown;
mod session;

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
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
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

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
    /// Project mode (bare `bwoc`): `Some` when `agent_path` is a plain working
    /// directory, not an incarnated agent. The runtime comes from here instead
    /// of a manifest, and `agent_id` is only a display name.
    pub project: Option<ProjectSession>,
}

/// One conversation in this directory, as [`SessionControl::list`] reports it.
pub struct SessionRow {
    pub id: String,
    pub title: String,
    pub messages: usize,
    /// Human-readable "last written" (e.g. `2m ago`).
    pub last: String,
    /// True for the conversation this TUI currently has open.
    pub current: bool,
}

/// Which conversation to open next.
pub enum SessionPick {
    New,
    /// An id or unique prefix.
    Id(String),
    /// Copy one (the open one when `None`) and open the copy.
    Fork(Option<String>),
}

/// Session management for the in-TUI `/session` commands, provided by the
/// caller. The TUI knows nothing about where conversations live — that belongs
/// to `bwoc-cli`, which owns the on-disk layout. Keeping it a trait is what lets
/// `bwoc-tui` compile-depend on `bwoc-core` alone (the dep quarantine).
pub trait SessionControl: Send {
    /// This directory's conversations, newest first.
    fn list(&self) -> Vec<SessionRow>;
    /// Resolve a pick to `(id, session file)`, creating or copying as needed.
    fn pick(&self, pick: &SessionPick) -> Result<(String, PathBuf), String>;
}

/// Environment facts the caller can answer for `/models`, `/backends`,
/// `/settings` and `/doctor`. Like [`SessionControl`], this is a trait so the
/// TUI keeps compile-depending on `bwoc-core` alone: probing Ollama, reading
/// `~/.bwoc/secrets.toml` and running `bwoc doctor` all belong to `bwoc-cli`.
///
/// Every method returns rows to print, not a model to interpret; a row is
/// `(label, value)` and is rendered as written.
pub trait EnvironmentInfo: Send {
    /// Models the active backend can enumerate, or why it cannot.
    fn models(&self) -> Result<Vec<String>, String>;
    /// One row per backend: whether it is usable here, and how.
    fn backends(&self) -> Vec<(String, String)>;
    /// The resolved runtime settings and where each value came from.
    fn settings(&self) -> Vec<(String, String)>;
    /// Health checks, as `bwoc doctor` reports them.
    fn doctor(&self) -> Result<Vec<(String, String)>, String>;
}

/// Runtime for a project session, resolved by the caller (`bwoc`).
pub struct ProjectSession {
    pub model: String,
    /// `None` lets the harness use the backend's default endpoint.
    pub endpoint: Option<String>,
    pub max_tokens: Option<u32>,
    /// Model context window override (`--max-context`). `None` lets the harness
    /// size compaction from the provider-reported or known window.
    pub max_context: Option<u32>,
    /// Where the harness persists the conversation. `None` keeps the harness
    /// default, `<workdir>/.bwoc/chat-session.json`.
    pub session_file: Option<PathBuf>,
    /// Answers `/models`, `/backends`, `/settings` and `/doctor`. `None` leaves
    /// those commands reporting that they are unavailable.
    pub environment: Option<Box<dyn EnvironmentInfo>>,
    /// Lets the in-TUI `/new`, `/session`, `/sessions` and `/fork` commands
    /// switch conversations. `None` (an agent session, or no home directory)
    /// leaves those commands reporting that they are unavailable.
    pub sessions: Option<Box<dyn SessionControl>>,
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
    let label = if args.project.is_some() {
        "bwoc"
    } else {
        "bwoc chat --tui"
    };
    let Some(harness) = bwoc_core::exec::sibling_binary("bwoc-harness") else {
        eprintln!(
            "{label}: bwoc-harness binary not found; install it \
             (`cargo install --path crates/bwoc-harness`) or add it to PATH."
        );
        return 2;
    };

    // Cloned before `args.project` is consumed below: the per-session argv is
    // rebuilt on every switch, but everything except the session file is fixed.
    let project_argv_base = args.project.as_ref().map(|p| ProjectRuntime {
        max_tokens: p.max_tokens,
        max_context: p.max_context,
    });

    // Project session: the caller already resolved the runtime. Agent session:
    // model + endpoint come from the agent's manifest (best-effort). A missing
    // manifest is not fatal — the harness falls back to its own defaults.
    let (model, endpoint) = match &args.project {
        Some(p) => (
            Some(p.model.clone()),
            p.endpoint
                .clone()
                .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string()),
        ),
        None => {
            let manifest =
                Manifest::load_from_path(&args.agent_path.join("config.manifest.json")).ok();
            (
                manifest.as_ref().map(|m| m.primary_model.clone()),
                manifest
                    .as_ref()
                    .and_then(|m| m.base_url.clone())
                    .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string()),
            )
        }
    };

    // The conversation this TUI has open. A `/session`, `/new` or `/fork`
    // swaps it and the harness is reopened on the new file, in the same
    // terminal — the alt screen is entered once, below, and left once.
    let mut session_file = args.project.as_ref().and_then(|p| p.session_file.clone());
    let mut project = args.project;
    let environment: Option<std::rc::Rc<dyn EnvironmentInfo>> = project
        .as_mut()
        .and_then(|p| p.environment.take())
        .map(std::rc::Rc::from);
    let sessions: Option<std::rc::Rc<dyn SessionControl>> =
        project.and_then(|p| p.sessions).map(std::rc::Rc::from);
    let mut session_id: Option<String> = None;

    let mut term = match setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("bwoc chat --tui: failed to enter alt screen: {e}");
            return 1;
        }
    };
    // Restore the terminal even if the loop below panics (#481).
    let mut terminal_guard = TerminalGuard::new();

    let result = loop {
        let mut argv = harness_argv(
            &args.agent_path,
            model.as_deref(),
            &endpoint,
            &args.backend_name,
            args.team_chat.as_deref(),
        );
        if let Some(p) = &project_argv_base {
            argv.extend(project_argv(&args.agent_id, p, session_file.as_deref()));
        }

        let mut child = match Command::new(&harness)
            .args(&argv)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(harness_stderr())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                break Err(io::Error::other(format!(
                    "failed to spawn bwoc-harness ({}): {e}",
                    harness.display()
                )));
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
                // Skip lines that aren't valid chat_proto events (the harness
                // prints a human banner before the stream begins).
                // Forward-compatible: unparseable lines are dropped, not fatal.
                if let Ok(ev) = serde_json::from_str::<ChatEvent>(line) {
                    if tx.send(ev).is_err() {
                        break; // UI hung up.
                    }
                }
            }
        });

        let stdin = child.stdin.take().expect("stdin piped above");
        let mut app = App::new(args.agent_id.clone(), &args.backend_name);
        app.workdir = Some(args.agent_path.clone());
        app.sessions = sessions.clone();
        app.environment = environment.clone();
        app.session_id = session_id.clone();
        let flow = event_loop(&mut term, &mut app, &rx, stdin);

        // Reap this harness before the next one starts (or before we leave).
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();

        match flow {
            Ok(Flow::Switch { id, file }) => {
                session_file = Some(file);
                session_id = Some(id);
            }
            Ok(_) => break Ok(()),
            Err(e) => break Err(e),
        }
    };

    // Explicit restore on the happy path; disarm the guard so it doesn't restore
    // a second time (the guard remains armed only for the panic path).
    if let Err(e) = restore_terminal() {
        eprintln!("bwoc chat --tui: warning — failed to restore terminal: {e}");
    }
    terminal_guard.disarm();

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

/// Extra harness argv for a project session: `--agent <name>` (display name in
/// the `Ready` status), optional `--max-tokens` / `--max-context`, and the
/// per-directory `--session-file` so the conversation is not written into the
/// repository.
fn project_argv(name: &str, p: &ProjectRuntime, session_file: Option<&Path>) -> Vec<String> {
    let mut argv = vec!["--agent".to_string(), name.to_string()];
    if let Some(n) = p.max_tokens {
        argv.push("--max-tokens".to_string());
        argv.push(n.to_string());
    }
    if let Some(n) = p.max_context {
        argv.push("--max-context".to_string());
        argv.push(n.to_string());
    }
    if let Some(file) = session_file {
        argv.push("--session-file".to_string());
        argv.push(file.to_string_lossy().into_owned());
    }
    argv
}

/// The part of a project session that stays fixed across a conversation switch
/// (the session file is the part that changes, so it is passed separately).
struct ProjectRuntime {
    max_tokens: Option<u32>,
    max_context: Option<u32>,
}

// --- app state ------------------------------------------------------------

/// What the event loop should do after a key press.
enum Flow {
    /// Keep the session running.
    Continue,
    /// Tear down: `Ctrl-C`, `/quit`.
    Quit,
    /// Restart the harness on another conversation (`/new`, `/session`,
    /// `/fork`), keeping the same terminal.
    Switch { id: String, file: PathBuf },
}

/// A pending permission request awaiting the operator's `a`/`d` decision.
struct Pending {
    id: String,
    tool: String,
    detail: String,
    shown_at: std::time::Instant,
}

/// How long a permission prompt must be on screen before `a`/`d` answer it.
/// A prompt can appear while the operator is typing; a letter already on its
/// way is typing, not consent.
const PERMISSION_KEY_GRACE: Duration = Duration::from_millis(500);

/// `Some(allow)` when `code` answers a pending permission prompt: `a` or `d` on
/// an empty input line, after [`PERMISSION_KEY_GRACE`]. Otherwise the key is
/// ordinary input.
fn permission_answer(code: KeyCode, input_empty: bool, shown_for: Duration) -> Option<bool> {
    if !input_empty || shown_for < PERMISSION_KEY_GRACE {
        return None;
    }
    match code {
        KeyCode::Char('a') => Some(true),
        KeyCode::Char('d') => Some(false),
        _ => None,
    }
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
    /// Reasoning text streamed for the current step. Shown dimmed while it
    /// arrives, then collapsed to one `∴` line when anything else follows.
    thinking: String,
    /// The current input buffer (one line).
    input: String,
    /// UTF-8 byte offset of the editing cursor in `input`. Kept on a character
    /// boundary by the input helpers below.
    input_cursor: usize,
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
    /// the view to the newest turn; arrows move one row, `PageUp`/`PageDown`
    /// move a page, and `End` returns to live.
    /// New content never yanks the view because the offset is bottom-relative.
    scroll: usize,
    /// Permission mode as last reported by the harness's `ModeChanged` — the
    /// harness is authoritative, so this mirrors whatever it sends (`default` |
    /// `accept_edits` | `bypass`, and `plan` if the harness enters it). `F2`
    /// cycles the first three; `plan` (only ever entered harness-side) falls
    /// back to `default` on the next `F2`. Shown in the status line.
    mode: String,
    /// Session working directory for `@` file mentions. `None` (fleet panes)
    /// disables `/` and `@` handling so a fleet `@agent` line routes as before.
    workdir: Option<PathBuf>,
    /// Files under `workdir`, listed on the first `@` and reused after.
    files: Option<Vec<String>>,
    /// Highlighted row in the `/` / `@` popup.
    popup_sel: usize,
    /// `Esc` hid the popup; it comes back once the input changes.
    popup_hidden: bool,
    /// A turn is running (between sending a message and its `TurnEnd`). `Esc`
    /// cancels it; between turns `Esc` is ordinary input.
    busy: bool,
    /// Session management for `/session`, `/sessions`, `/new` and `/fork`.
    /// `None` in a fleet pane or an agent session — those commands then say so.
    sessions: Option<std::rc::Rc<dyn SessionControl>>,
    /// The open conversation's id, shown in the status line when known.
    session_id: Option<String>,
    /// Answers `/models`, `/backends`, `/settings`, `/doctor`.
    environment: Option<std::rc::Rc<dyn EnvironmentInfo>>,
    /// Tool names the harness registered, from `Ready` — the agent's reach, for
    /// `/tools`.
    tools: Vec<String>,
    /// The last message sent, so `/retry` can send it again.
    last_sent: Option<String>,
    /// Completed turns this session (one per `TurnEnd`).
    turns: u32,
    /// Session cost in USD as the provider reported it (`None` = not reported —
    /// the status line then shows no cost rather than a made-up 0.00).
    cost_usd: Option<f64>,
}

/// The completion popup over the input line, derived from the input + cursor.
struct Popup {
    /// Byte range of the token a pick replaces.
    start: usize,
    /// `(completion, description)` rows.
    items: Vec<(String, String)>,
    /// `/` commands run on `Enter`; `@` files only complete.
    is_command: bool,
}

/// Rows shown in the popup at once.
const POPUP_ROWS: usize = 8;

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
            thinking: String::new(),
            input: String::new(),
            input_cursor: 0,
            pending: None,
            usage: None,
            total_out: 0,
            compactions: 0,
            done: false,
            scroll: 0,
            mode: "default".to_string(),
            workdir: None,
            files: None,
            popup_sel: 0,
            popup_hidden: false,
            busy: false,
            sessions: None,
            session_id: None,
            environment: None,
            tools: Vec::new(),
            last_sent: None,
            turns: 0,
            cost_usd: None,
        }
    }

    /// The `/` or `@` popup for the current input, if one applies.
    fn popup(&self) -> Option<Popup> {
        if self.workdir.is_none() || self.popup_hidden {
            return None;
        }
        if let Some(cmds) = complete::slash_matches(&self.input) {
            if self.input_cursor != self.input.len() || cmds.is_empty() {
                return None;
            }
            return Some(Popup {
                start: 0,
                items: cmds
                    .into_iter()
                    .map(|(n, d)| (n.to_string(), d.to_string()))
                    .collect(),
                is_command: true,
            });
        }
        let (start, query) = complete::mention_at(&self.input, self.input_cursor)?;
        let hits = complete::filter_files(self.files.as_deref()?, query, POPUP_ROWS);
        (!hits.is_empty()).then(|| Popup {
            start,
            items: hits
                .into_iter()
                .map(|f| (format!("@{f}"), String::new()))
                .collect(),
            is_command: false,
        })
    }

    /// Input changed: re-show a hidden popup, reset its selection, and list the
    /// workdir's files the first time an `@` token appears.
    fn input_changed(&mut self) {
        self.popup_hidden = false;
        self.popup_sel = 0;
        if self.files.is_none()
            && let Some(root) = &self.workdir
            && complete::mention_at(&self.input, self.input_cursor).is_some()
        {
            self.files = Some(complete::list_files(root));
        }
    }

    /// Replace the popup's token with its highlighted row.
    fn accept_popup(&mut self, popup: &Popup) {
        let Some((pick, _)) = popup.items.get(self.popup_sel.min(popup.items.len() - 1)) else {
            return;
        };
        let (line, cursor) = complete::complete(&self.input, popup.start, self.input_cursor, pick);
        self.input = line;
        self.input_cursor = cursor;
        self.popup_sel = 0;
        self.popup_hidden = true;
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

    fn input_left(&mut self) {
        if let Some(ch) = self.input[..self.input_cursor].chars().next_back() {
            self.input_cursor -= ch.len_utf8();
        }
    }

    fn input_right(&mut self) {
        if let Some(ch) = self.input[self.input_cursor..].chars().next() {
            self.input_cursor += ch.len_utf8();
        }
    }

    fn input_insert(&mut self, ch: char) {
        self.input.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    fn input_backspace(&mut self) {
        let end = self.input_cursor;
        self.input_left();
        if self.input_cursor < end {
            self.input.drain(self.input_cursor..end);
        }
    }

    fn take_input(&mut self) -> String {
        self.input_cursor = 0;
        std::mem::take(&mut self.input)
    }

    /// Apply a scroll key. Arrows move one row, `PageUp`/`PageDown` move by a
    /// fixed 10 rows (the offset is clamped in `draw_conversation`), and `End`
    /// returns to live. Returns true when the caller should stop processing.
    fn scroll_key(&mut self, code: KeyCode) -> bool {
        const PAGE: usize = 10;
        match code {
            KeyCode::Up | KeyCode::PageUp => {
                let amount = if code == KeyCode::Up { 1 } else { PAGE };
                self.scroll = self.scroll.saturating_add(amount);
                true
            }
            KeyCode::Down | KeyCode::PageDown => {
                let amount = if code == KeyCode::Down { 1 } else { PAGE };
                self.scroll = self.scroll.saturating_sub(amount);
                true
            }
            KeyCode::End => {
                self.scroll = 0;
                true
            }
            _ => false,
        }
    }

    /// Collapse streamed reasoning into one dimmed transcript line: its size and
    /// a whitespace-flattened preview.
    fn flush_thinking(&mut self) {
        const PREVIEW_CHARS: usize = 80;
        let text = std::mem::take(&mut self.thinking);
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.is_empty() {
            return;
        }
        let chars = flat.chars().count();
        let preview: String = flat.chars().take(PREVIEW_CHARS).collect();
        let more = if chars > PREVIEW_CHARS { "…" } else { "" };
        self.conversation
            .push(format!("∴ thinking ({chars} chars): {preview}{more}"));
    }

    /// Fold one harness event into the app state. Pure w.r.t. I/O — returns
    /// nothing; the loop redraws after applying. Factored so the event→state
    /// mapping is unit-testable without a terminal.
    fn apply(&mut self, ev: ChatEvent) {
        if !matches!(ev, ChatEvent::Thinking { .. }) {
            self.flush_thinking();
        }
        match ev {
            ChatEvent::Ready {
                agent,
                model,
                backend,
                tools,
            } => {
                self.tools = tools;
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
            ChatEvent::Thinking { text } => {
                self.thinking.push_str(&text);
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
                self.conversation.push(format!("→ {name}({args})  [{id}]"));
            }
            ChatEvent::ToolResult {
                id,
                name,
                ok,
                output,
            } => {
                let mark = if ok { "✓" } else { "✗" };
                self.conversation
                    .push(format!("{mark} {name}: {output}  [{id}]"));
            }
            ChatEvent::PermissionRequest { id, tool, detail } => {
                // Inline in the transcript, with the key affordance shown where
                // the operator is already reading (the input border echoes it too).
                self.conversation.push(format!(
                    "⚠ permission: {tool} — {detail}  [a]llow / [d]eny on an empty input line"
                ));
                self.pending = Some(Pending {
                    id,
                    tool,
                    detail,
                    shown_at: std::time::Instant::now(),
                });
            }
            ChatEvent::Diff {
                path,
                diff,
                truncated,
                ..
            } => {
                let more = if truncated { " (truncated)" } else { "" };
                self.conversation.push(format!("± {path}{more}"));
                self.conversation
                    .extend(diff.lines().map(|l| format!("±{l}")));
            }
            ChatEvent::ModelChanged { model } => {
                self.conversation.push(format!("● model: {model}"));
                if let Some(s) = &mut self.status {
                    s.model = model;
                }
            }
            ChatEvent::Reverted {
                undo,
                restored,
                conflicts,
                skipped,
            } => {
                let verb = if undo { "undid" } else { "redid" };
                if restored.is_empty() && conflicts.is_empty() && skipped.is_empty() {
                    let what = if undo { "to undo" } else { "to redo" };
                    self.conversation.push(format!("● nothing {what}"));
                } else {
                    self.conversation
                        .push(format!("● {verb} {} file(s)", restored.len()));
                    self.conversation
                        .extend(restored.iter().map(|p| format!("●   {p}")));
                }
                self.conversation.extend(
                    conflicts
                        .iter()
                        .map(|p| format!("✗ {p}: changed since — left alone")),
                );
                self.conversation.extend(
                    skipped
                        .iter()
                        .map(|p| format!("✗ {p}: not journalled (binary or too large)")),
                );
            }
            ChatEvent::Cancelled => {
                self.conversation.push("● cancelled".to_string());
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
                cost_usd,
            } => {
                self.busy = false;
                self.turns = self.turns.saturating_add(1);
                if cost_usd.is_some() {
                    self.cost_usd = cost_usd;
                }
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
                self.conversation.push(format!("✗ error: {message}"));
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

/// RAII guard: restores the terminal (leave raw mode + alt screen) on drop,
/// including while the stack **unwinds through a panic**. Without it, a panic in
/// the event loop stranded the operator's terminal in raw mode + alternate
/// screen (the explicit happy-path `restore_terminal()` never ran). BWOC uses
/// default unwind panics — no `panic = "abort"` — so destructors run and no
/// signal handler is needed (#481). Construct it right after `setup_terminal()`.
///
/// [`disarm`](TerminalGuard::disarm) it after a successful explicit restore so
/// the happy path restores exactly once (the guard is purely the panic-path net,
/// not a second restore relying on idempotency).
struct TerminalGuard {
    armed: bool,
}

impl TerminalGuard {
    fn new() -> Self {
        Self { armed: true }
    }

    /// Cancel the guard's restore — call after the explicit happy-path restore.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = restore_terminal();
        }
    }
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
) -> io::Result<Flow> {
    // Redraw only after state changes. Besides avoiding needless work, this
    // leaves an idle frame untouched so native terminal text selection remains
    // stable long enough to copy it.
    let mut dirty = true;
    loop {
        // Drain any harness events that arrived since the last poll.
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    app.apply(ev);
                    dirty = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Reader thread ended (child stdout closed). Draw the last
                    // state once before exiting.
                    app.done = true;
                    dirty = true;
                    break;
                }
            }
        }

        if dirty {
            term.draw(|f| draw_frame(f, app))?;
            dirty = false;
        }

        if app.done {
            // Best-effort polite quit; ignore a broken pipe (child already gone).
            let _ = send_input(&mut stdin, &ChatInput::Quit);
            return Ok(Flow::Quit);
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => match handle_key(app, &mut stdin, key)? {
                    Flow::Continue => dirty = true,
                    // Polite quit before we tear down the terminal (or restart
                    // the harness on another conversation).
                    flow => {
                        let _ = send_input(&mut stdin, &ChatInput::Quit);
                        return Ok(flow);
                    }
                },
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
    }
}

fn is_quit_key(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::Char('c' | 'C')) && modifiers.contains(KeyModifiers::CONTROL)
}

/// Process one key event, returning what the event loop should do next.
fn handle_key(app: &mut App, stdin: &mut ChildStdin, key: KeyEvent) -> io::Result<Flow> {
    let KeyEvent {
        code, modifiers, ..
    } = key;

    // Ctrl-C always quits, regardless of input/pending state.
    if is_quit_key(code, modifiers) {
        return Ok(Flow::Quit);
    }

    // A pending permission request captures a/d (see `permission_answer`);
    // anything else falls through so the user can keep typing while deciding.
    if let Some(p) = &app.pending
        && let Some(allow) = permission_answer(code, app.input.is_empty(), p.shown_at.elapsed())
    {
        let id = p.id.clone();
        let tool = p.tool.clone();
        app.pending = None;
        let mark = if allow { "✓ allowed" } else { "✗ denied" };
        app.conversation.push(format!("{mark} {tool}"));
        app.scroll = 0; // show the decision even if scrolled up
        send_input(stdin, &ChatInput::Permission { id, allow })?;
        return Ok(Flow::Continue);
    }

    // An open `/` / `@` popup takes the arrows, Tab, Enter and Esc.
    if let Some(popup) = app.popup() {
        match code {
            KeyCode::Up => {
                app.popup_sel = app
                    .popup_sel
                    .checked_sub(1)
                    .unwrap_or(popup.items.len() - 1);
                return Ok(Flow::Continue);
            }
            KeyCode::Down => {
                app.popup_sel = (app.popup_sel + 1) % popup.items.len();
                return Ok(Flow::Continue);
            }
            KeyCode::Tab => {
                app.accept_popup(&popup);
                return Ok(Flow::Continue);
            }
            KeyCode::Enter if !popup.is_command => {
                app.accept_popup(&popup);
                return Ok(Flow::Continue);
            }
            KeyCode::Enter => app.accept_popup(&popup), // then run it below
            KeyCode::Esc => {
                app.popup_hidden = true;
                return Ok(Flow::Continue);
            }
            _ => {}
        }
    }

    // Scrollback navigation (arrows/PageUp/PageDown/End) — before input editing.
    if app.scroll_key(code) {
        return Ok(Flow::Continue);
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
            Ok(Flow::Continue)
        }
        KeyCode::Enter => {
            let text = app.take_input();
            app.input_changed();
            if text.trim().is_empty() {
                return Ok(Flow::Continue);
            }
            app.scroll = 0; // jump to live so the reply is visible
            if app.workdir.is_some()
                && let Some(cmd) = complete::parse_slash(&text)
            {
                return run_slash(app, stdin, cmd);
            }
            app.conversation.push(format!("you: {text}"));
            app.busy = true;
            let text = match &app.workdir {
                Some(root) => {
                    let (expanded, report) = complete::expand_mentions(&text, root);
                    app.conversation.extend(report.iter().map(attach_line));
                    expanded
                }
                None => text,
            };
            // The local TUI is the trusted operator channel (Phase 5 t1).
            app.last_sent = Some(text.clone());
            send_input(
                stdin,
                &ChatInput::User {
                    text,
                    principal: Principal::LocalOperator,
                },
            )?;
            Ok(Flow::Continue)
        }
        KeyCode::Backspace => {
            app.input_backspace();
            app.input_changed();
            Ok(Flow::Continue)
        }
        KeyCode::Left => {
            app.input_left();
            app.input_changed();
            Ok(Flow::Continue)
        }
        KeyCode::Right => {
            app.input_right();
            app.input_changed();
            Ok(Flow::Continue)
        }
        KeyCode::Char(c) => {
            app.input_insert(c);
            app.input_changed();
            Ok(Flow::Continue)
        }
        // Esc cancels the turn in flight; between turns it stays inert (it must
        // never close the session — #531).
        KeyCode::Esc => {
            if app.busy {
                app.conversation.push("● cancelling…".to_string());
                send_input(stdin, &ChatInput::Cancel)?;
            }
            Ok(Flow::Continue)
        }
        _ => Ok(Flow::Continue),
    }
}

/// Run a `/` command locally. Session commands ask the event loop to restart
/// the harness; the rest map onto an existing `ChatInput`, and nothing reaches
/// the model.
fn run_slash(app: &mut App, stdin: &mut ChildStdin, cmd: complete::Slash) -> io::Result<Flow> {
    use complete::Slash;
    match cmd {
        Slash::Help => {
            app.conversation.push("● commands:".to_string());
            app.conversation.extend(
                complete::COMMANDS
                    .iter()
                    .map(|(name, desc)| format!("●   {name:<7} {desc}")),
            );
            app.conversation.push(
                "●   @path   attach a project file to your message (Tab completes)".to_string(),
            );
        }
        Slash::Clear => {
            send_input(stdin, &ChatInput::Forget)?;
            app.conversation.clear();
            app.streaming.clear();
            app.thinking.clear();
            app.usage = None;
            app.conversation
                .push("● conversation forgotten — starting fresh".to_string());
        }
        Slash::Mode(None) => {
            app.conversation.push(format!(
                "● permission mode: {} — /mode {} (or F2)",
                app.mode,
                complete::MODES.join("|")
            ));
        }
        Slash::Mode(Some(m)) if complete::MODES.contains(&m.as_str()) => {
            app.mode = m.clone();
            send_input(stdin, &ChatInput::SetMode { mode: m })?;
        }
        Slash::Mode(Some(m)) => {
            app.conversation.push(format!(
                "✗ unknown mode `{m}` — one of {}",
                complete::MODES.join(", ")
            ));
        }
        Slash::Model(None) => {
            let current = app
                .status
                .as_ref()
                .map_or("(unknown)", |s| s.model.as_str());
            app.conversation
                .push(format!("● model: {current} — /model <name> switches it"));
        }
        Slash::Model(Some(model)) => {
            send_input(stdin, &ChatInput::SetModel { model })?;
        }
        Slash::Status => {
            for line in status_report(app) {
                app.conversation.push(line);
            }
        }
        Slash::Models => match app.environment.as_ref() {
            Some(env) => match env.models() {
                Ok(models) if models.is_empty() => app
                    .conversation
                    .push("✗ the backend listed no models".to_string()),
                Ok(models) => {
                    let current = app.status.as_ref().map(|s| s.model.clone());
                    app.conversation.push(format!("● {} models:", models.len()));
                    for m in models {
                        let mark = if Some(&m) == current.as_ref() {
                            "▸"
                        } else {
                            " "
                        };
                        app.conversation.push(format!("●  {mark} {m}"));
                    }
                    app.conversation
                        .push("●   /model <name> switches".to_string());
                }
                Err(why) => app.conversation.push(format!("✗ {why}")),
            },
            None => app.conversation.push(NO_ENVIRONMENT.to_string()),
        },
        Slash::Backends => report_rows(app, "backends", |env| Ok(env.backends())),
        Slash::Settings => report_rows(app, "runtime", |env| Ok(env.settings())),
        Slash::Doctor => report_rows(app, "checks", |env| env.doctor()),
        Slash::Tools => {
            if app.tools.is_empty() {
                app.conversation
                    .push("✗ the harness reported no tools for this session".to_string());
            } else {
                app.conversation
                    .push(format!("● {} tools:", app.tools.len()));
                // Three per row: the list is long and mostly short names.
                for row in app.tools.chunks(3) {
                    app.conversation.push(format!("●   {}", row.join("  ")));
                }
            }
        }
        Slash::Cost => {
            for line in cost_report(app) {
                app.conversation.push(line);
            }
        }
        Slash::Retry => {
            if app.busy {
                app.conversation
                    .push("✗ a turn is running — press Esc to cancel it first".to_string());
            } else if let Some(text) = app.last_sent.clone() {
                app.conversation.push(format!("you: {text}"));
                app.busy = true;
                send_input(
                    stdin,
                    &ChatInput::User {
                        text,
                        principal: Principal::LocalOperator,
                    },
                )?;
            } else {
                app.conversation
                    .push("✗ nothing sent yet in this conversation".to_string());
            }
        }
        Slash::Save(path) => {
            let line = save_transcript(app, path.as_deref());
            app.conversation.push(line);
        }
        Slash::Undo => send_input(stdin, &ChatInput::Undo)?,
        Slash::Redo => send_input(stdin, &ChatInput::Redo)?,
        Slash::Sessions => match app.sessions.as_ref() {
            Some(ctl) => {
                app.conversation.push("● conversations here:".to_string());
                for row in ctl.list() {
                    let mark = if row.current { "▸" } else { " " };
                    app.conversation.push(format!(
                        "●  {mark} {}  {:>3} msgs  {:<8}  {}",
                        row.id,
                        row.messages,
                        row.last,
                        shorten(&row.title, 48)
                    ));
                }
                app.conversation
                    .push("●   /session <id> switches · /new · /fork".to_string());
            }
            None => app.conversation.push(UNAVAILABLE.to_string()),
        },
        Slash::NewSession => return switch_session(app, &complete::SessionArg::New),
        Slash::Session(Some(id)) => {
            return switch_session(app, &complete::SessionArg::Id(id));
        }
        Slash::Session(None) => {
            app.conversation
                .push("✗ /session <id> needs an id — /sessions lists them".to_string());
        }
        Slash::Fork(id) => return switch_session(app, &complete::SessionArg::Fork(id)),
        Slash::Quit => return Ok(Flow::Quit),
        Slash::Unknown(name) => {
            app.conversation
                .push(format!("✗ unknown command /{name} — /help lists them"));
        }
    }
    Ok(Flow::Continue)
}

/// What `/models`, `/backends`, `/settings` and `/doctor` say when the caller
/// provided no [`EnvironmentInfo`] (an agent session, or a frontend that only
/// wired the chat).
const NO_ENVIRONMENT: &str = "✗ this session has no environment information";

/// Print one of the row-shaped environment reports, or why it is unavailable.
/// A check that fails is the report, not an error — the session stays alive.
fn report_rows(
    app: &mut App,
    title: &str,
    rows: impl Fn(&dyn EnvironmentInfo) -> Result<Vec<(String, String)>, String>,
) {
    let Some(env) = app.environment.clone() else {
        app.conversation.push(NO_ENVIRONMENT.to_string());
        return;
    };
    match rows(env.as_ref()) {
        Ok(rows) if rows.is_empty() => app.conversation.push(format!("● no {title} to report")),
        Ok(rows) => {
            app.conversation.push(format!("● {title}:"));
            let width = rows
                .iter()
                .map(|(k, _)| k.chars().count())
                .max()
                .unwrap_or(0);
            for (key, value) in rows {
                let pad = " ".repeat(width - key.chars().count());
                app.conversation.push(format!("●   {key}{pad}  {value}"));
            }
        }
        Err(why) => app.conversation.push(format!("✗ {why}")),
    }
}

/// `/status`: what this session is, in one place — the status line is one row
/// and elides on a narrow terminal, and `--json` is not available from inside a
/// TUI. Pure, so the wording is testable.
fn status_report(app: &App) -> Vec<String> {
    let mut out = vec!["● session:".to_string()];
    let (agent, model, backend) = match &app.status {
        Some(s) => (s.agent.as_str(), s.model.as_str(), s.backend.as_str()),
        None => (app.agent_id.as_str(), "(not ready)", app.backend.as_str()),
    };
    out.push(format!("●   agent     {agent}"));
    out.push(format!("●   model     {model}  ({backend})"));
    if let Some(dir) = &app.workdir {
        out.push(format!("●   directory {}", dir.display()));
    }
    if let Some(id) = &app.session_id {
        out.push(format!("●   conversation {id}"));
    }
    out.push(format!("●   mode      {} (F2)", app.mode));
    out.push(format!(
        "●   turns     {} · {} tool(s) available",
        app.turns,
        app.tools.len()
    ));
    if app.compactions > 0 {
        out.push(format!("●   compacted {}×", app.compactions));
    }
    out.extend(cost_report(app).into_iter().skip(1));
    out
}

/// `/cost`: token usage, and a price only when a provider reported one.
fn cost_report(app: &App) -> Vec<String> {
    let mut out = vec!["● usage:".to_string()];
    match app.usage {
        Some((prompt, _)) => {
            out.push(format!(
                "●   context   {} tokens (last turn's prompt)",
                fmt_tokens(prompt)
            ));
            out.push(format!(
                "●   output    {} tokens this session",
                fmt_tokens(app.total_out)
            ));
        }
        None => out.push("●   no usage reported yet".to_string()),
    }
    out.push(match app.cost_usd {
        Some(c) => format!("●   cost      ${c:.4} (provider-reported)"),
        None => "●   cost      not reported by this provider".to_string(),
    });
    out
}

/// `/save`: write the transcript as Markdown. Returns the transcript line to
/// show — the path written, or why it could not be.
fn save_transcript(app: &App, path: Option<&str>) -> String {
    let default = format!(
        "bwoc-transcript-{}.md",
        app.session_id.as_deref().unwrap_or("session")
    );
    let name = path.unwrap_or(&default);
    let target = match app.workdir.as_ref() {
        Some(dir) if Path::new(name).is_relative() => dir.join(name),
        _ => PathBuf::from(name),
    };
    let mut body = String::new();
    for line in &app.conversation {
        body.push_str(line);
        body.push('\n');
    }
    match std::fs::write(&target, body) {
        Ok(()) => format!(
            "● saved {} line(s) to {}",
            app.conversation.len(),
            target.display()
        ),
        Err(e) => format!("✗ could not write {}: {e}", target.display()),
    }
}

/// Why the session commands are unavailable: an agent session has no
/// conversation store, and a project session without a home directory keeps no
/// history to switch between.
const UNAVAILABLE: &str =
    "✗ switching conversations needs a project session with a bwoc home directory";

/// Truncate a title to `max` characters, with an ellipsis when cut.
fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Resolve a session pick through the caller's [`SessionControl`] and ask the
/// event loop to reopen the harness on it. A failure is reported in the
/// transcript and the current conversation continues — switching must never
/// lose the session you are in.
fn switch_session(app: &mut App, arg: &complete::SessionArg) -> io::Result<Flow> {
    // Switching restarts the harness, which would drop a turn mid-flight.
    if app.busy {
        app.conversation
            .push("✗ a turn is running — press Esc to cancel it first".to_string());
        return Ok(Flow::Continue);
    }
    let Some(ctl) = app.sessions.as_ref() else {
        app.conversation.push(UNAVAILABLE.to_string());
        return Ok(Flow::Continue);
    };
    let pick = match arg {
        complete::SessionArg::New => SessionPick::New,
        complete::SessionArg::Id(id) => SessionPick::Id(id.clone()),
        complete::SessionArg::Fork(id) => SessionPick::Fork(id.clone()),
    };
    match ctl.pick(&pick) {
        Ok((id, file)) => Ok(Flow::Switch { id, file }),
        Err(e) => {
            app.conversation.push(format!("✗ {e}"));
            Ok(Flow::Continue)
        }
    }
}

/// Transcript line for one `@` mention resolved at send time.
fn attach_line(a: &complete::Attached) -> String {
    match a {
        complete::Attached::File {
            path,
            bytes,
            truncated,
        } => {
            let cut = if *truncated { ", truncated" } else { "" };
            format!("● attached {path} ({bytes} bytes{cut})")
        }
        complete::Attached::Skipped { path, reason } => {
            format!("✗ not attached {path}: {reason}")
        }
    }
}

// --- drawing --------------------------------------------------------------

fn draw_frame(f: &mut ratatui::Frame, app: &App) {
    let area = f.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status
            Constraint::Min(0),    // body (full-width transcript)
            Constraint::Length(3), // input box
            Constraint::Length(1), // key footer
        ])
        .split(area);

    draw_status(f, layout[0], app);
    draw_body(f, layout[1], app);
    draw_input(f, layout[2], app, true);
    draw_footer(f, layout[3], app.workdir.is_some());
    if let Some(popup) = app.popup() {
        draw_popup(f, layout[1], &popup, app.popup_sel);
    }
}

/// The `/` / `@` popup, overlaid on the bottom of the conversation pane just
/// above the input box.
fn draw_popup(f: &mut ratatui::Frame, body: Rect, popup: &Popup, sel: usize) {
    let rows = popup.items.len().min(POPUP_ROWS) as u16;
    // Inside the pane's border: one row/column in from each edge.
    let height = (rows + 2).min(body.height.saturating_sub(2));
    let width = body.width.saturating_sub(2).min(72);
    if height < 3 || width < 10 {
        return;
    }
    let area = Rect {
        x: body.x + 1,
        y: body.y + body.height - 1 - height,
        width,
        height,
    };
    let sel = sel.min(popup.items.len() - 1);
    let items: Vec<ListItem> = popup
        .items
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let style = if i == sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(tone(design::color::ACCENT))
            } else {
                Style::default()
            };
            let line = if desc.is_empty() {
                Line::from(Span::raw(name.clone()))
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("{name:<8}"),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!(" {desc}")),
                ])
            };
            ListItem::new(line).style(style)
        })
        .collect();
    let title = if popup.is_command {
        " commands — ↑/↓ · Tab complete · Enter run · Esc "
    } else {
        " files — ↑/↓ · Tab/Enter complete · Esc "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(tone(design::color::ACCENT)));
    f.render_widget(Clear, area);
    f.render_widget(List::new(items).block(block), area);
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

/// Compact a token count: `512`, `9.1k`, `1.2M`. Keeps the header narrow with a
/// single truncated decimal above 1k / 1M (a trailing `.0` is dropped).
///
/// Integer math, **truncating** — never rounds up across a unit boundary, so a
/// value just under the next unit (e.g. `999_950`) renders `999.9k`, never the
/// wider/inconsistent `1000k`.
fn fmt_tokens(n: u64) -> String {
    let unit = |value: u64, div: u64, suffix: char| {
        let whole = value / div;
        let tenths = (value % div) / (div / 10);
        if tenths == 0 {
            format!("{whole}{suffix}")
        } else {
            format!("{whole}.{tenths}{suffix}")
        }
    };
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        unit(n, 1_000, 'k')
    } else {
        unit(n, 1_000_000, 'M')
    }
}

/// Parse a leading `@mention` from a fleet input line. Returns the mentioned
/// name (without the `@`; the `agent-` prefix may or may not be present) and the
/// trimmed remaining message. `None` when the line doesn't begin with a
/// non-empty `@token` (so `"email @ me"` and a bare `"@"` route normally).
fn parse_mention(input: &str) -> Option<(&str, &str)> {
    let rest = input.trim_start().strip_prefix('@')?;
    let (name, msg) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    (!name.is_empty()).then_some((name, msg))
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
    let cost = match app.cost_usd {
        // Provider-reported only; 4 decimals because a turn is often sub-cent.
        Some(c) => format!("· ${c:.4} "),
        None => String::new(),
    };
    let compacted = if app.compactions > 0 {
        format!("· ⟳{} ", app.compactions)
    } else {
        String::new()
    };
    format!("{usage}{cost}{compacted}· mode {} (F2) ", app.mode)
}

fn draw_body(f: &mut ratatui::Frame, area: Rect, app: &App) {
    // Single full-width transcript: tool calls, results, permission prompts, and
    // errors are interleaved into `conversation` (see `App::apply`), so there is
    // no longer a separate tools/activity pane to split off.
    draw_conversation(f, area, app);
}

fn draw_conversation(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = app
        .conversation
        .iter()
        .flat_map(|l| entry_lines(l, transcript_style(l)))
        .collect();
    // Reasoning still arriving: one dimmed progress line (collapsed later).
    if app.streaming.is_empty() && !app.thinking.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("∴ thinking… ({} chars)", app.thinking.chars().count()),
            Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
        )));
    }
    // Show the in-flight streamed turn live, below the committed history.
    if !app.streaming.is_empty() {
        lines.extend(entry_lines(
            &format!("assistant: {}", app.streaming),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    // The view is anchored to the tail; `app.scroll` (lines up from the bottom)
    // lets the operator page back through history. `skip` drops that many lines
    // from the front, scaled up by the scroll offset; both are clamped, so the
    // window can never run past either end.
    let inner_height = area.height.saturating_sub(2) as usize; // borders
    // Window over the rows actually drawn, not transcript lines: one long tool
    // result wraps to several rows, and counting it as one pushed the newest
    // rows (the answer) below the box.
    let lines = hard_wrap(lines, area.width.saturating_sub(2) as usize);
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
    let p = Paragraph::new(visible).block(block);
    f.render_widget(p, area);
}

/// Hard-wrap display lines to `width` terminal columns (wide characters count
/// double), keeping each span's style, so every returned line is one screen row.
/// A glyph wider than the whole pane gets a row to itself; the unwrapped
/// paragraph clips it rather than spilling onto another row.
fn hard_wrap(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(1);
    let mut rows = Vec::with_capacity(lines.len());
    for line in lines {
        let line_style = line.style;
        let mut row: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;
        for span in line.spans {
            let style = span.style;
            let mut buf = String::new();
            for ch in span.content.chars() {
                let w = ch.width().unwrap_or(0);
                if used > 0 && used + w > width {
                    if !buf.is_empty() {
                        row.push(Span::styled(std::mem::take(&mut buf), style));
                    }
                    rows.push(Line::from(std::mem::take(&mut row)).style(line_style));
                    used = 0;
                }
                buf.push(ch);
                used += w;
            }
            if !buf.is_empty() {
                row.push(Span::styled(buf, style));
            }
        }
        rows.push(Line::from(row).style(line_style));
    }
    rows
}

/// A transcript entry as display lines. An assistant turn is Markdown, so it is
/// rendered (headings, bullets, quotes, code, emphasis); everything else — user
/// text, tool traffic, diffs, notices — is shown verbatim, since a tool result
/// that happens to contain `*` is not prose.
fn entry_lines(text: &str, style: Style) -> Vec<Line<'static>> {
    let Some(body) = text.strip_prefix("assistant: ") else {
        return styled_lines(text, style);
    };
    let code = style.fg(tone(design::color::ACCENT));
    let mut lines = markdown::render(body, style, code);
    // Keep the speaker label on the first row.
    if let Some(first) = lines.first_mut() {
        first.spans.insert(0, Span::styled("assistant: ", style));
    }
    lines
}

/// One transcript entry as display lines: split on `\n` (a trailing `\r` is
/// dropped) so multi-line assistant text and tool output keep their line
/// breaks. Every line takes the entry's style.
fn styled_lines(text: &str, style: Style) -> Vec<Line<'static>> {
    text.split('\n')
        .map(|part| {
            Line::from(Span::styled(
                part.strip_suffix('\r').unwrap_or(part).to_string(),
                style,
            ))
        })
        .collect()
}

/// Per-line style for the transcript: color the inline tool-action markers
/// (⚠ permission, ✗ error/denied, ✓ result/allowed, → tool call) so they stand
/// out from plain user/assistant turns now that they share one column.
fn transcript_style(line: &str) -> Style {
    if let Some(body) = line.strip_prefix('±') {
        // A diff row: colour additions and removals, dim the rest.
        return match body.chars().next() {
            Some('+') => Style::default().fg(tone(design::color::SUCCESS)),
            Some('-') => Style::default().fg(tone(design::color::DANGER)),
            _ => Style::default().add_modifier(Modifier::DIM),
        };
    }
    if line.starts_with('⚠') {
        Style::default()
            .fg(tone(design::color::WARNING))
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with('✗') {
        Style::default().fg(tone(design::color::DANGER))
    } else if line.starts_with('✓') {
        Style::default().fg(tone(design::color::SUCCESS))
    } else if line.starts_with('→')
        || line.starts_with('●')
        || line.starts_with('📢')
        || line.starts_with('∴')
    {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default()
    }
}

fn draw_input(f: &mut ratatui::Frame, area: Rect, app: &App, cursor_visible: bool) {
    let (title, border) = match &app.pending {
        Some(p) => (
            if app.input.is_empty() {
                format!(" permission: {} ({}) — [a]llow / [d]eny ", p.tool, p.detail)
            } else {
                format!(
                    " permission: {} ({}) — clear the input, then [a]llow / [d]eny ",
                    p.tool, p.detail
                )
            },
            Style::default()
                .fg(tone(design::color::WARNING))
                .add_modifier(Modifier::BOLD),
        ),
        None => (
            " input — Enter send ".to_string(),
            Style::default().fg(tone(design::color::ACCENT)),
        ),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border);
    use unicode_width::UnicodeWidthStr;
    let cursor_col = app.input[..app.input_cursor].width();
    let content_cursor = 2usize.saturating_add(cursor_col); // `> ` prefix
    let inner_width = area.width.saturating_sub(2) as usize;
    let horizontal_scroll = content_cursor
        .saturating_sub(inner_width.saturating_sub(1))
        .min(u16::MAX as usize);
    let p = Paragraph::new(Line::from(format!("> {}", app.input)))
        .block(block)
        .scroll((0, horizontal_scroll as u16));
    f.render_widget(p, area);
    if cursor_visible && area.width > 2 && area.height > 2 {
        let visible_cursor = content_cursor
            .saturating_sub(horizontal_scroll)
            .min(inner_width.saturating_sub(1));
        f.set_cursor_position((area.x + 1 + visible_cursor as u16, area.y + 1));
    }
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, completions: bool) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ↑/↓ ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("scroll · ←/→ cursor · PgUp/PgDn · End live · "),
        Span::raw(if completions {
            "/ commands · @ files · "
        } else {
            ""
        }),
        Span::raw("select/copy · "),
        Span::styled(
            "Ctrl-C exit ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]))
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(footer, area);
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

    /// Resolve a `@mention` name to a fleet-member index, tolerating the
    /// `agent-` prefix on either side (so `@busaba` and `@agent-busaba` both
    /// match `agent-busaba`). The id body is matched case-sensitively.
    fn resolve_agent(&self, name: &str) -> Option<usize> {
        let want = name.strip_prefix("agent-").unwrap_or(name);
        self.agents
            .iter()
            .position(|a| a.id.strip_prefix("agent-").unwrap_or(&a.id) == want)
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
                    p.conversation
                        .push(format!("✗ failed to start session: {e}"));
                }
            }
        }
    }

    /// Fold every live session's pending events into its pane. Collect each
    /// session's currently-available events first (immutable borrow), then apply
    /// them to the pane (mutable) — two phases so `sessions` and `panes` never
    /// alias, and no manual `try_recv` loop for clippy to grumble about.
    fn drain(&mut self) -> bool {
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        let mut dead = Vec::new();
        let mut changed = false;
        for id in ids {
            let events: Vec<ChatEvent> = match self.sessions.get(&id) {
                Some(s) => s.rx.try_iter().collect(),
                None => continue,
            };
            changed |= !events.is_empty();
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
                changed = true;
            }
        }
        for id in dead {
            self.sessions.remove(&id);
        }
        changed
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
    // Restore the terminal even if the event loop below panics (#481).
    let mut terminal_guard = TerminalGuard::new();

    let mut fleet = Fleet::new(agents, cfg);
    let result = fleet_event_loop(&mut term, &mut fleet);
    if let Err(e) = restore_terminal() {
        eprintln!("bwoc chat --tui: warning — failed to restore terminal: {e}");
    }
    terminal_guard.disarm();
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
    let mut dirty = true;
    loop {
        dirty |= fleet.drain();
        if dirty {
            term.draw(|f| draw_fleet(f, fleet))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if fleet_handle_key(fleet, key)? {
                        return Ok(());
                    }
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
    }
}

/// Deliver `text` to `id`'s own pane + live session (the normal path). Empty
/// text is a no-op. Echoes `you: …` and pins the view to live.
fn fleet_send_local(fleet: &mut Fleet, id: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if let Some(p) = fleet.panes.get_mut(id) {
        p.conversation.push(format!("you: {text}"));
        p.scroll = 0;
    }
    if let Some(s) = fleet.sessions.get_mut(id) {
        s.send(&ChatInput::User {
            text: text.to_string(),
            principal: Principal::LocalOperator,
        });
    }
}

/// Route a `@mention` message from `from`'s pane to fleet member `idx`
/// (`target`): note the handoff in the sender's pane, switch to the target,
/// ensure its session is live, and deliver `msg` there. An empty `msg` just
/// jumps to the target pane. A target the harness can't drive (no session after
/// `open`) reports the failure instead of silently dropping the message.
fn fleet_route_to(fleet: &mut Fleet, from: &str, idx: usize, target: &str, msg: &str) {
    if let Some(p) = fleet.panes.get_mut(from) {
        p.conversation.push(format!("→ routed to {target}"));
        p.scroll = 0;
    }
    fleet.selected = idx;
    fleet.open(target);
    // Pin the target to live so the routed message — or a failure note below —
    // is visible even if the user had scrolled up in that pane earlier.
    if let Some(p) = fleet.panes.get_mut(target) {
        p.scroll = 0;
    }
    if fleet.sessions.contains_key(target) {
        fleet_send_local(fleet, target, msg);
    } else if !msg.trim().is_empty() {
        if let Some(p) = fleet.panes.get_mut(target) {
            p.conversation
                .push(format!("✗ can't route here: {target} has no live session"));
        }
    }
}

/// Returns `Ok(true)` on quit. Routes input to the **active** pane's session.
fn fleet_handle_key(fleet: &mut Fleet, key: KeyEvent) -> io::Result<bool> {
    let KeyEvent {
        code, modifiers, ..
    } = key;
    if is_quit_key(code, modifiers) {
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

    // A pending approval on the active pane captures a/d (see `permission_answer`).
    let answer = fleet.panes.get(&id).and_then(|p| {
        let pd = p.pending.as_ref()?;
        let allow = permission_answer(code, p.input.is_empty(), pd.shown_at.elapsed())?;
        Some((pd.id.clone(), pd.tool.clone(), allow))
    });
    if let Some((pid, tool, allow)) = answer {
        if let Some(s) = fleet.sessions.get_mut(&id) {
            s.send(&ChatInput::Permission { id: pid, allow });
        }
        if let Some(p) = fleet.panes.get_mut(&id) {
            p.pending = None;
            // Name the tool so the decision is unambiguous in the shared
            // transcript (matches the single-agent handler).
            p.conversation.push(format!(
                "{} {tool}",
                if allow { "✓ allowed" } else { "✗ denied" }
            ));
            p.scroll = 0;
        }
        return Ok(false);
    }

    // Scrollback on the active pane (arrows/PageUp/PageDown/End).
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
                .map(App::take_input)
                .unwrap_or_default();
            if text.trim().is_empty() {
                return Ok(false);
            }
            // A leading `@agent` naming *another* fleet member routes there. A
            // self-mention or an unresolved name is not a route — fall through
            // and send the line verbatim to this pane (nothing swallowed, so a
            // bare `@self` isn't silently dropped).
            if let Some((name, msg)) = parse_mention(&text) {
                if let Some(idx) = fleet.resolve_agent(name) {
                    let target = fleet.agents[idx].id.clone();
                    if target != id {
                        fleet_route_to(fleet, &id, idx, &target, msg);
                        return Ok(false);
                    }
                }
            }
            fleet_send_local(fleet, &id, &text);
            Ok(false)
        }
        KeyCode::Backspace => {
            if let Some(p) = fleet.panes.get_mut(&id) {
                p.input_backspace();
            }
            Ok(false)
        }
        KeyCode::Left => {
            if let Some(p) = fleet.panes.get_mut(&id) {
                p.input_left();
            }
            Ok(false)
        }
        KeyCode::Right => {
            if let Some(p) = fleet.panes.get_mut(&id) {
                p.input_right();
            }
            Ok(false)
        }
        KeyCode::Char(c) => {
            if let Some(p) = fleet.panes.get_mut(&id) {
                p.input_insert(c);
            }
            Ok(false)
        }
        KeyCode::Esc => Ok(false),
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
            Constraint::Length(1),
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
            draw_input(f, rows[2], app, fleet.palette.is_none());
        }
    }
    draw_footer(f, rows[3], false);
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

/// Stderr for a TUI-spawned harness. Inherited stderr on a terminal paints
/// over the alternate screen (sandbox warnings land mid-conversation), so it is
/// appended to `~/.bwoc/logs/tui-harness.log` instead. A redirected stderr is
/// kept, so a shell capture still sees a crashed session.
pub(crate) fn harness_stderr() -> Stdio {
    use std::io::IsTerminal as _;
    if !io::stderr().is_terminal() {
        return Stdio::inherit();
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    harness_log_path(home.map(PathBuf::from))
        .and_then(|path| {
            std::fs::create_dir_all(path.parent()?).ok()?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
        })
        .map(Stdio::from)
        .unwrap_or_else(Stdio::inherit)
}

fn harness_log_path(home: Option<PathBuf>) -> Option<PathBuf> {
    home.map(|h| h.join(".bwoc").join("logs").join("tui-harness.log"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn project_argv_carries_name_max_tokens_and_session_file() {
        let p = super::ProjectRuntime {
            max_tokens: Some(4096),
            max_context: Some(32768),
        };
        let file = std::path::PathBuf::from("/h/.bwoc/sessions/dir/abc.json");
        assert_eq!(
            super::project_argv("repo", &p, Some(&file)),
            [
                "--agent",
                "repo",
                "--max-tokens",
                "4096",
                "--max-context",
                "32768",
                "--session-file",
                "/h/.bwoc/sessions/dir/abc.json"
            ]
        );
        let bare = super::ProjectRuntime {
            max_tokens: None,
            max_context: None,
        };
        assert_eq!(
            super::project_argv("repo", &bare, None),
            ["--agent", "repo"]
        );
    }

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
    fn thinking_streams_then_collapses_to_one_dim_line() {
        let mut app = App::new("a".into(), "anthropic");
        app.apply(ChatEvent::Thinking {
            text: "Let me\nthink ".into(),
        });
        app.apply(ChatEvent::Thinking {
            text: "about it.".into(),
        });
        assert_eq!(app.thinking, "Let me\nthink about it.");
        // The first non-thinking event collapses it.
        app.apply(ChatEvent::Token {
            text: "Answer".into(),
        });
        assert!(app.thinking.is_empty());
        let collapsed: Vec<_> = app
            .conversation
            .iter()
            .filter(|l| l.starts_with('∴'))
            .collect();
        assert_eq!(collapsed, ["∴ thinking (22 chars): Let me think about it."]);
        assert_eq!(app.streaming, "Answer");
        assert_eq!(
            transcript_style(collapsed[0]),
            Style::default().add_modifier(Modifier::DIM)
        );
    }

    #[test]
    fn styled_lines_keep_line_breaks() {
        let style = transcript_style("✓ list_dir");
        let lines = styled_lines("✓ list_dir: .git/\nAGENTS.md\r\nnotes.txt", style);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.spans[0].content.to_string())
            .collect();
        assert_eq!(text, ["✓ list_dir: .git/", "AGENTS.md", "notes.txt"]);
        assert!(lines.iter().all(|l| l.spans[0].style == style));
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
            cost_usd: None,
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
            cost_usd: None,
        });
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 1_500, // grew as history was resent
            completion_tokens: 300,
            cost_usd: None,
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
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1_000), "1k");
        assert_eq!(fmt_tokens(9_100), "9.1k");
        assert_eq!(fmt_tokens(1_200_000), "1.2M");
        assert_eq!(fmt_tokens(2_000_000), "2M");
        // Truncation never rounds up across a unit boundary → never "1000k".
        assert_eq!(fmt_tokens(999_950), "999.9k");
        assert_eq!(fmt_tokens(999_999), "999.9k");
    }

    #[test]
    fn permission_request_sets_pending_and_shows_inline_affordance() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::PermissionRequest {
            id: "p1".into(),
            tool: "run_command".into(),
            detail: "rm -rf build/".into(),
        });
        assert!(app.pending.is_some());
        assert_eq!(app.pending.as_ref().unwrap().id, "p1");
        // The prompt is inline in the transcript with the a/d affordance shown.
        let line = app
            .conversation
            .iter()
            .find(|l| l.starts_with('⚠'))
            .expect("inline permission line");
        assert!(line.contains("[a]llow / [d]eny"), "got: {line}");
    }

    #[test]
    fn tool_call_and_result_go_inline_to_the_transcript() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::ToolCall {
            id: "t1".into(),
            name: "read_file".into(),
            args: "{\"path\":\"x\"}".into(),
        });
        app.apply(ChatEvent::ToolResult {
            id: "t1".into(),
            name: "read_file".into(),
            ok: true,
            output: "ok".into(),
        });
        assert!(app.conversation.iter().any(|l| l.starts_with('→')));
        assert!(app.conversation.iter().any(|l| l.starts_with('✓')));
    }

    #[test]
    fn bye_marks_session_done() {
        let mut app = App::new("a".into(), "ollama");
        assert!(!app.done);
        app.apply(ChatEvent::Bye);
        assert!(app.done);
    }

    #[test]
    fn parse_mention_splits_name_and_message() {
        assert_eq!(
            parse_mention("@busaba do the thing"),
            Some(("busaba", "do the thing"))
        );
        assert_eq!(parse_mention("@agent-pi hi"), Some(("agent-pi", "hi")));
        // Bare mention → jump only (empty message).
        assert_eq!(parse_mention("@pi"), Some(("pi", "")));
        // Leading whitespace tolerated; message is trimmed.
        assert_eq!(parse_mention("  @x   y "), Some(("x", "y")));
    }

    #[test]
    fn parse_mention_ignores_non_mentions() {
        assert_eq!(parse_mention("no mention here"), None);
        assert_eq!(parse_mention("email @ me"), None); // '@' not leading a token
        assert_eq!(parse_mention("@"), None); // empty name
        assert_eq!(parse_mention(""), None);
    }

    fn type_into(app: &mut App, text: &str) {
        for c in text.chars() {
            app.input_insert(c);
            app.input_changed();
        }
    }

    #[test]
    fn a_diff_event_renders_as_marked_rows() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::Diff {
            id: "c1".into(),
            path: "src/main.rs".into(),
            diff: "@@ -1,1 +1,1 @@\n-old\n+new\n".into(),
            truncated: true,
        });
        assert!(
            app.conversation
                .iter()
                .any(|l| l == "± src/main.rs (truncated)")
        );
        assert!(app.conversation.iter().any(|l| l == "±-old"));
        // Added rows read as success, removed as danger, the rest dimmed.
        assert_eq!(
            transcript_style("±+new").fg,
            Some(tone(design::color::SUCCESS))
        );
        assert_eq!(
            transcript_style("±-old").fg,
            Some(tone(design::color::DANGER))
        );
    }

    #[test]
    fn turn_end_clears_busy_and_keeps_a_reported_cost() {
        let mut app = App::new("a".into(), "ollama");
        app.busy = true;
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 10,
            completion_tokens: 2,
            cost_usd: Some(0.0123),
        });
        assert!(!app.busy);
        assert!(
            status_line(&app).contains("$0.0123"),
            "{}",
            status_line(&app)
        );
        // A later turn without a reported cost keeps the last known total
        // instead of dropping it.
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 20,
            completion_tokens: 4,
            cost_usd: None,
        });
        assert_eq!(app.cost_usd, Some(0.0123));
    }

    #[test]
    fn a_provider_that_reports_no_cost_shows_none() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 10,
            completion_tokens: 2,
            cost_usd: None,
        });
        assert!(!status_line(&app).contains('$'));
    }

    #[test]
    fn model_changed_updates_the_status_line() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::Ready {
            agent: "a".into(),
            model: "m1".into(),
            backend: "ollama".into(),
            tools: vec![],
        });
        app.apply(ChatEvent::ModelChanged { model: "m2".into() });
        assert!(status_line(&app).contains("model m2"));
    }

    #[test]
    fn status_report_names_the_session_without_a_json_flag() {
        let mut app = App::new("repo".into(), "ollama");
        app.workdir = Some(std::path::PathBuf::from("/src/repo"));
        app.session_id = Some("20260920T055214Z-80e6".into());
        app.apply(ChatEvent::Ready {
            agent: "repo".into(),
            model: "qwen3.8:27b".into(),
            backend: "ollama".into(),
            tools: vec!["read_file".into(), "write_file".into()],
        });
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 1_200,
            completion_tokens: 30,
            cost_usd: None,
        });
        let report = status_report(&app).join("\n");
        assert!(report.contains("qwen3.8:27b  (ollama)"), "{report}");
        assert!(report.contains("/src/repo"), "{report}");
        assert!(report.contains("20260920T055214Z-80e6"), "{report}");
        assert!(report.contains("turns     1 · 2 tool(s)"), "{report}");
        assert!(report.contains("not reported by this provider"), "{report}");
    }

    #[test]
    fn cost_report_shows_a_price_only_when_one_was_reported() {
        let mut app = App::new("a".into(), "openrouter");
        assert!(
            cost_report(&app)
                .join("\n")
                .contains("no usage reported yet")
        );
        app.apply(ChatEvent::TurnEnd {
            prompt_tokens: 2_000,
            completion_tokens: 100,
            cost_usd: Some(0.25),
        });
        let report = cost_report(&app).join("\n");
        assert!(report.contains("context   2k tokens"), "{report}");
        assert!(report.contains("$0.2500 (provider-reported)"), "{report}");
    }

    #[test]
    fn save_writes_the_transcript_under_the_workdir() {
        let dir = std::env::temp_dir().join(format!("bwoc-tui-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new("a".into(), "ollama");
        app.workdir = Some(dir.clone());
        app.conversation = vec!["you: hi".into(), "assistant: hello".into()];

        let line = save_transcript(&app, Some("out.md"));
        assert!(line.starts_with("● saved 2 line(s)"), "{line}");
        assert_eq!(
            std::fs::read_to_string(dir.join("out.md")).unwrap(),
            "you: hi\nassistant: hello\n"
        );
        // A path that cannot be written reports why instead of failing silently.
        let line = save_transcript(&app, Some("no-such-dir/out.md"));
        assert!(line.starts_with("✗ could not write"), "{line}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ready_keeps_the_tool_list_for_slash_tools() {
        let mut app = App::new("a".into(), "ollama");
        app.apply(ChatEvent::Ready {
            agent: "a".into(),
            model: "m".into(),
            backend: "ollama".into(),
            tools: vec!["glob".into(), "grep".into()],
        });
        assert_eq!(app.tools, ["glob", "grep"]);
    }

    #[test]
    fn popups_are_off_without_a_workdir() {
        // Fleet panes have no workdir: `/` and `@` stay plain input there.
        let mut app = App::new("a".into(), "ollama");
        type_into(&mut app, "/cl");
        assert!(app.popup().is_none());
    }

    #[test]
    fn slash_popup_completes_the_command() {
        let mut app = App::new("a".into(), "ollama");
        app.workdir = Some(std::env::temp_dir());
        type_into(&mut app, "/m");
        let popup = app.popup().expect("slash popup");
        assert!(popup.is_command);
        assert_eq!(popup.items[0].0, "/mode");
        app.accept_popup(&popup);
        assert_eq!(app.input, "/mode ");
        assert_eq!(app.input_cursor, app.input.len());
        // Hidden after a pick; typing brings popups back (none match here).
        assert!(app.popup().is_none());
    }

    #[test]
    fn file_popup_lists_the_workdir_on_first_at() {
        let dir = std::env::temp_dir().join(format!("bwoc-tui-at-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        let mut app = App::new("a".into(), "ollama");
        app.workdir = Some(dir.clone());
        type_into(&mut app, "explain @mai");
        let popup = app.popup().expect("file popup");
        assert!(!popup.is_command);
        app.accept_popup(&popup);
        assert_eq!(app.input, "explain @src/main.rs ");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_agent_tolerates_prefix() {
        let fleet = fleet_for_test(&["agent-anna", "agent-busaba"]);
        assert_eq!(fleet.resolve_agent("busaba"), Some(1));
        assert_eq!(fleet.resolve_agent("agent-busaba"), Some(1));
        assert_eq!(fleet.resolve_agent("anna"), Some(0));
        assert_eq!(fleet.resolve_agent("nobody"), None);
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
    fn scroll_key_moves_rows_pages_and_end_returns_to_live() {
        let mut app = App::new("a".into(), "ollama");
        assert_eq!(app.scroll, 0);
        assert!(app.scroll_key(KeyCode::Up));
        assert_eq!(app.scroll, 1);
        assert!(app.scroll_key(KeyCode::PageUp));
        assert_eq!(app.scroll, 11);
        assert!(app.scroll_key(KeyCode::PageUp));
        assert_eq!(app.scroll, 21);
        assert!(app.scroll_key(KeyCode::Down));
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

    #[test]
    fn ctrl_c_is_the_only_quit_key() {
        assert!(is_quit_key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(is_quit_key(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        ));
        assert!(!is_quit_key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!is_quit_key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!is_quit_key(KeyCode::Char('c'), KeyModifiers::NONE));
    }

    #[test]
    fn input_cursor_edits_at_unicode_boundaries() {
        let mut app = App::new("a".into(), "ollama");
        for ch in "aéz".chars() {
            app.input_insert(ch);
        }
        assert_eq!(app.input_cursor, app.input.len());

        app.input_left();
        app.input_insert('🙂');
        assert_eq!(app.input, "aé🙂z");
        app.input_left();
        app.input_backspace();
        assert_eq!(app.input, "a🙂z");
        app.input_right();
        app.input_backspace();
        assert_eq!(app.input, "az");

        assert_eq!(app.take_input(), "az");
        assert_eq!(app.input_cursor, 0);
        assert!(app.input.is_empty());
    }

    // ── End-to-end render pipeline ────────────────────────────────────────────
    //
    // These drive the WHOLE render path the operator sees — a chat_proto JSON
    // line off the wire is deserialized with the *same* `serde_json::from_str::
    // <ChatEvent>` the reader thread uses, applied to a real `App`, and painted
    // by the real `draw_frame` onto a ratatui `TestBackend`. The only pieces not
    // exercised are the OS subprocess and the TTY (non-deterministic by nature);
    // everything from wire bytes to rendered cells is real.

    /// Flatten a rendered `TestBackend` frame into text (one line per row) so a
    /// test can assert on what the operator would actually see.
    fn rendered_text(
        term: &ratatui::Terminal<ratatui::backend::TestBackend>,
        w: u16,
        h: u16,
    ) -> String {
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..h {
            for x in 0..w {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    /// Apply a slice of raw chat_proto lines to `app`, deserializing each exactly
    /// as the reader thread does (unparseable lines dropped, never fatal).
    fn feed_wire(app: &mut App, lines: &[&str]) {
        for line in lines {
            if let Ok(ev) = serde_json::from_str::<ChatEvent>(line) {
                app.apply(ev);
            }
        }
    }

    #[test]
    fn e2e_renders_full_conversation_frame_from_wire() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = App::new("agent-pi".into(), "ollama");
        feed_wire(
            &mut app,
            &[
                r#"{"type":"ready","agent":"agent-pi","model":"llama3","backend":"ollama","tools":["read_file"]}"#,
                r#"you: read x.txt"#, // a human banner line — must be dropped, not fatal
                r#"{"type":"token","text":"Hello "}"#,
                r#"{"type":"token","text":"there"}"#,
                r#"{"type":"message","text":"Hello there"}"#,
                r#"{"type":"tool_call","id":"c1","name":"read_file","args":"{\"path\":\"x.txt\"}"}"#,
                r#"{"type":"tool_result","id":"c1","name":"read_file","ok":true,"output":"secret"}"#,
                r#"{"type":"turn_end","prompt_tokens":10,"completion_tokens":5}"#,
            ],
        );
        // Operator has typed the next question but not sent it yet.
        for ch in "what does it say?".chars() {
            app.input_insert(ch);
        }

        let (w, h) = (80u16, 24u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        term.draw(|f| draw_frame(f, &app)).expect("draw");
        let screen = rendered_text(&term, w, h);

        // Status line: agent, model, backend.
        assert!(screen.contains("agent-pi"), "status agent:\n{screen}");
        assert!(screen.contains("llama3"), "status model:\n{screen}");
        // Assistant reply flushed to the transcript.
        assert!(screen.contains("Hello there"), "assistant line:\n{screen}");
        // Tool call + result rendered inline (name visible).
        assert!(screen.contains("read_file"), "tool line:\n{screen}");
        // Input box shows the operator's in-flight text after the `>` prompt.
        assert!(
            screen.contains("what does it say?"),
            "input echo:\n{screen}"
        );
        // Footer exposes the complete navigation, copy and exit contract.
        assert!(screen.contains("↑/↓"), "scroll footer:\n{screen}");
        assert!(screen.contains("←/→ cursor"), "cursor footer:\n{screen}");
        assert!(screen.contains("select/copy"), "copy footer:\n{screen}");
        assert!(screen.contains("Ctrl-C exit"), "exit footer:\n{screen}");
        assert!(!screen.contains("q/Esc"), "stale exit keys:\n{screen}");
        // The unparseable banner line never reached the transcript.
        assert!(
            !screen.contains("you: read x.txt"),
            "banner must be dropped:\n{screen}"
        );
    }

    #[test]
    fn e2e_permission_prompt_renders_in_input_box() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = App::new("agent-pi".into(), "ollama");
        feed_wire(
            &mut app,
            &[
                r#"{"type":"ready","agent":"agent-pi","model":"llama3","backend":"ollama","tools":[]}"#,
                r#"{"type":"permission_request","id":"p1","tool":"write_file","detail":"x.txt"}"#,
            ],
        );

        let (w, h) = (80u16, 24u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        term.draw(|f| draw_frame(f, &app)).expect("draw");
        let screen = rendered_text(&term, w, h);

        // The pending permission takes over the input box border/title.
        assert!(screen.contains("permission:"), "prompt title:\n{screen}");
        assert!(screen.contains("write_file"), "prompt tool:\n{screen}");
        assert!(
            screen.contains("[a]llow") && screen.contains("[d]eny"),
            "prompt actions:\n{screen}"
        );
    }

    #[test]
    fn harness_log_lives_under_bwoc_home() {
        assert_eq!(
            harness_log_path(Some(PathBuf::from("/h"))),
            Some(PathBuf::from("/h/.bwoc/logs/tui-harness.log"))
        );
        assert_eq!(harness_log_path(None), None);
    }

    #[test]
    fn permission_keys_never_consume_typing() {
        let late = PERMISSION_KEY_GRACE + Duration::from_millis(1);
        assert_eq!(
            permission_answer(KeyCode::Char('a'), true, late),
            Some(true)
        );
        assert_eq!(
            permission_answer(KeyCode::Char('d'), true, late),
            Some(false)
        );
        // Mid-sentence: the letter is input, not an answer.
        assert_eq!(permission_answer(KeyCode::Char('a'), false, late), None);
        assert_eq!(permission_answer(KeyCode::Char('d'), false, late), None);
        // A prompt that just appeared under a typing burst is not answered.
        assert_eq!(
            permission_answer(KeyCode::Char('a'), true, Duration::ZERO),
            None
        );
        assert_eq!(permission_answer(KeyCode::Char('x'), true, late), None);
    }

    #[test]
    fn hard_wrap_counts_columns_and_keeps_styles() {
        let style = Style::default().add_modifier(Modifier::BOLD);
        let rows = hard_wrap(vec![Line::from(Span::styled("abcdefg", style))], 3);
        let text: Vec<String> = rows.iter().map(|l| l.to_string()).collect();
        assert_eq!(text, ["abc", "def", "g"]);
        assert!(
            rows.iter()
                .all(|l| l.spans.iter().all(|s| s.style == style))
        );
        // Wide characters take two columns; an empty line stays one row.
        let rows = hard_wrap(vec![Line::from("日本語"), Line::from("")], 4);
        let text: Vec<String> = rows.iter().map(|l| l.to_string()).collect();
        assert_eq!(text, ["日本", "語", ""]);
        // A glyph wider than the pane still takes exactly one row each.
        let rows = hard_wrap(vec![Line::from("日本")], 1);
        let text: Vec<String> = rows.iter().map(|l| l.to_string()).collect();
        assert_eq!(text, ["日", "本"]);
    }

    #[test]
    fn e2e_long_tool_output_never_hides_the_answer() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = App::new("agent-pi".into(), "ollama");
        let long = "x".repeat(400);
        let result = format!(
            r#"{{"type":"tool_result","id":"c1","name":"webfetch","ok":true,"output":"{long}"}}"#
        );
        let mut wire = vec![
            r#"{"type":"ready","agent":"agent-pi","model":"m","backend":"ollama","tools":[]}"#,
        ];
        for _ in 0..3 {
            wire.push(result.as_str());
        }
        wire.push(r#"{"type":"message","text":"FINAL-ANSWER"}"#);
        wire.push(r#"{"type":"turn_end","prompt_tokens":1,"completion_tokens":1}"#);
        feed_wire(&mut app, &wire);

        let (w, h) = (60u16, 20u16);
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        term.draw(|f| draw_frame(f, &app)).expect("draw");
        let screen = rendered_text(&term, w, h);
        assert!(screen.contains("FINAL-ANSWER"), "answer hidden:\n{screen}");
    }
}
