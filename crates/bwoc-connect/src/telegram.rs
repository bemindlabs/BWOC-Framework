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
        })
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

/// Parse a `getUpdates` JSON body into [`Incoming`] messages. Only text
/// messages with a sender are kept; everything else (edits, joins, stickers,
/// channel posts) is skipped. Pure — unit-tested without the network.
pub fn parse_updates(body: &Value) -> Vec<Incoming> {
    let mut out = Vec::new();
    let Some(results) = body.get("result").and_then(Value::as_array) else {
        return out;
    };
    for u in results {
        let Some(update_id) = u.get("update_id").and_then(Value::as_i64) else {
            continue;
        };
        // Only private/direct text messages in PR1 (group handling is PR2).
        let Some(msg) = u.get("message") else {
            continue;
        };
        // PR1 is DM-only: skip group/supergroup/channel chats so a group
        // message never gets the agent's reply broadcast to the room.
        if msg
            .get("chat")
            .and_then(|c| c.get("type"))
            .and_then(Value::as_str)
            != Some("private")
        {
            continue;
        }
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
        out.push(Incoming {
            update_id,
            chat_id,
            from_user_id,
            text: text.to_string(),
        });
    }
    out
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
        Ok(parse_updates(&body))
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
    fn parse_updates_keeps_text_dms_skips_the_rest() {
        let body = json!({
            "ok": true,
            "result": [
                { "update_id": 10, "message": {
                    "chat": {"id": 42, "type": "private"}, "from": {"id": 111}, "text": "hello" }},
                { "update_id": 11, "message": {  // no text (sticker) → skipped
                    "chat": {"id": 42, "type": "private"}, "from": {"id": 111}, "sticker": {} }},
                { "update_id": 12, "edited_message": {  // not a fresh message → skipped
                    "chat": {"id": 42, "type": "private"}, "from": {"id": 111}, "text": "edit" }},
                { "update_id": 13, "message": {  // no from → skipped
                    "chat": {"id": 9, "type": "private"}, "text": "anon" }},
                { "update_id": 14, "message": {  // group chat → skipped in PR1 (DM-only)
                    "chat": {"id": -100, "type": "supergroup"}, "from": {"id": 111}, "text": "grp" }},
            ]
        });
        let msgs = parse_updates(&body);
        assert_eq!(msgs.len(), 1, "only the private text DM survives");
        assert_eq!(
            msgs[0],
            Incoming {
                update_id: 10,
                chat_id: 42,
                from_user_id: 111,
                text: "hello".into()
            }
        );
    }

    #[test]
    fn parse_updates_empty_result_is_empty() {
        assert!(parse_updates(&json!({"ok": true, "result": []})).is_empty());
        assert!(parse_updates(&json!({"ok": true})).is_empty());
    }
}
