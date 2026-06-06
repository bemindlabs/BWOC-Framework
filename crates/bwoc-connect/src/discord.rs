//! Discord gateway transport (chat-connectors PR4).
//!
//! Discord has no long-poll — receiving messages requires the **Gateway
//! websocket**. A background task owns the connection (HELLO → IDENTIFY →
//! heartbeat → dispatch) and pushes `MESSAGE_CREATE` events into a queue that
//! [`Transport::poll`] drains; [`Transport::send`] uses the REST API. The
//! routing/allow-list/group logic ([`crate::run_bridge`]) is **shared with
//! Telegram and unchanged** — only this transport and the pure
//! [`parse_message_create`] are Discord-specific.
//!
//! Intents: `GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT`. The gateway
//! loop is the live, **integration-untested** edge (no Discord token in CI);
//! `parse_message_create` carries the unit tests.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;

use crate::{ConnectError, Incoming, Transport};

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const REST_BASE: &str = "https://discord.com/api/v10";
/// GUILD_MESSAGES (1<<9) | DIRECT_MESSAGES (1<<12) | MESSAGE_CONTENT (1<<15).
const INTENTS: u64 = (1 << 9) | (1 << 12) | (1 << 15);
/// Reconnect delay after the gateway drops.
const RECONNECT_SECS: u64 = 5;

pub struct DiscordTransport {
    token: String,
    http: reqwest::Client,
    /// Inbound messages from the gateway background task.
    rx: Mutex<mpsc::Receiver<Incoming>>,
}

impl DiscordTransport {
    /// Connect: spawn the gateway background task and return a transport whose
    /// `poll` drains its queue. The task reconnects on drop until the channel
    /// closes (transport dropped).
    pub async fn connect(token: &str) -> Result<Self, ConnectError> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| ConnectError::Transport(format!("http client: {e}")))?;
        let (tx, rx) = mpsc::channel(256);
        let task_token = token.to_string();
        tokio::spawn(async move { gateway_loop(task_token, tx).await });
        Ok(Self {
            token: token.to_string(),
            http,
            rx: Mutex::new(rx),
        })
    }
}

#[async_trait::async_trait]
impl Transport for DiscordTransport {
    async fn poll(&self, _offset: i64) -> Result<Vec<Incoming>, ConnectError> {
        // Gateway is push; drain whatever the task queued, with a ~1s liveness
        // window so the bridge stays responsive without busy-spinning.
        let mut rx = self.rx.lock().await;
        let mut out = Vec::new();
        match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
            Ok(Some(m)) => out.push(m),
            Ok(None) => return Err(ConnectError::Transport("gateway task ended".into())),
            Err(_) => {} // timed out — no messages this tick
        }
        while let Ok(m) = rx.try_recv() {
            out.push(m);
        }
        Ok(out)
    }

    async fn send(&self, chat_id: i64, text: &str) -> Result<(), ConnectError> {
        let url = format!("{REST_BASE}/channels/{chat_id}/messages");
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&json!({ "content": text }))
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("createMessage: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            // Discord returns a JSON error body (missing perms, invalid form,
            // rate limit, …) — include it so failures are diagnosable.
            let body = resp.text().await.unwrap_or_default();
            return Err(ConnectError::Transport(format!(
                "createMessage HTTP {status}: {}",
                body.trim()
            )));
        }
        Ok(())
    }
}

/// Reconnect loop: run one gateway session; on error/disconnect, wait and
/// retry until the queue closes (the transport was dropped).
async fn gateway_loop(token: String, tx: mpsc::Sender<Incoming>) {
    loop {
        if let Err(e) = run_gateway_once(&token, &tx).await {
            eprintln!("[bwoc-connect] discord gateway: {e}");
        }
        if tx.is_closed() {
            return; // transport dropped — stop reconnecting
        }
        eprintln!("[bwoc-connect] discord gateway: reconnecting in {RECONNECT_SECS}s");
        tokio::time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
    }
}

