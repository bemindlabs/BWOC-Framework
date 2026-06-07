//! LINE Messaging API transport (chat-connectors — LINE).
//!
//! Unlike Telegram (long-poll) and Discord (gateway), LINE **pushes events to an
//! HTTPS webhook** — there is no `getUpdates`. So this transport runs a small
//! inbound HTTP server (axum): each webhook POST is signature-verified
//! (`X-Line-Signature` = base64(HMAC-SHA256(channel secret, body))), parsed, and
//! its events are queued; [`Transport::poll`] drains the queue (same shape as the
//! Discord gateway→mpsc bridge). The routing/allow-list/group logic
//! ([`crate::run_bridge`]) is reused unchanged.
//!
//! **Reply vs push (cost).** LINE **reply messages** (using the one-time
//! `replyToken` from a webhook event) are *free and unlimited*; **push** messages
//! count against the monthly quota. So [`Transport::send`] uses the stored reply
//! token when it's still fresh ([`REPLY_WINDOW_SECS`]) and falls back to a push
//! otherwise (a slow agent turn that outlives the token). LINE has no
//! message-edit API → [`Transport::supports_edit`] is `false` (no streaming;
//! the reply is sent once).
//!
//! Identity: LINE ids are strings (`U…`/`C…`/`R…`). They're hashed to a stable
//! `i64` ([`hash_id`]) so they fit the `i64` `Incoming`/allow-list seam without a
//! framework-wide id change; `main` hashes the configured `allow_user_ids` the
//! same way. `verify_signature` / `parse_webhook` / `hash_id` are pure +
//! unit-tested; the live HTTP server + send are the eyeball-reviewed edge.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::{ConnectError, Incoming, Transport};

const API_BASE: &str = "https://api.line.me/v2/bot";
/// How long after receiving an event a `replyToken` is treated as usable. LINE
/// tokens are short-lived + one-use; past this we push instead (quota). Kept
/// conservative so we never burn a token Telegram-style "not modified" error.
const REPLY_WINDOW_SECS: u64 = 55;

/// A parsed inbound LINE text event (pure intermediate of [`parse_webhook`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEvent {
    /// One-time reply token (free reply path); `None` for events without one.
    pub reply_token: Option<String>,
    /// Reply target id: the group/room id for groups, else the user id.
    pub source_id: String,
    /// Sender's user id (checked against the allow-list); may be empty if LINE
    /// withholds it (then the allow-list rejects, closed by default).
    pub user_id: String,
    pub is_group: bool,
    pub mentions_bot: bool,
    pub text: String,
}

