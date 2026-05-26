# 2026-05-26 — HV2-2 durable / resumable runs (BWOC-5)

Built the first harness-v2 workstream: per-turn checkpointing of agent-loop state and a `--resume <run-id>` path that reloads and continues against the existing worktree. First build off the #39 planning note (`notes/2026-05-25_harness-v2-planning.md`); design decisions there held, with one dep correction.

## What changed

- New `crates/bwoc-harness/src/checkpoint.rs`:
  - `RunState` (serde) — `{run_id, task, history, turns, compactions, token_pressure_switches, active_model, telemetry_cursor}`. `save_atomic(path)` / `load_from(path)`.
  - `CheckpointConfig {run_id, root, resume}` — wiring carried on `LoopConfig`. `new()` / `resume()` / `path()` / `save()` / `delete()`. `runs_root()` resolves `$BWOC_HOME/runs` → `$HOME/.bwoc/runs` → `./.bwoc/runs`. `new_run_id()` = `run-<unix-secs>-<pid>`.
- `agent_loop.rs`: `LoopConfig.checkpoint: Option<CheckpointConfig>` (default `None` = durability off, preserves all prior behaviour). `run_loop()` seeds locals from `resume` before the loop (no replay), persists after each turn boundary (the two `record_turn` continue-sites), and deletes the checkpoint on the successful final-answer return.
- `main.rs`: `--resume <run-id>` (conflicts with `--task`); `--task` now `Option`. The binary always checkpoints — a fresh run mints a `run_id`, resume loads the prior one.
- Tests: roundtrip, atomic-no-partial-file, idempotent delete (checkpoint.rs); resume-continues-turn-count-no-replay and checkpoint-persisted-per-turn-kept-on-error (agent_loop.rs). 207 lib tests green; clippy + fmt clean.

## Decisions

- **Turn boundary is the only snapshot seam.** Persist after `TurnBuilder::finish()` + `record_turn`, when the reply and tool results are applied to `history` — tools mutate the worktree mid-turn, so a mid-turn snapshot could disagree with disk. *Anicca — persist the seam, not the whole world.*
- **Delete checkpoint on success; keep on crash/error.** A finished run has nothing to resume; an errored run (e.g. `MaxIterations`) keeps its checkpoint so `--resume` works. This resolves the OPEN retention decision in the BWOC-5 handoff in favour of delete-on-success. *Anattā — no clinging to completed state.*
- **Atomic write with `std` only — not `tempfile`.** The planning note assumed `tempfile` was available, but it is a `[dev-dependencies]` entry, not a runtime dep. Promoting it would add a runtime dependency and dent the dep-quarantine. Instead: write a sibling `.checkpoint.<pid>.<nanos>.tmp` + `fsync` + `std::fs::rename` (atomic, same-dir → never crosses a filesystem). Zero new runtime deps. *Mattaññutā — right amount; don't pull in a crate for what `std::fs::rename` already gives.*
- **One `Option<CheckpointConfig>` field on `LoopConfig`, not three loose fields.** Keeps the 26 `run_loop` call sites and the struct lean; `None` is the historical no-durability path.

## Alternatives considered

- `tempfile::NamedTempFile::persist` for the atomic write — rejected (dev-only dep; would breach dep-quarantine).
- Process-global `BWOC_HOME` env override for test isolation — rejected as the *primary* mechanism (racy under parallel tests); tests instead set `CheckpointConfig.root` to a `TempDir`. `BWOC_HOME` remains as an operator override in `runs_root()`.
- Replaying past turns on resume — rejected; the worktree already persists, so reload + re-attach is correct and cheap (matches the planning-note decision).

## Status / deferred

- `telemetry_cursor` is persisted but only consumed as a baseline later — wired for HV2-3 (run-end retrospective), not used this workstream.
- Status set to `review` on the workspace board (BWOC-5) per the epic's build-then-review process.
- Unblocks HV2-1 (BWOC-4) resumable subprocess workers.
- **Still open (separate from this build):** `HARNESS.en.md` §Not-Yet OS-sandbox-stub doc drift — fix EN+TH pair separately, as flagged in the planning note.

## Related (links)

- `notes/2026-05-25_harness-v2-planning.md` — the epic plan + HV2-2 code seams.
- GH #39 (harness-v2 epic, HV2-2); unblocks HV2-1.
- `<workspace>/.scrum/backlog.json` — BWOC-5.
