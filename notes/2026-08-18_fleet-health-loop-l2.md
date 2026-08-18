# 2026-08-18 — Fleet-health loop (loop-engineering L2, first slice)

The first L2 loop from the [loop-engineering plan](../docs/en/LOOP-ENGINEERING.en.md):
a **reconcile loop** (k8s-style) that drives observed fleet health toward
all-green by re-scanning on a ticker and auto-remediating the one auto-fixable
warn class. Second concrete loop after L1 (goal loop over the lead).

## What changed

- **`crates/bwoc-cli/src/fleet.rs`** — `bwoc fleet health --loop`: re-runs the
  7-condition scan (`evaluate_all`) on a ticker; when condition 2 (stale
  PID/socket) warns, runs `bwoc doctor --auto` to remediate. Terminates on:
  - **DoD** — no warns remain (all conditions green; `Info` is an acceptable
    steady state).
  - **Blocked** — a warn no auto-fix can clear (only condition 2 is remediable),
    or a remediation that didn't reduce the warn count (doctor can't fix it —
    stop rather than spin). Surfaced to the operator with the remaining warns.
  - **Budget** — a hard `--loop-max-iters` ceiling (default 20; `0` = unbounded).
  The gate is a pure `fleet_loop_decide(warn_numbers, prev_warn_count)` so the
  DoD/blocked logic is unit-tested without a workspace or subprocesses.
- **`crates/bwoc-cli/src/main.rs`** — `--loop`, `--loop-interval-secs` (30),
  `--loop-max-iters` (20) on `bwoc fleet health`.

## Decisions
- **Condition 2 is the only auto-remediable class.** `doctor --auto` only clears
  stale PID/socket artifacts; other warns (template-version lag, etc.) are
  surfaced as Blocked for the operator rather than spun on.
- **Stall detection stops a non-fixable condition-2 warn.** If a remediation
  fire doesn't reduce the warn count, the loop stops (doctor couldn't clear it)
  instead of looping to the budget.
- **Sync CLI loop** (`std::thread::sleep`), not the async daemon — `fleet health`
  is a CLI command; hosting a resident version on the daemon idle loop is L2's
  next slice.

## Status / deferred
- The shared **Ticker abstraction** (`Every`/`Adaptive`/`Cron`) is still inline
  per-loop (L1 and this both hand-roll a fixed-interval sleep). Extract it into a
  reusable type when the Tier-2-mining loop (next L2 slice) needs a second
  consumer — Mattaññutā, don't abstract on one use.
- **Cron** cadence + **daemon-hosted** (resident) variant deferred to later L2.
- **L3** (product loops, middle trust tier) remains design-gated.

## Related
- [`docs/en/LOOP-ENGINEERING.en.md`](../docs/en/LOOP-ENGINEERING.en.md) (spec, PR #436).
- [`notes/2026-08-18_goal-loop-l1.md`](2026-08-18_goal-loop-l1.md) (L1).
