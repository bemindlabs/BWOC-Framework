# 2026-08-07 — Durable offline delivery: outbox spool + `bwoc outbox flush`

A `bwoc send` to a remote peer that is offline no longer loses the message. The
signed envelope is spooled to a durable per-peer queue and retried later, instead
of relying on the gateway's in-memory park (which evaporates on restart / if the
peer never reconnects — the failure that forced a manual `ssh … bwoc send` to
reach an offline agent earlier this session).

## What changed

- **`bwoc_core::outbox`** — the spool store at `<workspace>/.bwoc/outbox/<recipientId>.jsonl`:
  `spool` (dedup by messageId), `read_spooled`, `rewrite` (drops delivered, removes empty file), `list_pending`. Mirrors `inbox`'s no-lock, dedup-on-append stance.
- **`bwoc send`** now spools on a *remote* soft failure (gateway relay / MQTT publish) → reports `Spooled` (exit 0, not lost). Hard errors (local I/O, unsigned gateway, missing sibling binary) still propagate.
- **`bwoc outbox` / `bwoc outbox list`** — pending counts per peer.
- **`bwoc outbox flush [--peer <id>]`** — replays each spooled envelope **verbatim** (same messageId + signature), so the recipient's inbox dedup makes at-least-once retry effectively-once. Delivered → dropped; still-offline → kept; hard error → kept + surfaced.
- `.bwoc/outbox/` added to the `bwoc init` gitignore template (transient per-machine delivery state, not an audit trail — unlike `inbox.jsonl`, which stays tracked).

## Decisions

- **Sender-side spool, not a gateway change.** The gateway is the external
  `bemindlabs/bwoc-gateway` repo; durability we own belongs on the sender. Spool
  + flush gives at-least-once without touching the relay.
- **Refactor `send()` into `resolve_target_for` + `deliver`.** Flush must replay
  the *stored* envelope over the same transport without re-signing (a new
  messageId/sig would break dedup + verification). Extracting the transport
  dispatch (`deliver`) and the routing (`resolve_target_for`) lets `redeliver`
  reuse them — one delivery path, no duplication (Mattaññutā).
- **Spool only remote relay failures** (`is_spoolable` = GatewayRelay | MqttPublish).
  A local inbox write can't be "offline"; a missing sibling binary or unsigned
  gateway is a setup error the operator must see now, not silently queue.
- **`Delivered::Spooled` as a success variant**, not an error — the message is
  durably retained, so exit 0 is honest; broadcast counts it as "not-live".
- **Flush keeps hard-errored lines** rather than dropping them — nothing is lost;
  the operator fixes the route/key and re-flushes.

## Not in scope (deferred)

- **Auto-flush on reconnect** — needs presence/liveness (the third coordination
  gap). For now flush is manual / cron-able. Worth wiring once `bwoc fleet
  status` grows real heartbeat presence.
- **Concurrency lock** — spool/flush are no-lock like `inbox` (a workspace's own
  sends are effectively serial); a `flock` is a later hardening.

## Tests

- `bwoc-core`: `outbox` — spool dedup, rewrite drop/remove-empty, list counts.
- `bwoc-cli`: `redeliver_replays_a_spooled_line_to_a_local_inbox` (parse → resolve
  → deliver + dedup), `is_spoolable_only_for_remote_relay_failures`,
  `outbox_cmd::flush_delivers_a_local_target_and_drains_the_spool`.
- E2E smoke (stub `bwoc-gateway-send` exiting non-zero, then zero): send → spooled
  → `outbox list` → flush stays queued while offline → flush drains when the peer
  returns. 855 cli + 137 core tests green; fmt + clippy (workspace + redteam) clean.

## Related

- `crates/bwoc-core/src/outbox.rs`, `crates/bwoc-cli/src/send.rs`, `crates/bwoc-cli/src/outbox_cmd.rs`, `crates/bwoc-cli/src/main.rs`, `crates/bwoc-cli/src/init.rs`
- `modules/agent-template/interconnect/messaging.md` §Durable offline delivery
- Prior: [[2026-08-07_send-broadcast]] (the sibling fan-out gap; this closes the "messages lost silently" gap it flagged)
