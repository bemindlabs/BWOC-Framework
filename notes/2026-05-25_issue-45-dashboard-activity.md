# 2026-05-25 — Dashboard live agent-activity (#45)

Turned `bwoc dashboard` from a live *roster* into a live *activity* monitor,
closing the dashboard's own deferred Phase 2/4. Each agent row now shows a live
activity state (working/idle/running/—) fed by the `sessions` resolution on the
existing 2s tick; the selected agent's detail pane gains a session block
(backend/pid/last-seen) and a capped log tail. Observe-only: no new control
actions. Reuses `sessions` (#21) and the `log` tail as-is — nothing
reimplemented. Design: agent-oracle. Implementation + tradeoff calls: agent-pi.

## What changed

- `crates/bwoc-cli/src/log.rs` — extracted `pub fn tail_lines(path, n) ->
  io::Result<Vec<String>>` from `print_tail` (which now delegates to it), so the
  dashboard reads the exact same tail the `--follow` path prints. 3 unit tests
  (last-N, fewer-than-N, missing-file-is-Err).
- `crates/bwoc-cli/src/sessions.rs` — made `SessionState::as_str` `pub` (label
  reused by the dashboard; no logic change).
- `crates/bwoc-cli/src/dashboard.rs`:
  - `App` gains `sessions: Vec<Session>` (snapshot), `detail_log: Vec<String>`,
    `detail_log_note: Option<String>`.
  - `refresh()` (the 2s tick) now also calls `collect_sessions(root,
    &ProcessScanRunner, IDLE_SECS)` and `refresh_detail()`. `next`/`prev` call
    `refresh_detail()` too, so the pane never lags the user's own keystroke.
  - New `ACTIVITY` column on the agents table (●working / ◑idle / ●running /
    ○stale / —), single-sourced through pure `activity_display(state)`.
  - Detail pane: a `session` line (state + backend + pid) and a `last seen` age
    line via `format_activity_age` (reuses `livecheck::format_uptime`), plus a
    `log (last N)` tail block.
  - 3 unit tests (`activity_display` all states + absence; `format_activity_age`
    none → "unknown", recent → "… ago").

## Decisions

- **Snapshot on tick, not per-draw.** `collect_sessions` shells out to
  `pgrep`/`tmux`; the draw loop polls at ~200ms. Computing sessions in
  `refresh()` (2s) and reading the cached snapshot per-draw keeps the scan off
  the hot path. The log tail follows the same rule but *also* refreshes on
  selection change, since `tail_lines` is a cheap single-file read and a 2s lag
  on navigation would feel stale.
- **Two distinct liveness notions, kept separate.** The existing `runtime ●`
  line tracks the daemon (`bwoc-agent --serve`); the new `session`/activity
  tracks the interactive backend process (claude/agy/…). They are complementary,
  so the pane shows both rather than collapsing them.
- **"Observe-only" read as additive, not subtractive.** The issue's non-goal
  ("no control actions from the dashboard; defer stop/spawn") scopes *this
  feature* — it does not mandate removing the dashboard's already-shipped
  `s`/`x`/`t`/`g` daemon-control hotkeys. No new session-driving action was
  added; existing hotkeys left untouched. Flagged to oracle for confirmation.
- **No new idle-secs knob.** Hardcoded `IDLE_SECS = 60` to match `bwoc
  sessions`' default rather than adding a dashboard flag (mattaññutā).

## Alternatives considered

- A `BackendDetector`/per-agent activity flag — rejected; `collect_sessions`
  already resolves everything and attributes marker sessions by `agentId`.
- Refreshing the log tail per-draw — rejected; re-reads the whole file 5×/sec.

## Bugs surfaced and fixed

- None. (One `cargo fmt` reflow of a `format!` in `format_activity_age`.)

## Status / deferred

- Done, gates green (clippy `-D warnings`, fmt `--check`, `cargo test
  --workspace`: 253 cli + 202 harness pass, incl. 6 new). Branch
  `feat/issue-45-dashboard-activity`. No PR — awaiting oracle review.
- Scan-sourced sessions (`agentId = null`) can't be attributed to a registry
  row, so they surface in `bwoc sessions` but not the dashboard column — by
  design (degrade to "—").
- #46 (`bwoc inbox --all --watch`) is the next queued item, not started.

## Related (links)

- [[2026-05-25_issue-44-update-check]] — prior item in the same #44→#45→#46 queue.
- GH #45 (this), #21 (`bwoc sessions`).
