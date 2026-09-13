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
use bwoc_core::trust::Principal;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::{AgentSession, ConnectError, ReplyStream, SessionFactory};

/// Longest id kept verbatim in a session filename; longer ids are hashed.
const MAX_SEGMENT_LEN: usize = 64;

/// A filesystem-safe filename segment for an untrusted id. An id that is
/// already plain (`[A-Za-z0-9_-]`, non-empty, ≤ [`MAX_SEGMENT_LEN`]) is kept
/// verbatim (Telegram's negative group ids stay readable); anything else —
/// `..`, `/`, `\`, NUL, over-long — becomes `h.<sha256 prefix>`. The `.` can
/// never appear in a verbatim segment, so a hashed name can't collide with one,
/// and no output can traverse out of the sessions directory.
fn sanitize_segment(raw: &str) -> String {
    let plain = !raw.is_empty()
        && raw.len() <= MAX_SEGMENT_LEN
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if plain {
        return raw.to_string();
    }
    let digest = Sha256::digest(raw.as_bytes());
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("h.{hex}")
}

/// Per-chat conversation file:
/// `<agent_dir>/.bwoc/chat-sessions/<platform>-<chat_id>.json`. One file per
/// bridged chat so two chats (or a DM and a group) never share or clobber
/// history; the interactive `bwoc chat --tui` default is untouched.
pub fn chat_session_file(agent_dir: &Path, platform: &str, chat_id: &str) -> PathBuf {
    agent_dir.join(".bwoc").join("chat-sessions").join(format!(
        "{}-{}.json",
        sanitize_segment(platform),
        sanitize_segment(chat_id)
    ))
}

/// Provenance for a bridged turn: the sending platform user. Always
/// [`TrustLevel::Untrusted`](bwoc_core::trust::TrustLevel) — `trust()` elevates
/// only the local operator and the agent's own constitution.
pub fn bridged_principal(platform: &str, user_id: i64) -> Principal {
    Principal::Platform {
        platform: platform.to_string(),
        user_id,
    }
}

/// Builds a `bwoc-harness --chat` session per conversation against one agent
/// directory (resolving model/endpoint from its manifest, like `bwoc chat`).
pub struct HarnessSessionFactory {
    harness: PathBuf,
    agent_dir: PathBuf,
    /// Connector platform (`telegram` / `discord` / `line` / `imessage`): keys
    /// the per-chat session file and tags each turn's [`Principal::Platform`].
    platform: String,
    model: Option<String>,
    endpoint: Option<String>,
    /// Manifest `backend`, forwarded as `--backend` so a `cli` / `openrouter`
    /// / `claude` agent gets the right provider (#277 — without this, connect
    /// sessions silently ran on the harness default backend).
    backend: Option<String>,
    /// Manifest `cliCmd`, forwarded as `--cli-cmd` for `backend = "cli"`.
    cli_cmd: Option<String>,
    /// When set, sessions are spawned with `--team-chat <path>` so they join
    /// the team's shared `chat.jsonl` (group rooms, PR2). `None` = solo (DM).
    team_chat: Option<PathBuf>,
}

impl HarnessSessionFactory {
    /// Resolve the harness binary (sibling of this process, then PATH) and the
    /// agent's model/endpoint from `config.manifest.json` (best-effort).
    pub fn new(agent_dir: impl AsRef<Path>, platform: &str) -> Result<Self, ConnectError> {
        let agent_dir = agent_dir.as_ref().to_path_buf();
        let harness = bwoc_core::exec::sibling_binary("bwoc-harness").ok_or_else(|| {
            ConnectError::Session("bwoc-harness binary not found (install it / add to PATH)".into())
        })?;
        let manifest = Manifest::load_from_path(&agent_dir.join("config.manifest.json")).ok();
        let model = manifest.as_ref().map(|m| m.primary_model.clone());
        let endpoint = manifest.as_ref().and_then(|m| m.base_url.clone());
        let backend = manifest.as_ref().and_then(|m| m.backend.clone());
        let cli_cmd = manifest.as_ref().and_then(|m| m.cli_cmd.clone());
        Ok(Self {
            harness,
            agent_dir,
            platform: platform.to_string(),
            model,
            endpoint,
            backend,
            cli_cmd,
            team_chat: None,
        })
    }

