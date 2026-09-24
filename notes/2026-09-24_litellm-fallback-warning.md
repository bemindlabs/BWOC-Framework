# 2026-09-24 — Warn when LiteLLM answers from a fallback model

Fern reported in #551 that LiteLLM can answer from a different model. She reproduced it on 2026-09-24. With `local-chat` and `max_tokens: 10000000`, vLLM returns 400 (the limit is 65536). The LiteLLM router then retries on the `local-chat-fast` group (typhoon-4b) and returns **HTTP 200**. It sends `x-litellm-attempted-fallbacks: 1` and `x-litellm-model-group: local-chat-fast`. An oversized *prompt* does not fall back; it returns `ContextWindowExceededError`.

## What changed

- `provider::client::litellm_fallback(headers)`: returns the serving group when the attempted-fallback count is above 0. The group name comes from the endpoint and is shown on screen, so control characters are stripped and it is capped at 80 characters.
- `OllamaClient::stream` puts one carrier chunk ahead of the content, with `StreamChunk.fallback = Some(group)`. The field is `#[serde(skip)]`: only headers set it, never the body. `complete()` logs the warning to stderr.
- The accumulator turns that chunk into `LiveDelta::Fallback`. The chat driver turns it into the new `ChatEvent::ModelFallback { requested, served }`. Runs without a live sink write it to stderr.
- The TUI shows ``⚠ this reply is from `<served>`, not `<requested>` ``. The status-line model stays the same, because the next turn asks for the requested model again.

## Decisions

- **A new event, not `Error`.** `bwoc-agent` warm and autoprocess treat `Error` as a failed turn: warm returns `Err`. A reply that came from a fallback is still a reply. Older consumers skip an event variant they don't know.
- **Not `ModelChanged`.** That event changes the session model. A fallback does not.
- **Headers, not the body's `model` field.** vLLM and Ollama echo concrete names that often differ from the alias that was requested. Only the LiteLLM header means the router actually fell back.
- **Warn, don't prevent.** Sending `disable_fallbacks: true`, or clamping `max_tokens`, would change routing. The owner's LiteLLM config decides that, not the harness.

## Verification

- Unit tests: header parsing; wiremock streams with and without the header; chat-session event order (the warning comes before the first token); TUI rendering.
- Live, against LiteLLM with the fern-mac virtual key (not the master key): `--max-tokens 10000000` → `{"type":"model_fallback","requested":"local-chat","served":"local-chat-fast"}` then the reply; `--max-tokens 256` → no event.
