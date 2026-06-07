# 2026-06-07 — Connector reply streaming (in-place edits)

Telegram + Discord replies now stream live instead of arriving as one block on
turn end. Asked for after shipping the subsystem; isolated to the bridge — no
`chat_proto` changes.

## What changed

- **`Transport`**: `send` returns the new message id (`i64`); new `edit(chat_id,
  message_id, text)`. Telegram → `editMessageText` (treats "message is not
  modified" as a no-op success); Discord → `PATCH …/messages/{id}`. Discord
  `send` parses the snowflake `id` (string → i64).
- **`AgentSession::ask_streamed(text, sink)`** — default delegates to `ask`
  (single send, non-streaming sessions unchanged); `HarnessSession` overrides it
  to accumulate `chat_proto` `Token` deltas and push the running reply to the
  sink. The terminal `Message` is still the canonical returned text.
- **`ReplyStream` + `PlatformStream`** (bridge): first non-empty push **sends** a
  message; later pushes **edit** it in place, debounced to `EDIT_INTERVAL` (1s);
  `finish` guarantees the complete reply is shown (final edit, or a single send
  if nothing streamed). `serve_turn` drives it.

## Decisions

- **Send-once-then-edit, debounced 1s.** Telegram allows ~1 edit/sec/chat,
  Discord ~5/5s/channel; 1s is clear of both. The final edit always fires, so a
  fast/short reply is never throttled away. (Mattaññutā — don't hammer the API
  to paint every token.)
- **Push accumulated text, not deltas.** The platform `edit` replaces the whole
  message, so the sink wants the full running string; deltas would force the
  sink to re-accumulate. The session accumulates once.
- **Debounce in the sink, not the session.** Rate limits are platform-specific;
  the session just emits, `PlatformStream` decides when to actually edit. Keeps
  `HarnessSession` platform-agnostic and the policy unit-testable (inject
  `min_interval = 0`).
- **Backwards compatible.** `ask_streamed`'s default = old behaviour, so
  `EchoSession` and every non-streaming impl need no change; only `send`'s return
  type rippled (mock + the two transports).

## Tests

`PlatformStream`: placeholder→edits (interval 0), no-tokens→single send,
blank-push skip; plus an end-to-end `run_bridge` streaming test (placeholder sent
+ final edit carries the full reply, intermediate edit debounced). 19
bwoc-connect tests; fmt + clippy -D warnings clean. The live edit calls
(editMessageText / Discord PATCH) stay eyeball-reviewed like the other network
edges — no bot/token in CI.

## Status / deferred

- Streaming done for both platforms. Still deferred: Discord gateway RESUME;
  long replies over the platform length cap (Telegram 4096) — a pre-existing
  limitation, not introduced here (would need message-splitting).

## Related

- `crates/bwoc-connect/src/{lib,session,telegram,discord}.rs`
- `notes/2026-06-07_connect-subsystem-complete.md`
