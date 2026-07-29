//! Fleet discovery + a live `bwoc-harness --chat` session.
//!
//! A [`Session`] spawns the harness in `--chat` mode and bridges its line
//! protocol ([`bwoc_core::chat_proto`]) to the TUI: a reader thread parses each
//! stdout line into a [`ChatEvent`] and forwards it over an `mpsc` channel;
//! [`Session::send`] writes a [`ChatInput`] to the child's stdin.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use serde::Deserialize;

/// One agent as reported by `bwoc list --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub backend: String,
    pub running: bool,
    #[serde(rename = "inbox_count", default)]
    pub inbox_count: u32,
}

#[derive(Debug, Deserialize)]
struct FleetSnapshot {
    agents: Vec<AgentInfo>,
    #[allow(dead_code)]
    workspace: String,
}

/// Run `<bwoc> list --json` and parse the fleet. Returns an empty list on any
/// failure; the caller decides what an empty fleet means (`run_fleet` treats it
/// as a user error and exits rather than opening an empty sidebar).
pub fn fetch_fleet(bwoc_bin: &str, workdir: &str) -> Vec<AgentInfo> {
    let out = Command::new(bwoc_bin)
        .args(["list", "--json", "--workspace", workdir])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    serde_json::from_slice::<FleetSnapshot>(&out.stdout)
        .map(|s| s.agents)
        .unwrap_or_default()
}

/// Where the harness binary lives + how to reach the model — passed straight
/// through to `bwoc-harness --chat`. (Per-agent manifest resolution is a later
/// slice; P1 uses one operator-supplied backend/model for every session.)
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub harness_bin: String,
    pub workdir: String,
    pub backend: String,
    pub model: String,
    pub endpoint: String,
}

/// A running chat session bound to one agent.
pub struct Session {
    child: Child,
    stdin: ChildStdin,
    /// Chat events parsed off the child's stdout by the reader thread.
    pub rx: Receiver<ChatEvent>,
}

impl Session {
    /// Spawn `bwoc-harness --chat` for `agent` and start the reader thread.
    pub fn spawn(agent: &str, cfg: &SessionConfig) -> std::io::Result<Self> {
        // An empty model omits `--model` entirely — matching the single-agent
        // path, so the harness falls back to its own default rather than being
        // handed an empty (invalid) model string.
        let mut args: Vec<&str> = vec![
            "--chat",
            "--agent",
            agent,
            "--workdir",
            &cfg.workdir,
            "--backend",
            &cfg.backend,
            "--endpoint",
            &cfg.endpoint,
        ];
        if !cfg.model.is_empty() {
            args.push("--model");
            args.push(&cfg.model);
        }
        let mut child = Command::new(&cfg.harness_bin)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherit stderr like the single-agent TUI: invisible under the alt
            // screen, but still captured by a shell redirect so a crashed
            // session is diagnosable (it would otherwise vanish silently).
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx): (Sender<ChatEvent>, Receiver<ChatEvent>) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<ChatEvent>(line) {
                    Ok(ev) => {
                        let is_bye = matches!(ev, ChatEvent::Bye);
                        if tx.send(ev).is_err() || is_bye {
                            break;
                        }
                    }
                    // Non-JSON chatter (a stray print) — ignore, keep reading.
                    Err(_) => continue,
                }
            }
        });

        Ok(Self { child, stdin, rx })
    }

    /// Whether the child is still running. Used to reap a session whose harness
    /// exited *without* emitting `Bye` (channel disconnect alone can't be told
    /// apart from "no messages yet" via `try_iter`).
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Send one input line to the session. Best-effort — a broken pipe means the
    /// child died; the caller notices via the reader channel closing.
    pub fn send(&mut self, input: &ChatInput) {
        if let Ok(mut line) = input.to_line() {
            line.push('\n');
            let _ = self.stdin.write_all(line.as_bytes());
            let _ = self.stdin.flush();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Ask the session to exit cleanly, then reap so no orphan lingers.
        self.send(&ChatInput::Quit);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
