//! Saṅgha lead loop (HV2-1).
//!
//! The lead drains claimable tasks from a [`TaskSource`], gives each its own
//! git worktree, and spawns a `bwoc-harness` subprocess worker (via the
//! injected [`SpawnRunner`]) to do the work.  On success the task is completed
//! and its worktree removed; on failure the claim is rolled back and the
//! worktree is left in place for inspection (a later re-claim self-heals it).
//!
//! ## Coordination, not control
//!
//! The lead never runs task code in-process — it spawns, waits, and records.
//! Each worker re-applies the full guardrails→permission→sandbox pipeline as a
//! fresh process, so the lead's authority does not extend into the worker.
//!
//! Collection is **sequential** in this first build (submit one worker, await
//! it, then the next): correct and easy to reason about.  Concurrent
//! collection up to the queue's capacity is a deferred follow-up — the queue
//! and per-worktree guard already support it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use bwoc_core::team::{self, Task, TaskState};

use crate::error::{HarnessError, HarnessResult};
use crate::queue::{TaskQueue, TaskSource, WorkItem};
use crate::worker::{SpawnRunner, WorkerConfig, git_worktree_add, git_worktree_remove};

// ---------------------------------------------------------------------------
// File-backed task source
// ---------------------------------------------------------------------------

/// A [`TaskSource`] backed by a `tasks.jsonl` file (the Saṅgha shared list).
///
/// Each operation reads the file, mutates the parsed list, and writes it back.
/// An internal mutex serialises in-process access; cross-process coordination
/// is the CLI's concern (the lead is the single writer in this loop).
pub struct JsonlTaskSource {
    path: PathBuf,
    lock: Mutex<()>,
}

impl JsonlTaskSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<Vec<Task>, HarnessError> {
        let raw = std::fs::read_to_string(&self.path)?;
        team::parse_tasks(&raw).map_err(|e| HarnessError::Other(format!("parse tasks: {e}")))
    }

    fn write(&self, tasks: &[Task]) -> Result<(), HarnessError> {
        let rendered = team::render_tasks(tasks)
            .map_err(|e| HarnessError::Other(format!("render tasks: {e}")))?;
        std::fs::write(&self.path, rendered)?;
        Ok(())
    }
}

impl TaskSource for JsonlTaskSource {
    fn list_tasks(&self) -> Vec<Task> {
        let _g = self.lock.lock().unwrap();
        self.read().unwrap_or_default()
    }

