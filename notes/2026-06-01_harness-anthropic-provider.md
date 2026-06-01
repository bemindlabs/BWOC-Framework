# 2026-06-01 — Native Anthropic (Claude) provider in bwoc-harness

Added a second `ProviderClient` implementation so `claude`-backend agents run
through the harness and emit the `chat_proto` stream — the same wire format
`ollama`/`openai-compatible` already produce. This lets `bwoc-chat` render a
native window for Claude agents (previously they fell back to a Terminal `bwoc
chat`, because only harness-driven backends emit `chat_proto`).

## What changed

- **`crates/bwoc-harness/src/provider/anthropic.rs`** (new) — `AnthropicClient:
  ProviderClient`. A pure translation layer over the OpenAI-shaped
  `provider::types`:
  - Request: lifts `Role::System` into the top-level `system`; maps roles to
    Anthropic content blocks; emits assistant `tool_use` and merges consecutive
    `Role::Tool` messages into one user turn of `tool_result` blocks; rewrites
    tools to `input_schema`.
  - Response: `content[]` text/`tool_use` → `ChatMessage` + `ToolCall`;
    `stop_reason` → `FinishReason`; `usage{input,output}` → OpenAI `Usage`.
  - Streaming: a spawned task parses Anthropic's typed SSE
    (`message_start`…`message_stop`) and re-emits `StreamChunk`s; input tokens
    (from `message_start`) are carried so the final `message_delta` reports a
    complete `Usage`. Exposed as a `Stream` via `unfold` over a tokio mpsc
    receiver (no new dep).
  - Auth: `x-api-key` from `ANTHROPIC_API_KEY`; missing key → a clear error at
    `validate_model`/first call. `validate_model` is lenient (best-effort
    `/v1/models` membership) like the OpenAI path.
- **`provider/mod.rs`** — export `anthropic` + `AnthropicClient`.
- **`main.rs`** — new `--backend` flag (default `ollama`); `build_provider()`
  factory replaces the two hardcoded `OllamaClient` sites; `ChatConfig.backend`
  now reflects the real backend instead of the literal `"ollama"`. For
  `claude`/`anthropic` with the endpoint left at the Ollama default, the factory
  substitutes `https://api.anthropic.com`.

## Decisions

- **Translate to the existing OpenAI-shaped types rather than generalize the
  trait.** The chat/agent loops, tool dispatch, and accumulators are all written
  against `provider::types`; a translation layer keeps the blast radius to one
  file (*Mattaññutā* — minimal surface; *Samānattatā* — Claude becomes a
  first-class backend equal to Ollama with no loop changes).
- **API key via `ANTHROPIC_API_KEY` env, not a manifest field.** Matches the
  Anthropic SDK convention and keeps secrets out of `config.manifest.json`. The
  vendor `claude` CLI's subscription auth is not reusable for the Messages API.
- **No total request timeout on the streaming client** (connect-timeout only) —
  a long generation would otherwise be cut mid-stream by reqwest's `.timeout()`.
- **`max_tokens` = 8192** — the Messages API requires it; generous for coding.

## Alternatives considered

- **OpenAI-compatible proxy (LiteLLM) in front of Claude** — zero harness code,
  but needs a long-lived proxy holding the key. Kept as a documented option; the
  native provider removes the extra moving part for the common case.
- **Inferring the backend from the endpoint URL** — rejected; an explicit
  `--backend` flag is unambiguous and symmetric across providers.

## Status / deferred

- Unit tests cover request mapping (system lift, tool_use + merged tool_results,
  `input_schema`), response parsing, and SSE translation (text delta, usage
  stitching, tool-call start + json delta) — 7 tests, green. fmt + clippy clean.
- Verified end-to-end through the real `bwoc-harness --chat --backend claude`
  binary against a mock Anthropic SSE server: `ready(backend=claude)` → `token`
  ×2 → `message` → `turn_end(11 in / 4 out)`.
- **Not** exercised against the live api.anthropic.com in this session (no key
  present). `codex`/`kimi`/`agy` remain Terminal-only by design — not requested.

## Related (links)

- Downstream: `projects/bwoc-chat` now allows the `claude` backend and passes
  `--backend`; `projects/bwoc-mcc` adds `claude`/`anthropic` to its native-chat
  backends.
- `crates/bwoc-harness/src/provider/client.rs` — the OpenAI-compatible sibling.
