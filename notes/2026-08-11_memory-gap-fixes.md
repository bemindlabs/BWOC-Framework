# 2026-08-11 — Memory gap fixes (Tier-1 recall, memoryPath, cap, frontmatter)

A four-track multi-agent audit of BWOC's workspace + agent memory surfaced that the
**two-tier memory model is built but Tier 1 is weak**: the CLI/tools exist, but recall,
path-honoring, cap enforcement, and validation were convention-only. This session closes
the four highest-ROI gaps across two PRs.

## What changed

**PR 1 — `bwoc-harness` Tier-1 memory (this note's branch):**
- **Recall at boot (SRS FR-7.16).** `run()` and the chat driver now load the agent's
  `MEMORY.md` index into the system prompt (a Tier-1 counterpart to the existing Tier-2
  `wake-up`), with a Yoniso-Manasikāra reminder to verify claims against current code.
  Previously nothing loaded Tier-1 memory — the agent only saw it if the model chose to
  call `memory_read`.
- **Honor `memoryPath`.** `ToolContext` gained a `memory_dir` (default `workdir/memories`,
  overridable via `with_memory_dir`). `memory_read`/`memory_write` and boot recall use it,
  resolved from the manifest's `memoryPath`. Previously the harness hardcoded `memories/`,
  silently ignoring a configured override.
- **200-line cap is honest.** `memory_write` appends a soft WARNING when a `MEMORY.md`
  write exceeds the cap (never truncates — that would lose curated content). Corrected the
  template `AGENTS.md` claim "Lines beyond 200 are truncated" (nothing truncates).

**PR 2 — `bwoc check` memory-file frontmatter validation (separate branch):** see that PR.

## Decisions
- **Inject the index only, not every memory file** (Mattaññutā) — individual files stay
  behind the `memory_read` tool so the prompt doesn't bloat.
- **Warn, never truncate** the over-cap index — silent truncation would destroy the very
  content the cap is meant to make the agent curate.
- **`with_memory_dir` as a builder** — real run paths honor the manifest; tests + eval keep
  the `workdir/memories` default with zero churn at ~dozen `ToolContext::new` call sites.

## Alternatives considered
- Threading `memoryPath` through every `ToolContext` constructor (rejected: churn; the
  builder + a single `memory_dir_for(workdir)` helper is leaner).
- Hard-truncating MEMORY.md on write (rejected: data loss).

## Status / deferred
- **Not addressed this session** (lower priority, from the same audit): `sessionsPath`
  mining pipeline unwired; two disconnected Tier-1 stores (workspace `.bwoc/memory/` vs
  agent `memories/`) with no unified cross-scope API; Tier-2 backend inert by default;
  no Tier-1 prune; `task-log.jsonl` has no writer; team shared-memory absent.

## Related
- Investigation: 4 parallel Explore agents (spec / impl / workspace-level / lifecycle).
- Spec: `modules/agent-template/docs/en/SRS.en.md` FR-7.x; `AGENTS.md` §7.
