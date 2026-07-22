---
title: Lawyer
aliases:
  - lawyer
tags:
  - group/framework-skills
  - type/skill
  - domain/governance
maturity: L1
---

# Lawyer ⚖️

> [!abstract] The craft of reasoning about **rules** — contracts, software licenses, policies, terms of service, and compliance obligations — precisely, conservatively, and against the *actual text*. Encodes **Sīla** (operate within the bounds), **Sacca** (represent the rule truthfully, no wishful reading), and a hard humility clause: this skill *informs*, it does not *give legal advice* — decisions with legal weight route to a qualified human.

## What This Skill Does

Two operations are exposed:

- **`review(document)`** — read the actual terms (not a summary of them): identify the obligations, permissions, and prohibitions; the parties and what each owes; the triggers, deadlines, and termination/liability clauses; and where the text is ambiguous. Quote the operative language; never paraphrase away a constraint.
- **`flag_risk(situation)`** — map a proposed action against the rules and surface the risk: what a clause forbids or requires, a license incompatibility, a policy a step would breach — with the specific clause cited and a conservative reading. When the stakes are real, the output is "get counsel," not a verdict.

## Why It Exists

Rules are load-bearing: a misread license taints a codebase, a missed clause voids a protection. **Sacca** demands the rule be represented as written, not as convenient; **Sīla** is staying inside the bounds by *knowing* them. But competence has a boundary — **Yoniso Manasikāra** applied to one's own limits: an agent can locate and explain terms, but binding legal judgment belongs to a licensed professional. The skill makes both explicit: read precisely, flag conservatively, and route the weighty call to a human.

Working rules:

1. **Read the actual text.** Quote the operative clause; don't paraphrase a constraint away.
2. **Conservative by default.** When a reading is ambiguous, assume the stricter interpretation.
3. **Cite the clause.** Every risk names the specific term it comes from.
4. **Know the boundary.** This is *information*, not legal advice. Decisions with legal weight → a qualified human; say so plainly.
5. **Flag, don't decide, on the weighty ones.** Surface the risk + route it; never improvise authority.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `review` | `document` (the actual terms) | Obligations · permissions · prohibitions · triggers · ambiguities, with quotes | Pure read — repeatable |
| `flag_risk` | `situation` + the rules | Cited risks + a conservative reading; "get counsel" when weighty | Pure — deterministic given the text |

Both are pure reads — the skill informs, it does not act or bind. No external effect, no gate; the real gate is the humility clause (route legal decisions to a human).

## Lifecycle Mapping

```
init       → obtain the actual governing text (license/contract/policy)
invoke     → review (understand the terms) → flag_risk (map an action, cite clauses)
teardown   → no-op
```

## Maturity

**L1**. → L2 once two reviews have correctly surfaced a real obligation/risk with citations; → L3 once `bwoc skill verify lawyer` is wired + green.

## Neutrality

Names no backend/model/vendor; the governing text is supplied per review. Satisfies **Samānattatā**.

## See Also

- [[../auditor/SPEC|auditor]] — the same evidence-cited, conservative stance for technical standards.
- [[../counselor/SPEC|counselor]] — shares the "know your limits, route to a professional" clause.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sīla, Sacca.
