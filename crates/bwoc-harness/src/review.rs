//! Peer-review gate (HV3-3c) — a designated reviewer agent judges a worker's
//! diff before the lead completes the task.
//!
//! Decided shape (architect, 2026-06-06): **fixed reviewer per team**. When a
//! team declares a `reviewer` (see `bwoc_core::team::Team`), the lead routes
//! each successful worker's worktree to that agent for review. The reviewer is
//! a `bwoc-harness` subprocess run **in the worker's worktree** (so it can
//! `git diff` and read the changed files), and its verdict is read back from
//! the HV3-3b result envelope it leaves behind — the same seam the worker uses.
//!
//! Fail-safe: a spawn failure, a timeout, or an unparseable verdict all resolve
//! to **REJECT** (Sīla — unreviewed work never auto-completes), so the task is
//! re-queued rather than silently accepted.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::{HarnessError, HarnessResult};

/// Default wall-clock bound on a review (a read-only diff inspection should be
/// quicker than the work itself, but generous for a slow model).
pub const DEFAULT_REVIEW_TIMEOUT: Duration = Duration::from_secs(900);

/// The reviewer's decision on a worker's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewVerdict {
    pub approved: bool,
    /// One-line rationale (the reject reason, or the approval note).
    pub feedback: String,
}

impl ReviewVerdict {
    fn approve(feedback: impl Into<String>) -> Self {
        Self {
            approved: true,
            feedback: feedback.into(),
        }
    }
    fn reject(feedback: impl Into<String>) -> Self {
        Self {
            approved: false,
            feedback: feedback.into(),
        }
    }
}

/// What the lead hands a [`Reviewer`] for one review.
#[derive(Debug, Clone)]
pub struct ReviewSpec {
    pub task_id: String,
    /// The task the worker was working on (for the review prompt).
    pub task_title: String,
    /// The worker's worktree — the reviewer runs here to see the diff.
    pub worktree: PathBuf,
    /// Reviewer agent id (for diagnostics / the prompt).
    pub reviewer_agent: String,
    /// Model + endpoint the reviewer subprocess runs against (reused from the
    /// worker config for slice 1).
    pub model: String,
    pub endpoint: String,
}

/// Reviews a worker's diff and returns a verdict. Injectable so the lead loop is
/// testable without spawning real review subprocesses.
#[async_trait]
pub trait Reviewer: Send + Sync {
    async fn review(&self, spec: &ReviewSpec) -> ReviewVerdict;
}

/// Parse a reviewer's response text for its verdict line.
///
/// The reviewer is instructed to make its **first** line exactly
/// `VERDICT: APPROVE` or `VERDICT: REJECT: <reason>` (first line, so it survives
/// the envelope's leading-chars truncation). Pure + fail-safe: no recognizable
/// verdict → REJECT, so unreviewed work is never auto-approved.
pub fn parse_verdict(text: &str) -> ReviewVerdict {
    for line in text.lines() {
        let l = line.trim();
        let Some(rest) = l.strip_prefix("VERDICT:") else {
            continue;
        };
        let rest = rest.trim();
        if rest.eq_ignore_ascii_case("APPROVE") || rest.to_ascii_uppercase().starts_with("APPROVE")
        {
            return ReviewVerdict::approve("approved by reviewer");
        }
        if let Some(reason) = rest
            .strip_prefix("REJECT:")
            .or_else(|| rest.strip_prefix("reject:"))
        {
            return ReviewVerdict::reject(reason.trim().to_string());
        }
        if rest.eq_ignore_ascii_case("REJECT") {
            return ReviewVerdict::reject("rejected by reviewer (no reason given)");
        }
    }
    ReviewVerdict::reject("no parseable VERDICT line in the reviewer's response")
}

/// The review prompt handed to the reviewer subprocess.
fn review_prompt(task_title: &str) -> String {
    format!(
        "You are the peer reviewer for a Saṅgha team. A teammate just finished this task \
         in the current working directory:\n\n  {task_title}\n\n\
         Review their changes: inspect the working-tree diff (run `git diff HEAD` and read \
         the changed files) and judge whether the work correctly and safely accomplishes the \
         task — correctness, safety, and no obvious regressions. Do NOT modify any files.\n\n\
         Your FIRST line MUST be exactly one of:\n  \
         VERDICT: APPROVE\n  VERDICT: REJECT: <one-line reason>\n\
         Then explain your reasoning."
    )
}

/// Spawns a `bwoc-harness` subprocess as the reviewer, in the worker's worktree.
pub struct SubprocessReviewer {
    exe: PathBuf,
    timeout: Option<Duration>,
}

