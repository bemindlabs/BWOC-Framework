# 2026-07-07 — harness `litellm` backend

Added `litellm` as a first-class harness backend: drive a self-hosted LiteLLM proxy (any
OpenAI-compatible `/v1`) through `bwoc spawn/run/chat --backend litellm`. It mirrors the
`openrouter` backend's footprint (reuse `OllamaClient`, no new provider client) but differs
in two deliberate ways because LiteLLM is self-hosted, not a hosted service.

## What changed

- `bwoc-harness/src/provider/client.rs` — `LITELLM_API_BASE_ENV`, `LITELLM_DEFAULT_ENDPOINT`
  (`http://localhost:4000/v1`), `LITELLM_API_KEY_ENV`, `resolve_litellm_endpoint()`,
  `resolve_litellm_api_key()`, and a `litellm_defaults_are_neutral` unit test.
- `bwoc-harness/src/main.rs` — `--backend` doc + `build_provider` `"litellm"` arm (reuse
  `OllamaClient`, bearer only when a key resolves) + `build_provider` doc. No
  `ensure_backend_credentials` arm (key optional).
- `bwoc-cli` — `Backend::LiteLlm` variant + every exhaustive match (`spawn.rs`), the
  `bwoc run` arm (`run.rs`), string parsers (`run.rs`, `chat.rs`), `banner.rs` backend list,
  and `help.rs` backend prose.

## Decisions

- **Endpoint resolved from env, never hardcoded (Samānattatā + open-source portability).**
  LiteLLM has no canonical URL, so the base comes from `--endpoint` / `baseUrl`, else the
  `LITELLM_API_BASE` env, else the LiteLLM default port. Deliberately *not* a workspace-
  specific infra host — that would leak a private hostname into a public, backend-neutral
  framework and break portability. A deployment points `LITELLM_API_BASE` at its own proxy
  (e.g. this workspace runs LiteLLM on bemind at `127.0.0.1:10400`, reached over a tunnel /
  tailscale-serve — set in the shell, not the source).
- **API key is optional** (unlike OpenRouter's required key). A local LiteLLM proxy is often
  keyless; bearer auth is attached only when `LITELLM_API_KEY` / `[litellm] api_key`
  resolves. So no `ensure_backend_credentials` fail-fast gate for `litellm`.
- **Mirror, don't fork.** LiteLLM speaks the exact OpenAI shape `OllamaClient` already
  implements, so no new provider client — same choice `openrouter` made (#268).

## Alternatives considered

- Hardcode the bemind tailnet URL as the default (rejected: leaks infra + non-neutral +
  non-portable; violates the framework's backend-neutrality HARD RULE).
- Require a key like OpenRouter (rejected: blocks the common keyless local-proxy setup).
- Route via the generic `openai-compatible` backend only (works, but a named `litellm`
  backend gives the env convention + optional-key semantics a first-class home, matching how
  `openrouter` is surfaced).

## Status / deferred

- Green locally: harness lib 420 pass (+1 new test), bwoc-cli 798 pass, fmt + clippy clean.
- Not added: a `HARNESS.en/th` section (openrouter has none either — the `--backend` help is
  the enumeration) and a `whats_new` highlight (would need the merged PR number).

## Related (links)

- Mirrors the OpenRouter backend (#268).
