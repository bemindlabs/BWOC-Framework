---
title: Software Engineer
aliases:
  - software-engineer
tags:
  - group/framework-skills
  - type/skill
  - domain/software
maturity: L1
---

# Software Engineer 💻

> [!abstract] The professional software role — **design a component well** and **review a change well** — the design+review bracket around the [[../engineering/SPEC|engineering]] craft (which implements + hardens). Where `engineering` is "build it right," `software-engineer` is "design the right shape" and "verify someone else's shape." Encodes **Sammā-diṭṭhi** (design from a clear model), **Sīla** (review holds the bar), and **Yoniso Manasikāra** (review the actual diff, not the description).

## What This Skill Does

Two operations are exposed:

- **`design_component(requirement)`** — design a unit before building it: the interface/API, the data model, the states + error paths, and the trade-offs the design makes (simplicity vs flexibility, coupling vs cohesion). Choose the boring, obvious shape that reads well over the clever one. Output a design a reviewer can reason about.
- **`review_code(change)`** — read someone's change against the bar: correctness (does it do what it claims, including the edge cases?), clarity (will the next reader understand it?), and risk (what could it break?). Comment on the diff with a concrete failing scenario per issue; approve only what you'd own.

## Why It Exists

Two failures bracket implementation: a component built to a bad shape (fast, wrong design) and a change merged unreviewed (fast, unverified). `software-engineer` covers both ends — **Sammā-diṭṭhi** in design (a clear model before code) and **Sīla** in review (the bar holds because someone checked). It pairs with `engineering` (implement/harden) and `auditor` (adversarial verification): design → build → review → audit.

Working rules:

1. **Design the interface + data model first**, and the error paths with them.
2. **Prefer boring + obvious** to clever; code is read far more than written.
3. **Name the trade-off.** Every design chooses; say what it gives up.
4. **Review the actual diff**, run it in your head with real inputs, and cite a concrete failure per comment (Yoniso Manasikāra).
5. **Approve only what you'd own.** A rubber-stamp review is worse than none.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `design_component` | `requirement` | An interface/data-model/error design with stated trade-offs | Pure — refines the design |
| `review_code` | a `change` (diff) | Findings (correctness · clarity · risk), each with a concrete scenario; verdict | Pure read — repeatable |

Both are design/review — no mutation. Building is `engineering`; landing follows the repo's own review gate.

## Lifecycle Mapping

```
init       → read the requirement (design) or the diff + its claim (review)
invoke     → design_component  ·  review_code
teardown   → hand the design to build, or the verdict to the author
```

## Maturity

**L1**. → L2 once two components have been designed→built→reviewed with the review catching a real defect; → L3 once `bwoc skill verify software-engineer` is wired + green.

## Neutrality

Names no backend/model/vendor; a language-agnostic software craft. Satisfies **Samānattatā**.

## See Also

- [[../engineering/SPEC|engineering]] — implements + hardens the designs (the build craft between design and review).
- [[../auditor/SPEC|auditor]] — the adversarial-verification stance `review_code` draws on.
- [[../systems-engineer/SPEC|systems-engineer]] — composes these components into systems.
- [[../../agent-template/docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sammā-diṭṭhi, Sīla, Yoniso Manasikāra.
