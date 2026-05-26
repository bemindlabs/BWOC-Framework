# 2026-05-26 — HV2-1 Saṅgha runtime: lead spawns subprocess workers (BWOC-4)

Replaced the `queue.rs` no-op executor placeholder with real worker spawning, and added a lead loop that drains the Saṅgha task list, gives each task its own git worktree, and runs a `bwoc-harness` subprocess worker per task. The headline harness-v2 workstream; unblocked by HV2-2. Third workstream in the same auto-pilot batch; scope confirmed with the maintainer (full driver + real spawn; worktrees via `git worktree add`).

## What changed

- New `crates/bwoc-harness/src/worker.rs`:
  - `SpawnRunner` trait (`async run(&WorkerSpec)`), injectable. `NoopRunner` (default) + `SubprocessRunner` (spawns `current_exe() --task --workdir --model --endpoint [--skip-model-check]` via `tokio::process`, maps exit code → `Result`).
  - `WorkerSpec` / `WorkerConfig`. `git_worktree_add` / `git_worktree_remove` (`git -C <repo> worktree add --detach` / `remove --force`).
- `queue.rs`: `TaskQueue` now carries a `RunnerCtx { runner, config }`. `new()` keeps the no-op default (existing scheduling/cancellation tests unchanged); new `with_runner()` enables real spawning. `execute_item` builds a `WorkerSpec` from the `WorkItem`'s task + config and delegates to the runner — the queue never runs task code in-process.
- New `lead.rs`: `JsonlTaskSource` (file-backed `TaskSource` over `tasks.jsonl`, via `bwoc_core::team::{parse,render,claim,complete}_tasks`); `run_lead()` driver — claim → `git worktree add` → submit → await → complete + remove worktree (success) / unclaim + keep worktree (failure). `LeadConfig` / `LeadSummary`.
- `main.rs`: `--lead` mode (`--tasks`, `--agent`, `--max-tasks`, `--concurrency`), mutually exclusive with `--task`/`--resume`. The parent spawns + waits + records; it never calls a provider.
- Tests: NoopRunner/SubprocessRunner exit-code + spawn-failure mapping (`/usr/bin/true`/`false`), git worktree add/remove against a temp repo, queue routes-to-runner + propagates-failure, `run_lead` success/failure/max-tasks, `JsonlTaskSource` claim/complete/unclaim roundtrip. **223 lib tests green**; clippy + fmt clean.

## Decisions

- **Worker = subprocess, never in-process.** The OS sandbox is process-scoped (landlock = process tree; `sandbox-exec` wraps a process), so an in-process worker would inherit the lead's sandbox profile + address space and a compromise would leak into the lead. A subprocess re-applies the full guardrails→permission→sandbox pipeline from scratch — the invariant wraps each worker for free, the lead writes no new exec path of its own. *Sīla — the safety invariant must hold for every new exec path; subprocess is the only option that keeps it true without new policy code.*
- **`--detach` worktrees.** No branch name → no collision between concurrent workers; the worker commits onto detached HEAD and the lead collects. *Anattā — the worktree is torn down on success; nothing clings to a finished run.*
- **Failure keeps the worktree, rolls back the claim.** A failed worker's worktree is left for post-mortem (Sutamayā feeds the retrospective from HV2-3); the task reverts to `Pending` so it is re-claimable rather than stranded `InProgress`.
- **`new()` stays a no-op default; spawning is opt-in via `with_runner()`.** Preserves every existing queue test and keeps embedders that only want scheduling unaffected. *Mattaññutā — add the seam, don't force the behaviour on existing callers.*
- **Only `SpawnRunner` is a trait.** Worktree creation uses real `git` (tested against a temp repo) rather than a second injectable abstraction — one seam is enough to test the orchestration without spawning real agents.

## Alternatives considered

- In-process `run_loop` workers — rejected (shared sandbox/address space breaks the safety invariant; the planning-note decision).
- Branch-per-task worktrees (`-b sangha/<id>`) — rejected for the first build (re-run branch-name collisions); `--detach` is simpler and collision-free.
- A `WorktreeProvider` trait for testability — rejected (over-abstraction; real git against a temp repo tests it directly).

## Status / deferred

- Status set to `review` on the workspace board (BWOC-4).
- **Sequential collection** (submit one worker, await, next). Correct and simple; the queue + per-worktree guard already support concurrent collection up to `--concurrency` — that is a deferred follow-up.
- `JsonlTaskSource` serialises in-process via a mutex; cross-process file-locking is the CLI's concern (the lead is the single writer here).
- No docs touched → no EN/TH parity work.

## Related (links)

- `notes/2026-05-25_harness-v2-planning.md` — epic plan + HV2-1 seams (queue.rs:53 TaskSource, :295 placeholder, :333 poll_sangha).
- `notes/2026-05-26_hv2-2-durable-runs.md`, `notes/2026-05-26_hv2-3-run-end-retrospective.md` — sibling workstreams (BWOC-5, BWOC-6).
- GH #39 (harness-v2 epic, HV2-1). `<workspace>/.scrum/backlog.json` — BWOC-4.
