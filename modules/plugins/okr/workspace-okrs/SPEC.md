---
title: workspace-okrs — Objectives + Key Results Tracker
aliases:
  - workspace-okrs
tags:
  - group/framework-plugins
  - type/plugin
  - kind/okr
  - domain/reporting
maturity: L1
---

# workspace-okrs — Objectives + Key Results Tracker

> [!abstract] The reference `okr` plugin for `BWOC-EPIC-4`. It tracks operator-authored Objectives + Key Results held in two local TOML files ([[objectives|objectives.toml]] + [[key_results|key_results.toml]]) and emits a normative progress report. Verbs: `track` (update a key result's `current` value — the only write, and it touches the operator's own local file, so **no** confirmation gate), `check-progress` (per-KR status + per-objective rollup), `report` (full [[../../../docs/en/PLUGINS.en#OKR Progress Schema|OKR Progress Schema]] JSON). **Local-file-only** — no network, no credential, no external system of record. Full framing: [[../../../notes/2026-05-28_okr-plugin-architecture|BWOC-46 design note]].

## Why the `okr` kind

`okr` is the framework's third **reporting** kind, alongside `audit`. Where `audit` checks the *workspace* against an external standard and emits findings, `okr` tracks *operator-authored* goals and emits progress. It is **not** a `workflow` kind: it reaches no external system, holds no credential, and its only write touches the operator's own local TOML — not a system of record — so it carries no operator-confirmation gate. The reuse of the audit [[../../../docs/en/PLUGINS.en#Evidence kinds|Evidence kinds]] (rather than inventing OKR-specific ones) keeps one evidence vocabulary across the framework. Full rationale: design note decisions 1, 3, 4.

## Data shape

Two operator-authored files ship in this directory as a self-contained seed example (the `BWOC-EPIC-4` objective tracking its own delivery):

### `objectives.toml` — the Objectives

| Field | Type | Required | Meaning |
|---|---|---|---|
| `objective_id` | string | yes | Stable id; referenced by `key_results.objective_id`. |
| `title` | string | yes | One-line statement of the objective. |
| `owner` | string | yes | Who owns it (an agent id or a person). |
| `period` | string | yes | The scoping window, e.g. `2026-Q2`. |
| `parent` | string | no | Parent `objective_id` for objective trees (`""` when top-level; multi-level rollup is deferred — design note §Status). |

### `key_results.toml` — the Key Results

| Field | Type | Required | Meaning |
|---|---|---|---|
| `key_result_id` | string | yes | Stable id, unique within the plugin. |
| `objective_id` | string | yes | **Referential** — must resolve to an `objectives.toml` id (enforced by `bwoc check`, BWOC-50). |
| `description` | string | yes | What the key result measures. |
| `target` | number | yes | The goal value. |
| `current` | number | yes | Latest tracked value (written by `track`). |
| `unit` | enum | yes | `count` \| `percent` \| `currency` \| `ratio` \| `boolean` (boolean uses `0`/`1`). |
| `confidence` | enum | yes | `high` \| `medium` \| `low` — qualitative trajectory read. |
| `evidence` | inline table | yes | `{ kind, value }` — reuses the audit Evidence kinds (`file` \| `content` \| `command` \| `attestation` \| `sample` \| `none`). |
| `as_of` | string | no | ISO-8601 date `current` was last tracked; omitted when never tracked. |

## Verbs

| Verb | Inputs | Output | Side effect |
|---|---|---|---|
| `track` | `--key-result <id> --current <value> [--evidence <kind:value>]` | The updated KR as one progress entry | Writes `current` (+ `evidence`, + `as_of`) back to `key_results.toml`. **Local-file write only** — no gate (design note §3). |
| `check-progress` | — | Per-KR status + per-objective rollup | None — read-only. |
| `report` | — | Full OKR Progress Schema JSON array for every KR | None — read-only. |

### `check-progress` heuristic

`attainment = current / target` (for `boolean`, `current ≥ target → 1`, else `0`; a `target` of `0` is treated as met). Status is then:

- **on-track** — `attainment ≥ 0.7` **or** `confidence == high`
- **at-risk** — otherwise, when `confidence == medium`
- **off-track** — otherwise (`confidence == low`)

The `0.7` line is the canonical OKR "green" threshold. The heuristic is intentionally simple and documented (design note §3): attainment carries the quantitative signal, `confidence` the qualitative one. The time-phased "expected attainment by period elapsed" model is **deferred** — v1 uses the flat line. A per-objective status rolls up to the **worst** KR status (off-track > at-risk > on-track).

## How it runs

The `bwoc okr` CLI (`BWOC-48`) spawns `okr.sh` from this directory, mirroring how `bwoc audit run` spawns `audit.sh`: it sets `BWOC_OKR_OPERATION` + `BWOC_PLUGIN_DIR` and pipes a one-line JSON request on stdin. The script is equally runnable by hand (argv flags) for smoke tests.

| Channel | What it carries |
|---|---|
| `BWOC_OKR_OPERATION` (env) | The verb — `report` \| `track` (the dispatcher path; also the fallback for the verb when no argument is given). |
| stdin | The dispatcher's one-line JSON request, e.g. `{"operation":"track","key_result_id":"O1-KR2","current":2,"evidence":"file:..."}`. `track` reads its parameters from here when present. |
| arg 1 | The verb for hand-invocation — `track` \| `check-progress` \| `report`. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this directory; resolves `objectives.toml` / `key_results.toml`. Falls back to the script's own directory. |

The CLI invokes the plugin only with `report` (it derives `list` / `show` / its objective rollup from the report output) and `track`. `check-progress` is the plugin's own read-only convenience verb, exercised by hand. On success: exit `0`, one JSON document on stdout. On error: a human diagnostic on stderr + non-zero exit.

```bash
# hand-invocation (argv)
./okr.sh report
./okr.sh check-progress
./okr.sh track --key-result O1-KR2 --current 2 --evidence "file:crates/bwoc-cli/src/okr.rs"

# dispatcher path (stdin JSON)
echo '{"operation":"track","key_result_id":"O1-KR2","current":2}' | BWOC_OKR_OPERATION=track ./okr.sh
```

## Output shapes

### `report`

A JSON array of progress entries conforming to [[../../../docs/en/PLUGINS.en#OKR Progress Schema|OKR Progress Schema]]:

```json
[
  {
    "objective_id": "O1",
    "key_result_id": "O1-KR1",
    "target": 1,
    "current": 1,
    "unit": "count",
    "confidence": "high",
    "evidence": { "kind": "file", "value": "docs/en/PLUGINS.en.md" },
    "as_of": "2026-05-28"
  }
]
```

`as_of` is omitted (not `null`) for a never-tracked key result.

### `check-progress`

```json
{
  "plugin": "workspace-okrs",
  "operation": "check-progress",
  "expected_attainment": 0.7,
  "key_results": [
    { "key_result_id": "O1-KR1", "objective_id": "O1", "attainment": 1.0, "status": "on-track" },
    { "key_result_id": "O1-KR2", "objective_id": "O1", "attainment": 0.0, "status": "at-risk" }
  ],
  "objectives": [
    { "objective_id": "O1", "title": "Ship the OKR plugin kind (BWOC-EPIC-4)", "status": "at-risk",
      "counts": { "on_track": 2, "at_risk": 2, "off_track": 0, "total": 4 } }
  ]
}
```

### `track`

Emits the single updated key result as a progress entry (the same shape as a `report` element), reflecting the new `current` / `evidence` / `as_of`.

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON document on stdout. |
| `1` | dependency / IO | `jq` missing, a data file missing, or malformed TOML (non-numeric `target`/`current`, missing required field). |
| `2` | usage | Unknown verb, missing/invalid flag, invalid `--evidence` kind, or `--key-result` not found. |

Missing or malformed TOML fails **gracefully**: a clear stderr message + non-zero exit; the plugin never panics. (`jq` is the one runtime dependency, matching the `workflow/gcloud-*` reference plugins.)

## Configuration

```toml
# workspace.toml
[plugins.workspace-okrs]
enabled = true
```

No `[config.schema]` — v1 reads its Objectives + Key Results from the sibling TOML files. The only workspace-level surface is the universal `enabled` key.

## Lifecycle mapping

Per [[../../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]], the `okr` kind is invoked by the `bwoc okr` CLI. `init`/`teardown` are per-invocation around `invoke`. The plugin holds no state beyond the two TOML files it reads (and, for `track`, writes).

| Phase | What this plugin does |
|---|---|
| `init` | Implicit; verify `jq` on PATH; resolve the data files. |
| `invoke` | Parse the verb, read `objectives.toml` / `key_results.toml`, emit JSON (and, for `track`, rewrite `key_results.toml`). |
| `teardown` | Implicit; no state to release. |

## Idempotency

- `report` and `check-progress` are read-only.
- `track` is idempotent at the operation level: tracking a key result to the same `current` + `evidence` rewrites the same bytes (only `as_of` advances to today). Replays after a transient failure converge — the write is atomic (temp file + `mv`).

## Maturity

Declared **L1** — first runnable `okr/workspace-okrs` reference plugin; all three verbs functional against the seed data. Bumps to **L2** once the `bwoc check` extension (`BWOC-50`) validates the manifest + Progress Schema + referential integrity, and the `bwoc okr` CLI (`BWOC-48`) exercises the verbs end-to-end.

> [!note] Validation split. This plugin makes `bwoc check` accept the `okr` kind at the basic-well-formedness level (the kind is in the supported set). Deep okr-specific validation — `objectives.toml` ↔ `key_results.toml` referential integrity, `confidence` enum enforcement, and Progress Schema field validation — lands in `BWOC-50` (owner: `agent-rose`), which is blocked on this story.

## Neutrality

Manifest values name no LLM backend or model. `kind = "okr"` is the framework's own enum value. No vendor name appears in `kind`, `entry`, or any config key. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-28_okr-plugin-architecture|BWOC-46 design note]] — full framing (decisions 1–6).
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `okr` kind row + OKR Progress Schema.
- [[objectives|objectives.toml]] / [[key_results|key_results.toml]] — the operator-authored data (seed example).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
