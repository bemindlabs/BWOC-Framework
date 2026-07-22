---
title: Physics
aliases:
  - physics
tags:
  - group/framework-skills
  - type/skill
  - domain/science
maturity: L1
---

# Physics ⚛️

> [!abstract] The craft of modelling a physical system honestly — see it clearly, choose the right regime and governing laws, and sanity-check every number with dimensions and order-of-magnitude before believing it. Encodes **Sammā-diṭṭhi** (right view — model the system as it is, not as convenient) and **Yoniso Manasikāra** (check against physical reality).

## What This Skill Does

Two operations are exposed:

- **`model(system)`** — build the right model: identify the regime (which effects dominate, which are negligible), state the simplifying assumptions and where they break, and pick the governing laws that actually apply at this scale. The hard part is choosing what to *ignore*.
- **`estimate(model)`** — get a number and trust it only after checks: **dimensional analysis** (the units must work out), **order-of-magnitude** (is it physically plausible?), limiting cases (does it reduce correctly when a term → 0 or ∞?), and known reference points. A number that fails a units or magnitude check is wrong.

## Why It Exists

Physics goes wrong at the modelling step — the wrong regime, an assumption that quietly breaks — long before the algebra. **Sammā-diṭṭhi** is the insistence on seeing the system truly; **Yoniso Manasikāra** is the refusal to trust a number that hasn't been checked against dimensions and scale. Separating `model` from `estimate` keeps both honest: name the model's assumptions, then let dimensional analysis and order-of-magnitude catch the errors that a confident derivation would carry through.

Working rules:

1. **Identify the regime first.** Which forces/effects dominate? What is negligible, and why?
2. **State assumptions + their breakdown.** Every model is wrong somewhere; say where.
3. **Dimensional analysis on every result.** Units that don't cancel = a real error, always.
4. **Order-of-magnitude sanity.** Is the answer physically plausible? Check against a reference.
5. **Check limiting cases.** The result must reduce correctly at the edges.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `model` | `system` | A regime + governing laws + stated assumptions | Pure — deterministic given the system |
| `estimate` | `model` | A number that passed units + magnitude + limit checks | Pure — repeatable |

Pure reasoning — no external system, no gate.

## Lifecycle Mapping

```
init       → describe the system + what's being asked
invoke     → model (regime + assumptions) → estimate (dims · magnitude · limits)
teardown   → no-op
```

## Maturity

**L1**. → L2 once two estimates have shipped with dimensional + magnitude checks catching or confirming; → L3 once `bwoc skill verify physics` is wired + green.

## Neutrality

Names no backend/model/vendor; language-agnostic. Satisfies **Samānattatā**.

## See Also

- [[../mathematics/SPEC|mathematics]] — the rigor `estimate` leans on.
- [[../../agent-template/docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sammā-diṭṭhi, Yoniso Manasikāra.
