//! Inbox delivery — the single, idempotent writer for `.bwoc/inbox.jsonl`.
//!
//! Every sender (`bwoc send`, the a2a/gateway receiver, council notifications)
//! appends one JSON-line envelope carrying a `messageId`. Delivering the *same*
//! `messageId` more than once produced visible duplicates — council notices
//! arrived 3× and retried "re-nudges" stacked up (#299). This module is the one
//! append path they all go through, so a re-delivery of an already-present
//! `messageId` is suppressed.
//!
//! **Concurrency:** delivery is check-then-append, which dedups *sequential*
//! re-sends (the reported case: a notify loop / a blind retry). It does not lock,
//! so two processes appending the same id in the very same instant could still
//! both write — acceptable for an agent's own inbox, where writes are effectively
//! serial; a `flock` is a later hardening if a real concurrent writer appears.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// The outcome of an inbox delivery — a minimal **delivery receipt** the sender
/// can surface ("delivered" vs "already there, suppressed").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The envelope was appended (first time this `messageId` reached the inbox).
    Delivered,
    /// An envelope with the same `messageId` was already present — not appended.
    Duplicate,
}

/// Append `line` (a serialized envelope JSON whose `messageId` is `message_id`)
/// to the inbox at `path`, **unless** an envelope with that `messageId` is
/// already present. Creates the parent directory. Returns whether it was a fresh
/// delivery or a suppressed duplicate.
pub fn append_envelope_deduped(path: &Path, message_id: &str, line: &str) -> io::Result<Delivery> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if is_duplicate(path, message_id)? {
        return Ok(Delivery::Duplicate);
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(Delivery::Delivered)
}

/// True when the inbox at `path` already holds an envelope with `messageId ==
/// message_id`. A missing inbox is "not present" (no one has written yet);
/// malformed lines are skipped (they can't be the id we're looking for).
///
/// Exposed for writers that have their own append path (e.g. the a2a receiver's
/// size-capped writer) and only need the dedup *check* before delivering.
pub fn is_duplicate(path: &Path, message_id: &str) -> io::Result<bool> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if v.get("messageId").and_then(|m| m.as_str()) == Some(message_id) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, msg: &str) -> String {
        serde_json::json!({ "messageId": id, "from": "u", "to": "agent-x", "message": msg })
            .to_string()
    }

    #[test]
    fn first_delivery_appends_duplicate_is_suppressed() {
        let dir = std::env::temp_dir().join(format!("bwoc-inbox-dedup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("inbox.jsonl");

        assert_eq!(
            append_envelope_deduped(&path, "m1", &line("m1", "hello")).unwrap(),
            Delivery::Delivered
        );
        // Same id again → suppressed, file unchanged at one line.
        assert_eq!(
            append_envelope_deduped(&path, "m1", &line("m1", "hello again")).unwrap(),
            Delivery::Duplicate
        );
        // A different id → delivered.
        assert_eq!(
            append_envelope_deduped(&path, "m2", &line("m2", "next")).unwrap(),
            Delivery::Delivered
        );

        let body = fs::read_to_string(&path).unwrap();
        assert_eq!(body.lines().filter(|l| !l.trim().is_empty()).count(), 2);
        assert!(body.contains("\"messageId\":\"m1\""));
        assert!(body.contains("\"messageId\":\"m2\""));
        assert!(!body.contains("hello again"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_inbox_is_not_a_duplicate() {
        let path = std::env::temp_dir()
            .join(format!("bwoc-inbox-absent-{}", std::process::id()))
            .join("inbox.jsonl");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        assert!(!is_duplicate(&path, "anything").unwrap());
    }
}
