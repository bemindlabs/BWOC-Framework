# 2026-07-30 — bwoc-tui P2a: conversation scrollback

First of the TUI "app phases" after fleet mode (#393). The conversation view was tail-only (newest lines, no way back). Adds bottom-anchored scrollback.

## What changed
`App.scroll` (lines up from the live bottom; `0` = live). `App::scroll_key` handles **PageUp/PageDown** (a fixed 10-line step) and **End** (return to live); `draw_conversation` offsets the tail window by `scroll`, clamped so it can't run past either end, and titles the pane `conversation ↑N (End=live)` while scrolled. Sending a message resets to live so the reply is visible. Wired into both the single-agent and fleet key handlers (per-pane in fleet, since `scroll` lives on each `App`). Bottom-relative offset means new streamed content never yanks the view.

## Verification
fmt + clippy clean; `bwoc-tui` tests 12/12 (new `scroll_key_pages_and_end_returns_to_live`). Interactive scroll is not automatable in a non-TTY shell — logic is unit-tested.

## Next phases
P2b mode selector (SetMode plan/build/bypass) · P2c Ctrl-P command palette · P3 per-agent manifest model · @mention routing · inline tool-actions.
