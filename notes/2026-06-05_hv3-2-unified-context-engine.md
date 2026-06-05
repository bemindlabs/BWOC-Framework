# 2026-06-05 — HV3-2: Unified context engine

Second workstream of harness v3 (plan: `notes/2026-06-05_harness-v3-plan.md`).
Batch and `--chat` now share one compaction policy, and what falls out of the
context window falls into Tier 2 memory.

## What changed

- **`compact::compact_context`** — the unified engine, replacing both
  `compact::maybe_compact` (chat-only summarize, no fallback) and
  `agent_loop::compact_history` (batch-only truncate). One pass:
  1. `plan_compaction` picks the fold span (unchanged semantics: tail grown to
     ~half the budget, never starting on an orphan tool result);
  2. **summarize-first** — one provider call folds the span into a system
     note; 3. **truncate fallback** — summarizer failure/empty folds the same
  span behind the v1 `[context compacted: …]` marker, so the history always
  shrinks; 4. **Tier 2 synergy** — with `deepMemoryCmd` configured, the folded
  content (summary, or raw excerpt on fallback) is written to
  `.bwoc/compacted-context.md` and mined `--mode compaction`.
  Returns `Compaction::{None, Summarized, Truncated}` with `removed()`.
- **Batch loop** gains real summarization (was truncate-only by explicit v1
  design — that rationale is preserved as the fallback's no-new-failure-mode
  floor); **chat** gains the guaranteed-shrink fallback (a failing summarizer
  previously left the history over budget indefinitely).
- `LoopConfig`/`ChatConfig` untouched: the engine resolves `DeepMemoryCmd`
  from `ctx.workdir` itself (compactions are rare; one manifest read per pass
  beats threading a field through two configs and every test literal).
- **Honest metrics**: the loop's `compactions` counter now increments only
  when messages were actually folded — previously a *triggered no-op* (history
  too short to fold) also incremented.

## Tests

- 4 new engine tests in `compact.rs` (local `EngineMock` provider): under
  budget → `None`; provider success → `Summarized` with the summary in the
  note; provider failure → `Truncated` with the marker and a genuinely shorter
  history; Tier-2-configured workdir → `.bwoc/compacted-context.md` written
  (cfg(unix), `deepMemoryCmd = "true"`).
- 3 loop tests updated for the honest semantics — they previously passed
  because the old code counted no-op triggers: each now feeds a real
  multi-message over-budget history and queues a summarizer response ahead of
  the turn's final response. The old `compact_history_*` unit tests (4) were
  deleted with the function they tested.
- Full harness suite: 303 pass; fmt + clippy `-D warnings` clean.

## Doc updates

- `agent_loop.rs` module doc: the "Why truncate-with-marker rather than
  LLM-summarise?" section described the pre-HV3-2 world — rewritten to
  describe the engine.
- HARNESS EN+TH caveat row "Context compaction: truncate-with-marker, LLM is
  a future upgrade" → unified-engine description.

## Status / next

- HV3-2 done. Next per the plan: **HV3-3 Saṅgha collaboration** — needs the
  reviewer-selection decision (fixed / round-robin / manifest-declared), and
  HV3-5's MCP-vs-A2A decision is still open.

## Related

- `crates/bwoc-harness/src/{compact,agent_loop,chat_session}.rs`
- `notes/2026-06-05_hv3-1-memory-in-the-loop.md` (the Tier 2 plumbing this
  engine's synergy rides on)
