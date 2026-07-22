---
title: Worktree Discipline
aliases:
  - worktree-discipline
tags:
  - group/framework-skills
  - type/skill
  - domain/lifecycle
maturity: L1
---

# Worktree Discipline

> [!abstract] First reference framework skill. Encapsulates the Anattā worktree-isolation contract every agent applies when claiming and releasing a task: one task → one worktree → one short-lived branch → cleanup on completion.

## What This Skill Does

Wraps the worktree lifecycle described in the agent base profile (`AGENTS.md §4 — Worktree Discipline`) so an agent does not re-derive it per task. Two operations are exposed; together they bound every unit of work an agent performs.

- **`claim_task(taskId)`** — establish an isolated working surface for the given task. Creates `<worktreeBase>/<taskId>` on a new short-lived branch (`feat|fix|docs|refactor|test|chore/<taskId>`, prefixed with `agent/<agentId>/` when multi-agent collision is possible). Idempotent: re-claiming an existing task returns the same worktree without re-creating it.
- **`release_task(taskId)`** — tear the working surface back down after the task lands. Removes the worktree directory and deletes the short-lived branch. Idempotent: releasing an already-released task is a no-op.

## Why It Exists

Worktrees are the framework's expression of **Anattā** — "no clinging." A branch is never "the agent's"; it exists only as long as the task that justified it. Centralising the create/cleanup steps as a skill keeps every agent honest about that contract: the agent invokes `claim_task` at the start and `release_task` at the end, and the manifest's verify gate (`bwoc skill verify worktree-discipline`) checks that nothing leaked.

The skill enforces the four hard rules from `AGENTS.md §4.1`:

1. Never share a working directory with another agent.
2. Never `git stash`.
3. Never switch branches in place.
4. Never work on `main`/`master` directly.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `claim_task` | `taskId` (kebab-case identifier) | `git worktree add <base>/<taskId> -b <type>/<taskId>` | Re-claim returns the existing worktree path; no double-add |
| `release_task` | `taskId` | `git worktree remove` + `git branch -d` | Re-release reports already-clean and exits success |

Both operations are observed by `Kāyānupassanā` (filesystem state) and `Dhammānupassanā` (which gate is in force). Failures surface the worktree path, the root cause, and the remedy — never just "failed."

## Lifecycle Mapping

```
init       → reads <worktreeBase> from the agent's config.manifest.json
invoke     → claim_task / release_task per task
teardown   → no-op (worktree cleanup is task-scoped, not skill-scoped)
```

The skill holds no global state between invocations. Replay-safe.

## Maturity

Declared **L1** — first successful use, unverified across backends. Bumps to L2 once at least two agents have used both operations end-to-end without manual cleanup; to L3 once `bwoc skill verify worktree-discipline` is wired and passes in CI.

## Neutrality

Manifest values name no backend, model, or vendor CLI. The skill's verify command is a framework command (`bwoc skill verify`), not a backend command — satisfies the **Samānattatā** rule enforced by `bwoc check`.

## See Also

- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
- [[../../agent-template/AGENTS|agent-template AGENTS.md §4]] — the worktree discipline contract this skill encapsulates.
- [[../../agent-template/docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Anattā framing in full.
- [[../../docs/en/GLOSSARY.en|GLOSSARY.en.md]] — Anattā, Samānattatā, Kāyānupassanā term lookup.
