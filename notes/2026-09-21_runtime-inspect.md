# 2026-09-21 — `/compact`, `/permissions`, `/mcp`, `/context`

The last group from the slash-command gap pass: the four questions that needed the harness to answer, not the TUI.

## What changed
- Protocol: `ChatInput::Compact`, `ChatInput::Describe { topic }`, `ChatEvent::Described { topic, rows }`.
- `bwoc-harness::chat_session::describe()` answers `permissions`, `mcp` and `context`; `Compact` runs `compact::compact_context` on demand and emits the existing `Compacted`.
- `bwoc-tui`: the four commands, and a renderer for `Described` rows.

## Decisions
- **One `Describe` input for every read-only topic** instead of one variant per question (Mattaññutā): a future topic is a new string, and an old frontend that asks for an unknown one gets an `Error` naming the valid topics.
- **`/compact` reuses the budget path** rather than a second summarizer — one compaction implementation, one behaviour.
- **`/mcp` reads the registry**, not a config field: tools register as `mcp__<server>__<tool>`, so what the model can actually call is what gets listed. A server that failed to connect contributes nothing and is therefore absent, which is the honest answer.
- **No secrets are reachable**: the policy holds modes and patterns, and MCP tokens live in the transport, never in the registry.

## Status / deferred
- `/permissions` is read-only; changing a rule is still a `.bwoc/harness-policy.toml` edit (`/mode` covers the session-level switch).
- `/context` reports sizes, not the text; dumping the system prompt into the transcript would push the conversation out of view.
