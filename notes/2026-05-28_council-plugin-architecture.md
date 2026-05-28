---
title: Agents Council Plugin Kind — architecture framing for EPIC-5
date: 2026-05-28
sprint: BWOC sprint-10
epic: BWOC-EPIC-5
story: BWOC-56
related_stories: [BWOC-57, BWOC-58, BWOC-59, BWOC-60]
---

# 2026-05-28 — Agents Council Plugin Kind (EPIC-5 framing)

This note sets the spec frame for `BWOC-EPIC-5` before any code or spec lands. It answers the design questions Sprint 10 must resolve so `BWOC-57` (PLUGINS spec + Council Decision Schema), `BWOC-58` (`bwoc council` CLI), `BWOC-59` (the `council-sangha-7` reference plugin), and `BWOC-60` (the `bwoc check` extension) can be drafted without churn: why `council` is its own plugin kind, the decision protocol (rounds, votes, outcome), the voting models a plugin may declare, quorum + tie-break rules, what "binding" vs "advisory" means for the fleet, how it builds on `bwoc team` / `bwoc send` / `bwoc task`, and the Council Decision Schema shape.

The throughline: **`council` is the first plugin kind that coordinates the fleet itself.** Every prior kind acts *outward* (integration: `workflow`/`jira`/gcloud) or *over the workspace* (reporting: `audit`/`okr`). `council` acts *among the agents* — it is the framework's structured-decision substrate, the machine-readable counterpart to the Saṅgha governance already surfaced by `bwoc fleet` (the Aparihāniya-dhamma 7 signals). It turns "the agents should agree on X" from an ad-hoc inbox thread into a recorded decision with participants, rounds, votes, an outcome, and preserved dissent.

## Decisions

### 1. `council` is a distinct plugin kind — fleet coordination, not integration or reporting

The current PLUGINS spec enumerates six kinds after EPIC-4: `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`, `okr`. `BWOC-57` adds `council` as the **seventh**. It earns its own kind on the lifecycle-hook + call-site test:

- **Not `workflow`/`jira`.** Those reach an external system over a network. `council` reaches no external system — it coordinates *internal* fleet agents via the existing `bwoc team` roster and `bwoc send` inbox. No auth, no external write.
- **Not `audit`/`okr`.** Those *read and emit a report* (findings / progress). `council` is *stateful and interactive across time* — a decision opens, accumulates discussion rounds + votes from multiple agents, then closes with an outcome. Its lifecycle is a multi-step protocol with a quorum gate, not a single read→emit `invoke`. That protocol is the new lifecycle hook.

`council` is invoked by the `bwoc council` CLI, its participants are resolved from a `bwoc team`, its discussion turns route through `bwoc send`, and it persists a decision record. That call site + lifecycle is distinct enough to be its own kind.

> [!note]
> Kinds now group into three families: **integration** (`workflow`, `jira`) act outward; **reporting** (`audit`, `okr`) read + emit; **coordination** (`council`) acts among agents. `council` opens the third family.

### 2. The decision protocol — open → discuss (rounds) → vote → resolve

A council decision moves through explicit states:

```
proposed → discussing → voting → resolved
                    ↘ (quorum not met / withdrawn) → abandoned
```

- **`propose`** — any fleet agent (or the operator) opens a decision: a question + a set of `options`. Status `proposed`.
- **`discuss`** — participants add turns across one or more structured **rounds**. Each turn routes through `bwoc send` (the inbox is the transport; the council record is the ledger). Status `discussing`.
- **`vote`** — each participant casts a vote for an option (or abstains). Status `voting`.
- **`resolve`** — the protocol tallies per its voting model, checks quorum, records the `outcome` + any `dissent`, and closes. Status `resolved` (or `abandoned` if quorum fails).

`list` / `show` are the read verbs. The protocol is **append-only** per decision: turns and votes accumulate, never overwrite — the record is an audit trail (Saccā — truthfulness of the deliberation).

### 3. Voting models — the plugin declares one

A `council` plugin declares its `voting_model` in the manifest. Four are specified; the reference plugin (`council-sangha-7`) uses **consensus-seeking** (decision 6):

| Model | Resolve rule | Tie-break |
|---|---|---|
| `simple-majority` | option with > 50% of cast votes | re-open one discuss round, then operator decides |
| `consensus` | all non-abstaining participants on one option | no tie possible — unresolved → another round or `abandoned` |
| `weighted` | highest sum of per-participant weights (weights from the team manifest) | highest-weight participant's vote |
| `sangha` | Aparihāniya-dhamma style: meet in concord; a decision passes only by **unanimous assent of the quorum** (abstentions allowed, dissent recorded), mirroring the Vinaya's `apalokana`/`ñatti` consensus | no tie — lack of concord → another round |

The model is a plugin-level declaration so different councils (a fast simple-majority ops council vs a consensus design council) can coexist. `bwoc check` (BWOC-60) validates the declared model is one of the four.

### 4. Quorum + tie-break

- **Quorum** is declared in the plugin manifest as `quorum` — an integer (minimum participants who must vote) or a fraction of the team (e.g. `"2/3"`). `resolve` refuses (status → `abandoned`, surfaced to operator) if quorum is not met. Quorum is computed against the **team roster** the council references, not the whole fleet.
- **Tie-break** is per voting model (table above). The universal fallback when a model can't resolve is: **re-open one more discuss round; if still unresolved, surface to the operator** — the framework never silently breaks a tie. Anattā: no agent's vote is privileged by default (except the explicit `weighted` model).

