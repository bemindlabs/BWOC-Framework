# 2026-07-26 — Harness LLM Sprint 2: prompt caching (Anthropic)

Second slice of the LLM-backend modernization. Closes gap #3 — no prompt caching — building on Sprint 1 (#380), which already parses the cache-token usage that makes the payoff measurable.

## What changed

- **`AnthropicClient`** now marks the stable system-prompt prefix with `cache_control: {type: "ephemeral"}`. `build_anthropic_body` gained a `cache: bool` param: when on and the system prompt is non-empty, `system` is emitted as a one-element text-block array carrying `cache_control` (the API's last-block-of-prefix cache — the `tools` block, rendered before `system`, is covered by the same breakpoint); when off it stays the plain string (byte-identical to the prior wire form). New `with_prompt_cache(bool)` builder, default **on**.
- **`promptCache: Option<bool>`** manifest field (`crates/bwoc-core/src/manifest.rs`) — opt-**out** (`None ≡ on`). Threaded through `build_provider` (+ its 3 manifest-load sites: run / eval / chat) as `m.prompt_cache.unwrap_or(true)`; absent/malformed manifest → caching on.
- **Docs (EN + TH parity):** `docs/{en,th}/HARNESS.md` + template `config.manifest.json` document `promptCache` and the caching behaviour.

## Decisions

- **On-by-default + opt-out** (user's call). A BWOC agent's system prompt (AGENTS.md-derived) is stable across a session, so an agentic loop resending it every turn is the textbook prompt-caching win (~0.1× read vs full input). Below the provider minimum cacheable size the marker is a silent no-op, so on-by-default never *hurts*; a volatile prompt can opt out with `promptCache: false`.
- **System-block breakpoint, not a separate tools breakpoint** — one `cache_control` on the system block caches the whole `tools → system` prefix (render order), so a single marker suffices. No separate tools handling.
- **Anthropic native only** — OpenAI / DeepSeek do prompt caching provider-side automatically (no `cache_control` to send); the cache-*read* tokens they report are already surfaced by Sprint 1's `Usage`. So Sprint 2's request-side change is Anthropic-specific.
- **Cache-hit verification is free** — Sprint 1's `Usage::cached_tokens()` / `cache_creation_tokens` already read the response accounting, so an operator can confirm hits without further work.

## Verification

macOS: `cargo fmt` + `clippy --workspace` clean; **workspace tests 1696 passed / 0 failed** (new: `system_cache_control_toggles`, `with_prompt_cache` default/opt-out, manifest `promptCache` serde). Manifest JSON valid; EN/TH parity maintained.

## Status / deferred

Still open on the roadmap: #5 structured output (Sprint 2's sibling — can pair or follow), #4 extended thinking (Sprint 3), #7 MCP modernize, #8 multimodal. Streaming cache-token capture (from Sprint 1) remains the one deferred telemetry item.

## Related

- Sprint 1 (#380) — `Usage` cache fields, effort parity, configurable max_tokens · `provider/anthropic.rs` · `manifest.rs` · claude-api skill (Anthropic prompt-caching surface).
