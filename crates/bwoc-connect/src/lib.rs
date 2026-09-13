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
pub mod imessage;
pub mod line;
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

/// Highest connector-config `schema_version` this build understands. An
/// unmarked file reads as this revision (every connector file written so far).
pub const CONNECTOR_SCHEMA_VERSION: u32 = 2;

fn default_schema_version() -> u32 {
    CONNECTOR_SCHEMA_VERSION
}

/// Per-agent Telegram connector config. The token is **not** here — it
/// resolves via the keyring / env fallback (see `main`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TelegramConfig {
    /// Format revision. Absent ⇒ [`CONNECTOR_SCHEMA_VERSION`]; a revision ahead
    /// of this build is refused by [`TelegramConfig::parse`] (fail closed).
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Connector is off unless explicitly enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Platform user ids permitted to reach the agent. **Empty ⇒ nobody**
    /// (closed by default; `[bot] public = true` opts into limited public).
    #[serde(default)]
    pub allow_from: Vec<i64>,
    /// Optional bot surface: fixed slash-command replies, per-sender rate and
    /// length caps, and the limited-public opt-in. Absent ⇒ Phase 0 behaviour.
    #[serde(default)]
    pub bot: Option<BotConfig>,
    /// Group binding (PR2). Parsed now so the config shape is stable; unused
    /// in PR1's DM-only path.
    #[serde(default)]
    pub group: Option<GroupConfig>,
    /// LINE-only block (webhook bind + the LINE user-id allow-list). LINE ids
    /// are strings, so its allow-list lives here and `main` hashes it into
    /// `allow_from` (see `line::hash_id`).
    #[serde(default)]
    pub line: Option<LineConfig>,
    /// iMessage-only block (#229). iMessage handles are strings (phone/email),
    /// so its allow-list lives here and `main` hashes it into `allow_from` (see
    /// `imessage::hash_id`), the same pattern as LINE.
    #[serde(default)]
    pub imessage: Option<ImessageConfig>,
}

/// iMessage connector config (`connectors/imessage.toml`, under `[imessage]`).
/// macOS-only — there is no server iMessage API.
#[derive(Debug, Clone, Deserialize)]
pub struct ImessageConfig {
    /// Handles (phone `+1…` / email) permitted to reach the agent. **Empty ⇒
    /// nobody** (closed by default).
    #[serde(default)]
    pub allow_handles: Vec<String>,
    /// Path to the Messages SQLite DB. Defaults to the standard per-user
    /// location; overridable for tests / non-default homes.
    #[serde(default = "default_imessage_db")]
    pub db_path: String,
    /// Seconds between `chat.db` reads (no long-poll for a local file).
    #[serde(default)]
    pub poll_interval_secs: Option<u64>,
}

fn default_imessage_db() -> String {
    // `~/Library/Messages/chat.db`. `~` is expanded by `main` against $HOME.
    "~/Library/Messages/chat.db".to_string()
}

/// LINE connector config (`connectors/line.toml`, under `[line]`).
#[derive(Debug, Clone, Deserialize)]
pub struct LineConfig {
    /// LINE user ids (`U…`) permitted to reach the agent. **Empty ⇒ nobody.**
    #[serde(default)]
    pub allow_user_ids: Vec<String>,
    /// Address the inbound webhook server binds (put an HTTPS proxy in front).
    #[serde(default = "default_line_bind")]
    pub bind: String,
    /// Webhook path registered with the LINE channel.
    #[serde(default = "default_line_path")]
    pub path: String,
}

