# 2026-07-14 — Provider conformance suite

Added the first **cross-implementation** conformance tests for the `ProviderClient` trait (`crates/bwoc-harness/src/provider/conformance.rs`), asserting the trait *contract* holds uniformly across the two HTTP implementations that speak it. Hermetic — `wiremock` servers + a fake CLI, no live network.

## What changed

- **`provider/conformance.rs`** (new, `#[cfg(test)]`) — an `HttpSubject` trait implemented for `Ollama` (→ `OllamaClient`, which backs `ollama`/`openrouter`/`litellm`) and `Anthropic` (→ `AnthropicClient`). Each subject knows how to program its own mock backend (different URLs + request/response shapes); the shared `assert_http_contract::<S>()` runs 7 contract checks against each:
  1. `validate_model` — served model → `Ok`; absent → `ModelNotFound`.
  2. `complete` success → parsed assistant text.
  3. error mapping — `5xx` → `TransientProvider`; generic `4xx` (401) → fatal `Provider`.
  4. `list_models` → advertised ids.
  5. malformed 200 body → `Provider` parse error, **never a panic**.
  6. `stream` → deltas reassemble to the full content and terminate.
  Plus two `CliClient` graceful-default checks (`list_models` → empty, `model_context_limit` → None) — the best-effort half a chat-only CLI backend can't implement.
- **`provider/anthropic.rs`** — added `AnthropicClient::with_api_key(key)` (parity with `OllamaClient::with_api_key`) so the hermetic tests inject a key **without** mutating process-global `ANTHROPIC_API_KEY` (avoids adding env-mutating tests — see the flakiness finding below).
- **`provider/mod.rs`** — declares the test-only module.

## Decisions

- **Wire drift where it lives, not a rigid 5×3 matrix.** Reading the impls first (Yoniso Manasikāra) showed the contract is **not** uniform across all three: `CliClient` validates the *binary*, not the model, and has no HTTP-status mapping; even Ollama vs Anthropic diverge on `404` (Ollama → `ModelNotFound`, Anthropic → fatal `Provider`). So the shared suite targets the two HTTP impls (where a new OpenAI-/Anthropic-shaped backend actually drifts) using `5xx`/`401` for error mapping (not `404`), and covers `CliClient` via its existing subprocess tests + the two new default checks. The divergences are documented in the module header rather than papered over.
- **First `wiremock` use in `bwoc-harness`.** The existing per-impl tests cover parsing/helpers, not full HTTP round-trips — this is genuinely new coverage. `wiremock` was already a dev-dep.
- **`with_api_key` over env mutation** — deliberate, informed by the C-verification flakiness finding (below): more process-global env mutation is exactly what makes tests flaky under parallelism.

## Related finding (C — Linux verification, separate from this change)

Verifying today's macOS work on bemind Linux (6.17): the **`sandbox_escape` security gate passed 4/4** (FS-jail + egress + C7 git) — today's macOS SBPL changes don't touch the Linux proofs. But the full lib suite is **flaky under high parallelism**: `provider::cli::tests::subprocess::*` fail at 128 threads, `turn_executor::…::nproc_usage_counts_threads_not_processes` at 8 — all pass in isolation. Pre-existing test-infra flakiness (CLI subprocess spawn under load; thread-count timing), **not** a regression. Worth a separate tracking issue.

## Status / deferred

- Verified on macOS 26.5.1: 434 lib tests + 2 ignored pass (incl. 4 new conformance tests), fmt/clippy clean, no version churn (feature branch).
- Live-endpoint smoke (opt-in against a real ollama/litellm) deliberately deferred — hermetic-only keeps CI deterministic.
- Branch `feat/harness-provider-conformance`; PR pending.

## Related

- `crates/bwoc-harness/src/provider/{client,anthropic,cli}.rs` · the `ProviderClient` trait · #330 (litellm, the backend that motivated a drift guard).
