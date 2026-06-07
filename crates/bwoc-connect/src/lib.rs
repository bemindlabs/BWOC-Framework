//! `bwoc-connect` — chat connectors bridging external platforms (Telegram
//! first) to BWOC agents. Design: `notes/2026-06-06_chat-connectors-design.md`.
//!
//! **Dep-quarantine**: the network deps (reqwest/tokio) live in this crate
//! only. `bwoc-cli` / `bwoc-agent` / `bwoc-core` never pull them in.
//!
//! ## Shape (PR1 — Telegram DM)
//!
//! The bridge is just another chat frontend: for each allow-listed sender it
//! holds a [`AgentSession`] (a `bwoc-harness --chat` subprocess speaking the
//! existing `bwoc_core::chat_proto`) and relays text both ways over a
//! [`Transport`]. Both are traits so [`run_bridge`] — the routing/allow-list/
//! offset logic — is unit-tested without a live bot or a real harness.
//!
//! Security (PR1): a **closed-by-default** sender allow-list (empty ⇒ nobody);
//! the harness session is non-TTY so `ask`-mode tools fail safe to deny — a
//! remote user can never approve a tool call. Group rooms + daemon supervision
//! are PR2/PR3.

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;

pub mod discord;
pub mod session;
pub mod telegram;

/// Seconds to wait after a poll error before retrying (avoids a hot loop / log
/// flood when the platform is unreachable or rate-limiting).
const POLL_BACKOFF_SECS: u64 = 2;

/// Minimum gap between in-place edits while streaming a reply. Telegram allows
/// ~1 edit/sec per chat and Discord ~5/5s per channel; 1s stays clear of both.
/// The final edit (on turn end) always fires regardless, so the complete reply
/// is never throttled away.
const EDIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1000);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("config error: {0}")]
    Config(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("agent session error: {0}")]
    Session(String),
    #[error("no bot token found — set {0}")]
    NoToken(String),
}

// ---------------------------------------------------------------------------
// Config — <agent>/connectors/telegram.toml
// ---------------------------------------------------------------------------

/// Per-agent Telegram connector config. The token is **not** here — it
/// resolves via the keyring / env fallback (see `main`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TelegramConfig {
    /// Connector is off unless explicitly enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Platform user ids permitted to reach the agent. **Empty ⇒ nobody**
    /// (closed by default — no public bots).
    #[serde(default)]
    pub allow_from: Vec<i64>,
    /// Group binding (PR2). Parsed now so the config shape is stable; unused
    /// in PR1's DM-only path.
    #[serde(default)]
    pub group: Option<GroupConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GroupConfig {
    /// Saṅgha team id whose `chat.jsonl` backs group rooms (PR2).
    #[serde(default)]
    pub team: Option<String>,
    /// Reply only when @mentioned (PR2).
    #[serde(default = "default_true")]
    pub mention_only: bool,
}

fn default_true() -> bool {
    true
}

/// The connector config shape is platform-agnostic (enabled / allow_from /
/// group), so Discord reuses it from `connectors/discord.toml`.
pub type ConnectorConfig = TelegramConfig;

impl TelegramConfig {
    /// Parse a connector config body (`telegram.toml` / `discord.toml`).
    pub fn parse(toml_src: &str) -> Result<Self, ConnectError> {
        toml::from_str(toml_src).map_err(|e| ConnectError::Config(e.to_string()))
    }

    /// Closed-by-default membership: an empty/absent allow-list permits nobody.
    pub fn is_allowed(&self, user_id: i64) -> bool {
        self.allow_from.contains(&user_id)
    }
}

// ---------------------------------------------------------------------------
// Seams — Transport + AgentSession
// ---------------------------------------------------------------------------

/// One inbound platform message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    /// Monotonic update id — the bridge advances its poll offset past it.
    pub update_id: i64,
    /// Where a reply goes (a DM's chat id == the sender's private chat).
    pub chat_id: i64,
    /// Sender's platform user id (checked against the allow-list).
    pub from_user_id: i64,
    pub text: String,
    /// `true` for a group/supergroup chat, `false` for a private DM (PR2).
    pub is_group: bool,
    /// `true` if this group message @mentions the bot (drives the
    /// mention-gate; always `false`/ignored for DMs). (PR2)
    pub mentions_bot: bool,
}

