# 2026-08-18 — Shared loop-control primitives (Ticker + Budget)

Extracted the **Ticker** (cadence) and **Budget** (termination ceiling) that the
goal loop (L1) and the fleet-health reconcile loop (L2) had each hand-rolled into
one shared `bwoc-core` module, so the flooring, budget-exhaustion, and
banner-label logic live in one place. This is the loop-engineering layer's shared
core — done now that there are two real consumers (L1 + L2), not before
(Mattaññutā).

## What changed

- **`crates/bwoc-core/src/loop_control.rs`** (new) — `Ticker::every_secs(secs)`
  (fixed cadence, floored at 1 s so a `0` can't spin a loop) and `Budget`
  (`new`/`exhausted`/`describe`; `0` = unbounded). Pure `std`, no deps. Unit-tested.
- **`crates/bwoc-harness/src/lead.rs`** — `GoalLoopConfig` now holds
  `{ ticker, budget }` instead of `{ interval, max_iterations }`; `run_goal_loop`
  uses `budget.exhausted()` + `ticker.interval()`.
- **`crates/bwoc-cli/src/fleet.rs`** — `run_health_loop` uses the same primitives;
  drops its local interval-floor and budget-describe/exhausted logic.
- **`crates/bwoc-harness/src/main.rs`** — builds `Ticker`/`Budget` from the
  `--loop-*` flags; the `--loop` banner uses `budget.describe()`.

## Decisions
- **`Ticker` ships `Every` only.** The spec's `Adaptive`/`Cron` variants are
  documented as future but not implemented — every current consumer is a fixed
  interval, so adding them now would be an unused abstraction. They join when a
  loop (e.g. a monitoring loop) actually drives them.
- **Lives in `bwoc-core`** so both `bwoc-harness` (L1) and `bwoc-cli` (L2) share
  one copy — the flooring rule and budget semantics can't drift between loops.

## Status / deferred
- `Adaptive`/`Cron` tickers + a daemon-hosted (resident) loop variant remain
  future L2/L3 work, added with their first real consumer.
- L3 (product loops, middle trust tier) stays design-gated.

## Related
- [`docs/en/LOOP-ENGINEERING.en.md`](../docs/en/LOOP-ENGINEERING.en.md) (spec).
- L1: `notes/2026-08-18_goal-loop-l1.md`; L2: `notes/2026-08-18_fleet-health-loop-l2.md`.
