# 2026-06-18 — Inbox dedup + delivery receipt (issue #299)

Gateway/council messages were delivered more than once — council notices arrived
**3×** in one inbox and blind "re-nudge" retries stacked up — and senders had no
signal that a message landed, so they re-sent. Root: three independent writers
(`bwoc send`, the a2a/gateway receiver, council notifications) each appended to
`.bwoc/inbox.jsonl` with no idempotency on the envelope's `messageId`.

## What changed

- **One idempotent writer** — `bwoc_core::inbox`:
  - `append_envelope_deduped(path, message_id, line) -> Delivery` — appends unless
    an envelope with that `messageId` is already present; returns `Delivered` or
    `Duplicate` (a minimal **delivery receipt** the sender can surface).
  - `is_duplicate(path, message_id)` — the bare check, for a writer that has its
    own append path (the a2a receiver's size-capped writer).
- **Wired all three writers** through it:
  - `bwoc send` — a re-send of the same id prints "already delivered — duplicate
    suppressed" instead of stacking a line (and skips the tmux wakeup).
  - council `deliver` — a re-delivered turn isn't double-counted or duplicated.
  - a2a `send_message` — a peer retry (lost ack) is **acked but not appended
    twice**; the dedup check runs before the existing 64 MiB cap writer.

## Decisions

- **Dedup at the writer, keyed on `messageId`.** Every envelope already carries
  one (`send::generate_message_id`, mirrored by council + a2a), so this is the
  natural idempotency key — no new field.
- **Check-then-append, not a lock.** The reported duplicates are *sequential*
  (a notify loop, a blind retry), which this fixes. A `flock` against a genuinely
  concurrent writer is a later hardening, documented in the module.
- **a2a returns the same ack for a duplicate** — idempotent from the peer's view:
  retrying after a lost ack converges instead of double-delivering.

## Status / deferred

- This is the **delivery** half of #299 (dedup + a delivered/duplicate signal).
  Sender-queryable **read** receipts (did the recipient consume it?) need the
  reader to write back a cursor/ack the sender can poll — a larger, separate
  slice. The `bwoc triage` receipts (#296) and the daemon inbox cursor are the
  building blocks for it.

## Related

- issue #299; `crates/bwoc-core/src/inbox.rs`,
  `crates/bwoc-cli/src/{send.rs,council.rs}`, `crates/bwoc-a2a/src/rpc.rs`
