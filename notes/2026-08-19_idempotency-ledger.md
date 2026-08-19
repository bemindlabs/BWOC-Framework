# 2026-08-19 — IdempotencyLedger (L3 Phase 1)

The sole net-new primitive the L3 **monitoring/alerting flagship** needs — a
durable dedup/edge-trigger so a poll-driven loop acts *once* rather than every
fire. Phase 1 of the ratified L3 sequencing (monitoring flagship first, zero
trust-model change); the `bwoc monitor --loop` consumer is Phase 2.

## What changed

- New `crates/bwoc-core/src/idempotency.rs` — `IdempotencyLedger`, a tiny
  `std`+`serde_json` `key→value` sidecar with two operations:
  - `seen_or_record(key) -> bool` — one-shot idempotency (period-bucket keys for
    a once-per-period digest; message-id keys for at-most-once).
  - `latch(key, value) -> bool` — edge-trigger: true iff `value` changed since
    last record. This is what "alert once per trip" needs — a plain seen-set
    keyed on `loop+state` would suppress a *second* trip forever; the latch fires
    on each OK→TRIP transition.
  - `get(key)` — non-mutating read.
- Registered in `bwoc-core/src/lib.rs`.

## Decisions

- **Two ops, one durable map.** The digest (`seen_or_record`) and the monitor
  (`latch`) share one atomically-written (`tmp`+rename) sidecar — the single
  primitive the workflow synthesis called for.
- **`latch` first-observation = transition.** A key with no prior value counts as
  changed, so a monitor starting against an already-tripped source alerts on
  startup. A caller wanting seed-silent startup ignores the first result. (Mirror
  of the a2a `collect_changes` edge-trigger, made durable.)
- **No lock, single-owner.** Same posture as `inbox.rs` check-then-write: a loop
  owns its own ledger; concurrent writers to the *same* ledger are out of scope
  (an advisory lock is a later hardening if one appears).
- **GC deferred (Mattaññutā).** `latch` keys are bounded (one per loop); only the
  digest period-bucket mode accumulates, and that loop is Phase 4. The `{k,v}`
  line shape carries no schema that would block adding a timestamp + age-prune
  later.

## Reuse grounding

Generalizes two existing shapes: `inbox.rs::is_duplicate`/`append_envelope_deduped`
(check-then-append dedup) and `bwoc-a2a serve.rs::collect_changes` (the in-memory
edge-trigger `HashMap<id,state>`, here made restart-durable).

## Status / deferred

- **Phase 2** (next): `bwoc monitor --loop` — fetch → predicate → on trip-transition
  `bwoc send`, all in a trusted wrapper (the untrusted fetched bytes never drive
  an effectful model tool → #271 untouched). Its source/predicate UX is the next
  design call.

## Related

- L3 design proposal: the #410/L3 workflow synthesis (issue #452 carries the ADR).
- LOOP-ENGINEERING spec: `docs/en/LOOP-ENGINEERING.en.md` (L3 = flagship monitoring).
