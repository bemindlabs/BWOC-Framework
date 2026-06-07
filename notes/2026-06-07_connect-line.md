# 2026-06-07 — LINE connector (`bwoc-connect line`)

A fourth platform after Telegram/Discord — and the first **webhook-driven** one.
Highest practical value here (LINE is Thailand's dominant app) and it runs on the
Ubuntu deployment host, unlike iMessage.

## What changed

- **`bwoc-connect::line`** — `LineTransport` implementing the shared `Transport`:
  - **Receive = inbound webhook.** LINE has no long-poll; it POSTs events to an
    HTTPS endpoint. The transport runs a small **axum** server (one route): each
    POST is signature-verified (`X-Line-Signature` = base64(HMAC-SHA256(channel
    secret, raw body)); constant-time via `hmac::verify_slice`), `parse_webhook`
    turns it into events, which are queued to an mpsc; `poll()` drains the queue
    — the same shape as the Discord gateway→mpsc bridge.
  - **Send = reply-token-first, push-fallback.** LINE **reply** messages (the
    one-time `replyToken` from the event) are free + unlimited; **push** counts
    against the monthly quota. `send` uses a stored reply token when fresh
    (`REPLY_WINDOW_SECS = 55`) and pushes otherwise. No edit API →
    `supports_edit() = false`.
  - `parse_webhook` / `verify_signature` / `hash_id` are pure + unit-tested.
- **`PlatformStream` honours `Transport::supports_edit()`** (new, default true):
  when false (LINE), it streams nothing and `finish` sends the reply once. So the
  streaming work added for Telegram/Discord degrades cleanly on a non-editing
  platform — no broken half-streamed messages.
- **String ids without a framework change.** LINE ids (`U…`/`C…`/`R…`) are hashed
  to a stable `i64` (`hash_id` = first 8 bytes of SHA-256) so they ride the `i64`
  `Incoming`/allow-list seam. `[line].allow_user_ids` (strings) is hashed in
  `main` into `allow_from` with the same function — closed by default preserved.
- **`main`** `line` arm + **`bwoc-agent`** supervises `connectors/line.toml`.

## Decisions

- **Webhook inside the transport, queue-drained `poll`.** Keeps `run_bridge` and
  the `Transport` trait unchanged; the inbound server is just the LINE receive
  half (mirror of Discord's gateway task).
- **Hash string ids to i64 (option B from the iMessage spec), not the full
  string-id generalization (A).** Three platforms now want string ids
  (iMessage/LINE/future), so (A) is the eventual cleanup — but (A) collides with
  the in-flight streaming PR (it also touched the `Transport` signatures), so LINE
  ships self-contained via hashing now. Collision risk is cryptographically
  negligible for a handful of allow-listed ids. **Follow-up: do (A) and drop the
  hash.**
- **Reply-token window 55s.** Fast replies are free forever; a turn slower than
  the token's life falls to a push (quota) — logged honestly. Tunable later.
- **axum** for the one webhook route — it rides the hyper/tower stack reqwest
  already pulls in, so the marginal dep weight is small, and it stays quarantined
  in `bwoc-connect` (verified absent from cli/agent/core).

## Tests

`line::{signature_verifies_and_rejects_tampering, hash_id_is_stable_and_distinct,
parse_dm_text_event, parse_group_mention_and_non_mention,
skips_non_text_and_non_message_and_sourceless}` + a `PlatformStream`
non-editing-transport single-send test. 25 bwoc-connect tests; fmt + clippy -D
warnings clean; quarantine verified. Live webhook server + reply/push are the
integration-untested edge (no LINE channel in CI), same posture as the other
network edges.

## Status / deferred

- Done: DM + group⇄team, signature-verified webhook, reply/push, non-streaming.
- Deferred: the string-id generalization (A) to drop the hash; LINE loading
  animation as a "thinking" affordance; >5000-char message splitting; richer
  message types (the bridge is text-only). Also still: Discord gateway RESUME.

## Related

- `crates/bwoc-connect/src/{line,lib,main}.rs`, `crates/bwoc-agent/src/connectors.rs`
- `notes/2026-06-07_connect-subsystem-complete.md`, `notes/2026-06-07_imessage-connector-design.md` (the string-id decision)
