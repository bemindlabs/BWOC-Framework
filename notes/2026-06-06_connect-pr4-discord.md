# 2026-06-06 — Connector PR4: Discord (completes the subsystem)

Final connector platform. Discord joins Telegram; the literal task
("Telegram/Discord chat plugins, DM + group") is now complete.

## What changed

- **`bwoc-connect::discord`** — `DiscordTransport` implementing the shared
  `Transport`:
  - Discord has **no long-poll**; a background task (`gateway_loop`) owns the
    Gateway websocket — connect → HELLO (heartbeat interval) → IDENTIFY (intents
    `GUILD_MESSAGES|DIRECT_MESSAGES|MESSAGE_CONTENT` = 37376) → heartbeat (op 1,
    last seq) ∥ read dispatches — pushing `MESSAGE_CREATE` into an mpsc.
    `poll()` drains the queue (1s liveness window); `send()` is REST
    (`POST /channels/{id}/messages`, `Bot <token>`). Reconnects on drop/op-7/op-9
    until the transport is dropped.
  - **`parse_message_create`** (pure, tested): `guild_id` present ⇒ group room;
    mention-gating via Discord's structured `mentions[]` (more robust than
    substring); bot authors + empty content skipped (no loops). Snowflake ids
    parse string→i64 (fit < 2^63).
- **`bwoc-connect` main** generalized to `<telegram|discord>`: per-platform
  config file + token env (`DISCORD_BOT_TOKEN`) + transport, then the **same**
  `run_bridge` / factories / group-binding path. `ConnectorConfig` (alias of the
  platform-agnostic config) is shared.
- **`bwoc-agent`** supervises `connectors/discord.toml` too (added to `KNOWN`).

## Decisions

- **Reuse the bridge wholesale.** `Transport`/`AgentSession` seams meant Discord
  was a new `Transport` + a parse fn — allow-list, mention-gate, DM/group
  routing, team `chat.jsonl`, session reuse, dead-session respawn all carried
  over untouched.
- **Background-task gateway, queue-drained `poll`.** Bridges Discord's push
  model to the poll-based `Transport` without changing the trait or `run_bridge`.
- **Gateway is the integration-untested edge** (no Discord token in CI), like
  the Telegram reqwest edge — `parse_message_create` carries the unit tests; the
  live loop is built to the verified v10 protocol and may need live iteration
  (RESUME/session-resume is a future refinement; PR4 reconnects with a fresh
  IDENTIFY).
- **rustls-only** `tokio-tungstenite` features (`connect`,
  `rustls-tls-webpki-roots`) — no native TLS, matches reqwest; quarantined in
  `bwoc-connect` (verified absent from cli/agent/core).

## Tests

`discord::parse_message_create`: DM, guild ± bot mention, skip bot-author + empty.
15 bwoc-connect tests pass; fmt + clippy `-D warnings` clean; `cargo check
--workspace` green; quarantine verified.

## Status / next

- **PR4 done → Telegram + Discord, DM + group, daemon-supervised: the task is
  complete.** Remaining hardening (separate small PRs): keyring token resolution
  (the cross-platform-dep piece), `bwoc status` connector-health, gateway RESUME,
  per-message broadcast UX.

## Related

- `crates/bwoc-connect/src/{discord,main,lib}.rs`, `crates/bwoc-agent/src/connectors.rs`
- `notes/2026-06-06_connect-pr{1,2,3}-*.md`, `..._chat-connectors-design.md`
