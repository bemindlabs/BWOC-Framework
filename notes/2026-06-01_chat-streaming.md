# 2026-06-01 — Stream token deltas from the `--chat` driver

The interactive `bwoc-harness --chat` driver now streams the assistant turn token-by-token instead of emitting the whole reply at once, so the frontends (`bwoc chat --tui`, the `bwoc-chat` desktop app) render replies live.

## What changed

- New `stream_turn` helper in `chat_session.rs`: calls `provider.stream(...)`, emits a `chat_proto::ChatEvent::Token { text }` for every content delta as it arrives, and accumulates the full message (content + tool_calls) + usage — the streaming analogue of `provider.complete()`. Mirrors `agent_loop::stream_and_accumulate`, adding the per-delta emit.
- `run_turn` now calls `stream_turn` instead of `provider.complete()`. The final `Message` + `TurnEnd` and the tool/permission flow are unchanged, so non-streaming frontends still work (the `Message` carries the full text).
- The test `MockProvider::stream` was a stub returning an error; it now converts the next queued completion into a single `StreamChunk` (content + tool-call deltas + usage), so the existing tests exercise the streaming path. `user_message_round_trips_to_final` updated to assert the streamed `Token`s reconstruct the content.

## Bugs surfaced and fixed

While rewriting the provider call I found the `emit_turn_end`-on-error fix from #160's review had **regressed** — the error paths returned without a `TurnEnd`, which would hang a frontend waiting on the turn delimiter. This was lost during the parallel-build integration (the same churn that dropped the Windows `.exe` fix earlier). Restored the helper + the calls.

## Decisions

- **Always stream; the final `Message` stays.** A frontend can render `Token`s live and then settle on `Message` (identical text) — no protocol change, non-streaming clients unaffected.
- **No new config knob.** The chat driver streams unconditionally (interactive chat always wants it); the batch `run_loop` keeps its own `--stream` flag.

## Status / deferred

Shipped on `feat/chat-streaming`. Usage counts depend on the provider honoring `stream_options.include_usage` (ollama may omit it → `TurnEnd` shows 0/0; harmless).

## Related (links)

- Protocol: `bwoc_core::chat_proto` (#160). Frontends: `bwoc chat --tui` (#163), `projects/bwoc-chat` (desktop).
- Reused: `agent_loop::stream_and_accumulate` (the batch streaming accumulator).
