# 2026-08-01 — Fleet TUI: inline tool-actions (single-column transcript)

Tool calls, tool results, permission prompts, and errors now render **inline in
the conversation** as one coherent transcript. The separate tools/activity side
pane is removed; the conversation is full-width.

## What changed

- `App::apply` routes `ToolCall` (`→ …`), `ToolResult` (`✓/✗ …`),
  `PermissionRequest` (`⚠ … [a]llow / [d]eny`), and `Error` (`✗ …`) into
  `conversation` instead of `activity`. The a/d handlers (single-agent + fleet)
  and the session-spawn failure line follow suit.
- The `activity` field, `draw_activity`, and the horizontal body split are gone;
  `draw_body` renders `draw_conversation` full-width.
- `transcript_style` colors the inline markers (⚠ warning, ✗ danger, ✓ success,
  →/●/📢 dim) so tool actions stand out from plain turns in the shared column.
- The permission prompt shows `[a]llow / [d]eny` inline (the input border still
  echoes the pending state).

## Decisions

- **Removed the tools/activity pane** rather than duplicating events across two
  panes. This is the notable, reviewable design change: it realizes the
  OpenCode-style single transcript the fleet work targeted, and drops a pane that
  became redundant once actions are inline. Easily reverted if the split is
  preferred. Chosen over "inline permission only, keep the pane" because that
  path duplicates lines or leaves tool calls stranded off-transcript.
- **No mouse affordance.** The TUI is keyboard-only by design; the inline prompt
  surfaces the existing `a`/`d` keys where the operator reads, rather than adding
  click targets that wouldn't work.

## Status / deferred

- Closes the named TUI phase plan (P1–P6). Further ideas (collapsible long tool
  output, per-tool color) are unrequested — not added (Mattaññutā).

## Related (links)

- [[2026-07-31_tui-mention-routing]] · [[2026-07-31_tui-header-tokens]]
- [[2026-07-29_tui-fleet-multi-agent]] — the fleet layer.
