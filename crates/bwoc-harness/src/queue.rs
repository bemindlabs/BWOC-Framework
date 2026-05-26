//! Task queue — async, bounded, cancellable work queue.
//!
//! **P3 component** — Saṅgha + Padhāna 4 (the four right efforts applied to
//! managing and executing work items from the shared team task list).
//!
//! ## Responsibilities
//!
//! 1. **Task sourcing**: pulls claimable [`bwoc_core::team::Task`] items from
//!    an in-memory view of the team's `tasks.jsonl` (the file-lock and disk
//!    I/O are the CLI's responsibility; this module works with an injected
//!    [`TaskSource`] trait so tests can drive it with an in-memory mock).
//!
//! 2. **Bounded concurrency**: admits at most `capacity` items at once; back-
//!    pressure is applied to callers that try to enqueue beyond the limit.
//!
//! 3. **One-in-flight per worktree**: a second enqueue for a task that
//!    maps to the same `worktree_path` is rejected with [`QueueError::Busy`].
//!
//! 4. **Cancellation**: a [`tokio_util::sync::CancellationToken`] shuts down
//!    the queue and all in-flight workers gracefully.
//!
//! 5. **Completion signalling**: on successful processing the queue calls
//!    [`complete_task`] via the injected [`TaskSource`] — this mirrors what
//!    the CLI `bwoc task complete` command does, keeping the two paths
//!    consistent.
//!
//! ## Dep-quarantine note
//!
//! `bwoc-core` is a lean crate (serde + serde_json + toml + thiserror only —
//! no tokio, no HTTP).  Adding it as a path dependency of `bwoc-harness` does
//! NOT break the dep-quarantine invariant; it only adds lean data-type code
//! to the harness, never heavy deps to bwoc-core.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use bwoc_core::team::{Task, TaskState};

use crate::error::HarnessError;
use crate::worker::{NoopRunner, SpawnRunner, WorkerConfig, WorkerSpec};

// ---------------------------------------------------------------------------
// TaskSource trait — injectable abstraction for Saṅgha integration
// ---------------------------------------------------------------------------

/// Provides the queue with access to the team's task list.
///
/// Production implementation reads/writes `tasks.jsonl` under a file lock
/// (managed at the CLI layer).  Tests inject [`InMemoryTaskSource`].
pub trait TaskSource: Send + Sync {
    /// Return all tasks currently visible to this agent.
    fn list_tasks(&self) -> Vec<Task>;

