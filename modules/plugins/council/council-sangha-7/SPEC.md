---
title: council-sangha-7 — Aparihaniya-dhamma 7 Consensus Council
aliases:
  - council-sangha-7
tags:
  - group/framework-plugins
  - type/plugin
  - kind/council
  - domain/coordination
maturity: L1
---

# council-sangha-7 — Aparihaniya-dhamma 7 Consensus Council

> [!abstract] The reference `council` plugin for `BWOC-EPIC-5`, and the framework's first **coordination**-kind plugin. It **records** a fleet decision through the protocol `propose → discuss → vote → resolve` under the **`sangha`** voting model — a decision passes only by **unanimous assent of the quorum** (abstentions allowed, dissent preserved); no tie is possible, so lack of concord re-opens a discuss round. Verbs (via [[protocol|protocol.sh]]): `propose`, `discuss`, `vote`, `resolve`, `list`, `show`, all operating on a record conforming to the [[../../../docs/en/PLUGINS.en#Council Decision Schema|Council Decision Schema]]. Seeded with the seven [[decisions|Aparihaniya-dhamma 7 templates]]. **Local/fleet-only** — no network, no credential. It **records** decisions; it never **executes** them — a `binding` outcome is noted as a `bwoc task` to emit, never carried out by the plugin. Full framing: [[../../../notes/2026-05-28_council-plugin-architecture|BWOC-56 design note]].

## Why the `council` kind

`council` is the framework's first **coordination** kind. Every prior kind acts *outward* to an external system (**integration**: `workflow` / `jira`) or *over the workspace* as a report (**reporting**: `audit` / `okr`). `council` acts *among the fleet's own agents* — it is the machine-readable counterpart to the Saṅgha governance already surfaced by `bwoc fleet` (the Aparihaniya-dhamma 7 signals). It turns "the agents should agree on X" from an ad-hoc inbox thread into a recorded decision with participants, rounds, votes, an outcome, and preserved dissent. Full rationale (why not `workflow`, why not `audit`): design note decision 1.

## The decision protocol

```
proposed → discussing → voting → resolved
                    ↘ (quorum not met) → abandoned
                    ↘ (no concord)      → back to discussing (another round)
```

The protocol is **append-only** per decision: rounds and votes accumulate, never overwrite — the record is the audit trail (Saccā, truthfulness of the deliberation). `participants` and `options` are **fixed at propose time**; a change to either is a new decision, not an edit (design note §4, schema §Field stability).

## The `sangha` voting model

This plugin declares `voting_model = "sangha"` (one of the four models in design note §3). A decision **resolves** only when both hold:

1. **Quorum** — the number of distinct participants who cast a vote (abstentions count as participation) is at least the manifest's `[council].quorum`, computed against the decision's `participants` (the team-roster snapshot). `"2/3"` of a 4-member roster rounds up to 3.
2. **Concord** — every **non-abstaining** voter is on the **same** option.

Outcomes:

| Condition | Result | `status` |
|---|---|---|
| Quorum not met | Abandoned | `abandoned` |
| Quorum met **and** all non-abstainers on one option | Resolved; `outcome` set | `resolved` |
| Quorum met but non-abstainers split (or all abstained) | **No concord → another round** (re-opened) | `discussing` |