    fn claim(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError> {
        let _g = self.lock.lock().unwrap();
        let mut tasks = self.read()?;
        team::claim_task(&mut tasks, task_id, agent_id)
            .map_err(|e| HarnessError::Other(format!("claim `{task_id}`: {e}")))?;
        self.write(&tasks)
    }

    fn complete(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError> {
        let _g = self.lock.lock().unwrap();
        let mut tasks = self.read()?;
        team::complete_task(&mut tasks, task_id, agent_id)
            .map_err(|e| HarnessError::Other(format!("complete `{task_id}`: {e}")))?;
        self.write(&tasks)
    }

    fn unclaim(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError> {
        let _g = self.lock.lock().unwrap();
        let mut tasks = self.read()?;
        let task = tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| HarnessError::Other(format!("unclaim: task `{task_id}` not found")))?;
        if task.claimed_by.as_deref() != Some(agent_id) {
            return Err(HarnessError::Other(format!(
                "unclaim: task `{task_id}` is not claimed by `{agent_id}`"
            )));
        }
        task.state = TaskState::Pending;
        task.claimed_by = None;
        self.write(&tasks)
    }
}

// ---------------------------------------------------------------------------
// Lead loop
// ---------------------------------------------------------------------------

/// Configuration for one [`run_lead`] invocation.
#[derive(Debug, Clone)]
pub struct LeadConfig {
    /// Agent id the lead claims tasks as.
    pub agent_id: String,
    /// Git repository the per-task worktrees branch off.
    pub repo_root: PathBuf,
    /// Directory under which per-task worktrees are created (`<base>/<task-id>`).
    pub worktree_base: PathBuf,
    /// Worker spawn config (model, endpoint) passed to each child.
    pub worker: WorkerConfig,
    /// Queue concurrency capacity.
    pub capacity: usize,
    /// Maximum tasks to process this invocation; `0` = no cap (drain).
    pub max_tasks: usize,
}

/// Outcome counts from a [`run_lead`] invocation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LeadSummary {
    pub claimed: usize,
    pub completed: usize,
    pub failed: usize,
}

/// Drain claimable tasks from `source`, spawning a worker per task via `runner`.
///
/// Returns the outcome counts.  Best-effort and resilient: a worktree-creation
/// or spawn failure for one task rolls back that claim and moves on rather than
/// aborting the whole loop.
pub async fn run_lead(
    source: &dyn TaskSource,
    runner: Arc<dyn SpawnRunner>,
    cfg: &LeadConfig,
) -> HarnessResult<LeadSummary> {
    let cancel = CancellationToken::new();
    let queue = TaskQueue::with_runner(
        cfg.capacity.max(1),
        cancel.clone(),
        runner,
        Arc::new(cfg.worker.clone()),
    );

    let mut summary = LeadSummary::default();

    for task in source.list_tasks() {
        if cfg.max_tasks != 0 && summary.claimed >= cfg.max_tasks {
            break;
        }
        if task.state != TaskState::Pending {
            continue;
        }
        // Claim — skips blocked/already-claimed tasks.
        if source.claim(&task.id, &cfg.agent_id).is_err() {
            continue;
        }
        summary.claimed += 1;

        let worktree = cfg.worktree_base.join(&task.id);
        if let Err(e) = git_worktree_add(&cfg.repo_root, &worktree) {
            eprintln!(
                "[bwoc-harness] lead: worktree add failed for `{}`: {e}",
                task.id
            );
            let _ = source.unclaim(&task.id, &cfg.agent_id);
            summary.failed += 1;
            continue;
        }

        let (tx, rx) = oneshot::channel();
        let item = WorkItem {
            task: task.clone(),
            worktree_path: worktree.clone(),
            result_tx: tx,
        };
        if let Err(e) = queue.submit(item).await {
            eprintln!("[bwoc-harness] lead: submit failed for `{}`: {e}", task.id);
            let _ = git_worktree_remove(&cfg.repo_root, &worktree);
            let _ = source.unclaim(&task.id, &cfg.agent_id);
            summary.failed += 1;
            continue;
        }

        match rx.await {
            Ok(Ok(())) => {
                if let Err(e) = source.complete(&task.id, &cfg.agent_id) {
                    eprintln!(
                        "[bwoc-harness] lead: complete failed for `{}`: {e}",
                        task.id
                    );
                }
                // Worker succeeded — tear down its worktree (Anattā).
                let _ = git_worktree_remove(&cfg.repo_root, &worktree);
                summary.completed += 1;
            }
            Ok(Err(e)) => {
                eprintln!("[bwoc-harness] lead: worker for `{}` failed: {e}", task.id);
                let _ = source.unclaim(&task.id, &cfg.agent_id);
                // Leave the worktree in place for post-mortem inspection; a
                // later re-claim self-heals it (see `git_worktree_add`).
                summary.failed += 1;
            }
            Err(_) => {
                // Worker channel dropped (queue cancelled / worker panicked).
                let _ = source.unclaim(&task.id, &cfg.agent_id);
                summary.failed += 1;
            }
        }
    }

    queue.cancel();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::InMemoryTaskSource;
    use crate::worker::WorkerSpec;
    use async_trait::async_trait;
    use std::process::Command;
    use tempfile::TempDir;

    /// Mock runner returning a per-task-id verdict (no real subprocess).
    struct ScriptedRunner {
        fail_ids: Vec<String>,
    }
    #[async_trait]
    impl SpawnRunner for ScriptedRunner {
        async fn run(&self, spec: &WorkerSpec) -> HarnessResult<()> {
            if self.fail_ids.contains(&spec.task_id) {
                Err(HarnessError::Other("scripted failure".to_string()))
            } else {
                Ok(())
            }
        }
    }

    /// A throwaway git repo with one commit so worktrees can branch off HEAD.
    fn temp_repo() -> TempDir {
        let repo = TempDir::new().unwrap();
        let r = repo.path();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(r)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t.t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(r.join("seed.txt"), "x").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "seed"]);
        repo
    }

    fn lead_cfg(repo: &TempDir) -> LeadConfig {
        LeadConfig {
            agent_id: "agent-lead".to_string(),
            repo_root: repo.path().to_path_buf(),
            worktree_base: repo.path().join(".worktrees"),
            worker: WorkerConfig::default(),
            capacity: 2,
            max_tasks: 0,
        }
    }

    fn pending(id: &str) -> Task {
        Task::new(id, format!("task {id}"), vec![])
    }

    #[tokio::test]
    async fn lead_completes_successful_tasks_and_cleans_worktrees() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a"), pending("b")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });

        let summary = run_lead(&source, runner, &lead_cfg(&repo)).await.unwrap();

        assert_eq!(
            summary,
            LeadSummary {
                claimed: 2,
                completed: 2,
                failed: 0
            }
        );
        // Both tasks marked completed.
        let states: Vec<_> = source.list_tasks().into_iter().map(|t| t.state).collect();
        assert!(states.iter().all(|s| *s == TaskState::Completed));
        // Worktrees torn down on success.
        assert!(!repo.path().join(".worktrees").join("a").exists());
        assert!(!repo.path().join(".worktrees").join("b").exists());
    }

    #[tokio::test]
    async fn lead_unclaims_failed_task_and_keeps_worktree() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("ok"), pending("bad")]);
        let runner = Arc::new(ScriptedRunner {
            fail_ids: vec!["bad".to_string()],
        });

        let summary = run_lead(&source, runner, &lead_cfg(&repo)).await.unwrap();

        assert_eq!(summary.completed, 1);
        assert_eq!(summary.failed, 1);
        // Failed task rolled back to Pending (re-claimable); succeeded one done.
        let by_id = |id: &str| {
            source
                .list_tasks()
                .into_iter()
                .find(|t| t.id == id)
                .unwrap()
                .state
        };
        assert_eq!(by_id("ok"), TaskState::Completed);
        assert_eq!(by_id("bad"), TaskState::Pending);
        // Failed worktree kept for inspection.
        assert!(repo.path().join(".worktrees").join("bad").exists());
    }

    #[tokio::test]
    async fn lead_respects_max_tasks_cap() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a"), pending("b"), pending("c")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });
        let mut cfg = lead_cfg(&repo);
        cfg.max_tasks = 1;

        let summary = run_lead(&source, runner, &cfg).await.unwrap();
        assert_eq!(summary.claimed, 1);
        assert_eq!(summary.completed, 1);
    }

    #[test]
    fn jsonl_task_source_roundtrips_claim_complete() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tasks.jsonl");
        std::fs::write(&path, team::render_tasks(&[pending("t1")]).unwrap()).unwrap();

        let src = JsonlTaskSource::new(&path);
        assert_eq!(src.list_tasks().len(), 1);
        src.claim("t1", "agent-lead").unwrap();
        assert_eq!(src.list_tasks()[0].state, TaskState::InProgress);
        src.complete("t1", "agent-lead").unwrap();
        assert_eq!(src.list_tasks()[0].state, TaskState::Completed);
    }

    #[test]
    fn jsonl_task_source_unclaim_reverts_to_pending() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tasks.jsonl");
        std::fs::write(&path, team::render_tasks(&[pending("t1")]).unwrap()).unwrap();

        let src = JsonlTaskSource::new(&path);
        src.claim("t1", "agent-lead").unwrap();
        src.unclaim("t1", "agent-lead").unwrap();
        assert_eq!(src.list_tasks()[0].state, TaskState::Pending);
        assert!(
            src.unclaim("t1", "agent-lead").is_err(),
            "not claimed anymore"
        );
    }
}