    /// Spawn sessions joined to a team's shared `chat.jsonl` (group rooms).
    pub fn with_team_chat(mut self, chat_log: impl Into<PathBuf>) -> Self {
        self.team_chat = Some(chat_log.into());
        self
    }

    /// `(workdir, session file)` for a conversation. Allow-listed: the agent dir
    /// and its per-chat file (unchanged). Public: a prepared isolated workdir
    /// with the session file inside it, so it can't even list other chats.
    fn spawn_paths(&self, chat_id: i64, public: bool) -> Result<(PathBuf, PathBuf), ConnectError> {
        let chat = chat_id.to_string();
        if public {
            let workdir = prepare_public_workdir(&self.agent_dir, &self.platform, &chat)?;
            let file = workdir.join(".bwoc").join("chat-session.json");
            Ok((workdir, file))
        } else {
            let file = chat_session_file(&self.agent_dir, &self.platform, &chat);
            Ok((self.agent_dir.clone(), file))
        }
    }
}

/// Isolated workdir for a limited-public session:
/// `<agent_dir>/.bwoc/public/<platform>-<chat_id>/`. The harness runs with this
/// as `--workdir`, so its (canonicalized) file-tool confinement keeps a public
/// turn out of the agent's memories, connectors, skills, and other chats.
pub fn public_workdir(agent_dir: &Path, platform: &str, chat_id: &str) -> PathBuf {
    agent_dir.join(".bwoc").join("public").join(format!(
        "{}-{}",
        sanitize_segment(platform),
        sanitize_segment(chat_id)
    ))
}

/// Create the public workdir and copy in the persona (`AGENTS.md`, or the
/// `CLAUDE.md` the harness would fall back to, written as `AGENTS.md`) and
/// `config.manifest.json` — nothing else. Copies, never symlinks; re-copied
/// when the source is newer.
pub fn prepare_public_workdir(
    agent_dir: &Path,
    platform: &str,
    chat_id: &str,
) -> Result<PathBuf, ConnectError> {
    let dir = public_workdir(agent_dir, platform, chat_id);
    let fail = |what: &str, e: std::io::Error| {
        ConnectError::Session(format!("public workdir {}: {what}: {e}", dir.display()))
    };
    std::fs::create_dir_all(&dir).map_err(|e| fail("create", e))?;
    if let Some(src) = ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .map(|f| agent_dir.join(f))
        .find(|p| p.is_file())
    {
        copy_if_newer(&src, &dir.join("AGENTS.md"), Some).map_err(|e| fail("AGENTS.md", e))?;
    }
    let manifest = agent_dir.join("config.manifest.json");
    if manifest.is_file() {
        copy_if_newer(
            &manifest,
            &dir.join("config.manifest.json"),
            strip_deep_memory,
        )
        .map_err(|e| fail("config.manifest.json", e))?;
    }
    Ok(dir)
}

/// Copy `src` → `dst` (through `transform`) unless `dst` is already at least as
/// new. `dst` is removed first so a pre-existing link there is replaced, never
/// written through. `transform` returning `None` (e.g. a malformed manifest)
/// **removes** `dst` rather than leaving a stale copy, so the harness falls back
/// to its defaults instead of reading an out-of-date file.
fn copy_if_newer(
    src: &Path,
    dst: &Path,
    transform: impl FnOnce(Vec<u8>) -> Option<Vec<u8>>,
) -> std::io::Result<()> {
    let src_time = std::fs::metadata(src)?.modified()?;
    let fresh = std::fs::symlink_metadata(dst)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t >= src_time);
    if fresh {
        return Ok(());
    }
    let transformed = transform(std::fs::read(src)?);
    let _ = std::fs::remove_file(dst);
    match transformed {
        Some(bytes) => std::fs::write(dst, bytes),
        None => Ok(()),
    }
}

/// Drop `deepMemoryCmd` from the public manifest copy: its wake-up would inject
/// the agent's recalled memory into a stranger's system prompt, and its
/// session-end mine would write the stranger's conversation into that memory.
/// A malformed manifest isn't copied (the harness then uses its defaults).
fn strip_deep_memory(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.as_object_mut()?.remove("deepMemoryCmd");
    serde_json::to_vec_pretty(&v).ok()
}

