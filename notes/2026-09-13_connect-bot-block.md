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
- `SessionFactory::create(chat_id, public)`. For a public session, `HarnessSessionFactory` spawns the harness with `--workdir <agent>/.bwoc/public/<platform>-<chat_id>/` (Phase 0's sanitizer on both segments) and puts the session file inside that dir. It then sends `SetMode{"plan"}` and requires a `ModeChanged{"plan"}` ack (the `Restored` replay is skipped). Anything else fails session creation. Allow-listed sessions are unchanged.
- The public workdir is prepared on every spawn:
  - `AGENTS.md` is **copied** in, falling back to `CLAUDE.md` (the harness's load order), always written as `AGENTS.md`.
  - `config.manifest.json` is copied with `deepMemoryCmd` removed.
  - A file is re-copied when its source is newer.
  - Nothing else goes in: no memories, connectors, sessions or skills.
- Harness (`fix(harness)` commit): `ToolContext::resolve_path` now also canonicalizes the target's deepest existing ancestor and requires it inside the canonical workdir. The check is `sandbox::is_confined`, split out of `confine_path`. Before, a symlink inside the workdir pointing outside escaped every confined file tool, and `--chat` has no Landlock jail to catch it. `memory_read`/`memory_write` use the same check, and `grep` skips escaping links. `--unrestricted` is unchanged.
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

- Read exposure closed in review: a public session originally ran against the agent dir. It now gets a separate workdir plus symlink-safe confinement.
- `deepMemoryCmd` is stripped from the public manifest copy. Copying it verbatim would have let the wake-up inject the agent's deep memory into a stranger's prompt, and let the session-end mine write the stranger's chat into that memory. This is a deliberate narrowing of "copy the manifest".
- No per-turn token cap for `--chat` remains a residual.
- A stranger's Telegram `/cmd@bot` in a group isn't detected as a mention (`mentions()` needs a non-username byte before `@`), so it's dropped; DMs work.

## Related

- `notes/2026-09-13_connect-per-chat-session.md` (Phase 0)
- `docs/en/CONNECTORS.en.md`, `docs/en/THREAT-MODEL.en.md`
