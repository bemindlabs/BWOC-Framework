//! Durable idempotency / edge-trigger ledger.
//!
//! A tiny, dependency-free (`std` + `serde_json`) key→value store persisted as a
//! sidecar file, giving loops two things a poll-driven loop needs to act **once**
//! rather than every fire:
//!
//! - [`IdempotencyLedger::seen_or_record`] — one-shot idempotency: "have I ever
//!   acted on this key?" Records the key and reports whether it was already
//!   present. For period-bucket keys (`loop:2026-08-19`) so a recurring digest
//!   is delivered once per period, or message-id keys for at-most-once handling.
//! - [`IdempotencyLedger::latch`] — edge-trigger: "did this key's value just
//!   change?" Records the new value and reports whether it differs from the last.
//!   For "alert once per trip" — key on the loop id, value on the predicate
//!   outcome, so an alert fires on the OK→TRIP transition and stays quiet while
//!   still tripped, then fires again on a *later* trip (a plain seen-set cannot:
//!   it would suppress the second trip forever).
//!
//! Both operations share one durable map, written atomically (tmp + rename) so a
//! crash between check and record can't corrupt it. The ledger is small — one
//! entry per active key (a handful of loops; period buckets accumulate slowly) —
//! so a load-modify-save per call is cheap. Retention/GC (prune stale keys) is a
//! later concern, tracked when a period-bucket ledger actually grows; the on-disk
//! shape carries no schema that would block adding it.
//!
//! **Concurrency:** like [`crate::inbox`], this is check-then-write without a
//! lock — correct for a single loop process owning its own ledger. Two processes
//! mutating the *same* ledger concurrently is out of scope (a loop owns its
//! ledger); an advisory lock is a later hardening if that ever appears.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A durable `key → value` ledger backed by a sidecar file.
#[derive(Debug, Clone)]
pub struct IdempotencyLedger {
    path: PathBuf,
}

impl IdempotencyLedger {
    /// Open (lazily — nothing is read until an operation) the ledger at `path`.
    /// The parent directory is created on first write.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// One-shot idempotency. Returns `true` when `key` was **already recorded**
    /// (the caller should skip); otherwise records it and returns `false` (the
    /// caller should act — this is the first time).
    pub fn seen_or_record(&self, key: &str) -> io::Result<bool> {
        let mut map = self.load()?;
        if map.contains_key(key) {
            return Ok(true);
        }
        map.insert(key.to_string(), String::new());
        self.save(&map)?;
        Ok(false)
    }

    /// Edge-trigger. Records `value` for `key` and returns `true` iff it
    /// **differs** from the previously-recorded value — a transition the caller
    /// acts on once. An unchanged value returns `false` (suppress). A key with no
    /// prior value counts as changed, so the first observation is a transition
    /// (a monitor that starts against an already-tripped source alerts on
    /// startup; seed silently by ignoring the first result if that is unwanted).
    pub fn latch(&self, key: &str, value: &str) -> io::Result<bool> {
        let mut map = self.load()?;
        let changed = map.get(key).map(|v| v != value).unwrap_or(true);
        if changed {
            map.insert(key.to_string(), value.to_string());
            self.save(&map)?;
        }
        Ok(changed)
    }

    /// The current recorded value for `key`, if any (a read that does not mutate).
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        Ok(self.load()?.get(key).cloned())
    }

    // ── storage ──

    /// Load the map. A missing file is an empty ledger; malformed lines are
    /// skipped (a partial line can't be a key we'd match).
    fn load(&self) -> io::Result<BTreeMap<String, String>> {
        let raw = match fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e),
        };
        let mut map = BTreeMap::new();
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
                && let (Some(k), Some(val)) = (
                    v.get("k").and_then(|x| x.as_str()),
                    v.get("v").and_then(|x| x.as_str()),
                )
            {
                // Later lines win (append-tolerant), though `save` rewrites the
                // whole map so duplicates don't normally accumulate.
                map.insert(k.to_string(), val.to_string());
            }
        }
        Ok(map)
    }

    /// Write the whole map atomically: serialize to a temp sibling, then rename
    /// over the target so a reader never sees a half-written ledger.
    fn save(&self, map: &BTreeMap<String, String>) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut out = String::new();
        for (k, v) in map {
            out.push_str(&serde_json::json!({ "k": k, "v": v }).to_string());
            out.push('\n');
        }
        let tmp = tmp_path(&self.path);
        // Clean up the staged temp on any failure so a write/rename error (e.g.
        // permission, cross-device) can't accumulate `.tmp` litter across runs.
        if let Err(e) = fs::write(&tmp, out.as_bytes()) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        if let Err(e) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }
}

/// A unique-enough temp sibling for the atomic write (pid-suffixed so two
/// ledgers under the same dir don't collide on their temp files).
fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("ledger"));
    name.push(format!(".{}.tmp", std::process::id()));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> (IdempotencyLedger, PathBuf) {
        // Unique per call so parallel tests never share a dir (a monotonic
        // counter + pid — a shared dir would race remove_dir_all across threads).
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bwoc-idem-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("ledger.jsonl");
        (IdempotencyLedger::new(path.clone()), dir)
    }

    #[test]
    fn seen_or_record_is_once_only() {
        let (l, dir) = ledger();
        assert!(!l.seen_or_record("a").unwrap(), "first time → act (false)");
        assert!(l.seen_or_record("a").unwrap(), "second time → seen (true)");
        assert!(!l.seen_or_record("b").unwrap(), "different key → act");
        assert!(l.seen_or_record("b").unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn latch_fires_on_transition_and_suppresses_repeats() {
        let (l, dir) = ledger();
        // First observation = transition (from unknown).
        assert!(
            l.latch("svc", "TRIP").unwrap(),
            "first observation is a change"
        );
        // Still tripped → suppressed.
        assert!(!l.latch("svc", "TRIP").unwrap());
        assert!(!l.latch("svc", "TRIP").unwrap());
        // Recovers → transition.
        assert!(l.latch("svc", "OK").unwrap());
        assert!(!l.latch("svc", "OK").unwrap());
        // Trips AGAIN → transition (a seen-set would wrongly suppress this).
        assert!(l.latch("svc", "TRIP").unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn latch_and_get_are_durable_across_reopen() {
        let (l, dir) = ledger();
        assert!(l.latch("svc", "TRIP").unwrap());
        // A fresh handle on the same path sees the recorded value.
        let reopened = IdempotencyLedger::new(l.path.clone());
        assert_eq!(reopened.get("svc").unwrap().as_deref(), Some("TRIP"));
        // And suppresses the unchanged value.
        assert!(!reopened.latch("svc", "TRIP").unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_ledger_reads_empty() {
        let path = std::env::temp_dir()
            .join(format!("bwoc-idem-absent-{}", std::process::id()))
            .join("ledger.jsonl");
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let l = IdempotencyLedger::new(path);
        assert_eq!(l.get("x").unwrap(), None);
        assert!(!l.seen_or_record("x").unwrap());
    }

    #[test]
    fn malformed_lines_are_skipped_not_fatal() {
        let (l, dir) = ledger();
        l.seen_or_record("good").unwrap();
        // Corrupt the file with a junk line; load must tolerate it.
        let mut body = fs::read_to_string(&l.path).unwrap();
        body.push_str("not json at all\n");
        fs::write(&l.path, body).unwrap();
        assert!(l.seen_or_record("good").unwrap(), "existing key still seen");
        assert!(!l.seen_or_record("fresh").unwrap(), "new key still records");
        let _ = fs::remove_dir_all(dir);
    }
}
