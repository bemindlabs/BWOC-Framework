---
title: Data Scientist
aliases:
  - data-scientist
tags:
  - group/framework-skills
  - type/skill
  - domain/data-science
maturity: L1
---

# Data Scientist 📊

> [!abstract] The craft of **learning from data honestly** — explore and reason statistically, then build and evaluate models without fooling yourself, reporting *uncertainty* rather than a single confident number. Leans on [[../mathematics/SPEC|mathematics]] for rigor and [[../data-engineer/SPEC|data-engineer]] for trustworthy inputs. Encodes **Yoniso Manasikāra** (let the data speak; verify against it) and **Sacca** (report what's true, including how unsure you are).

## What This Skill Does

Two operations are exposed:

- **`analyze(dataset, question)`** — explore before concluding: understand the data's shape, distributions, and gaps; state a hypothesis; use the right statistic (and check its assumptions); and separate correlation from cause. The output is an evidenced answer with its confidence + caveats — never a chart that flatters a pre-decided story.
- **`build_model(dataset, target)`** — model honestly: split train/validation/test *before* looking, engineer features without **leakage** (no target signal bleeding into inputs), evaluate on held-out data with a metric that matches the real objective, and report the uncertainty + failure modes (where it's wrong, for whom). A model that isn't validated out-of-sample isn't a result.

## Why It Exists

Data science's failure mode is self-deception at speed — leakage, p-hacking, a metric that looks great and means nothing, a confident point estimate hiding a wide interval. **Sacca** demands the honest number (with its uncertainty); **Yoniso Manasikāra** demands the conclusion be checked against held-out reality, not the story you hoped for. Separating `analyze` (what does the data say?) from `build_model` (can it predict out-of-sample?) keeps both disciplined, and pairs with `auditor`'s verify-before-trust and `data-engineer`'s trustworthy inputs.

Working rules:

1. **Explore before concluding.** Know the distributions + gaps; check the statistic's assumptions.
2. **Correlation ≠ causation.** Say which you have; don't dress one as the other.
3. **No leakage.** Split before you look; keep the target out of the features.
4. **Evaluate out-of-sample**, with a metric that matches the real objective.
5. **Report uncertainty + failure modes.** A point estimate without an interval is a half-truth; say where the model is wrong.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `analyze` | `dataset` + `question` | An evidenced answer with confidence + caveats | Pure — deterministic given data + seed |
| `build_model` | `dataset` + `target` | A leakage-free model + honest out-of-sample eval + uncertainty | Reproducible with a fixed seed/split |

Both are analysis — no external mutation, no gate. Inputs come from `data-engineer`; conclusions feed `product-manager` / decisions.

## Lifecycle Mapping

```
init       → obtain trustworthy data (data-engineer) + a real question
invoke     → analyze (what does it say?) → build_model (can it predict, honestly?)
teardown   → report the answer + its uncertainty; hand to the decision
```

## Maturity

**L1**. → L2 once two analyses/models have shipped with out-of-sample eval + reported uncertainty; → L3 once `bwoc skill verify data-scientist` is wired + green.

## Neutrality

Names no backend/model/vendor; a tool-agnostic analytical craft. Satisfies **Samānattatā**.

## See Also

- [[../data-engineer/SPEC|data-engineer]] — supplies the trustworthy data this skill learns from.
- [[../mathematics/SPEC|mathematics]] — the statistical rigor `analyze` leans on.
- [[../auditor/SPEC|auditor]] — the same verify-before-trust stance for claims.
- [[../../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Yoniso Manasikāra, Sacca.
