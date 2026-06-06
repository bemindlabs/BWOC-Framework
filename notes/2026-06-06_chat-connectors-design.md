# 2026-06-06 — Design: Telegram/Discord chat connectors (DM + group)

Requested by the architect: chat with an agent from Telegram/Discord, both DM
and **group rooms shared with other humans**. Decisions already taken: runs
under the **bwoc-agent daemon**; groups map to the **HV3-3a team `chat.jsonl`**.
Design note only — no code in this change.

## Architecture

```
Telegram API ──long-poll──┐                       ┌─ bwoc-harness --chat (DM A)
Discord gateway ──ws──────┤   bwoc-connect        ├─ bwoc-harness --chat (DM B)
                          ├──(new, quarantined ───┤
                          │   crate/binary)       └─ team chat.jsonl ⇄ group room
bwoc-agent --serve ───────┘   spawn + supervise (PR3)
```

(Diagram shows the **end-state**. In PR1/PR2 `bwoc-connect` is launched
directly — `bwoc-connect telegram --agent <dir>` — with no `bwoc-agent`
changes; daemon spawn + restart-supervision lands in **PR3**.)

- **`bwoc-connect`** — a NEW crate/binary holding the heavy deps (HTTP,
  websocket). `bwoc-agent` stays lean (dep-quarantine HARD RULE) and honours
  the "on the daemon" decision **operationally** *(from PR3)*: the daemon spawns
  `bwoc-connect <platform> --agent <dir>` when the agent declares a connector,
  and restarts it on crash — exactly the `bwoc-harness` subprocess pattern.
  Until then (PR1/PR2) it runs standalone, which also keeps it independently
  testable.
- **The bridge is just another chat frontend.** For DMs it spawns/holds a
  `bwoc-harness --chat` subprocess per conversation and speaks the existing
  `bwoc_core::chat_proto` JSON-lines — the same contract `bwoc chat --tui`
  uses. No protocol invention; streaming/permissions/compaction come free.
- **Groups ride HV3-3a.** A platform group binds to a Saṅgha team: human
  messages append to the team's `chat.jsonl` as `TeamChatMessage{from:
  "tg:<user>"}`; the agent's session joins via `--team-chat` and already
  injects peers + broadcasts replies; the bridge relays agent replies (and
  `TeamMessage` events) back to the room. Multi-human is therefore the
  *existing* team-chat semantics, not a new subsystem.

## Security (Sīla — the load-bearing part)

1. **Tokens** via the existing `CredentialBroker` convention (see
   `bwoc-harness::tools::auth`): an OS-keyring entry
   (`keyring_service = "bwoc/telegram"`, `keyring_account = <agentId>`) with an
   **env-var fallback** (`TELEGRAM_BOT_TOKEN`) for headless hosts. Never stored
   in config files — the config names the source, not the secret. Default on a
   server is the env-var fallback (the bemind host is headless); keyring is the
   hardened default where a keyring exists.
2. **Sender allow-list**: only listed platform user ids may reach an agent;
   unknown senders are ignored and logged. **Empty/absent ⇒ no one is allowed**
   (closed by default) — so the field must be populated to permit anyone. No
   public bots by default.
3. **No permission escalation**: the bridged session is non-TTY, so `ask`
   falls back to **deny** under the standard `.bwoc/harness-policy.toml`.
   Remote users can never approve tool calls.
4. **Mention-gating in groups** (default): the agent replies only when
   @mentioned, so it is a participant, not a firehose.
5. Per-sender rate limit + max message length; text-only in v1 (no media).

## Config (per agent)

`.bwoc/connectors/telegram.toml` (next to the agent's other config):

```toml
enabled    = true
# Token resolves via CredentialBroker: keyring (bwoc/telegram · <agentId>) →
# env TELEGRAM_BOT_TOKEN fallback. No secret in this file.
allowFrom  = [123456789]   # platform user ids; EMPTY/absent ⇒ nobody allowed
[group]
team        = "squad"      # binds platform groups → this Saṅgha team
mentionOnly = true
```

## Phasing (one PR each)

1. **PR1 — `bwoc-connect` + Telegram DM**: long-poll `getUpdates` →
   `chat_proto` bridge; allow-list; keyring token; config above.
2. **PR2 — Telegram group ⇄ team chat**: mention-gated, `chat.jsonl` binding.
3. **PR3 — daemon supervision**: `bwoc-agent --serve` spawns/restarts the
   bridge when a connector config exists; `bwoc status` shows connector health.
4. **PR4 — Discord** (gateway websocket + intents): same model, second
   platform proves the abstraction.
5. Later: media, slash-commands, multiple agents per room, webhook mode.

Telegram first: plain HTTPS long-poll (no websocket), simplest token model —
the cheapest end-to-end proof.

## Decisions (architect, 2026-06-06)

- **v1 = Telegram first** (PR1 DM → PR2 group → PR3 supervision); Discord is PR4.
- **Token: keyring default, env-var fallback documented** as the headless-server
  pattern — both supported from PR1 (matches the `CredentialBroker` convention
  above).

## Related

- `notes/2026-06-06_hv3-3a-team-chat-broadcast.md` (group-room substrate)
- `crates/bwoc-tui` (reference `chat_proto` frontend the bridge mirrors)
- bwoc-agent `--serve` daemon (supervision host)