/// A platform transport: long-poll for messages, send + edit replies.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Fetch messages with `update_id >= offset`. May block up to the
    /// transport's long-poll timeout, returning `[]` when nothing arrives.
    async fn poll(&self, offset: i64) -> Result<Vec<Incoming>, ConnectError>;
    /// Send `text` to `chat_id`; return the new message's id (needed to edit it
    /// while streaming).
    async fn send(&self, chat_id: i64, text: &str) -> Result<i64, ConnectError>;
    /// Replace the text of an existing message (used to stream a growing reply
    /// in place). Editing to identical text should be treated as a no-op error
    /// by the impl, not propagated.
    async fn edit(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), ConnectError>;
}

/// Sink the agent session pushes the **accumulated** reply to as tokens arrive,
/// so a [`Transport`] can render a growing message in place. The bridge's
/// [`PlatformStream`] is the production impl (send-then-debounced-edit); the
/// default [`AgentSession::ask_streamed`] ignores it (single send on finish).
#[async_trait]
pub trait ReplyStream: Send {
    /// Called with the reply-so-far (accumulated, not a delta) on each token.
    async fn push(&mut self, accumulated: &str);
}

/// One agent conversation (a `bwoc-harness --chat` subprocess in PR1).
#[async_trait]
pub trait AgentSession: Send {
    /// Deliver a user message; return the agent's final reply text.
    async fn ask(&mut self, text: &str) -> Result<String, ConnectError>;

    /// Like [`ask`](Self::ask) but pushing the accumulated reply to `sink` as
    /// tokens stream in. The default delegates to `ask` (no streaming — one
    /// send on finish); `HarnessSession` overrides it to relay `chat_proto`
    /// `Token` events.
    async fn ask_streamed(
        &mut self,
        text: &str,
        sink: &mut dyn ReplyStream,
    ) -> Result<String, ConnectError> {
        let _ = sink;
        self.ask(text).await
    }
}

/// Makes a fresh [`AgentSession`] per conversation (lazily, on first message).
#[async_trait]
pub trait SessionFactory: Send + Sync {
    async fn create(&self) -> Result<Box<dyn AgentSession>, ConnectError>;
}

// ---------------------------------------------------------------------------
// Bridge loop (the testable core)
// ---------------------------------------------------------------------------

/// Group-bridge wiring (PR2): a team's shared `chat.jsonl` (HV3-3a) plus the
/// factory that spawns `--team-chat` sessions for group rooms.
pub struct GroupBridge<'a> {
    pub factory: &'a dyn SessionFactory,
    pub chat_log: std::path::PathBuf,
    /// Short platform tag for the peer `from` field in the team `chat.jsonl`
    /// (e.g. `"tg"` / `"dc"`), so a logged peer reads `tg:<id>` / `dc:<id>`.
    pub peer_prefix: String,
}

