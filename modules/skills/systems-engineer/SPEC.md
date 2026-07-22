---
title: Systems Engineer
aliases:
  - systems-engineer
tags:
  - group/framework-skills
  - type/skill
  - domain/systems
maturity: L1
---

# Systems Engineer 🧩

> [!abstract] The craft of designing and holding together **the whole system** — how components, services, and infrastructure fit, the interfaces + constraints between them, and how the thing stays reliable under load, faults, and change. Where [[../software-engineer/SPEC|software-engineer]] designs *a* component, `systems-engineer` designs the *composition*. Encodes **Paṭicca-samuppāda** (dependent origination — nothing stands alone; a change here surfaces there) and **Sammā-diṭṭhi** (see the whole, not the part).

## What This Skill Does

Two operations are exposed:

- **`architect_system(goal)`** — design the composition: decompose the system into components with clear responsibilities, define the interfaces + data flow between them, name the constraints (latency, throughput, consistency, cost, security boundary), and record the trade-offs the shape makes. The output is a system design that says *why* it's shaped this way.
- **`assure_reliability(system)`** — make it hold: enumerate failure modes (what breaks, blast radius, recovery), define the signals that show health (SLIs/SLOs), size capacity + back-pressure, and check that a fault in one part degrades gracefully instead of cascading. Design for the failure, not just the happy path.

## Why It Exists

Systems fail at the seams — the interface no one owned, the dependency assumed always-up, the fault that cascaded. **Paṭicca-samuppāda** is the engineering truth that components are defined by their relationships; a system engineer's job is those relationships and their failure. Separating `architect_system` (the composition + trade-offs) from `assure_reliability` (the failure modes + signals) keeps both explicit — a design isn't done until it says how it breaks and how it's observed.

Working rules:

1. **Design the interfaces + boundaries first** — that's where systems actually live.
2. **Name the constraints + the trade-offs.** Every shape trades something; say what.
3. **Design for failure.** Enumerate failure modes + blast radius + recovery before the happy path is "done."
4. **Make it observable.** If you can't see its health (SLIs), you can't operate it.
5. **Bound the blast radius.** A fault degrades gracefully; it does not cascade.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `architect_system` | `goal` + constraints | A component/interface design with stated trade-offs | Pure — refines the design |
| `assure_reliability` | a `system` design | Failure modes · SLIs/SLOs · capacity · graceful-degradation checks | Pure analysis — repeatable |

Both are design/analysis — no external effect, no gate. Implementation happens downstream (`engineering`); operation is observed via the SLIs this skill defines.

## Lifecycle Mapping

```
init       → capture the goal + the hard constraints (latency/consistency/cost/security)
invoke     → architect_system (composition + trade-offs) → assure_reliability (failure + signals)
teardown   → hand the design + SLOs to build/operate
```

## Maturity

**L1**. → L2 once two systems have been architected + run with the defined SLIs catching a real fault; → L3 once `bwoc skill verify systems-engineer` is wired + green.

## Neutrality

Names no backend/model/vendor; a platform-agnostic systems craft. Satisfies **Samānattatā**.

## See Also

- [[../software-engineer/SPEC|software-engineer]] — designs the components this skill composes.
- [[../data-engineer/SPEC|data-engineer]] — the data-plane counterpart (pipelines as systems).
- [[../../agent-template/docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Paṭicca-samuppāda, Sammā-diṭṭhi.
- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
