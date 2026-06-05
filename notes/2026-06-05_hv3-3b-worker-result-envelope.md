# 2026-06-05 — HV3-3b: Worker result envelope

Part of HV3-3 (Saṅgha collaboration; plan: `notes/2026-06-05_harness-v3-plan.md`).
The decision-free half of the workstream: a Saṅgha worker now returns a
**structured result** to the lead instead of a bare exit code. (HV3-3a team-chat
broadcast and HV3-3c peer-review gate remain — the latter still gated on the
reviewer-selection decision.)

## What changed

- **New `bwoc-harness::result`** — `WorkerResult` (task, success, turns,
  compactions, token-pressure switches, active model, bounded `summary`, and a
  `DiffSummary`) + `DiffSummary { files_changed, insertions, deletions }`
  computed from the worktree via `git diff --numstat HEAD` plus an untracked-file
  count. Serde JSON at `<worktree>/.bwoc/worker-result.json` (`RESULT_FILE`).
- **Worker write-side** (`main.rs::run`): at session end the worker writes the
  envelope for **both** success and failure (before the abort propagates),
  beside the existing Tier 2 mine. Best-effort — a write failure warns, never
  fails the run.
- **Lead read-side** (`lead.rs`): on worker success the lead reads the envelope
  *before* worktree teardown and logs `N turn(s), +X/-Y across F file(s),
  model=…`. A worker that wrote none degrades silently to the exit code.

## Decisions

- **Envelope-as-artifact, not a trait/channel change.** The obvious move —
  widen `SpawnRunner::run -> HarnessResult<WorkerResult>` and the queue's
  `oneshot` — would ripple through `worker.rs`, `queue.rs`, `lead.rs` and every
  runner mock. A worker is a *subprocess* that can't return a value in-process
  anyway, so the file artifact is the real contract; the lead already holds the
  worktree path pre-teardown to read it. Far smaller blast radius, and it is the
  same seam HV3-3c's reviewer reads from. (Mattaññutā — right amount.)
- **Metrics taken from `LoopResult` only.** No new field on `LoopResult` or the
  checkpoint `RunState` (a resumed run couldn't faithfully reconstruct a token
  total). Full per-turn token accounting already lives in
  `session-metrics.jsonl`; the envelope is a *summary for the lead*, not a
  second metrics store.
- **Diff counts untracked files.** A worker that only adds new files would read
  as a no-op under `git diff` alone, so `ls-files --others --exclude-standard`
  is folded into `files_changed`.

## Tests

- `result.rs`: numstat parsing (incl. binary `-\t-` rows), char-boundary
  truncation (Thai), write/read roundtrip, absent-envelope `None`, and a
  **git-backed** `from_worktree` integration test (tracked edit + untracked
  file) — not cfg-gated, matching the existing unconditional worktree tests.
- `lead.rs`: `EnvelopeRunner` writes a real envelope into the worktree; the lead
  reads-and-logs it and **still tears the worktree down** (guards against a read
  that blocks removal).

## Status / next

- HV3-3b done. Remaining in HV3-3: **(a)** team-chat broadcast and **(c)** the
  peer-review gate — (c) still needs the architect's reviewer-selection decision
  (fixed / round-robin / manifest-declared). HV3-5's MCP-vs-A2A also open.

## Related

- `crates/bwoc-harness/src/{result,lead}.rs`, `main.rs::run`
- `notes/2026-06-05_harness-v3-plan.md` (HV3-3 workstream)
