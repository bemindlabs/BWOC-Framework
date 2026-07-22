---
title: Engineering
aliases:
  - engineering
tags:
  - group/framework-skills
  - type/skill
  - domain/engineering
maturity: L1
---

# Engineering 🛠️

> [!abstract] The craft of building software the disciplined way — the smallest change that solves the problem, matching the surrounding code, proven by tests + gates before it's called done, verified against reality not assumption. Encodes **Sīla** (the gates are the discipline), **Appamāda** (diligence — don't declare done unchecked), and **Anattā** (isolate work in a worktree; no clinging to a branch).

## What This Skill Does

Two operations are exposed:

- **`implement(task)`** — build the change: understand the current code first (read before writing), make the **minimal** edit that solves the task and reads like the code around it, then run the applicable gates (build · lint · test · format) and fix until green. Not done until the gates pass and the result is verified — never "should work."
- **`harden(change)`** — strengthen what shipped: add the missing test that would have caught the bug, close the edge case, tighten the error path, and remove the dead scaffolding. Turns a working change into a durable one.

## Why It Exists

Speed without discipline ships regressions; discipline is what makes speed safe. The gates are **Sīla** made executable — a change isn't trusted because it looks right but because build/lint/test say so. **Appamāda** is the refusal to report "done" on an unverified result. **Anattā** is worktree isolation — the branch exists only as long as the task. Centralising the implement→harden loop keeps every agent to the same bar: minimal blast radius, gates green, verified, then hardened.

Working rules:

1. **Read before you write.** Match the surrounding idiom, naming, and comment density.
2. **Minimal blast radius.** The smallest change that solves it; no unrequested refactors.
3. **Gates are the definition of done.** Build · lint · test · format green — or it isn't done.
4. **Verify against reality.** Run it; report the actual result, including failures — never "should."
5. **One concern per change.** Keep the diff focused so it can be reviewed and reverted cleanly.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `implement` | `task` | A minimal, gate-green, verified change | Converges — re-running on the same task no-ops once green |
| `harden` | a shipped `change` | Added tests, closed edges, removed scaffolding | Idempotent — re-hardening a solid change is a no-op |

The skill mutates code in an **isolated worktree** (Anattā); landing it follows the repo's own PR/gate discipline, so the operator-facing gate lives at merge, not in the skill.

## Lifecycle Mapping

```
init       → claim an isolated worktree; read the current code
invoke     → implement (minimal + gates green + verified) → harden
teardown   → release the worktree on completion (no clinging)
```

## Maturity

**L1**. → L2 once two tasks have gone implement→harden with gates green and zero manual cleanup; → L3 once `bwoc skill verify engineering` is wired + green in CI.

## Neutrality

Names no backend/model/vendor; the gates are the repo's own (build/lint/test/format). Satisfies **Samānattatā**.

## See Also

- [[../auditor/SPEC|auditor]] — independently verifies what this skill builds.
- [[../documenter/SPEC|documenter]] — records how the built thing works.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sīla, Appamāda, Anattā.