/// Run the relay: poll → allow-list filter → per-chat session → reply.
///
/// DMs go through `dm_factory`. When `group` is set, group/supergroup messages
/// from allow-listed senders are bridged to a Saṅgha team (HV3-3a): a
/// mention (or a non-mention-gated room) is served by a `--team-chat` session
/// and the reply is sent back; a non-mention message is just appended to the
/// team `chat.jsonl` as peer context. A group message with no `group` binding
/// is ignored. One `AgentSession` is held per `chat_id` (DM/room continuity).
/// `max_polls` bounds the loop for tests (`None` = forever).
pub async fn run_bridge(
    transport: &dyn Transport,
    dm_factory: &dyn SessionFactory,
    group: Option<GroupBridge<'_>>,
    config: &TelegramConfig,
    max_polls: Option<usize>,
) -> Result<(), ConnectError> {
    let mut offset: i64 = 0;
    let mut sessions: HashMap<i64, Box<dyn AgentSession>> = HashMap::new();
    let mut polls = 0usize;
    let mention_only = config.group.as_ref().is_none_or(|g| g.mention_only);

    loop {
        if let Some(max) = max_polls {
            if polls >= max {
                return Ok(());
            }
        }
        polls += 1;

        let messages = match transport.poll(offset).await {
            Ok(m) => m,
            Err(e) => {
                // Back off before retrying so an unreachable / rate-limiting
                // endpoint can't spin the loop hot or flood the log.
                eprintln!("[bwoc-connect] poll error (retrying in {POLL_BACKOFF_SECS}s): {e}");
                tokio::time::sleep(std::time::Duration::from_secs(POLL_BACKOFF_SECS)).await;
                continue;
            }
        };

        for msg in messages {
            offset = offset.max(msg.update_id + 1);

            if !config.is_allowed(msg.from_user_id) {
                eprintln!(
                    "[bwoc-connect] ignoring message from non-allow-listed user {}",
                    msg.from_user_id
                );
                continue;
            }

            if msg.is_group {
                let Some(gb) = group.as_ref() else {
                    eprintln!(
                        "[bwoc-connect] group message but no team binding configured; ignoring"
                    );
                    continue;
                };
                // Mention-gated rooms: a non-mention message is logged as peer
                // context (so the agent sees it on its next addressed turn) but
                // draws no reply.
                if mention_only && !msg.mentions_bot {
                    append_peer(
                        &gb.chat_log,
                        &format!("{}:{}", gb.peer_prefix, msg.from_user_id),
                        &msg.text,
                    );
                    continue;
                }
                // Addressed: serve via the --team-chat group session, which
                // injects the room's peer messages and broadcasts its reply.
                serve_turn(&mut sessions, gb.factory, transport, msg.chat_id, &msg.text).await;
            } else {
                serve_turn(&mut sessions, dm_factory, transport, msg.chat_id, &msg.text).await;
            }
        }
    }
}

/// Spawn-or-reuse the `chat_id`'s session, deliver `text`, relay the reply.
/// On an agent error the (likely dead) session is dropped so the next message
/// respawns a fresh one rather than failing forever.
async fn serve_turn(
    sessions: &mut HashMap<i64, Box<dyn AgentSession>>,
    factory: &dyn SessionFactory,
    transport: &dyn Transport,
    chat_id: i64,
    text: &str,
) {
    if let std::collections::hash_map::Entry::Vacant(slot) = sessions.entry(chat_id) {
        match factory.create().await {
            Ok(s) => {
                slot.insert(s);
            }
            Err(e) => {
                eprintln!("[bwoc-connect] could not start agent session: {e}");
                let _ = transport
                    .send(chat_id, "⚠️ couldn't start the agent session; try again.")
                    .await;
                return;
            }
        }
    }
    let session = sessions.get_mut(&chat_id).expect("inserted above");
    let mut stream = PlatformStream::new(transport, chat_id);
    match session.ask_streamed(text, &mut stream).await {
        Ok(reply) => {
            // Ensure the platform shows the complete reply: a final edit (or a
            // single send if no tokens streamed). Errors are already logged
            // inside the stream; finish swallows the no-op-identical case.
            stream.finish(&reply).await;
        }
        Err(e) => {
            eprintln!("[bwoc-connect] agent error: {e}");
            sessions.remove(&chat_id);
            let _ = transport
                .send(chat_id, &format!("⚠️ agent error: {e}"))
                .await;
        }
    }
}

/// A [`ReplyStream`] that renders a growing reply into one platform message:
/// the first non-empty push **sends** a message; later pushes **edit** it in
/// place, debounced to [`EDIT_INTERVAL`]; [`finish`](Self::finish) guarantees
/// the final text is shown. If nothing was ever pushed (a non-streaming
/// session), `finish` sends the whole reply once — the original behaviour.
struct PlatformStream<'a> {
    transport: &'a dyn Transport,
    chat_id: i64,
    message_id: Option<i64>,
    /// Last text actually pushed to the platform (skip identical edits).
    last_text: String,
    /// When the last platform call was *attempted* (success or failure). Gates
    /// every call — so a failing send/edit can't be retried on each token.
    last_attempt: Option<tokio::time::Instant>,
    min_interval: std::time::Duration,
}

