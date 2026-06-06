# 2026-06-06 — Connector PR2: Telegram group ⇄ team chat

Second build of the connector subsystem (PR1: DM). Group/supergroup rooms now
bridge to a Saṅgha team's shared `chat.jsonl` — the genuinely multi-human piece,
riding HV3-3a end to end.

## What changed

- **`Incoming`** gains `is_group` + `mentions_bot`. `telegram::parse_updates`
  now keeps group/supergroup messages (skips channels), classifies the kind from
  `chat.type`, and sets `mentions_bot` via a case-insensitive `@<username>`
  match. `TelegramTransport::resolve_identity()` (`getMe`) caches the bot
  `@username` at startup for that match.
- **`run_bridge`** takes `group: Option<GroupBridge>` (`{ factory, chat_log }`)
  alongside the DM factory. Per allow-listed group message:
  - **mention (or `mention_only = false`)** → served by the group's
    `--team-chat` session (`serve_turn`); the session injects the room's unseen
    peer messages (HV3-3a) and broadcasts its reply; the reply is sent back.
  - **non-mention** → appended to the team `chat.jsonl` as peer context
    (`TeamChatMessage{from:"tg:<user>"}`, atomic O_APPEND) — no reply.
  - **no team binding** → ignored.
  DM and group paths share `serve_turn` (spawn-or-reuse, reply, dead-session
  respawn).
- **`HarnessSessionFactory::with_team_chat(path)`** spawns sessions with
  `--team-chat <path>`; `main` builds a DM factory + a group factory and resolves
  the team `chat.jsonl` (`<workspace>/.bwoc/teams/<team>/chat.jsonl`) by walking
  up from the agent dir to `.bwoc/workspace.toml`.

## Decisions

- **Mention message = the agent's user turn; non-mention = peer context.** Avoids
  double-exposing the trigger message (no append + user-turn for the same line).
  The agent sees other humans' chatter via `--team-chat` injection and the
  mention as its direct prompt. (The exact mention text isn't re-logged to
  `chat.jsonl`; the agent's reply is, via HV3-3a auto-broadcast — acceptable for
  PR2, refine later if other team agents need the verbatim human line.)
- **Closed posture carries over**: the allow-list gates *who* may reach the
  agent in a group too; non-allow-listed members are ignored entirely (not even
  logged) — Sīla over completeness. Broadening to "log all, trigger allow-listed"
  is a later config knob.
- **Mention via `@username` substring** (resolved by `getMe`), not entity
  offsets — simplest robust form; `text_mention` entities are a later refinement.

## Tests

12 total (was 7): `parse_updates` DM/group classification + channel skip +
mention flag; `mentions` case-insensitivity + `None`; group mention → served by
the group factory + reply; group non-mention → peer line in `chat.jsonl`, no
reply, no session; group with no binding → ignored. fmt + clippy `-D warnings`
clean; the live reqwest/subprocess edges stay eyeball-reviewed (no bot/model in CI).

## Status / next

- PR2 done. **PR3**: `bwoc-agent --serve` spawns/supervises `bwoc-connect` from
  an agent's connector config + keyring token resolution + `bwoc status`
  connector health. **PR4**: Discord (gateway). Then media / slash-commands /
  multi-group.

## Related

- `crates/bwoc-connect/src/{lib,telegram,session,main}.rs`
- `notes/2026-06-06_connect-pr1-telegram-dm.md`, `notes/2026-06-06_hv3-3a-team-chat-broadcast.md`
