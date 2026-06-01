//! Saṅgha worker spawning (HV2-1).
//!
//! A worker is **not** an in-process task — it is a spawned `bwoc-harness`
//! subprocess running in its own git worktree.  The OS sandbox is
//! process-scoped (landlock applies to a process tree; `sandbox-exec` wraps a
//! process), so an in-process worker would share the lead's sandbox profile and
//! address space — a compromised worker would leak into the lead.  A subprocess
//! worker gets its own worktree and re-applies the v1 safety pipeline from
//! scratch, so the guardrails→permission→sandbox invariant wraps it for free.
//!
//! This module owns the *spawn* seam ([`SpawnRunner`]) and the per-worker
//! worktree lifecycle (`git worktree add` / `remove`).  The lead loop in
//! [`crate::lead`] drives them; the queue in [`crate::queue`] schedules them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;

use crate::error::{HarnessError, HarnessResult};

/// Per-worker spawn parameters, built by the queue from a [`WorkItem`] plus the
/// lead's [`WorkerConfig`].
///
/// [`WorkItem`]: crate::queue::WorkItem
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// Task id (for diagnostics).
    pub task_id: String,
    /// The task prompt handed to the child via `--task` (the task title).
    pub prompt: String,
    /// The worktree the child runs in (`--workdir`).
    pub worktree: PathBuf,
    /// Model id (`--model`).
    pub model: String,
    /// Provider endpoint (`--endpoint`).
    pub endpoint: String,
    /// Skip the child's startup model-availability check (`--skip-model-check`).
    pub skip_model_check: bool,
    /// Per-run token budget forwarded as `--token-budget` (only if set).
    pub token_budget: Option<u64>,
    /// Per-run cost limit forwarded as `--cost-limit` (only if set).
    pub cost_limit: Option<f64>,
    /// Price per 1M tokens forwarded as `--cost-per-1m` (only if set).
    pub cost_per_1m: Option<f64>,
    /// Vetted-model handling forwarded as `--vetted-mode` (only if set).
    pub vetted_mode: Option<String>,
}

/// Lead-level configuration shared across spawned workers.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub model: String,
    pub endpoint: String,
    pub skip_model_check: bool,
    /// Per-run token budget propagated to workers as `--token-budget`.
    pub token_budget: Option<u64>,
    /// Per-run cost limit propagated to workers as `--cost-limit`.
    pub cost_limit: Option<f64>,
    /// Price per 1M tokens propagated to workers as `--cost-per-1m`.
    pub cost_per_1m: Option<f64>,
    /// Vetted-model handling propagated to workers as `--vetted-mode`.
    pub vetted_mode: Option<String>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            model: "gemma4".to_string(),
            endpoint: crate::provider::client::DEFAULT_ENDPOINT.to_string(),
            skip_model_check: false,
            token_budget: None,
            cost_limit: None,
            cost_per_1m: None,
            vetted_mode: None,
        }
    }
}

/// Runs a single worker for a [`WorkerSpec`].  Injectable so the queue and lead
/// loop can be tested without spawning real processes.
#[async_trait]
pub trait SpawnRunner: Send + Sync {
    /// Run the worker to completion.  `Ok(())` = the worker succeeded;
    /// `Err` = it failed (non-zero exit, spawn error).
    async fn run(&self, spec: &WorkerSpec) -> HarnessResult<()>;
}

/// No-op runner: succeeds without spawning anything.  The default for a queue
/// created with [`TaskQueue::new`](crate::queue::TaskQueue::new) — preserves the
/// scheduling-only behaviour used by the queue's own tests.
pub struct NoopRunner;

#[async_trait]
impl SpawnRunner for NoopRunner {
    async fn run(&self, _spec: &WorkerSpec) -> HarnessResult<()> {
        Ok(())
    }
}

/// Default per-worker wall-clock limit.  Bounded so one hung worker (stuck
/// network, runaway loop) can't block the (currently serial) lead drain
/// indefinitely; generous enough not to kill legitimate long tasks.
pub const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(1800);

/// Spawns a real `bwoc-harness` subprocess per worker.
pub struct SubprocessRunner {
    /// Path to the `bwoc-harness` binary.  Defaults to the current executable
    /// (the lead is itself a `bwoc-harness`); overridable for tests.
    exe: PathBuf,
    /// Max wall-clock time a worker may run before it's killed and reaped.
    /// `None` = no limit (tests pointing at instant stubs).
    timeout: Option<Duration>,
}

impl SubprocessRunner {
    /// Spawn copies of the currently-running executable, bounded by
    /// [`DEFAULT_WORKER_TIMEOUT`].
    pub fn new() -> HarnessResult<Self> {
        let exe = std::env::current_exe()
            .map_err(|e| HarnessError::Other(format!("cannot resolve current exe: {e}")))?;
        Ok(Self {
            exe,
            timeout: Some(DEFAULT_WORKER_TIMEOUT),
        })
    }

