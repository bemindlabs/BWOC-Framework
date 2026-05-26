# 2026-05-26 — Real OpenTelemetry OTLP exporter (BWOC-2)

Replaced the `--features otel` stub (an `eprintln!`) with a real OTLP span exporter. One span per finished session ships to an OTEL collector. Last backlog item this auto-pilot batch; standalone (not harness-v2 epic). Maintainer ratified the OTLP (network-egress) variant over the lighter stdout exporter.

## What changed

- `Cargo.toml`: `otel` feature now pulls `opentelemetry` 0.27, `opentelemetry_sdk` 0.27 (`rt-tokio`), `opentelemetry-otlp` 0.27 — all `optional = true`, so the **default build still has zero OTEL deps** (dep-quarantine holds; heavy deps live only in bwoc-harness, the quarantine crate).
- `telemetry.rs::export_otel_span` (feature-gated): builds an OTLP/tonic `SpanExporter`, a batch `TracerProvider` (tokio runtime), emits a `bwoc.session` span with `session.id` / `agent.id` / `tasks.attempted` / `tasks.completed` / `gates.passed` / `gates.failed` attributes, then `force_flush`. Endpoint from the standard `OTEL_EXPORTER_OTLP_ENDPOINT` env (default `http://localhost:4317`).
- Verified: default `cargo test` 235 green (OTEL excluded); `cargo clippy` clean both default and `--features otel`; `cargo build --features otel` compiles against the real 0.27 API.

## Decisions

- **OTLP/tonic exporter, network egress (maintainer choice).** Full production telemetry to a collector; the network surface is opt-in (feature + env-configured endpoint), not present in the default build.
- **Best-effort, never fatal.** Exporter-build / flush errors are logged (`eprintln!`) and swallowed — telemetry must not break a run. *Observe-don't-drive.*
- **Per-session one-shot provider.** Build provider → emit → flush → drop, per `Telemetry::finish`. Sessions are infrequent (one per run), so a long-lived global provider isn't worth the lifecycle complexity. *Mattaññutā.*
- **Span attributes carry counts only**, consistent with the module's secrets rule (no arg/env/credential strings in telemetry).

## Alternatives considered

- stdout/file exporter (`opentelemetry-stdout`) — lighter, no network surface; not chosen (maintainer wanted real OTLP to a collector).
- Hand-rolled OTLP/JSON over HTTP (no SDK) — rejected; the OTLP wire format + ret/proto is exactly what the SDK exists for, and it's feature-gated so the dep-quarantine isn't dented.

## Status / deferred

- Status set to `review` on the workspace board (BWOC-2).
- **No automated test of the live export path** (needs a running collector) — same posture as the MCP real transport; covered by the compile check under `--features otel`. The default build/CI path is fully tested.
- Span/trace context propagation across the lead → worker subprocess boundary (HV2-1) is not wired — each process exports its own session span; linking them is a follow-up.

## Related (links)

- `notes/2026-05-26_hv2-3-run-end-retrospective.md` — the SessionRecord this span mirrors.
- `<workspace>/.scrum/backlog.json` — BWOC-2.
