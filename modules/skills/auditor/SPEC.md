---
title: Auditor
aliases:
  - auditor
tags:
  - group/framework-skills
  - type/skill
  - domain/assurance
maturity: L1
---

# Auditor 🔎

> [!abstract] The craft of checking work or a claim against a standard, surfacing what's wrong, and — before reporting it — **adversarially verifying every finding** so no false positive survives. Encodes **Yoniso Manasikāra** (check against current reality, not memory) and **Sacca** (truthfulness): every reported issue carries evidence and a concrete failure it would cause.

## What This Skill Does

Two operations are exposed:

- **`audit(target, standard)`** — inspect a target (a diff, a doc, a config, a fleet) against a standard (a spec, a convention, a checklist) and produce candidate findings — each with a location, the rule it breaks, and the concrete way it would fail.
- **`verify_finding(finding)`** — try to *refute* each candidate before it ships: reproduce the failure, or trace the exact inputs/state that trigger it. A finding that can't be substantiated is dropped, not reported. Default to "not a real issue" when the evidence is thin.

## Why It Exists

An audit that cries wolf is worse than none — it trains people to ignore it. **Sacca** demands that a reported issue be *real*; **Yoniso Manasikāra** demands it be checked against the current artifact, not a remembered one. Separating `audit` (find candidates) from `verify_finding` (adversarially confirm) makes the discipline explicit: the bar to *report* is higher than the bar to *notice*. This mirrors the fleet's verify-before-trust rule and the framework's own review discipline (find → adversarially verify → confirm).

Working rules:

1. **Evidence or it didn't happen.** Every finding names a location + a concrete failure.
2. **Refute before you report.** Try to prove the finding wrong; only survivors ship.
3. **Verify against the current artifact**, never a memory or a stale copy (Yoniso Manasikāra).
4. **Rank by severity**, and say what you did *not* check (no false "all clear").
5. **No blame, just the defect + the remedy.** Report the fix, not fault.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `audit` | `target` + `standard` | Candidate findings (location · rule · failure) | Pure read — repeatable |
| `verify_finding` | one `finding` | CONFIRMED (with evidence) or DROPPED | Pure — deterministic given the artifact |

Both are pure reads — an audit inspects, it does not mutate the target — so no operator-confirm gate.

## Lifecycle Mapping

```
init       → fix the standard + the target's current state
invoke     → audit (candidates) → verify_finding on each → report survivors, ranked
teardown   → no-op
```

## Maturity

**L1**. → L2 once two audits have shipped zero false positives against real targets; → L3 once `bwoc skill verify auditor` is wired + green.

## Neutrality

Names no backend/model/vendor; the standard is supplied per audit. Satisfies **Samānattatā**.

## See Also

- [[../engineering/SPEC|engineering]] — builds the work this skill checks (gates ↔ audit).
- [[../manager/SPEC|manager]] — audits confirm delegated work met its bar.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Yoniso Manasikāra, Sacca.
