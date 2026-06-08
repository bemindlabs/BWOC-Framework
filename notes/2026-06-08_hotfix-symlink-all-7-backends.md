# 2026-06-08 — Hotfix: symlink all 7 backends on incarnation + audit

`bwoc new` force-created only 5 backend symlinks (`CLAUDE/AGY/CODEX/KIMI/OLLAMA`) and `bwoc check` audited only 4 of them, yet `modules/agent-template/` ships **7** (`+ COPILOT.md, OPENAI.md`). The CLI was out of sync with the artifact it clones.

## What changed

- New `spawn::BACKEND_ENTRY_FILES` — single canonical list of the 7 backend entry filenames. `new.rs::create_symlinks` and `check.rs::audit` both consume it (check excludes `CLAUDE.md`, handled separately) so the two call sites can no longer drift — the drift that dropped `COPILOT.md`/`OPENAI.md` is the bug this PR fixes.
- `check_symlink_to_agents` now distinguishes "exists but is not a symlink" (stale copied file) from "missing", with a clearer fix hint.
- Tests: `create_symlinks_covers_all_seven_backends` (new.rs); `audit_passes_copilot_and_openai_when_symlinked`, `audit_warns_when_copilot_missing`, `audit_distinguishes_stale_regular_file_from_missing` (check.rs); extended `write_temp_agent` to model all 7.

## Decisions

- Missing backend stays a **warning, not a violation** (existing `check_symlink_to_agents` policy) — agents incarnated before this fix have `OPENAI.md` but no `COPILOT.md`; `check --all` must not turn red. **Samānattatā**: all backends treated equally, but back-compat over a hard gate.
- Belt-and-suspenders: `copy_tree` already preserves the template symlinks on Unix, but `create_symlinks` is the authoritative force-create path and must own the full set (covers broken/partial template links).

## Status

Hotfix #1 of a set surfaced while reviewing common `bwoc new`/`check` user-error traps. Companions (spawn TTY guard, check completeness warnings, fallback-on-vendor warning) ship as separate PRs.
