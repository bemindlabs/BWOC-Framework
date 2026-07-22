---
title: Manager
aliases:
  - manager
tags:
  - group/framework-skills
  - type/skill
  - domain/coordination
maturity: L1
---

# Manager 🗂️

> [!abstract] The craft of turning a goal into coordinated, right-sized work — decompose it, form or assign to a **Saṅgha** team with a shared task list, and track to done — without micromanaging. Encodes **Mattaññutā** (right-sized pieces), **Sīlasāmaññatā** (shared conventions), and the **Saṅgha** model of self-organising community.

## What This Skill Does

Two operations are exposed:

- **`plan(goal)`** — decompose a goal into tasks that are each independently shippable and right-sized (small enough to finish in one focused unit, large enough to matter), with dependencies named so work can parallelise where it's safe to.
- **`delegate(tasks, team)`** — place the tasks on a team's **shared task list** (via the framework's `team`/`task` surface), letting members *claim* work rather than being pushed it, and track state (open → claimed → done) to completion. Escalate a blocked or unclaimed task; never silently drop one.

## Why It Exists

Coordination fails in two directions: too little (work collides, nothing lands) and too much (a manager becomes the bottleneck). The Saṅgha model routes between them — a shared task list with self-claim, bounded by **Mattaññutā** (don't over-plan; the smaller plan that ships beats the exhaustive one that doesn't) and **Sīlasāmaññatā** (everyone follows the same conventions, so no one needs a babysitter). Centralising the plan→delegate loop keeps an agent honest about both.

Working rules:

1. **Right-size before assigning.** A task that can't be claimed-and-finished is two tasks.
2. **Name dependencies, then parallelise the rest.** Sequence only what must be sequenced.
3. **Pull, don't push.** Members claim; the manager surfaces, unblocks, and tracks.
4. **No silent drops.** Every task ends in done or an explicit, surfaced reason it didn't.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `plan` | `goal` | A dependency-ordered set of right-sized tasks | Pure — re-planning refines the breakdown |
| `delegate` | `tasks` + `team` | Tasks land on the team's shared list; state tracked to done | Idempotent add (re-delegating an existing task doesn't duplicate it) |

`plan` is a pure read/generation. `delegate` mutates the workspace's own task list (not an external system of record), so it follows the local-write pattern — no operator-confirm gate beyond what the underlying `task` verb already applies.

## Lifecycle Mapping

```
init       → read the goal + the available team(s)
invoke     → plan → delegate → track to done (unblock/escalate as needed)
teardown   → no-op (the task list is the durable artifact, not skill state)
```

## Maturity

**L1**. → L2 once two goals have gone plan→delegate→done on a real team; → L3 once `bwoc skill verify manager` is wired + green.

## Neutrality

Names no backend/model/vendor; coordinates through the framework's own team/task surface. Satisfies **Samānattatā**.

## See Also

- [[../auditor/SPEC|auditor]] — verifies that delegated work actually met its bar.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Saṅgha, Mattaññutā, Sīlasāmaññatā.
- [[../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
