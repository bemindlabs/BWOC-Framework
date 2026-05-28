---
title: OKR Plugin Kind — architecture framing for EPIC-4
date: 2026-05-28
sprint: BWOC sprint-9
epic: BWOC-EPIC-4
story: BWOC-46
related_stories: [BWOC-47, BWOC-48, BWOC-49, BWOC-50]
---

# 2026-05-28 — OKR Plugin Kind (EPIC-4 framing)

This note sets the spec frame for `BWOC-EPIC-4` before any code or spec lands. It answers the design questions Sprint 9 must resolve so `BWOC-47` (PLUGINS spec + OKR Progress Schema), `BWOC-48` (`bwoc okr` CLI), `BWOC-49` (the `workspace-okrs` reference plugin), and `BWOC-50` (the `bwoc check` extension) can be drafted without churn: why `okr` is its own plugin kind rather than a `workflow` or `audit` plugin, what the `objectives.toml` + `key_results.toml` data shape is, what the `track` / `check-progress` / `report` verbs do, how the OKR Progress Schema reuses the audit `evidence` model, and why `confidence` is an enum rather than a number.

The throughline: **`okr` is the framework's third reporting kind, and the first that is operator-data-driven rather than workspace-derived.** `audit/*` reads the workspace and emits findings against an external standard; `okr/*` reads operator-authored objectives + key results and emits a progress report. Both are read-only over an `invoke` — they never mutate an external system of record (the property that earned `jira` and motivates `gcloud`'s write verbs their own treatment). OKR sits firmly on the reporting side of that line.

## Decisions

### 1. `okr` is a distinct plugin kind — a reporting kind, not `workflow` or `audit`