/// The harness session mode a limited-public session is locked into: the
/// `plan` mode's fixed read-only tool allow-list (`read_file`, `list_dir`,
/// `grep`, `memory_read`); every write, `run_command`, git, delegation, and
/// MCP tool is refused before the permission gate.
const READ_ONLY_MODE: &str = "plan";

/// One startup event while waiting for the read-only ack: `None` ⇒ keep
/// reading (the restored-history replay). Anything but the `plan` ack — an
/// error, another mode, EOF — fails session creation (fail closed).
fn read_only_ack(ev: Option<ChatEvent>) -> Option<Result<(), ConnectError>> {
    match ev {
        Some(ChatEvent::ModeChanged { mode }) if mode == READ_ONLY_MODE => Some(Ok(())),
        Some(ChatEvent::Restored { .. }) => None,
        _ => Some(Err(ConnectError::Session(
            "harness did not confirm read-only mode for a public session".into(),
        ))),
    }
}

#[async_trait]
impl SessionFactory for HarnessSessionFactory {
    async fn create(
        &self,
        chat_id: i64,
        public: bool,
    ) -> Result<Box<dyn AgentSession>, ConnectError> {
        let (workdir, session_file) = self.spawn_paths(chat_id, public)?;
        let mut s = HarnessSession::spawn(
            &self.harness,
            &workdir,
            &session_file,
            self.platform.clone(),
            self.model.as_deref(),
            self.endpoint.as_deref(),
            self.backend.as_deref(),
            self.cli_cmd.as_deref(),
            self.team_chat.as_deref(),
        )
        .await?;
        if public {
            s.enter_read_only().await?;
        }
        Ok(Box::new(s))
    }
}

pub struct HarnessSession {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    platform: String,
}

