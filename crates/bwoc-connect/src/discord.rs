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
//! `parse_message_create` and the pure reconnect helpers carry the unit tests.
//!
//! Reconnects **RESUME** (op 6) when the session is still valid: READY's
//! `session_id` + `resume_gateway_url` (+ the last dispatch `seq`) survive a
//! drop, so a RECONNECT (op 7) re-attaches without consuming an IDENTIFY.
//! IDENTIFYs are rate-limited (1 per 5s, ~1000/day) — the previous
//! fresh-IDENTIFY-every-5s loop sat exactly on that limit, and Discord answers
//! an over-limit session with another RECONNECT: a self-sustaining reconnect
//! loop. RESUME plus exponential backoff (reset after a healthy session) is
//! what breaks it. INVALID_SESSION (op 9) and the 4xxx close codes that the
//! spec marks non-resumable drop the state and fall back to IDENTIFY.

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
/// First reconnect delay; doubles per consecutive failure (capped). Stays
/// under Discord's 1-IDENTIFY-per-5s limit even when resume state is lost.
const RECONNECT_MIN_SECS: u64 = 5;
const RECONNECT_MAX_SECS: u64 = 60;
/// A session that lived this long was healthy — reset the backoff.
const HEALTHY_SESSION_SECS: u64 = 60;

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

    async fn send(&self, chat_id: i64, text: &str) -> Result<i64, ConnectError> {
        let url = format!("{REST_BASE}/channels/{chat_id}/messages");
        let req = self
            .http
            .post(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&json!({ "content": text }));
        let body = send_message_request(req, "createMessage").await?;
        // Discord ids are snowflakes sent as strings; they fit in i64.
        body.get("id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| ConnectError::Transport("createMessage: no message id".into()))
    }

    async fn edit(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), ConnectError> {
        let url = format!("{REST_BASE}/channels/{chat_id}/messages/{message_id}");
        let req = self
            .http
            .patch(&url)
            .header("Authorization", format!("Bot {}", self.token))
            .json(&json!({ "content": text }));
        send_message_request(req, "editMessage").await.map(|_| ())
    }
}

/// Execute a create/edit-message request, returning the decoded JSON body. On a
/// non-2xx, surfaces Discord's JSON error body (capped so a huge/HTML page can't
/// bloat the error).
async fn send_message_request(
    req: reqwest::RequestBuilder,
    what: &str,
) -> Result<Value, ConnectError> {
    let resp = req
        .send()
        .await
        .map_err(|e| ConnectError::Transport(format!("{what}: {e}")))?;
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
            "{what} HTTP {status}: {body}"
        )));
    }
    resp.json::<Value>()
        .await
        .map_err(|e| ConnectError::Transport(format!("{what} decode: {e}")))
}

/// Resume coordinates captured from `READY`: enough to re-attach to the same
/// session (op 6) after a drop instead of IDENTIFYing from scratch. `bot_id`
/// rides along because a resumed session replays dispatches without a new
/// `READY` to re-learn it from.
struct ResumeState {
    session_id: String,
    /// Gateway host to resume against (`resume_gateway_url` from READY).
    resume_url: String,
    /// Bot user id (mention-gating) — survives RESUME.
    bot_id: String,
    /// Last seen dispatch sequence (heartbeats + the RESUME payload).
    seq: Option<u64>,
}

/// Reconnect loop: run one gateway session; on error/disconnect, wait and
/// retry until the queue closes (the transport was dropped). Resumes when the
/// session is still valid; exponential backoff between consecutive failures,
/// reset once a session proves healthy.
async fn gateway_loop(token: String, tx: mpsc::Sender<Incoming>) {
    let mut resume: Option<ResumeState> = None;
    let mut backoff = RECONNECT_MIN_SECS;
    loop {
        let started = std::time::Instant::now();
        if let Err(e) = run_gateway_once(&token, &tx, &mut resume).await {
            eprintln!("[bwoc-connect] discord gateway: {e}");
        }
        if tx.is_closed() {
            return; // transport dropped — stop reconnecting
        }
        if started.elapsed() >= Duration::from_secs(HEALTHY_SESSION_SECS) {
            backoff = RECONNECT_MIN_SECS;
        }
        let mode = if resume.is_some() {
            "resume"
        } else {
            "identify"
        };
        eprintln!("[bwoc-connect] discord gateway: reconnecting ({mode}) in {backoff}s");
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = next_backoff(backoff);
    }
}

