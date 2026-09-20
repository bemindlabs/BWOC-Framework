# 2026-09-20 — Runtime R5: in-TUI session switching and Markdown

The last R4/R5 item from the runtime plan: the chat TUI can move between this directory's conversations, and it renders assistant turns instead of showing Markdown source.

## What changed
- `bwoc-tui`: `run()` now loops — spawn harness → `event_loop` → on `Flow::Switch` reopen the harness on another session file, in the same alt screen. `handle_key`/`run_slash` return a `Flow` (`Continue` / `Quit` / `Switch`).
- New commands `/sessions`, `/session <id>`, `/new`, `/fork [<id>]`.
- New `bwoc-tui::markdown` — a small renderer (headings, bullets, quotes, fences, inline code, bold, italic) applied **only** to assistant turns.
- `bwoc-cli::coding_session::TuiSessions` implements the new `bwoc_tui::SessionControl` over `SessionStore`; `runtime.rs` hands one to the TUI.

## Decisions
- **A trait, not a dependency.** `bwoc-tui` must not know the on-disk session layout (it compile-depends on `bwoc-core` only), so the TUI declares `SessionControl` and `bwoc-cli` implements it.
- **Restart, don't multiplex.** One harness per conversation, reopened on switch: no protocol change, and a switch can never leave two sessions writing the same file.
- **Markdown for assistant text only.** A tool result containing `*` is not prose; rendering it would misreport output.
- **No new dependency** for Markdown — a CommonMark crate is far more surface than a transcript pane needs, and unrecognised syntax renders verbatim.

## Status / deferred
- `/fork` with no argument forks the conversation the TUI has open (not merely the newest on disk).
- A fleet pane has no `SessionControl`; the commands report that there.
- Tables, images and nested emphasis are not rendered — they appear as written.
