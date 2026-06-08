# 2026-06-08 — Hotfix: symlink all 7 backends on incarnation + audit

`bwoc new` force-created only 5 backend symlinks (`CLAUDE/AGY/CODEX/KIMI/OLLAMA`) and `bwoc check` audited only 4 of them, yet `modules/agent-template/` ships **7** (`+ COPILOT.md, OPENAI.md`). The CLI was out of sync with the artifact it clones.

## What changed

- `new.rs::create_symlinks` — added `COPILOT.md` and `OPENAI.md` to the force-created list (now 7).
- `check.rs::audit` — added `COPILOT.md` and `OPENAI.md` to the backend-symlink audit loop; updated `check_symlink_to_agents` doc comment.
- Tests: `create_symlinks_covers_all_seven_backends` (new.rs); `audit_passes_copilot_and_openai_when_symlinked` + `audit_warns_when_copilot_missing` (check.rs); extended `write_temp_agent` to model all 7.

## Decisions

- Missing backend stays a **warning, not a violation** (existing `check_symlink_to_agents` policy) — agents incarnated before this fix have `OPENAI.md` but no `COPILOT.md`; `check --all` must not turn red. **Samānattatā**: all backends treated equally, but back-compat over a hard gate.
- Belt-and-suspenders: `copy_tree` already preserves the template symlinks on Unix, but `create_symlinks` is the authoritative force-create path and must own the full set (covers broken/partial template links).

## Status

Hotfix #1 of a set surfaced while reviewing common `bwoc new`/`check` user-error traps. Companions (spawn TTY guard, check completeness warnings, fallback-on-vendor warning) ship as separate PRs.
