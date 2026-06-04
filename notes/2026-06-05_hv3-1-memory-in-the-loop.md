# 2026-06-05 — HV3-1: Memory in the loop

First workstream of harness v3 (plan: `notes/2026-06-05_harness-v3-plan.md`).
The harness now closes the Tier 2 memory loop around every session — the
agent remembers across sessions (Sati).

## What changed

- **New `bwoc-harness::deep_memory`** (async, `tokio::process`, all calls
  timeout-bounded): `DeepMemoryCmd` resolves `deepMemoryCmd` from the workdir
  manifest (same placeholder filtering as `bwoc-core::deep_memory`), with
  `wake_up()` (10 s), `search()` (15 s), `mine()` (60 s, best-effort) and a
  `MemorySearch` `ToolImpl`.
- **Session start** (batch `run()` + `run_chat_mode()`): wake-up output is
  appended to the system prompt as a "Prior context (Tier 2 memory)" block.
- **Mid-run**: `memory_search` registered only when configured; read-only;
  added to chat's default-allow list beside `memory_read`. Batch policy stays
  operator-controlled via `harness-policy.toml`.
- **Session end**:
  - chat → mine `.bwoc/chat-session.json` (`--mode chat`);
  - batch success → distil *task → outcome(turns)* into `.bwoc/last-run.md`
    (overwritten per run) and mine that;
  - batch failure → mine the surviving checkpoint (`--mode run`).
- Docs: HARNESS EN+TH — new "Tier 2 Deep Memory (HV3-1)" section, tool-table
  row, **and the stale "OS-level sandbox: stub" caveat row fixed** (landlock/
  sandbox-exec shipped 2.3.0 — drift flagged in the v3 plan, fixed here).

## Decisions

- **Async re-implementation of the contract invocation in the harness**
  rather than calling `bwoc-core`'s sync `ShellDeepMemory`: the harness is
  tokio-native and the boundary calls would otherwise block the runtime or
  need `spawn_blocking` noise; the duplicated surface is ~40 lines and the
  placeholder constant is shared from core (single source for the filter).
- **Success-path mining mines a distillate, not the checkpoint.** First live
  test caught this: the agent loop deletes the checkpoint dir on success
  (Anattā — finished runs don't linger), so mining it after `run_loop` found
  nothing. The *task → outcome* distillate is the better memory material
  anyway; failures keep mining the surviving checkpoint since those carry the
  full history worth learning from.
- **Strictly opt-in, never fatal** — mirrors the CLI-side Tier 2 design:
  unset/placeholder ⇒ zero behaviour change; warnings, not errors; timeouts
  on every call.

## Verification

- `cargo test -p bwoc-harness deep_memory` — 6 pass (filter/argv/block pure
  tests cross-platform; exec-path tests cfg(unix)); fmt + clippy
  (`-D warnings`) clean.
- **Live two-run cycle** (real `bwoc-deep-memory` + ollama embeddings +
  gemma4): run 1 on an empty store → "Tier 2 configured (no prior context)",
  mined its distillate after success; run 2 → **"Tier 2 wake-up injected
  (109 chars)"** containing run 1's task/outcome, then mined again.

## Status / next

- HV3-1 done. Next per the plan: **HV3-2 unified context engine** (LLM
  compaction for the batch path + retrieval-augmented context under the
  existing budget gates).

## Related

- `crates/bwoc-harness/src/deep_memory.rs`, `crates/bwoc-harness/src/main.rs`
- `docs/en/HARNESS.en.md` (+ TH) §Tier 2 Deep Memory
- Reference backend: `crates/bwoc-deep-memory` (2026-06-03)