impl SubprocessReviewer {
    /// Review by spawning copies of the running executable (the lead is itself a
    /// `bwoc-harness`), bounded by [`DEFAULT_REVIEW_TIMEOUT`].
    pub fn new() -> HarnessResult<Self> {
        let exe = std::env::current_exe()
            .map_err(|e| HarnessError::Other(format!("cannot resolve current exe: {e}")))?;
        Ok(Self {
            exe,
            timeout: Some(DEFAULT_REVIEW_TIMEOUT),
        })
    }

    /// Point at a specific binary (tests). No timeout unless set.
    pub fn with_exe(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            timeout: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl Reviewer for SubprocessReviewer {
    async fn review(&self, spec: &ReviewSpec) -> ReviewVerdict {
        let mut cmd = tokio::process::Command::new(&self.exe);
        cmd.arg("--task")
            .arg(review_prompt(&spec.task_title))
            .arg("--workdir")
            .arg(&spec.worktree)
            .arg("--model")
            .arg(&spec.model)
            .arg("--endpoint")
            .arg(&spec.endpoint)
            .arg("--skip-model-check");
        cmd.kill_on_drop(true);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ReviewVerdict::reject(format!("reviewer spawn failed: {e}")),
        };

        let waited = match self.timeout {
            Some(dur) => match tokio::time::timeout(dur, child.wait()).await {
                Ok(res) => res,
                Err(_) => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return ReviewVerdict::reject(format!(
                        "reviewer timed out after {}s",
                        dur.as_secs()
                    ));
                }
            },
            None => child.wait().await,
        };
        if let Err(e) = waited {
            return ReviewVerdict::reject(format!("reviewer wait failed: {e}"));
        }

        // The reviewer left an HV3-3b result envelope in the worktree; its
        // summary begins with the VERDICT line.
        match crate::result::WorkerResult::read(&spec.worktree) {
            Some(r) => parse_verdict(&r.summary),
            None => {
                ReviewVerdict::reject("reviewer produced no result envelope to read a verdict from")
            }
        }
    }
}

/// A reviewer that approves everything — the default when no team reviewer is
/// configured is simply *not to call a reviewer at all*, but tests and the
/// no-reviewer lead path use this to mean "gate open".
pub struct AlwaysApprove;

#[async_trait]
impl Reviewer for AlwaysApprove {
    async fn review(&self, _spec: &ReviewSpec) -> ReviewVerdict {
        ReviewVerdict::approve("no review gate")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_verdict_approve() {
        let v = parse_verdict("VERDICT: APPROVE\nLooks correct and safe.");
        assert!(v.approved);
    }

    #[test]
    fn parse_verdict_reject_with_reason() {
        let v = parse_verdict("VERDICT: REJECT: missing error handling on the parse path\n…");
        assert!(!v.approved);
        assert_eq!(v.feedback, "missing error handling on the parse path");
    }

    #[test]
    fn parse_verdict_reject_bare() {
        assert!(!parse_verdict("VERDICT: REJECT").approved);
    }

    #[test]
    fn parse_verdict_missing_is_failsafe_reject() {
        let v = parse_verdict("I think it looks fine overall but I forgot the verdict line.");
        assert!(!v.approved, "no VERDICT line must fail safe to reject");
        assert!(v.feedback.contains("no parseable VERDICT"));
    }

    #[test]
    fn parse_verdict_tolerates_leading_prose_lines() {
        // The marker need not be line 1 of the parsed text (truncation aside),
        // so a leading blank/prose line before VERDICT still parses.
        let v = parse_verdict("\nVERDICT: APPROVE");
        assert!(v.approved);
    }

    #[tokio::test]
    async fn always_approve_approves() {
        let spec = ReviewSpec {
            task_id: "t".into(),
            task_title: "do x".into(),
            worktree: std::env::temp_dir(),
            reviewer_agent: "agent-yanluo".into(),
            model: "mock".into(),
            endpoint: "http://localhost".into(),
        };
        assert!(AlwaysApprove.review(&spec).await.approved);
    }

    // Exec-path: a stub binary that ignores args and exits 0 leaves no envelope,
    // so the verdict fail-safes to reject. Unix-only (needs `/usr/bin/true`); the
    // parser + routing logic above is platform-free.
    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_reviewer_no_envelope_failsafe_rejects() {
        let tmp = tempfile::TempDir::new().unwrap();
        let spec = ReviewSpec {
            task_id: "t".into(),
            task_title: "do x".into(),
            worktree: tmp.path().to_path_buf(),
            reviewer_agent: "agent-yanluo".into(),
            model: "mock".into(),
            endpoint: "http://localhost".into(),
        };
        let v = SubprocessReviewer::with_exe("/usr/bin/true")
            .review(&spec)
            .await;
        assert!(!v.approved);
    }
}
