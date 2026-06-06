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

pub mod session;
pub mod telegram;

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
    #[error("no bot token: set it in the keyring (bwoc/telegram · <agent>) or {0}")]
    NoToken(String),
}

// ---------------------------------------------------------------------------
// Config — .bwoc/connectors/telegram.toml
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

impl TelegramConfig {
    /// Parse a `telegram.toml` body.
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
}

/// A platform transport: long-poll for messages, send replies.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Fetch messages with `update_id >= offset`. May block up to the
    /// transport's long-poll timeout, returning `[]` when nothing arrives.
    async fn poll(&self, offset: i64) -> Result<Vec<Incoming>, ConnectError>;
    /// Send `text` to `chat_id`.
    async fn send(&self, chat_id: i64, text: &str) -> Result<(), ConnectError>;
}

/// One agent conversation (a `bwoc-harness --chat` subprocess in PR1).
#[async_trait]
pub trait AgentSession: Send {
    /// Deliver a user message; return the agent's final reply text.
    async fn ask(&mut self, text: &str) -> Result<String, ConnectError>;
}

/// Makes a fresh [`AgentSession`] per conversation (lazily, on first message).
#[async_trait]
pub trait SessionFactory: Send + Sync {
    async fn create(&self) -> Result<Box<dyn AgentSession>, ConnectError>;
}

// ---------------------------------------------------------------------------
// Bridge loop (the testable core)
// ---------------------------------------------------------------------------

/// Run the relay: poll → allow-list filter → per-chat session → reply.
///
/// One `AgentSession` is held per `chat_id` (DM continuity). `max_polls`
/// bounds the loop for tests (`None` = run forever). Per-message errors are
/// logged and skipped — one bad message never tears the bridge down.
pub async fn run_bridge(
    transport: &dyn Transport,
    factory: &dyn SessionFactory,
    config: &TelegramConfig,
    max_polls: Option<usize>,
) -> Result<(), ConnectError> {
    let mut offset: i64 = 0;
    let mut sessions: HashMap<i64, Box<dyn AgentSession>> = HashMap::new();
    let mut polls = 0usize;

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
                eprintln!("[bwoc-connect] poll error (continuing): {e}");
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

            // Lazily spawn one session per chat (DM continuity).
            if let std::collections::hash_map::Entry::Vacant(slot) = sessions.entry(msg.chat_id) {
                match factory.create().await {
                    Ok(s) => {
                        slot.insert(s);
                    }
                    Err(e) => {
                        eprintln!("[bwoc-connect] could not start agent session: {e}");
                        let _ = transport
                            .send(
                                msg.chat_id,
                                "⚠️ couldn't start the agent session; try again.",
                            )
                            .await;
                        continue;
                    }
                }
            }
            let session = sessions.get_mut(&msg.chat_id).expect("inserted above");

            match session.ask(&msg.text).await {
                Ok(reply) => {
                    if let Err(e) = transport.send(msg.chat_id, &reply).await {
                        eprintln!("[bwoc-connect] send failed: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("[bwoc-connect] agent error: {e}");
                    let _ = transport
                        .send(msg.chat_id, &format!("⚠️ agent error: {e}"))
                        .await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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

    /// Transport that yields one scripted batch then empty, recording sends.
    struct MockTransport {
        batches: Mutex<Vec<Vec<Incoming>>>,
        sent: Mutex<Vec<(i64, String)>>,
    }
    #[async_trait]
    impl Transport for MockTransport {
        async fn poll(&self, _offset: i64) -> Result<Vec<Incoming>, ConnectError> {
            Ok(self.batches.lock().unwrap().pop().unwrap_or_default())
        }
        async fn send(&self, chat_id: i64, text: &str) -> Result<(), ConnectError> {
            self.sent.lock().unwrap().push((chat_id, text.to_string()));
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
        let t = MockTransport {
            batches: Mutex::new(vec![vec![Incoming {
                update_id: 5,
                chat_id: 42,
                from_user_id: 111,
                text: "hi".into(),
            }]]),
            sent: Mutex::new(vec![]),
        };
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = EchoFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, &cfg(&[111]), Some(1)).await.unwrap();
        assert_eq!(
            t.sent.lock().unwrap().as_slice(),
            &[(42, "echo: hi".to_string())]
        );
        assert_eq!(created.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_allow_listed_message_is_ignored() {
        let t = MockTransport {
            batches: Mutex::new(vec![vec![Incoming {
                update_id: 1,
                chat_id: 9,
                from_user_id: 999, // not allowed
                text: "spam".into(),
            }]]),
            sent: Mutex::new(vec![]),
        };
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = EchoFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, &cfg(&[111]), Some(1)).await.unwrap();
        assert!(t.sent.lock().unwrap().is_empty(), "no reply to a stranger");
        assert_eq!(
            created.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no session spawned"
        );
    }

    #[tokio::test]
    async fn two_messages_same_chat_reuse_one_session() {
        let t = MockTransport {
            batches: Mutex::new(vec![vec![
                Incoming {
                    update_id: 1,
                    chat_id: 7,
                    from_user_id: 111,
                    text: "a".into(),
                },
                Incoming {
                    update_id: 2,
                    chat_id: 7,
                    from_user_id: 111,
                    text: "b".into(),
                },
            ]]),
            sent: Mutex::new(vec![]),
        };
        let created = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let f = EchoFactory {
            created: created.clone(),
        };
        run_bridge(&t, &f, &cfg(&[111]), Some(1)).await.unwrap();
        assert_eq!(t.sent.lock().unwrap().len(), 2);
        assert_eq!(
            created.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one session reused across the DM"
        );
    }
}
