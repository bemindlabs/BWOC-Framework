//! Sender-side outbox spool — a durable retry queue for messages that could not
//! be delivered live to an offline/unreachable remote peer.
//!
//! `bwoc send` to a gateway/MQTT peer that is offline would otherwise lose the
//! message: the relay's park is in-memory and evaporates on restart or if the
//! peer never reconnects. Instead the *signed* envelope is spooled here, and
//! `bwoc outbox flush` re-delivers it later. Re-delivery replays the stored
//! envelope verbatim (same `messageId` + signature), so the recipient's inbox
//! dedup (`inbox::append_envelope_deduped`) turns at-least-once retry into
//! effectively-once delivery.
//!
//! Layout: `<workspace>/.bwoc/outbox/<recipientId>.jsonl` — one queue per peer,
//! one JSON-line envelope per pending message. This is sender-side runtime state
//! (like an agent's `inbox.jsonl`), not tracked config.
//!
//! **Concurrency:** spool is check-then-append and flush is read-then-rewrite,
//! matching `inbox`'s deliberate no-lock stance — a workspace's own sends are
//! effectively serial. A `flock` is a later hardening if a real concurrent
//! writer appears.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// The `.bwoc/outbox/` directory for a workspace.
pub fn outbox_dir(workspace: &Path) -> PathBuf {
    workspace.join(".bwoc").join("outbox")
}

/// Spool file for one recipient. Callers pass a canonical `agent-<x>` id (a
/// single path segment), so it is a safe filename.
pub fn outbox_path(workspace: &Path, recipient_id: &str) -> PathBuf {
    outbox_dir(workspace).join(format!("{recipient_id}.jsonl"))
}

/// Append `line` to the recipient's spool unless its `message_id` is already
/// queued (don't double-spool a re-send). Returns `true` when newly spooled,
/// `false` when it was already pending.
pub fn spool(
    workspace: &Path,
    recipient_id: &str,
    message_id: &str,
    line: &str,
) -> io::Result<bool> {
    let path = outbox_path(workspace, recipient_id);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if contains_message_id(&path, message_id)? {
        return Ok(false);
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{line}")?;
    Ok(true)
}

/// All pending envelope lines for a recipient (empty if the spool is absent).
pub fn read_spooled(workspace: &Path, recipient_id: &str) -> io::Result<Vec<String>> {
    read_lines(&outbox_path(workspace, recipient_id))
}

/// Overwrite a recipient's spool with `lines`, removing the file when `lines` is
/// empty (a fully-drained queue leaves no stray file).
pub fn rewrite(workspace: &Path, recipient_id: &str, lines: &[String]) -> io::Result<()> {
    let path = outbox_path(workspace, recipient_id);
    if lines.is_empty() {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    fs::write(&path, body)
}

/// Every recipient with a non-empty spool as `(recipient_id, pending_count)`,
/// sorted by recipient id. Empty when the outbox directory does not exist.
pub fn list_pending(workspace: &Path) -> io::Result<Vec<(String, usize)>> {
    let dir = outbox_dir(workspace);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let count = read_lines(&path)?.len();
        if count > 0 {
            out.push((stem.to_string(), count));
        }
    }
    out.sort();
    Ok(out)
}

fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut lines = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }
    Ok(lines)
}

fn contains_message_id(path: &Path, message_id: &str) -> io::Result<bool> {
    for line in read_lines(path)? {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
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

    fn line(id: &str) -> String {
        serde_json::json!({ "messageId": id, "from": "agent-a", "to": "agent-b", "message": "hi" })
            .to_string()
    }

    fn ws(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-outbox-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        root
    }

    #[test]
    fn spool_dedups_by_message_id() {
        let root = ws("dedup");
        assert!(spool(&root, "agent-b", "m1", &line("m1")).unwrap());
        assert!(!spool(&root, "agent-b", "m1", &line("m1")).unwrap()); // already queued
        assert!(spool(&root, "agent-b", "m2", &line("m2")).unwrap());
        assert_eq!(read_spooled(&root, "agent-b").unwrap().len(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rewrite_drops_delivered_and_removes_empty() {
        let root = ws("rewrite");
        spool(&root, "agent-b", "m1", &line("m1")).unwrap();
        spool(&root, "agent-b", "m2", &line("m2")).unwrap();
        // Keep only m2 (m1 "delivered").
        rewrite(&root, "agent-b", &[line("m2")]).unwrap();
        let left = read_spooled(&root, "agent-b").unwrap();
        assert_eq!(left.len(), 1);
        assert!(left[0].contains("\"messageId\":\"m2\""));
        // Drain fully → file removed.
        rewrite(&root, "agent-b", &[]).unwrap();
        assert!(!outbox_path(&root, "agent-b").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_pending_counts_per_recipient() {
        let root = ws("list");
        assert!(list_pending(&root).unwrap().is_empty()); // no dir yet
        spool(&root, "agent-b", "m1", &line("m1")).unwrap();
        spool(&root, "agent-b", "m2", &line("m2")).unwrap();
        spool(&root, "agent-c", "m3", &line("m3")).unwrap();
        let pending = list_pending(&root).unwrap();
        assert_eq!(pending, vec![("agent-b".into(), 2), ("agent-c".into(), 1)]);
        let _ = fs::remove_dir_all(&root);
    }
}
