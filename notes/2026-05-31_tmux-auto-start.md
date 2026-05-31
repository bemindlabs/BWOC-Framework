# 2026-05-31 — `bwoc chat --tmux` auto-starts tmux when needed

`bwoc chat --tmux` previously refused outside a tmux session (`not inside a tmux session. Run tmux new-session first…`), forcing a manual two-step. It now auto-starts a session so a bare `bwoc chat <agent> --tmux` from a plain shell just works.

## What changed (`crates/bwoc-cli/src/chat.rs`)

- `open_in_tmux` branches on `$TMUX`:
  - **inside** tmux → `tmux new-window -n <id>` (unchanged — adds a window to the current session).
  - **outside** tmux → `tmux new-session -A -s bwoc-<id> -n <id>` — creates and attaches a dedicated session, or reattaches if `bwoc-<id>` already exists (`-A`).
- Extracted the pure `tmux_launch_args(inside_tmux, id, path, backend)` arg-builder + unit tests for both branches.
- A missing `tmux` binary (`ErrorKind::NotFound`) now prints an install hint — more likely now that we invoke tmux even when the caller wasn't already in it.
- Doc fixes: the `--tmux` clap help (`main.rs`) said "Requires $TMUX" (now false); the `bwoc` banner one-liner and the module header updated too.

## Decisions

- **Scoped to `bwoc chat --tmux`.** The dashboard's `t`/`l`/`i` hotkeys also require `$TMUX`, but the dashboard is a TUI already running in the terminal — it can't wrap itself into tmux retroactively, so its "run tmux first, then re-launch" hint stays correct. The launch-an-agent entry point (`chat`) is where auto-start cleanly applies. *(Mattaññutā — fix the case that actually fits.)*
- **`-A` (attach-or-create), not a fresh session each time.** Re-running `chat --tmux` for the same agent reattaches to the running one instead of erroring on a duplicate session name — the intuitive behaviour.
- **Session name `bwoc-<id>`** — namespaced so it won't collide with the operator's own sessions and is greppable in `tmux ls`.

## Status / deferred

- Dashboard hotkeys still require an existing tmux session (see decision). A detached-session variant for the dashboard (`new-session -d` + "attach with…" hint) is possible later if wanted.

## Related (links)

- `crates/bwoc-cli/src/chat.rs` (`open_in_tmux`, `tmux_launch_args`)
- `crates/bwoc-cli/src/dashboard.rs` (the hotkeys left as hint-only)
