# 2026-07-31 — Fleet TUI: honest token header

The status header now shows live token usage per pane: current context size,
session output total, and compaction count — all from real `chat_proto` data,
with **no** model→context-window or model→price tables.

## What changed

- `App` gains `total_out: u64` (Σ `completion_tokens` over every `TurnEnd`) and
  `compactions: u32` (incremented on each `Compacted`).
- `status_line` token segment is now `· ctx {p} · out {total_out}` plus `· ⟳N`
  when compactions have occurred. `fmt_tokens` renders `512` / `9.1k` / `1.2M`.
- Covers both single-agent and fleet panes (both render via `draw_status`).

## Decisions

- **No %/cost — absolute counts only.** `TurnEnd` carries `prompt_tokens` +
  `completion_tokens`; nothing in the protocol reports the model's context
  window or price. A true "18% ctx" / "$0.04" would need model→window/price
  tables baked into the TUI — a drift liability the framework rejects
  (Mattaññutā). Deferred until the harness reports the window in `Ready`.
- **`ctx` = latest `prompt_tokens`, not a sum.** Each turn resends the whole
  history, so the newest `prompt_tokens` already *is* the current context size;
  summing it across turns would double-count. Only `completion_tokens` (disjoint
  per turn) is summed, into `total_out`.
- **Chose "honest tokens" over "full tables"** at the user's direction when the
  data gap surfaced — see the P6 fork.

## Status / deferred

- True %-of-window and cost need a harness change (report context window +
  optionally price in `Ready`/`TurnEnd`) — a separate, larger slice.
- `@mention` routing (P4) and inline tool-actions (P5) still pending.

## Related (links)

- [[2026-07-30_tui-per-agent-manifest]] — prior fleet slice.
- [[2026-07-29_tui-fleet-multi-agent]] — the fleet layer.
