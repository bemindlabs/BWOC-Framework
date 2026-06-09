# 2026-06-09 — t6 NPROC floor must count tasks (threads), not processes

The turn-executor child died (`OS can't spawn worker thread: EAGAIN`) on any host
whose UID runs many threads, taking down 3 `process_isolation` tests
(`write_file…`, `c8_memory_write…`, `c2_token_scrubbed…`) with an
`UnexpectedEof` in the parent's IPC read.

## Root cause
t6's RELATIVE `RLIMIT_NPROC` floor is `current_uid_proc_count() + headroom(128)`.
On Linux `current_uid_proc_count()` counted only `/proc/<pid>` entries —
**processes** — but the kernel enforces `RLIMIT_NPROC` against the per-UID
**task (thread)** count (each thread bumps `user->processes` in `copy_process`).
On a heavily-threaded host (dev box: 73 processes vs 1275 threads, ~17×) the
floor was set far below current usage, so the child's first `clone()` (tokio
spawning a worker thread for an async tool) hit `EAGAIN`, panicked, and exited
before writing its framed response → parent `UnexpectedEof`. CI stayed green:
a runner's threads ≈ processes, so the floor cleared usage.

t9's cgroup `pids.max` (the absolute cap that would have masked this) is
unavailable here — no delegated writable cgroup v2 subtree — so the path fell
through to exactly this t6 floor.

## What changed
- `current_uid_proc_count()` (Linux) now sums `/proc/<pid>/task` for each owned
  process, measuring usage in the same unit the kernel caps. macOS variant
  unchanged (BSD `RLIMIT_NPROC` is per-process). Doc comments corrected.
- New regression test `nproc_usage_counts_threads_not_processes`: spawns 24 live
  threads in one process and asserts the count tracks them. Verified it fails
  pre-fix (before==during, process count unmoved) and passes post-fix.

## Decisions
- Fix the measurement, not the headroom. Counting tasks keeps the RELATIVE
  semantics intact (usage + headroom) and does not weaken the fork guard — it
  removes a false starvation where the floor sat below live usage.
- Kept the function name (`…_proc_count`): the macOS impl legitimately counts
  processes; only the Linux unit changes. Renaming would over-imply a semantic
  shift on macOS. (Mattaññutā — minimal diff.)

## Status
Lib green; `process_isolation` 9/12 → **12/12** on the dev host. clippy/fmt clean.

## Related
- Phase 5 t6/t9 containment (turn_executor / cgroup). Follow-up to #265.
