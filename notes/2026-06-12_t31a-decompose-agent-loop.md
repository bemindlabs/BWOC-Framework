---
title: Phase 6 t31a — decompose agent_loop.rs into a directory module
date: 2026-06-12
tags:
  - type/note
  - area/harness
  - phase/6
---

# Phase 6 t31a — agent_loop.rs (3117 lines) → directory module

## What changed

`crates/bwoc-harness/src/agent_loop.rs` (3117 lines, of which ~63% was one
`#[cfg(test)] mod tests`) became a directory module `agent_loop/` with the
production helpers split into cohesive siblings, following the crate's existing
convention (`policy/`, `provider/`, `tools/` are already directory modules):

- `agent_loop/mod.rs` — config/result types (`VettedMode`, `LoopConfig`,
  `LoopResult`), the `run_loop` driver + `persist_checkpoint`, and the **single
  consolidated `mod tests`** (47 tests, unchanged).
- `agent_loop/provider.rs` — `call_with_retry_v2` (+ private `call_provider_once`,
  `backoff_ms`) and the retry/backoff consts. The provider-call + transient-retry
  layer.
- `agent_loop/context.rs` — `estimate_context_tokens`, `model_effective_limit`,
  `find_larger_vetted_model`, `has_malformed_tool_calls`, and the
  `CONTEXT_HEADROOM_FRAC` / `MALFORMED_TOOL_CALL_THRESHOLD` consts. Token/context
  pressure + malformed-tool detection (pure functions).
- `agent_loop/execute.rs` — `execute_tool_calls` (+ `ToolCallResult`) and
  `stream_and_accumulate` (+ private `ToolCallAccumulator`). The safety-pipeline
  dispatch + streaming accumulation.

Net: mod.rs dropped from 3117 → ~1158 production-relevant lines around the test
module; the moved helpers are ~95 + ~140 + ~210 lines in their own files.

## Decisions

- **Production-split, one test module (operator's call).** The crate convention
  is "directory module + sibling files, each with its own inline tests." The
  tests here are integration-style around `run_loop` and share one heavy mock
  surface (`MockProvider` + `Limited`/`StreamingMockProvider` + factory helpers),
  so relocating them per-cluster would have meant a shared `test_support` module
  and visibility plumbing across four files. Per the owner's decision we kept the
  **single consolidated `mod tests` in mod.rs** and split only production code —
  the lower-risk cut. (Mattaññutā — right amount; the convention's
  tests-with-code intent is relaxed deliberately, recorded here.)
- **`pub(super)` for the moved items.** Everything the driver or a sibling calls
  is `pub(super)` (visible within `agent_loop`), nothing wider. `call_with_retry_v2`
  keeps its prior `pub(crate)`. The driver re-imports what it uses; the test
  module imports the two helpers + consts the driver only calls indirectly
  (`backoff_ms`, `stream_and_accumulate`, `MAX_*`).
- **Pure move, zero behaviour change.** No logic edited — verified by the full
  suite staying green (47 agent_loop tests + workspace).

## Alternatives considered

- Full tests-with-code split (Q1 option B) — most convention-faithful but highest
  effort/risk (47 tests + 4-file visibility plumbing + shared mock module).
  Rejected for this pass per owner.
- Defer agent_loop, do chat_session/turn_executor first — agent_loop is the
  largest (3117 vs 1777/1387) and the cleanest seam set, so it went first.

## Status / deferred

- `chat_session.rs` (1777) and `turn_executor.rs` (1387) remain as follow-up
  decompositions (separate PRs — one file per concern).
- t31b (backend-parametrized eval + ambient guard) is the other half of t31.

## Proof

`cargo clippy -p bwoc-harness --all-targets -- -D warnings` clean; `cargo test
-p bwoc-harness --lib agent_loop` 47 passed; workspace fmt/clippy/test green
(see session summary for counts).
