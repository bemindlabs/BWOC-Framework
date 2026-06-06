# 2026-06-06 — Connector PR1: `bwoc-connect` + Telegram DM

First build of the chat-connector subsystem (design:
`notes/2026-06-06_chat-connectors-design.md`; decisions: Telegram-first, runs
under bwoc-agent daemon eventually, keyring-default tokens). PR1 delivers a
working **Telegram DM** bridge as a standalone binary; daemon supervision is PR3.

## What changed

- **New crate `crates/bwoc-connect`** (workspace member) — the dep-quarantine
  home for the network stack (`reqwest`, extra `tokio` features). Verified with
  `cargo tree` that `bwoc-cli` / `bwoc-core` pull in neither.
- **`lib.rs`** — the testable core:
  - `TelegramConfig` (`enabled`, `allow_from`, optional `[group]`) + closed-by-
    default `is_allowed` (empty allow-list ⇒ nobody).
  - `Transport` + `AgentSession` + `SessionFactory` seams; `Incoming` message.
  - `run_bridge(transport, factory, config, max_polls)` — poll → allow-list →
    per-`chat_id` session (DM continuity) → reply. Per-message errors logged and
    skipped; `max_polls` bounds it for tests.
- **`telegram.rs`** — `TelegramTransport` (reqwest): `getUpdates` long-poll +
  `sendMessage`. `parse_updates` (pure) keeps only text DMs, skips
  edits/stickers/anon — unit-tested.
- **`session.rs`** — `HarnessSession`/`HarnessSessionFactory`: spawns
  `bwoc-harness --chat --workdir <agent>` (model/endpoint from the manifest),
  speaks `chat_proto`, returns each turn's final `Message`. **Auto-denies**
  `PermissionRequest` (remote users can't approve tools).
- **`main.rs`** — `bwoc-connect telegram --agent <dir> [--max-polls N]`;
  hand-rolled args (no clap — keep the crate lean); config at
  `<agent>/connectors/telegram.toml`; token from `TELEGRAM_BOT_TOKEN`.

## Decisions

- **Standalone binary in PR1**, not yet daemon-managed: keeps it independently
  runnable + testable; `bwoc-agent --serve` spawn/supervision is PR3 (per the
  design's phasing).
- **Env token in PR1** (`TELEGRAM_BOT_TOKEN`) — the architect-sanctioned
  headless-server path. Keyring-default resolution lands with the
  CredentialBroker wiring (PR3), avoiding the `keyring` crate's cross-platform
  surface in this first slice.
- **Seams over live I/O for tests**: `Transport`/`AgentSession` are traits, so
  the routing/allow-list/offset/per-chat-reuse logic is fully unit-tested with
  mocks; the reqwest + subprocess adapters are the thin, eyeball-reviewed edges
  (no Telegram bot token or live model in CI).

## Tests

- `lib.rs`: config parse + closed allow-list, group block defaults, and three
  `run_bridge` tests (allow-listed → echoed reply; stranger → ignored, no
  session; two msgs same chat → one session reused).
- `telegram.rs`: `parse_updates` keeps text DMs / skips the rest; empty result.
- 7 tests pass; fmt + clippy `-D warnings` clean.

## Status / next

- PR1 done. **PR2**: Telegram group ⇄ team `chat.jsonl` (mention-gated). **PR3**:
  `bwoc-agent --serve` spawns/supervises connectors + keyring tokens + `bwoc
  status` connector health. **PR4**: Discord. Then media/slash-commands.
- Deferred discoverability: a `connectors/telegram.toml` example in the agent
  template + a `bwoc handbook` connectors note (land with PR3 when it's
  daemon-wired and user-facing).

## Related

- `crates/bwoc-connect/src/{lib,telegram,session,main}.rs`
- `notes/2026-06-06_chat-connectors-design.md`, `notes/2026-06-06_hv3-3a-team-chat-broadcast.md`
