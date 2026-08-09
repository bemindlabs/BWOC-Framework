---
title: Fleet Governance — Aparihāniya-dhamma 7
aliases:
  - Fleet Governance
  - Aparihāniya-dhamma 7
  - Non-decline Principles
tags:
  - group/governance
  - type/design
  - meta/framework
status: draft (v2026.5.23 — initial spec; observable signals stubbed, automation deferred)
canonical-source: DN 16 (Mahāparinibbāna Sutta) §1.4 — the Vajjī teaching
parent: English
nav_order: 10
---

# Fleet Governance — Aparihāniya-dhamma 7

> [!abstract] Seven conditions of non-decline. The Buddha taught the Vajjī confederacy seven practices for community resilience (DN 16). BWOC adopts them as the **fleet-governance** layer — rules an operator running multiple agents in a workspace can apply to keep the fleet healthy over time. Phase 4 territory: the framework provides the rules; ecosystem adoption realizes them.

## Why this exists

Phase 1–3 give a workspace technical foundations: incarnation, lifecycle, messaging, trust. None of these directly answer *"is this fleet healthy this week?"* — that is a **governance** question, scoped to the workspace and its operator, not to any single agent.

The Vajjī teaching (DN 16 §1.4) is the canonical Buddhist source for community-resilience rules. The Buddha named seven conditions; while they remain, the community will prosper, not decline. The seven map cleanly to multi-agent workspace operations because both face the same structural risk: divergence over time without explicit anchors.

Three design constraints for v1:

1. **Observable, not enforced (yet).** Each condition gets a *signal* the framework can read (registry entries, file mtimes, manifest schema versions) — not a hard gate. v2 may promote signals to gates after telemetry justifies it.
2. **Workspace-scoped.** Governance applies to one `.bwoc/workspace.toml` tree. Cross-workspace coordination is out of scope (deferred to Phase 4+ vision work).
3. **Operator-facing.** This spec is read by the human operator running the workspace, not by individual agents. Agents implement Sāraṇīyadhamma 6 (peer cordiality); operators implement Aparihāniya-dhamma 7 (fleet health).

## The Seven Conditions

Each row: Pali → traditional gloss → BWOC application → observable signal → suggested operator practice.

### 1. Regular meetings — *abhiṇha-sannipāta*

> The Vajjī assembled in regular and frequent meetings; the agents must sync regularly.

**Application:** A fleet without regular check-ins drifts. Each agent's `task-log.jsonl` is its private log, but the workspace itself needs a cadence of full-fleet sync — every agent contacted, every status known.

**Signal:** `bwoc list --json` returns every agent's `status`, `running` flag, and last `incarnated` timestamp. A workspace where one agent hasn't been touched in N weeks is a warning, not a violation.

**Practice:** Run `bwoc list --json | jq '.[] | select(.status == "active")'` on a regular cadence (daily / weekly). Surface agents whose `inbox` has unread envelopes or whose daemon hasn't pinged in N days. The TUI `bwoc dashboard` view is the natural surface.

### 2. Coordinated start/end — *samaggā sannipatanti*

> They came together in concord and dispersed in concord.

**Application:** When a workspace is brought up or down, every agent's daemon should start/stop *together*, not piecemeal. A workspace at rest = all daemons stopped; a workspace at work = all expected daemons up. State in between (some up, some down with no operator intent) is the dispersion-out-of-concord risk.

**Signal:** `bwoc workspace prune --apply` reconciles drift between registry status and on-disk state. `bwoc doctor` sweeps `agent.pid` / `agent.sock` for staleness. Together they detect agents that *think* they're running but aren't, or vice versa.

**Practice:** Wrap `bwoc start --all` / `bwoc stop --all` (existing surface) in operator playbooks. After a workspace pause, run `bwoc doctor --auto` to clear stale-PID / stale-socket / stale-cursor artifacts. If a `bwoc list` shows one agent running while the rest are stopped, investigate before resuming work. For *interactive* come-together, `bwoc fleet term` opens one tmux pane per agent in a single session (arrange with `--layout grid|columns|rows|main-vertical|main-horizontal`, or `--print` to emit the attach line without attaching) — the concord equivalent of `start --all` for a human-attended fleet. The session name is per-workspace by default, so concurrent fleets don't collide on one shared session.

