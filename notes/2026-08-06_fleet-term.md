# 2026-08-06 — `bwoc fleet term`: tiled fleet terminals + cross-pane messaging

New command that opens a terminal for **every** agent in the fleet, tiled in one
tmux session with selectable layouts — and where agents can message each other
across the tiles while the human watches.

## What changed

- **`bwoc fleet term [--layout grid|columns|rows|main-vertical|main-horizontal] [--session <name>] [--print]`** (`crates/bwoc-cli/src/fleet_term.rs`). One tmux pane per registered agent (each running `bwoc spawn` in the agent's dir), arranged by a built-in tmux layout. Portable macOS + Linux (tmux is the shared multiplexer); `<prefix> Space` cycles layouts live.
  - Panes are **titled by agent id** (`pane-border-status top`) — legible *and* addressable.
  - Rebalances to `tiled` after each split so a large fleet never hits "no space for new pane".
  - `remain-on-exit on` so an agent that exits leaves an `[exited]` pane instead of collapsing the layout.
  - Pure `tmux_fleet_commands` builder + 6 unit tests.
- **Cross-pane wake** (`send.rs`): `notify_tmux` now resolves a recipient to its **pane** (matched by title → `%N`) when no whole session matches, so `bwoc send agent-X` wakes agent-X's *tile* in the shared session — not just whatever pane is active. Pure `match_pane_by_title` + test.

## Decisions

- **tmux, not OS windows.** The request said macOS + Linux first; tmux `select-layout` gives the arrangements identically on both. Ghostty / Terminal.app window tiling is a mac-only follow-up.
- **`bwoc spawn` per pane, not `bwoc chat`.** spawn execs the backend CLI (claude) in the agent dir → the agent's `.claude` Stop hook loads, so the bus auto-reply ([[2026-08-06_inbox-auto-reply-stop-race]]) works inside the tiles.
- **Wake by pane title, reusing the session-name candidate set.** No new naming scheme; the same `agent-<x>` / bare `<x>` candidates match a pane title.

## Verified e2e (bemind)

- Layout: `bwoc fleet term` builds N titled panes; `--layout` applies (grid/columns/…).
- **Cross-pane messaging:** 3 panes titled `agent-{aaa,bbb,ccc}`; `bwoc send aaa→bbb` landed in **bbb's pane only** (marker present), `aaa→ccc` in **ccc's pane only**, `aaa`'s own pane untouched — the wake reaches the correct tile.
- End-to-end interaction = this + the verified claude auto-reply ([[2026-08-06_inbox-auto-reply-stop-race]]): a peer message wakes the right tile, the agent replies, the human sees the exchange in place.

## Deferred

- OS-native window tiling (Ghostty/Terminal.app), and a `--windows` (one tmux window per agent) mode. Not needed for the mac+linux tiled use case.
