# 2026-05-30 — `primaryModel: "auto"` model selection

Agents can now declare `primaryModel: "auto"` in `config.manifest.json` and let the harness pick a concrete model at runtime from an `autoModels` candidate pool, scored against the live provider. Scoped to harness-backed backends (`ollama`, `openai-compatible`) — vendor CLIs (Claude/Codex/Kimi) self-select their model, so `"auto"` is a no-op there.

## What changed

- **`bwoc-core` manifest** — new optional `autoModels: Option<Vec<String>>` field (`autoModels` on the wire). Ordered by operator preference; required (and only meaningful) when `primary_model == "auto"`. `primaryModel` stays a `String`; `"auto"` is a sentinel, not a new type.
- **`bwoc-harness` provider trait** — new `ProviderClient::list_models()` (default `vec![]`; Ollama impl parses `GET /v1/models`). Empty result ≡ "availability unknown".
- **`bwoc-harness::model_select` (new module)** — `resolve_auto(provider, candidates, task) -> AutoSelection`. Deterministic pipeline over four criteria: availability filter → context-fit filter (`model_context_limit` vs `estimate_task_tokens`) → task class (`classify_task`: EN/TH keyword + length heuristic) → cost (candidate order is the cost axis). Returns the chosen model plus `remaining` (preference order) and probed `context_limits`.
- **`bwoc-harness` main** — when `--model auto`, loads `autoModels` from the workdir manifest, resolves, and wires the by-products into the previously-empty `LoopConfig.fallback_models` / `token_pressure_models` / `model_context_limits`.
- **`bwoc-harness` error** — new `NoAutoCandidate { reason, candidates }`.
- **Template schema** — documented `primaryModel: "auto"` + `autoModels` in `modules/agent-template/config.manifest.json`.

## Decisions

- **Resolution lives in the harness, not `bwoc-cli/run.rs`.** All four criteria need the live `ProviderClient` (availability, context limits) and the task text — only the harness has both. `run.rs` passes `"auto"` through unchanged. *(Yoniso Manasikāra — site chosen from where the data actually exists.)*
- **Reuse, don't expand, `LoopConfig`.** The fallback/context/token-pressure fields already existed but were hardcoded empty at the harness call site. Auto-selection populates them from real probe data instead of adding parallel config. *(Mattaññutā — one new module + one manifest field, no new config subsystem.)*
- **Candidate order is the cost axis.** Avoids per-model cost/capability metadata in the manifest. Heavy tasks pick largest-context; light tasks pick last-in-order (cheapest). Unknown context limit ≠ disqualified; all-too-small falls back to the largest window rather than erroring.
- **`bwoc new` was NOT extended** with an `--auto-models` flag. Operators opt in by hand-editing the manifest. *(Over-Engineering Protection — no CLI surface added without explicit ask.)*

## Alternatives considered

- **Task-aware LLM classifier** instead of a keyword heuristic — rejected for v1 (extra provider call, non-deterministic, untestable offline). The heuristic + length threshold covers the common heavy/light split.
- **Per-candidate cost/capability metadata in the manifest** — rejected as bloat; preference order encodes the same intent with one field.

## Status / deferred

- Heuristic `classify_task` keyword list is EN/TH only; extend as agents are themed in other languages.
- `estimate_task_tokens` is a ~4-chars/token approximation with fixed headroom — fine for window-fit gating, not for billing.
- No `--auto-models` CLI flag on `bwoc new` (see decision above).
- **Reached via `bwoc run` (and direct `bwoc-harness` invocation) only — not `bwoc spawn`.** `bwoc spawn` does not forward `manifest.primary_model` to the harness as `--model` (it relies on the harness reading its own default), so `primaryModel: "auto"` never reaches the resolver through `spawn`. This is a pre-existing `spawn` gap (it ignores `primaryModel` generally for harness backends), broader than this feature; forwarding the model in `spawn.rs` is tracked as a separate follow-up rather than widened into this PR.
- **`--resume` does not re-resolve.** A resumed run reuses the model recorded in its checkpoint (`RunState.active_model`); re-resolving with no `--task` would reclassify the work as Light and could swap the run onto a smaller model mid-history. Auto-resolution and startup model-validation are both skipped on resume.

## Related (links)

- `crates/bwoc-harness/src/model_select.rs`
- `crates/bwoc-harness/src/agent_loop.rs` (LoopConfig fields now fed by auto-selection)
- `crates/bwoc-core/src/manifest.rs`
