# 2026-07-26 — Harness LLM: streaming thinking-block preservation + cache-token capture

Closes the Sprint 3 follow-up (streaming thinking guard) and the Sprint 1 follow-up (streaming cache-token capture) in one PR — both live in the Anthropic SSE translator, so they share the same refactor.

## Why together

Sprint 3 shipped extended thinking on the **non-streaming** Claude path only; `stream()` was guarded to send `thinking:false` because dropping the returned thinking block on the next tool turn 400s the Messages API. Enabling streaming thinking therefore *requires* reassembling the block (with signature) from the SSE deltas — the same event loop that already carries token usage. Cache-token capture (Sprint 1) also arrives at `message_start`/`message_delta`, so both follow-ups fold into the same state struct.

## What changed

- **`translate_sse_event(ev, &mut input_tokens)` → `translate_sse_event(ev, &mut SseState)`** (`provider/anthropic.rs`). `SseState` carries `input_tokens`, `cache_read`, `cache_creation`, and the in-flight `ThinkingAccum` (index, redacted flag, thinking/signature/data). `message_start` now captures `cache_read_input_tokens` + `cache_creation_input_tokens`; `message_delta` emits them on `Usage`. A `thinking`/`redacted_thinking` block accumulates across `thinking_delta`/`signature_delta` and finalizes at `content_block_stop` into a carrier `StreamChunk` (empty `choices`, `usage: None`) whose new `thinking_block` field is the **exact** Value shape `parse_completion` produces.
- **`StreamChunk.thinking_block: Option<Value>`** (`provider/types.rs`) — serde `default` + `skip_serializing_if`, so it's a pure additive carrier that off-Anthropic paths never populate.
- **`stream_and_accumulate`** (`agent_loop/execute.rs`) collects `chunk.thinking_block` in stream order and attaches them to the accumulated assistant message via `with_thinking_blocks` — the same field the non-streaming path fills, so downstream replay in `build_anthropic_body` is unchanged and identical across both paths.
- **`AnthropicClient::stream()`** — guard removed; it now passes `self.thinking` like `complete()`.
- **Docs (EN + TH parity)** + template `config.manifest.json` updated: thinking now applies to streaming too, with the SSE-reassembly note.

## Decisions

- **Carrier chunk, not a new stream event type.** Reusing `StreamChunk` (with empty choices) keeps the `Stream<Item = StreamChunk>` contract intact — the accumulator already tolerates usage-only chunks with empty choices, so thinking-only chunks slot in the same way. No enum churn across the provider trait.
- **`into_block()` mirrors `parse_completion` verbatim.** The replay path (`build_anthropic_body`) is shape-sensitive; producing the identical `{type,thinking,signature}` / `{type,redacted_thinking:data}` JSON means streaming and non-streaming feed the exact same replay code. A unit test pins the shape.
- **Index guard on `content_block_stop`.** Only finalize the accumulator when the stop index matches the started thinking block; a stop for a tool_use/text block leaves state untouched (restored via `take()` round-trip). Blocks are sequential in practice, but this keeps a stray stop from emitting an empty block.

## Verification (honest)

macOS: fmt + clippy (`--workspace` and `-p bwoc-harness --features test-redteam`) clean with `-D warnings`; **workspace tests pass, 0 failed**. New unit tests: `sse_message_start_captures_cache_tokens`, `sse_thinking_block_round_trips_for_replay` (start→deltas→signature→stop → carrier block with reassembled text+signature, state cleared, no token deltas), `sse_redacted_thinking_block_preserves_data`. The live Anthropic **streaming** thinking round-trip is **not** verifiable on this Mac (no API key/cost) — flagged, not run; same posture accepted in Sprint 3.

## Status / deferred

Remaining roadmap (from "go next all"): #7 MCP modernize (protocol bump + HTTP/SSE transport — LARGE), #8 multimodal input (LARGE). #5 structured output stays parked (no in-harness consumer). Streaming/non-streaming thinking + caching + usage are now at parity.

## Related

- Sprint 1 (#380 usage/effort/max_tokens) · Sprint 2 (#381 prompt caching) · Sprint 3 (#382 extended thinking, non-streaming) · `provider/{anthropic,types}.rs` · `agent_loop/execute.rs` · claude-api skill (adaptive thinking + replay rules).
