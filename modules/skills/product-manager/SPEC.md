---
title: Product Manager
aliases:
  - product-manager
tags:
  - group/framework-skills
  - type/skill
  - domain/product
maturity: L1
---

# Product Manager 🎯

> [!abstract] The craft of deciding **what to build and why** — discover the real user problem, prioritise by value against cost, and define requirements + success metrics *before* code. Where [[../manager/SPEC|manager]] coordinates *how* work gets done, `product-manager` decides *which* work is worth doing. Encodes **Yoniso Manasikāra** (understand the actual problem, not the asked-for solution) and **Mattaññutā** (the right scope — the smallest thing that delivers the value).

## What This Skill Does

Two operations are exposed:

- **`discover(signal)`** — from a signal (a request, a complaint, a metric, a market gap) find the *real* problem and who has it: separate the stated solution from the underlying need, size the value, and prioritise against the alternatives. The output is a prioritised problem worth solving — or the honest conclusion that it isn't.
- **`define(problem)`** — turn the chosen problem into a buildable definition: requirements, scope boundaries (what's explicitly *out*), acceptance criteria, and the **success metric** that will say afterwards whether it worked. Hand this to the lifecycle (`ai-dlc` / `manager`), not a wish.

## Why It Exists

The most expensive software is the kind that's built well but shouldn't have been built. **Yoniso Manasikāra** applied to product means solving the problem the user *has*, not the feature they *named*; **Mattaññutā** means shipping the smallest slice that delivers the value, then learning. Separating `discover` (is this worth doing?) from `define` (what exactly, and how will we know it worked?) keeps an agent from building fast in the wrong direction.

Working rules:

1. **Problem before solution.** Name the user + the need before any feature.
2. **Prioritise by value ÷ cost**, and say what you're *not* doing.
3. **Define "done" as a metric**, not a feeling — decide up front how success is measured.
4. **Scope down.** The smallest slice that delivers value ships first (Mattaññutā).
5. **Kill honestly.** "Not worth building" is a valid, valuable output.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `discover` | `signal` | A prioritised, validated problem (or a reasoned "no") | Pure — repeatable analysis |
| `define` | a chosen `problem` | Requirements + scope + acceptance + success metric | Pure — refines, builds nothing |

Both are pure analysis/definition — no external effect, no gate. The build happens downstream via `ai-dlc`.

## Lifecycle Mapping

```
init       → gather the signals + who the user is
invoke     → discover (worth it?) → define (what + success metric)
teardown   → hand the definition to the lifecycle; no clinging to the idea
```

## Maturity

**L1**. → L2 once two definitions have shipped and been judged against their success metric; → L3 once `bwoc skill verify product-manager` is wired + green.

## Neutrality

Names no backend/model/vendor; a domain-agnostic product craft. Satisfies **Samānattatā**.

## See Also

- [[../ai-dlc/SPEC|ai-dlc]] — receives the `define` output as the intent it drives.
- [[../manager/SPEC|manager]] — coordinates the *how* once the *what* is set.
- [[../../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Yoniso Manasikāra, Mattaññutā.
- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
