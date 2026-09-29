# 2026-09-29 — PgUp/PgDn recall sent lines in the chat TUI

The owner asked for `PgUp`/`PgDn` to bring back earlier chat input instead of scrolling. In `bwoc-tui` they now step through the lines already sent from that input, like shell history. Scrolling stays on `↑`/`↓`, the wheel and `End`.

## What changed
- `App` gains `sent_inputs` (oldest first, capped at `INPUT_HISTORY_MAX = 100`), `recall` (the position, `None` = live line) and `recall_draft`.
- `take_input` records every non-blank sent line and skips an immediate repeat. It is the single send path for both the single-agent view and fleet panes, so no call site needed to change.
- New `recall_key`, checked before `scroll_key` in both key handlers. Side panes reuse `handle_key`, so they get it too. `scroll_key` no longer handles the page keys.
- Footer: `PgUp/PgDn recall · End`. `/help` lists the keys. `crates/bwoc-tui/README.md` is updated.

## Decisions
- **History is per input, in memory.** Each pane recalls its own lines. Nothing is written to disk: the conversation store already persists what was said, and a second store would be a new file format for little gain (Mattaññutā).
- **The draft is kept.** `PgDn` past the newest entry restores the half-typed line, so browsing history never loses work.
- **Recalled text is editable and is not re-recorded until sent.** Editing a recalled line doesn't change history; sending it records it as a new entry (if different from the last).
- **Footer budget.** The footer must keep `Ctrl-C exit` visible at 80 columns (`e2e_renders_full_conversation_frame_from_wire`). "End live" became "End" to make room; the conversation title still shows `(End=live)` while scrolled.

## Alternatives considered
- Recall on `↑`/`↓` when the input is empty (the common shell/REPL binding). Rejected: `↑`/`↓` already scroll here and drive the `/`/`@` popup, and the owner asked for the page keys specifically.

## Related
- `crates/bwoc-tui/src/lib.rs` (`recall_key`, `take_input`, tests `page_keys_recall_sent_lines_and_restore_the_draft`, `sent_lines_are_capped`).