fn default_line_bind() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_line_path() -> String {
    "/webhook".to_string()
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

/// Per-sender message cap when `rate_limit_per_min` is unset.
pub const DEFAULT_RATE_LIMIT_PER_MIN: u32 = 20;
/// Input length cap (characters) when `max_input_chars` is unset.
pub const DEFAULT_MAX_INPUT_CHARS: usize = 4000;

/// The `[bot]` table. One agent = one bot; the agent's `AGENTS.md` is the
/// persona, so there is deliberately no persona field here.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BotConfig {
    /// Slash command → fixed reply text. Exact first-token match only (a
    /// Telegram `@botname` suffix is stripped); no templating, no execution,
    /// and a matched command never reaches the agent.
    #[serde(default)]
    pub commands: std::collections::BTreeMap<String, String>,
    /// Messages per sender per minute. Unset ⇒ [`DEFAULT_RATE_LIMIT_PER_MIN`];
    /// `0` ⇒ uncapped for allow-listed senders only.
    #[serde(default)]
    pub rate_limit_per_min: Option<u32>,
    /// Longest forwarded message, in characters. Unset ⇒
    /// [`DEFAULT_MAX_INPUT_CHARS`]; `0` ⇒ uncapped for allow-listed senders only.
    #[serde(default)]
    pub max_input_chars: Option<usize>,
    /// Limited public mode: non-allow-listed senders may reach the agent in a
    /// DM or by @mentioning the bot in a group, on a read-only session.
    #[serde(default)]
    pub public: bool,
}

impl BotConfig {
    /// The fixed reply for a message whose first token is a configured command.
    pub fn command_reply(&self, text: &str) -> Option<&str> {
        let first = text.split_whitespace().next()?;
        if !first.starts_with('/') {
            return None;
        }
        // Telegram appends `@botname` to commands sent in groups.
        let key = first.split_once('@').map_or(first, |(cmd, _)| cmd);
        self.commands.get(key).map(String::as_str)
    }

    /// Effective rate cap; `None` = uncapped.
    fn rate_limit(&self, public: bool) -> Option<u32> {
        effective_cap(self.rate_limit_per_min, DEFAULT_RATE_LIMIT_PER_MIN, public)
    }

    /// Effective length cap; `None` = uncapped.
    fn max_chars(&self, public: bool) -> Option<usize> {
        effective_cap(self.max_input_chars, DEFAULT_MAX_INPUT_CHARS, public)
    }
}

/// Unset ⇒ default. `0` disables the cap, except for a public sender: a public
/// bot is never uncapped, so `0` falls back to the default there.
fn effective_cap<T: Copy + Default + PartialEq>(
    set: Option<T>,
    default: T,
    public: bool,
) -> Option<T> {
    match set {
        None => Some(default),
        Some(v) if v == T::default() => public.then_some(default),
        Some(v) => Some(v),
    }
}

/// The connector config shape is platform-agnostic (enabled / allow_from /
/// group), so Discord reuses it from `connectors/discord.toml`.
pub type ConnectorConfig = TelegramConfig;

