# 2026-08-19 — `bwoc monitor` (L3 monitoring flagship, Phase 2)

The Loop-Engineering L3 flagship: watch a source on a cadence and alert **once
per transition**. Phase 2 of the ratified L3 sequencing (monitoring first, zero
trust-model change); consumes the Phase-1 `IdempotencyLedger` (#453).

## What changed

- New `crates/bwoc-cli/src/monitor.rs` + `bwoc monitor` subcommand:
  `bwoc monitor --exec "<cmd>" [--alert <agent>] [--loop --interval-secs N
  --max-iters M] [--id X]`. Each tick probes the operator's shell command (exit
  0 = OK, non-zero / spawn-fail / signal = TRIP), detects OK↔TRIP transitions,
  and on a transition `bwoc send`s a fleet member. `--loop` runs it as a service
  (max-iters 0 = unbounded, Ctrl-C stops); without `--loop` it probes once and
  exits 0/1 (CI/cron-friendly). State is durable via the ledger under
  `.bwoc/monitors/<id>.jsonl`, so a restart mid-trip doesn't re-alert *when the
  last write succeeded* — the write is best-effort, so a restart after a failed
  write can re-alert (a duplicate, never a missed edge).

## Grounded in an adversarial review (7 confirmed findings, all fixed pre-PR)

The first draft's "zero trust-model change / #271 untouched" claim was **false**,
and the review caught it — a good ultracode catch a solo pass missed:

- **#271 laundering (high ×2)** — the draft put the probe's stderr tail into the
  `bwoc send` alert body, which lands in a fleet agent's inbox read by a model on
  its next turn (and was sent as `from: user`, the max-trust identity). That is
  an indirect prompt-injection channel — exactly what #271 governs. **Fix:** the
  delivered body is now **trusted scalars only** (monitor id + exit code) via
  `alert_body`; the untrusted stderr is logged *locally* and never enters
  `bwoc send`. The claim is now true, and `alert_body_is_scalar_only` guards the
  regression.
- **Ledger write-failure edge bugs (medium ×2 + low)** — the draft drove
  transition detection off the on-disk `prev`, so a transient write failure could
  *miss* a re-trip (stale disk) or *storm* re-alerts. **Fix:** the `--loop` path
  drives detection off an **in-process `last_state`** (seeded once from the
  ledger, updated every tick regardless of write success); the ledger is a
  best-effort durability cache for the next restart only. A ledger *read* error
  at seed is logged and treated as unknown, not fabricated into a spurious state.
- **Non-stable id hash (low)** — the derived monitor id used `DefaultHasher`,
  whose algorithm can change across Rust versions and would silently orphan a
  monitor's ledger on upgrade. **Fix:** a fixed **FNV-1a**, pinned by a test.

## Decisions

- **A monitor is a service, not a goal-loop.** No DoD; its provable stop is the
  operator / supervisor (Ctrl-C), like the daemon — so `--max-iters 0`
  (unbounded) is the default, with a budget available for bounded/CI runs.
- **Command-based source** (`--exec` via the platform shell) over an HTTP client:
  maximal flexibility (curl/ping/any script), zero new dep, and it keeps the
  trust story clean — only the exit-code scalar drives the decision.
- **Alert = `bwoc send`** (chosen with the architect), scalar body only.

## Status / deferred

- Middle trust tier + the `Dispatch` seam (#452) remain deferred — the monitor
  flagship needs none of them (trusted wrapper, scalar alert).
- Later L3: recurring-digest (reuses the ledger's `seen_or_record` + a Cron
  ticker) and A2A-delegation, when a consumer drives them.

## Related

- Phase 1 primitive: `notes/2026-08-19_idempotency-ledger.md` (#453).
- Design ADR: issue #452. Spec: `docs/en/LOOP-ENGINEERING.en.md` (L3 flagship).
