# 2026-09-23 — `/models` lists through the harness (#551)

`/models` in the chat TUI refused every backend but Ollama. LiteLLM (and OpenRouter, OpenAI-compatible servers, Anthropic) serve `GET /models`, and LiteLLM scopes that list to the key — so the right answer was already one authenticated GET away.

## What changed

- `ProviderClient::try_list_models` — `list_models` that keeps the failure. The default wraps `list_models`; `OllamaClient` (behind ollama/openrouter/litellm/openai-compatible) implements it and `list_models` now delegates with `unwrap_or_default()`, so auto-resolution is unchanged.
- `bwoc-harness --list-models` prints one id per line, resolved through the same `build_provider` + `ensure_backend_credentials` as a chat session.
- `Environment::models` in `bwoc-cli` keeps the native `/api/tags` probe for Ollama and shells to `bwoc-harness --list-models` for every other harness backend.

## Decisions

- **Ask the harness, don't reimplement.** `bwoc-cli` has no HTTP client (the Ollama probe is raw HTTP/1.0 TCP and cannot speak HTTPS). Shelling out reuses the exact endpoint/key resolution the chat uses, so the listing cannot drift from it — same reasoning as `/doctor` running `bwoc doctor --json`.
- The error keeps the first 200 chars of the server's body. LiteLLM's 401 names the reason ("No api key passed in", "Invalid proxy server token") and masks the key to its last 4 chars.

## Alternatives considered

- `reqwest` in `bwoc-cli` — new heavy dep plus a second copy of key/endpoint resolution.
- A `ListModels` chat-protocol input — the listing would need a live session and a new variant on both sides.

## Verified

Against the local LiteLLM with a 5-model virtual key: `127.0.0.1:10400` and the tailnet HTTPS URL both list the 5; no key / bogus key → `HTTP 401` with the reason; closed port → request error. End-to-end: `bwoc` with a temp `HOME` holding the issue's exact `config.toml` + `secrets.toml`, `/models` → `5 models`, `▸ local-chat`.
