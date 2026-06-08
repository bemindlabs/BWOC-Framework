# 2026-06-08 — Hotfix: bwoc check audits memory-scaffold completeness

`bwoc check` validated AGENTS.md/symlinks/manifest/neutrality but was silent
on the runtime knowledge-base scaffold (`memories/`, `MEMORY.md`,
`task-log.jsonl`) — so "check passes" gave false confidence that an agent was
fully set up. It also never enforced the `MEMORY.md` ≤ 200-line cap that the
framework `CLAUDE.md` and `WORKSPACE.en.md` both state (Mattaññutā).

## What changed

- `check.rs::audit` — new step 11 `check_memory_completeness(target, mode, memory_dir, report)`:
  - memory directory missing → warning. The directory is the manifest's
    `memoryPath` (default `memories/`), so a non-default config is not false-flagged.
  - `MEMORY.md` missing → warning (incarnation mode only — template ships none)
  - `MEMORY.md` > `MEMORY_MD_MAX_LINES` (200) → warning, counted by streaming
    (`count_lines` via `BufReader`) so a pathological file isn't slurped in; the
    underlying I/O error is surfaced in the warning.
  - `task-log.jsonl` missing → warning (incarnation mode only)
- Tests: `incarnated_agent_warns_on_missing_memory_scaffold`,
  `memory_md_over_cap_warns`, `template_mode_does_not_warn_on_missing_memory_md`,
  `custom_memory_path_is_honored_not_hardcoded`.

## Decisions

- **All advisory warnings, never violations.** A freshly incarnated agent
  legitimately has no memory yet; blocking would punish the normal first state.
  Consistent with the existing `check_symlink_to_agents` "missing = warning" policy.
- **Mode-gated absence.** `MEMORY.md`/`task-log.jsonl` are seeded at runtime, not
  by `bwoc new`; flagging them in template mode would be noise. `memories/` is
  checked in both modes (the template does ship `memories/README.md`).
- No doc edit: `WORKSPACE.en.md` already documents the 200-line cap as convention;
  this PR makes `check` actually surface it rather than restating it.

## Status

Hotfix #2 of the `bwoc new`/`check` user-error trap set (companion to the
symlink-completeness, spawn-TTY-guard, and fallback-warning PRs).
