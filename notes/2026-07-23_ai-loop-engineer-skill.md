# 2026-07-23 — `ai-loop-engineer` framework skill

Adds a framework skill (`modules/skills/ai-loop-engineer/`, `domain/autonomy`) for engineering **autonomous agent loops** — the `perceive → act → observe` cycle an agent runs on its own — so it converges, stays bounded, and knows when to stop or escalate. Distinct from `ai-dlc` (the human-steered *development* lifecycle): this builds the *agent's own* iteration engine (harness loops, monitor/retry, self-improvement, budget-bounded sweeps).

## What changed

- New `modules/skills/ai-loop-engineer/` (`manifest.toml` + `SPEC.md`), L1.
  - `design_loop(objective)` — iteration unit · stop condition (done-when + max-iterations + budget ceiling) · guardrails · escalation gate.
  - `tune_loop(running_loop)` — fix non-convergence / runaway cost / oscillation / silent failure on a live loop.
- Grounded in the two failure modes of autonomy — heedlessness + unboundedness — mapped to Appamāda (heedful), Mattaññutā (bounded), Yoniso Manasikāra (verify each pass), Anattā (stop/pivot), + deferred-control at the irreversible gate.
- Correct wikilink depths (PHILOSOPHY → `../../agent-template/docs/en/`, SKILLS → `../../../docs/en/`).

## Related (links)

- `modules/skills/ai-loop-engineer/`; siblings `ai-dlc` / `engineering` / `auditor` / `systems-engineer`.
