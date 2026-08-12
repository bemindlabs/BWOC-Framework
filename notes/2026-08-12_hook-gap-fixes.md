# 2026-08-12 — Agent hook gap fixes (security, git-hardening, neutrality, docs)

A four-track multi-agent audit of BWOC's agent **hooks** found gaps across every
layer: the backend-neutral `.bwoc/hooks/` task-hook mechanism, backend-specific
`.claude/hooks/`, the harness runtime (no general hook system), and hook-execution
security. This session closes the four highest-ROI, well-scoped gaps.

## What changed (four focused PRs)

1. **Harden `.bwoc/hooks/` exec (security) — `bwoc-cli/src/sangha.rs`.**
   - **G1 (HIGH):** `run_task_hook` no longer inherits the operator's full
     environment (which leaked `GITHUB_TOKEN`/`SSH_AUTH_SOCK`/cloud creds to a
     planted executable). It now `env_clear()`s and passes only a safe base
     (PATH/HOME/…) + the `BWOC_*` context — matching the scrub `extra_tools.rs`
     and `result.rs` already do.
   - **G3 (defense-in-depth):** the `event` name is validated to a single kebab
     segment, so a future caller forwarding agent/user input can't traverse out
     of `.bwoc/hooks/` (`..`/absolute would let `Path::join` escape).
2. **`GIT_HARDENING` on parent-side git (G4) — `bwoc-harness` worker.rs/lead.rs.**
   Apply the same `-c core.hooksPath=/dev/null …` + `GIT_CONFIG_GLOBAL/SYSTEM`
   scrub that `result.rs` uses, to the parent-privilege `worktree add/remove`
   git calls — closing a C7 (planted `core.fsmonitor`/`hooksPath`) bypass.
3. **`bwoc check` flags non-neutral `.claude/hooks/` (Track 3) — `bwoc-cli/src/check.rs`.**
   The neutrality audit never opened `.claude/`, so a Claude-only load-bearing
   behavior (inbox auto-reply Stop hook) passed clean. Now warn when an agent
   ships a `.claude/hooks/` behavior with no backend-neutral equivalent.
4. **Document the hook event catalog (Track 1) — `interconnect/sangha.md` (+ th).**
   The docs listed only `task-created`/`task-completed`; add `task-claimed` (and
   its `BWOC_WORKTREE_BASE`), the full env-var schema per event, the env-scrub +
   blocking-veto contract.

## Decisions
- **Scrub, don't allowlist-per-hook** — a fixed safe base is simpler and safe;
  a hook needing more reads it from a file, not ambient env.
- **Validate event as a kebab segment** (not a general path check) — the events
  are a closed kebab set; the strict charset is the simplest correct guard.

## Status / deferred (need design, not a fix)
- **Harness general hook system** — no PreToolUse/**PostToolUse**/SessionStart/
  Stop/PreCompact; tool *results* are trusted verbatim (no redaction/injection
  scan). A real hook bus is a design-level feature.
- **Backend-neutral inbox reply loop** — the turn-end reply mirror is Claude-only
  (`.claude/hooks/inbox-auto-reply.sh`); non-Claude backends silently never close
  the loop. `autoprocess.rs` covers only the remote/gateway path. Generalizing it
  to the local/trusted path is the fix, but a design choice.
- **G2 (provenance/trust-gated hook exec)** + **G6/G7/G8** (supply-chain hook
  threat, T0-T3 gate, unsigned template hook) — threat-model + trust-model work.

## Related
- Investigation: 4 parallel Explore agents (neutral hooks / backend-specific /
  harness runtime / security).
