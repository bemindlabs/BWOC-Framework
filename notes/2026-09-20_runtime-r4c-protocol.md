# 2026-09-20 — Runtime R4c: cancel, set-model, diff and cost

The second of the R4 follow-ups (R4a shipped in 3.3.0). The chat protocol gains the four things a frontend could not do before: stop a turn, change the model, see what a tool changed on disk, and show what the turn cost.

## What changed
- `bwoc-core::chat_proto`: inputs `Cancel`, `SetModel { model }`; events `Cancelled`, `ModelChanged { model }`, `Diff { id, path, diff, truncated }`; `TurnEnd.cost_usd: Option<f64>` (`#[serde(default)]`, skipped when absent — an older frontend keeps parsing).
- `bwoc-harness::chat_session`: a `Pending` inbox (queued lines + cancel latch); `stream_call` selects on stdin while the provider streams; cancel is honoured mid-stream, at each tool-call boundary and during a permission prompt; `ModelChain::set_active`; provider-reported cost accumulated per session.
- New `bwoc-harness::diff` — dependency-free unified diff with LCS alignment, 3 lines of context, an 8 KB byte cap and a coarse hunk above 1,000 changed lines.
- `bwoc-tui`: `Esc` cancels while busy, `/model`, diff rows rendered with `+`/`-` colouring, cost in the status line.

## Decisions
- **Cancel is a latch, not a kill.** Dropping a provider future mid-stream is safe (nothing reached `history`); killing a tool mid-execution is not, so a cancel during a tool waits for that call's result. Documented in the protocol docs.
- **`Lines::next_line` is cancel-safe**, so reading stdin inside `select!` cannot lose a partial line. Anything read ahead that is not a `Cancel` is queued and handled in order — a race never drops input.
- **Cost is provider-reported only.** A local price table would be a fabricated number the moment a model's price changes; the status line shows nothing when the provider reports nothing.
- **Diffs are display-only and bounded**, and only for the three file-mutating tools with a resolvable in-workdir `path`.

## Status / deferred
- A cancel during a long `run_command` lands when that command returns, not while it runs.
- `diff` does not cover a file changed by `run_command`; only the file tools report one.
