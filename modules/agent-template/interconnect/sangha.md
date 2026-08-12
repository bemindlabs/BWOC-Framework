---
title: Saṅgha — Teams & Shared Task List
aliases:
  - Saṅgha
  - Agent Teams
  - Saṅgaha-vatthu 4
tags:
  - group/agents
  - type/design
  - meta/template
status: draft (v2026.5.23 — Phase A: membership + shared task list + self-claim)
canonical-source: Saṅgaha-vatthu (AN 4.32, DN 31) · Saṅghakamma (Vinaya, Mahāvagga)
---

# Saṅgha — Teams & Shared Task List

> [!abstract] A **team** (saṅgha) groups a subset of a workspace's agents under one shared task list. The human operator is the implicit lead. Teammates **self-claim** pending, unblocked tasks; an advisory file lock makes each claim a Saṅghakamma — a communal act settled by exactly one member. Shipped: the CLI + on-disk foundation (Phase A), daemon task-watch with opt-in wakeup + auto-claim (Phase B/B+), task hooks, and plan approval (Pavāraṇā, Phase C). A team-aware dashboard pane and a designated lead-agent remain future work.

## Motivation

BWOC already has the pieces for agents to *talk* — the inbox ([`send.rs`](../../../crates/bwoc-cli/src/send.rs)), verified sender identity + the Kalyāṇamitta-7 trust gate ([[trust]]), and Sāraṇīyadhamma-6 cordiality norms ([[messaging]]). What it lacked was a place for agents to *coordinate work*: a shared list of tasks they can claim, complete, and gate on each other.

Compared to one-shot subagents (which only report back), a team shares a task list and lets members pick up work independently — the same distinction Claude Agent Teams draws between subagents and teammates. BWOC's twist: the coordination state is plain files in the workspace (`.bwoc/teams/`), transport-agnostic and readable by any backend running through `bwoc spawn`.

## Buddhist grounding

