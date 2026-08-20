# bwoc-connect

Chat connectors that bridge everyday messaging platforms to a [BWOC](../../README.md) agent.

A connector is just another chat frontend: each inbound message becomes one user turn for a `bwoc-harness --chat` subprocess (the same `bwoc_core::chat_proto` that backs `bwoc chat --tui`), and the agent's reply is relayed back. This crate sits on the network side of the **dep-quarantine** line: `reqwest`, `tokio-tungstenite` (the Discord gateway — this crate is its only user in the workspace), `axum` (shared with `bwoc-a2a`), and `rusqlite` (shared with `bwoc-deep-memory`, macOS-only here) are linked from leaf crates like this one and never from [`bwoc-cli`](../bwoc-cli/), [`bwoc-agent`](../bwoc-agent/), or [`bwoc-core`](../bwoc-core/), which stay lean. Its only in-workspace crate dependency is `bwoc-core`. Full operator guide: [`CONNECTORS.en.md`](../../docs/en/CONNECTORS.en.md).

Four platforms are supported — no others:

| Platform | Receive | Send | Streaming | Runs on |
|---|---|---|---|---|
| Telegram | long-poll `getUpdates` | `sendMessage` | edit-in-place | anywhere |
| Discord | gateway websocket | REST | edit-in-place | anywhere |
| LINE | inbound webhook (axum) | reply token / push | no (no edit API) | needs a public URL |
| iMessage | read-only `chat.db` poll | `osascript` → Messages.app | no (no edit API) | **macOS only** |

## Scope

- **root (`lib.rs`)** — the testable core: `run_bridge` (poll → allow-list → per-`chat_id` session → reply), the `Transport` / `AgentSession` / `SessionFactory` / `ReplyStream` traits, `Incoming`, `ConnectorConfig` (alias of `TelegramConfig`), `GroupBridge`, `ConnectError`.
- **`telegram`** — `TelegramTransport` (reqwest long-poll) + the pure `parse_updates`.
- **`discord`** — `DiscordTransport`: gateway task (HELLO → IDENTIFY → heartbeat → dispatch) with RESUME and backoff, REST send; pure `parse_message_create`.
- **`line`** — `LineTransport`: axum webhook server with `verify_signature` (HMAC-SHA256 over `X-Line-Signature`), `parse_webhook`, `hash_id` (string LINE ids → the `i64` allow-list seam).
- **`imessage`** — macOS-only `ImessageTransport`: read-only SQLite poll of `~/Library/Messages/chat.db` plus `build_send_script` / `escape_applescript` / `decode_message_text` / `hash_id`. Hard-errors on other platforms.
- **`session`** — `HarnessSessionFactory` / `HarnessSession`: the `bwoc-harness --chat` subprocess edge. `ChatEvent::PermissionRequest` is **auto-denied** — a remote chat user can never approve a tool call.

## Usage

Config lives at `<agent-dir>/connectors/<platform>.toml` and is **closed by default** — an empty allow-list ignores everyone.

```toml
# agents/agent-<name>/connectors/telegram.toml
enabled = true
allow_from = [123456789]
```

Tokens resolve env-first, with the OS keyring (`bwoc/<platform>`, account = agent-dir basename) as a fallback on macOS/Windows; Linux is env-only.

```bash
TELEGRAM_BOT_TOKEN=... bwoc-connect telegram --agent agents/agent-<name>
```

Platform argument is one of `telegram`, `discord`, `line`, `imessage`; `--max-polls N` bounds the loop for smoke tests. Env vars: `TELEGRAM_BOT_TOKEN`, `DISCORD_BOT_TOKEN`, `LINE_CHANNEL_ACCESS_TOKEN` + `LINE_CHANNEL_SECRET`.

No other crate takes a Cargo dependency on this one — `bwoc-agent --serve` supervises the `bwoc-connect` *binary* as a subprocess, which is the point of the quarantine. So it has no `[workspace.dependencies]` entry; take it by path:

```toml
[dependencies]
bwoc-connect = { path = "../bwoc-connect" }
```

```rust
use bwoc_connect::ConnectorConfig;

let cfg = ConnectorConfig::parse("enabled = true\nallow_from = [42]")?;
assert!(cfg.is_allowed(42));
```

## Status

All four connectors are implemented and shipped. DM relay, group rooms bound to a Saṅgha team's shared `chat.jsonl` (with mention-gating), streaming on the edit-capable platforms, and closed-by-default allow-lists all work today. The routing core carries the unit tests via mock transports; the live network loops (Discord gateway, LINE webhook, iMessage AppleScript) have no CI credentials and are eyeball-reviewed.

## License

[MIT](../../LICENSE).
