---
title: Loop Engineering
parent: English
nav_order: 8
---

# Loop Engineering

A **goal + ticker loop** is an agent (or fleet) that works toward a persistent objective, re-fired on a cadence, until a Definition-of-Done — instead of running once and exiting. This document specifies the loop-engineering layer BWOC is building: the three objects (**Goal**, **Ticker**, **Gate**), the iteration cycle, how it internalizes the retired *Refinement Loop* pattern, the use-case catalog it enables, and the phased build plan.

> [!abstract] BWOC already ships every raw primitive a loop is made of — persistent daemon ticks, per-fire actions (lead drain, mine, health scan), and gates (budget, plan-approval, peer-review). What is missing is the **orchestration envelope** around them: a first-class Goal with a machine-checkable done predicate, a configurable Ticker (not a hardcoded 2 s poll), and a re-fire-until-done gate. Loop engineering is that envelope.

## Why this exists

Two things prove the need and the shape:

1. **The retired [Refinement Loop](https://github.com/bemindlabs/BWOC-Framework/blob/main/.claude/loop-roadmap.md)** drove the framework's own doc + implementation work for weeks via an external cron × a hand-maintained Markdown checklist × "one coherent unit per fire". It worked, but every element was ad-hoc: opaque out-of-repo cron IDs, three drifting Markdown goal-stores, and an **honor-system** `🔒 HELD` gate that nothing enforced. It ended not by completing but by *supersession* — a human re-pointed it.
2. **The native daemon** (`bwoc-agent --serve`) is already a persistent tick loop (`crates/bwoc-agent/src/main.rs`), but its only "goals" are *a message arrived* or *a task became claimable*, and its cadences began as hardcoded constants (the Saṅgha task-poll is now operator-tunable via `BWOC_TASK_POLL_SECS` — a first step; a first-class Ticker generalizes it).

Loop engineering closes the gap: it turns the honor-system Markdown pattern into **typed objects and enforced gates**, reusing the battle-tested primitives.

## The three objects

### Goal

A persistent objective plus a **machine-checkable Definition-of-Done**.

```
Goal {
  objective:  string        # human-legible intent
  dod:        Predicate      # evaluated each fire: is the goal met?
  budget:     { iterations?, wall_clock?, tokens?, cost? }  # cross-iteration ceiling
}
```

A `Goal` differs from a Saṅgha `task`: a task has a *state* (`pending → in_progress → completed`); a goal has a *done predicate* re-evaluated every fire (e.g. "team T's task list is fully `Completed`", "no un-mined session older than 24 h", "service healthy for M consecutive checks"). Tasks are the unit of work a goal decomposes into.

### Ticker

What fires the next iteration. A single abstraction over four sources:

```
Ticker =
  | Cron    "0 9 * * MON"      # calendar cadence
  | Every   Duration           # fixed interval
  | Event   <source>           # inbox/task-mtime/webhook/A2A push
  | Adaptive { base, backoff } # widen when idle, tighten when active
```

This generalizes what the daemon already does with a single cadence: its task-poll was lifted from a hardcoded `TASK_POLL_EVERY = 2s` to the operator-tunable `BWOC_TASK_POLL_SECS` (backed by `bwoc-core`'s `Ticker::every_secs`); the full Ticker adds the other three sources. The steering **prompt/objective is attached to the ticker**, exactly as the Refinement Loop attached a prompt to each cron — re-aiming the loop means swapping the objective, not the machinery.

### Gate

What decides, each fire, whether to act, pause, or stop. Composed of already-shipping gates:

| Gate | Meaning | Backed by |
|---|---|---|
| **DoD met** | goal predicate true → stop (success) | new predicate check |
| **HELD** | needs user policy → surface, never auto-act | plan-approval (Pavāraṇā) `crates/bwoc-cli/src/sangha.rs`, trust posture |
| **Budget** | iteration/wall-clock/token/cost ceiling → stop | `crates/bwoc-harness/src/budget.rs`; rolling-window pattern from `crates/bwoc-cli/src/supervise.rs` |

The `🔒 HELD` convention that the Refinement Loop honored by hand becomes an **enforced** gate here: a HELD item routes to the plan-approval flow and cannot be auto-actioned.

## The iteration cycle

Each ticker fire runs one turn of: **evaluate DoD → if met, stop → else select one coherent unit → execute it → log/discover → re-gate**.

- **One coherent unit per fire** (*Mattaññutā*) — one lead drain, one warm-harness turn, one mine — not a broad multi-step burst. This is the Refinement Loop's core discipline, now a design invariant.
- **Discover → schedule** — work found mid-loop (a bug, a follow-up) is captured as a new Saṅgha task via `bwoc task add`, replacing the Refinement Loop's free-text "Discovered" append-log. This is the **trigger → task bridge**: a signal becomes a scheduled, gated unit of work rather than a prose note.

## Grounding — from ad-hoc loop to enforced layer

Every element of the retired Refinement Loop maps to a BWOC-native equivalent that replaces its ad-hoc mechanism:

| Refinement Loop (ad-hoc) | Loop-engineering equivalent (enforced) |
|---|---|
| cron ID + `/loop` skill | **Ticker** on the daemon idle loop |
| Markdown tiered checklist | Saṅgha task queue (`tasks.jsonl`) + **Goal / DoD** object |
| "one coherent unit per fire" (Mattaññutā) | one lead drain / one harness turn per fire — a design invariant |
| "Discovered" append-log | retro triggers → `bwoc task add` (**trigger → task bridge**) |
| `🔒 HELD` honor-system | plan-approval gate (Pavāraṇā) + trust posture — **enforced** |
| CHANGELOG + git + version ledger | task states + retro metrics + budget accounting |

## Use-case catalog

Loops the layer enables. All share the **same missing core** (`Goal + Ticker + Gate`); the "net-new" column is what each needs beyond it.

### Internal / fleet

| Goal | Ticker | One-fire action | Net-new beyond the core |
|---|---|---|---|
| **Drive team T's tasks to all-`Completed`** | task-mtime event | one `run_lead` drain (`crates/bwoc-harness/src/lead.rs:152`) | DoD predicate + re-fire wrapper — *cheapest win* |
| **Keep fleet-health conditions green** | interval | `bwoc fleet health` → `bwoc doctor --auto` on Warn | timer glue + auto-fixable-class policy |
| **Keep each agent's Tier-2 memory current** | adaptive / nightly | `bwoc memory mine <sessions> <agent>` | session cursor + scheduler entry |
| **A dated retro/report per period** | cron | `bwoc retro new` (metrics-prefill) + `bwoc report` | calendar trigger + one-per-period idempotency |
| **Framework self-improvement** (the retired loop, productized) | run-end event | retro `Trigger` → `bwoc task add` → lead drains it | trigger→task bridge + multi-run DoD |
| **Ship a release** | operator kick | `bwoc run` gates → tag → notes | release orchestration (tag / semver / changelog) |

### External / product

| Goal | Ticker | One-fire action | Note |
|---|---|---|---|
| **Keep repo(s) green + current** (CI-babysit) | interval / CI webhook | trusted headless turn: bump → build → open PR | productizes the release-PR work |
| **Resolve an inbound request** | message arrival + follow-ups | warm per-sender turn via `AutoProcessor` | needs a *middle trust tier* (act-as-user) |
| **Watch a source, alert on trip** | cron / interval | fetch → predicate → `bwoc send` alert | **flagship**: missing piece is just a scheduler |
| **Deliver a recurring digest** | cron | aggregate → render → deliver | one-per-period idempotency |
| **Delegate a sub-goal to a peer** | poll / A2A push | `message/send` → `tasks/get` until `Completed` | needs the driver loop + join |
| **Drive an incident to recovery** | alert → tightened cadence | read-only diagnose → notify → verify | dynamic cadence + recovery gate |
| **Run a research→draft→publish pipeline** | cron | multi-step research (MCP/web) → draft → deliver/commit | staged pipeline state + a review-before-publish gate |

## Build plan (phased)

1. **Phase L1 — Goal loop over the lead** *(highest ROI, lowest risk)* — **shipped**. `bwoc-harness --lead --loop` wraps the already-hardened `run_lead` in a `Goal + Ticker + Gate`: re-fire on task-list change, DoD = list fully `Completed`, HELD on a `requires_plan` task, budget-bounded so it provably halts. The `Ticker` + `Budget` primitives live in `bwoc-core::loop_control`, and the `bwoc loop` operator console (a ratatui TUI, `crates/bwoc-loop-tui`) starts, monitors, and edits a goal-loop over a team's task list.
2. **Phase L2 — Ticker-driven fleet loops** — **partially shipped**. `bwoc fleet health --loop` is a reconcile loop that drives the fleet to all-green, and the daemon's Saṅgha task-poll cadence is now operator-tunable (`BWOC_TASK_POLL_SECS`) rather than a hardcoded constant. The `Every` ticker ships; `Cron`/`Adaptive` and the Tier-2-mining loop are deferred until a consumer drives them.
3. **Phase L3 — Product loops** — **not started** (design-gated). Scheduled monitoring/alerting (the flagship external case), then inbound-service and A2A-delegation loops — these also require a **middle trust tier** (act-as-authenticated-user, between today's trusted-headless and untrusted-read-only) and an **idempotency/dedup** primitive.

## Non-goals & safety

- **Not an always-running autopilot.** Every loop must have a provable stop: a DoD, a budget, or a HELD surface. A loop with no terminal condition (the BabyAGI/AutoGPT failure mode) is rejected at spec time — the budget gate is mandatory.
- **HELD is enforced, not advised.** Policy-bearing work routes to plan-approval; the loop cannot draft or action it.
- **Trust posture is preserved.** Untrusted inbound loops stay read-only; effectful loops stay trusted-or-approved. New product loops that "act as a user" require the L3 middle tier — they are not enabled by widening the existing binary.
- **Durability is deferred.** L1–L2 loops are daemon-hosted (crash-restart via `bwoc supervise`); durable pause/resume across restarts (Temporal-style) is a later concern, tracked when a real loop needs to survive a restart mid-goal.

## Cross-references

- [Refinement Loop (retired)](https://github.com/bemindlabs/BWOC-Framework/blob/main/.claude/loop-roadmap.md) — the ad-hoc prototype this layer internalizes.
- Operator console: [`crates/bwoc-loop-tui`](../../crates/bwoc-loop-tui/src/lib.rs) — `bwoc loop`, the ratatui TUI that starts / monitors / edits an L1 goal-loop over a team.
- [`ROADMAP.en.md`](ROADMAP.en.md) — where L1–L3 will be ticketed.
- [`FLEET-GOVERNANCE.en.md`](FLEET-GOVERNANCE.en.md) — the fleet-health conditions the monitoring loop drives.
- Saṅgha teams + task queue: [`sangha.md`](../../modules/agent-template/interconnect/sangha.md).
