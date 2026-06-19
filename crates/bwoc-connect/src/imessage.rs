//! iMessage transport (chat-connectors — iMessage, #229). **macOS-only, free.**
//!
//! There is no server/cross-platform iMessage API: every method drives
//! Messages.app on a logged-in Mac. So this transport runs **only on a macOS
//! agent host** signed into iMessage and bridges Apple Messages to a BWOC agent
//! on the *same* Mac, with no API key and no subscription:
//!
//! - **receive** — poll the local `~/Library/Messages/chat.db` (SQLite, opened
//!   **read-only**). `message.ROWID` is monotonic → it maps straight onto the
//!   bridge's integer [`Transport::poll`] offset. Needs **Full Disk Access**.
//! - **send** — shell `osascript` → Messages.app (`send … to buddy …`). Public
//!   AppleScript, no SIP changes. Needs an **Automation** TCC grant.
//!
//! **Identity (option B, #229).** iMessage peers are *string* handles (phone /
//! email / chat GUID), but the `Incoming`/allow-list seam is `i64`. Rather than
//! a framework-wide id change, handles are hashed to a stable `i64` ([`hash_id`],
//! the same scheme as `line::hash_id`); `main` hashes the configured
//! `allow_handles` the same way. The transport keeps a `hash → handle` map built
//! during `poll` so [`Transport::send`] can address the `osascript` back to the
//! real handle.
//!
//! **Non-streaming** ([`Transport::supports_edit`] = `false`): AppleScript can't
//! edit a sent message, so the bridge single-sends the reply on turn end (like
//! LINE). Editing/streaming would need the BlueBubbles / `imessage-rs` private
//! API — out of scope for this free MVP.
//!
//! **Caveats (surface to users):** the agent speaks as the **Mac's own Apple
//! ID** (no bot identity — replies look like they came from you); automating
//! Messages is against Apple's ToS (personal-use only); both TCC grants are
//! manual one-time approvals.
//!
//! Testability: the pure helpers ([`hash_id`], [`escape_applescript`],
//! [`build_send_script`], [`decode_message_text`]) are unit-tested and compile
//! on every platform; the live `chat.db` poll + `osascript` send are the
//! macOS-only, eyeball-reviewed edge (no iMessage in CI).

/// Stable `i64` for an iMessage handle string (first 8 bytes of its SHA-256,
/// big-endian). Lets string handles ride the `i64` `Incoming`/allow-list seam;
/// collisions are cryptographically negligible for a handful of allow-listed
/// handles. Same scheme as `line::hash_id`.
pub fn hash_id(s: &str) -> i64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(s.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

/// Escape a string for inclusion in an AppleScript double-quoted literal.
/// Backslash and double-quote are escaped; newlines become `" & return & "` so
/// a multi-line reply stays a valid one-line literal concatenation (AppleScript
/// string literals can't contain a raw newline).
pub fn escape_applescript(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\" & return & \""),
            '\r' => {} // drop CR (a CRLF becomes one `return` via the \n arm)
            _ => out.push(ch),
        }
    }
    out
}

/// Build the `osascript` source that sends `text` to `handle` over iMessage.
/// `handle` and `text` are escaped into AppleScript literals. The script
/// resolves the iMessage service and sends to the buddy by handle (phone/email).
pub fn build_send_script(handle: &str, text: &str) -> String {
    format!(
        "tell application \"Messages\"\n\
         \tset targetService to 1st account whose service type = iMessage\n\
         \tset targetBuddy to participant \"{handle}\" of targetService\n\
         \tsend \"{text}\" to targetBuddy\n\
         end tell",
        handle = escape_applescript(handle),
        text = escape_applescript(text),
    )
}

