---
title: Mathematics
aliases:
  - mathematics
tags:
  - group/framework-skills
  - type/skill
  - domain/reasoning
maturity: L1
---

# Mathematics ➗

> [!abstract] The craft of rigorous symbolic + quantitative reasoning — state what you assume, derive each step so it follows from the last, and **check the answer** before trusting it. Encodes **Yoniso Manasikāra** (verify against reality, not intuition) and **Sacca** (a result is true only when it survives a second look).

## What This Skill Does

Two operations are exposed:

- **`derive(problem)`** — solve it in the open: state the assumptions and what's given, choose a method and say why, then work step by step so each line follows from the previous. No leaps — a skipped step is where the error hides.
- **`check_result(result)`** — try to break the answer before trusting it: dimensional/units consistency, boundary + degenerate cases (0, 1, ∞, negative), sign/magnitude sanity, and — where feasible — a **second, independent method** that must agree. A result that fails a check is wrong, however elegant the derivation looked.

## Why It Exists

Confidence is not correctness. **Sacca** demands the answer be *checked*, not merely produced; **Yoniso Manasikāra** demands the check be against reality (units, limits, an independent path), not against how sure it felt. Separating `derive` from `check_result` makes the second half non-optional — the discipline that catches the sign error, the dropped factor, the mis-stated assumption.

Working rules:

1. **State assumptions + givens first.** An unstated assumption is an ungrounded answer.
2. **One step per line, each following the last.** No leaps; show the work.
3. **Check units + limits + magnitude** on every result.
4. **Confirm with a second method** when feasible; two independent paths agreeing is the bar.
5. **Exact where exact is possible**; bound the error where it isn't. Say which.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `derive` | `problem` | A step-by-step solution with stated assumptions | Pure — deterministic given the problem |
| `check_result` | `result` | VALID (with the checks passed) or a located error | Pure — repeatable |

Pure reasoning — no external system, no gate.

## Lifecycle Mapping

```
init       → fix the problem statement + assumptions
invoke     → derive → check_result (units · limits · second method)
teardown   → no-op
```

## Maturity

**L1**. → L2 once two derivations have shipped with an independent `check_result` catching or confirming; → L3 once `bwoc skill verify mathematics` is wired + green.

## Neutrality

Names no backend/model/vendor; language-agnostic reasoning. Satisfies **Samānattatā**.

## See Also

- [[../physics/SPEC|physics]] — applies this rigor to physical systems (models + estimates).
- [[../auditor/SPEC|auditor]] — the same "verify before trust" stance for artifacts.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Yoniso Manasikāra, Sacca.
