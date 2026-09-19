# 2026-09-19 — Chat pane controls

The chat TUI now reserves `Ctrl-C` for exit, exposes its controls in a footer, scrolls the transcript with vertical arrow keys, edits at a movable horizontal cursor, and leaves idle frames untouched so native terminal selection can be copied reliably.

## What changed

- `q` is always typed into the prompt and `Esc` leaves the session open; only `Ctrl-C` exits.
- `↑` / `↓` move the transcript one rendered row, while `PgUp` / `PgDn` retain ten-row paging and `End` returns to the live bottom.
- `←` / `→` move through the input by Unicode scalar value; typing inserts at the cursor and Backspace removes the preceding character.
- Single-agent and fleet layouts show the same persistent key footer.
- Both event loops redraw only after input or session state changes. This prevents an idle 20 Hz repaint from disrupting terminal text selection.

## Decisions

Copying stays with the terminal's native selection and clipboard rather than adding a platform-specific clipboard dependency or sending clipboard escape sequences.
