# 2026-08-19 — Loop-Engineering control-center TUI (PR2: start/stop + live log)

Second slice of `bwoc loop`. PR1 was observe-only; PR2 makes it *drive* a
goal-loop: `s` starts `bwoc-harness --lead --loop` over the selected team's
`tasks.jsonl`, its stdout+stderr stream into a live log pane, `x` stops it.

## What changed

- **`crates/bwoc-loop-tui/src/run.rs`** (new module) — owns the subprocess:
  `LaunchSpec` (argv builder), `LoopRun` (spawn / drain / poll / stop, RAII
  `Drop`), `LogBuf` (bounded tail), `parse_outcome` + `upgraded_status` +
  `status_label` (pure). Both harness streams are piped and read by one reader
  thread each into a single `mpsc<String>`; liveness is `child.try_wait()` (the
  two senders make channel-disconnect unreliable). The crate keeps its
  dep-quarantine: `bwoc-core` + `ratatui` + `crossterm` only, harness resolved as
  a sibling binary and spawned at runtime.
- **`lib.rs`** — `App` gains `run` + backend/model/endpoint; `start_loop` /
  `stop_loop` / `tick_loop`; `event_loop` drains+polls each tick and binds `s`/`x`;
  `draw_loop` renders the status header + log tail in the right column (Goal on
  top, Loop below); footer flips `s start`↔`x stop`; team switch is blocked while
  a loop runs. `bwoc loop` gains `--backend/--model/--endpoint` passthrough.

## Grounding (understand workflow)

A 4-agent fan-out mapped the harness contract before coding. Load-bearing facts
that shaped the design: the tasks flag is `--tasks <path>` (requires `--lead`);
the outcome is conveyed by the **final stdout line, not the exit code** (Done,
Blocked, BudgetExhausted all exit 0); per-task progress is on **stderr**; a real
loop needs a live backend + a git-repo workspace (workers spawn real
`--task` runs). The workspace-root-must-be-a-git-repo and orphan-worker facts are
surfaced honestly rather than hidden.

## Adversarial review — confirmed bugs fixed before merge

A find→verify workflow (4 dimensions → refute) confirmed and this PR fixes:

- **Reap race (high)** — `poll` could reap the child (sealing `Finished{outcome:
  None}`) before the summary line travelled reader→channel, mislabelling a
  successful loop as a red "exited (code 0)". Fix: `upgraded_status` lets a later
  `drain` recover the outcome from `Finished{None}` too (non-blocking — a
  blocking re-drain would freeze the UI thread). Unit-tested.
- **Drop deadlock (high)** — workers inherit the lead's capture pipe; killing
  only the lead orphaned a worker holding the write-end, so the reader-thread
  `join()` blocked forever (TUI hangs on quit). Fix: spawn the lead as a Unix
  process-group leader and SIGKILL the whole group on teardown (reaps workers →
  pipes close); reader threads are **detached** (never joined) as the
  cross-platform belt-and-suspenders so teardown can never block.
- **Parser latch (medium)** — worker/LLM output on the shared stream could
  false-positive the outcome parser. Fix: `parse_outcome` matches the **full line
  shape** (prefix + interior markers + tail), not a loose prefix.
- **Render math (low)** — extracted `log_rows` / `log_line_width` pure helpers
  with saturating math, tested at tiny heights/widths (no underflow/panic).

## Decisions

- **Session-only, no daemon.** A loop is a foreground child of the TUI, bound to
  the team it started on; quitting kills it. Daemon-hosted loops remain out of
  scope (the daemon already drains reactively — see the task-poll note).
- **libc only on Unix**, `[target.'cfg(unix)'.dependencies]`, for the group kill
  (`bwoc-cli` already depends on it). Not a quarantine violation — it is a system
  crate, not a `bwoc` crate.

## Status / deferred

- **Known limitation**: on non-Unix, or if a group-kill misses a worker, a
  detached reader + orphan worker can linger until process exit (no hang, minor
  leak). The clean source-side fix is harness worker stdio capture (worker.rs) —
  a separate concern, not this PR.
- **PR3**: edit ticker / budget / tasks in-TUI (add task, adjust cadence/budget,
  approve a plan-gated task) via the locked `bwoc task …` CLI path.
- Spec cross-ref (`LOOP-ENGINEERING.{en,th}.md`) + handbook entry still deferred
  to the feature-complete slice.

## Related

- PR1 (observe): `notes/2026-08-18_loop-tui-observe.md`.
- L1 goal-loop runtime: `crates/bwoc-harness/src/lead.rs`, `main.rs::run_lead_mode`.
