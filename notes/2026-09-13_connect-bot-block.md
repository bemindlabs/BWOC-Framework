# 2026-09-13 — Connect: `[bot]` block + limited public mode

Phase 1 of bwoc-bot, built on Phase 0 (per-chat session files, `Principal::Platform`). Connector TOML gains a `schema_version` marker and an optional `[bot]` table with fixed commands, rate/length caps, and a `public` opt-in. One agent = one bot; `AGENTS.md` is the persona.

## What changed

- `TelegramConfig` (shared by all platforms):
  - `schema_version: u32` (absent ⇒ 2). `parse` refuses a value above `CONNECTOR_SCHEMA_VERSION` and names `schema_version` in the error.
  - `bot: Option<BotConfig>`, with `commands` (BTreeMap), `rate_limit_per_min`, `max_input_chars`, `public`.
- `run_bridge` pipeline:
  - The gate decides allow-listed vs public. A public sender must DM or @mention; their other group chatter is dropped, never peer-logged.
  - Routing happens next: public senders always use the solo DM factory, never `--team-chat`.
  - Then `admit()` runs the rate cap, commands and length cap, in that order.
  - Finally `serve_turn`.
  - Sessions are keyed `(chat_id, public)`.
- `RateLimiter`: in-memory 60 s sliding window per sender. `check()` takes `now: Instant`, so tests don't sleep. One notice per over-limit episode; the notice re-arms once a slot frees. The map prunes idle senders past 4096 entries.
- `SessionFactory::create(chat_id, public)`. `HarnessSessionFactory` keys the file as `<platform>-<chat_id>-public.json` for public sessions. It then sends `SetMode{"plan"}` and requires a `ModeChanged{"plan"}` ack (the `Restored` replay is skipped). Anything else fails session creation.
- Docs: CONNECTORS and THREAT-MODEL (EN + TH).

## Decisions

- **Read-only seam = the harness's existing `plan` session mode; no harness change.** `chat_session` doesn't run the Layer-0 capability gate at all (only guardrails + permission). An untrusted chat turn can therefore write, and even run commands, whenever `harness-policy.toml` allows it. Plan mode is a fixed allow-list checked before the permission gate (`PLAN_READ_ONLY_TOOLS`), so policy can't widen it. Existing harness tests already cover the denials: `plan_mode_blocks_mutating_tool_without_prompt` for `write_file` at drive level, and the `plan_block("run_command")` unit assertions. Only the bridge writes harness stdin, and remote text always travels JSON-encoded inside `User`, so a sender can't toggle the mode back.
- **Caps apply to allow-listed senders too, once `[bot]` exists.** With no `[bot]`, nothing changes. `0` turns a cap off for allow-listed senders only; for public senders it means the default.
- **Order: rate cap → commands → length cap.** Rate comes first so `/help` spam is capped too. Caps apply only to messages that would be served, so an allow-listed member's peer-logged group chatter never draws a notice.
- **Public group @mentions go to a solo session**, even when the room has a team binding. Injecting the team's `chat.jsonl` would leak members' context to a stranger. A side effect: strangers can use the bot in a group that has no team binding.

## Deviations from the brief

- **No `SchemaVersion` type on this base.** `bwoc-core::schema`, `bwoc migrate`, and `docs/en/COMPATIBILITY.en.md` exist only on the unmerged `feat/v3-w1-schema-seam` (CURRENT = 3). Pulling that in would drag a 3.0 feature into this PR. The connector uses a local `u32` with the same TOML shape (`schema_version = N`, absent ⇒ 2, future ⇒ refuse). It becomes a one-line swap to `SchemaVersion` once v3 lands; at that point `CONNECTOR_SCHEMA_VERSION` must follow `CURRENT`, or v3-stamped files would be refused.
- **`bwoc migrate` does not stamp connector files.** The command doesn't exist on this base.
- **No per-turn token cap.** `--token-budget` feeds only the batch `run_loop` (`LoopConfig.max_tokens`); `ChatConfig` has no budget. Per the brief, no new budget system was built. Token-cost abuse is bounded by rate cap × senders and is recorded as a residual.

## Status / deferred

- Read exposure: public turns can `read_file` anything in the agent dir, including other chats' `chat-sessions/*.json`. Documented as a residual. A per-session read confinement would need a new harness seam.
- A stranger's Telegram `/cmd@bot` in a group isn't detected as a mention (`mentions()` needs a non-username byte before `@`), so it's dropped; DMs work.

## Related

- `notes/2026-09-13_connect-per-chat-session.md` (Phase 0)
- `docs/en/CONNECTORS.en.md`, `docs/en/THREAT-MODEL.en.md`