impl TelegramConfig {
    /// Parse a connector config body (`telegram.toml` / `discord.toml`).
    ///
    /// A `schema_version` ahead of this build is refused: a newer revision may
    /// have tightened a key this build would silently ignore, and this file
    /// decides who reaches the agent.
    pub fn parse(toml_src: &str) -> Result<Self, ConnectError> {
        let cfg: Self =
            toml::from_str(toml_src).map_err(|e| ConnectError::Config(e.to_string()))?;
        if cfg.schema_version > CONNECTOR_SCHEMA_VERSION {
            return Err(ConnectError::Config(format!(
                "connector config declares schema_version {} but this bwoc-connect understands \
                 at most {CONNECTOR_SCHEMA_VERSION} — refusing to start a connector written for a \
                 newer BWOC (upgrade bwoc)",
                cfg.schema_version
            )));
        }
        Ok(cfg)
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

    /// Whether this transport can edit a sent message — i.e. supports streaming
    /// via send-then-edit. Default `true` (Telegram/Discord); platforms with no
    /// edit API (LINE) override to `false`, and the bridge then sends the reply
    /// once on turn end instead of streaming.
    fn supports_edit(&self) -> bool {
        true
    }
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
    /// Deliver a user message from platform user `from_user_id`; return the
    /// agent's final reply text.
    async fn ask(&mut self, text: &str, from_user_id: i64) -> Result<String, ConnectError>;

    /// Like [`ask`](Self::ask) but pushing the accumulated reply to `sink` as
    /// tokens stream in. The default delegates to `ask` (no streaming — one
    /// send on finish); `HarnessSession` overrides it to relay `chat_proto`
    /// `Token` events.
    async fn ask_streamed(
        &mut self,
        text: &str,
        from_user_id: i64,
        sink: &mut dyn ReplyStream,
    ) -> Result<String, ConnectError> {
        let _ = sink;
        self.ask(text, from_user_id).await
    }
}

/// Makes a fresh [`AgentSession`] per conversation (lazily, on first message).
/// `(chat_id, public)` keys the conversation, so an impl can isolate per-chat
/// state. `public` marks a limited-public (non-allow-listed) sender: the impl
/// must keep that session apart from the chat's allow-listed one and restrict
/// it to read-only tools.
#[async_trait]
pub trait SessionFactory: Send + Sync {
    async fn create(
        &self,
        chat_id: i64,
        public: bool,
    ) -> Result<Box<dyn AgentSession>, ConnectError>;
}

/// Sliding window for [`RateLimiter`].
const RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
/// Sender-map size that triggers pruning idle senders (bounds memory under a
/// flood of distinct public senders).
const RATE_PRUNE_AT: usize = 4096;
const RATE_NOTICE: &str = "⏳ Too many messages — please wait a minute.";

#[derive(Debug, PartialEq, Eq)]
enum RateVerdict {
    Allow,
    /// Over the cap for the first time this window: send one notice.
    Notice,
    /// Still over the cap after the notice: drop silently.
    Drop,
}

/// In-memory per-sender sliding window (no persistence). `now` is passed in so
/// tests drive the clock without sleeping.
#[derive(Default)]
struct RateLimiter {
    senders: HashMap<i64, SenderWindow>,
}

#[derive(Default)]
struct SenderWindow {
    hits: std::collections::VecDeque<std::time::Instant>,
    notified: bool,
}

impl RateLimiter {
    fn check(&mut self, sender: i64, limit: u32, now: std::time::Instant) -> RateVerdict {
        if self.senders.len() >= RATE_PRUNE_AT {
            self.senders.retain(|_, w| {
                w.hits
                    .back()
                    .is_some_and(|t| now.duration_since(*t) < RATE_WINDOW)
            });
        }
        let w = self.senders.entry(sender).or_default();
        while w
            .hits
            .front()
            .is_some_and(|t| now.duration_since(*t) >= RATE_WINDOW)
        {
            w.hits.pop_front();
        }
        if w.hits.len() < limit as usize {
            w.hits.push_back(now);
            w.notified = false;
            RateVerdict::Allow
        } else if !w.notified {
            w.notified = true;
            RateVerdict::Notice
        } else {
            RateVerdict::Drop
        }
    }
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
    let mut sessions: HashMap<(i64, bool), Box<dyn AgentSession>> = HashMap::new();
    let mut limiter = RateLimiter::default();
    let bot = config.bot.as_ref();
    let mut polls = 0usize;
    let mut announced = false;
    let mention_only = config.group.as_ref().is_none_or(|g| g.mention_only);

    eprintln!("[bwoc-connect] poll loop started (offset {offset}); awaiting messages");

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

        // First successful poll confirms the loop is live — the #305 failure mode
        // was a silent never-poll, so make "is it polling?" answerable.
        if !announced {
            eprintln!("[bwoc-connect] polling active (first getUpdates returned ok)");
            announced = true;
        }
        if !messages.is_empty() {
            eprintln!("[bwoc-connect] drained {} message(s)", messages.len());
        }