    /// Spawn a specific binary (tests point this at a stub like `/usr/bin/true`).
    /// No timeout by default — set one with [`with_timeout`](Self::with_timeout).
    pub fn with_exe(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            timeout: None,
        }
    }

    /// Override the per-worker timeout (`None` disables it).
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Build the `bwoc-harness` worker argv for `spec`.
///
/// Pure (no process spawn) so the flag-propagation logic is unit-testable:
/// every scalar lead flag is forwarded only when set, leaving an unset flag at
/// the worker's own default rather than pinning it to a sentinel.
fn build_worker_argv(spec: &WorkerSpec) -> Vec<OsString> {
    // OsString end-to-end so the worktree path is passed verbatim (a lossy
    // `to_string_lossy()` could corrupt a non-UTF8 path and point the worker at
    // the wrong directory). Flag names + scalar values are ASCII.
    let mut argv: Vec<OsString> = vec![
        "--task".into(),
        spec.prompt.clone().into(),
        "--workdir".into(),
        spec.worktree.clone().into_os_string(),
        "--model".into(),
        spec.model.clone().into(),
        "--endpoint".into(),
        spec.endpoint.clone().into(),
    ];
    if spec.skip_model_check {
        argv.push("--skip-model-check".into());
    }
    if let Some(budget) = spec.token_budget {
        argv.push("--token-budget".into());
        argv.push(budget.to_string().into());
    }
    if let Some(limit) = spec.cost_limit {
        argv.push("--cost-limit".into());
        argv.push(limit.to_string().into());
    }
    if let Some(rate) = spec.cost_per_1m {
        argv.push("--cost-per-1m".into());
        argv.push(rate.to_string().into());
    }
    if let Some(mode) = &spec.vetted_mode {
        argv.push("--vetted-mode".into());
        argv.push(mode.clone().into());
    }
    argv
}

