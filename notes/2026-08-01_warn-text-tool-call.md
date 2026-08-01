# 2026-08-01 — Warn when a model emits a tool call as text (#403)

A weak or mis-templated model sometimes returns a tool call as **plain text**
(no structured `tool_calls`), which both harness loops treated as a final
answer — a silent no-op: no tool ran, no warning, the turn just ended.

## What changed

- `looks_like_text_tool_call` (crate-root `lib.rs`, `pub(crate)`): returns true
  when the content contains an unambiguous tool-call **control marker**
  (`<tool_call>`, `<|python_tag|>`, `<function=`, `[TOOL_CALLS]`, `functools[`).
- **Chat path** (`chat_session.rs`): in the empty-`tool_calls` branch, emit a
  `ChatEvent::Error` warning before the `Message` so the TUI shows it inline.
- **Batch path** (`agent_loop/mod.rs`): `eprintln!` the same warning (no
  frontend there; logs capture it).
- Neither path parses or executes the text — warn only.

## Decisions

- **Marker-based, not "looks like JSON".** Detecting a tool call by "starts with
  `{` and has a name/arguments key" false-positives on legitimate JSON answers.
  Keying on tool-call control tokens — which never appear in normal prose or a
  real JSON reply — keeps false positives ~nil. Tested both ways.
- **Reused `ChatEvent::Error`, no new protocol variant.** A dropped tool call is
  genuinely an error condition (the intended action didn't happen), and the TUI
  already renders `Error` inline. Adding a `Warning` variant to `chat_proto`
  (core + TUI + emit) was heavier than the signal warranted (Mattaññutā).
- **Warn, never execute.** Parsing model free-text into a tool call is an
  injection/ambiguity risk; the fix only makes the failure visible.

## Related (links)

- Issue #403. Found during the fleet-TUI headless verification session.
- Sibling error-message fix: [[2026-08-01_model-not-found-endpoint-hint]] (#402).
