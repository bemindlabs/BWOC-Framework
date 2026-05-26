# 2026-05-26 — HV2-3 run-end retrospective (BWOC-6)

Closed the Paññā-3 loop in the harness: at run end the binary now reads its own `SessionRecord` back and surfaces the AGENTS.md §8b self-improvement triggers. Until now telemetry was write-only (emitted, never read) and `eval` ran only offline — there was no live feedback path. Second harness-v2 workstream; independent of HV2-2, built in the same auto-pilot batch.

## What changed

- New `crates/bwoc-harness/src/retrospective.rs`:
  - `RetroThresholds` (defaults 70% / 70%, mirroring AGENTS.md §8b verbatim).
  - `Trigger` enum — `LowCompletionRate` / `LowGatePassRate`, each carrying the observed rate + threshold; `suggestion()` returns the Paññā-staged next move (Sutamayā re-read / Cintāmayā synthesise / Bhāvanāmayā save feedback memory).
  - `Retrospective::analyze(&SessionRecord, &RetroThresholds)` — pure; computes the two §8b rates, fires triggers below threshold, returns observations + triggers. `render()` for stderr.
  - 4 unit tests: low-gate-pass fires, healthy run fires nothing, low-completion fires, zero-denominator skips without a false trigger.
- `main.rs`: on a successful run, set `tasks_attempted`/`tasks_completed += 1` (gives the completion-rate trigger a denominator), then after `telemetry.finish()` run `Retrospective::analyze(&telemetry.build_record(), &default)` and print `render()` to stderr.
- `lib.rs`: `pub mod retrospective;`.
- 211 lib tests green; clippy + fmt clean; bin builds.

## Decisions

- **Observe-don't-drive is structural, not just documented.** `Retrospective` holds only text (observations + suggestions); it has no handle to `Policy`, the prompt, or run state, so it *cannot* mutate them even by mistake. The safety pipeline stays the single authority. *Satisfies the BWOC-6 AC "surfaces adjustments, does not silently mutate policy."*
- **Zero denominators skip, not score 0%.** A run with no gates did not *fail* its gates; reporting 0% would fire a false trigger. `analyze` emits an `n/a` observation instead. *Yoniso manasikāra — read the data honestly, don't manufacture a signal.*
- **Single-run scope, with the §8b "5+ sessions" caveat surfaced.** §8b intends the thresholds over an aggregate of 5+ sessions; a per-run retrospective can't have that history, so when a trigger fires the output adds "treat a single-run trigger as a hint, not a verdict." Cross-session aggregation (and the "same correction 3+ times → consolidate" trigger, which needs history) is deferred.
- **Reused the existing `SessionRecord` / `build_record()` — no new telemetry plumbing.** The record the harness already builds for `session-metrics.jsonl` is exactly the retrospective's input. *Mattaññutā — read back what already exists rather than add a parallel data path.*

## Alternatives considered

- Feeding `eval` results into the live retrospective — `eval/mod.rs::run_fixture` is offline-only (no eval signal exists during a real run), so the live path is telemetry-based; eval remains the offline counterpart. Not wired this workstream.
- A configurable thresholds source (TOML) — deferred; `RetroThresholds::default()` matches §8b and is the only caller for now. Add config only when an operator needs to tune it.

## Status / deferred

- Status set to `review` on the workspace board (BWOC-6).
- Deferred: cross-session aggregation and the "repeated correction" consolidation trigger (both need multi-session history); optional thresholds config; eval-signal feed.
- No docs touched, so no EN/TH parity work; thresholds were read from `modules/agent-template/AGENTS.md §8b`, not modified.

## Related (links)

- `notes/2026-05-25_harness-v2-planning.md` — epic plan + HV2-3 seams (telemetry.rs:71/170, main.rs:186, eval/mod.rs:183).
- `notes/2026-05-26_hv2-2-durable-runs.md` — sibling workstream (BWOC-5).
- GH #39 (harness-v2 epic, HV2-3). `<workspace>/.scrum/backlog.json` — BWOC-6.
- AGENTS.md §8b (thresholds), §11 + `docs/en/SELF-IMPROVEMENT.en.md` (Paññā-3 triggers).
