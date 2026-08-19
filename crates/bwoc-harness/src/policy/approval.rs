//! Human-in-the-loop approval channel (Phase 6).
//!
//! When a tool resolves to `ask` but there is **no controlling TTY** (a fleet
//! agent spawned by `bwoc spawn` / the macOS control center), the operator can
//! still be reached out-of-band: the harness emits an [`ApprovalRequest`] over
//! an [`ApprovalChannel`] and blocks for the decision. The
//! [`FileApprovalChannel`] realises this over the same file-queue idiom the rest
//! of BWOC uses — a request appears under `<workspace>/.bwoc/approvals/pending/`
//! and a console (e.g. `bwoc-mcc`) writes the verdict to `decided/`.
//!
//! **Fail-safe is preserved.** The channel is an *extension* of `ask`, never a
//! bypass: a timeout or any I/O error returns `None`, and the caller then
//! applies the exact same fail-safe it would have applied with no channel at
//! all (deny for high-blast-radius tools / `default_mode` otherwise). A channel
//! can only ever turn a would-be *deny* into an *allow* with an explicit human
//! yes — it can never weaken a deny.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A pending request for operator approval of a gated tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique id — also the filename stem for the request/decision pair.
    pub id: String,
    /// The agent whose turn triggered the request.
    pub agent: String,
    /// The tool being gated.
    pub tool: String,
    /// A **truncated** argument preview — never the full args (may be large or
    /// carry sensitive content); enough for a human to judge intent.
    pub args_preview: String,
    /// The turn's trust level, rendered (e.g. `"Untrusted"`), for the UI badge.
    pub trust: String,
    /// Request timestamp (ms since epoch).
    pub ts_ms: u128,
    /// How long the harness will wait before falling back to fail-safe.
    pub timeout_s: u64,
}

impl ApprovalRequest {
    /// Longest argument preview carried in a request (chars).
    const PREVIEW_CAP: usize = 400;

    /// Build a request, stamping a unique id + timestamp. `args_preview` is
    /// truncated on a char boundary to [`Self::PREVIEW_CAP`].
    pub fn new(
        agent: impl Into<String>,
        tool: impl Into<String>,
        args: &str,
        trust: impl Into<String>,
        timeout_s: u64,
    ) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!("{ts_ms}-{}-{seq}", std::process::id());
        let preview: String = args.chars().take(Self::PREVIEW_CAP).collect();
        Self {
            id,
            agent: agent.into(),
            tool: tool.into(),
            args_preview: preview,
            trust: trust.into(),
            ts_ms,
            timeout_s,
        }
    }
}

/// The operator's decision, written back by the console.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// True → allow the call; false → deny it.
    pub allow: bool,
    /// Operator asked to remember this. When `allow && always`, the harness
    /// records a **session-scoped** grant keyed on `(tool, args)`, so subsequent
    /// identical calls in the same process skip the prompt (see
    /// `permission::Policy::session_grants`, #409). In-memory only — never
    /// written to `harness-policy.toml`; durable rules stay human-authored.
    #[serde(default)]
    pub always: bool,
    /// Who decided (free-form, e.g. the console's user/host) — provenance only.
    #[serde(default)]
    pub by: String,
}

/// A channel that can escalate an `ask` to a human when no TTY is available.
pub trait ApprovalChannel: Send + Sync + std::fmt::Debug {
    /// Block until the operator decides, or return `None` on timeout / I/O
    /// error — the caller then applies the existing fail-safe.
    fn request(&self, req: &ApprovalRequest) -> Option<ApprovalDecision>;
}

/// File-based channel over `<root>/{pending,decided}/<id>.json`.
///
/// Writes the request atomically (tmp + rename) so a watcher never observes a
/// half-written file, polls for the decision until `req.timeout_s`, and cleans
/// up both files on the way out.
#[derive(Debug, Clone)]
pub struct FileApprovalChannel {
    root: PathBuf,
    poll: Duration,
}