        for msg in messages {
            offset = offset.max(msg.update_id + 1);

            let allowed = config.is_allowed(msg.from_user_id);
            // Limited public: a non-allow-listed sender passes only when the
            // operator opted in with `[bot] public = true`.
            let public = !allowed && bot.is_some_and(|b| b.public);
            if !allowed && !public {
                eprintln!(
                    "[bwoc-connect] ignoring message from non-allow-listed user {}",
                    msg.from_user_id
                );
                continue;
            }

            let factory: &dyn SessionFactory = if public {
                // A public sender must address the bot: a DM, or a group
                // @mention. Unaddressed group chatter is dropped, never logged.
                if msg.is_group && !msg.mentions_bot {
                    continue;
                }
                // Always the solo factory — a --team-chat session would inject
                // the room's allow-listed peer context into a stranger's turn.
                dm_factory
            } else if msg.is_group {
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
                gb.factory
            } else {
                dm_factory
            };

            if let Some(b) = bot {
                if !admit(b, &mut limiter, transport, &msg, public).await {
                    continue;
                }
            }
            serve_turn(&mut sessions, factory, transport, &msg, public).await;
        }
    }
}

/// `[bot]` checks before any model/session: rate cap (one notice, then silent
/// drops), fixed command replies, input length cap. `true` ⇒ forward to the
/// agent.
async fn admit(
    bot: &BotConfig,
    limiter: &mut RateLimiter,
    transport: &dyn Transport,
    msg: &Incoming,
    public: bool,
) -> bool {
    if let Some(limit) = bot.rate_limit(public) {
        match limiter.check(msg.from_user_id, limit, std::time::Instant::now()) {
            RateVerdict::Allow => {}
            RateVerdict::Notice => {
                let _ = transport.send(msg.chat_id, RATE_NOTICE).await;
                return false;
            }
            RateVerdict::Drop => return false,
        }
    }
    if let Some(reply) = bot.command_reply(&msg.text) {
        let _ = transport.send(msg.chat_id, reply).await;
        return false;
    }
    if let Some(max) = bot.max_chars(public) {
        if msg.text.chars().count() > max {
            let _ = transport
                .send(
                    msg.chat_id,
                    &format!("✂️ Message too long (max {max} characters)."),
                )
                .await;
            return false;
        }
    }
    true
}