#[async_trait]
impl SpawnRunner for SubprocessRunner {
    async fn run(&self, spec: &WorkerSpec) -> HarnessResult<()> {
        let mut cmd = tokio::process::Command::new(&self.exe);
        cmd.args(build_worker_argv(spec));
        // Reap the child if this future is dropped (e.g. queue cancellation)
        // so a cancelled or abandoned worker never becomes an orphan.
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            HarnessError::Other(format!("spawn failed for `{}`: {e}", spec.task_id))
        })?;

        let wait_err = |e: std::io::Error| {
            HarnessError::Other(format!("wait failed for `{}`: {e}", spec.task_id))
        };
        let status = match self.timeout {
            Some(dur) => match tokio::time::timeout(dur, child.wait()).await {
                Ok(res) => res.map_err(wait_err)?,
                Err(_) => {
                    // Timed out — kill and reap so the child doesn't leak.
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    return Err(HarnessError::Other(format!(
                        "worker for task `{}` timed out after {}s",
                        spec.task_id,
                        dur.as_secs()
                    )));
                }
            },
            None => child.wait().await.map_err(wait_err)?,
        };

        if status.success() {
            Ok(())
        } else {
            Err(HarnessError::Other(format!(
                "worker for task `{}` exited with {status}",
                spec.task_id
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Per-worker worktree lifecycle
// ---------------------------------------------------------------------------

/// Create an isolated git worktree at `worktree` off `repo_root`'s HEAD.
///
/// Uses `--detach` so concurrent workers never collide on a branch name; the
/// worker commits onto the detached HEAD and the lead collects the result. The
/// parent directory is created if missing.
pub fn git_worktree_add(repo_root: &Path, worktree: &Path) -> HarnessResult<()> {
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Self-heal: a worktree left behind by a prior failed run (kept for
    // inspection) would make `add` fail with "already exists" and strand the
    // re-claimed task forever.  Prune stale registrations and force-remove any
    // leftover at this path so a retry starts clean.  Both are best-effort.
    let _ = run_git(repo_root, &["worktree", "prune"]);
    if worktree.exists() {
        let _ = git_worktree_remove(repo_root, worktree);
    }
    run_git(
        repo_root,
        &["worktree", "add", "--detach", &worktree.to_string_lossy()],
    )
}

/// Remove a worktree created by [`git_worktree_add`].  `--force` so a worktree
/// with uncommitted changes is still cleaned up.
pub fn git_worktree_remove(repo_root: &Path, worktree: &Path) -> HarnessResult<()> {
    run_git(
        repo_root,
        &["worktree", "remove", "--force", &worktree.to_string_lossy()],
    )
}

/// Run `git -C <repo_root> <args…>`, mapping a non-zero exit to an error.
fn run_git(repo_root: &Path, args: &[&str]) -> HarnessResult<()> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(HarnessError::Other(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn spec(worktree: &Path) -> WorkerSpec {
        WorkerSpec {
            task_id: "t1".to_string(),
            prompt: "do the thing".to_string(),
            worktree: worktree.to_path_buf(),
            model: "mock".to_string(),
            endpoint: crate::provider::client::DEFAULT_ENDPOINT.to_string(),
            skip_model_check: true,
            token_budget: None,
            cost_limit: None,
            cost_per_1m: None,
            vetted_mode: None,
        }
    }

    #[tokio::test]
    async fn noop_runner_always_succeeds() {
        let tmp = TempDir::new().unwrap();
        assert!(NoopRunner.run(&spec(tmp.path())).await.is_ok());
    }

    // These two pin the exit-code → Ok/Err mapping using the `/usr/bin/true`
    // and `/usr/bin/false` stubs; unix-only because there is no portable
    // Windows binary that ignores the bwoc-style args and exits 0/1. The
    // mapping logic itself is platform-agnostic.
    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_runner_maps_exit_zero_to_ok() {
        // `/usr/bin/true` ignores args and exits 0 — a trivial child process.
        let tmp = TempDir::new().unwrap();
        let runner = SubprocessRunner::with_exe("/usr/bin/true");
        assert!(runner.run(&spec(tmp.path())).await.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_runner_maps_nonzero_exit_to_err() {
        let tmp = TempDir::new().unwrap();
        let runner = SubprocessRunner::with_exe("/usr/bin/false");
        let err = runner.run(&spec(tmp.path())).await.unwrap_err();
        assert!(format!("{err}").contains("exited with"));
    }

    #[test]
    fn argv_omits_scalar_flags_when_unset() {
        let tmp = TempDir::new().unwrap();
        let argv = build_worker_argv(&spec(tmp.path()));
        for flag in [
            "--token-budget",
            "--cost-limit",
            "--cost-per-1m",
            "--vetted-mode",
        ] {
            assert!(
                !argv.iter().any(|a| a.to_str() == Some(flag)),
                "unset {flag} must not appear in argv: {argv:?}"
            );
        }
    }

    #[test]
    fn argv_forwards_scalar_flags_when_set() {
        let tmp = TempDir::new().unwrap();
        let mut s = spec(tmp.path());
        s.token_budget = Some(50_000);
        s.cost_limit = Some(1.5);
        s.cost_per_1m = Some(3.0);
        s.vetted_mode = Some("enforce".to_string());
        let argv = build_worker_argv(&s);

        let pair = |flag: &str| {
            let i = argv.iter().position(|a| a.to_str() == Some(flag));
            i.and_then(|i| argv.get(i + 1))
                .and_then(|a| a.to_str())
                .map(str::to_string)
        };
        assert_eq!(pair("--token-budget").as_deref(), Some("50000"));
        assert_eq!(pair("--cost-limit").as_deref(), Some("1.5"));
        assert_eq!(pair("--cost-per-1m").as_deref(), Some("3"));
        assert_eq!(pair("--vetted-mode").as_deref(), Some("enforce"));
    }

    #[tokio::test]
    async fn subprocess_runner_spawn_failure_is_err() {
        let tmp = TempDir::new().unwrap();
        let runner = SubprocessRunner::with_exe("/no/such/binary-xyz");
        assert!(runner.run(&spec(tmp.path())).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_runner_kills_on_timeout() {
        use std::os::unix::fs::PermissionsExt;
        // A child that ignores its args and sleeps far past the timeout.
        let tmp = TempDir::new().unwrap();
        let script = tmp.path().join("sleeper.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let runner =
            SubprocessRunner::with_exe(&script).with_timeout(Some(Duration::from_millis(150)));
        let start = std::time::Instant::now();
        let err = runner.run(&spec(tmp.path())).await.unwrap_err();

        assert!(format!("{err}").contains("timed out"), "got: {err}");
        // Returned promptly — did not block for the full 30s sleep.
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn git_worktree_add_then_remove() {
        // Build a throwaway repo with one commit so HEAD exists.
        let repo = TempDir::new().unwrap();
        let r = repo.path();
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(r)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "git {args:?} failed"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t.t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(r.join("f.txt"), "hi").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        let wt = repo.path().join("worktrees").join("t1");
        git_worktree_add(r, &wt).unwrap();
        assert!(wt.join("f.txt").exists(), "worktree checked out HEAD");

        git_worktree_remove(r, &wt).unwrap();
        assert!(!wt.exists(), "worktree dir removed");
    }
}
