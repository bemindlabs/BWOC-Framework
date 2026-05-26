# 2026-05-26 — HV2-6 budget governance: per-run hard gate (BWOC-8)

Added a per-run token/cost budget that aborts the run when cumulative usage crosses a configured limit — the hard stop atop the existing warn→switch ladder. Sixth workstream in the auto-pilot batch; built after HV2-7 (BWOC-9) closed the streaming-usage gap, so the gate sees tokens on both paths.

## What changed

- New `crates/bwoc-harness/src/budget.rs`: `BudgetConfig { max_tokens, max_cost, cost_per_1m_tokens }` (all-`None` = no gate). `check(total_tokens)` returns `Err(BudgetExceeded)` on the first breached limit (token, then cost); `cost()` derives cost from a price; `is_unlimited()`.
- `error.rs`: new `HarnessError::BudgetExceeded { kind, used, limit }` (non-transient — never retried).
- `agent_loop.rs`: `run_loop` accumulates `total_tokens` from each turn's usage and, right after usage accounting (before tool dispatch / final-answer handling), runs the budget gate — records the turn + checkpoints, then returns `BudgetExceeded`. `LoopConfig.budget: BudgetConfig` (default unlimited).
- `main.rs`: `--token-budget`, `--cost-limit`, `--cost-per-1m` flags feed the config.
- Tests: 4 unit (`budget.rs`) + 2 `run_loop` integration (aborts over budget / completes within). 232 lib tests green; clippy + fmt clean.

## Decisions

- **Gate fires right after usage accounting, before further work.** The model call is already paid for; on breach the loop records that turn (telemetry + checkpoint stay consistent) and aborts rather than dispatching tools or taking another turn. A final-answer turn that tips over budget is also aborted — a hard gate is hard. *Mattaññutā at runtime.*
- **Token budget works everywhere; cost needs a price.** `max_cost` is only enforced with `cost_per_1m_tokens` set — without a price, cost is left *unknown*, not assumed zero (local Ollama has no price). *Yoniso manasikāra — don't fabricate a cost.*
- **`BudgetExceeded` is non-transient.** The retry wrapper must not retry a budget breach.
- **At-the-limit is within budget** (`>` not `>=`): spending exactly the budget is allowed; only crossing it trips.

## Alternatives considered

- A pre-turn budget check — rejected; the meaningful signal is *after* a call's usage is known. (A pre-turn estimate would either over- or under-count.)
- Bundling cost price into a model-pricing table — deferred; a single `--cost-per-1m` is enough until multiple models with different prices run in one process.

## Status / deferred

- Status set to `review` on the workspace board (BWOC-8).
- Per-model pricing (different price per model in a fallback chain) deferred — current cost uses one price for the run.
- Unblocked by HV2-7; the streaming path is budgeted now that usage is exposed there.

## Related (links)

- `notes/2026-05-25_harness-v2-planning.md` — HV2-6 (warn→switch→hard gate; non-streaming first; depends on HV2-7).
- `notes/2026-05-26_hv2-7-streaming-usage-parallel-tools.md` — the dependency (BWOC-9).
- GH #39 (harness-v2 epic, HV2-6). `<workspace>/.scrum/backlog.json` — BWOC-8.