/// Spawn-or-reuse the `(chat_id, public)` session, deliver `text`, relay the
/// reply. On an agent error the (likely dead) session is dropped so the next
/// message respawns a fresh one rather than failing forever.
async fn serve_turn(
    sessions: &mut HashMap<(i64, bool), Box<dyn AgentSession>>,
    factory: &dyn SessionFactory,
    transport: &dyn Transport,
    msg: &Incoming,
    public: bool,
) {
    let chat_id = msg.chat_id;
    let key = (chat_id, public);
    if let std::collections::hash_map::Entry::Vacant(slot) = sessions.entry(key) {
        match factory.create(chat_id, public).await {
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
    let session = sessions.get_mut(&key).expect("inserted above");
    let mut stream = PlatformStream::new(transport, chat_id);
    match session
        .ask_streamed(&msg.text, msg.from_user_id, &mut stream)
        .await
    {
        Ok(reply) => {
            // Ensure the platform shows the complete reply: a final edit (or a
            // single send if no tokens streamed). Errors are already logged
            // inside the stream; finish swallows the no-op-identical case.
            stream.finish(&reply).await;
        }
        Err(e) => {
            eprintln!("[bwoc-connect] agent error: {e}");
            sessions.remove(&key);
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
    /// Whether the transport can edit — if not, stream nothing and let `finish`
    /// send the whole reply once (LINE).
    can_edit: bool,
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
            can_edit: transport.supports_edit(),
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
        if !self.can_edit || accumulated.trim().is_empty() {
            // No edit API (or nothing yet) ⇒ don't stream; `finish` sends once.
            return;
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
        async fn ask(&mut self, text: &str, _from: i64) -> Result<String, ConnectError> {
            Ok(format!("echo: {text}"))
        }
    }
    /// Factory counting how many sessions it created (proves per-chat reuse).
    struct EchoFactory {
        created: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl SessionFactory for EchoFactory {
        async fn create(
            &self,
            _chat_id: i64,
            _public: bool,
        ) -> Result<Box<dyn AgentSession>, ConnectError> {
            self.created
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Box::new(EchoSession))
        }
    }

    fn cfg(allow: &[i64]) -> TelegramConfig {
        TelegramConfig {
            schema_version: CONNECTOR_SCHEMA_VERSION,
            enabled: true,
            allow_from: allow.to_vec(),
            bot: None,
            group: None,
            line: None,
            imessage: None,
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
        async fn ask(&mut self, _text: &str, _from: i64) -> Result<String, ConnectError> {
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
        async fn create(
            &self,
            _chat_id: i64,
            _public: bool,
        ) -> Result<Box<dyn AgentSession>, ConnectError> {
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
            schema_version: CONNECTOR_SCHEMA_VERSION,
            enabled: true,
            allow_from: allow.to_vec(),
            bot: None,
            group: Some(GroupConfig {
                team: Some("squad".into()),
                mention_only,
            }),
            line: None,
            imessage: None,
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
        async fn ask(&mut self, text: &str, _from: i64) -> Result<String, ConnectError> {
            Ok(format!("echo: {text}"))
        }
        async fn ask_streamed(
            &mut self,
            text: &str,
            _from: i64,
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
        async fn create(
            &self,
            _chat_id: i64,
            _public: bool,
        ) -> Result<Box<dyn AgentSession>, ConnectError> {
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

    /// A transport that can't edit (LINE) — streaming must collapse to one send.
    struct NoEditTransport {
        sent: Mutex<Vec<(i64, String)>>,
        edited: Mutex<Vec<(i64, i64, String)>>,
    }
    #[async_trait]
    impl Transport for NoEditTransport {
        async fn poll(&self, _offset: i64) -> Result<Vec<Incoming>, ConnectError> {
            Ok(vec![])
        }
        async fn send(&self, chat_id: i64, text: &str) -> Result<i64, ConnectError> {
            self.sent.lock().unwrap().push((chat_id, text.to_string()));
            Ok(1)
        }
        async fn edit(&self, chat_id: i64, mid: i64, text: &str) -> Result<(), ConnectError> {
            self.edited
                .lock()
                .unwrap()
                .push((chat_id, mid, text.to_string()));
            Ok(())
        }
        fn supports_edit(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn non_editing_transport_sends_once_no_edits() {
        let t = NoEditTransport {
            sent: Mutex::new(vec![]),
            edited: Mutex::new(vec![]),
        };
        let mut s = PlatformStream::new(&t, 9);
        s.push("partial").await; // can't edit → ignored
        s.push("partial more").await; // ignored
        s.finish("partial more done").await; // single send of the full reply
        assert_eq!(
            t.sent.lock().unwrap().as_slice(),
            &[(9, "partial more done".to_string())]
        );
        assert!(t.edited.lock().unwrap().is_empty(), "never edits");
    }

    // ── bwoc-bot Phase 1: schema_version + [bot] ─────────────────────────────

    #[test]
    fn schema_version_absent_is_accepted_future_is_refused() {
        let absent = TelegramConfig::parse("enabled = true\n").unwrap();
        assert_eq!(absent.schema_version, CONNECTOR_SCHEMA_VERSION);
        assert!(absent.bot.is_none(), "no [bot] table ⇒ None");
        TelegramConfig::parse("schema_version = 2\nenabled = true\n").unwrap();
        let err = TelegramConfig::parse("schema_version = 3\nenabled = true\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("schema_version"), "{err}");
    }

    /// Records every `create(chat_id, public)` call; sessions echo.
    #[derive(Default)]
    struct RecFactory {
        calls: Mutex<Vec<(i64, bool)>>,
    }
    #[async_trait]
    impl SessionFactory for RecFactory {
        async fn create(
            &self,
            chat_id: i64,
            public: bool,
        ) -> Result<Box<dyn AgentSession>, ConnectError> {
            self.calls.lock().unwrap().push((chat_id, public));
            Ok(Box::new(EchoSession))
        }
    }

    fn bot_cfg(allow: &[i64], bot_toml: &str) -> TelegramConfig {
        let mut c = cfg(allow);
        c.bot = Some(toml::from_str(bot_toml).unwrap());
        c
    }

    fn dm(update_id: i64, user: i64, text: &str) -> Incoming {
        Incoming {
            update_id,
            chat_id: user,
            from_user_id: user,
            text: text.into(),
            is_group: false,
            mentions_bot: false,
        }
    }

    fn sent(t: &MockTransport) -> Vec<(i64, String)> {
        t.sent.lock().unwrap().clone()
    }

    #[test]
    fn command_reply_matches_first_token_and_strips_bot_suffix() {
        let b: BotConfig =
            toml::from_str("[commands]\n\"/start\" = \"welcome\"\n\"/help\" = \"usage\"\n")
                .unwrap();
        assert_eq!(b.command_reply("/start"), Some("welcome"));
        assert_eq!(b.command_reply("/help@my_bot"), Some("usage"));
        assert_eq!(b.command_reply("  /help@my_bot please"), Some("usage"));
        assert_eq!(b.command_reply("/helpme"), None, "exact keys only");
        assert_eq!(b.command_reply("say /start"), None, "first token only");
        assert_eq!(b.command_reply("/unknown"), None);
        assert_eq!(b.command_reply(""), None);
    }

    #[tokio::test]
    async fn command_short_circuits_the_agent() {
        let t = MockTransport::new(vec![vec![dm(1, 111, "/start@my_bot")]]);
        let f = RecFactory::default();
        let c = bot_cfg(&[111], "[commands]\n\"/start\" = \"welcome\"\n");
        run_bridge(&t, &f, None, &c, Some(1)).await.unwrap();
        assert_eq!(sent(&t), vec![(111, "welcome".to_string())]);
        assert!(f.calls.lock().unwrap().is_empty(), "agent never called");
    }

    #[tokio::test]
    async fn without_bot_table_a_command_reaches_the_agent_as_before() {
        let t = MockTransport::new(vec![vec![dm(1, 111, "/start")]]);
        let f = RecFactory::default();
        run_bridge(&t, &f, None, &cfg(&[111]), Some(1))
            .await
            .unwrap();
        assert_eq!(sent(&t), vec![(111, "echo: /start".to_string())]);
        assert_eq!(f.calls.lock().unwrap().as_slice(), &[(111, false)]);
    }

    #[test]
    fn rate_limiter_notices_once_then_drops_until_the_window_frees() {
        let mut rl = RateLimiter::default();
        let t0 = std::time::Instant::now();
        let s = std::time::Duration::from_secs;
        assert_eq!(rl.check(7, 2, t0), RateVerdict::Allow);
        assert_eq!(rl.check(7, 2, t0 + s(1)), RateVerdict::Allow);
        assert_eq!(rl.check(7, 2, t0 + s(2)), RateVerdict::Notice);
        assert_eq!(rl.check(7, 2, t0 + s(3)), RateVerdict::Drop);
        assert_eq!(rl.check(8, 2, t0 + s(3)), RateVerdict::Allow, "per sender");
        // t0's hit ages out → one slot frees; the notice re-arms.
        assert_eq!(rl.check(7, 2, t0 + s(60)), RateVerdict::Allow);
        assert_eq!(rl.check(7, 2, t0 + s(60)), RateVerdict::Notice);
    }

    #[tokio::test]
    async fn rate_limit_sends_one_notice_and_drops_the_rest() {
        let batch = (1..=5).map(|i| dm(i, 111, "hi")).collect();
        let t = MockTransport::new(vec![batch]);
        let f = RecFactory::default();
        let c = bot_cfg(&[111], "rate_limit_per_min = 2\n");
        run_bridge(&t, &f, None, &c, Some(1)).await.unwrap();
        let out = sent(&t);
        assert_eq!(out.iter().filter(|(_, m)| m == "echo: hi").count(), 2);
        assert_eq!(out.iter().filter(|(_, m)| m == RATE_NOTICE).count(), 1);
        assert_eq!(out.len(), 3, "messages 4 and 5 dropped silently");
    }

    #[tokio::test]
    async fn over_length_message_gets_a_notice_and_is_not_forwarded() {
        let t = MockTransport::new(vec![vec![dm(1, 111, "123456"), dm(2, 111, "12345")]]);
        let f = RecFactory::default();
        let c = bot_cfg(&[111], "max_input_chars = 5\n");
        run_bridge(&t, &f, None, &c, Some(1)).await.unwrap();
        let out = sent(&t);
        assert_eq!(out.len(), 2);
        assert!(out[0].1.contains("too long"), "{out:?}");
        assert_eq!(out[1].1, "echo: 12345");
    }

    #[test]
    fn public_senders_are_always_capped_even_when_set_to_zero() {
        let b: BotConfig =
            toml::from_str("public = true\nrate_limit_per_min = 0\nmax_input_chars = 0\n").unwrap();
        assert_eq!(b.rate_limit(false), None, "0 uncaps allow-listed");
        assert_eq!(b.max_chars(false), None);
        assert_eq!(b.rate_limit(true), Some(DEFAULT_RATE_LIMIT_PER_MIN));
        assert_eq!(b.max_chars(true), Some(DEFAULT_MAX_INPUT_CHARS));
        let unset = BotConfig::default();
        assert_eq!(unset.rate_limit(false), Some(DEFAULT_RATE_LIMIT_PER_MIN));
        assert_eq!(unset.max_chars(false), Some(DEFAULT_MAX_INPUT_CHARS));
    }

    #[tokio::test]
    async fn public_false_still_ignores_strangers() {
        let t = MockTransport::new(vec![vec![dm(1, 999, "/start"), dm(2, 999, "hi")]]);
        let f = RecFactory::default();
        let c = bot_cfg(
            &[111],
            "public = false\n[commands]\n\"/start\" = \"welcome\"\n",
        );
        run_bridge(&t, &f, None, &c, Some(1)).await.unwrap();
        assert!(sent(&t).is_empty(), "no command reply, no agent reply");
        assert!(f.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn public_serves_stranger_dm_and_mention_but_not_group_chatter() {
        let t = MockTransport::new(vec![vec![
            dm(1, 999, "hello"),
            group_msg(2, 998, "@bot hi", true),
            group_msg(3, 997, "just chatting", false),
        ]]);
        let f = RecFactory::default();
        let c = bot_cfg(&[111], "public = true\n");
        run_bridge(&t, &f, None, &c, Some(1)).await.unwrap();
        assert_eq!(
            sent(&t),
            vec![
                (999, "echo: hello".to_string()),
                (-100, "echo: @bot hi".to_string())
            ]
        );
        // Both public turns are created with the read-only (`public`) marker.
        assert_eq!(
            f.calls.lock().unwrap().as_slice(),
            &[(999, true), (-100, true)]
        );
    }

    #[tokio::test]
    async fn public_and_allow_listed_senders_in_one_group_never_share_a_session() {
        let tmp = TempDir::new().unwrap();
        let t = MockTransport::new(vec![vec![
            group_msg(1, 111, "@bot from member", true),
            group_msg(2, 999, "@bot from stranger", true),
            group_msg(3, 999, "stranger chatter", false),
        ]]);
        let dmf = RecFactory::default();
        let gf = RecFactory::default();
        let log = tmp.path().join("chat.jsonl");
        let gb = GroupBridge {
            factory: &gf,
            chat_log: log.clone(),
            peer_prefix: "tg".into(),
        };
        let mut c = group_cfg(&[111], true);
        c.bot = Some(toml::from_str("public = true\n").unwrap());
        run_bridge(&t, &dmf, Some(gb), &c, Some(1)).await.unwrap();
        // Member → the team-chat session; stranger → a separate public solo one.
        assert_eq!(gf.calls.lock().unwrap().as_slice(), &[(-100, false)]);
        assert_eq!(dmf.calls.lock().unwrap().as_slice(), &[(-100, true)]);
        assert_eq!(sent(&t).len(), 2);
        // Stranger chatter is dropped, not logged into the team's chat.
        assert!(!log.exists(), "public chatter never reaches the team log");
    }
}