impl<'a> PlatformStream<'a> {
    fn new(transport: &'a dyn Transport, chat_id: i64) -> Self {
        Self {
            transport,
            chat_id,
            message_id: None,
            last_text: String::new(),
            last_attempt: None,
            min_interval: EDIT_INTERVAL,
        }
    }

    /// Show the complete reply: edit the streamed message to `final_text` (if it
    /// differs), or — if nothing streamed — send it as a single message.
    async fn finish(&mut self, final_text: &str) {
        match self.message_id {
            None => {
                if !final_text.is_empty() {
                    if let Err(e) = self.transport.send(self.chat_id, final_text).await {
                        eprintln!("[bwoc-connect] send failed: {e}");
                    }
                }
            }
            Some(id) => {
                if final_text != self.last_text {
                    if let Err(e) = self.transport.edit(self.chat_id, id, final_text).await {
                        eprintln!("[bwoc-connect] final edit failed: {e}");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl ReplyStream for PlatformStream<'_> {
    async fn push(&mut self, accumulated: &str) {
        if accumulated.trim().is_empty() {
            return; // nothing worth a message yet
        }
        // Debounce EVERY platform call (the initial send included), keyed on the
        // last attempt regardless of outcome — so a failing send/edit waits the
        // interval instead of being retried on every single token. The first
        // token (no prior attempt) sends immediately; `finish` still guarantees
        // the complete text at turn end.
        let now = tokio::time::Instant::now();
        if self
            .last_attempt
            .is_some_and(|t| now.duration_since(t) < self.min_interval)
        {
            return;
        }
        match self.message_id {
            None => {
                self.last_attempt = Some(now);
                match self.transport.send(self.chat_id, accumulated).await {
                    Ok(id) => {
                        self.message_id = Some(id);
                        self.last_text = accumulated.to_string();
                    }
                    Err(e) => eprintln!("[bwoc-connect] stream send failed: {e}"),
                }
            }
            Some(id) => {
                if accumulated != self.last_text {
                    self.last_attempt = Some(now);
                    if self
                        .transport
                        .edit(self.chat_id, id, accumulated)
                        .await
                        .is_ok()
                    {
                        self.last_text = accumulated.to_string();
                    }
                }
            }
        }
    }
}

/// Append a human's group message to the team's `chat.jsonl` as peer context
/// (same append-only `TeamChatMessage` shape + atomic write as HV3-3a).
fn append_peer(chat_log: &std::path::Path, from: &str, text: &str) {
    use std::io::Write;
    let Ok(line) = bwoc_core::team::TeamChatMessage::new(from, text).to_line() else {
        return;
    };
    if let Some(parent) = chat_log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(chat_log)
    {
        let _ = f.write_all(format!("{line}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[test]
    fn parse_config_and_closed_allow_list() {
        let cfg = TelegramConfig::parse("enabled = true\nallow_from = [111, 222]\n").unwrap();
        assert!(cfg.enabled);
        assert!(cfg.is_allowed(111));
        assert!(!cfg.is_allowed(333));
        // Empty/absent allow-list ⇒ nobody (closed by default).
        let empty = TelegramConfig::parse("enabled = true\n").unwrap();
        assert!(!empty.is_allowed(111));
    }

    #[test]
    fn parse_group_block() {
        let cfg = TelegramConfig::parse("enabled = true\n[group]\nteam = \"squad\"\n").unwrap();
        let g = cfg.group.unwrap();
        assert_eq!(g.team.as_deref(), Some("squad"));
        assert!(g.mention_only, "mention_only defaults true");
    }

    /// Transport that yields one scripted batch then empty, recording sends +
    /// edits. `send` returns an incrementing message id so streaming can edit.
    struct MockTransport {
        batches: Mutex<Vec<Vec<Incoming>>>,
        sent: Mutex<Vec<(i64, String)>>,
        edited: Mutex<Vec<(i64, i64, String)>>,
        next_id: std::sync::atomic::AtomicI64,
    }
    impl MockTransport {
        fn new(batches: Vec<Vec<Incoming>>) -> Self {
            Self {
                batches: Mutex::new(batches),
                sent: Mutex::new(vec![]),
                edited: Mutex::new(vec![]),
                next_id: std::sync::atomic::AtomicI64::new(1),
            }
        }
    }
    #[async_trait]
    impl Transport for MockTransport {
        async fn poll(&self, _offset: i64) -> Result<Vec<Incoming>, ConnectError> {
            Ok(self.batches.lock().unwrap().pop().unwrap_or_default())
        }
        async fn send(&self, chat_id: i64, text: &str) -> Result<i64, ConnectError> {
            self.sent.lock().unwrap().push((chat_id, text.to_string()));
            Ok(self
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst))
        }
        async fn edit(&self, chat_id: i64, mid: i64, text: &str) -> Result<(), ConnectError> {
            self.edited
                .lock()
                .unwrap()
                .push((chat_id, mid, text.to_string()));
            Ok(())
        }
    }

    /// Session that echoes its input back.
    struct EchoSession;
    #[async_trait]
    impl AgentSession for EchoSession {
        async fn ask(&mut self, text: &str) -> Result<String, ConnectError> {
            Ok(format!("echo: {text}"))
        }
    }
    /// Factory counting how many sessions it created (proves per-chat reuse).
    struct EchoFactory {
        created: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl SessionFactory for EchoFactory {
        async fn create(&self) -> Result<Box<dyn AgentSession>, ConnectError> {
            self.created
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Box::new(EchoSession))
        }
    }

    fn cfg(allow: &[i64]) -> TelegramConfig {
        TelegramConfig {
            enabled: true,
            allow_from: allow.to_vec(),
            group: None,
        }
    }

    #[tokio::test]
    async fn allow_listed_message_gets_an_echoed_reply() {
        let t = MockTransport::new(vec![vec![Incoming {
            update_id: 5,
            chat_id: 42,
            from_user_id: 111,
            text: "hi".into(),

            is_group: false,
            mentions_bot: false,
        }]]);
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = EchoFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, None, &cfg(&[111]), Some(1))
            .await
            .unwrap();
        assert_eq!(
            t.sent.lock().unwrap().as_slice(),
            &[(42, "echo: hi".to_string())]
        );
        assert_eq!(created.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_allow_listed_message_is_ignored() {
        let t = MockTransport::new(vec![vec![Incoming {
            update_id: 1,
            chat_id: 9,
            from_user_id: 999, // not allowed
            text: "spam".into(),

            is_group: false,
            mentions_bot: false,
        }]]);
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = EchoFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, None, &cfg(&[111]), Some(1))
            .await
            .unwrap();
        assert!(t.sent.lock().unwrap().is_empty(), "no reply to a stranger");
        assert_eq!(
            created.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no session spawned"
        );
    }

    /// A session that always errors (simulates a harness that died).
    struct DeadSession;
    #[async_trait]
    impl AgentSession for DeadSession {
        async fn ask(&mut self, _text: &str) -> Result<String, ConnectError> {
            Err(ConnectError::Session("harness died".into()))
        }
    }
    /// First `create()` yields a dead session, later ones a working echo —
    /// models "the first subprocess died, the respawn is healthy".
    struct FlakyFactory {
        created: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl SessionFactory for FlakyFactory {
        async fn create(&self) -> Result<Box<dyn AgentSession>, ConnectError> {
            let n = self
                .created
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                Ok(Box::new(DeadSession))
            } else {
                Ok(Box::new(EchoSession))
            }
        }
    }

    #[tokio::test]
    async fn dead_session_is_dropped_and_respawned() {
        // Two polls, each delivering one message to the same chat. First ask
        // errors (session removed); second poll respawns + succeeds.
        let t = MockTransport::new(vec![
            vec![Incoming {
                update_id: 2,
                chat_id: 7,
                from_user_id: 111,
                text: "b".into(),

                is_group: false,
                mentions_bot: false,
            }],
            vec![Incoming {
                update_id: 1,
                chat_id: 7,
                from_user_id: 111,
                text: "a".into(),

                is_group: false,
                mentions_bot: false,
            }],
        ]);
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = FlakyFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, None, &cfg(&[111]), Some(2))
            .await
            .unwrap();
        // Two sessions created (one per message, because the first died).
        assert_eq!(created.load(std::sync::atomic::Ordering::SeqCst), 2);
        let sent = t.sent.lock().unwrap();
        assert!(
            sent.iter().any(|(_, m)| m.contains("agent error")),
            "1st msg errored"
        );
        assert!(
            sent.iter().any(|(_, m)| m == "echo: b"),
            "2nd msg recovered via respawn"
        );
    }

    #[tokio::test]
    async fn two_messages_same_chat_reuse_one_session() {
        let t = MockTransport::new(vec![vec![
            Incoming {
                update_id: 1,
                chat_id: 7,
                from_user_id: 111,
                text: "a".into(),

                is_group: false,
                mentions_bot: false,
            },
            Incoming {
                update_id: 2,
                chat_id: 7,
                from_user_id: 111,
                text: "b".into(),

                is_group: false,
                mentions_bot: false,
            },
        ]]);
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = EchoFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, None, &cfg(&[111]), Some(1))
            .await
            .unwrap();
        assert_eq!(t.sent.lock().unwrap().len(), 2);
        assert_eq!(
            created.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one session reused across the DM"
        );
    }

    // ── Group ⇄ team chat (PR2) ──────────────────────────────────────────────

    fn group_cfg(allow: &[i64], mention_only: bool) -> TelegramConfig {
        TelegramConfig {
            enabled: true,
            allow_from: allow.to_vec(),
            group: Some(GroupConfig {
                team: Some("squad".into()),
                mention_only,
            }),
        }
    }

    fn group_msg(update_id: i64, user: i64, text: &str, mentions: bool) -> Incoming {
        Incoming {
            update_id,
            chat_id: -100,
            from_user_id: user,
            text: text.into(),
            is_group: true,
            mentions_bot: mentions,
        }
    }

    fn ctr() -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0))
    }

    #[tokio::test]
    async fn group_mention_serves_via_team_session() {
        let tmp = TempDir::new().unwrap();
        let log = tmp.path().join("chat.jsonl");
        let t = MockTransport::new(vec![vec![group_msg(1, 111, "@bot hi", true)]]);
        let gcreated = ctr();
        let gf = EchoFactory {
            created: gcreated.clone(),
        };
        let dmf = EchoFactory { created: ctr() };
        let gb = GroupBridge {
            factory: &gf,
            chat_log: log.clone(),
            peer_prefix: "tg".into(),
        };
        run_bridge(&t, &dmf, Some(gb), &group_cfg(&[111], true), Some(1))
            .await
            .unwrap();
        assert_eq!(
            t.sent.lock().unwrap().as_slice(),
            &[(-100, "echo: @bot hi".to_string())]
        );
        assert_eq!(
            gcreated.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "group session served it"
        );
    }

    #[tokio::test]
    async fn group_non_mention_logs_peer_and_does_not_reply() {
        let tmp = TempDir::new().unwrap();
        let log = tmp.path().join("teams/squad/chat.jsonl"); // parent auto-created
        let t = MockTransport::new(vec![vec![group_msg(1, 111, "hi team", false)]]);
        let gcreated = ctr();
        let gf = EchoFactory {
            created: gcreated.clone(),
        };
        let dmf = EchoFactory { created: ctr() };
        let gb = GroupBridge {
            factory: &gf,
            chat_log: log.clone(),
            peer_prefix: "tg".into(),
        };
        run_bridge(&t, &dmf, Some(gb), &group_cfg(&[111], true), Some(1))
            .await
            .unwrap();
        // No reply, no session — just a peer line appended for context.
        assert!(t.sent.lock().unwrap().is_empty(), "mention-gated: no reply");
        assert_eq!(gcreated.load(std::sync::atomic::Ordering::SeqCst), 0);
        let body = std::fs::read_to_string(&log).unwrap();
        assert!(body.contains("\"from\":\"tg:111\""), "peer logged: {body}");
        assert!(body.contains("hi team"));
    }

    #[tokio::test]
    async fn group_message_without_binding_is_ignored() {
        let t = MockTransport::new(vec![vec![group_msg(1, 111, "@bot hi", true)]]);
        let dmf = EchoFactory { created: ctr() };
        // group = None → group messages ignored even when allow-listed + mention.
        run_bridge(&t, &dmf, None, &group_cfg(&[111], true), Some(1))
            .await
            .unwrap();
        assert!(t.sent.lock().unwrap().is_empty(), "no binding ⇒ ignored");
    }

    // ── Streaming: send placeholder, edit in place (PR streaming) ────────────

    /// A session that streams a growing reply, then returns the full text.
    struct StreamSession;
    #[async_trait]
    impl AgentSession for StreamSession {
        async fn ask(&mut self, text: &str) -> Result<String, ConnectError> {
            Ok(format!("echo: {text}"))
        }
        async fn ask_streamed(
            &mut self,
            text: &str,
            sink: &mut dyn ReplyStream,
        ) -> Result<String, ConnectError> {
            let full = format!("echo: {text}");
            sink.push("e").await; // first token → placeholder send
            sink.push(&full).await; // more (debounced by the 1s interval)
            Ok(full)
        }
    }
    struct StreamFactory;
    #[async_trait]
    impl SessionFactory for StreamFactory {
        async fn create(&self) -> Result<Box<dyn AgentSession>, ConnectError> {
            Ok(Box::new(StreamSession))
        }
    }

    #[tokio::test]
    async fn platform_stream_sends_placeholder_then_edits_in_place() {
        let t = MockTransport::new(vec![]);
        let mut s = PlatformStream::new(&t, 42);
        s.min_interval = std::time::Duration::ZERO; // edit on every push (deterministic)
        s.push("ab").await; // first non-empty → send placeholder
        s.push("abcd").await; // → edit
        s.push("abcdef").await; // → edit
        s.finish("abcdef").await; // identical to last → no extra edit
        assert_eq!(t.sent.lock().unwrap().as_slice(), &[(42, "ab".to_string())]);
        let edited = t.edited.lock().unwrap();
        assert_eq!(edited.len(), 2);
        assert_eq!(edited[0].2, "abcd");
        assert_eq!(edited[1].2, "abcdef");
    }

    #[tokio::test]
    async fn platform_stream_no_tokens_sends_once() {
        let t = MockTransport::new(vec![]);
        let mut s = PlatformStream::new(&t, 7);
        s.finish("done").await; // nothing streamed → single send, no edits
        assert_eq!(
            t.sent.lock().unwrap().as_slice(),
            &[(7, "done".to_string())]
        );
        assert!(t.edited.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn platform_stream_skips_blank_pushes() {
        let t = MockTransport::new(vec![]);
        let mut s = PlatformStream::new(&t, 1);
        s.push("   ").await; // whitespace-only → no message yet
        assert!(t.sent.lock().unwrap().is_empty());
        s.finish("real").await;
        assert_eq!(
            t.sent.lock().unwrap().as_slice(),
            &[(1, "real".to_string())]
        );
    }

    #[tokio::test]
    async fn streaming_session_sends_placeholder_and_final_edit() {
        let t = MockTransport::new(vec![vec![Incoming {
            update_id: 1,
            chat_id: 5,
            from_user_id: 111,
            text: "hi".into(),
            is_group: false,
            mentions_bot: false,
        }]]);
        let f = StreamFactory;
        run_bridge(&t, &f, None, &cfg(&[111]), Some(1))
            .await
            .unwrap();
        // Placeholder "e" sent; the complete reply lands via the final edit
        // (the intermediate edit is skipped by the 1s debounce — pushes are
        // microseconds apart).
        assert_eq!(t.sent.lock().unwrap().as_slice(), &[(5, "e".to_string())]);
        let edited = t.edited.lock().unwrap();
        assert!(!edited.is_empty(), "final edit fired");
        assert_eq!(
            edited.last().unwrap().2,
            "echo: hi",
            "final edit shows the full reply"
        );
    }
}
