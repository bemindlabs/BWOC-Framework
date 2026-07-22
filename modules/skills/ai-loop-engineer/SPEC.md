---
title: AI Loop Engineer
aliases:
  - ai-loop-engineer
tags:
  - group/framework-skills
  - type/skill
  - domain/autonomy
maturity: L1
---

# AI Loop Engineer ♻️

> [!abstract] The craft of engineering an **autonomous agent loop** — the `perceive → act → observe` cycle an agent runs on its own — so it *converges, stays bounded, and knows when to stop or ask a human*. Where [[../ai-dlc/SPEC|ai-dlc]] is the human-steered development lifecycle, `ai-loop-engineer` builds the *agent's own* iteration engine (harness loops, monitor/retry loops, self-improvement loops, budget-bounded sweeps). Encodes **Appamāda** (a loop must run *heedful*, never blind), **Mattaññutā** (every loop is bounded — budget + max-iterations), **Yoniso Manasikāra** (each iteration verifies against reality), and **Anattā** (no clinging to a stuck approach — stop or pivot).

## What This Skill Does

Autonomy lives in the loop — and so does the runaway. An agent loop that never converges spins forever; one with no budget burns the pool; one that swallows errors fails silently. This skill wraps the discipline of building loops that terminate, cost what they should, and escalate when they can't finish.

Two operations are exposed:

- **`design_loop(objective)`** — define the loop before running it: the **iteration unit** (what one pass does), the **stop condition** (done-when + a hard `max-iterations` + a **budget ceiling**), the **guardrails** (what the loop may do autonomously vs. what needs confirmation), and the **escalation gate** (when to hand to a human instead of iterating again). A loop without a stop condition is a bug, not a feature.
- **`tune_loop(running_loop)`** — observe a live loop and correct it: **non-convergence** (spinning without progress → add a completeness signal or a "K quiet rounds → done" counter); **runaway cost** (bound to a token/time budget, degrade or stop at the ceiling); **oscillation** (detect the repeat, break it); **silent failure** (surface the error to the operator, never swallow it). Watch the loop's signals and adjust the bounds.

## Why It Exists

The two failure modes of autonomy are *heedlessness* and *unboundedness* — a loop that acts without checking, or one that never stops. **Appamāda** is the loop that verifies each pass; **Mattaññutā** is the loop that is *bounded* (a budget ceiling and a max-iteration count are non-optional); **Yoniso Manasikāra** is each iteration checking reality rather than its own assumption; **Anattā** is the willingness to abandon a non-converging approach instead of clinging. And because an autonomous loop can produce irreversible side-effects, the **deferred-control / operator-confirm** principle sits at the gate: the loop automates the reversible, and asks a human at the irreversible. Centralising this as a skill keeps every loop an agent builds honest about its stop condition, its budget, and its escalation.

Working rules:

1. **Every loop has a stop condition.** Done-when + `max-iterations` + a budget ceiling — all three.
2. **Bound the budget.** A loop that can't name its cost ceiling isn't ready to run.
3. **Each iteration verifies.** Check progress against reality every pass (Yoniso Manasikāra), or the loop is spinning blind.
4. **Detect no-progress + oscillation.** "K consecutive quiet rounds → stop"; break a detected repeat.
5. **Never swallow failure.** Surface errors to the operator; a silent loop is the worst loop.
6. **Escalate at the irreversible.** Automate the reversible; hand the irreversible / stuck to a human (deferred-control).

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `design_loop` | `objective` | A loop spec: iteration unit · stop condition · budget · guardrails · escalation gate | Pure — refines the spec, runs nothing |
| `tune_loop` | a `running_loop` + its signals | Adjusted bounds / added convergence + failure handling | Converges — a well-tuned loop needs no further tuning |

`design_loop` is pure design. `tune_loop` observes and adjusts a loop's *configuration*; the loop's own side-effects stay behind the guardrails + escalation gate it defines, so the operator-confirm lives where the loop touches the irreversible, not bypassed.

## Lifecycle Mapping

```
init       → capture the objective + the acceptable budget/blast-radius
invoke     → design_loop (bounded + escalation gate) → run → tune_loop (converge / bound / surface)
teardown   → stop the loop on done / budget / escalation (no clinging — Anattā)
```

## Maturity

**L1**. → L2 once two loops have run to a clean stop condition under budget with escalation honored; → L3 once `bwoc skill verify ai-loop-engineer` is wired + green.

## Neutrality

Names no backend/model/vendor; loop discipline is runtime-agnostic. The budget + gates are the operator's / framework's own. Satisfies **Samānattatā**.

## See Also

- [[../ai-dlc/SPEC|ai-dlc]] — the human-steered *development* lifecycle; this skill builds the *agent's own* loop inside it.
- [[../engineering/SPEC|engineering]] — implements the loop; [[../auditor/SPEC|auditor]] verifies it terminates + stays bounded.
- [[../systems-engineer/SPEC|systems-engineer]] — a loop is a system; the failure-mode + observability stance is shared.
- [[../../agent-template/docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Appamāda, Mattaññutā, Anattā, deferred-control.
- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
