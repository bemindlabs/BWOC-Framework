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
//! Collection runs **up to `--concurrency` workers in parallel**: the lead
//! keeps that many in flight, claiming + submitting new tasks as earlier ones
//! finish (`--concurrency 1` reproduces the original one-at-a-time behaviour).
//! Claim/complete/unclaim and the summary are only ever touched from the single
//! lead task across `.await` points — never in parallel — so the parallelism is
//! confined to the queue's worker pool, each worker in its own worktree.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures_util::stream::{FuturesUnordered, StreamExt};
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
    /// Designated peer reviewer agent id (HV3-3c). When `Some` and different
    /// from `agent_id`, each successful worker's diff is routed to the injected
    /// [`Reviewer`] before completion; a rejection re-queues the task. `None`
    /// (or self) = no review gate.
    pub reviewer: Option<String>,
}

/// Outcome counts from a [`run_lead`] invocation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LeadSummary {
    pub claimed: usize,
    pub completed: usize,
    pub failed: usize,
    /// Workers that succeeded but whose diff the reviewer rejected — re-queued
    /// (HV3-3c). Distinct from `failed` (worker errored).
    pub rejected: usize,
}

/// Drain claimable tasks from `source`, spawning a worker per task via `runner`.
///
/// Returns the outcome counts.  Best-effort and resilient: a worktree-creation
/// or spawn failure for one task rolls back that claim and moves on rather than
/// aborting the whole loop.
pub async fn run_lead(
    source: &dyn TaskSource,
    runner: Arc<dyn SpawnRunner>,
    reviewer: Arc<dyn crate::review::Reviewer>,
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

    // Concurrent collection up to `cfg.capacity` (`--concurrency`): keep that
    // many workers in flight, claiming + submitting new tasks as earlier ones
    // finish. `capacity = 1` reproduces the original sequential behaviour. The
    // `source` claim/complete/unclaim and `summary` are only ever touched from
    // this one task (across `.await` points, never in parallel), so there is no
    // shared-state race; the actual parallelism is the queue's worker pool.
    let cap = cfg.capacity.max(1);
    let mut pending = source
        .list_tasks()
        .into_iter()
        .filter(|t| t.state == TaskState::Pending);
    // Each in-flight entry resolves to (task, worktree, worker-result).
    let mut inflight = FuturesUnordered::new();

    loop {
        // Top up to capacity with freshly claimed + submitted tasks.
        while inflight.len() < cap {
            if cfg.max_tasks != 0 && summary.claimed >= cfg.max_tasks {
                break;
            }
            let Some(task) = pending.next() else { break };
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
            inflight.push(async move { (task, worktree, rx.await) });
        }

        // Drain one completed worker (or stop when nothing is left).
        let Some((task, worktree, res)) = inflight.next().await else {
            break;
        };
        match res {
            Ok(Ok(())) => {
                // Collect the worker's structured result envelope (HV3-3b)
                // before teardown — it lives in the worktree. A worker that
                // wrote none degrades silently to the exit code we already have.
                if let Some(r) = crate::result::WorkerResult::read(&worktree) {
                    eprintln!("[bwoc-harness] lead: `{}` done — {}", task.id, r.one_line());
                }

                // Peer-review gate (HV3-3c): if a reviewer (≠ the author) is
                // configured, route the worktree's diff to it before completing.
                // A rejection re-queues the task (worktree kept for the next
                // claimer + inspection); fail-safe — the Reviewer impl maps a
                // spawn/timeout/parse failure to a rejection.
                let verdict = match &cfg.reviewer {
                    Some(rid) if rid != &cfg.agent_id => {
                        let spec = crate::review::ReviewSpec {
                            task_id: task.id.clone(),
                            task_title: task.title.clone(),
                            worktree: worktree.clone(),
                            reviewer_agent: rid.clone(),
                            model: cfg.worker.model.clone(),
                            endpoint: cfg.worker.endpoint.clone(),
                        };
                        Some(reviewer.review(&spec).await)
                    }
                    _ => None, // no gate (unset, or would be a self-review)
                };

                match verdict {
                    Some(v) if !v.approved => {
                        eprintln!(
                            "[bwoc-harness] lead: `{}` REJECTED by reviewer `{}` — {} (re-queued)",
                            task.id,
                            cfg.reviewer.as_deref().unwrap_or(""),
                            v.feedback
                        );
                        let _ = source.unclaim(&task.id, &cfg.agent_id);
                        // Keep the worktree so the next claimer (and a human) can
                        // see the rejected diff and the feedback.
                        summary.rejected += 1;
                    }
                    _ => {
                        // Approved, or no gate — complete and tear down (Anattā).
                        if let Err(e) = source.complete(&task.id, &cfg.agent_id) {
                            eprintln!(
                                "[bwoc-harness] lead: complete failed for `{}`: {e}",
                                task.id
                            );
                        }
                        let _ = git_worktree_remove(&cfg.repo_root, &worktree);
                        summary.completed += 1;
                    }
                }
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

// ---------------------------------------------------------------------------
// Goal loop (Loop-Engineering L1) — see docs/en/LOOP-ENGINEERING.en.md
// ---------------------------------------------------------------------------
//
// The first goal+ticker loop: wrap the (already-hardened) `run_lead` drain in a
// Goal (drive the task list to all-`Completed`), a Ticker (re-fire on an
// interval), and a Gate (stop on DoD, on being blocked/HELD, or on the
// iteration budget). `run_lead` drains once and exits; this re-fires it until
// one of the terminal conditions holds — so a loop always provably halts.

/// Ticker + budget for [`run_goal_loop`] — the shared loop-control primitives.
pub struct GoalLoopConfig {
    /// Cadence between fires (floored so a 0 can't spin the loop).
    pub ticker: bwoc_core::loop_control::Ticker,
    /// Iteration ceiling so an unattended loop provably halts (DoD/Blocked still
    /// terminate independently).
    pub budget: bwoc_core::loop_control::Budget,
}

/// Where a [`run_goal_loop`] stopped and why.
#[derive(Debug, PartialEq, Eq)]
pub enum GoalLoopOutcome {
    /// DoD reached — every task is `Completed`.
    Done {
        iterations: usize,
        total: LeadSummary,
    },
    /// No progress possible without help (dependency-blocked, or a task awaits
    /// plan approval — HELD). The loop stops rather than spin.
    Blocked {
        iterations: usize,
        total: LeadSummary,
        reason: String,
    },
    /// The iteration budget was exhausted before DoD.
    BudgetExhausted {
        iterations: usize,
        total: LeadSummary,
    },
}

/// The goal gate, as a pure predicate over the task list after a fire plus how
/// many that fire claimed. Split out so the DoD/HELD logic is unit-testable
/// without spawning workers or git worktrees.
#[derive(Debug, PartialEq, Eq)]
pub enum GoalStatus {
    Done,
    Blocked(String),
    InProgress,
}

pub fn evaluate_goal(tasks: &[Task], claimed_this_fire: usize) -> GoalStatus {
    if tasks.iter().all(|t| t.state == TaskState::Completed) {
        return GoalStatus::Done;
    }
    // Work remains but the fire claimed nothing → nothing is claimable, so the
    // loop can't advance on its own: a dependency cycle, or a task that awaits
    // plan approval (HELD), or an in_progress task that can't complete. Stop.
    if claimed_this_fire == 0 {
        let remaining = tasks
            .iter()
            .filter(|t| t.state != TaskState::Completed)
            .count();
        // A task is *plan-blocked* (HELD) only when plan approval is the actual
        // blocker — it's already claimed (`InProgress`, ran but couldn't complete),
        // or it's `Pending` with every dependency satisfied so nothing but the
        // plan stands in the way. A pending task whose deps aren't done is
        // dependency-blocked, not awaiting plan — don't mislabel it.
        // Precompute the completed-id set so the deps check is O(deps), not
        // O(tasks × deps × tasks).
        let completed: std::collections::HashSet<&str> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Completed)
            .map(|t| t.id.as_str())
            .collect();
        let dep_done = |t: &Task| t.deps.iter().all(|d| completed.contains(d.as_str()));
        let awaiting_plan = tasks.iter().any(|t| {
            t.state != TaskState::Completed
                && t.requires_plan
                && t.plan_approved != Some(true)
                && (t.state == TaskState::InProgress || dep_done(t))
        });
        let reason = if awaiting_plan {
            format!(
                "{remaining} task(s) not completed; some await plan approval (HELD — needs operator)"
            )
        } else {
            format!("{remaining} task(s) not completed and none claimable (dependency-blocked)")
        };
        return GoalStatus::Blocked(reason);
    }
    GoalStatus::InProgress
}

/// Run [`run_lead`] on a ticker until the goal's DoD, a Blocked/HELD gate, or the
/// iteration budget. Accumulates each fire's [`LeadSummary`] into the outcome.
pub async fn run_goal_loop(
    source: &dyn TaskSource,
    runner: Arc<dyn SpawnRunner>,
    reviewer: Arc<dyn crate::review::Reviewer>,
    cfg: &LeadConfig,
    loop_cfg: &GoalLoopConfig,
) -> HarnessResult<GoalLoopOutcome> {
    let mut total = LeadSummary::default();
    let mut iterations = 0usize;
    loop {
        iterations += 1;
        let s = run_lead(source, runner.clone(), reviewer.clone(), cfg).await?;
        total.claimed += s.claimed;
        total.completed += s.completed;
        total.failed += s.failed;
        total.rejected += s.rejected;

        match evaluate_goal(&source.list_tasks(), s.claimed) {
            GoalStatus::Done => return Ok(GoalLoopOutcome::Done { iterations, total }),
            GoalStatus::Blocked(reason) => {
                return Ok(GoalLoopOutcome::Blocked {
                    iterations,
                    total,
                    reason,
                });
            }
            GoalStatus::InProgress => {}
        }
        if loop_cfg.budget.exhausted(iterations) {
            return Ok(GoalLoopOutcome::BudgetExhausted { iterations, total });
        }
        // Ticker: wait before the next fire so a fast-draining loop doesn't spin.
        tokio::time::sleep(loop_cfg.ticker.interval()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::InMemoryTaskSource;
    use crate::worker::WorkerSpec;
    use async_trait::async_trait;
    use std::process::Command;
    use tempfile::TempDir;

    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

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

    /// Runner that records the peak number of concurrently-running workers, so a
    /// test can assert the lead actually parallelises up to `--concurrency`.
    struct ConcurrencyRunner {
        inflight: AtomicUsize,
        max_seen: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl SpawnRunner for ConcurrencyRunner {
        async fn run(&self, _spec: &WorkerSpec) -> HarnessResult<()> {
            let now = self.inflight.fetch_add(1, SeqCst) + 1;
            self.max_seen.fetch_max(now, SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.inflight.fetch_sub(1, SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn lead_runs_workers_concurrently_up_to_capacity() {
        let repo = temp_repo();
        let source =
            InMemoryTaskSource::new(vec![pending("a"), pending("b"), pending("c"), pending("d")]);
        let max_seen = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(ConcurrencyRunner {
            inflight: AtomicUsize::new(0),
            max_seen: max_seen.clone(),
        });
        let mut cfg = lead_cfg(&repo);
        cfg.capacity = 3;
        let summary = run_lead(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &cfg,
        )
        .await
        .unwrap();
        assert_eq!(summary.completed, 4);
        let peak = max_seen.load(SeqCst);
        assert!(peak >= 2, "expected parallel workers, peak was {peak}");
        assert!(peak <= 3, "peak {peak} exceeded --concurrency 3");
    }

    #[tokio::test]
    async fn lead_capacity_one_stays_sequential() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a"), pending("b"), pending("c")]);
        let max_seen = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(ConcurrencyRunner {
            inflight: AtomicUsize::new(0),
            max_seen: max_seen.clone(),
        });
        let mut cfg = lead_cfg(&repo);
        cfg.capacity = 1;
        let summary = run_lead(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &cfg,
        )
        .await
        .unwrap();
        assert_eq!(summary.completed, 3);
        assert_eq!(
            max_seen.load(SeqCst),
            1,
            "capacity 1 must run one at a time"
        );
    }

    // ── goal loop (L1) ─────────────────────────────────────────────────────────

    fn completed(id: &str) -> Task {
        let mut t = pending(id);
        t.state = TaskState::Completed;
        t
    }

    #[test]
    fn evaluate_goal_done_when_all_completed() {
        assert_eq!(
            evaluate_goal(&[completed("a"), completed("b")], 0),
            GoalStatus::Done
        );
    }

    #[test]
    fn evaluate_goal_in_progress_when_a_fire_claimed_work() {
        // Work remains and the fire claimed something → keep looping.
        assert_eq!(
            evaluate_goal(&[completed("a"), pending("b")], 1),
            GoalStatus::InProgress
        );
    }

    #[test]
    fn evaluate_goal_blocked_when_no_progress_dependency() {
        // Nothing claimed, work remains, no plan gate → dependency-blocked.
        match evaluate_goal(&[completed("a"), pending("b")], 0) {
            GoalStatus::Blocked(r) => assert!(r.contains("dependency-blocked"), "got: {r}"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_goal_blocked_flags_plan_approval_held() {
        let mut t = pending("b");
        t.requires_plan = true; // no deps → plan approval is the real blocker → HELD
        match evaluate_goal(&[completed("a"), t], 0) {
            GoalStatus::Blocked(r) => assert!(r.contains("HELD"), "got: {r}"),
            other => panic!("expected Blocked/HELD, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_goal_dep_blocked_task_with_plan_is_not_mislabeled_held() {
        // `b` requires a plan AND depends on the still-pending `a`. The real
        // blocker is the dependency, not plan approval — must read as
        // dependency-blocked, not HELD.
        let mut b = pending("b");
        b.requires_plan = true;
        b.deps = vec!["a".to_string()];
        match evaluate_goal(&[pending("a"), b], 0) {
            GoalStatus::Blocked(r) => {
                assert!(r.contains("dependency-blocked"), "got: {r}");
                assert!(!r.contains("HELD"), "must not mislabel as HELD: {r}");
            }
            other => panic!("expected Blocked/dependency, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn goal_loop_stops_at_dod_when_all_completed() {
        // A list that is already fully Completed: the first fire claims nothing,
        // the DoD predicate is met, and the loop stops at iteration 1 (never sleeps).
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![completed("a"), completed("b")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });
        let cfg = lead_cfg(&repo);
        let loop_cfg = GoalLoopConfig {
            ticker: bwoc_core::loop_control::Ticker::every_secs(0),
            budget: bwoc_core::loop_control::Budget::new(5),
        };
        let outcome = run_goal_loop(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &cfg,
            &loop_cfg,
        )
        .await
        .unwrap();
        assert!(
            matches!(outcome, GoalLoopOutcome::Done { iterations: 1, .. }),
            "expected Done at iter 1, got {outcome:?}"
        );
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
            reviewer: None,
        }
    }

    fn pending(id: &str) -> Task {
        Task::new(id, format!("task {id}"), vec![])
    }

    /// Runner that writes a real result envelope into the worktree before
    /// returning Ok — exercises the lead's HV3-3b read-and-log path and proves
    /// the read doesn't block worktree teardown.
    struct EnvelopeRunner;
    #[async_trait]
    impl SpawnRunner for EnvelopeRunner {
        async fn run(&self, spec: &WorkerSpec) -> HarnessResult<()> {
            crate::result::WorkerResult::new(
                spec.prompt.clone(),
                true,
                2,
                0,
                0,
                "mock",
                crate::result::DiffSummary {
                    files_changed: 1,
                    insertions: 3,
                    deletions: 0,
                },
                "wrote the envelope",
            )
            .write(&spec.worktree)
            .map_err(|e| HarnessError::Other(e.to_string()))
        }
    }

    #[tokio::test]
    async fn lead_reads_envelope_and_still_tears_down_worktree() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a")]);
        let runner = Arc::new(EnvelopeRunner);

        let summary = run_lead(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &lead_cfg(&repo),
        )
        .await
        .unwrap();

        assert_eq!(summary.completed, 1);
        // Envelope was read pre-teardown, then the worktree (and its
        // `.bwoc/worker-result.json`) was removed on success.
        assert!(!repo.path().join(".worktrees").join("a").exists());
    }

    #[tokio::test]
    async fn lead_completes_successful_tasks_and_cleans_worktrees() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a"), pending("b")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });

        let summary = run_lead(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &lead_cfg(&repo),
        )
        .await
        .unwrap();

        assert_eq!(
            summary,
            LeadSummary {
                claimed: 2,
                completed: 2,
                failed: 0,
                rejected: 0,
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

        let summary = run_lead(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &lead_cfg(&repo),
        )
        .await
        .unwrap();

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

        let summary = run_lead(
            &source,
            runner,
            Arc::new(crate::review::AlwaysApprove),
            &cfg,
        )
        .await
        .unwrap();
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

    // ── Peer-review gate (HV3-3c) ─────────────────────────────────────────────

    /// A reviewer with a fixed verdict (no real subprocess).
    struct ScriptedReviewer {
        approve: bool,
    }
    #[async_trait]
    impl crate::review::Reviewer for ScriptedReviewer {
        async fn review(&self, _spec: &crate::review::ReviewSpec) -> crate::review::ReviewVerdict {
            crate::review::ReviewVerdict {
                approved: self.approve,
                feedback: "scripted".to_string(),
            }
        }
    }

    /// A different reviewer agent that approves → the task completes and its
    /// worktree is torn down (the no-gate path with the gate present-but-passing).
    #[tokio::test]
    async fn lead_review_approve_completes_and_cleans_worktree() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });
        let mut cfg = lead_cfg(&repo);
        cfg.reviewer = Some("agent-yanluo".to_string()); // ≠ agent-lead

        let summary = run_lead(
            &source,
            runner,
            Arc::new(ScriptedReviewer { approve: true }),
            &cfg,
        )
        .await
        .unwrap();

        assert_eq!(summary.completed, 1);
        assert_eq!(summary.rejected, 0);
        assert_eq!(source.list_tasks()[0].state, TaskState::Completed);
        assert!(!repo.path().join(".worktrees").join("a").exists());
    }

    /// A rejection re-queues the task (back to Pending), keeps the worktree for
    /// inspection, and counts as `rejected` — not `completed` or `failed`.
    #[tokio::test]
    async fn lead_review_reject_requeues_and_keeps_worktree() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });
        let mut cfg = lead_cfg(&repo);
        cfg.reviewer = Some("agent-yanluo".to_string());

        let summary = run_lead(
            &source,
            runner,
            Arc::new(ScriptedReviewer { approve: false }),
            &cfg,
        )
        .await
        .unwrap();

        assert_eq!(summary.completed, 0);
        assert_eq!(summary.rejected, 1);
        assert_eq!(
            source.list_tasks()[0].state,
            TaskState::Pending,
            "rejected task is re-queued"
        );
        assert!(
            repo.path().join(".worktrees").join("a").exists(),
            "rejected worktree kept for the next claimer / inspection"
        );
    }

    /// A reviewer equal to the claiming agent is a self-review — the gate is
    /// skipped and the task completes (no `Reviewer` call is made).
    #[tokio::test]
    async fn lead_review_skips_self_review() {
        let repo = temp_repo();
        let source = InMemoryTaskSource::new(vec![pending("a")]);
        let runner = Arc::new(ScriptedRunner { fail_ids: vec![] });
        let mut cfg = lead_cfg(&repo);
        cfg.reviewer = Some(cfg.agent_id.clone()); // self → no gate

        // Pass a reviewer that would REJECT if called; completion proves it wasn't.
        let summary = run_lead(
            &source,
            runner,
            Arc::new(ScriptedReviewer { approve: false }),
            &cfg,
        )
        .await
        .unwrap();

        assert_eq!(
            summary.completed, 1,
            "self-review is skipped, task completes"
        );
        assert_eq!(summary.rejected, 0);
    }
}
