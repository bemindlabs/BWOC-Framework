# 2026-09-06 — End-to-end render tests for the chat TUI

Added two e2e tests to `bwoc-tui` that exercise the whole render path the
operator sees — the part that previously had **zero coverage** (existing tests
only checked `App` state transitions and the `status_line` string, never a
rendered frame).

## What they cover

A chat_proto JSON line off the wire → deserialized with the **same**
`serde_json::from_str::<ChatEvent>` the reader thread uses → applied to a real
`App` → painted by the real `draw_frame` onto a ratatui `TestBackend` → assert on
the flattened buffer text. The only pieces not exercised are the OS subprocess
and the TTY (non-deterministic by nature); everything from wire bytes to rendered
cells is real.

- `e2e_renders_full_conversation_frame_from_wire` — Ready → tokens → Message →
  ToolCall → ToolResult → TurnEnd, plus a human-banner line that must be dropped
  (not fatal). Asserts the status line (agent/model), the assistant reply, the
  inline tool name, and the operator's in-flight input echo all render — and the
  unparseable banner never reaches the transcript.
- `e2e_permission_prompt_renders_in_input_box` — a `permission_request` takes over
  the input box: title shows `permission:`, the tool, and `[a]llow` / `[d]eny`.

## Notes

- Uses ratatui 0.30's `backend::TestBackend` (no new dep). Two small test
  helpers: `rendered_text` (flatten a frame to text) and `feed_wire` (apply raw
  chat_proto lines the way the reader thread does).
- Deliberately not a subprocess/TTY test: the reader thread is five trivial lines
  and `run()` requires a real terminal, so driving it live would be flaky for no
  added signal.

## Related

- Complements #490 (TUI panic-restore) and #481; source of the render path is
  `crates/bwoc-tui/src/lib.rs` `draw_frame`.