impl FileApprovalChannel {
    /// `root` is `<workspace>/.bwoc/approvals`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            poll: Duration::from_millis(500),
        }
    }

    /// Test hook: tighter poll interval.
    #[cfg(test)]
    fn with_poll(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    fn pending_path(&self, id: &str) -> PathBuf {
        self.root.join("pending").join(format!("{id}.json"))
    }
    fn decided_path(&self, id: &str) -> PathBuf {
        self.root.join("decided").join(format!("{id}.json"))
    }
}

impl ApprovalChannel for FileApprovalChannel {
    fn request(&self, req: &ApprovalRequest) -> Option<ApprovalDecision> {
        let pending = self.pending_path(&req.id);
        let decided = self.decided_path(&req.id);
        std::fs::create_dir_all(pending.parent()?).ok()?;
        std::fs::create_dir_all(decided.parent()?).ok()?;

        // Atomic publish: write a temp file then rename into place. Clean the
        // temp up on either failure path so a permission/IO error never leaves a
        // stray `*.tmp` cluttering `pending/`.
        let payload = serde_json::to_vec(req).ok()?;
        let tmp = pending.with_extension("tmp");
        if std::fs::write(&tmp, &payload).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        if std::fs::rename(&tmp, &pending).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return None;
        }

        let deadline = Instant::now() + Duration::from_secs(req.timeout_s);
        let result = loop {
            if let Ok(bytes) = std::fs::read(&decided) {
                break serde_json::from_slice::<ApprovalDecision>(&bytes).ok();
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(self.poll);
        };
        // Best-effort cleanup — leftover files are harmless (the ids are unique).
        let _ = std::fs::remove_file(&pending);
        let _ = std::fs::remove_file(&decided);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_truncates_preview_on_char_boundary() {
        let big = "x".repeat(1000);
        let r = ApprovalRequest::new("agent-a", "run_command", &big, "Untrusted", 5);
        assert_eq!(r.args_preview.chars().count(), ApprovalRequest::PREVIEW_CAP);
        assert!(!r.id.is_empty());
        assert_eq!(r.tool, "run_command");
    }

    #[test]
    fn request_ids_are_unique() {
        let a = ApprovalRequest::new("x", "t", "{}", "T", 1);
        let b = ApprovalRequest::new("x", "t", "{}", "T", 1);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn file_channel_times_out_to_none_when_undecided() {
        let dir = tempfile::tempdir().unwrap();
        let ch = FileApprovalChannel::new(dir.path().join(".bwoc/approvals"))
            .with_poll(Duration::from_millis(20));
        let req = ApprovalRequest::new("agent-a", "write_file", "{}", "Untrusted", 0);
        // timeout_s = 0 → the poll loop sees the deadline immediately.
        assert!(ch.request(&req).is_none());
    }

    #[test]
    fn file_channel_returns_operator_decision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".bwoc/approvals");
        let ch = FileApprovalChannel::new(root.clone()).with_poll(Duration::from_millis(20));
        let req = ApprovalRequest::new(
            "agent-a",
            "run_command",
            r#"{"command":"ls"}"#,
            "Untrusted",
            5,
        );
        let id = req.id.clone();
        // Simulate the console: once the pending request appears, write a verdict.
        let handle = std::thread::spawn(move || {
            let pending = root.join("pending").join(format!("{id}.json"));
            for _ in 0..200 {
                if pending.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let decided_dir = root.join("decided");
            std::fs::create_dir_all(&decided_dir).unwrap();
            let d = ApprovalDecision {
                allow: true,
                always: false,
                by: "operator@mac".into(),
            };
            std::fs::write(
                decided_dir.join(format!("{id}.json")),
                serde_json::to_vec(&d).unwrap(),
            )
            .unwrap();
        });
        let decision = ch.request(&req);
        handle.join().unwrap();
        let d = decision.expect("operator decision delivered");
        assert!(d.allow);
        assert_eq!(d.by, "operator@mac");
    }
}
