---
title: AI-DLC
aliases:
  - ai-dlc
  - ai-driven-development-lifecycle
tags:
  - group/framework-skills
  - type/skill
  - domain/methodology
maturity: L1
---

# AI-DLC 🔁

> [!abstract] The **AI-Driven Development Life Cycle** as a repeatable practice: an agent takes a human *intent*, elaborates it into an agreed plan, then drives **build → verify → document → operate** — with the human steering at bounded checkpoints, not typing every line. It is the *meta-skill* that sequences the craft skills across a lifecycle. Encodes the framework's own arc — **uppāda / ṭhiti / vaya** (birth · live · retire) applied to a unit of work — plus **Sīla** (gates), **Yoniso Manasikāra** (verify each phase), **Saṅgha** (mob with the human), and **Appamāda** (diligence to done).

## What This Skill Does

AI-DLC inverts the old lifecycle: the **agent drives** and the human **steers**. Work moves in small, bounded increments; the human is in a "mob" with the agent — approving intent, unblocking, and confirming the irreversible steps — rather than writing the code. This skill wraps that loop so an agent runs a whole increment to done with the checkpoints in the right places.

Two operations are exposed:

- **`plan_increment(intent)`** — the **Inception** phase. Elaborate a human intent (a feature, a fix, a goal) into concrete requirements + a right-sized plan: what changes, the acceptance bar, the risks, and where the human must approve. Surface it and **stop for approval before any build** — the plan is the contract.
- **`execute_increment(plan)`** — the **Construction + Operation** phase. Drive the approved plan through the loop — build (`engineering`), verify against the acceptance bar (`auditor` + gates), record how it works (`documenter`) — pausing at each **bounded, irreversible gate** for the operator-confirm the framework already enforces (merge, deploy, destructive ops), then observe the result and close or spawn the next increment.

## Why It Exists

Left unstructured, AI-driven work fails at the two ends: **no agreed intent** (the agent builds the wrong thing fast) or **no checkpoints** (the agent ships an irreversible change unconfirmed). AI-DLC fixes both by making the lifecycle explicit — intent is *agreed before build*, and the human sits at the *bounded gates*, not in the loop of every keystroke. That is exactly BWOC's grain: agents do the work, **Sīla** gates prove it, and the **deferred-control / operator-confirm** principle keeps the human at the irreversible steps. Centralising the loop as a skill means an agent runs the whole cycle to the same standard instead of re-deriving the phases each time.

The phases and their checkpoints:

```
intent ──plan_increment──▶ [PLAN]  ⟵ human APPROVES intent + plan (checkpoint 1)
[PLAN] ──execute_increment──▶ build ─▶ verify ─▶ document
                                              └─ human CONFIRMS at each bounded/
                                                 irreversible gate (merge/deploy/…)
                                     ─▶ operate ─▶ close increment / spawn next
```

Working rules:

1. **Agree intent before building.** No construction until the plan is approved (checkpoint 1).
2. **Small increments.** One bounded unit at a time; a big intent becomes several increments (compose `manager`).
3. **Human at the irreversible gates, not the keystrokes.** Confirm merges/deploys/destructive ops; automate the rest.
4. **Every phase verifies.** Build isn't done until gates + acceptance bar pass (Yoniso Manasikāra, Sīla).
5. **Record as you go.** Documentation and decisions land with the increment, not "later" (`documenter`).
6. **Close the loop.** Operate/observe, then retire the increment (vaya) or spawn the next — no clinging.

## Composition

AI-DLC orchestrates the craft skills rather than re-implementing them — the phases delegate to:

| Phase | Leans on |
|---|---|
| Inception / plan | [[../manager/SPEC\|manager]] (decompose + right-size) |
| Build | [[../engineering/SPEC\|engineering]] (implement + harden, in an isolated worktree) |
| Verify | [[../auditor/SPEC\|auditor]] (audit + adversarially verify against the bar) |
| Document | [[../documenter/SPEC\|documenter]] (capture + keep in sync) |

An agent enabling `ai-dlc` typically enables those four too.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `plan_increment` | `intent` | An agreed, right-sized plan with the acceptance bar + checkpoints; stops for approval | Pure until approved — re-planning refines, builds nothing |
| `execute_increment` | approved `plan` | Build → verify → document → operate, pausing at bounded gates for operator-confirm | Converges — re-running a completed increment no-ops once its bar is met |

`plan_increment` is a pure read/generation. `execute_increment` mutates code/docs in an **isolated worktree** and lands only through the repo's own gate + operator-confirm at each irreversible step — so the gate lives where the framework already puts it (merge/deploy), not bypassed inside the skill.

## Lifecycle Mapping

```
init       → capture the intent + the human who steers
invoke     → plan_increment (approve) → execute_increment (build/verify/document/operate, confirm at gates)
teardown   → close the increment (vaya); spawn the next or stop (no clinging)
```

## Maturity

**L1**. → L2 once two intents have gone intent→operate with the checkpoints honored and gates green; → L3 once `bwoc skill verify ai-dlc` is wired + green in CI.

## Neutrality

Names no backend/model/vendor; the gates are the repo's own and the checkpoints are the framework's operator-confirm. Satisfies **Samānattatā**.

## See Also

- [[../engineering/SPEC|engineering]] · [[../auditor/SPEC|auditor]] · [[../documenter/SPEC|documenter]] · [[../manager/SPEC|manager]] — the craft skills AI-DLC sequences.
- [[../worktree-discipline/SPEC|worktree-discipline]] — the Anattā isolation each increment builds in.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — uppāda/ṭhiti/vaya, Sīla, deferred-control framing.
- [[../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