/// Stable `i64` for a LINE id string (first 8 bytes of its SHA-256, big-endian).
/// Lets LINE's string ids ride the `i64` `Incoming`/allow-list seam; collisions
/// are cryptographically negligible for a handful of allow-listed ids.
pub fn hash_id(s: &str) -> i64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(s.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// Verify a LINE webhook signature: `header_sig` must equal
/// base64(HMAC-SHA256(`secret`, `body`)). Constant-time via `hmac::verify_slice`.
pub fn verify_signature(secret: &[u8], body: &[u8], header_sig: &str) -> bool {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let Ok(sig) = STANDARD.decode(header_sig) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

/// Parse a LINE webhook body into text [`LineEvent`]s. Keeps `message`/`text`
/// events with a usable source; skips everything else (follows, stickers,
/// postbacks, …). Group/room ⇒ `is_group`; `mentions_bot` is the structured
/// `message.mention.mentionees[].isSelf` flag. Pure — unit-tested.
pub fn parse_webhook(body: &Value) -> Vec<LineEvent> {
    let mut out = Vec::new();
    let Some(events) = body.get("events").and_then(Value::as_array) else {
        return out;
    };
    for e in events {
        if e.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let msg = e.get("message");
        if msg.and_then(|m| m.get("type")).and_then(Value::as_str) != Some("text") {
            continue;
        }
        let Some(text) = msg.and_then(|m| m.get("text")).and_then(Value::as_str) else {
            continue;
        };
        let source = e.get("source");
        let stype = source
            .and_then(|s| s.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let user_id = source
            .and_then(|s| s.get("userId"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (is_group, source_id) = match stype {
            "user" => (false, user_id.clone()),
            "group" => (
                true,
                source
                    .and_then(|s| s.get("groupId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            "room" => (
                true,
                source
                    .and_then(|s| s.get("roomId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            _ => continue,
        };
        if source_id.is_empty() {
            continue;
        }
        let mentions_bot = is_group
            && msg
                .and_then(|m| m.get("mention"))
                .and_then(|mn| mn.get("mentionees"))
                .and_then(Value::as_array)
                .is_some_and(|arr| {
                    arr.iter()
                        .any(|m| m.get("isSelf").and_then(Value::as_bool) == Some(true))
                });
        out.push(LineEvent {
            reply_token: e
                .get("replyToken")
                .and_then(Value::as_str)
                .map(str::to_string),
            source_id,
            user_id,
            is_group,
            mentions_bot,
            text: text.to_string(),
        });
    }
    out
}

/// The latest reply context for a chat: where to push, and the (one-use) reply
/// token if still fresh.
struct ReplySlot {
    source_id: String,
    reply_token: Option<String>,
    received: Instant,
}

/// Shared state the webhook handler writes and the transport reads.
struct WebhookState {
    secret: String,
    tx: mpsc::Sender<Incoming>,
    replies: Arc<Mutex<HashMap<i64, ReplySlot>>>,
    counter: AtomicI64,
}

pub struct LineTransport {
    client: reqwest::Client,
    access_token: String,
    rx: AsyncMutex<mpsc::Receiver<Incoming>>,
    replies: Arc<Mutex<HashMap<i64, ReplySlot>>>,
    reply_window: Duration,
}

impl LineTransport {
    /// Bind the webhook server on `bind` (e.g. `0.0.0.0:8080`) at `path`, then
    /// return a transport whose `poll` drains received events. Put an HTTPS
    /// reverse proxy (nginx/Cloudflare) in front and register that URL as the
    /// channel's webhook.
    pub async fn start(
        access_token: &str,
        secret: &str,
        bind: &str,
        path: &str,
    ) -> Result<Self, ConnectError> {
        let client = reqwest::Client::builder()
            // Bound a hung reply/push so an unreachable LINE API can't block the
            // bridge turn forever (mirrors the Telegram client).
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| ConnectError::Transport(format!("http client: {e}")))?;
        let (tx, rx) = mpsc::channel(256);
        let replies = Arc::new(Mutex::new(HashMap::new()));
        let state = Arc::new(WebhookState {
            secret: secret.to_string(),
            tx,
            replies: replies.clone(),
            counter: AtomicI64::new(1),
        });
        let app = Router::new().route(path, post(webhook)).with_state(state);
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|e| ConnectError::Transport(format!("bind {bind}: {e}")))?;
        eprintln!("[bwoc-connect] line webhook listening on {bind}{path}");
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("[bwoc-connect] line webhook server ended: {e}");
            }
        });
        Ok(Self {
            client,
            access_token: access_token.to_string(),
            rx: AsyncMutex::new(rx),
            replies,
            reply_window: Duration::from_secs(REPLY_WINDOW_SECS),
        })
    }

    async fn post_messages(&self, url: &str, payload: Value) -> Result<i64, ConnectError> {
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("line send: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body: String = resp
                .text()
                .await
                .unwrap_or_default()
                .trim()
                .chars()
                .take(300)
                .collect();
            return Err(ConnectError::Transport(format!(
                "line send HTTP {status}: {body}"
            )));
        }
        // Response carries sentMessages[].id (numeric string) on success.
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        Ok(body
            .get("sentMessages")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|m| m.get("id"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0))
    }
}

#[async_trait::async_trait]
impl Transport for LineTransport {
    async fn poll(&self, _offset: i64) -> Result<Vec<Incoming>, ConnectError> {
        let mut rx = self.rx.lock().await;
        let mut out = Vec::new();
        match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(m)) => out.push(m),
            Ok(None) => return Err(ConnectError::Transport("line webhook task ended".into())),
            Err(_) => {} // no events this tick
        }
        while let Ok(m) = rx.try_recv() {
            out.push(m);
        }
        Ok(out)
    }

    async fn send(&self, chat_id: i64, text: &str) -> Result<i64, ConnectError> {
        // Pick reply (free, fresh token) vs push (quota). Lock is dropped before
        // the await — never held across the network call.
        let target = {
            let mut map = self.replies.lock().unwrap();
            match map.get_mut(&chat_id) {
                Some(slot) => {
                    let token = if slot.received.elapsed() < self.reply_window {
                        slot.reply_token.take() // one-use
                    } else {
                        None
                    };
                    Some((slot.source_id.clone(), token))
                }
                None => None,
            }
        };
        match target {
            Some((_, Some(token))) => {
                self.post_messages(
                    &format!("{API_BASE}/message/reply"),
                    json!({ "replyToken": token, "messages": [{ "type": "text", "text": text }] }),
                )
                .await
            }
            Some((source_id, None)) => {
                self.post_messages(
                    &format!("{API_BASE}/message/push"),
                    json!({ "to": source_id, "messages": [{ "type": "text", "text": text }] }),
                )
                .await
            }
            None => Err(ConnectError::Transport(format!(
                "line: no reply target known for chat {chat_id}"
            ))),
        }
    }

    async fn edit(&self, _chat_id: i64, _message_id: i64, _text: &str) -> Result<(), ConnectError> {
        Err(ConnectError::Transport(
            "line has no message-edit API".into(),
        ))
    }

    fn supports_edit(&self) -> bool {
        false // no edit API → reply is sent once (no streaming)
    }
}

/// Webhook endpoint: verify the signature over the raw body, then queue events.
/// Always returns quickly (LINE retries on non-2xx).
async fn webhook(
    State(st): State<Arc<WebhookState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> StatusCode {
    let sig = headers
        .get("x-line-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !verify_signature(st.secret.as_bytes(), &body, sig) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(json) = serde_json::from_slice::<Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    for ev in parse_webhook(&json) {
        let chat_id = hash_id(&ev.source_id);
        st.replies.lock().unwrap().insert(
            chat_id,
            ReplySlot {
                source_id: ev.source_id,
                reply_token: ev.reply_token,
                received: Instant::now(),
            },
        );
        let inc = Incoming {
            update_id: st.counter.fetch_add(1, Ordering::SeqCst),
            chat_id,
            from_user_id: hash_id(&ev.user_id),
            text: ev.text,
            is_group: ev.is_group,
            mentions_bot: ev.mentions_bot,
        };
        let _ = st.tx.send(inc).await;
    }
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use hmac::{Hmac, Mac};
    use serde_json::json;
    use sha2::Sha256;

    fn sign(secret: &[u8], body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        STANDARD.encode(mac.finalize().into_bytes())
    }

    #[test]
    fn signature_verifies_and_rejects_tampering() {
        let secret = b"channel-secret";
        let body = br#"{"events":[]}"#;
        let good = sign(secret, body);
        assert!(verify_signature(secret, body, &good));
        assert!(
            !verify_signature(secret, b"{\"events\":[1]}", &good),
            "body tampered"
        );
        assert!(
            !verify_signature(b"wrong-secret", body, &good),
            "wrong secret"
        );
        assert!(
            !verify_signature(secret, body, "not-base64!!"),
            "bad header"
        );
    }

    #[test]
    fn hash_id_is_stable_and_distinct() {
        assert_eq!(hash_id("U123"), hash_id("U123"));
        assert_ne!(hash_id("U123"), hash_id("U124"));
        assert_ne!(hash_id("Cgroup"), hash_id("U123"));
    }

    #[test]
    fn parse_dm_text_event() {
        let body = json!({
            "events": [{
                "type": "message",
                "replyToken": "rt-1",
                "source": { "type": "user", "userId": "Uabc" },
                "message": { "type": "text", "text": "hello" }
            }]
        });
        let evs = parse_webhook(&body);
        assert_eq!(evs.len(), 1);
        let e = &evs[0];
        assert_eq!(e.reply_token.as_deref(), Some("rt-1"));
        assert_eq!(e.source_id, "Uabc");
        assert_eq!(e.user_id, "Uabc");
        assert!(!e.is_group && !e.mentions_bot);
        assert_eq!(e.text, "hello");
    }

    #[test]
    fn parse_group_mention_and_non_mention() {
        let mk = |mention: Value| {
            json!({ "events": [{
                "type": "message",
                "replyToken": "rt",
                "source": { "type": "group", "groupId": "Cxyz", "userId": "Usender" },
                "message": { "type": "text", "text": "yo", "mention": mention }
            }]})
        };
        let mentioned = parse_webhook(&mk(json!({ "mentionees": [{ "isSelf": true }] })));
        assert!(mentioned[0].is_group && mentioned[0].mentions_bot);
        assert_eq!(mentioned[0].source_id, "Cxyz");
        assert_eq!(mentioned[0].user_id, "Usender");
        let other = parse_webhook(&mk(json!({ "mentionees": [{ "isSelf": false }] })));
        assert!(other[0].is_group && !other[0].mentions_bot);
    }

    #[test]
    fn skips_non_text_and_non_message_and_sourceless() {
        let body = json!({
            "events": [
                { "type": "follow", "source": { "type": "user", "userId": "U1" } },
                { "type": "message", "source": { "type": "user", "userId": "U1" },
                  "message": { "type": "sticker" } },
                { "type": "message", "source": { "type": "group" }, // no groupId
                  "message": { "type": "text", "text": "x" } }
            ]
        });
        assert!(parse_webhook(&body).is_empty());
    }
}