/// Doubling backoff, capped. Pure — unit-tested.
fn next_backoff(cur: u64) -> u64 {
    (cur * 2).min(RECONNECT_MAX_SECS)
}

/// `resume_gateway_url` arrives as a bare host (`wss://gateway-us-east1-b.discord.gg`);
/// resume against it with the same version/encoding params as [`GATEWAY_URL`].
/// Pure — unit-tested.
fn resume_ws_url(base: &str) -> String {
    format!("{}/?v=10&encoding=json", base.trim_end_matches('/'))
}

/// Close codes after which the gateway spec says the session cannot be
/// resumed — drop the resume state and IDENTIFY fresh. 4004 (auth failed) and
/// 4010–4014 (bad shard/version/intents) won't heal on retry either, but the
/// loop keeps trying at the capped backoff so an operator-side fix (e.g.
/// enabling an intent) picks up without a restart. Pure — unit-tested.
fn close_clears_resume(code: Option<u16>) -> bool {
    matches!(
        code,
        Some(4004) | Some(4007) | Some(4009) | Some(4010..=4014)
    )
}

/// One gateway session: HELLO → IDENTIFY or RESUME → (heartbeat ∥ dispatch)
/// until the socket closes or Discord asks us to reconnect. Returns `Ok(())`
/// only when the queue closed (transport dropped); every disconnect is an
/// `Err` so the caller reconnects. `resume` is read to pick IDENTIFY vs
/// RESUME and updated as the session evolves (READY captures it, op 9 /
/// non-resumable close codes clear it).
async fn run_gateway_once(
    token: &str,
    tx: &mpsc::Sender<Incoming>,
    resume: &mut Option<ResumeState>,
) -> Result<(), ConnectError> {
    let url = match resume.as_ref() {
        Some(r) => resume_ws_url(&r.resume_url),
        None => GATEWAY_URL.to_string(),
    };
    let (ws, _) = tokio_tungstenite::connect_async(&url)
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

    // RESUME (op 6) when we hold live session coordinates — re-attaches and
    // replays missed dispatches without consuming an IDENTIFY slot. IDENTIFY
    // (op 2) otherwise.
    let first = match resume.as_ref() {
        Some(r) => json!({
            "op": 6,
            "d": { "token": token, "session_id": r.session_id, "seq": r.seq.unwrap_or(0) }
        }),
        None => json!({
            "op": 2,
            "d": {
                "token": token,
                "intents": INTENTS,
                "properties": { "os": "linux", "browser": "bwoc-connect", "device": "bwoc-connect" }
            }
        }),
    };
    write
        .send(Message::Text(first.to_string()))
        .await
        .map_err(|e| ConnectError::Transport(format!("identify/resume: {e}")))?;

    // Per the gateway spec, wait heartbeat_interval * jitter before the FIRST
    // beat (don't beat immediately after IDENTIFY). `interval`'s first tick is
    // immediate, so start it half an interval out — a fixed, dependency-free
    // jitter that avoids thundering-herd without needing an RNG.
    // `.max(1)` — `interval_at` panics on a zero period, which a malformed
    // HELLO (heartbeat_interval: 0) would otherwise trigger.
    let period = Duration::from_millis(interval_ms.max(1));
    let mut hb = tokio::time::interval_at(tokio::time::Instant::now() + period / 2, period);
    // A resumed session replays from the stored cursor and never re-sends
    // READY — seed both from the resume state.
    let mut seq: Option<u64> = resume.as_ref().and_then(|r| r.seq);
    let mut bot_id: Option<String> = resume.as_ref().map(|r| r.bot_id.clone());

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
                    Message::Close(frame) => {
                        // Non-resumable 4xxx codes ⇒ the session is dead for
                        // good; IDENTIFY fresh next attempt.
                        let code = frame.as_ref().map(|f| u16::from(f.code));
                        if close_clears_resume(code) {
                            *resume = None;
                        }
                        let detail = frame
                            .map(|f| format!("code {} {}", u16::from(f.code), f.reason))
                            .unwrap_or_else(|| "no close frame".into());
                        return Err(ConnectError::Transport(format!("gateway closed ({detail})")));
                    }
                    _ => continue,
                };
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| ConnectError::Transport(format!("gateway decode: {e}")))?;
                if let Some(s) = v.get("s").and_then(Value::as_u64) {
                    seq = Some(s);
                    if let Some(r) = resume.as_mut() {
                        r.seq = Some(s); // keep the resume cursor current
                    }
                }
                match v.get("op").and_then(Value::as_u64) {
                    Some(0) => match v.get("t").and_then(Value::as_str) {
                        Some("READY") => {
                            let d = &v["d"];
                            bot_id = d
                                .get("user")
                                .and_then(|u| u.get("id"))
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            // Capture the resume coordinates for future drops.
                            *resume = match (
                                d.get("session_id").and_then(Value::as_str),
                                d.get("resume_gateway_url").and_then(Value::as_str),
                                bot_id.as_deref(),
                            ) {
                                (Some(sid), Some(rurl), Some(bid)) => Some(ResumeState {
                                    session_id: sid.to_string(),
                                    resume_url: rurl.to_string(),
                                    bot_id: bid.to_string(),
                                    seq,
                                }),
                                _ => None,
                            };
                        }
                        Some("RESUMED") => {
                            eprintln!("[bwoc-connect] discord gateway: session resumed");
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
                    // RECONNECT (op 7): drop the socket; the session is still
                    // valid — the next attempt RESUMEs.
                    Some(7) => {
                        return Err(ConnectError::Transport("gateway requested reconnect".into()));
                    }
                    // INVALID_SESSION (op 9): `d` says whether it can still be
                    // resumed; `false` ⇒ the session is gone — re-IDENTIFY.
                    Some(9) => {
                        if !v.get("d").and_then(Value::as_bool).unwrap_or(false) {
                            *resume = None;
                        }
                        return Err(ConnectError::Transport(
                            "gateway invalidated the session".into(),
                        ));
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
    fn backoff_doubles_and_caps() {
        assert_eq!(next_backoff(RECONNECT_MIN_SECS), RECONNECT_MIN_SECS * 2);
        assert_eq!(next_backoff(40), RECONNECT_MAX_SECS, "doubling caps at max");
        assert_eq!(next_backoff(RECONNECT_MAX_SECS), RECONNECT_MAX_SECS);
    }

    #[test]
    fn resume_url_gets_version_params() {
        assert_eq!(
            resume_ws_url("wss://gateway-us-east1-b.discord.gg"),
            "wss://gateway-us-east1-b.discord.gg/?v=10&encoding=json"
        );
        assert_eq!(
            resume_ws_url("wss://gateway-us-east1-b.discord.gg/"),
            "wss://gateway-us-east1-b.discord.gg/?v=10&encoding=json",
            "trailing slash is not doubled"
        );
    }

    #[test]
    fn non_resumable_close_codes_clear_resume() {
        for fatal in [4004, 4007, 4009, 4013, 4014] {
            assert!(close_clears_resume(Some(fatal)), "{fatal} re-identifies");
        }
        for resumable in [4000, 4001, 4002, 4003, 4005, 4008] {
            assert!(!close_clears_resume(Some(resumable)), "{resumable} resumes");
        }
        assert!(
            !close_clears_resume(None),
            "no close frame (network blip) keeps the session for RESUME"
        );
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
