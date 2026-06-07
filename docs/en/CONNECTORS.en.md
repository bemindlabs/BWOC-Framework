---
title: Connectors
parent: English
nav_order: 13
---

# Chat Connectors

**Chat connectors** let a human reach a BWOC agent from an everyday chat app —
**Telegram**, **Discord**, or **LINE** — in a private DM or a shared group room.
They are operator-facing infrastructure: the network code lives in one crate
(`bwoc-connect`) so the `bwoc` CLI, agent runtime, and core stay lean
(dep-quarantine).

> [!abstract] A connector is "just another chat frontend." Each inbound message becomes one user turn for a `bwoc-harness --chat` session (the same protocol as `bwoc chat --tui`), and the agent's reply is relayed back. Streaming, permission prompts, and compaction come for free from that shared protocol.

---

## The three platforms

| Platform | Receive | Send | Streaming | Runs on |
|---|---|---|---|---|
| **Telegram** | long-poll (`getUpdates`) | `sendMessage` | ✅ edit-in-place | anywhere |
| **Discord** | gateway websocket | REST `createMessage` | ✅ edit-in-place | anywhere |
| **LINE** | inbound **webhook** (HTTPS) | reply-token / push | ✗ (no edit API) | anywhere (needs a public URL) |

All three share the same routing core (`run_bridge`): allow-list filtering,
DM-vs-group handling, group→team bridging, and per-chat session reuse. Adding a
platform is a new `Transport` implementation, not new routing.

> [!note] **macOS-only platforms are not included.** iMessage has no server API (it requires driving Messages.app on a Mac); it is **spec'd but not built** — see the design note `notes/2026-06-07_imessage-connector-design.md`.

---

## Configuration

Each agent opts in with a per-platform file under its directory:
`agents/agent-<name>/connectors/<platform>.toml`.

```toml
# connectors/telegram.toml  (or discord.toml)
enabled    = true
allow_from = [123456789, 987654321]   # platform user ids; CLOSED BY DEFAULT

[group]                                # optional — bridge group rooms to a team
team         = "tianting"              # a Saṅgha team id
mention_only = true                    # reply only when the bot is @mentioned
```

```toml
# connectors/line.toml  — LINE ids are strings, so its allow-list lives here
enabled = true

[line]
allow_user_ids = ["U1234..."]          # LINE user ids; CLOSED BY DEFAULT
bind           = "0.0.0.0:8080"         # the inbound webhook server's address
path           = "/webhook"             # webhook path (put an HTTPS proxy in front)
```

> [!warning] **Closed by default.** An empty or absent allow-list permits **nobody** — there are no public bots. List the exact user ids that may reach the agent. Non-allow-listed senders are ignored entirely (Sīla over completeness).

### Tokens

Tokens are **never** stored in the config. They resolve, in order:

1. **OS keyring** (macOS / Windows) — service `bwoc/<platform>`, account = the
   agent directory's basename.
2. **Environment variable** — the documented headless-server path (and the only
   path on Linux, which has no keyring backend):
   - `TELEGRAM_BOT_TOKEN`
   - `DISCORD_BOT_TOKEN`
   - `LINE_CHANNEL_ACCESS_TOKEN` **and** `LINE_CHANNEL_SECRET` (the secret
     verifies the webhook's `X-Line-Signature`).

A missing or locked keyring is never fatal — it falls through to the env var.

---

## Running

The agent daemon spawns and supervises the connector — you don't run
`bwoc-connect` by hand:

```bash
bwoc-agent --serve        # in the agent directory; detects enabled connectors/*.toml,
                          # spawns the bridge, respawns it on crash, kills it on shutdown
bwoc status               # shows a "Connectors" line per running bridge (platform · state · pid)
```

The daemon supervises the `bwoc-connect` binary as a child — exactly the
`bwoc-harness` spawn pattern — so the network dependencies never enter the CLI /
agent / core build.

---

## Groups and teams

With a `[group] team = "<id>"` binding, group/supergroup rooms bridge to that
Saṅgha team's shared append-only `chat.jsonl` (the HV3-3a team-chat substrate):

- A message that **@mentions the bot** (or any message when `mention_only =
  false`) is served by a `--team-chat` agent session, which injects the room's
  recent peer messages and broadcasts its reply back to the room.
- A **non-mention** message is appended to the team chat as peer context (tagged
  `tg:`/`dc:`/`ln:<id>`) so the agent has the conversation when next addressed —
  no reply.
- A group message with **no** team binding is ignored.

---

## Streaming

Telegram and Discord **stream the reply live**: the bridge sends a placeholder on
the first token and edits it in place as the reply grows, debounced to ~1 edit/sec
(clear of both platforms' edit rate limits), with a guaranteed final edit showing
the complete text. LINE has no message-edit API, so it sends the reply **once** on
turn end. This is automatic — a transport advertises `supports_edit`.

---

## Security posture

- **Closed-by-default allow-list** gates who may reach the agent.
- The bridged harness session is **non-TTY**, so `ask`-mode tool calls fail safe
  to **deny**, and a `PermissionRequest` is auto-denied — a remote chat user can
  never approve a tool call.
- **LINE** webhooks are verified (`X-Line-Signature` = base64(HMAC-SHA256(channel
  secret, body)), constant-time); unsigned/forged requests are rejected.

---

## Limitations & deferred

- **One connector platform per agent daemon** (the first enabled config).
- Text only — no media/attachments yet.
- LINE replies that outlive the one-time reply token (~a slow agent turn) fall
  back to a **push** message, which counts against LINE's monthly quota; prompt
  replies stay free.
- **Discord gateway RESUME** is deferred — reconnects re-IDENTIFY (works, just
  not the lighter resume path).

## Related

- [[PLUGINS]] — framework plugins (a different extension axis)
- [[HARNESS]] — the `bwoc-harness --chat` session each connector drives
- `notes/2026-06-07_connect-subsystem-complete.md` and the per-platform notes