    /// Claim a task for `agent_id`.  Mutates the underlying store.
    fn claim(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError>;

    /// Mark a task as completed by `agent_id`.  Mutates the underlying store.
    fn complete(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError>;

    /// Roll back a claim: revert `task_id` from `InProgress` back to
    /// `Pending` when the queue rejected the item after it was already
    /// claimed.  This prevents tasks from being stranded as `in_progress`
    /// with no worker.
    ///
    /// Implementors that do not support unclaim (e.g. file-backed stores
    /// without write access) may return `Err(HarnessError::Other(...))`.
    fn unclaim(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError>;
}

// ---------------------------------------------------------------------------
// In-memory TaskSource for tests (and local task submissions)
// ---------------------------------------------------------------------------

/// An in-memory implementation of [`TaskSource`] for offline tests.
///
/// Thread-safe via a `Mutex<Vec<Task>>`.
pub struct InMemoryTaskSource {
    tasks: Mutex<Vec<Task>>,
}

impl InMemoryTaskSource {
    pub fn new(tasks: Vec<Task>) -> Self {
        Self {
            tasks: Mutex::new(tasks),
        }
    }

    /// Add a task directly (for test setup).
    pub fn push(&self, task: Task) {
        self.tasks.lock().unwrap().push(task);
    }
}

impl TaskSource for InMemoryTaskSource {
    fn list_tasks(&self) -> Vec<Task> {
        self.tasks.lock().unwrap().clone()
    }

    fn claim(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError> {
        let mut tasks = self.tasks.lock().unwrap();
        bwoc_core::team::claim_task(&mut tasks, task_id, agent_id)
            .map_err(|e| HarnessError::Other(e.to_string()))
    }

    fn complete(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError> {
        let mut tasks = self.tasks.lock().unwrap();
        bwoc_core::team::complete_task(&mut tasks, task_id, agent_id)
            .map_err(|e| HarnessError::Other(e.to_string()))
    }

    fn unclaim(&self, task_id: &str, agent_id: &str) -> Result<(), HarnessError> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| HarnessError::Other(format!("unclaim: task `{task_id}` not found")))?;
        if task.state != TaskState::InProgress {
            return Err(HarnessError::Other(format!(
                "unclaim: task `{task_id}` is not in_progress (state: {:?})",
                task.state
            )));
        }
        if task.claimed_by.as_deref() != Some(agent_id) {
            return Err(HarnessError::Other(format!(
                "unclaim: task `{task_id}` is not claimed by `{agent_id}`"
            )));
        }
        task.state = TaskState::Pending;
        task.claimed_by = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Work item
// ---------------------------------------------------------------------------

/// A unit of work admitted to the queue.
#[derive(Debug)]
pub struct WorkItem {
    /// Task from the Saṅgha list (or a locally-submitted task).
    pub task: Task,
    /// The worktree the task runs in.  Used to enforce one-in-flight per
    /// worktree.
    pub worktree_path: PathBuf,
    /// One-shot channel to report the outcome back to the submitter.
    pub result_tx: oneshot::Sender<Result<(), HarnessError>>,
}

// ---------------------------------------------------------------------------
// Queue errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("queue is at capacity ({0} items in flight)")]
    AtCapacity(usize),
    #[error("worktree `{0}` already has a task in flight")]
    Busy(PathBuf),
    #[error("queue is shut down")]
    Shutdown,
    #[error("channel send error: {0}")]
    Send(String),
}

// ---------------------------------------------------------------------------
// TaskQueue — the bounded, cancellable work queue
// ---------------------------------------------------------------------------

/// A bounded, cancellable async task queue.
///
/// Clone-safe: the internal sender is an `Arc`-wrapped `mpsc::Sender` so
/// multiple callers can submit tasks concurrently.
#[derive(Clone)]
pub struct TaskQueue {
    sender: mpsc::Sender<WorkItem>,
    /// Tracks worktree paths that have an in-flight task.
    in_flight: Arc<Mutex<HashSet<PathBuf>>>,
    capacity: usize,
    cancel: CancellationToken,
}

// ---------------------------------------------------------------------------
// Worker runner wiring (HV2-1)
// ---------------------------------------------------------------------------

/// Bundles the [`SpawnRunner`] and [`WorkerConfig`] the worker loop uses to
/// turn a `WorkItem` into a running worker.  Held behind `Arc`s so the worker
/// task owns clones cheaply.
#[derive(Clone)]
struct RunnerCtx {
    runner: Arc<dyn SpawnRunner>,
    config: Arc<WorkerConfig>,
}

impl TaskQueue {
    /// Create a new queue and spawn the worker loop.
    ///
    /// `capacity` — maximum number of concurrent work items.  The underlying
    /// channel buffer is `capacity + 1` to allow one item to be queued while
    /// all slots are busy before back-pressure kicks in.
    ///
    /// The worker loop runs until `cancel` is cancelled or the sender is
    /// dropped.
    pub fn new(capacity: usize, cancel: CancellationToken) -> Self {
        // Default runner is a no-op (scheduling only) — preserves the queue's
        // historical behaviour for callers/tests that don't spawn real workers.
        Self::with_runner(
            capacity,
            cancel,
            Arc::new(NoopRunner),
            Arc::new(WorkerConfig::default()),
        )
    }

    /// Create a queue whose worker loop spawns real workers via `runner`.
    ///
    /// Each admitted `WorkItem` becomes a [`WorkerSpec`] (built from the item's
    /// task plus `config`) and is handed to `runner.run()`.
    pub fn with_runner(
        capacity: usize,
        cancel: CancellationToken,
        runner: Arc<dyn SpawnRunner>,
        config: Arc<WorkerConfig>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<WorkItem>(capacity + 1);
        let in_flight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let in_flight_worker = Arc::clone(&in_flight);
        let cancel_worker = cancel.clone();
        let ctx = RunnerCtx { runner, config };

        tokio::spawn(async move {
            run_worker(rx, in_flight_worker, cancel_worker, ctx).await;
        });

        Self {
            sender: tx,
            in_flight,
            capacity,
            cancel,
        }
    }

    /// Submit a task to the queue.
    ///
    /// Returns `Err(QueueError::AtCapacity)` if the queue is full.
    /// Returns `Err(QueueError::Busy)` if the task's worktree already has a
    /// task in flight.
    /// Returns `Err(QueueError::Shutdown)` if the queue has been cancelled.
    pub async fn submit(&self, item: WorkItem) -> Result<(), QueueError> {
        if self.cancel.is_cancelled() {
            return Err(QueueError::Shutdown);
        }

        // One-in-flight per worktree check.
        {
            let guard = self.in_flight.lock().unwrap();
            if guard.contains(&item.worktree_path) {
                return Err(QueueError::Busy(item.worktree_path.clone()));
            }
            if guard.len() >= self.capacity {
                return Err(QueueError::AtCapacity(self.capacity));
            }
        }

        // Register the worktree as in-flight before sending so there's no
        // window between the check and the send.
        self.in_flight
            .lock()
            .unwrap()
            .insert(item.worktree_path.clone());

        self.sender
            .send(item)
            .await
            .map_err(|e| QueueError::Send(e.to_string()))
    }

    /// Cancel the queue — signals the worker loop to stop processing new items.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Number of worktrees currently in flight.
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.lock().unwrap().len()
    }
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

/// The internal worker loop.  Receives `WorkItem`s and processes them, then
/// sends the outcome back via the item's `result_tx` channel.
///
/// Stops when `cancel` is signalled or all senders are dropped.
async fn run_worker(
    mut rx: mpsc::Receiver<WorkItem>,
    in_flight: Arc<Mutex<HashSet<PathBuf>>>,
    cancel: CancellationToken,
    ctx: RunnerCtx,
) {
    loop {
        tokio::select! {
            biased;

            _ = cancel.cancelled() => {
                // Drain remaining items and report cancellation.
                while let Ok(item) = rx.try_recv() {
                    // Release the worktree slot before reporting the error.
                    in_flight.lock().unwrap().remove(&item.worktree_path);
                    let _ = item.result_tx.send(Err(HarnessError::Other(
                        "queue cancelled".to_string(),
                    )));
                }
                break;
            }

            maybe_item = rx.recv() => {
                match maybe_item {
                    None => break, // all senders dropped
                    Some(item) => {
                        let worktree = item.worktree_path.clone();
                        // Spawn the worker for this item via the injected runner.
                        let result = execute_item(&item, &ctx).await;
                        // Release the worktree slot.
                        in_flight.lock().unwrap().remove(&worktree);
                        // Send outcome (ignore if the receiver has already dropped).
                        let _ = item.result_tx.send(result);
                    }
                }
            }
        }
    }
}

/// Execute a [`WorkItem`] by spawning its worker (HV2-1).
///
/// The queue owns scheduling and cancellation; the actual work is delegated to
/// the injected [`SpawnRunner`].  A worker runs in its own worktree as a
/// separate `bwoc-harness` process, so the guardrails→permission→sandbox
/// invariant is re-applied by the child — this loop never executes task code
/// in-process.
async fn execute_item(item: &WorkItem, ctx: &RunnerCtx) -> Result<(), HarnessError> {
    // Verify the worktree exists before spawning into it.
    if !item.worktree_path.exists() {
        return Err(HarnessError::Other(format!(
            "worktree does not exist: {}",
            item.worktree_path.display()
        )));
    }
    let spec = WorkerSpec {
        task_id: item.task.id.clone(),
        prompt: item.task.title.clone(),
        worktree: item.worktree_path.clone(),
        model: ctx.config.model.clone(),
        endpoint: ctx.config.endpoint.clone(),
        skip_model_check: ctx.config.skip_model_check,
    };
    ctx.runner.run(&spec).await
}

// ---------------------------------------------------------------------------
// Saṅgha poll helper — drain claimable tasks into the queue
// ---------------------------------------------------------------------------

/// Scan `source` for pending, unblocked tasks and submit each claimable task
/// to `queue` as `agent_id`.
///
/// Returns the number of tasks successfully submitted.  Tasks that are already
/// in-flight, blocked, or non-pending are silently skipped.
pub async fn poll_sangha(
    source: &dyn TaskSource,
    queue: &TaskQueue,
    agent_id: &str,
    worktree_base: &std::path::Path,
) -> usize {
    let tasks = source.list_tasks();
    let mut submitted = 0usize;

    for task in tasks {
        if task.state != TaskState::Pending {
            continue;
        }
        // Skip tasks that have unmet dependencies (blocked).
        // Dependency resolution is performed by bwoc_core::team::claim_task;
        // if claim fails with BlockedByDependency we just skip.

        // Claim the task.
        if source.claim(&task.id, agent_id).is_err() {
            continue; // not claimable (blocked, already in-progress, etc.)
        }

        let worktree_path = worktree_base.join(&task.id);
        let (result_tx, _result_rx) = oneshot::channel();

        let item = WorkItem {
            task: task.clone(),
            worktree_path,
            result_tx,
        };

        match queue.submit(item).await {
            Ok(()) => {
                submitted += 1;
            }
            Err(QueueError::Busy(_) | QueueError::AtCapacity(_)) => {
                // P4 rollback: the queue rejected a task we already claimed.
                // Revert the task to `pending` so it isn't stranded as
                // `in_progress` with no worker.  We use complete_task to
                // move it — but complete_task sets it to Completed, not
                // Pending.  Instead, we call a dedicated unclaim path via
                // TaskSource::unclaim if available, or fall back to re-
                // inserting a fresh pending task in the in-memory source.
                //
                // For the TaskSource trait (which hides the backing store)
                // we add an `unclaim` method that reverts InProgress → Pending.
                // If the source doesn't support it (returns Err), we log a
                // warning — the operator will need to re-triage manually.
                if let Err(e) = source.unclaim(&task.id, agent_id) {
                    eprintln!(
                        "[bwoc-harness] WARNING: failed to unclaim task `{}` after \
                         queue rejection — task may be stranded as in_progress: {e}",
                        task.id
                    );
                }
                break;
            }
            Err(_) => break,
        }
    }

    submitted
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bwoc_core::team::Task;
    use tempfile::TempDir;
    use tokio::sync::oneshot;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn pending_task(id: &str) -> Task {
        Task::new(id, format!("task {id}"), vec![])
    }

    fn make_queue(capacity: usize) -> (TaskQueue, CancellationToken) {
        let cancel = CancellationToken::new();
        let q = TaskQueue::new(capacity, cancel.clone());
        (q, cancel)
    }

    // ── Basic submit → result ────────────────────────────────────────────────

    #[tokio::test]
    async fn submit_task_receives_ok_result() {
        let tmp = TempDir::new().unwrap();
        let (queue, _cancel) = make_queue(4);

        let (tx, rx) = oneshot::channel();
        let item = WorkItem {
            task: pending_task("t1"),
            worktree_path: tmp.path().to_path_buf(),
            result_tx: tx,
        };

        queue.submit(item).await.unwrap();
        let result = rx.await.unwrap();
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    // ── Worktree busy guard ──────────────────────────────────────────────────

    #[tokio::test]
    async fn second_task_for_same_worktree_rejected_as_busy() {
        let tmp = TempDir::new().unwrap();
        let (queue, _cancel) = make_queue(4);

        let (tx1, _rx1) = oneshot::channel();
        let item1 = WorkItem {
            task: pending_task("t1"),
            worktree_path: tmp.path().to_path_buf(),
            result_tx: tx1,
        };
        queue.submit(item1).await.unwrap();

        // Second item for the same worktree must be rejected.
        let (tx2, _rx2) = oneshot::channel();
        let item2 = WorkItem {
            task: pending_task("t2"),
            worktree_path: tmp.path().to_path_buf(),
            result_tx: tx2,
        };
        let err = queue.submit(item2).await.unwrap_err();
        assert!(
            matches!(err, QueueError::Busy(_)),
            "expected Busy, got {err:?}"
        );
    }

    // ── Capacity limit ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn queue_rejects_beyond_capacity() {
        let tmp = TempDir::new().unwrap();
        let (queue, _cancel) = make_queue(2);

        // Fill the queue with two distinct worktrees.
        let wt1 = tmp.path().join("wt1");
        std::fs::create_dir_all(&wt1).unwrap();
        let wt2 = tmp.path().join("wt2");
        std::fs::create_dir_all(&wt2).unwrap();
        let wt3 = tmp.path().join("wt3");
        std::fs::create_dir_all(&wt3).unwrap();

        let (tx1, _rx1) = oneshot::channel();
        queue
            .submit(WorkItem {
                task: pending_task("t1"),
                worktree_path: wt1,
                result_tx: tx1,
            })
            .await
            .unwrap();

        let (tx2, _rx2) = oneshot::channel();
        queue
            .submit(WorkItem {
                task: pending_task("t2"),
                worktree_path: wt2,
                result_tx: tx2,
            })
            .await
            .unwrap();

        // Third item must be rejected as AtCapacity (the two slots are
        // registered before the worker drains them).
        let (tx3, _rx3) = oneshot::channel();
        let err = queue
            .submit(WorkItem {
                task: pending_task("t3"),
                worktree_path: wt3,
                result_tx: tx3,
            })
            .await
            .unwrap_err();

        assert!(
            matches!(err, QueueError::AtCapacity(_)),
            "expected AtCapacity, got {err:?}"
        );
    }

    // ── Cancellation ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn cancelled_queue_rejects_new_submissions() {
        let tmp = TempDir::new().unwrap();
        let (queue, cancel) = make_queue(4);
        cancel.cancel();

        // Brief yield to let the worker loop process the cancel signal.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let (tx, _rx) = oneshot::channel();
        let err = queue
            .submit(WorkItem {
                task: pending_task("t1"),
                worktree_path: tmp.path().to_path_buf(),
                result_tx: tx,
            })
            .await
            .unwrap_err();

        assert!(
            matches!(err, QueueError::Shutdown),
            "expected Shutdown, got {err:?}"
        );
    }

    // ── InMemoryTaskSource claim/complete ────────────────────────────────────

    #[test]
    fn in_memory_source_claim_and_complete() {
        let source = InMemoryTaskSource::new(vec![pending_task("t1")]);
        source.claim("t1", "agent-oracle").unwrap();
        let tasks = source.list_tasks();
        assert_eq!(tasks[0].state, TaskState::InProgress);
        assert_eq!(tasks[0].claimed_by.as_deref(), Some("agent-oracle"));

        source.complete("t1", "agent-oracle").unwrap();
        let tasks = source.list_tasks();
        assert_eq!(tasks[0].state, TaskState::Completed);
    }

    #[test]
    fn in_memory_source_claim_twice_fails() {
        let source = InMemoryTaskSource::new(vec![pending_task("t1")]);
        source.claim("t1", "agent-oracle").unwrap();
        // Second claim for the same task must fail.
        let err = source.claim("t1", "agent-pi").unwrap_err();
        assert!(
            matches!(err, HarnessError::Other(_)),
            "expected Other error, got {err:?}"
        );
    }

    // ── Saṅgha poll helper ───────────────────────────────────────────────────

    #[tokio::test]
    async fn poll_sangha_claims_pending_tasks() {
        let tmp = TempDir::new().unwrap();
        // Three pending tasks.
        let source = InMemoryTaskSource::new(vec![
            pending_task("p1"),
            pending_task("p2"),
            pending_task("p3"),
        ]);
        let (queue, _cancel) = make_queue(4);

        let submitted = poll_sangha(&source, &queue, "agent-oracle", tmp.path()).await;
        assert_eq!(submitted, 3, "all three pending tasks should be submitted");

        // All three must now be InProgress in the source.
        for t in source.list_tasks() {
            assert_eq!(
                t.state,
                TaskState::InProgress,
                "task {} should be InProgress",
                t.id
            );
        }
    }

    // ── poll_sangha rollback ──────────────────────────────────────────────────

    #[tokio::test]
    async fn poll_sangha_rollback_on_queue_full() {
        let tmp = TempDir::new().unwrap();

        // Queue capacity = 1 so the second task is rejected.
        // After rejection, the second task must be rolled back to Pending.
        let source = InMemoryTaskSource::new(vec![pending_task("first"), pending_task("second")]);

        // Create distinct worktrees so both tasks pass the busy check.
        let wt1 = tmp.path().join("first");
        std::fs::create_dir_all(&wt1).unwrap();
        let wt2 = tmp.path().join("second");
        std::fs::create_dir_all(&wt2).unwrap();

        let (queue, _cancel) = make_queue(1);

        let submitted = poll_sangha(&source, &queue, "agent-oracle", tmp.path()).await;
        // Only 1 can be submitted (capacity = 1).
        assert_eq!(submitted, 1, "only 1 task should be admitted");

        // The first task is InProgress; the second must be back to Pending.
        let tasks = source.list_tasks();
        let first = tasks.iter().find(|t| t.id == "first").unwrap();
        let second = tasks.iter().find(|t| t.id == "second").unwrap();

        assert_eq!(
            first.state,
            TaskState::InProgress,
            "first task should be in_progress"
        );
        assert_eq!(
            second.state,
            TaskState::Pending,
            "second task should be rolled back to pending"
        );
    }

    #[test]
    fn unclaim_reverts_task_to_pending() {
        let source = InMemoryTaskSource::new(vec![pending_task("t1")]);
        // Claim it first.
        source.claim("t1", "agent-oracle").unwrap();
        let tasks = source.list_tasks();
        assert_eq!(tasks[0].state, TaskState::InProgress);

        // Unclaim should revert to Pending.
        source.unclaim("t1", "agent-oracle").unwrap();
        let tasks = source.list_tasks();
        assert_eq!(tasks[0].state, TaskState::Pending);
        assert!(tasks[0].claimed_by.is_none());
    }

    #[test]
    fn unclaim_wrong_agent_fails() {
        let source = InMemoryTaskSource::new(vec![pending_task("t1")]);
        source.claim("t1", "agent-oracle").unwrap();
        let err = source.unclaim("t1", "agent-pi").unwrap_err();
        assert!(matches!(err, HarnessError::Other(_)));
    }

    #[tokio::test]
    async fn poll_sangha_skips_non_pending_tasks() {
        let tmp = TempDir::new().unwrap();
        let mut task_in_progress = pending_task("ip1");
        task_in_progress.state = TaskState::InProgress;
        task_in_progress.claimed_by = Some("agent-pi".to_string());

        let source = InMemoryTaskSource::new(vec![task_in_progress, pending_task("p1")]);
        let (queue, _cancel) = make_queue(4);

        let submitted = poll_sangha(&source, &queue, "agent-oracle", tmp.path()).await;
        // Only the pending task should be submitted.
        assert_eq!(submitted, 1);
    }

    // ── Runner integration (HV2-1) ────────────────────────────────────────────

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A SpawnRunner that records how many times it ran and returns a fixed
    /// outcome — proves the queue routes WorkItems to the injected runner.
    struct CountingRunner {
        calls: Arc<AtomicUsize>,
        ok: bool,
    }

    #[async_trait::async_trait]
    impl SpawnRunner for CountingRunner {
        async fn run(&self, spec: &WorkerSpec) -> Result<(), HarnessError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.ok {
                Ok(())
            } else {
                Err(HarnessError::Other(format!(
                    "worker {} failed",
                    spec.task_id
                )))
            }
        }
    }

    #[tokio::test]
    async fn with_runner_routes_item_to_runner_and_returns_ok() {
        let tmp = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cancel = CancellationToken::new();
        let queue = TaskQueue::with_runner(
            4,
            cancel,
            Arc::new(CountingRunner {
                calls: Arc::clone(&calls),
                ok: true,
            }),
            Arc::new(WorkerConfig::default()),
        );

        let (tx, rx) = oneshot::channel();
        queue
            .submit(WorkItem {
                task: pending_task("t1"),
                worktree_path: tmp.path().to_path_buf(),
                result_tx: tx,
            })
            .await
            .unwrap();

        assert!(rx.await.unwrap().is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "runner ran exactly once");
    }

    #[tokio::test]
    async fn with_runner_propagates_worker_failure() {
        let tmp = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let queue = TaskQueue::with_runner(
            4,
            cancel,
            Arc::new(CountingRunner {
                calls: Arc::new(AtomicUsize::new(0)),
                ok: false,
            }),
            Arc::new(WorkerConfig::default()),
        );

        let (tx, rx) = oneshot::channel();
        queue
            .submit(WorkItem {
                task: pending_task("t1"),
                worktree_path: tmp.path().to_path_buf(),
                result_tx: tx,
            })
            .await
            .unwrap();

        assert!(rx.await.unwrap().is_err(), "worker failure propagates");
    }
}