/// One gateway session: HELLO → IDENTIFY → (heartbeat ∥ dispatch) until the
/// socket closes or Discord asks us to reconnect. Returns `Ok(())` only when
/// the queue closed (transport dropped); every disconnect is an `Err` so the
/// caller reconnects.
async fn run_gateway_once(token: &str, tx: &mpsc::Sender<Incoming>) -> Result<(), ConnectError> {
    let (ws, _) = tokio_tungstenite::connect_async(GATEWAY_URL)
        .await
        .map_err(|e| ConnectError::Transport(format!("gateway connect: {e}")))?;
    let (mut write, mut read) = ws.split();

    // HELLO (op 10) carries the heartbeat interval.
    let hello = next_json(&mut read).await?;
    let interval_ms = hello
        .get("d")
        .and_then(|d| d.get("heartbeat_interval"))
        .and_then(Value::as_u64)
        .unwrap_or(45_000);

    // IDENTIFY (op 2).
    let identify = json!({
        "op": 2,
        "d": {
            "token": token,
            "intents": INTENTS,
            "properties": { "os": "linux", "browser": "bwoc-connect", "device": "bwoc-connect" }
        }
    });
    write
        .send(Message::Text(identify.to_string()))
        .await
        .map_err(|e| ConnectError::Transport(format!("identify: {e}")))?;

    // Per the gateway spec, wait heartbeat_interval * jitter before the FIRST
    // beat (don't beat immediately after IDENTIFY). `interval`'s first tick is
    // immediate, so start it half an interval out — a fixed, dependency-free
    // jitter that avoids thundering-herd without needing an RNG.
    let period = Duration::from_millis(interval_ms);
    let mut hb = tokio::time::interval_at(tokio::time::Instant::now() + period / 2, period);
    let mut seq: Option<u64> = None;
    let mut bot_id: Option<String> = None;

    loop {
        if tx.is_closed() {
            return Ok(()); // transport dropped
        }
        tokio::select! {
            _ = hb.tick() => {
                // Heartbeat (op 1) with the last sequence number.
                let beat = json!({ "op": 1, "d": seq });
                write.send(Message::Text(beat.to_string())).await
                    .map_err(|e| ConnectError::Transport(format!("heartbeat: {e}")))?;
            }
            msg = read.next() => {
                let Some(msg) = msg else {
                    return Err(ConnectError::Transport("gateway stream ended".into()));
                };
                let msg = msg.map_err(|e| ConnectError::Transport(format!("gateway read: {e}")))?;
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Ping(p) => {
                        let _ = write.send(Message::Pong(p)).await;
                        continue;
                    }
                    Message::Close(_) => {
                        return Err(ConnectError::Transport("gateway closed".into()));
                    }
                    _ => continue,
                };
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| ConnectError::Transport(format!("gateway decode: {e}")))?;
                if let Some(s) = v.get("s").and_then(Value::as_u64) {
                    seq = Some(s);
                }
                match v.get("op").and_then(Value::as_u64) {
                    Some(0) => match v.get("t").and_then(Value::as_str) {
                        Some("READY") => {
                            bot_id = v.get("d")
                                .and_then(|d| d.get("user"))
                                .and_then(|u| u.get("id"))
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        }
                        Some("MESSAGE_CREATE") => {
                            if let Some(bid) = &bot_id {
                                if let Some(inc) = parse_message_create(&v["d"], bid) {
                                    if tx.send(inc).await.is_err() {
                                        return Ok(()); // transport dropped
                                    }
                                }
                            }
                        }
                        _ => {}
                    },
                    // RECONNECT (7) / INVALID_SESSION (9) → drop + reconnect.
                    Some(7) | Some(9) => {
                        return Err(ConnectError::Transport("gateway asked to reconnect".into()));
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Read websocket frames until the next JSON text frame.
async fn next_json<S>(read: &mut S) -> Result<Value, ConnectError>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let Some(msg) = read.next().await else {
            return Err(ConnectError::Transport(
                "gateway closed before HELLO".into(),
            ));
        };
        let msg = msg.map_err(|e| ConnectError::Transport(format!("gateway read: {e}")))?;
        if let Message::Text(t) = msg {
            return serde_json::from_str(&t)
                .map_err(|e| ConnectError::Transport(format!("gateway decode: {e}")));
        }
    }
}

/// Parse a `MESSAGE_CREATE` payload (`d`) into an [`Incoming`]. Skips bot
/// authors (incl. self — avoids loops) and empty/non-text content. `guild_id`
/// present ⇒ a group room; mention is the structured `mentions[]` array (more
/// robust than substring matching). Pure — unit-tested without the gateway.
pub fn parse_message_create(d: &Value, bot_id: &str) -> Option<Incoming> {
    if d.get("author")
        .and_then(|a| a.get("bot"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return None;
    }
    let chat_id = d
        .get("channel_id")
        .and_then(Value::as_str)?
        .parse::<i64>()
        .ok()?;
    let from_user_id = d
        .get("author")
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)?
        .parse::<i64>()
        .ok()?;
    let text = d.get("content").and_then(Value::as_str)?.to_string();
    if text.is_empty() {
        return None;
    }
    let is_group = d.get("guild_id").is_some_and(|g| !g.is_null());
    let mentions_bot = is_group
        && d.get("mentions")
            .and_then(Value::as_array)
            .is_some_and(|arr| {
                arr.iter()
                    .any(|u| u.get("id").and_then(Value::as_str) == Some(bot_id))
            });
    Some(Incoming {
        update_id: 0, // gateway is push — no offset cursor
        chat_id,
        from_user_id,
        text,
        is_group,
        mentions_bot,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_dm_message() {
        let d = json!({
            "channel_id": "555",
            "author": { "id": "111", "bot": false },
            "content": "hello"
            // no guild_id ⇒ DM
        });
        let inc = parse_message_create(&d, "999").unwrap();
        assert_eq!(inc.chat_id, 555);
        assert_eq!(inc.from_user_id, 111);
        assert_eq!(inc.text, "hello");
        assert!(!inc.is_group);
        assert!(!inc.mentions_bot);
    }

    #[test]
    fn parse_guild_message_with_and_without_bot_mention() {
        let base = |mentions: Value| {
            json!({
                "channel_id": "777", "guild_id": "100",
                "author": { "id": "222", "bot": false },
                "content": "hey", "mentions": mentions
            })
        };
        let mentioned = parse_message_create(&base(json!([{ "id": "999" }])), "999").unwrap();
        assert!(mentioned.is_group && mentioned.mentions_bot);
        let not = parse_message_create(&base(json!([{ "id": "333" }])), "999").unwrap();
        assert!(not.is_group && !not.mentions_bot);
    }

    #[test]
    fn skips_bot_authors_and_empty_content() {
        let bot =
            json!({ "channel_id": "1", "author": { "id": "2", "bot": true }, "content": "x" });
        assert!(
            parse_message_create(&bot, "9").is_none(),
            "bot author skipped (no loops)"
        );
        let empty =
            json!({ "channel_id": "1", "author": { "id": "2", "bot": false }, "content": "" });
        assert!(
            parse_message_create(&empty, "9").is_none(),
            "empty content skipped"
        );
    }
}
