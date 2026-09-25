# 2026-09-25 — Click an input box to focus its pane

The owner reported that clicking a pane did not select it. The TUI had no mouse support at all. The owner chose the scope: a click on the **input box** only.

## What changed

- `set_mouse(on)` writes `?1000h ?1006h` (press/release, SGR coordinates) or turns them off. The chat event loop enables it while `panes.side` is non-empty. `restore_terminal` always turns it off, which is harmless when it was never on.
- `pane_input_at(screen, layout, count, focus, col, row)` returns the pane whose input box contains the cell. It uses the same geometry as `draw_panes`/`draw_pane`: panes above the one-row footer, the border, then the last three inner rows.
- `Event::Mouse` handling: `Down(Left)` on an input sets `panes.focus`. The wheel maps to `scroll_key(Up/Down)` on the focused pane.

## Decisions

- **Mouse capture only while panes are open.** Any mouse tracking takes drag-to-select away from the terminal. The single-session TUI advertises select/copy, so it keeps that. With panes open, Shift-drag (Option on macOS) selects.
- **Mode 1000, not crossterm's `EnableMouseCapture`.** That call also turns on 1002/1003 motion tracking, which floods events while the pointer moves.
- **The wheel is handled.** It isn't a new feature: with capture on, the terminal no longer converts the wheel to ↑/↓, so without this, scrolling would regress.

## Verification

- Test `a_click_lands_only_on_a_pane_input_box`: the input rows, the transcript, the border and the footer.
- tmux, real TUI: with no panes, `#{mouse_standard_flag}#{mouse_sgr_flag}` = `00`; after `/agents agent-helper` it is `11`. A click on the main input followed by typing lands in main, and the same for the side input. A click on the transcript leaves focus where it was. Wheel-up scrolls the focused pane.
