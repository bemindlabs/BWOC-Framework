# 2026-08-19 — `bwoc digest` (L3 recurring-digest loop)

The second Loop-Engineering L3 product loop: run an operator command **once
per period** (hourly/daily/weekly) and deliver its rendered output. Reuses
the Phase-1 `IdempotencyLedger` (#453) — the durable dedup half — as the
once-per-period gate a bare cron can't give (a restart mid-period would re-run
the job; a poll-driven loop re-fires every tick). The latch is a durable
check-then-write for a loop owning its own ledger, **not** a cross-process
mutex — truly concurrent invocations on the same ledger are out of scope (run
one loop per `--id`), matching the primitive's stated single-owner scope.
Sibling of `bwoc monitor` (#454); the monitoring flagship delivered *scalars*,
the digest delivers *content*, and that difference drove the trust design below.

## What changed

- New `crates/bwoc-cli/src/digest.rs` + `bwoc digest` subcommand:
  `bwoc digest --exec "<cmd>" [--period hourly|daily|weekly] [--out <file>]
  [--loop --interval-secs N --max-iters M] [--id X] [--workspace P]`. Each tick
  computes the period bucket `floor(now / period_secs)`, and `seen_or_record`
  latches it: the first call for a bucket delivers (runs `--exec`, renders
  stdout+stderr, writes it out), every later call in that bucket is a no-op.
  `--loop` runs it as a service on the `Every` ticker (max-iters 0 = unbounded);
  without `--loop` it delivers at most once for the current period and exits
  (the cron-driven mode). State is durable under `.bwoc/digests/<id>.jsonl`.

## Decisions

- **v1 delivers to stdout / `--out <file>` only — never a model inbox.** A digest
  delivers *arbitrary command output*, unlike the monitor's scalar. Forwarding
  that into a fleet agent's inbox would put untrusted content in front of a model
  on its next turn — the exact indirect-prompt-injection defect the monitor
  review (#454) caught. So the operator collects the digest; no model is
  involved; the trust posture is unchanged (#271 untouched). Inbox delivery
  (`--to`) is deferred behind the same middle trust tier as the other L3 loops
  (#452).
- **Period = epoch bucket, no cron/calendar parser** (Mattaññutā). `floor(now /
  {3600,86400,604800})` with a per-period tag (`h`/`d`/`w`) so buckets never
  collide in one ledger. Calendar alignment (a real `Cron` ticker) stays deferred
  until a loop genuinely needs wall-clock boundaries.
- **Record-before-deliver (at-most-once).** `tick()` records the bucket *before*
  running `--exec`, so a crash mid-render burns the period rather than risking a
  re-delivery on restart — a duplicate digest is worse than a missed one for this
  use case. A ledger write error skips the period (stays at-most-once) rather than
  fabricating a delivery.
- **`fnv1a` / `is_safe_id` duplicated from `monitor.rs`** with a NOTE — kept
  duplicated while there are only two L3 consumers; consolidate into a shared
  helper once a third appears (the refactor's blast radius isn't earned yet).

## Grounded in an adversarial review (15 raised → 2 real, both fixed pre-PR)

- **Core gate untested (med)** — the first draft's `once_per_period_via_ledger`
  test re-exercised the `bwoc_core` ledger primitive (already pinned by
  `idempotency.rs`), never driving digest's own `tick()`/`deliver()`. Reordering
  deliver-before-record (→ double-delivery, destroying the whole guarantee) would
  have passed all tests green. **Fix:** replaced it with
  `tick_delivers_once_per_bucket_and_burns_on_failure`, which drives the real
  `tick()` against a temp ledger + `--out`: two ticks in one bucket → exactly one
  block; an `exit 4` exec → returns `2`, still delivers, and burns the bucket so a
  retry is a no-op. Pins at-most-once, burn-on-failure, and the exit-code mapping.
- **EPIPE panic on stdout (low)** — `deliver()`'s stdout branch used
  `print!`/`println!`, which `panic!` on a broken pipe (`bwoc digest … | head -1`
  with a body larger than the pipe buffer), while the `--out` branch defensively
  swallowed IO errors. **Fix:** the stdout branch now writes through a locked
  `io::stdout()` handle with the `Result` ignored, matching the file path — a
  broken pipe is a swallowed error, not a panic.

## Status / deferred

- Inbox delivery (`--to`), calendar `Cron` ticker, and `id`-helper consolidation
  all deferred as above — none earns its place until a consumer drives it.

## Related

- Phase 1 primitive: `notes/2026-08-19_idempotency-ledger.md` (#453).
- Sibling flagship: `notes/2026-08-19_monitor-loop.md` (#454).
- Design ADR / middle-tier: issue #452. Spec: `docs/en/LOOP-ENGINEERING.en.md`.