There is **no tie-break** — by design, lack of concord is never resolved by a casting vote; it re-opens deliberation (Anattā: no agent's vote is privileged). An **abstention carrying a `--rationale`** is preserved on resolve as **dissent** — the Vinaya stand-aside: the participant does not block concord, but their reservation is recorded and never discarded.

## How it runs

`protocol.sh` resolves its verb from the first argument, or from `$BWOC_COUNCIL_OPERATION` when none is given (the dispatcher path). A `bwoc council` CLI (`BWOC-58`) may also pipe a one-line JSON request on stdin; **argv flags override stdin fields**. The script is fully runnable by hand for smoke tests.

| Channel | What it carries |
|---|---|
| arg 1 | The verb — `propose` \| `discuss` \| `vote` \| `resolve` \| `list` \| `show`. |
| `$BWOC_COUNCIL_OPERATION` (env) | The verb when no argument is given (dispatcher fallback). |
| stdin | An optional one-line JSON request; verbs read their parameters from it when present, argv overrides. |
| `$BWOC_PLUGIN_DIR` (env) | This directory; resolves `manifest.toml` + `decisions.toml`. Falls back to the script's own directory. |

**Decision records** persist as one JSON file per decision (`<decision_id>.json`) under the records directory, resolved in order:

1. `$BWOC_COUNCIL_DIR` — explicit override.
2. `$BWOC_WORKSPACE/.bwoc/council` — when a workspace is in context.
3. `$BWOC_PLUGIN_DIR/records` — plugin-local fallback (hand-invocation / smoke tests).

```bash
# propose from a template (seeds question + options), resolving participants from a team
./protocol.sh propose --decision-id D1 --template ap1-regular-meetings --team design-council

# or fully explicit
./protocol.sh propose --decision-id D1 --question "Adopt convention X?" --options "adopt,defer" \
  --participants "agent-jisoo,agent-jennie,agent-lisa,agent-rose" --effect advisory

./protocol.sh discuss --decision-id D1 --participant agent-jisoo --message-ref msg-20260528T120000Z-a1b2c
./protocol.sh vote    --decision-id D1 --participant agent-jisoo --option adopt
./protocol.sh vote    --decision-id D1 --participant agent-rose  --abstain --rationale "prefers to defer; stands aside"
./protocol.sh resolve --decision-id D1
./protocol.sh list
./protocol.sh show D1
```

## Verbs

| Verb | Inputs | Output | Side effect |
|---|---|---|---|
| `propose` | `--decision-id <id>` and (`--template <tid>` \| `--question <q> --options a,b`); optional `--team <id>` \| `--participants a,b`, `--effect advisory\|binding` (default `advisory`), `--evidence <kind:value>` (repeatable) | The new decision record (`status=proposed`) | Creates `<id>.json`. Refuses to clobber an existing id. |
| `discuss` | `--decision-id <id> --participant <agent> --message-ref <msg-id>` `[--round <n>]` | The updated record | Appends a turn `{ participant, message_ref }` to a round; `proposed → discussing`. |
| `vote` | `--decision-id <id> --participant <agent>` (`--option <opt>` \| `--abstain`) `[--rationale <text>]` | The updated record | Appends a vote `{ participant, option, abstain }`; `→ voting`. Re-cast appends (latest wins). |
| `resolve` | `--decision-id <id>` | `{ record, resolution }` — the closed/updated record plus a resolution summary | Tallies under the sangha rule; sets `outcome` + `dissent` and closes (`resolved`), or `abandoned`, or re-opens (`discussing`). |
| `list` | — | A JSON array of decision summaries | None — read-only. Skips (never dies on) a malformed record. |
| `show` | `--decision-id <id>` (or `show <id>`) | The full decision record | None — read-only. |

## Records, never executes

A `binding` decision's resolution carries a **`binding_task`** note — a *suggested* `bwoc task` for an agent to carry out the outcome. The plugin **never** mutates code or config and never emits the task itself: `council` is a coordination kind, not an execution kind (design note §5). The task-emission bridge is deferred — the foundation ships the record + the note; wiring the actual `bwoc task` emission follows once a binding council is used in anger.

## Output shapes

### `propose` / `discuss` / `vote`

Emit the full decision record (the [[../../../docs/en/PLUGINS.en#Council Decision Schema|Council Decision Schema]] shape):

```json
{
  "decision_id": "D1",
  "status": "voting",
  "question": "Shall the fleet hold standups on a fixed, frequent cadence?",
  "effect": "advisory",
  "participants": ["agent-jisoo","agent-jennie","agent-lisa","agent-rose"],
  "options": ["affirm-cadence","revise-cadence"],
  "rounds": [{ "round": 1, "turns": [{ "participant": "agent-jisoo", "message_ref": "msg-20260528T120000Z-a1b2c" }] }],
  "votes": [{ "participant": "agent-jisoo", "abstain": false, "option": "affirm-cadence" }],
  "evidence_links": [{ "kind": "file", "value": "notes/2026-05-28_council-plugin-architecture.md" }],
  "opened_at": "2026-05-28T12:00:00Z"
}
```

### `resolve` (concord)

```json
{
  "record": {
    "decision_id": "D1", "status": "resolved", "outcome": "affirm-cadence",
    "dissent": [{ "participant": "agent-rose", "rationale": "prefers a weekly cadence; stands aside" }],
    "closed_at": "2026-05-28T12:30:00Z", "...": "..."
  },
  "resolution": {
    "resolved": true, "status": "resolved", "concord": true,
    "outcome": "affirm-cadence", "quorum_required": 3, "quorum_voted": 4,
    "dissent": [{ "participant": "agent-rose", "rationale": "prefers a weekly cadence; stands aside" }]
  }
}
```

A `binding` decision additionally carries `resolution.binding_task` (the suggested `bwoc task`). When quorum is not met, `resolution` is `{ resolved: false, status: "abandoned", reason: "quorum not met", ... }`; when concord is absent, `{ resolved: false, status: "discussing", concord: false, reason: "...another round needed", options_chosen: [...] }`.

> [!note] Schema extensions. The persisted record adds three fields beyond the BWOC-57 Council Decision Schema's named set: `question` (the human prompt, for `list`/`show` readability), `effect` (`advisory`\|`binding`, from design note §5), and an optional `rationale` on a vote (the source of dissent on resolve). They sit alongside the schema's required fields without altering them — candidates for the schema to absorb in a future revision (flagged to the spec lead).

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON document on stdout. |
| `1` | dependency / IO | `jq` missing, a record or the manifest malformed, an IO/write failure. |
| `2` | usage | Unknown verb, missing/invalid flag, unknown decision id, a participant outside the roster, an option outside the declared options, or a closed decision. |

Missing team, missing/malformed record, and malformed manifest fail **gracefully**: a clear stderr diagnostic + non-zero exit; the plugin never panics. (`jq` is the one runtime dependency, matching the `okr/workspace-okrs` and `workflow/gcloud-*` reference plugins.)

## Configuration

```toml
# manifest.toml
[council]
voting_model = "sangha"   # this reference plugin's model
quorum       = "2/3"      # an integer, or a fraction of the participants

# workspace.toml
[plugins.council-sangha-7]
enabled = true
```

No `[config.schema]` — v1 reads templates from `decisions.toml` and persists records under the workspace (or a plugin-local fallback). The only workspace-level surface is the universal `enabled` key.

## Lifecycle mapping

Per [[../../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]], the `council` kind is invoked by the `bwoc council` CLI (`BWOC-58`). `init`/`teardown` are per-invocation around `invoke`. The plugin holds no state beyond the JSON records it reads and writes.

| Phase | What this plugin does |
|---|---|
| `init` | Implicit; verify `jq` on PATH; resolve `decisions.toml` + the records directory. |
| `invoke` | Parse the verb, read/append to the decision record, emit JSON. |
| `teardown` | Implicit; no state to release. |

## Idempotency

- `list` / `show` are read-only.
- `propose` refuses to clobber an existing decision id (participants/options are fixed at propose time) — a replay with the same id is rejected, not silently overwritten.
- `discuss` / `vote` are **append-only**; a replay appends another turn/vote. At tally, the **latest** vote per participant wins, so a re-cast converges. Writes are atomic (temp file + `mv`).
- `resolve` is deterministic over the current votes: re-running with the same votes yields the same outcome.

## Maturity

Declared **L1** — first runnable `council/council-sangha-7` reference plugin; all six verbs functional against the seed templates, the full `propose → discuss → vote → resolve` cycle exercised (concord, abstain-as-dissent, split→another-round, below-quorum→abandoned, re-cast→latest-wins). Bumps to **L2** once `bwoc check` deep-validation (`BWOC-60`) covers the `[council]` manifest + Decision Schema, and the `bwoc council` CLI (`BWOC-58`) exercises the verbs end-to-end against a live ≥2-agent fleet (the L2 bar per design note §Status).

> [!note] Validation split. This plugin makes `bwoc check` accept the `council` kind at the basic-well-formedness level (the kind is in the supported set; the `[council]` table is ignored, not rejected). Deep council-specific validation — `voting_model` ∈ the four models, `quorum` is an int-or-fraction, and Decision Schema field validation — lands in `BWOC-60` (owner: `agent-rose`), which is blocked on this story.

## Neutrality

Manifest values name no LLM backend or model. `kind = "council"` and `voting_model = "sangha"` are the framework's own enum values; `sangha` / `Aparihaniya-dhamma` are Pāli governance terms, not vendor names. No vendor name appears in `kind`, `entry`, or any config key. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-28_council-plugin-architecture|BWOC-56 design note]] — full framing (decisions 1–7).
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `council` kind row + Council Decision Schema.
- [[decisions|decisions.toml]] — the seven Aparihaniya-dhamma issue templates (seed data).
- [[protocol|protocol.sh]] — the protocol implementation.
- `bwoc fleet` / `crates/bwoc-cli/src/fleet.rs` — the live Aparihaniya-dhamma 7 governance signals this plugin formalizes into recorded decisions.
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