| Concept | Source | BWOC application |
|---|---|---|
| **Saṅgha** | the community of practitioners | A team — a named, bounded set of agents working a common list. ≥1 member; the human is lead, never a member. |
| **Saṅgaha-vatthu 4** | AN 4.32 — four bases of social cohesion | The norms a team runs on: **dāna** (share findings, don't hoard), **peyyavajja** (kindly task titles + messages — see [[messaging]]), **atthacariyā** (claim work that helps the team, not just yourself), **samānattatā** (every member equal before the task list — no privileged claimer). |
| **Saṅghakamma** | Vinaya, Mahāvagga — formal communal acts | The claim protocol: a task transitions to one member under a lock, so two members never claim the same item. The lock *is* the quorum that makes the act valid. |

> [!note] Saṅgaha-vatthu 4 are **norms, not gates** in Phase A — `bwoc` does not enforce kindly titles or unselfish claims. They live here so an incarnated agent can internalize them, the same way [[messaging]] carries Sāraṇīyadhamma 6.

## Data model

A team is membership + a task list.

```toml
# .bwoc/teams/<team-id>.toml
id = "squad"
members = ["agent-pi", "agent-oracle"]
created_at = "2026-05-23T06:47:15Z"
```

```jsonl
# .bwoc/teams/<team-id>/tasks.jsonl  — one task per line
{"id":"t1","title":"design schema","state":"completed","created_at":"…","claimed_by":"agent-pi","completed_at":"…"}
{"id":"t2","title":"implement","state":"in_progress","deps":["t1"],"created_at":"…","claimed_by":"agent-oracle"}
```

- **No `lead` field.** The human operator is the implicit lead — they create the team, add tasks, and synthesize results. (A designated lead *agent* is a possible v2 extension, not v1.)
- **`deps`** is a list of task ids that must be `completed` before a task is claimable. Omitted when empty.
- **`claimed_by`** is set on claim and kept through completion (audit trail of who did the work).

### Task state machine

```
pending ──claim──▶ in_progress ──complete──▶ completed
```

- **claim**: task must be `pending` AND every dependency `completed`. Sets `in_progress` + `claimed_by`.
- **complete**: task must be `in_progress` AND the actor must be the claimant. Sets `completed` + `completed_at`.
- Tasks are never deleted (completed tasks stay for the audit trail), so auto-ids (`t1`, `t2`, …) are monotonic.

## CLI surface

```bash
# Teams
bwoc team create <id> --members a,b,c     # define a team
bwoc team list                            # teams + member/task counts
bwoc team retire <id> [--yes]             # remove membership + task list (destructive)

# Tasks (operate on one team)
bwoc task add <team> "<title>" [--deps t1,t2] [--id <custom>]
bwoc task list <team>                     # id · state · claimant · title
bwoc task claim <team> <task> --as <agent>      # self-claim (member only)
bwoc task complete <team> <task> --as <agent>   # claimant only
```

All commands take `--workspace` (standard resolution: flag → `BWOC_WORKSPACE` → ancestor walk → cwd) and `--json` for structured output.

> [!example] An agent running inside `bwoc spawn` self-claims with its own id:
> ```bash
> bwoc task claim squad t2 --as agent-oracle
> ```
> Claiming a blocked or already-claimed task exits `2` with an actionable message; the task file is left untouched.

## Concurrency — the lock

`bwoc task add/claim/complete` acquire an advisory lock (`.bwoc/teams/<id>/tasks.lock`) before the read-modify-write. The lock is a dependency-free `O_CREAT | O_EXCL` file holding the holder's PID; a stale lock (PID dead, signal-0 probe) is stolen. Two agents racing to claim the same task serialize: one wins (`in_progress`), the other reads the now-`in_progress` task and is refused. Verified live with two concurrent `bwoc task claim` processes.

## Refusal & exit codes

| Situation | Exit | Message shape |
|---|---|---|
| Blocked by dependency | 2 | `task 't2' is blocked: dependency 't1' is not completed` |
| Already claimed / wrong state | 2 | `task 't1' is in_progress — only pending tasks can be claimed` |
| Non-member claims | 2 | `agent 'x' is not a member of this team (members: …)` |
| Complete by non-claimant | 2 | `task 't1' is claimed by 'agent-pi', not 'agent-oracle'` |
| Lock contention timeout | 1 | `could not acquire task lock (… remove tasks.lock if stale)` |

## Phase B — daemon task-watch (shipped)

A running `bwoc-agent --serve` watches the shared task lists of every team its agent belongs to and announces newly-claimable tasks to stderr — the same shape as the inbox watch:

```text
bwoc-agent: task available ← squad/t3: implement the parser
```

"Claimable" = `pending` with every dependency `completed`, in a member team. The daemon snapshots what's already open at startup (no replay — like the inbox cursor starting at EOF) and polls on a 2-second cadence (tasks change rarely). Inert when the agent is on no team or no workspace resolves. See [`crates/bwoc-agent/src/task_watch.rs`](../../../crates/bwoc-agent/src/task_watch.rs).

**Opt-in wakeup** (`BWOC_TASK_WAKEUP=1`): on a newly-claimable task the daemon also pings the agent's tmux session (`agent-<x>` → session `<x>`) with a `[bwoc task <team>/<id>] <title>` marker — the same best-effort two-step send-keys the inbox uses. A live Claude session running the agent sees it and can `bwoc task claim`. The agent stays in control: the daemon does not mutate the list. Default off (announce-only).

**Opt-in auto-claim** (`BWOC_AUTO_CLAIM=1`): the autonomous-teamwork mode. On a newly-claimable task the daemon claims it for its agent — via the locked `bwoc task claim` CLI path, so the lock + state machine serialize against other members (a lost race just logs `auto-claim … skipped`) — then wakes the agent to work it. This is the riskiest mode (the daemon mutates shared state), so it's gated separately from `wakeup` and off by default. The full loop: `bwoc task add` → daemon sees it → claims for the agent → wakes the agent. See [`crates/bwoc-agent/src/task_watch.rs`](../../../crates/bwoc-agent/src/task_watch.rs).

## Task hooks (shipped)

Optional workspace-level shell hooks fire on task lifecycle, mirroring Claude Agent Teams' `TaskCreated` / `TaskClaimed` / `TaskCompleted`. A hook is an executable at `<workspace>/.bwoc/hooks/<event>`; a missing file — or, on Unix, a present-but-non-executable one — is a silent no-op (hooks are opt-in). The env-var columns below list the **`BWOC_*` context** each event adds; the full environment the hook sees is that context plus the scrubbed safe base described under *Contract*.

| Event — `<workspace>/.bwoc/hooks/…` | Fires when | `BWOC_*` context added |
|---|---|---|
| `task-created` | `bwoc task add` is about to persist a task | `BWOC_TASK_EVENT`, `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_TASK_TITLE` |
| `task-claimed` | `bwoc task claim` is about to persist a claim | `BWOC_TASK_EVENT`, `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_AGENT`, `BWOC_WORKTREE_BASE` |
| `task-completed` | `bwoc task complete` is about to persist a completion | `BWOC_TASK_EVENT`, `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_AGENT` |

**Contract.** A **non-zero exit blocks the operation** — the task file is left unchanged and the hook's first stderr line is surfaced to the operator (exit 2). The hook runs with cwd set to the workspace and a **scrubbed environment**: only a small safe base (`PATH`, `HOME`, …) plus the `BWOC_*` context above — the operator's ambient secrets (tokens, SSH agent, cloud creds) are **not** inherited, so a hook that needs more must read it from a file. The event name is validated to a single kebab segment. Use hooks for quality gates: e.g. a `task-completed` hook that runs `cargo test` and exits non-zero to refuse completion until tests pass.

## Plan approval — Pavāraṇā (shipped)

For risky or far-reaching work, a task can require the lead's sign-off on a plan before it completes — mapping **Pavāraṇā** (the monk's invitation, at Vassa's end, for the Saṅgha to point out his faults: submitting oneself for review before proceeding).

- `bwoc task add <team> "<title>" --requires-plan` — gate this task on plan approval.
- `bwoc task plan <team> <task> --as <agent> --plan "<text>"` (or `--plan-file`) — the claimant submits or revises a plan (must have the task `in_progress`). Re-submitting clears any prior verdict back to pending.
- `bwoc task plan <team> <task>` (no `--as`/`--plan`) — show the current plan + verdict.
- `bwoc task approve <team> <task>` / `bwoc task reject <team> <task>` — the lead's verdict (no `--as`; the human operator is the lead). Reject sends it back for revision.
- `bwoc task complete` on a `requires_plan` task is refused until `plan_approved == true` — the gate lives in `bwoc-core::team::complete_task`, so it holds no matter which surface triggers completion (including a daemon auto-claim that later tries to complete).

A non-plan task (`requires_plan` default false) completes exactly as before — the gate is opt-in per task.

## Deferred (later phases)

- **Team-aware dashboard** — a task pane in `bwoc dashboard` (the detail pane already shows per-agent team standing; a full team/task pane is the next step).
- **Designated lead agent** — a `lead` field + lead-only operations. Only if the human-implicit-lead model proves limiting.

## Related

- [[trust]] — Kalyāṇamitta-7; gates *who* a teammate accepts messages from.
- [[messaging]] — Sāraṇīyadhamma 6; the channel teammates talk over.
- [`crates/bwoc-core/src/team.rs`](../../../crates/bwoc-core/src/team.rs) — data model + transition rules.
- [`crates/bwoc-cli/src/sangha.rs`](../../../crates/bwoc-cli/src/sangha.rs) — CLI + lock.
- Fleet-level governance (operator view of many agents): `docs/en/FLEET-GOVERNANCE.en.md` (Aparihāniya-dhamma 7).