### 5. Binding vs advisory

A decision declares `effect: "binding" | "advisory"`:

- **`advisory`** — the recorded outcome is a recommendation. Nothing in the framework enforces it; agents + operator are informed. This is the safe default.
- **`binding`** — the outcome is a commitment the fleet records as authoritative (e.g. "the council resolved to adopt convention X"). Still, the framework does **not** auto-execute a binding outcome — `council` records decisions, it does not perform side-effects. A binding decision that implies action produces a `bwoc task` (via the existing `bwoc task` foundation) for an agent to carry out; the council never mutates code or config itself. This keeps `council` a *coordination* kind, never an *execution* one — the same read-vs-write discipline the reporting kinds hold.

### 6. `council-sangha-7` — the reference plugin (Aparihāniya-dhamma 7)

`BWOC-59` ships `modules/plugins/council/council-sangha-7/`, modelled on the **Aparihāniya-dhamma 7** (the seven conditions of non-decline the Buddha gave the Vajjī, already surfaced as governance signals by `bwoc fleet`). It uses the `sangha` voting model (unanimous assent of the quorum, dissent recorded) and ships `decisions.toml` issue templates for the seven conditions (e.g. "meet often + in concord", "act in concord", "honor what is established"). It is the canonical example of a council that values concord over speed — the philosophical anchor for the kind, exactly as `audit-iso-29110` anchored the `audit` kind.

### 7. Council Decision Schema (for BWOC-57)

`BWOC-57` adds a normative **Council Decision Schema** to PLUGINS (parallel to Audit Findings / Jira Issue Mapping / OKR Progress). Fields:

| Field | Type | Required | Notes |
|---|---|---|---|
| `decision_id` | string | yes | Stable key for the decision. |
| `status` | enum | yes | `proposed` \| `discussing` \| `voting` \| `resolved` \| `abandoned`. |
| `participants` | array | yes | Agent ids drawn from the referenced team. |
| `options` | array | yes | The choices being decided among (≥2). |
| `rounds` | array | yes | Ordered discussion rounds; each carries turns `{ participant, message_ref }` where `message_ref` points at the `bwoc send` envelope (the inbox is the transport, the record references it — single source of truth, no duplication). |
| `votes` | array | yes | `{ participant, option, abstain }` per voter; append-only. |
| `outcome` | string | no | The resolved option. Omitted until `resolved`. |
| `dissent` | array | no | Recorded minority positions `{ participant, option, rationale }` — never discarded (preserving dissent is the point of a recorded council). |
| `evidence_links` | array | no | **Reuses the audit [Evidence kinds](#evidence-kinds)** — referents backing the decision; no council-specific evidence kinds. |
| `opened_at` / `closed_at` | ISO datetime | opened_at yes, closed_at no | Lifecycle timestamps; `closed_at` omitted until resolved/abandoned. |

Optional fields are omitted (not `null`) when absent, per the framework convention.

## Alternatives considered

- **Make council a framework feature, not a plugin** — rejected. The voting model + quorum + protocol vary by council type; a plugin lets `simple-majority` and `consensus` councils coexist and lets the operator author their own. Same reasoning that made `audit`/`okr` plugins not features.
- **Reuse `workflow`** — rejected (Decision 1). Council reaches no external system; it coordinates internal agents.
- **One hardcoded voting model** — rejected (Decision 3). The plugin declares its model so different councils serve different needs.
- **Auto-execute binding decisions** — rejected (Decision 5). Council records decisions and emits a `bwoc task`; it never performs side-effects. Coordination kind, not execution.
- **Store discussion turns inside the council record** — rejected (Decision 7). Turns route through `bwoc send`; the record holds a `message_ref`, not a copy — one source of truth, no drift between inbox and ledger.

## Status / deferred

- Decisions 1-7 frozen for EPIC-5 unless `BWOC-57`/`BWOC-59` surface a concrete contradiction.
- Multi-agent **live** e2e (propose → 2+ agents discuss → vote → resolve against a real fleet) is the L2 bar; the L1 ship is a scripted/simulated multi-participant exercise via the CLI. Live multi-agent verification may gate on operator availability of ≥2 spawned agents.
- The `weighted` model's weight source (team manifest field) is declared here but its manifest shape is finalized in `BWOC-57`.
- `bwoc task` emission from a binding outcome is specified as the discipline (Decision 5) but its wiring is deferred — the foundation ships the record; the task-emission bridge can follow once a binding council is actually used.

## Related

- Sprint 10 planning: [`.scrum/planning/sprint-10-planning.md`](../../.scrum/planning/sprint-10-planning.md) (workspace)
- EPIC-2 [ISO-compliance note](2026-05-26_iso-compliance-plugins.md) — the kind-boundary + `evidence` model this schema reuses.
- EPIC-4 [okr-plugin note](2026-05-28_okr-plugin-architecture.md) — the most recent kind decision (reporting family); council opens the coordination family.
- [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds) — the enumeration `BWOC-57` extends to seven.
- `bwoc fleet` / `crates/bwoc-cli/src/sangha.rs` — the existing Aparihāniya-dhamma 7 governance signals `council-sangha-7` formalizes into recorded decisions.
