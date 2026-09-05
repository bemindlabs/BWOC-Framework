# 2026-09-05 — Recover operator text typed during a permission prompt

Closes #480. While a tool's `ask` permission prompt was pending,
`read_permission` treated **any** non-permission `User` line as a bare deny
(`_ => return Ok(false)`) and discarded its text. The frontend echoes
`you: <text>` locally, so the operator believed it was sent — but the model never
saw it and no `Error` was emitted. The text was simply gone.

## What changed

- `read_permission` returns `bool` → `enum PermissionOutcome { Allow, Deny,
  DenyWithUserText { text, principal } }`. A `User` line mid-prompt still denies
  the tool (fail-safe) but **recovers** its text + ingress principal.
- `dispatch_call` gains an `interjection: &mut Option<(String, Principal)>`
  out-param; on `DenyWithUserText` it sets it and returns the denial as the tool
  result (so the `tool_call` still has a well-formed `tool_result`).
- `run_turn` threads the same out-param and, once an interjection is captured,
  stops the turn **at the batch boundary** (every `tool_call` already has its
  `tool_result`, so history stays valid) and returns.
- The main loop's `User` arm now loops: after each turn it replays a captured
  interjection as the next user turn — through the existing `ChatMessage::ingest`
  path, so the Principal clamp (trust handling) is unchanged.

## Decisions

- **Stop the turn on interjection**, don't finish it. The operator interrupted;
  continuing the model's plan and *then* handling their message would be
  surprising. Stopping at the batch boundary keeps the OpenAI tool-call/result
  ordering invariant intact.
- **Fail-safe deny is preserved** — the recovered text never turns a denial into
  an execution; it only stops the text from vanishing.

## Tests

- `user_text_during_permission_prompt_denies_then_becomes_next_turn`: a `user`
  line sent while a `write_file` prompt is pending ⇒ tool denied (`ToolResult
  ok=false`, file not written) AND the interjected text becomes the next turn
  (its final `Message` appears; two `TurnEnd`s — aborted + replay).

## Related

- Issue #480; source `research/2026-08-23_grok-build-comparison.md`.
