# 2026-07-22 — `ai-dlc` framework skill (AI-Driven Development Life Cycle)

Adds a methodology framework skill (`modules/skills/ai-dlc/`) — the *meta-skill* that sequences the craft skills across a full lifecycle: an agent takes a human **intent**, elaborates it into an agreed **plan**, then drives **build → verify → document → operate**, with the human steering at bounded checkpoints rather than typing every line.

## What changed

- New `modules/skills/ai-dlc/` (`manifest.toml` + `SPEC.md`), `domain/methodology`, L1.
  - `plan_increment(intent)` — Inception: elaborate intent → requirements + right-sized plan + acceptance bar; **stop for human approval before any build**.
  - `execute_increment(plan)` — Construction + Operation: build → verify → document → operate, pausing at each **bounded, irreversible gate** for the framework's operator-confirm (merge/deploy/destructive).
- `bwoc skill list` + `bwoc skill verify ai-dlc` pass (static).

## Decisions

- **Mapped AI-DLC onto BWOC's real grain, not the generic slideware.** The fleet has no bespoke ai-dlc doc (checked local workspaces + the Second Brain), so the skill grounds AI-DLC's principles — agent drives, human steers, small increments, explicit gates — in mechanisms the framework already has: `Sīla` gates, deferred-control / operator-confirm at irreversible steps, `Saṅgha` (mob), and the `uppāda/ṭhiti/vaya` arc applied to a work increment.
- **Meta-skill, not a re-implementation.** The phases *delegate* to the craft skills (`manager` → plan, `engineering` → build, `auditor` → verify, `documenter` → document) via See-Also composition; `ai-dlc` sequences them and places the checkpoints. `requires = []` (soft composition, not a hard dep) so it enables standalone.
- **Human at the gates, not the keystrokes** — the load-bearing AI-DLC inversion, expressed as: intent approved before build (checkpoint 1) + operator-confirm at each irreversible gate.

## Status / deferred

- L1; not enabled on any agent yet. Pairs naturally with `engineering/auditor/documenter/manager` on an agent that runs full increments.

## Related (links)

- `modules/skills/ai-dlc/`; composes `modules/skills/{manager,engineering,auditor,documenter}/`; `modules/skills/worktree-discipline/`.
