# 2026-05-26 — HV2-7 streaming-usage + parallel tool execution (BWOC-9)

Closed the v1 streaming-usage gap and made independent tool calls run concurrently. Sequenced before HV2-6 (budget) so a streaming budget gate isn't blind. Fifth workstream in the auto-pilot batch; built after BWOC-3 was parked at its spec gate.

## What changed

- `provider/types.rs`: `StreamChunk` gains `usage: Option<Usage>` (serde default) and `#[serde(default)]` on `choices` — so the final usage-only chunk (empty choices) parses.
- `provider/client.rs`: `build_request_body` sets `stream_options.include_usage = true` on the streaming path. Providers that don't support it ignore the field.
- `agent_loop.rs`:
  - `stream_and_accumulate` now returns `(ChatMessage, Option<Usage>)`, capturing the last non-empty `chunk.usage`. `call_provider_once`'s streaming branch returns it instead of the hardcoded `None` (was `agent_loop.rs:584`). Telemetry token counts (`tb.tokens_in/out`) now populate on streaming runs.
  - `execute_tool_calls` runs calls concurrently via `futures_util::future::join_all` instead of a sequential loop; `join_all` preserves input order so results line up with `calls`. Each call still flows through guardrails → permission → sandbox independently.
- Tests: `streaming_path_exposes_usage`, `streaming_without_usage_chunk_returns_none`, `independent_tool_calls_all_execute_in_order` (+ a `StreamingMockProvider` replaying chunks). 226 lib tests green; clippy + fmt clean.

## Decisions

- **Usage via `stream_options.include_usage`, last-chunk-wins.** OpenAI-compatible endpoints emit a final chunk with `usage` and empty `choices`; capture it rather than estimate. Absent chunk → `None`, never a fabricated zero (a streaming run against a provider that omits usage stays honestly unknown). *Yoniso manasikāra — surface the real number or none, don't invent one.*
- **`join_all`, not `tokio::spawn` per call.** The futures borrow `&registry`/`&ctx`/`&config`/`&os_sandbox`; `join_all` overlaps them on one task without requiring `'static`, and preserves order for free. Spawning would force `'static`+`Arc` churn for no benefit at this fan-out. *Mattaññutā.*
- **Safety invariant unchanged.** Concurrency is per-call within the existing pipeline — each call independently runs `run_pipeline` then sandbox/dispatch. No new bypass; the gate is still per-call.

## Alternatives considered

- Estimating streaming usage from accumulated text — rejected (inaccurate; the provider's own counts are authoritative when offered).
- `tokio::spawn` for tool calls — rejected (`'static` + `Arc` overhead; `join_all` suffices).

## Status / deferred

- Status set to `review` on the workspace board (BWOC-9).
- **Concurrency overlap is not timing-asserted** — tests verify correctness + order (no sleepy test tool to assert wall-clock overlap). The refactor's observable contract is "all calls execute, results ordered."
- Potential concurrent write-races between tool calls targeting the same path are the model's responsibility (it issues the calls); not guarded here.
- Unblocks HV2-6 (BWOC-8) — a streaming budget gate now has token counts to read.

## Related (links)

- `notes/2026-05-25_harness-v2-planning.md` — HV2-7 seam (agent_loop.rs:584) + the before-HV2-6 ordering rationale.
- GH #39 (harness-v2 epic, HV2-7). `<workspace>/.scrum/backlog.json` — BWOC-9.
- Sibling built workstreams: `hv2-2-durable-runs.md`, `hv2-3-run-end-retrospective.md`, `hv2-1-sangha-runtime.md`.