The current PLUGINS spec enumerates five kinds in [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds): `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`. `BWOC-47` adds `okr` as the sixth. Per the kind-boundary precedent ([ISO-compliance note](2026-05-26_iso-compliance-plugins.md#1-audit-becomes-the-4th-plugin-kind-not-compliance-or-policy), [jira note §1](2026-05-27_jira-plugin-architecture.md)), a kind earns its place by **the lifecycle hook the framework calls and who owns the call site**. `okr` earns it:

- **Not `audit`.** An `audit/*` plugin checks the *workspace* against an external standard and emits *findings* (pass/fail per criterion). An `okr/*` plugin tracks *operator-authored goals* and emits *progress* (current-vs-target per key result). They share the report-emitting shape but differ in subject (workspace state vs operator goals), data source (the repo vs `objectives.toml`/`key_results.toml`), and verbs (`audit run` vs `track`/`check-progress`/`report`). Overloading `audit` with goal-tracking would blur the Findings Schema with a Progress Schema — two normative shapes under one kind.
- **Not `workflow`.** A `workflow/*` plugin integrates an *external system* (jira, gcloud) — it reaches out over a network, handles auth, and (for write-capable ones) mutates external state. `okr/*` reads local TOML files and emits JSON. No network, no auth, no external mutation. Forcing it under `workflow` would misrepresent its blast radius (local-file-only) as integration-grade.

`okr` is invoked by the `bwoc okr` CLI (mirroring `bwoc audit`), reads operator-authored local files, and emits a normative report. That call site + lifecycle is distinct enough to be its own kind.

> [!note]
> Three reporting kinds now exist or are planned: `audit` (workspace-vs-standard), `okr` (operator-goals-vs-targets), and the planned `council` (decision protocol). They share "read + emit, never mutate external state" — the reporting family — and stand opposite the integration family (`workflow`, `jira`, future `gcloud` write slices).

### 2. Data shape — `objectives.toml` + `key_results.toml`, operator-authored

An `okr/*` plugin ships (or an operator authors) two TOML files:

```toml
# objectives.toml — the O's. Operator-authored.
[[objective]]
objective_id = "O1"
title        = "Ship the OKR plugin kind"
owner        = "agent-jisoo"
period       = "2026-Q2"
parent       = ""              # optional — supports objective trees
```

```toml
# key_results.toml — the KR's. Operator-authored; current values updated via `track`.
[[key_result]]
key_result_id = "O1-KR1"
objective_id  = "O1"           # referential — must resolve to an objective
description   = "PLUGINS spec declares the okr kind"
target        = 1
current       = 0
unit          = "count"        # count | percent | currency | ratio | boolean
confidence    = "medium"       # enum: high | medium | low
evidence      = { kind = "none", value = "" }
```

The split (objectives vs key results) mirrors how OKRs are actually authored — a handful of objectives, each with 2-5 measurable key results — and lets `check-progress` roll KR status up to objective status. **Referential integrity** (`key_result.objective_id` resolves to an `objective.objective_id`) is the load-bearing invariant `bwoc check` enforces (`BWOC-50`).

### 3. Verbs — `track`, `check-progress`, `report`

`bwoc okr <verb> <plugin>` dispatches the plugin (mirroring `bwoc audit run`). Three verbs:

| Verb | Inputs | Output | Side effect |
|---|---|---|---|
| `track` | `--key-result <id> --current <value> [--evidence <ref>]` | The updated KR row | Writes the new `current` (+ `evidence`) back to `key_results.toml`. **Local-file write only** — the operator's own goal data, not an external system. |
| `check-progress` | — | Per-KR status (`on-track` / `at-risk` / `off-track`) + per-objective rollup | None — read-only. |
| `report` | — | Full OKR Progress Schema JSON for every KR | None — read-only. |

`check-progress` derives status from a simple, transparent heuristic: `attainment = current / target`; combined with `confidence`, it labels `on-track` (attainment ≥ expected-for-period **or** confidence high), `at-risk` (lagging but confidence medium), `off-track` (lagging + confidence low). The heuristic is intentionally simple and documented — OKR scoring is a judgment aid, not a precise metric. `report` emits the raw schema and leaves interpretation to the reader.

> [!note]
> `track` is the one verb that writes — but it writes the **operator's own** `key_results.toml`, not an external system of record. This is categorically different from `jira sync` or `gcloud set-default`: no network, no credentials, fully reversible (it's a tracked file the operator can `git diff`). It therefore needs **no** operator-confirmation gate — it is the OKR equivalent of editing a local config file.

### 4. OKR Progress Schema — reuse the audit `evidence` model

`BWOC-47` adds a normative **OKR Progress Schema** to PLUGINS (parallel to the [Audit Findings Schema](../docs/en/PLUGINS.en.md#audit-findings-schema) and the [Jira Issue Mapping Schema](../docs/en/PLUGINS.en.md#jira-issue-mapping-schema)). Fields:

| Field | Type | Required | Notes |
|---|---|---|---|
| `objective_id` | string | yes | Resolves to an objective. |
| `key_result_id` | string | yes | Unique KR identifier. |
| `target` | number | yes | The goal value. |
| `current` | number | yes | Latest tracked value. |
| `unit` | enum | yes | `count` \| `percent` \| `currency` \| `ratio` \| `boolean`. |
| `confidence` | enum | yes | `high` \| `medium` \| `low` (see Decision 5). |
| `evidence` | object | yes | **Reuses the audit `evidence` kinds** — `{ kind, value, ... }` where `kind ∈ { file, content, command, attestation, sample, none }`. |
| `as_of` | ISO date | optional | When `current` was last tracked. |

**The `evidence` reuse is deliberate and non-negotiable.** The audit Findings Schema already defines a reproducible-evidence model (the **Musāvāda** guard — no claim without a referent). An OKR key result claiming `current = 0.8` should carry the same kind of referent (a file, a command output, an attestation) as an audit finding. Re-using the kinds rather than inventing OKR-specific evidence keeps one evidence vocabulary across the framework and lets `bwoc check` reuse the existing evidence validator. `BWOC-46`'s recommendation: **do not invent new evidence kinds for OKR.** If a genuine gap appears (e.g. a "metric-snapshot" kind), raise it against the shared Evidence kinds section, not as an OKR-local addition.

### 5. `confidence` is an enum {high, medium, low}, not a numeric score

OKR tooling often uses a 0.0-1.0 confidence score. This spec uses a **three-value enum** instead. Rationale:

- **Honest precision.** A `0.7` confidence implies a calibration the operator rarely has; `medium` does not over-claim. Mattaññutā — the right amount of precision.
- **Validation simplicity.** `bwoc check` enforces membership in a three-value set — no range/precision rules.
- **Consistency.** It mirrors the framework's preference for small enums over free scalars (e.g. severity in the Findings Schema is an enum, not a number).

Attainment (`current / target`) carries the quantitative signal; `confidence` carries the *operator's qualitative read* of whether the trajectory holds. Two axes, each in its natural type.

### 6. Reference designs scanned — adopt vs reject

Surveyed before fixing the shape:

| Source | Adopted | Rejected |
|---|---|---|
| **GitLab OKR** | Objective→KR hierarchy; period scoping | Their issue-tracker coupling (we stay file-based, no tracker dependency) |
| **Notion OKR** | Simple target/current/unit per KR | Free-form rollup formulas (we use a fixed, documented heuristic) |
| **Lattice** | Confidence as a coarse signal (red/yellow/green) → our `low/medium/high` | Their continuous check-in cadence + notifications (out of scope; no scheduler) |
| **Weekdone** | Status rollup O ← KRs | Their weekly-PPP reporting model (not a framework concern) |

The throughline of the rejections: **OKR-the-plugin tracks goals as local data and reports on them; it is not a goal-management SaaS.** No notifications, no cadence enforcement, no tracker coupling — those belong to whatever consumes the `report` output.

## Alternatives considered

- **Fold OKR into `audit` as a second report type** — rejected (Decision 1). Two normative schemas under one kind blurs the Findings/Progress distinction and overloads `bwoc audit`.
- **Make `okr` a `workflow` plugin** — rejected (Decision 1). It misrepresents a local-file reporter as an external integration.
- **Numeric confidence score** — rejected (Decision 5). Over-claims precision; enum is honest + simpler to validate.
- **OKR-specific evidence kinds** — rejected (Decision 4). Reuse the audit evidence vocabulary; one model across the framework.
- **A single `okrs.toml`** instead of `objectives.toml` + `key_results.toml` — rejected. The split mirrors authoring reality and makes the objective↔KR referential check (and the rollup) cleaner.
- **Gate `track` behind operator confirmation** — rejected (Decision 3). It writes the operator's own local file, fully reversible; a confirmation gate would be ceremony without risk.

## Status / deferred

- Decisions 1-6 frozen for EPIC-4 unless `BWOC-47`/`BWOC-49` surface a concrete contradiction during impl.
- The `check-progress` heuristic (attainment × confidence → status) is fixed here at the simple form; a richer time-phased model (expected attainment by period elapsed) is **deferred** — the simple form ships first, the spec leaves room.
- Objective **trees** (`parent` field) are declared in the shape but the rollup in `check-progress` ships flat (KR → O) first; multi-level O→O rollup is deferred.
- No scheduler / no notifications — OKR is read-on-demand via `bwoc okr`; any cadence is the operator's (or a future cron's) concern.

## Related

- Sprint 9 planning: [`.scrum/planning/sprint-9-planning.md`](../../.scrum/planning/sprint-9-planning.md) (workspace, not framework)
- EPIC-2 [ISO-compliance plugins note](2026-05-26_iso-compliance-plugins.md) — the `audit` reporting-kind precedent + the `evidence` model this schema reuses.
- EPIC-6 [jira-plugin-architecture note](2026-05-27_jira-plugin-architecture.md) — the write-capable integration kind OKR is explicitly *not*.
- EPIC-8 [gcloud-workflow-plugin note](2026-05-28_gcloud-workflow-plugin-architecture.md) — the other recent kind decision (reuse `workflow`); contrast with OKR earning its own kind.
- [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds) — the kind enumeration `BWOC-47` extends; the Findings + Issue Mapping schemas the Progress Schema parallels.
