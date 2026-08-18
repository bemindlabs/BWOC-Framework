# 2026-08-18 — Daemon task-poll cadence: shared Ticker + `BWOC_TASK_POLL_SECS`

The `bwoc-agent --serve` daemon already hosts a reactive drain loop — `task_watch`
scans team task lists on a cadence and (with `BWOC_AUTO_CLAIM=1` + warm) claims
and runs claimable work. That cadence was a hardcoded
`const TASK_POLL_EVERY: Duration = Duration::from_secs(2)`, unrelated to the
shared `Ticker` primitive extracted for the L1/L2 loops (#440). This wires the
daemon onto that primitive and makes the cadence operator-tunable.

## What changed

- `crates/bwoc-agent/src/main.rs`: replaced the hardcoded const with
  `task_poll_interval(std::env::var("BWOC_TASK_POLL_SECS").ok())` — a small pure
  helper that parses the env (unset/unparseable → 2s default) and derives the
  interval via `bwoc_core::loop_control::Ticker::every_secs`, so a `0` is floored
  to 1s and the poll can't spin the team-file reads. The startup banner now
  prints the effective cadence (`… watching Saṅgha tasks for member 'x' every 2s`).
  This makes the daemon the **third consumer** of the shared Ticker (after the
  goal-loop and fleet-health loop) — retroactively confirming the extraction
  earned its place (Samānattatā / DRY).
- Docs: `interconnect/sangha.md` (+ `.th.md` parity), README env table — document
  `BWOC_TASK_POLL_SECS` (default `2`, floored `1`; raise for a large fleet,
  lower for snappier pickup).
- Test: `task_poll_interval_defaults_and_floors` locks the parse-fallback and
  floor branches (the Ticker floor itself is already tested in `loop_control`).

## Decisions

- **Did not** add a daemon-hosted *goal-loop* (DoD/budget run-to-completion).
  Investigation showed a daemon is run-forever and already drains reactively via
  `BWOC_AUTO_CLAIM` + warm; bolting a terminating goal-loop on top overlaps that
  path and fits poorly. The one real, non-speculative gap was the ungoverned
  hardcoded cadence — so that is all this changes (Mattaññutā).
- Env knob, not a CLI flag: the daemon's other serve-time toggles
  (`BWOC_TASK_WAKEUP`, `BWOC_AUTO_CLAIM`, `BWOC_WARM`) are all env vars —
  kept the convention (Sīlasāmaññatā).

## Related

- Ticker/Budget primitive: `notes/2026-08-18_loop-control-ticker-budget.md` (#440).
- Loop-Engineering spec: `docs/en/LOOP-ENGINEERING.en.md`.