/// Resolve the displayable text of a message row. Prefers the plain `text`
/// column; when it is `NULL`/empty (increasingly common on recent macOS, where
/// the body moved to `attributedBody`), falls back to a **best-effort** decode
/// of the `attributedBody` typedstream blob. Returns `None` when neither yields
/// text — the caller then skips the row rather than surfacing an empty/garbled
/// message.
pub fn decode_message_text(text: Option<&str>, attributed_body: Option<&[u8]>) -> Option<String> {
    if let Some(t) = text {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    attributed_body.and_then(decode_attributed_body)
}

/// Best-effort extraction of the plain string from an `attributedBody`
/// NSAttributedString typedstream blob. The plain text follows the `NSString`
/// class marker as a length-prefixed UTF-8 run: after `NSString` come a few
/// type bytes, a `+` (0x2B) sentinel, then the length (one byte, or `0x81`
/// followed by a little-endian `u16` for longer strings), then the bytes.
/// Returns `None` on any inconsistency (bounds, invalid UTF-8) — conservative on
/// purpose, since a wrong guess would surface garbage to the agent.
fn decode_attributed_body(blob: &[u8]) -> Option<String> {
    let marker = b"NSString";
    let start = blob.windows(marker.len()).position(|w| w == marker)? + marker.len();
    // Find the `+` (0x2B) sentinel that precedes the length, within a small
    // window after the marker (the intervening bytes are fixed type tags).
    let plus_rel = blob[start..].iter().take(16).position(|&b| b == b'+')?;
    let mut i = start + plus_rel + 1;
    let len_byte = *blob.get(i)?;
    i += 1;
    let len: usize = if len_byte == 0x81 {
        let lo = *blob.get(i)? as usize;
        let hi = *blob.get(i + 1)? as usize;
        i += 2;
        lo | (hi << 8)
    } else {
        len_byte as usize
    };
    let end = i.checked_add(len)?;
    let slice = blob.get(i..end)?;
    std::str::from_utf8(slice).ok().map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// macOS-only live transport
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub use macos::ImessageTransport;

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    use async_trait::async_trait;
    use rusqlite::{Connection, OpenFlags};

    use super::{build_send_script, decode_message_text, hash_id};
    use crate::{ConnectError, Incoming, Transport};

    /// Default poll cadence between `chat.db` reads (no long-poll for SQLite).
    const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

    /// Live iMessage transport over the local `chat.db` + `osascript`.
    pub struct ImessageTransport {
        db_path: PathBuf,
        poll_interval: Duration,
        /// `hash_id(handle) → handle` so `send` can address `osascript` back to
        /// the real phone/email. Populated as messages are polled.
        handles: Mutex<HashMap<i64, String>>,
    }

    impl ImessageTransport {
        /// Build a transport reading `db_path`. Verifies the DB is present and
        /// readable up front (Full Disk Access), failing with a clear message
        /// rather than silently polling nothing.
        pub fn open(db_path: &Path, poll_interval_secs: Option<u64>) -> Result<Self, ConnectError> {
            if !db_path.exists() {
                return Err(ConnectError::Transport(format!(
                    "iMessage chat.db not found at {} — is this a Mac signed into iMessage, \
                     and does the process have Full Disk Access?",
                    db_path.display()
                )));
            }
            // Probe read-only open now so a permission error surfaces at startup.
            Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(
                |e| {
                    ConnectError::Transport(format!(
                        "cannot open {} read-only ({e}). Grant Full Disk Access to the process.",
                        db_path.display()
                    ))
                },
            )?;
            Ok(Self {
                db_path: db_path.to_path_buf(),
                poll_interval: poll_interval_secs
                    .map(Duration::from_secs)
                    .unwrap_or(DEFAULT_POLL_INTERVAL),
                handles: Mutex::new(HashMap::new()),
            })
        }

        /// Read incoming (`is_from_me = 0`) DM rows with `ROWID >= offset`,
        /// newest-handle map updated as a side effect.
        fn read_new(&self, offset: i64) -> Result<Vec<Incoming>, ConnectError> {
            let conn = Connection::open_with_flags(&self.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| ConnectError::Transport(format!("open chat.db: {e}")))?;
            // DM-first MVP: restrict to **1:1** chats and skip group rooms.
            // `chat.style` is 45 for a direct (1:1) chat and 43 for a group, so
            // the join to chat_message_join → chat + `c.style = 45` filters group
            // messages out in SQL — without it a group message would be emitted
            // as a DM to its sender (wrong routing + a privacy leak). Group
            // addressing via chat GUID is a follow-up. `GROUP BY m.ROWID` dedupes
            // a message that maps to more than one chat row.
            let mut stmt = conn
                .prepare(
                    "SELECT m.ROWID, h.id, m.text, m.attributedBody \
                     FROM message m \
                     JOIN handle h ON m.handle_id = h.ROWID \
                     JOIN chat_message_join cmj ON cmj.message_id = m.ROWID \
                     JOIN chat c ON c.ROWID = cmj.chat_id \
                     WHERE m.ROWID >= ?1 AND m.is_from_me = 0 AND c.style = 45 \
                     GROUP BY m.ROWID \
                     ORDER BY m.ROWID ASC",
                )
                .map_err(|e| ConnectError::Transport(format!("prepare query: {e}")))?;
            let rows = stmt
                .query_map([offset], |row| {
                    let rowid: i64 = row.get(0)?;
                    let handle: String = row.get(1)?;
                    let text: Option<String> = row.get(2)?;
                    let body: Option<Vec<u8>> = row.get(3)?;
                    Ok((rowid, handle, text, body))
                })
                .map_err(|e| ConnectError::Transport(format!("run query: {e}")))?;

            let mut out = Vec::new();
            let mut map = self.handles.lock().expect("handles mutex poisoned");
            for r in rows {
                let (rowid, handle, text, body) =
                    r.map_err(|e| ConnectError::Transport(format!("read row: {e}")))?;
                let Some(text) = decode_message_text(text.as_deref(), body.as_deref()) else {
                    continue; // undecodable body → skip rather than surface garbage
                };
                let id = hash_id(&handle);
                map.insert(id, handle);
                out.push(Incoming {
                    update_id: rowid,
                    chat_id: id,
                    from_user_id: id,
                    text,
                    is_group: false,
                    mentions_bot: false,
                });
            }
            Ok(out)
        }
    }

    #[async_trait]
    impl Transport for ImessageTransport {
        async fn poll(&self, offset: i64) -> Result<Vec<Incoming>, ConnectError> {
            // No long-poll for a local SQLite file: pace the reads so the bridge
            // doesn't busy-loop, then return whatever is new (possibly empty).
            // The rusqlite read is brief (indexed ROWID range on a local file).
            tokio::time::sleep(self.poll_interval).await;
            self.read_new(offset)
        }

        async fn send(&self, chat_id: i64, text: &str) -> Result<i64, ConnectError> {
            let handle = {
                let map = self.handles.lock().expect("handles mutex poisoned");
                map.get(&chat_id).cloned()
            };
            let Some(handle) = handle else {
                return Err(ConnectError::Transport(format!(
                    "no known iMessage handle for chat {chat_id} (cannot address a reply to a \
                     peer not seen this run)"
                )));
            };
            let script = build_send_script(&handle, text);
            let output = tokio::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                // Reap the child if this task is cancelled mid-send (don't orphan
                // an osascript process) — matches the rest of the crate.
                .kill_on_drop(true)
                .output()
                .await
                .map_err(|e| ConnectError::Transport(format!("spawn osascript: {e}")))?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(ConnectError::Transport(format!(
                    "osascript send failed: {}. Grant Automation (Messages) access to the process.",
                    err.trim()
                )));
            }
            // iMessage gives no usable sent-message id for editing; supports_edit
            // is false so the bridge never tries to edit. Return 0.
            Ok(0)
        }

        async fn edit(&self, _chat: i64, _mid: i64, _text: &str) -> Result<(), ConnectError> {
            // Unreachable: supports_edit() == false means the bridge single-sends.
            Err(ConnectError::Transport(
                "iMessage does not support message edit (no streaming)".into(),
            ))
        }

        fn supports_edit(&self) -> bool {
            false
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Build a minimal `chat.db` (the columns `read_new` touches) at a unique
        /// temp path, seed it, and return the path.
        fn seed_db(label: &str) -> PathBuf {
            let path = std::env::temp_dir()
                .join(format!("bwoc-im-test-{}-{label}.db", std::process::id()));
            let _ = std::fs::remove_file(&path);
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT);
                 CREATE TABLE message (ROWID INTEGER PRIMARY KEY, handle_id INTEGER, \
                    text TEXT, attributedBody BLOB, is_from_me INTEGER);
                 CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, style INTEGER);
                 CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);
                 INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567');
                 INSERT INTO handle (ROWID, id) VALUES (2, '+15559990000');
                 -- a 1:1 chat (style 45) and a group chat (style 43)
                 INSERT INTO chat (ROWID, style) VALUES (100, 45);
                 INSERT INTO chat (ROWID, style) VALUES (200, 43);
                 -- incoming DM (must be returned)
                 INSERT INTO message (ROWID, handle_id, text, attributedBody, is_from_me) \
                    VALUES (10, 1, 'hello there', NULL, 0);
                 INSERT INTO chat_message_join (chat_id, message_id) VALUES (100, 10);
                 -- our own outbound reply (must be skipped: is_from_me=1)
                 INSERT INTO message (ROWID, handle_id, text, attributedBody, is_from_me) \
                    VALUES (11, 1, 'my reply', NULL, 1);
                 INSERT INTO chat_message_join (chat_id, message_id) VALUES (100, 11);
                 -- incoming GROUP message (must be skipped: chat.style=43)
                 INSERT INTO message (ROWID, handle_id, text, attributedBody, is_from_me) \
                    VALUES (12, 2, 'group hi', NULL, 0);
                 INSERT INTO chat_message_join (chat_id, message_id) VALUES (200, 12);",
            )
            .unwrap();
            path
        }

        #[test]
        fn read_new_maps_incoming_skips_outbound_and_fills_handle_map() {
            let path = seed_db("map");
            let t = ImessageTransport::open(&path, Some(0)).unwrap();
            let got = t.read_new(0).unwrap();
            assert_eq!(
                got.len(),
                1,
                "only the inbound 1:1 DM — outbound (is_from_me=1) and the group \
                 message (chat.style=43) are both excluded"
            );
            assert!(
                !got.iter().any(|m| m.update_id == 12),
                "the group-room message must not be surfaced as a DM"
            );
            let m = &got[0];
            assert_eq!(m.update_id, 10);
            assert_eq!(m.text, "hello there");
            assert_eq!(m.from_user_id, hash_id("+15551234567"));
            assert_eq!(m.chat_id, m.from_user_id, "DM: reply target == sender");
            assert!(!m.is_group);
            // send() can now resolve the handle for this chat.
            let handle = t.handles.lock().unwrap().get(&m.chat_id).cloned();
            assert_eq!(handle.as_deref(), Some("+15551234567"));
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn read_new_respects_offset() {
            let path = seed_db("offset");
            let t = ImessageTransport::open(&path, Some(0)).unwrap();
            // Offset past the only inbound row (ROWID 10) → nothing.
            assert!(t.read_new(11).unwrap().iter().all(|m| m.update_id >= 11));
            assert_eq!(t.read_new(11).unwrap().len(), 0);
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_id_is_stable_and_distinct() {
        assert_eq!(hash_id("+15551234567"), hash_id("+15551234567"));
        assert_ne!(hash_id("+15551234567"), hash_id("a@b.com"));
    }

    #[test]
    fn applescript_escaping() {
        assert_eq!(escape_applescript(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_applescript(r"a\b"), r"a\\b");
        // Newline becomes a `return` concatenation; CR is dropped.
        assert_eq!(escape_applescript("a\r\nb"), "a\" & return & \"b");
    }

    #[test]
    fn send_script_embeds_escaped_handle_and_text() {
        let s = build_send_script("+15551234567", r#"he said "hi"#);
        assert!(s.contains("participant \"+15551234567\""));
        assert!(s.contains(r#"send "he said \"hi""#));
        assert!(s.contains("service type = iMessage"));
    }

    #[test]
    fn decode_prefers_plain_text() {
        assert_eq!(
            decode_message_text(Some("hello"), Some(b"ignored")),
            Some("hello".to_string())
        );
        // Empty/whitespace text falls through to the body.
        assert_eq!(decode_message_text(Some("   "), None), None);
    }

    #[test]
    fn decode_attributed_body_extracts_string() {
        // Synthetic blob: `NSString` marker, type tags, `+`, length, UTF-8.
        let msg = "warm reply";
        let mut blob = Vec::new();
        blob.extend_from_slice(b"some prefix NSString");
        blob.extend_from_slice(&[0x01, 0x94, 0x84, 0x01]); // type tags
        blob.push(b'+');
        blob.push(msg.len() as u8);
        blob.extend_from_slice(msg.as_bytes());
        blob.extend_from_slice(&[0x86, 0x84]); // trailer
        assert_eq!(
            decode_message_text(None, Some(&blob)),
            Some(msg.to_string())
        );
    }

    #[test]
    fn decode_attributed_body_two_byte_length() {
        // 0x81 + LE u16 length form, for strings ≥ 128 bytes.
        let msg = "x".repeat(200);
        let mut blob = Vec::new();
        blob.extend_from_slice(b"NSString");
        blob.push(b'+');
        blob.push(0x81);
        blob.push((msg.len() & 0xff) as u8);
        blob.push((msg.len() >> 8) as u8);
        blob.extend_from_slice(msg.as_bytes());
        assert_eq!(decode_message_text(None, Some(&blob)), Some(msg));
    }

    #[test]
    fn decode_attributed_body_rejects_garbage() {
        assert_eq!(decode_message_text(None, Some(b"no marker here")), None);
        assert_eq!(decode_message_text(None, None), None);
        // Marker present but length overruns the buffer → None (no panic).
        let mut blob = Vec::new();
        blob.extend_from_slice(b"NSString+");
        blob.push(0x50); // claims 80 bytes, none follow
        assert_eq!(decode_message_text(None, Some(&blob)), None);
    }
}
