//! [`AgentSession`] backed by a `bwoc-harness --chat` subprocess.
//!
//! The bridge is just another `chat_proto` frontend (like `bwoc chat --tui`):
//! it writes one [`ChatInput::User`] per inbound message and reads
//! [`ChatEvent`]s until the turn's final [`ChatEvent::Message`]. A
//! [`ChatEvent::PermissionRequest`] is **auto-denied** — a remote chat user can
//! never approve a tool call (the same fail-safe as `ask`-in-non-TTY).
//!
//! This is the live subprocess/protocol edge; the routing/allow-list logic it
//! plugs into ([`crate::run_bridge`]) is what carries the unit tests.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use bwoc_core::manifest::Manifest;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::{AgentSession, ConnectError, SessionFactory};

/// Builds a `bwoc-harness --chat` session per conversation against one agent
/// directory (resolving model/endpoint from its manifest, like `bwoc chat`).
pub struct HarnessSessionFactory {
    harness: PathBuf,
    agent_dir: PathBuf,
    model: Option<String>,
    endpoint: Option<String>,
    /// When set, sessions are spawned with `--team-chat <path>` so they join
    /// the team's shared `chat.jsonl` (group rooms, PR2). `None` = solo (DM).
    team_chat: Option<PathBuf>,
}

impl HarnessSessionFactory {
    /// Resolve the harness binary (sibling of this process, then PATH) and the
    /// agent's model/endpoint from `config.manifest.json` (best-effort).
    pub fn new(agent_dir: impl AsRef<Path>) -> Result<Self, ConnectError> {
        let agent_dir = agent_dir.as_ref().to_path_buf();
        let harness = bwoc_core::exec::sibling_binary("bwoc-harness").ok_or_else(|| {
            ConnectError::Session("bwoc-harness binary not found (install it / add to PATH)".into())
        })?;
        let manifest = Manifest::load_from_path(&agent_dir.join("config.manifest.json")).ok();
        let model = manifest.as_ref().map(|m| m.primary_model.clone());
        let endpoint = manifest.as_ref().and_then(|m| m.base_url.clone());
        Ok(Self {
            harness,
            agent_dir,
            model,
            endpoint,
            team_chat: None,
        })
    }

    /// Spawn sessions joined to a team's shared `chat.jsonl` (group rooms).
    pub fn with_team_chat(mut self, chat_log: impl Into<PathBuf>) -> Self {
        self.team_chat = Some(chat_log.into());
        self
    }
}

#[async_trait]
impl SessionFactory for HarnessSessionFactory {
    async fn create(&self) -> Result<Box<dyn AgentSession>, ConnectError> {
        let s = HarnessSession::spawn(
            &self.harness,
            &self.agent_dir,
            self.model.as_deref(),
            self.endpoint.as_deref(),
            self.team_chat.as_deref(),
        )
        .await?;
        Ok(Box::new(s))
    }
}

pub struct HarnessSession {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
}

impl HarnessSession {
    async fn spawn(
        harness: &Path,
        agent_dir: &Path,
        model: Option<&str>,
        endpoint: Option<&str>,
        team_chat: Option<&Path>,
    ) -> Result<Self, ConnectError> {
        let mut cmd = Command::new(harness);
        cmd.arg("--chat").arg("--workdir").arg(agent_dir);
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }
        if let Some(e) = endpoint {
            cmd.arg("--endpoint").arg(e);
        }
        if let Some(tc) = team_chat {
            cmd.arg("--team-chat").arg(tc);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| ConnectError::Session(format!("spawn bwoc-harness: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ConnectError::Session("no child stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ConnectError::Session("no child stdout".into()))?;
        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
        };
        // Drain startup events up to and including `Ready` so the first `ask`
        // doesn't race the banner/restore replay.
        session.drain_until_ready().await?;
        Ok(session)
    }

    async fn next_event(&mut self) -> Result<Option<ChatEvent>, ConnectError> {
        loop {
            let line = self
                .stdout
                .next_line()
                .await
                .map_err(|e| ConnectError::Session(format!("read: {e}")))?;
            let Some(line) = line else { return Ok(None) }; // EOF
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Non-event lines shouldn't appear on the chat stdout, but skip any
            // defensively (forward-compatible with unknown future variants too).
            if let Ok(ev) = serde_json::from_str::<ChatEvent>(line) {
                return Ok(Some(ev));
            }
        }
    }

    async fn drain_until_ready(&mut self) -> Result<(), ConnectError> {
        while let Some(ev) = self.next_event().await? {
            if matches!(ev, ChatEvent::Ready { .. }) {
                return Ok(());
            }
        }
        Err(ConnectError::Session("harness exited before Ready".into()))
    }

    async fn write_input(&mut self, input: &ChatInput) -> Result<(), ConnectError> {
        let line = input
            .to_line()
            .map_err(|e| ConnectError::Session(format!("encode input: {e}")))?;
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| ConnectError::Session(format!("write: {e}")))
    }
}

#[async_trait]
impl AgentSession for HarnessSession {
    async fn ask(&mut self, text: &str) -> Result<String, ConnectError> {
        self.write_input(&ChatInput::User {
            text: text.to_string(),
        })
        .await?;

        loop {
            let Some(ev) = self.next_event().await? else {
                return Err(ConnectError::Session("harness ended mid-turn".into()));
            };
            match ev {
                // The turn's final assistant text.
                ChatEvent::Message { text } => return Ok(text),
                // A remote user can't approve tools — deny and continue the turn.
                ChatEvent::PermissionRequest { id, .. } => {
                    self.write_input(&ChatInput::Permission { id, allow: false })
                        .await?;
                }
                ChatEvent::Bye => {
                    return Err(ConnectError::Session("session ended".into()));
                }
                // Token/ToolCall/ToolResult/TurnEnd/Compacted/Error/etc. — keep reading.
                _ => {}
            }
        }
    }
}

impl Drop for HarnessSession {
    fn drop(&mut self) {
        // Best-effort graceful quit; kill_on_drop reaps if it ignores us.
        let _ = self.child.start_kill();
    }
}
