# 2026-09-21 — A fixed context pane in the chat TUI

The transcript says what was said; nothing said *where you are*. The right pane answers that: directory, branch, workspace, its agents, this session's numbers, and the files tools changed.

## What changed
- New `bwoc-tui::panel`: `gather(workdir)` reads the branch from `.git/HEAD`, finds the nearest `.bwoc/workspace.toml` above the cwd, and lists that workspace's agents.
- `App` caches the panel and re-reads it at session start and on `TurnEnd`; live rows (id, turns, ctx/out, cost, mode, changed files) come from state the TUI already keeps.
- `draw_body` splits off 34 columns when the terminal is ≥ 100 wide and the pane is open; `F3` toggles, and the footer names it only where it can be drawn.

## Decisions
- **No new dependency and no trait this time**: `bwoc-core` already owns `Workspace` and `AgentsRegistry`, so the TUI can read them directly and stay within the dep quarantine.
- **No `git` process.** `.git/HEAD` gives the branch (and a short commit when detached); a worktree's `.git` file is followed. Dirty-state would need real `git`, so it is not claimed.
- **Refresh at turn boundaries**, not per frame — the draw loop must not stat the filesystem 20×/second.
- **Clip, don't wrap.** A pane row is a fact; wrapping one across lines makes the column unreadable.
- **Hidden under 100 columns.** The conversation is what the operator reads; the pane is context, not content.

## Status / deferred
- The fleet TUI keeps its own left sidebar and is unchanged.
- Agents are listed from the registry only (status as declared) — live process state belongs to `bwoc fleet`.
