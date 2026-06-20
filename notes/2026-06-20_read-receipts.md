# 2026-06-20 — Read receipts: `bwoc receipts` (#299, second half)

Closes the read-receipt half of #299. The first half (#311) made `bwoc send`
idempotent by `messageId` and returned a **delivery** receipt (Delivered /
Duplicate) at write time. This adds the **consumption** ("was it read?") side a
sender can query.

## What changed

- **`messageId` now rides into the triage receipt.** `bwoc triage` (#296)
  already writes a receipt to `<agent>/.bwoc/inbox.triage.jsonl` for every
  envelope it processes; that receipt now carries the source envelope's
  `messageId` (omitted when the envelope has none). One field threaded through
  `Triaged` + `append_receipts`.
- **`bwoc receipts`** (`crates/bwoc-cli/src/receipts.rs`) — a fleet-wide,
  read-only query over those receipt logs. Filters `--message-id` (the
  `bwoc send` `[id …]`), `--from` (sender; bare name also matches `agent-…`),
  `--agent` (recipient), and `--json`. Answers "was my message consumed, and how
  (ack / escalate / forward)?" A message-id query with no hit prints an explicit
  "not consumed yet" line.
- **`bwoc help messaging`** gains a Receipts section covering both halves.

## Decisions

- **Query the recipient's existing triage receipts; don't invent a new ack
  channel.** Consumption is *already* recorded by triage (the no-LLM coordinator
  that drains an inbox). The gap was only that it wasn't keyed by message or
  queryable. `bwoc receipts` is exactly the missing reader — same Mattaññutā
  move as `bwoc tasks` (#300): add the fleet-wide query, nothing more.
- **No receipt-back-to-sender routing.** In a single workspace the sender (or
  operator) reads the recipient's receipt log directly — no envelope round-trip
  needed. A recipient on **another machine** acking back through `bwoc-gateway`
  is a transport follow-up; the issue itself scoped the cross-machine ack as a
  gateway concern.

## Status / deferred

- Covers consumption via `bwoc triage` (the inbox-drain path the issue cites).
  Receipts for messages a **harness session** or the **warm daemon** (#301)
  consumes are not yet written — a natural follow-up (have those paths append to
  the same receipt log) if session-level read receipts are wanted.
- Cross-workspace ack-back over the gateway: deferred (transport).

## Related

- issue #299 (read-receipt half); #311 (delivery half), #296 (triage receipts),
  #300 (the fleet-query pattern). `crates/bwoc-cli/src/{receipts,triage,main,help}.rs`.
