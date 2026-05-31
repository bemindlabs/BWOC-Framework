# 2026-05-31 — Research: OpenTelemetry exporter for `bwoc-harness` (BWOC-2)

Deep-research synthesis (102 agents, 20 sources, 25 claims adversarially verified — 23 confirmed / 2 killed). Informs BWOC-2 ("replace telemetry stub with a real OpenTelemetry exporter"). The harness already ships a feature-gated (`--features otel`) one-span-per-session OTLP exporter at opentelemetry 0.27; this note is the basis for bringing it to current best practice.

## Recommended crate stack (verified current, late May 2026)

| crate | version | note |
|---|---|---|
| `opentelemetry` | 0.32 | API surface |
| `opentelemetry_sdk` | 0.32 | `SdkTracerProvider`, `BatchSpanProcessor` |
| `opentelemetry-otlp` | 0.32 | feature `grpc-tonic` (gRPC) or `http-proto` |
| `tracing-opentelemetry` | 0.33 | only if bridging from `tracing` |

- Pre-1.0 **lockstep** — all `opentelemetry*` must share the same minor. **Logs/Metrics = Stable, Traces = Beta.**
- **Killed claim** (0-3): the docs "getting-started" pin of `opentelemetry-otlp 0.28.0` is stale — 0.32.x is current.

## GenAI semantic conventions (status: **Development**, not yet Stable)

- **Spans**: `gen_ai.operation.name` + `gen_ai.provider.name` (Required), `gen_ai.request.model` (Conditionally Required), `gen_ai.usage.input_tokens` / `gen_ai.usage.output_tokens` + cache (Recommended).
- **Metrics**: `gen_ai.client.operation.duration`, `gen_ai.client.token.usage`.
- Maps cleanly onto the harness's existing per-turn token/cost + model-switch records.

## Exporter + CLI-lifecycle (the load-bearing part)

- **BatchSpanProcessor** (not Simple); gRPC/tonic is the default transport.
- **Short-lived CLI flush pitfall (verified)**: hold the provider and call `provider.shutdown()` **before process exit** — the global/layer shutdown path **does not flush** (PR 1625) → spans silently dropped. `force_flush` historically deadlocked; fixed in 0.28 by the thread-based processor (so `rt-tokio` is no longer required for batching).
- **Killed claim** (1-2): "keep a handle and call `force_flush()` instead of shutdown" is *not* the right workaround — use a proper `shutdown()`.

## Optional / zero-overhead gating

- Gate on **`OTEL_EXPORTER_OTLP_ENDPOINT`** (SDK default `localhost:4317`/`4318`). When unset → build no exporter at all (no connection attempts, no overhead). "Layer-on-Option" if bridging `tracing`.
- For BWOC: **keep the compile-time `otel` feature** (dep-quarantine — opentelemetry is heavy + pulls tonic/prost; the default build must stay lean) **and** add the runtime env-gate inside it.

## Pitfalls

- Rebuilding the provider per export is wrong — construct once, reuse, shut down once.
- Attribute **cardinality**: don't put unbounded values (raw prompts, ids) on metric attributes.
- Pre-1.0 churn: 0.28 was a large breaking migration (`TracerProvider` → `SdkTracerProvider`, batch processor lost its runtime arg).

## Caveats

GenAI semconv is all **Development** (may change). Shutdown/deadlock evidence predates 0.28 but the guidance still holds (global helper removed; processor now thread-based).

## Decision for BWOC-2

Not a from-scratch task — the exporter exists. Scope: (1) bump 0.27 → 0.32 + fix the API migration; (2) explicit env-gate on `OTEL_EXPORTER_OTLP_ENDPOINT` (silent no-op when unset, even under the feature); (3) GenAI-semconv attributes on the session span + correct construct-once / shutdown-flush lifecycle. **Per-turn child spans** (a span per agent-loop turn carrying `gen_ai.*` token attrs) are a clean **phase-2** once the session span lands.

## Implemented (same session)

Phase-1 landed in `crates/bwoc-harness`:
- `Cargo.toml`: `opentelemetry*` 0.27 → **0.32** (lockstep; `opentelemetry-otlp` gains `grpc-tonic`; dropped the now-unneeded `rt-tokio` — the 0.32 batch processor is thread-based).
- `telemetry.rs::export_otel_span`: rewritten for the 0.32 API (`SdkTracerProvider`, `with_batch_exporter(exporter)`); **env-gated** on `OTEL_EXPORTER_OTLP_ENDPOINT` (silent no-op when unset); GenAI-semconv attrs (`gen_ai.operation.name`, `gen_ai.usage.input_tokens`/`output_tokens` from `harness.totals`); explicit `provider.shutdown()` flush.
- Verified: default build keeps zero OTEL deps; `--features otel` compiles on 0.32; E2E against ollama with no endpoint runs clean (silent no-op) and the local `session-metrics.jsonl` is unaffected. Actual OTLP wire-export needs a live collector (untested here).
- **Deferred (phase-2 / acceptance criterion 6):** per-turn `gen_ai.*` child spans (thread the tracer through `agent_loop`).

## Sources (primary)

- opentelemetry-rust repo + 0.28 migration doc; `opentelemetry-otlp` / `opentelemetry_sdk` docs.rs
- OTel GenAI semconv: agent-spans, gen-ai-spans, gen-ai-metrics
- Flush/shutdown issues: opentelemetry-rust #1961, #1395, #1637, #2715
