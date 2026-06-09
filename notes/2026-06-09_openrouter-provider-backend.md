# 2026-06-09 — OpenRouter provider backend for bwoc-harness

Added `openrouter` as a first-class spawn backend so any agent can drive any
vendor's model (OpenAI / Anthropic / Google / Meta / NVIDIA / …) through
OpenRouter's hosted OpenAI-compatible aggregator with a single key.

## What changed

- **`bwoc-harness/src/provider/client.rs`** — extended the OpenAI-compatible
  `OllamaClient` with optional **bearer auth** (`with_api_key`) and **extra
  headers** (`with_headers`), applied via a new `auth()` helper at all four
  request sites (`complete`, `stream`, `validate_model`, `list_models`).
  `api_key = None` is byte-for-byte the old Ollama path, so nothing regresses.
  Added OpenRouter constants (`OPENROUTER_DEFAULT_ENDPOINT`,
  `OPENROUTER_API_KEY_ENV`), `resolve_openrouter_api_key()`, and
  `openrouter_headers()` (optional `HTTP-Referer` / `X-Title` attribution).
- **`bwoc-harness/src/provider/anthropic.rs`** — generalized the secrets reader:
  `api_key_from_secrets(path, section)` + shared `resolve_provider_api_key(env,
  section)` (both `pub(crate)`), reused by OpenRouter so the chmod-600
  "Adinnādāna" guard is identical. Anthropic behaviour unchanged (`"anthropic"`).
- **`bwoc-harness/src/main.rs`** — `build_provider` gained an `"openrouter"` arm:
  `OllamaClient` + bearer key + headers, swapping the Ollama-localhost default
  for OpenRouter's base when `--endpoint` is unset (mirrors the Anthropic swap).
- **`bwoc-cli`** — new `Backend::OpenRouter` wired through `spawn.rs`, `run.rs`,
  and `chat.rs::parse_backend`. `baseUrl` is **optional** (harness defaults).
  Doc/display touch-ups in `manifest.rs`, `banner.rs`, `help.rs`.
- **`bwoc-tui/src/lib.rs`** — `harness_argv` now forwards `--backend <name>`.
  This was the subtle gap: the TUI chat path never passed `--backend`, so an
  OpenRouter agent would have silently fallen through to the unauthenticated
  client and 401'd. Now load-bearing for `openrouter`; a no-op for ollama /
  openai-compatible (both still resolve to the same OpenAI-compatible client).

## Decisions

- **Extend the OpenAI-compatible client, not a new `OpenRouterClient`.**
  OpenRouter speaks the exact OpenAI shape the client already implements
  (`/chat/completions`, `/models`, SSE, tools, `reasoning_effort`); the only gap
  was the missing `Authorization` header. Mattaññutā — add the one missing thing.
- **`--backend openrouter` is mandatory on every harness invocation** (spawn,
  run, tui) because the harness defaults to `ollama` and would otherwise skip
  auth. Without the flag, requests 401 silently — so the flag is wired at all
  three call sites, not just spawn.
- **Key: `OPENROUTER_API_KEY` env → `~/.bwoc/secrets.toml [openrouter] api_key`**
  (per-user, chmod-600 guarded), reusing the Anthropic resolver.

## Alternatives considered

- Dedicated `OpenRouterClient` mirroring `AnthropicClient` — rejected as
  duplicate chat/SSE/tools/models code for a provider that needs only auth.
- Reusing the existing `openai-compatible` backend — rejected: that path has no
  way to attach a bearer token, which OpenRouter requires.

## Status / known cosmetics

- The startup banner prints `endpoint: http://localhost:11434/v1` for an
  `openrouter` agent with no `--endpoint`, because it echoes `args.endpoint`
  before `build_provider` swaps in the OpenRouter base. Pre-existing behaviour
  shared with the `claude` backend; the real request goes to OpenRouter. Left
  as-is to keep the diff focused.
- `check::BACKEND_NAMES` (neutrality allowlist, doc-pinned to ARCHITECTURE.en.md
  "six backends") intentionally **not** extended — out of scope here.

## Verification

`cargo fmt` + `clippy` clean; `cargo test` green (harness 750, cli 331, tui 11,
core — 0 failed). Live smoke against OpenRouter free models confirmed bearer auth
end-to-end: `nvidia/nemotron-3-nano-30b-a3b:free` → "OK",
`google/gemma-4-31b-it:free` → "4", and an empty key fails fast with a clear
`HTTP 401` rather than hanging or silently degrading.
