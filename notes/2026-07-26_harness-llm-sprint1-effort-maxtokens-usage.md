# 2026-07-26 — Harness LLM Sprint 1: effort parity + max_tokens + usage accounting

First slice of the LLM-backend modernization (gap analysis → Sprint 1). Closed the three cheapest/highest-ROI gaps where BWOC's provider layer had fallen behind the 2026 landscape, all backend-neutral and low-risk.

## What changed

- **#1 Effort parity** — the manifest `reasoningEffort` now reaches the **native Claude path** too, not just OpenAI-compat. `AnthropicClient::with_reasoning_effort` emits `output_config.effort` (GA on Opus 4.6+/Sonnet 4.6+/Fable 5); OpenAI-compat continues to send `reasoning_effort`. Value space stays backend-specific/pass-through (operator sets a literal valid for their backend), so neutrality holds.
- **#2 `max_tokens` configurable** — new optional manifest field `maxTokens` (`crates/bwoc-core/src/manifest.rs`). Anthropic's hardcoded 8192 default is now overridable via `AnthropicClient::with_max_tokens` (default unchanged when unset → no behaviour change); OpenAI-compat sends `max_tokens` only when configured (`OllamaClient::with_max_tokens`), else provider default. Threaded through `build_provider` (+ its 3 manifest-load sites: run / eval / chat).
- **#6 Usage accounting** — `Usage` (`provider/types.rs`) gained the modern token fields that were previously parsed away: OpenAI-compat nested `prompt_tokens_details.cached_tokens` / `completion_tokens_details.reasoning_tokens` (serde-native), and Anthropic flat `cache_read_input_tokens` / `cache_creation_input_tokens` (set by `parse_usage`). Provider-agnostic accessors `Usage::cached_tokens()` / `reasoning_tokens()`. All `#[serde(default)]` + `Default` → backward-compatible; existing `Usage { … }` literals updated with `..Usage::default()`.
- **Docs (EN + TH parity):** `docs/{en,th}/HARNESS.md` + template `config.manifest.json` schema document `maxTokens` and the effort-reaches-Claude change.

## Decisions

- **Manifest field for `maxTokens`, not a CLI flag** — mirrors the existing `reasoningEffort` plumbing exactly (per-agent config, same 3 load sites), so it's consistent and cache/neutrality-clean. Default stays 8192 for Anthropic (conservative; operators raise it) rather than a blanket bump — zero behaviour change unless configured.
- **Effort is pass-through, not mapped** — the manifest carries the operator's literal; sending it only-when-set matches the pre-existing OpenAI-compat behaviour (a value the model rejects 400s, same as before). Keeps BWOC backend-neutral (no hardcoded effort mapping table).
- **Streaming cache-token capture deferred** — Anthropic's flat cache fields live on the `message_start` usage; capturing them in the SSE path needs threading extra stream-state. Non-streaming `parse_usage` covers it; noted inline as a follow-up.
- **CLI backend untouched** — chat-only subprocess owns its own config; effort/max_tokens don't apply.

## Verification

macOS: `cargo fmt` + `clippy --workspace` clean; **workspace tests 1693 passed / 0 failed** (added: manifest `maxTokens` serde, OllamaClient max_tokens body emission + nested-usage accessors, Anthropic `output_config.effort` + builders + `parse_usage` cache capture). Manifest JSON valid; EN/TH parity maintained.

## Status / deferred (rest of the roadmap)

Sprint 2: #3 prompt caching (send `cache_control` + already-parsed cache tokens make the payoff measurable), #5 structured output. Sprint 3: #4 extended thinking. Later: #7 MCP modernize (protocol `2024-11-05` → current, HTTP/SSE), #8 multimodal.

## Related

- Gap analysis (this session) · `provider/{types,client,anthropic}.rs` · `manifest.rs` · #330 (litellm) · claude-api skill (current Anthropic effort/thinking surface).
