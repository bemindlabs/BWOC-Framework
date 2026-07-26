# 2026-07-26 — Harness LLM Sprint 3: extended thinking (Anthropic, non-streaming)

Closes gap #4. Enables adaptive extended thinking on the native Claude path and — critically — **preserves + replays** the returned thinking blocks so the agentic tool loop doesn't 400.

## Why replay is mandatory (not optional)

The Messages API requires the `thinking` block that precedes a `tool_use` to be present in that assistant turn when the following `tool_result` is sent. Drop it and the next turn returns 400. So "enable thinking" and "replay thinking" are one feature — a half-measure is a footgun. The autonomous agent loop is tool-heavy, so this is the load-bearing path.

## What changed

- **`ChatMessage.thinking_blocks: Option<Vec<Value>>`** (`provider/types.rs`) — raw `thinking`/`redacted_thinking` blocks (incl. `signature`), preserved verbatim. `#[serde(default, skip_serializing_if)]` → backward-compatible + round-trips on disk (checkpoint/session). Builder `with_thinking_blocks`. The OpenAI-compat egress DTO already lists only role/content/tool_calls/id/name, so thinking blocks are **automatically stripped** off-Anthropic — no leak.
- **`AnthropicClient`** — new `thinking: bool` field + `with_thinking(bool)` (default off). `complete()` sends `thinking:{type:"adaptive"}` when on; `parse_completion` collects thinking blocks onto the assistant message; `build_anthropic_body` **replays** them as the FIRST content blocks of an assistant turn. `stream()` is **guarded** (no thinking) — streaming thinking-block preservation is a follow-up; enabling it there without replay would 400.
- **Manifest `thinking: Option<bool>`** — opt-in (`None ≡ off`), threaded through `build_provider`'s run/eval/chat load sites as `m.thinking.unwrap_or(false)`.
- **Docs (EN + TH parity)** + template `config.manifest.json` document `thinking` and the replay/streaming-guard behaviour.

## Decisions

- **Scope: non-streaming trio + guard streaming** (user's call). The autonomous loop (`config.stream = false`) is the replay-critical tool path and gets the full trio. Streaming (interactive chat) is guarded off to avoid the 400 footgun; streaming thinking-preservation is an explicit follow-up. De-scoped thinking-display-in-UI (reasoning still happens + replays, just isn't surfaced in the token stream).
- **OpenAI-compat "bonus" is already covered** — `reasoning_content` is an unknown field serde ignores (no crash), and reasoning-token accounting landed in Sprint 1 (`Usage::reasoning_tokens()`). OpenAI-compat does not require thinking replay, so no ChatMessage change is needed there.
- **Opt-in default off** — thinking changes behaviour + cost and only helps on supporting models; existing agents are untouched.
- **Verification limit (honest):** unit tests cover build/parse/**replay** round-trip (thinking block preserved with signature, re-emitted first, before text/tool_use). The live Anthropic thinking round-trip is **not** verifiable on this Mac without an API key/cost — flagged, not run.

## Verification

macOS: `cargo fmt` + `clippy --workspace` clean; **workspace tests 1699 passed / 0 failed** (new: `thinking_config_and_builder`, `thinking_blocks_round_trip_and_replay`, manifest `thinking` serde). Manifest JSON valid; EN/TH parity.

## Status / deferred

Remaining roadmap: streaming thinking-block preservation (this sprint's follow-up), #5 structured output (parked — no in-harness consumer; tool-calling covers agent structure), #7 MCP modernize, #8 multimodal, streaming cache-token capture (Sprint 1 follow-up).

## Related

- Sprint 1 (#380: effort/max_tokens/usage) · Sprint 2 (#381: prompt caching) · `provider/{types,anthropic}.rs` · `manifest.rs` · claude-api skill (adaptive thinking, budget_tokens removed on 4.7+, replay rules).