impl HarnessSession {
    #[allow(clippy::too_many_arguments)]
    async fn spawn(
        harness: &Path,
        agent_dir: &Path,
        session_file: &Path,
        platform: String,
        model: Option<&str>,
        endpoint: Option<&str>,
        backend: Option<&str>,
        cli_cmd: Option<&str>,
        team_chat: Option<&Path>,
    ) -> Result<Self, ConnectError> {
        let mut cmd = Command::new(harness);
        cmd.arg("--chat")
            .arg("--workdir")
            .arg(agent_dir)
            .arg("--session-file")
            .arg(session_file);
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }
        if let Some(e) = endpoint {
            cmd.arg("--endpoint").arg(e);
        }
        if let Some(b) = backend {
            cmd.arg("--backend").arg(b);
        }
        if let Some(c) = cli_cmd {
            cmd.arg("--cli-cmd").arg(c);
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
            platform,
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

    /// Lock this session to [`READ_ONLY_MODE`] before any user text is sent.
    /// Only this bridge writes the harness's stdin, and remote text only ever
    /// travels inside a JSON-encoded `User` input, so a sender can't switch
    /// the mode back.
    async fn enter_read_only(&mut self) -> Result<(), ConnectError> {
        self.write_input(&ChatInput::SetMode {
            mode: READ_ONLY_MODE.to_string(),
        })
        .await?;
        loop {
            let ev = self.next_event().await?;
            if let Some(outcome) = read_only_ack(ev) {
                return outcome;
            }
        }
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
    async fn ask(&mut self, text: &str, from_user_id: i64) -> Result<String, ConnectError> {
        // Non-streaming callers get the final text only.
        let mut noop = NoopStream;
        self.ask_streamed(text, from_user_id, &mut noop).await
    }

    async fn ask_streamed(
        &mut self,
        text: &str,
        from_user_id: i64,
        sink: &mut dyn ReplyStream,
    ) -> Result<String, ConnectError> {
        // Phase 5 t1: chat-connector ingress is unauthenticated adversarial
        // input. Stamp the sender's platform identity as provenance; a
        // `Platform` principal is Untrusted, so the turn stays fail-closed.
        self.write_input(&ChatInput::User {
            text: text.to_string(),
            principal: bridged_principal(&self.platform, from_user_id),
        })
        .await?;

        // Accumulate `Token` deltas and push the running reply to the sink; the
        // terminal `Message` carries the canonical full text we return.
        let mut acc = String::new();
        loop {
            let Some(ev) = self.next_event().await? else {
                return Err(ConnectError::Session("harness ended mid-turn".into()));
            };
            match ev {
                ChatEvent::Token { text } => {
                    acc.push_str(&text);
                    sink.push(&acc).await;
                }
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
                // ToolCall/ToolResult/TurnEnd/Compacted/Error/etc. — keep reading.
                _ => {}
            }
        }
    }
}

/// A [`ReplyStream`] that drops every push — used by the non-streaming
/// [`AgentSession::ask`] path.
struct NoopStream;

#[async_trait]
impl ReplyStream for NoopStream {
    async fn push(&mut self, _accumulated: &str) {}
}

impl Drop for HarnessSession {
    fn drop(&mut self) {
        // Best-effort graceful quit; kill_on_drop reaps if it ignores us.
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_that_no_longer_parses_removes_the_stale_public_copy() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("config.manifest.json");
        let dst = dst_dir.path().join("config.manifest.json");
        std::fs::write(&src, br#"{"name":"x","deepMemoryCmd":"mine"}"#).unwrap();
        copy_if_newer(&src, &dst, strip_deep_memory).unwrap();
        assert!(dst.is_file());

        // Make the source newer and malformed: the old copy must go.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&src, b"{ not json").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(later)
            .unwrap();
        copy_if_newer(&src, &dst, strip_deep_memory).unwrap();
        assert!(!dst.exists(), "stale manifest copy must be removed");
    }
    use bwoc_core::trust::TrustLevel;

    fn sessions_dir(agent: &Path) -> PathBuf {
        agent.join(".bwoc").join("chat-sessions")
    }

    #[test]
    fn distinct_chats_get_distinct_session_files() {
        let agent = Path::new("/agents/agent-x");
        let dm = chat_session_file(agent, "telegram", "42");
        let group = chat_session_file(agent, "telegram", "-100123");
        assert_ne!(dm, group);
        assert_eq!(dm, sessions_dir(agent).join("telegram-42.json"));
        assert_eq!(group, sessions_dir(agent).join("telegram--100123.json"));
        // Same id on another platform is a different conversation.
        assert_ne!(dm, chat_session_file(agent, "discord", "42"));
        // Never the interactive TUI's default file.
        assert_ne!(dm, agent.join(".bwoc").join("chat-session.json"));
    }

    #[test]
    fn hostile_ids_are_sanitized_into_the_sessions_dir() {
        let agent = Path::new("/agents/agent-x");
        let long = "9".repeat(10_000);
        let hostile = ["../x", "/", "..", "a/../../b", "a\\b", "x\0y", "", &long];
        let mut names = std::collections::HashSet::new();
        for id in hostile {
            let p = chat_session_file(agent, "telegram", id);
            assert_eq!(p.parent(), Some(sessions_dir(agent).as_path()), "{id:?}");
            let name = p.file_name().unwrap().to_str().unwrap();
            assert!(!name.contains('/') && !name.contains('\\') && !name.contains(".."));
            assert!(name.len() < 100, "bounded length: {name}");
            assert!(names.insert(name.to_string()), "no collision for {id:?}");
        }
        // A hostile platform tag is contained the same way.
        let p = chat_session_file(agent, "../../etc", "1");
        assert_eq!(p.parent(), Some(sessions_dir(agent).as_path()));
    }

    fn factory(agent: &Path) -> HarnessSessionFactory {
        HarnessSessionFactory {
            harness: PathBuf::from("bwoc-harness"),
            agent_dir: agent.to_path_buf(),
            platform: "telegram".into(),
            model: None,
            endpoint: None,
            backend: None,
            cli_cmd: None,
            team_chat: None,
        }
    }

    /// An agent dir with the persona, a manifest, and everything a public
    /// session must not see.
    fn populated_agent() -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().unwrap();
        let a = tmp.path();
        std::fs::write(a.join("AGENTS.md"), "persona").unwrap();
        std::fs::write(
            a.join("config.manifest.json"),
            r#"{"agentId":"agent-x","primaryModel":"m","deepMemoryCmd":"mem recall"}"#,
        )
        .unwrap();
        for d in ["memories", "connectors", "skills", ".bwoc/chat-sessions"] {
            std::fs::create_dir_all(a.join(d)).unwrap();
        }
        std::fs::write(a.join("memories/MEMORY.md"), "private").unwrap();
        std::fs::write(a.join("connectors/telegram.toml"), "enabled = true").unwrap();
        std::fs::write(a.join(".bwoc/chat-sessions/telegram-111.json"), "[]").unwrap();
        tmp
    }

    #[test]
    fn public_sessions_get_an_isolated_workdir_allow_listed_keep_the_agent_dir() {
        let agent = populated_agent();
        let f = factory(agent.path());
        let (wd, file) = f.spawn_paths(111, false).unwrap();
        assert_eq!(wd, agent.path());
        assert_eq!(file, chat_session_file(agent.path(), "telegram", "111"));
        let (pwd, pfile) = f.spawn_paths(-100, true).unwrap();
        assert_eq!(pwd, agent.path().join(".bwoc/public/telegram--100"));
        assert!(
            pfile.starts_with(&pwd),
            "session file lives in the public dir"
        );
        assert!(pwd.is_dir());
    }

    #[test]
    fn public_workdir_holds_only_the_copied_persona_and_manifest() {
        let agent = populated_agent();
        let wd = prepare_public_workdir(agent.path(), "telegram", "42").unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&wd)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["AGENTS.md", "config.manifest.json"]);
        let persona = wd.join("AGENTS.md");
        assert!(!persona.is_symlink(), "a copy, not a link");
        assert_eq!(std::fs::read_to_string(&persona).unwrap(), "persona");
        let m: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(wd.join("config.manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(m["primaryModel"], "m");
        assert!(m.get("deepMemoryCmd").is_none(), "deep memory stripped");
    }

    #[test]
    fn public_workdir_recopies_a_newer_persona() {
        let agent = populated_agent();
        let wd = prepare_public_workdir(agent.path(), "telegram", "42").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(wd.join("AGENTS.md"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        std::fs::write(agent.path().join("AGENTS.md"), "persona v2").unwrap();
        prepare_public_workdir(agent.path(), "telegram", "42").unwrap();
        assert_eq!(
            std::fs::read_to_string(wd.join("AGENTS.md")).unwrap(),
            "persona v2"
        );
    }

    #[test]
    fn hostile_ids_stay_inside_the_public_root() {
        let agent = tempfile::TempDir::new().unwrap();
        let root = agent.path().join(".bwoc").join("public");
        for (platform, chat) in [
            ("../../etc", "1"),
            ("telegram", "../x"),
            ("telegram", "a/../../b"),
            ("..", ".."),
        ] {
            let wd = prepare_public_workdir(agent.path(), platform, chat).unwrap();
            assert_eq!(wd.parent(), Some(root.as_path()), "{platform:?} {chat:?}");
        }
    }

    #[test]
    fn read_only_handshake_requires_the_plan_ack() {
        assert!(matches!(
            read_only_ack(Some(ChatEvent::ModeChanged {
                mode: "plan".into()
            })),
            Some(Ok(()))
        ));
        // History replay after Ready is skipped, not treated as a failure.
        assert!(
            read_only_ack(Some(ChatEvent::Restored {
                role: "user".into(),
                text: "hi".into()
            }))
            .is_none()
        );
        // Anything else fails closed: another mode, an error, EOF.
        for ev in [
            Some(ChatEvent::ModeChanged {
                mode: "default".into(),
            }),
            Some(ChatEvent::Error {
                message: "unknown permission mode".into(),
            }),
            None,
        ] {
            assert!(matches!(read_only_ack(ev), Some(Err(_))));
        }
        // The mode string is one the harness parses.
        let line = ChatInput::SetMode {
            mode: READ_ONLY_MODE.into(),
        }
        .to_line()
        .unwrap();
        assert!(
            line.contains("\"set_mode\"") && line.contains("\"plan\""),
            "{line}"
        );
    }

    #[test]
    fn bridged_turn_principal_is_platform_and_untrusted() {
        let p = bridged_principal("telegram", 111);
        assert_eq!(
            p,
            Principal::Platform {
                platform: "telegram".into(),
                user_id: 111
            }
        );
        assert_eq!(p.trust(), TrustLevel::Untrusted);
        assert!(p.carries_untrusted_taint());
    }
}