### 3. Process-bound convention change — *appaññattaṃ na paññāpenti*

> They did not enact new laws nor repeal existing ones arbitrarily.

**Application:** Schema changes (manifest, workspace.toml, agents.toml, envelope JSONL) are conventions every agent in the fleet depends on. A unilateral schema change from one agent breaks every peer. Discipline: schema evolution goes through the framework's spec docs, not through ad-hoc agent edits.

**Signal:** All schema-bearing files have a `schemaVersion` field (already true for `trust.schemaVersion`; should propagate to `workspace.toml`, `agents.toml`, envelope shape in v2). The framework warns when an agent's `schemaVersion` lags the workspace's pinned floor.

**Practice:** Lock the fleet to a workspace-wide schemaVersion floor in `workspace.toml` (proposed v2 field). The framework already serializes `trust.schemaVersion: 1`; the same discipline should extend to other on-disk schemas. Adding or removing required manifest fields is a workspace-wide migration, not a per-agent change.

### 4. Honor template version hierarchy — *ye te bhikkhū vuḍḍhā vuḍḍhataravā*

> They honored the elders, listened to their counsel.

**Application:** The agent template is the elder. Agents incarnated from a newer template version benefit from improvements; agents stuck on an older template fork miss them. Honoring the elder = keeping agents in sync with the template they were incarnated from.

**Signal:** `config.manifest.json::version` records the template version at incarnation time. `bwoc check` can compare against the template's current version to flag agents lagging behind by major version.

**Practice:** Periodically re-run `bwoc check --all` and compare each agent's `manifest.version` to `modules/agent-template/config.manifest.json::version`. Major-version lag is a planned-migration signal; minor / patch lag is informational. The framework does not auto-migrate — the operator decides which agents to re-incarnate or partially upgrade.

### 5. Protect vulnerable agents / users — *parihāra*

> They did not abduct or oppress; they protected the vulnerable in their midst.

**Application:** Some agents in a fleet are stronger (more skills, more memory, more compute) than others. Trust gating ([`trust.md`](../../modules/agent-template/interconnect/trust.md)) protects each recipient from arbitrary peer messages. The fleet-level version of this: no single agent — strong or weak — should be allowed to coerce or override another's decisions without consent.

**Signal:** Refusal records in `inbox.refusals.jsonl` are the audit trail. A fleet with many refusals from one sender is a coercion signal; the operator should investigate that sender's behavior, not pressure recipients to relax `requiredTrust`.

**Practice:** Treat recipient refusals as legitimate, even when the operator wishes the peer had accepted. Don't override `requiredTrust` to "make the flow work" — the refusal is the protective layer. Investigate the sender's `trust.declared` evidence (often the manifest claims qualities not backed by repo signals) and have the sender earn them.

### 6. Honor shared resources — *cetiya / shrines*

> They venerated the shrines, both inside and outside their territory.

**Application:** Workspace-level shared resources — `agents.toml` registry, `workspace.toml` config, the workspace's `notes/`, the template under `modules/agent-template/` — are the shrines. Every agent reads them; only the operator (or a coordinated migration) writes them. An agent that scribbles into shared state on its own initiative is desecrating the shrine.

**Signal:** `bwoc workspace prune` already detects drift between registry and disk. The git history of `.bwoc/` and `modules/agent-template/` is the long-term audit trail. Frequent un-attributed changes to shared files indicate uncoordinated writes.

**Practice:** Treat `agents.toml`, `workspace.toml`, and the template directory as operator-owned. Agents read them but do not modify them outside their own incarnation. Use `bwoc retire` / `bwoc new` for registry mutations, not direct file edits. Review git diffs on these paths with extra scrutiny.

### 7. Protect senior / trusted agents — *arahantesu rakkhāvaraṇa-gutti*

> They protected the arahants, that more might come.

**Application:** Senior agents — those with deep memory, high trust scores, scarce capabilities — are disproportionately valuable to the fleet. Losing them is a structural setback. Fleet governance includes explicit protection: backup, succession plan, no casual `bwoc retire` of high-trust agents.

