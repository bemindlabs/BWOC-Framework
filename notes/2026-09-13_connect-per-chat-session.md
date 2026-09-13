# 2026-09-13 — Connect: per-chat session files + Platform provenance

Phase 0 of bwoc-bot. Two defects in `bwoc-connect`, confirmed against the code before fixing (yoniso manasikāra).

## Bugs surfaced and fixed

1. **Shared session file.** `run_bridge` holds one `AgentSession` per `chat_id`, but every one spawned `bwoc-harness --chat --workdir <agent>` and the harness always persisted to `<workdir>/.bwoc/chat-session.json`. Two chats reloaded and overwrote the same history, so context leaked between chats and history got clobbered.
2. **Lost provenance.** Bridged turns were sent with `Principal::default()` (`Unknown`). That failed closed, but the sender was never recorded.

## What changed

- `bwoc-harness`: new `--session-file <path>` → `ChatConfig.session_path` (`None` = the old default path). `session_path_for` resolves it for both the driver and the Tier-2 mine.
- `bwoc-connect`: `SessionFactory::create(chat_id)` and `AgentSession::ask{,_streamed}(text, from_user_id, ..)`. `HarnessSessionFactory::new(agent_dir, platform)` passes `--session-file <agent>/.bwoc/chat-sessions/<platform>-<chat_id>.json` and tags each turn `Principal::Platform { platform, user_id }`.
- Filename segments stay as-is when plain (`[A-Za-z0-9_-]`, ≤ 64). Anything else becomes `h.<sha256[..16] hex>`. A plain segment can't contain `.`, so a hashed name never collides with one, and no id can escape the sessions dir. `sha2` was already a dependency (LINE HMAC).

## Decisions

- No on-disk format change. The TUI default path and file shape are untouched (3.0 compatibility contract). Existing single-file connector history is **not** migrated: it belonged to an unknown mix of chats, so assigning it to one chat would re-create the leak.
- `Platform` stays Untrusted. `trust()` elevates only `LocalOperator` / `SelfAgent`, and a test pins this for bridged turns.

## Alternatives considered

- Env var instead of a flag: rejected. A flag is explicit in `ps` output and matches `--team-chat`.
- One workdir per chat: rejected. That's a much larger change and would also break memory/policy lookup.
