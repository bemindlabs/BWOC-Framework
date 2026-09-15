# 2026-09-15 — Lean, honest claims

The template and framework docs promised behaviour the code does not have. This pass re-verified each claim against the source, deleted or relabelled the false ones, and retired the two template shell scripts that `bwoc new` / `bwoc check` replaced. No runtime behaviour changed; the only code edits are doc comments and one `bwoc help manifest` line.

## What changed

- **Scripts retired.** `modules/agent-template/scripts/{incarnate,check-agent-neutrality}.sh` removed (`scripts/` is gone). `new.rs` never copied or chmod-ed `scripts/`; the embedded `include_dir!` template simply has one directory fewer. References repointed at `bwoc new` / `bwoc check` across template, `docs/en|th`, README, the two Claude Code skills and root `CLAUDE.md`.
- **Manifest claims.** `fallbackModel` = metadata only (harness `fallback_models` come from `autoModels` via `model_select`). `sessionsPath` = reserved/unused (serialized by `bwoc-core`, read nowhere). Schema docs use `primaryModel` and flat `memoryPath`. The template manifest `defaults` block (`maxConcurrentTasks`, `worktreeIsolation`, `memory.*`) was dropped — nothing read it. `context_limit` is hardcoded `0` in `bwoc-harness/src/main.rs`; the harness loads `AGENTS.md` (or `CLAUDE.md`) + `MEMORY.md` index + optional Tier-2 wake-up.
- **Slots / task-log.** `capabilities.md` is not parsed (A2A card = `bwoc_a2a::card::card_from_manifest`). `persona/`, `mindsets/`, `skills/` are reference material (`load_system_prompt` reads one file). `task-log.jsonl` is an agent-maintained convention; `bwoc check` only tests existence.
- **Dangling refs.** Repointed or removed; no placeholder files created.
- **SKILLS / PLUGINS.** Lifecycle hooks, spawn-time skill resolution, and plugin startup loading (schema validation, init/configure dispatch, missing-plugin refusal) labelled "specified, not enforced by the runtime".
- **SRS.** One implementation-status table by FR group.

## Decisions

- **Relabel, don't delete, the SKILLS/PLUGINS lifecycle.** It is a contract a future loader should honour; deleting it would lose design intent. False *present-tense* claims were the problem, not the spec.
- **Dropped the template manifest `defaults` block** rather than annotate it: JSON cannot carry a "not enforced" comment, and no reader existed (Mattaññutā).
- **`{{maxConcurrentTasks}}` substitution in `new.rs` left in place** — now dead (no template file uses it) but harmless; removing it is a code change outside this docs pass.
- **SRS FR-8.8 removed** — it named a `.claude/commands/new-agent` that never existed.

## Skipped because already implemented

- `bwoc skill verify` resolves `[contract].requires_plugins` against enabled plugin kinds and fails verify (`crates/bwoc-cli/src/skill.rs`, BWOC-45) — marked *implemented* in SKILLS.
- Plugin `[plugin] compat` enforcement (`util::check_plugin_compat`, used by `check.rs` and plugin dispatch in `figma.rs`) — kept as enforced.
- Memory front-matter validation (SRS FR-7.2, `check.rs::check_memory_frontmatter`) and the `MEMORY.md` 200-line warning — kept.

## Status / deferred

- Whether to delete the `persona/`, `mindsets/`, `skills/` slots is a separate decision.
- `trust.md`'s `requiredTrust` scaffolding floor for `bwoc new` is still unimplemented (`new.rs` writes `trust: None`); the doc now says so.
