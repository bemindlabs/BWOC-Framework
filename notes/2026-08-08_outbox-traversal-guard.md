# 2026-08-08 — Security: guard outbox spool against path traversal

The durable outbox (#418) keys its spool file by `recipient_id`
(`.bwoc/outbox/<recipient_id>.jsonl`). For a **remote** (gateway/MQTT) peer that
id is the raw `--to` argument (only `agent-`-prefixed by `canonicalize`), so
`bwoc send 'agent-x/../../evil'` to an offline remote route would spool outside
the outbox dir. The `--team` sibling was already guarded (`is_safe_segment`);
recipient ids were not.

## What changed
- `bwoc_core::outbox`: `spool` / `read_spooled` / `rewrite` now `reject_unsafe`
  a recipient id that isn't a single normal path segment (no separators, no
  `..`/`.`, not absolute) → `io::ErrorKind::InvalidInput`. `send()` propagates it
  as a hard error (the traversal attempt fails loudly, nothing escapes).
- Test: `spool_rejects_traversal_recipient_ids` (spool/read/rewrite all refuse;
  no stray file escapes).

## Note
Local delivery is unaffected — it resolves through the registry (ids are already
validated). Only the remote-relay-failure spool path took the raw id. Surfaced
by the multi-agent gap audit (§4 messaging-security, MED).