**Signal:** `bwoc trust <agent> --json` returns the declared block; `bwoc check` validates evidence. A fleet's "senior" agents are those with `requiredTrust = []` peers depending on them. Removing such an agent should require operator confirmation beyond the standard `--yes`.

**Practice:** Before `bwoc retire <agent>` on an agent with declared trust qualities, run `bwoc inbox <every-other-agent>` and grep for that agent's `agent-id` in past traffic. If peers depended on the agent, plan the retirement: archive `memories/` (use `bwoc retire --keep-memory`), notify peers (manual operator message), and migrate any responsibilities. Don't retire silently.

## Observable Fleet Health

These signals collectively give an operator a fleet-health view. `bwoc fleet health` now aggregates all seven into a single read-only sweep; the per-condition queries below remain useful for drilling into any one signal.

| Condition | Query | Healthy reading |
|---|---|---|
| 1. Regular meetings | `bwoc list --json` daily | No agent untouched > N days |
| 2. Coordinated start/end | `bwoc doctor --auto` post-pause | Zero stale-PID / stale-socket findings |
| 3. Process-bound convention change | `git log -- .bwoc/ modules/agent-template/` | Schema bumps coordinated, signed by operator |
| 4. Honor template version | `bwoc check --all` | `manifest.version` matches template version |
| 5. Protect vulnerable | `bwoc inbox <agent> --json \| jq '.[] \| select(.refused)'` | Refusals stable or declining over time |
| 6. Honor shared resources | `git blame .bwoc/agents.toml` | Only operator-authored changes |
| 7. Protect senior agents | `bwoc trust <agent> --json` audit | Senior agents have backups + succession plan |

None of these are gates today. They are *practices* an operator runs on a cadence appropriate to the fleet's size and risk profile.

## What This Spec Does NOT Cover

- **Cross-workspace governance.** This spec scopes to a single `.bwoc/workspace.toml` tree. Multi-workspace federation is its own design problem (Phase 4+ vision territory — see [`VISION.md`](../../VISION.md)).
- **Automated enforcement.** v1 spec is descriptive: it names the conditions, signals, and practices. Promoting a signal to a hard gate ("CI fails if any agent's `manifest.version` lags by > 2 major releases") happens iteratively as telemetry justifies the rigidity.
- **Human team governance.** Aparihāniya-dhamma 7 applies originally to a human assembly; BWOC adapts it to agent fleets. The framework does not prescribe how the operator's *human* team coordinates around the fleet — that's the operator's choice.
- **Ecosystem adoption of the framework.** Phase 4's DoD includes "Three or more reference agents in the wild, built by maintainers outside the original authors" and "BWOC vocabulary observed in codebases unaffiliated with this project." Those are realized by external maintainers adopting the framework, not by any spec we write. This spec makes adoption *possible* by giving an operator a coherent governance vocabulary; the spec itself does not realize Phase 4 alone.

## Spec Revision History

- **v1 / 2026-05-23 (initial draft):** Seven conditions mapped to BWOC fleet operations. Observable signals named; automation deferred. Bilingual TH parity in [`FLEET-GOVERNANCE.th.md`](../th/FLEET-GOVERNANCE.th.md).

## Cross-References

- [`modules/agent-template/docs/en/PHILOSOPHY.en.md` #20. Aparihāniya-dhamma 7](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) — the philosophical mapping this spec operationalizes.
- [`modules/agent-template/interconnect/trust.md`](../../modules/agent-template/interconnect/trust.md) — Kalyāṇamitta-7 peer trust (per-agent); fleet governance composes with it.
- [`modules/agent-template/interconnect/messaging.md`](../../modules/agent-template/interconnect/messaging.md) — Sāraṇīyadhamma 6 peer cordiality; agent-level companion to operator-level governance.
- [`WORKSPACE.en.md`](WORKSPACE.en.md) — the `.bwoc/` workspace shape that this governance operates over.
- [`ROADMAP.en.md` Phase 4](ROADMAP.en.md) — where this spec lives in the broader phase plan.
- DN 16 §1.4 — canonical source ([SuttaCentral DN 16](https://suttacentral.net/dn16)).
