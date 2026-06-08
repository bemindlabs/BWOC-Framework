# 2026-06-08 — Hotfix: bwoc spawn fails fast on non-TTY stdin

`bwoc spawn` attaches an interactive backend session (a vendor REPL or the
harness loop) that reads stdin. Run non-interactively (pipe, CI, redirect),
the spawned CLI died with a cryptic backend-side error and no hint that the
real problem was the missing terminal. `spawn()` had no TTY check.

## What changed

- `spawn.rs` — new pure helper `spawn_blocked_by_no_tty(stdin_is_tty, override_set)`;
  `spawn()` calls it right after `validate_agent_path` and returns the new
  `SpawnError::NotInteractive` when stdin is not a TTY.
- Escape hatch: `BWOC_SPAWN_ALLOW_NO_TTY=1` forces a headless spawn (harness
  backends that can run without a REPL, automation that supplies its own I/O).
- `NotInteractive` maps to exit code 2 (a usage error, alongside the other
  pre-flight failures), and its message points to `bwoc run`/`send`/`chat`.
- Tests: `no_tty_guard_blocks_only_without_override`,
  `not_interactive_error_message_is_actionable`.

## Decisions

- **Guard the decision, not the syscall.** The TTY read + env lookup stay in
  `spawn()`; the policy (`!tty && !override`) is a pure fn so CI (where stdin is
  never a TTY) can test it without a real terminal.
- **Escape hatch over a hard wall.** A pure block would break legitimate headless
  harness use; one env var keeps the default safe without a dead end (Mattaññutā —
  the minimum surface that covers the real case).
- Mirrors the existing `bwoc new` precedent (non-TTY = fail-fast via `IsTerminal`).

## Status

Hotfix #4 of the `bwoc new`/`check`/`spawn` user-error trap set.
