//! Telegram Bot API transport (long-poll). The live network edge — the
//! routing logic it feeds lives in [`crate::run_bridge`] and is tested with a
//! mock transport; this module is the thin reqwest adapter.

use serde_json::Value;

use crate::{ConnectError, Incoming, Transport};

/// Long-poll window (seconds) for `getUpdates`. The HTTP client timeout sits a
/// few seconds above this so a full-window poll isn't cut off mid-flight.
const POLL_TIMEOUT_SECS: u64 = 25;

pub struct TelegramTransport {
    client: reqwest::Client,
    /// `https://api.telegram.org/bot<token>` — the token is in the URL, never
    /// logged (Debug is not derived).
    api_base: String,
    /// Bot's `@username` (without the `@`), resolved via `getMe`, used for
    /// mention detection in groups. `None` ⇒ mentions never match (DM-only).
    bot_username: Option<String>,
}

impl TelegramTransport {
    pub fn new(token: &str) -> Result<Self, ConnectError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(POLL_TIMEOUT_SECS + 10))
            .build()
            .map_err(|e| ConnectError::Transport(format!("http client: {e}")))?;
        Ok(Self {
            client,
            api_base: format!("https://api.telegram.org/bot{token}"),
            bot_username: None,
        })
    }

    /// Resolve and cache the bot's `@username` via `getMe` (call once at
    /// startup; required for group mention-gating).
    pub async fn resolve_identity(&mut self) -> Result<(), ConnectError> {
        let body = self.get_json("getMe", &[]).await?;
        self.bot_username = body
            .get("result")
            .and_then(|r| r.get("username"))
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(())
    }

    async fn get_json(
        &self,
        method: &str,
        query: &[(&str, String)],
    ) -> Result<Value, ConnectError> {
        let url = format!("{}/{method}", self.api_base);
        let resp = self
            .client
            .get(&url)
            .query(query)
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("{method}: {e}")))?;
        resp.json::<Value>()
            .await
            .map_err(|e| ConnectError::Transport(format!("{method} decode: {e}")))
    }
}

/// Parse a `getUpdates` JSON body into [`Incoming`] messages. Keeps text
/// messages (DM + group/supergroup) with a sender; skips edits, stickers,
/// channels, and anon posts. `bot_username` (no `@`) drives group
/// mention-detection. Pure — unit-tested without the network.
pub fn parse_updates(body: &Value, bot_username: Option<&str>) -> Vec<Incoming> {
    let mut out = Vec::new();
    let Some(results) = body.get("result").and_then(Value::as_array) else {
        return out;
    };
    for u in results {
        let Some(update_id) = u.get("update_id").and_then(Value::as_i64) else {
            continue;
        };
        let Some(msg) = u.get("message") else {
            continue; // edits / non-message updates
        };
        let chat_type = msg
            .get("chat")
            .and_then(|c| c.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let is_group = match chat_type {
            "private" => false,
            "group" | "supergroup" => true,
            _ => continue, // channels / unknown — skip
        };
        let (Some(chat_id), Some(from_user_id), Some(text)) = (
            msg.get("chat")
                .and_then(|c| c.get("id"))
                .and_then(Value::as_i64),
            msg.get("from")
                .and_then(|f| f.get("id"))
                .and_then(Value::as_i64),
            msg.get("text").and_then(Value::as_str),
        ) else {
            continue;
        };
        let mentions_bot = is_group && mentions(text, bot_username);
        out.push(Incoming {
            update_id,
            chat_id,
            from_user_id,
            text: text.to_string(),
            is_group,
            mentions_bot,
        });
    }
    out
}

/// Does `text` @mention the bot? Case-insensitive `@<username>` match (the
/// common Telegram mention form). `None` username never matches.
fn mentions(text: &str, bot_username: Option<&str>) -> bool {
    let Some(u) = bot_username else { return false };
    let needle = format!("@{}", u.to_ascii_lowercase());
    text.to_ascii_lowercase().contains(&needle)
}

#[async_trait::async_trait]
impl Transport for TelegramTransport {
    async fn poll(&self, offset: i64) -> Result<Vec<Incoming>, ConnectError> {
        let body = self
            .get_json(
                "getUpdates",
                &[
                    ("offset", offset.to_string()),
                    ("timeout", POLL_TIMEOUT_SECS.to_string()),
                    ("allowed_updates", "[\"message\"]".to_string()),
                ],
            )
            .await?;
        if body.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(ConnectError::Transport(format!(
                "getUpdates not ok: {}",
                body.get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
            )));
        }
        Ok(parse_updates(&body, self.bot_username.as_deref()))
    }

    async fn send(&self, chat_id: i64, text: &str) -> Result<(), ConnectError> {
        let url = format!("{}/sendMessage", self.api_base);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "chat_id": chat_id, "text": text }))
            .send()
            .await
            .map_err(|e| ConnectError::Transport(format!("sendMessage: {e}")))?;
        if !resp.status().is_success() {
            return Err(ConnectError::Transport(format!(
                "sendMessage HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_updates_classifies_dm_and_group_and_skips_noise() {
        let body = json!({
            "ok": true,
            "result": [
                { "update_id": 10, "message": {  // private DM
                    "chat": {"id": 42, "type": "private"}, "from": {"id": 111}, "text": "hello" }},
                { "update_id": 11, "message": {  // no text (sticker) → skipped
                    "chat": {"id": 42, "type": "private"}, "from": {"id": 111}, "sticker": {} }},
                { "update_id": 12, "edited_message": {  // not a fresh message → skipped
                    "chat": {"id": 42, "type": "private"}, "from": {"id": 111}, "text": "edit" }},
                { "update_id": 13, "message": {  // no from → skipped
                    "chat": {"id": 9, "type": "private"}, "text": "anon" }},
                { "update_id": 14, "message": {  // group, no mention
                    "chat": {"id": -100, "type": "supergroup"}, "from": {"id": 222}, "text": "hi team" }},
                { "update_id": 15, "message": {  // group, mentions the bot
                    "chat": {"id": -100, "type": "supergroup"}, "from": {"id": 222}, "text": "hey @MyBot ping" }},
                { "update_id": 16, "channel_post": {  // channel → skipped
                    "chat": {"id": -200, "type": "channel"}, "text": "broadcast" }},
            ]
        });
        let msgs = parse_updates(&body, Some("mybot")); // case-insensitive
        assert_eq!(msgs.len(), 3, "DM + 2 group messages survive");
        assert_eq!(
            msgs[0],
            Incoming {
                update_id: 10,
                chat_id: 42,
                from_user_id: 111,
                text: "hello".into(),
                is_group: false,
                mentions_bot: false
            }
        );
        // group, no mention
        assert!(msgs[1].is_group && !msgs[1].mentions_bot);
        // group, mentions @MyBot (matched case-insensitively)
        assert!(msgs[2].is_group && msgs[2].mentions_bot);
    }

    #[test]
    fn mentions_is_case_insensitive_and_none_never_matches() {
        assert!(mentions("yo @MyBot", Some("mybot")));
        assert!(mentions("yo @mybot now", Some("MyBot")));
        assert!(!mentions("no mention here", Some("mybot")));
        assert!(!mentions("@mybot", None));
    }

    #[test]
    fn parse_updates_empty_result_is_empty() {
        assert!(parse_updates(&json!({"ok": true, "result": []}), None).is_empty());
        assert!(parse_updates(&json!({"ok": true}), None).is_empty());
    }
}
