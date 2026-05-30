---
title: Plugins
parent: English
nav_order: 12
---

# Framework Plugins

A **framework plugin** extends the framework with capabilities that do not belong in every agent but should be available to agents and workspaces that need them. Plugins are loaded by the **framework runtime** — they are operator-facing, not agent-facing.

This spec defines the plugin kinds, manifest format, lifecycle hooks, loading mechanism, and verification gates. The first reference plugin (`memory-tier2-noop`) ships alongside this spec — both lands and proves the format together.

> [!abstract] Status: initial scaffold. Manifest tables and lifecycle hooks below are normative; prose may be refined as story BWOC-1..3 work refines the contract. The first reference plugin lands in BWOC-7.

---

## Skill vs Plugin

Skills and plugins share a substrate (TOML manifest, neutrality gate, per-workspace opt-in) and split on **who invokes them**.

| | Skill | Plugin |
|---|---|---|
| Spec | [`SKILLS.en.md`](SKILLS.en.md) | this doc |
| Audience | Agent author | Workspace operator |
| Opt-in via | `<agent>/config.manifest.json` | `workspace.toml [plugins]` |
| Invoker | The agent during its own operation | The framework runtime |
| Example | worktree discipline, bilingual parity check | Tier 2 memory backend, additional LLM backend |
| Lifecycle scope | Per-agent | Per-workspace |

Pick the layer that matches *who turns it on*. If an individual agent's logic calls it, it is a skill. If the workspace loads it once for everyone, it is a plugin.

---

## Plugin Kinds

Every plugin declares a `kind`. Kinds define the lifecycle hooks the framework will call. Nine kinds ship with this spec:

| Kind | What it extends | Lifecycle owner |
|---|---|---|
| `memory-backend` | Tier 2 memory (semantic search, vector store, deep-memory CLI) | The agent's memory subsystem |
| `llm-backend` | Backends beyond the six declared (`claude`, `antigravity`, `codex`, `kimi`, `ollama`, `openai-compatible`) | `bwoc spawn` |
| `workflow` | External system integrations (issue trackers, code review, CI) | The agent calling out |
| `audit` | Inspection of the workspace against external standards (ISO/IEC 29110, ISO 9001, ISO 20000-1, ISO 27001) or operator-authored audits (license headers, doc parity, secret scans) | `bwoc audit` CLI |
| `jira` | Bidirectional sync with an external issue tracker (Jira Cloud) — reads issues via JQL and **writes** status transitions, field updates, and sprint assignment back to the tracker | `bwoc jira` CLI |
| `okr` | Tracking of operator-authored Objectives + Key Results — reads `objectives.toml` / `key_results.toml`, records progress, and emits a normative progress report | `bwoc okr` CLI |
| `council` | A structured decision protocol among fleet agents — any agent opens a decision, participants discuss in rounds and vote, an outcome is recorded with evidence + dissent | `bwoc council` CLI |
| `figma` | Read-mostly integration with Figma's REST API — fetches frame/node metadata, exports images, queries component libraries, and surfaces design tokens; bridges design→dev | `bwoc figma` CLI |
| `gws` | Read-mostly integration with Google Workspace REST APIs (Drive / Gmail / Calendar) via OAuth2 user scopes — lists/reads files, mail threads, and calendar events | `bwoc gws` CLI |

A plugin sets `kind` once. Cross-kind plugins are not supported — split them.

The `audit` kind was added in `BWOC-EPIC-2`; for the rationale (why `audit`, not `compliance` or `policy`) and the ISO standards roadmap that motivates it, see the [BWOC-19 design note](../../notes/2026-05-26_iso-compliance-plugins.md).

The `jira` kind was added in `BWOC-EPIC-6` as the framework's **first write-capable plugin kind** — an *integration adapter*, not a reporting kind. Every kind above it (`audit`, plus the planned reporting kinds) only **reads** the workspace and emits a report; `jira` reads **and writes** an external system of record. That single property — durable, hard-to-reverse external side-effects on `invoke` — is what sets it apart: it persists a sync ledger (`.scrum/jira-sync.json`), gates its write verbs behind operator confirmation, and carries a normative [Jira Issue Mapping Schema](#jira-issue-mapping-schema). For why it is a distinct kind rather than a `workflow` plugin, the auth model, JQL and rate-limit bounds, and the bidirectional conflict policy, see the [BWOC-40 design note](../../notes/2026-05-27_jira-plugin-architecture.md) — this spec declares the kind and the mapping schema and does not duplicate that rationale.

The `okr` kind was added in `BWOC-EPIC-4` as the framework's third **reporting** kind, alongside `audit`. Where `audit` checks the *workspace* against an external standard and emits findings, `okr` tracks *operator-authored* Objectives + Key Results (`objectives.toml` / `key_results.toml`) and emits progress. It is **not** a `workflow` kind: it reaches no external system, holds no credential, and its only write — `track`, which updates a key result's `current` value — touches the operator's own local TOML, not a system of record, so it carries **no** operator-confirmation gate. It carries a normative [OKR Progress Schema](#okr-progress-schema) and **reuses the audit [Evidence kinds](#evidence-kinds)** rather than inventing its own — one evidence vocabulary across the framework. For why `okr` is a distinct kind rather than an `audit` or `workflow` plugin, the data shape, the `track` / `check-progress` / `report` verb contracts, and the `confidence`-as-enum decision, see the [BWOC-46 design note](../../notes/2026-05-28_okr-plugin-architecture.md) — this spec declares the kind and the progress schema and does not duplicate that rationale.

The `council` kind was added in `BWOC-EPIC-5` as the framework's first **coordination** kind — it acts neither outward to an external system (like `workflow`/`jira`) nor over the workspace as a report (like `audit`/`okr`), but **among the fleet's own agents**. A council decision follows a multi-step protocol — `propose` → `discuss` (rounds) → `vote` → `resolve` — with a quorum gate and a declared voting model (`simple-majority` / `consensus` / `weighted` / `sangha`); it draws participants from a `bwoc team`, routes discussion turns through `bwoc send`, and persists a normative [Council Decision Schema](#council-decision-schema) with the outcome and any dissent preserved. It **records** decisions, it does not execute them — a `binding` outcome emits a `bwoc task` rather than mutating anything itself. For the protocol detail, the voting models, quorum + tie-break rules, the binding-vs-advisory distinction, and the `council-sangha-7` reference (modelled on Aparihāniya-dhamma 7), see the [BWOC-56 design note](../../notes/2026-05-28_council-plugin-architecture.md) — this spec declares the kind and the decision schema and does not duplicate that rationale.

The `figma` kind was added in `BWOC-EPIC-7` as a **read-mostly** integration with Figma's REST API. Like `jira` (and unlike `gcloud`, which reused `workflow`), it earns its own kind because it carries a normative [Figma Asset Mapping Schema](#figma-asset-mapping-schema) — a durable BWOC-owned relationship tying a Figma node to an exported artifact + design tokens; the rule is **own-kind when BWOC defines a normative schema over the integration, `workflow`-reuse when it is a passthrough with no BWOC-owned shape**. Unlike `jira`, `figma` never writes back to the external system: every verb either reads Figma (`fetch` / `tokens` / `status`) or writes **locally** (`export` drops a content-addressable image under `figma/exports/`), so it carries jira's schema discipline but none of its bidirectional-sync machinery — no ledger, no conflict policy, no operator-confirm gates. Auth is an operator personal access token (`BWOC_FIGMA_TOKEN` env / `.bwoc/secrets.toml`, shape-only in `auth.toml`, never committed). For the auth model, file-vs-team-library scope, REST rate-limit handling, and the export-caching strategy, see the [BWOC-61 design note](../../notes/2026-05-28_figma-plugin-architecture.md) — this spec declares the kind and the asset schema and does not duplicate that rationale.

The `gws` kind was added in `BWOC-EPIC-13` as a **read-mostly** integration with Google Workspace REST APIs (Drive / Gmail / Calendar). Like `figma`, it earns its own kind because it carries normative [Workspace resource schemas](#workspace-resource-schema) (a Drive file, a Gmail thread, a Calendar event) + an OAuth scope model. It is **not** part of `gcloud`: gcloud reaches GCP *infrastructure* through the local `gcloud` CLI with ADC/service-account; `gws` reaches productivity *apps* through the Workspace REST APIs with **OAuth2 user-consent scopes** (`drive.readonly` / `gmail.readonly` / `calendar.readonly`) — a different auth family and surface entirely. It ships a credential-foundation plugin (`gws-auth`) that the per-service plugins (`gws-drive` / `gws-gmail` / `gws-calendar`) source, the gcloud-* family shape. Read-mostly: write verbs (send mail, create event, upload file) are deferred to future slices, each inheriting the [write-verb operator-confirm gate](#write-verbs--the-operator-confirm-gate-normative). OAuth token via `BWOC_GWS_TOKEN` env / `.bwoc/secrets/gws-token.json` (shape-only in `auth.toml`, never committed). For the OAuth model, per-service scopes, pagination, and rate-limit handling, see the [BWOC-72 design note](../../notes/2026-05-28_google-workspace-plugin-architecture.md) — this spec declares the kind and the resource schemas and does not duplicate that rationale.

### Write verbs — the operator-confirm gate (normative)

Most plugin verbs read. A **write verb** — one whose `invoke` produces a durable side-effect outside the plugin's own address space — carries a **normative operator-confirm gate**. This pattern is shared across every write-capable plugin regardless of kind: `jira` (`transition` / `sync`), `workflow/gcloud-project` (`set-default`, local config), and the `workflow/gcloud-compute` instance lifecycle (`start` / `stop`, added in `BWOC-EPIC-9`, tracking [bemindlabs#96]).

The gate's contract:

1. **The gate lives at the operator boundary — the `bwoc <cli>` command, not the plugin.** One confirmation point per write; the plugin executes when invoked and does not re-implement (nor bypass) the gate.
2. **It shows the exact effect before acting** — the target, the current state, and the literal external command that will run (for shell-out plugins, with the `--` argument separator so user-supplied values can never be parsed as flags).
3. **Default is No.** An interactive operator answers a `y/N` prompt. A non-interactive context (a headless agent) must pass an explicit `--yes`, which an agent sets **only** when the operator has authorized that specific action — never auto-set.
4. **A refused or unconfirmed write reports "no change" with the reason** (Dhammānupassanā), never a bare failure or a silent write.

Read verbs carry **no** gate — they are free. **Destructive, irreversible verbs** (e.g. `delete`) are held to a higher bar than start/stop and are introduced deliberately, per-slice, with their own stronger gate — they are not shipped just because they are adjacent to a reversible write. See the per-integration design notes for each verb set's risk matrix (e.g. the [BWOC-66 gcloud-compute note](../../notes/2026-05-28_gcloud-compute-write-verbs.md)).

### What plugins are NOT

- **Not a loophole for vendor-specific framework logic.** The six declared backends are first-class and live in spec, not as plugins. Vendor phrasing in `AGENTS.md` is still forbidden (**Samānattatā**).
- **Not a place for one-off scripts.** Those belong with the agent that uses them.
- **Not skills with extra steps.** If an agent calls it during its own operation, it is a skill (see [`SKILLS.en.md`](SKILLS.en.md)).

---

## Audit Findings Schema

Every `audit` plugin's `invoke` returns a list of **findings**. The schema below is normative — runnable plugins and stubs alike MUST emit findings conforming to this shape, and the `bwoc audit run --json` envelope from `BWOC-12` is built directly over it. The framework validates closed enums at every `invoke` boundary; an unknown value is a plugin bug that fails the audit run, not a finding the operator must triage.

### Fields

| Field | Type | Required | Semantics |
|---|---|---|---|
| `criterion_id` | string, kebab-case | yes | Stable identifier for the criterion being checked. **Plugin-scoped** — unique within one plugin, not globally. MUST match an entry in the plugin's declared criteria list. **Stable across releases** — renaming a `criterion_id` is a breaking change to the plugin's contract (see [Stability](#stability)). |
| `severity` | closed enum: `info` \| `low` \| `medium` \| `high` \| `critical` | yes | Intrinsic severity of the criterion, declared once in the plugin's criteria list — **not** decided per-run. A `critical` finding with `status = "pass"` is normal and means "we checked the most important thing and it's fine." Severity describes the criterion's importance, not the outcome. |
| `status` | closed enum: `pass` \| `fail` \| `not_applicable` \| `not_implemented` | yes | Outcome of this check on this workspace. `not_applicable` is for criteria that don't apply to this workspace's profile (e.g. a multi-tenant clause on a solo workspace). `not_implemented` is the stub-plugin status — used by `audit-iso-9001`, `audit-iso-20000-1`, and `audit-iso-27001` until runtime lands in `BWOC-EPIC-3`. Free-text status values are a plugin bug. |
| `evidence` | structured: `{ kind, value, ...kind-specific fields }` where `kind ∈ { "file", "content", "command", "attestation", "sample", "none" }` and `value` is a string. Some kinds carry additional required sub-fields (see [Evidence kinds](#evidence-kinds)). Two optional fields apply across any kind: `as_of` (ISO 8601 date when evidence was current) and `valid_through` (ISO 8601 date when evidence expires, operator-declared). | yes | Where the plugin looked. `kind` is always required; `value` is required unless `kind = "none"`. Evidence MUST be reproducible — an operator running the same check by hand finds the same artifact. This is the **Musāvāda** guard: no claim without a referent. The dispatcher stamps `as_of` / `valid_through` if present; it does not enforce expiry semantics — downstream tooling decides. See [Evidence kinds](#evidence-kinds). |
| `remedy` | string, plain prose | conditional | Actionable next step. **Required** when `status` is `fail`, `not_applicable`, or `not_implemented` ("why this status, and what to do"). **Omitted** when `status = "pass"`. The framework rejects findings that supply `remedy` with `pass`, and findings that omit it with any other status. |

### Evidence kinds

| `evidence.kind` | `evidence.value` semantics | Required sub-fields | Use when |
|---|---|---|---|
| `file` | Path relative to the workspace root (e.g. `docs/en/PROJECT-PLAN.en.md`). The file exists at that path. | — | The criterion is "this artifact exists." |
| `content` | Path with a locator (e.g. `Cargo.toml#workspace.package.license`, `docs/en/SRS.en.md:§3.2`). The plugin found the expected content at the locator. | — | The criterion is "this artifact contains/declares X." |
| `command` | Shell-safe command the operator can rerun (e.g. `bwoc check --all`). The plugin ran the command and observed its exit. | — | The criterion is "this command succeeds on this workspace." |
| `attestation` | Free-text statement, verbatim — multi-line allowed. The artefact is an operator-signed assertion, not a workspace file. | `signer` (string — free-text identity, e.g. `"CISO: Suchada N."`), `signed_at` (ISO 8601 date or datetime) | The criterion reduces to "X happened, here's who signed off when." Used by ISO 9001 (most clauses), ISO/IEC 27001 (5.2 / 6.1.2 / 6.1.3), ISO/IEC 20000-1 (5.2 service policy). |
| `sample` | Short human summary (e.g. `"49 of 50 incidents resolved within SLA"`). | `sampled_count` (integer N), `sampled_of` (integer M), optional `window` (free-text time period, e.g. `"2026-Q1"`, `"last 90 days"`) | The criterion is statistical — "N of M items meet the bar over a window." Used by ISO/IEC 20000-1 (incident/change rates, SLA performance), ISO/IEC 27001 (Annex A sampling, SoA-driven scope). |
| `none` | Empty string. | — | `status = "not_applicable"` (no check needed) or `status = "not_implemented"` (runtime deferred). MUST NOT appear with `status = "pass"` or `"fail"` — those statuses always have a referent. |

**`attestation` and `sample` are additive in this revision** — `file`, `content`, `command`, and `none` are unchanged. v1 producers and consumers continue to validate. See [design note 2026-05-27_iso-runtime-evidence-model](../../notes/2026-05-27_iso-runtime-evidence-model.md) for the per-standard mapping rationale.

### Schema rules

- **Closed enums, not free strings.** `severity`, `status`, and `evidence.kind` are validated at plugin load and at every `invoke` boundary. An unknown value is a plugin bug that fails the audit run.
- **No nested findings.** A criterion passes or fails as a unit. Sub-checks are separate criterion entries with their own `criterion_id`. The report stays flat and machine-parseable.
- **Stable serialization order.** Findings serialize in **criterion-declaration order** — the order in which they appear in the plugin's criteria list — not check-execution order. Diffs across runs are meaningful only with this guarantee.
- **JSON is the canonical wire format.** `bwoc audit run --json` (per `BWOC-12`) emits one envelope per plugin: `{ plugin, version, started_at, finished_at, findings: [...] }`. Human-readable output is a renderer over this shape; the JSON is normative.

### Stability

`criterion_id` values are part of the plugin's public surface. Adding criteria is a minor-version bump under the plugin's own semver. **Renaming or removing** a `criterion_id` is a major-version bump (independent of the framework version in `[plugin].compat`) — downstream consumers (diff tooling, report archives, dashboards) key on these identifiers.

### Examples

A passing finding omits `remedy`:

```json
{
  "criterion_id": "29110-bp-project-plan-exists",
  "severity":     "high",
  "status":       "pass",
  "evidence":     { "kind": "file", "value": "docs/en/PROJECT-PLAN.en.md" }
}
```

A failing finding carries `remedy`:

```json
{
  "criterion_id": "29110-bp-traceability-matrix",
  "severity":     "medium",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": "docs/en/TRACEABILITY.en.md" },
  "remedy":       "Create docs/en/TRACEABILITY.en.md linking each SRS requirement to its design element and test case."
}
```

Stub plugins (`audit-iso-9001`, `audit-iso-20000-1`, `audit-iso-27001` per `BWOC-EPIC-2`) emit `status = "not_implemented"` with a uniform remedy:

```json
{
  "criterion_id": "iso-9001-internal-audit-program",
  "severity":     "medium",
  "status":       "not_implemented",
  "evidence":     { "kind": "none", "value": "" },
  "remedy":       "Runtime deferred to BWOC-EPIC-3."
}
```

An `attestation` finding (target shape for the EPIC-3 ISO 9001 runtime per BWOC-28):

```json
{
  "criterion_id": "9001-management-review",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":       "attestation",
    "value":      "Management review held 2026-04-15 covering Q1 QMS performance, customer feedback, internal audit results, improvement opportunities. Minutes archived.",
    "signer":     "Quality Manager: Tonkla K.",
    "signed_at":  "2026-04-15",
    "valid_through": "2027-04-15"
  }
}
```

A `sample` finding (target shape for the EPIC-3 ISO/IEC 20000-1 runtime):

```json
{
  "criterion_id": "20000-1-incident-management",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "sample",
    "value":         "49 of 50 incidents resolved within SLA",
    "sampled_count": 49,
    "sampled_of":    50,
    "window":        "2026-Q1"
  }
}
```

### Exit codes — `bwoc audit run`

The process exit code is normative and stable across releases. Operators and CI consumers can branch on `$?` without parsing stdout; the `--json` envelope's `summary.fail_count` and `summary.framework_error` fields carry the same signal in structured form.

| Code | Meaning |
|---|---|
| `0` | No `fail` findings across the selected plugins. Also returned when no audit plugins are enabled (or `--plugin <name>` matched a plugin that emitted only `pass` / `not_applicable` / `not_implemented` findings). |
| `1..=254` | Count of `fail` findings across all selected plugins, clamped at `254`. A run that produces ≥ 255 fails still reports the exact count under `summary.fail_count` in `--json`. |
| `255` | Framework or plugin runtime error — discovery failed, manifest parsed badly, a plugin failed to spawn or returned non-JSON, or a finding violated the schema above. The `--json` envelope's `summary.framework_error` is `true` in this case. |
| `2` | Operator/usage error — no workspace found (no `--workspace`, no `BWOC_WORKSPACE`, no ancestor `.bwoc/workspace.toml`), or `--plugin <name>` did not resolve to an installed audit-kind plugin. |

`0` and `1..=254` mean the framework completed the run cleanly and is reporting on plugin output. `255` means the framework itself could not produce a trustworthy report. `2` means the operator's invocation was wrong before any plugin ran.

---

## Jira Issue Mapping Schema

A `jira` plugin maps a scrum story to a Jira issue through a **mapping entry**. The schema below is normative — the reference plugin and the sync ledger (`.scrum/jira-sync.json`) alike MUST persist mapping entries conforming to this shape, and the `bwoc jira sync` resolution plan (per `BWOC-42`) is computed directly over it. The framework validates the required fields at every `invoke` boundary that reads or writes a mapping; a missing required field is a plugin bug that fails the sync run, not a state the operator must reconcile by hand.

This is the `jira` kind's contract, the write-side analogue of the [Audit Findings Schema](#audit-findings-schema) the `audit` kind carries. The auth model, JQL/rate-limit bounds, and the bidirectional conflict policy that consume these fields live in the [BWOC-40 design note](../../notes/2026-05-27_jira-plugin-architecture.md) and are not duplicated here.

### Fields

| Field | Type | Required | Semantics |
|---|---|---|---|
| `issue_key` | string | yes | The Jira issue key (e.g. `BWOC-123`). **The stable external key** — the field the mapping is keyed on, paired with the scrum story id. A change here is mapping drift (a Jira project move re-keys the issue), not a field update (see [Field stability](#field-stability)). |
| `project` | string | yes | The Jira project key the issue lives under (e.g. `BWOC`). Every read is project-scoped; a mapping whose `project` escapes the configured project(s) is rejected. |
| `summary` | string | yes | The issue title. A mutable projection of Jira state, refreshed each sync. |
| `status` | string | yes | The issue's workflow status (e.g. `In Progress`), mapped to the scrum status. Mutable; compared field-by-field against the `last_synced` watermark for conflict detection. |
| `assignee` | string | no | Account identifier of the assignee (Atlassian `accountId` or email). Omitted when the issue is unassigned. |
| `story_points` | number | no | Estimation points. Omitted when the issue is unestimated. |
| `parent_epic` | string | no | The `issue_key` of the parent epic. Omitted for issues that belong to no epic. |
| `sprint` | string | no | The sprint name or identifier the issue is assigned to. Omitted when the issue sits in the backlog (no sprint). |
| `last_synced` | string (ISO 8601 datetime) | yes | Watermark of the last successful sync for this issue. Drives the per-field last-writer-wins conflict detection; independent of credentials, so rotating the API token never invalidates it. |

Optional fields are **omitted from the entry** when the issue has no value — an unassigned issue carries no `assignee` key — never serialized as `null`. This mirrors how a passing audit finding omits `remedy` rather than emitting an empty one.

### Field stability

`issue_key` is the stable external key. The mapping is keyed on `issue_key` (paired with the scrum story id); it is the **one** field a consumer — the sync ledger, diff tooling, a dashboard — may treat as a durable identifier. The other eight fields are mutable projections of Jira state, refreshed on every sync and compared field-by-field against the `last_synced` watermark; never key on `summary`, `status`, `assignee`, `story_points`, `parent_epic`, or `sprint`. A change to `issue_key` itself (a Jira project move that re-keys the issue) is **mapping drift**, not a field update — surfaced to the operator, never silently rewritten, per the `404 → mapping drift` handling in the [BWOC-40 design note](../../notes/2026-05-27_jira-plugin-architecture.md).

### Example

A mapping entry for a fully populated, in-sprint story:

```json
{
  "issue_key":    "BWOC-123",
  "project":      "BWOC",
  "summary":      "Declare jira plugin kind in PLUGINS spec",
  "status":       "In Progress",
  "assignee":     "agent-jisoo@bwoc.local",
  "story_points": 5,
  "parent_epic":  "BWOC-100",
  "sprint":       "Sprint 6",
  "last_synced":  "2026-05-27T10:00:00Z"
}
```

An entry for an unassigned backlog issue omits the optional fields it has no value for:

```json
{
  "issue_key":   "BWOC-200",
  "project":     "BWOC",
  "summary":     "Draft scrum-via-jira skill",
  "status":      "To Do",
  "last_synced": "2026-05-27T10:00:00Z"
}
```

---

## OKR Progress Schema

An `okr` plugin tracks operator-authored Objectives + Key Results and emits a **progress entry** per key result. The schema below is normative — the reference plugin's `report` verb (per `BWOC-49`) and the `bwoc okr report` output (per `BWOC-48`) alike MUST emit entries conforming to this shape, and `bwoc check` (per `BWOC-50`) validates it. This is the `okr` kind's contract, the goal-tracking analogue of the [Audit Findings Schema](#audit-findings-schema) the `audit` kind carries.

The objectives and key results themselves are operator-authored in two local TOML files (`objectives.toml`, `key_results.toml`); their authoring shape and the `track` / `check-progress` / `report` verb contracts live in the [BWOC-46 design note](../../notes/2026-05-28_okr-plugin-architecture.md) and are not duplicated here.

### Fields

| Field | Type | Required | Semantics |
|---|---|---|---|
| `objective_id` | string | yes | The parent objective. **Referential** — MUST resolve to an `objective_id` declared in `objectives.toml`; a dangling reference is a plugin bug that fails `bwoc check`, not operator state. |
| `key_result_id` | string | yes | The stable key for this key result, unique within the plugin. The one field a consumer (a dashboard, diff tooling) may treat as a durable identifier. |
| `target` | number | yes | The goal value the key result aims for. |
| `current` | number | yes | The latest tracked value. Updated by the `track` verb; never exceeds nor is clamped to `target` — over-attainment (`current > target`) is meaningful and preserved. |
| `unit` | enum | yes | One of `count` \| `percent` \| `currency` \| `ratio` \| `boolean`. How `target` / `current` are read. |
| `confidence` | enum | yes | One of `high` \| `medium` \| `low` — the operator's qualitative read of whether the trajectory holds. An enum, not a numeric score, by deliberate choice (BWOC-46 §5): attainment carries the quantitative signal, `confidence` the qualitative one. |
| `evidence` | object | yes | **Reuses the audit [Evidence kinds](#evidence-kinds)** — `{ kind, value, ...kind-specific fields }` where `kind ∈ { "file", "content", "command", "attestation", "sample", "none" }`. The Musāvāda guard applies: a tracked `current` value should carry a reproducible referent (or `kind = "none"` when none exists). No OKR-specific evidence kinds are introduced. |
| `as_of` | string (ISO 8601 date) | no | When `current` was last tracked. Omitted when never tracked. |

Optional fields are **omitted from the entry** when they have no value — a never-tracked key result carries no `as_of` key — never serialized as `null`, mirroring the Audit Findings and Jira Issue Mapping conventions.

### Field stability

`key_result_id` is the stable key — the mapping a consumer may key on durably. `objective_id` is a stable reference (it points at an objective by its declared id). The remaining fields (`target`, `current`, `unit`, `confidence`, `evidence`, `as_of`) are mutable projections of tracking state, refreshed as the operator authors targets and the `track` verb records progress; never key on them. `current` and `confidence` in particular change every check-in.

### Example

```json
{
  "objective_id":  "O1",
  "key_result_id": "O1-KR1",
  "target":        1,
  "current":       1,
  "unit":          "count",
  "confidence":    "high",
  "evidence":      { "kind": "file", "value": "docs/en/PLUGINS.en.md" },
  "as_of":         "2026-05-28"
}
```

---

## Council Decision Schema

A `council` plugin records a fleet decision as a **decision entry**. The schema below is normative — the reference plugin's verbs (per `BWOC-59`) and the `bwoc council` output (per `BWOC-58`) alike MUST emit entries conforming to this shape, and `bwoc check` (per `BWOC-60`) validates it. This is the `council` kind's contract, the coordination analogue of the [Audit Findings Schema](#audit-findings-schema).

The decision moves through the protocol `proposed → discussing → voting → resolved` (or `abandoned` if quorum fails); the protocol detail, voting models, and quorum/tie-break rules live in the [BWOC-56 design note](../../notes/2026-05-28_council-plugin-architecture.md) and are not duplicated here.

### Fields

| Field | Type | Required | Semantics |
|---|---|---|---|
| `decision_id` | string | yes | The stable key for the decision — the one field a consumer may treat as a durable identifier. |
| `status` | enum | yes | `proposed` \| `discussing` \| `voting` \| `resolved` \| `abandoned`. The protocol state. |
| `participants` | array of string | yes | Agent ids drawn from the referenced `bwoc team`. A participant outside the team is rejected. |
| `options` | array of string | yes | The choices being decided among (≥2). |
| `rounds` | array | yes | Ordered discussion rounds. Each round carries turns `{ participant, message_ref }`, where `message_ref` points at the `bwoc send` envelope that holds the turn — the inbox is the transport, the record references it, never copies it. Append-only. |
| `votes` | array | yes | One `{ participant, option, abstain }` per voter. Append-only; a re-cast appends, never overwrites (the trail is the point). |
| `outcome` | string | no | The resolved option. Omitted until `status = resolved`. |
| `dissent` | array | no | Recorded minority positions `{ participant, option, rationale }`. Preserved, never discarded — recording dissent is a purpose of the council. |
| `evidence_links` | array | no | **Reuses the audit [Evidence kinds](#evidence-kinds)** — `{ kind, value, ... }` referents backing the decision. No council-specific evidence kinds. |
| `opened_at` | string (ISO 8601 datetime) | yes | When the decision was proposed. |
| `closed_at` | string (ISO 8601 datetime) | no | When it resolved or was abandoned. Omitted while open. |

Optional fields are **omitted from the entry** when absent — an unresolved decision carries no `outcome` / `closed_at` key — never serialized as `null`, per the Audit Findings / Jira / OKR conventions.

### Field stability

`decision_id` is the stable key. `status`, `rounds`, `votes`, `outcome`, `dissent`, and `closed_at` are mutable as the protocol advances (rounds + votes accumulate, status transitions, outcome/closed_at fill on resolve); never key on them. `participants` and `options` are fixed at propose time — a change to either is a new decision, not an edit.

### Example

```json
{
  "decision_id":  "D1",
  "status":       "resolved",
  "participants": ["agent-jisoo", "agent-jennie", "agent-lisa", "agent-rose"],
  "options":      ["adopt", "defer"],
  "rounds":       [{ "round": 1, "turns": [{ "participant": "agent-jisoo", "message_ref": "msg-20260528T120000Z-a1b2c" }] }],
  "votes":        [{ "participant": "agent-jisoo", "option": "adopt", "abstain": false }],
  "outcome":      "adopt",
  "dissent":      [],
  "evidence_links": [{ "kind": "file", "value": "notes/2026-05-28_council-plugin-architecture.md" }],
  "opened_at":    "2026-05-28T12:00:00Z",
  "closed_at":    "2026-05-28T12:30:00Z"
}
```

---

## Figma Asset Mapping Schema

A `figma` plugin maps a Figma node to an exported artifact + design tokens through an **asset entry**. The schema below is normative — the reference plugin's verbs (per `BWOC-64`) and the `bwoc figma` output (per `BWOC-63`) alike MUST emit entries conforming to this shape, and `bwoc check` (per `BWOC-65`) validates it. This is the `figma` kind's contract, a read-mostly design→dev analogue of the [Jira Issue Mapping Schema](#jira-issue-mapping-schema) — it carries jira's schema discipline but writes nothing back to the external system.

The auth model, file-vs-team-library scope, REST rate-limit handling, and the content-addressable export-caching strategy live in the [BWOC-61 design note](../../notes/2026-05-28_figma-plugin-architecture.md) and are not duplicated here.

### Fields

| Field | Type | Required | Semantics |
|---|---|---|---|
| `file_key` | string | yes | The Figma file key (from the file URL). Paired with `node_id`, **the stable external key** the mapping is keyed on. |
| `node_id` | string | yes | The node within the file (frame / component / instance / …). The second half of the stable key. |
| `name` | string | yes | The node's name. A mutable projection of Figma state, refreshed each `fetch`. |
| `type` | string | yes | Node type (`FRAME`, `COMPONENT`, `INSTANCE`, …). |
| `last_modified` | string (ISO 8601 datetime) | yes | Figma's last-modified timestamp for the file — the cache-invalidation signal for the content-addressable export. |
| `exported_path` | string | no | Workspace-relative path of the exported image under `figma/exports/`. Omitted until the node is exported. |
| `image_url` | string | no | The Figma-hosted render URL from an export call. **Non-durable** — Figma's render URLs expire; the durable artifact is `exported_path`, not this. Omitted when not requested. |
| `design_tokens` | object | no | Extracted design tokens `{ name: value }` (colors, spacing, type) tied to this node — the design→spec bridge. Omitted when none extracted. |

Optional fields are **omitted from the entry** when absent — a never-exported node carries no `exported_path` key — never serialized as `null`, per the Audit Findings / Jira / OKR / Council conventions.

### Field stability

`file_key` + `node_id` is the stable key — the pair a consumer (a dashboard, a spec-doc token reference) may treat as a durable identifier. The other fields are mutable projections of Figma state (`name`, `type`, `last_modified`, `design_tokens`) or local export results (`exported_path`, `image_url`), refreshed on each `fetch`/`export`; never key on them. `image_url` in particular is non-durable (it expires) — persist `exported_path` instead.

### Example

```json
{
  "file_key":      "AbC123dEf456",
  "node_id":       "12:345",
  "name":          "Primary Button",
  "type":          "COMPONENT",
  "last_modified": "2026-05-27T09:00:00Z",
  "exported_path": "figma/exports/9f86d081884c7d65.png",
  "design_tokens": { "color/primary": "#2D7FF9", "radius/sm": "4px" }
}
```

---

## Workspace Resource Schema

A `gws` plugin surfaces Google Workspace resources read-mostly. Each service emits a **resource entry** in its own normative shape — the `gws` kind's contract, validated by `bwoc check` (per `BWOC-77`) and emitted by the `bwoc gws` verbs (per `BWOC-74`). The OAuth model, per-service scopes, pagination, and rate-limit handling live in the [BWOC-72 design note](../../notes/2026-05-28_google-workspace-plugin-architecture.md) and are not duplicated here. All three shapes follow the framework conventions: a stable id key, mutable projections refreshed each read, optional fields omitted (not `null`).

### Drive file

| Field | Type | Required | Semantics |
|---|---|---|---|
| `file_id` | string | yes | **Stable key** — the Drive file id. |
| `name` | string | yes | File name (mutable projection). |
| `mime_type` | string | yes | e.g. `application/vnd.google-apps.document`. |
| `modified_time` | string (ISO 8601) | yes | Last-modified; cache-invalidation signal. |
| `owners` | array of string | no | Owner email(s). Omitted when not returned. |
| `web_view_link` | string | no | Browser URL for the file. Omitted when absent. |

### Gmail thread

| Field | Type | Required | Semantics |
|---|---|---|---|
| `thread_id` | string | yes | **Stable key** — the Gmail thread id. |
| `subject` | string | yes | Thread subject. |
| `from` | string | yes | Sender of the latest message. |
| `snippet` | string | no | Short preview. Omitted when empty. |
| `labels` | array of string | no | Label ids/names. Omitted when none. |
| `last_message_time` | string (ISO 8601) | yes | Timestamp of the latest message. |

### Calendar event

| Field | Type | Required | Semantics |
|---|---|---|---|
| `event_id` | string | yes | **Stable key** — the Calendar event id. |
| `calendar_id` | string | yes | The calendar the event belongs to. |
| `summary` | string | yes | Event title. |
| `start` | string (ISO 8601) | yes | Start time (or date for all-day). |
| `end` | string (ISO 8601) | yes | End time. |
| `attendees_count` | number | no | Number of attendees. Omitted when none. |

### Example (Drive file)

```json
{
  "file_id":       "1AbC_dEfGhIjKlMnOpQrStUvWxYz",
  "name":          "BWOC Architecture.gdoc",
  "mime_type":     "application/vnd.google-apps.document",
  "modified_time": "2026-05-27T09:00:00Z",
  "web_view_link": "https://docs.google.com/document/d/1AbC_dEfGhIjKlMnOpQrStUvWxYz/edit"
}
```

---

## Directory Layout

```
modules/plugins/
└── <name>/
    ├── manifest.toml       # required — the contract
    ├── SPEC.md             # required — Obsidian-formatted plugin description
    └── ...                 # implementation (binary, Rust crate, script)
```

`<name>` is `kebab-case`. One plugin per directory. The plugin's `kind` is declared in `manifest.toml` (see [Manifest](#manifest--manifesttoml)) and is not encoded in the directory path — symmetric with [`SKILLS.en.md`](SKILLS.en.md#directory-layout).

---

## Manifest — `manifest.toml`

```toml
[plugin]
name        = "memory-tier2-noop"               # required — must match the directory name
kind        = "memory-backend"                  # required — one of: memory-backend | llm-backend | workflow | audit | jira
version     = "0.1.0"                           # required — semver
description = "No-op Tier 2 memory backend that forwards to Tier 1."   # required — one-sentence summary
compat      = ">=2.5.0"                         # required — semver range; framework versions this plugin works with
entry       = "bwoc-plugin-memory-tier2-noop"   # required — binary on PATH (preferred) or sibling Rust crate name

[config.schema]                                 # optional — omit the table entirely if the plugin takes no config
# Plugin-defined; JSON-schema-lite. The workspace's [plugins.<name>] table is validated against this.
# Each key maps to an inline table with: type, required (bool), and (when required = false) default.
# Example:
# storage_path = { type = "string", required = false, default = "memories/tier2" }
# max_results  = { type = "integer", required = true }
```

### Field reference

| Section | Field | Required | Type | Meaning |
|---|---|---|---|---|
| `[plugin]` | `name` | yes | string (kebab-case) | Plugin identifier; must equal the directory name under `modules/plugins/` |
| `[plugin]` | `kind` | yes | enum | One of `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`; immutable after `init` |
| `[plugin]` | `version` | yes | string (semver) | Semver of the plugin itself, separate from the framework version |
| `[plugin]` | `description` | yes | string | One-sentence summary; the **only** manifest value where a vendor name is tolerated |
| `[plugin]` | `compat` | yes | string (semver range) | Framework versions this plugin is compatible with; framework refuses to load on mismatch |
| `[plugin]` | `entry` | yes | string | Binary on `PATH` (preferred) or sibling Rust crate name the framework dispatches to |
| `[config.schema]` | (free keys) | no | inline-table per key | Schema the operator's `workspace.toml [plugins.<name>]` block is validated against; each key declares `type`, `required`, optional `default` |

### Neutrality constraint (HARD)

A `memory-backend` plugin must work for any agent regardless of backend. An `llm-backend` plugin must not pretend it is one of the six declared backends. Vendor names in plugin **manifest values** are tolerated only inside `description` (where they describe the integration target); they remain forbidden anywhere else. This is the same **Samānattatā** rule that `bwoc check` already enforces on `AGENTS.md`.

---

## Lifecycle

```
init  → configure → invoke (many) → teardown
```

- **`init`** — called once when the framework first sees the plugin in `workspace.toml`. **Idempotent.** No side-effects on external systems beyond what is necessary to confirm the plugin can run.
- **`configure`** — called with the resolved `[plugins.<name>]` config block. **Idempotent**: re-running with the same block is a no-op; re-running with a changed block reconciles to the new state. Validates the config against `[config.schema]`; refuses to proceed on schema violation.
- **`invoke`** — called once per logical operation (write a memory, dispatch a model call, post to an issue tracker). **Idempotent at the operation level.**
- **`teardown`** — called once when the framework shuts down or the plugin is disabled. **Idempotent.** Cleanup-only.

Idempotency is a **hard requirement at every phase**. The framework may retry an init or configure call after a crash; an `invoke` may run twice if the framework's caller retried; teardown may be replayed across shutdowns. A plugin that mutates external state non-idempotently is broken by design.

### Lifecycle owner per kind

| Kind | Owner | When init fires | When invoke fires |
|---|---|---|---|
| `memory-backend` | Agent's memory subsystem | First memory read/write that escalates to Tier 2 | Per Tier 2 read/write |
| `llm-backend` | `bwoc spawn` | Agent spawn whose registry entry names this plugin | Per model call from the agent's harness |
| `workflow` | Agent code that imports the integration | First call from the agent | Per agent-initiated operation |
| `audit` | `bwoc audit` CLI | First `bwoc audit run` that selects this plugin in the current invocation | Per `bwoc audit run [--plugin <name>]` operator invocation; never implicit |

### Hook contract — success, failure, partial state

Plugins integrate via the `entry` field — either a binary on `PATH` or a sibling Rust crate. The contract is therefore expressed in both exit-code (binary) and return-value (crate) forms; the framework treats them as equivalent. For each hook, "success" and "failure" are the dispatch result the framework observes; "partial state" is the plugin author's responsibility to bound.

| Hook | Success means | Failure means | Partial state |
|---|---|---|---|
| `init` | Exit `0` (binary) / returns `Ok` (crate). | Non-zero exit / `Err`. Framework refuses to load the plugin and surfaces the diagnostic on stderr. | Init must fully complete or roll back before failing. The framework treats a failed init as if it never ran. |
| `configure` | Exit `0` / `Ok`. The plugin is ready for `invoke`. | Non-zero exit / `Err` citing the offending key (e.g. `max_results: required, missing`). Framework refuses to start the workspace. | Validate-first, apply-second — never half-apply config. A partial apply is a plugin bug. |
| `invoke` | Exit `0` / `Ok` with a typed result. Stdout is the payload, stderr is diagnostics (binary form). | Non-zero exit / `Err`. Framework surfaces the error to the caller (agent or operator); caller decides whether to retry. | Operations are durable-or-discarded — never half-applied. Retries land on the idempotent path. |
| `teardown` | Exit `0` / `Ok`. Framework releases the plugin slot. | Non-zero exit / `Err`. Logged, not fatal — framework shutdown must not block. | Idempotent on replay. The framework may call teardown again on a subsequent shutdown if the first never completed. |

### Per-phase examples

```text
# init — confirm the plugin can run; no business side-effects yet
init():
  if not writable(cfg.storage_path):
    exit 1, "storage_path not writable: <path>"
  open_lazy_handle(cfg.storage_path)
  exit 0

# configure — validate against [config.schema], then apply atomically
configure({ storage_path: "memories/tier2", max_results: 8 }):
  errors = validate_against_schema(input)
  if errors:
    exit 2, "configure: " + errors.join(", ")
  apply_atomic(input)               # all-or-nothing
  exit 0

# invoke — operation-level idempotency
invoke("write_memory", { id: "m-1", body: "..." }):
  existing = lookup("m-1")
  if existing and body_hash(existing) == body_hash(input):
    exit 0, { status: "noop" }      # replay-safe
  store("m-1", input)
  exit 0, { status: "written" }

# teardown — cleanup-only, idempotent
teardown():
  flush_pending(timeout = 5s)       # best-effort
  close_handles()
  exit 0                            # safe to call twice
```

---

## Loading — `workspace.toml`

The operator declares which plugins this workspace uses by adding entries to `workspace.toml`:

```toml
[plugins]

[plugins.memory-tier2-noop]
enabled      = true
storage_path = "memories/tier2"

[plugins.workflow-github]
enabled = false      # registered but off — kept here to document intent
```

Schema for each `[plugins.<name>]` table:

- `<name>` (table key, string, required) — the installed plugin's directory name under `modules/plugins/`. The key is the plugin name; `kind` is **not** declared in `workspace.toml` — it is owned by the plugin's own `manifest.toml` `[plugin].kind` field and read from there at load time.
- `enabled` (bool, required) — gates whether the plugin is loaded at framework startup. Set `false` to keep the entry as documented intent without loading. Mirrors the `config.manifest.json skills.framework[] enabled` pattern in [`SKILLS.en.md`](SKILLS.en.md#discovery); flip with `bwoc plugin disable <name>` to preserve the entry.
- All other keys (plugin-defined) — validated against the plugin's `[config.schema]` at framework startup. Refused on schema violation; never half-applied (see [Lifecycle](#lifecycle)).

A missing `enabled` field is a manifest error — `bwoc check` rejects entries that omit it. There is no implicit default; explicit intent is the contract.

At framework startup the runtime:

1. Reads the `[plugins]` table from `workspace.toml`.
2. Filters to entries where `enabled` is `true`. Entries with `enabled = false` are kept in `workspace.toml` (as documented intent) but skipped at load.
3. Resolves each entry against the workspace's `modules/plugins/<name>/` directory. `<kind>` is read from the installed plugin's manifest, not encoded in the path.
4. Validates the entry's config block against the plugin's `[config.schema]`, then dispatches `init` followed by `configure`.
5. Refuses to start the workspace if any enabled plugin is missing under `modules/plugins/`, has a `[plugin] compat` mismatch with the running framework version, fails `[config.schema]` validation, or returns a non-zero `init` / `configure` result.

No central index. Plugins exist for a workspace only because they are installed under `modules/plugins/` and named in `workspace.toml`. The resolution lookup is always local to the workspace — no runtime network calls during framework startup. **Anattā** preserved.

---

## CLI Surface

Read-only surfaces (no side-effects on the workspace):

```
bwoc plugin list                    # list installed plugins (enabled + disabled)
bwoc plugin list --enabled          # filter to enabled only
bwoc plugin list --kind memory-backend
bwoc plugin list --json

bwoc plugin show <name>             # full manifest + spec + current config
bwoc plugin show <name> --json
```

Lifecycle surfaces (write — see referenced sections for details):

```
bwoc plugin init <name> --kind <k>  # scaffold a new plugin from modules/plugin-template/
                                    #   (see "Scaffolding from template")

bwoc plugin install <source>        # install from local path / git URL / tarball URL
                                    #   (see "Sources & Installation")

bwoc plugin enable <name>           # set enabled=true in workspace.toml [plugins.<name>]
bwoc plugin disable <name>          # set enabled=false (keeps the entry)

bwoc plugin remove <name>           # delete modules/plugins/<name>/ and clean workspace.toml
                                    #   (see "Removal")
```

No `bwoc plugin verify` in v1 — plugins do not declare a uniform verify gate (the kinds differ too much). Verification is the plugin's own concern, surfaced through its `invoke` exit semantics. A future v2 may add per-kind verify if patterns emerge.

All read-only commands have `--json` twins. Lifecycle commands emit structured JSON when `--json` is passed; `install` exits non-zero on trust-gate failure; `remove` exits non-zero on missing target unless `--yes` was passed.

### "Current workspace" resolution

Plugins are workspace-scoped (unlike skills which are agent-scoped). `enable`, `disable`, `remove` resolve the target workspace in this order:

1. **`--workspace <path>` flag** — explicit override.
2. **`BWOC_WORKSPACE` environment variable**.
3. **Working directory** — walks up from cwd to find the nearest `.bwoc/workspace.toml`.
4. **Otherwise** — error: `no workspace context; pass --workspace <path> or run from inside a workspace`.

The resolution is identical to how `bwoc list` and `bwoc workspace info` already locate the workspace today.

---

## Sources & Installation

A framework plugin enters a workspace either by being authored in place under `modules/plugins/<name>/` or by being installed from one of three source kinds:

| Source kind | Example | Detection |
|---|---|---|
| **Local path** | `bwoc plugin install ./vendor/my-plugin/` | Argument starts with `./`, `../`, or `/` and resolves to a directory |
| **Git URL** | `bwoc plugin install https://github.com/org/plugin.git#v0.1.0` | Argument scheme is `http(s)://` or `git://` AND ends with `.git` (optional `#<ref>`) |
| **Tarball URL** | `bwoc plugin install https://example.com/plugin-0.1.0.tar.gz` | Argument scheme is `http(s)://` AND ends with `.tar.gz` or `.tgz` |

The install mechanism:

1. Resolves the source kind from the argument.
2. **Pre-flight** — if source has no `manifest.toml` at its root, refuse with `source missing manifest.toml; cannot resolve name or kind`. Nothing is fetched / extracted / written.
3. **Trust gate** (see below) — fetches and verifies a SHA-256 checksum.
4. Reads the plugin's manifest from the source to learn its `name` and `kind`. The kind is **always derived from the source manifest** — it cannot be overridden by a flag.
5. Materializes the source into `modules/plugins/<name>/` (copy for local; clone-then-discard-`.git` for git; extract for tarball).
6. Validates the installed manifest with `bwoc check`.
7. Records the install in `.bwoc/installed-sources.toml` (schema below). Only writes the registry record on successful completion.
8. **Does not** auto-enable. The installed plugin is dormant until `bwoc plugin enable <name>` adds an entry to `workspace.toml [plugins.<name>]` with `enabled = true`.

### Re-install and failure handling

- **Target already exists** — if `modules/plugins/<name>/` already exists, the default behavior is to refuse with `<name> already installed at version X; pass --upgrade to replace`.
  - `--upgrade` — replaces in place, retains the `installed-sources.toml` record (updates `last_hash` and `installed_at`).
  - `--force` — replaces unconditionally, even if the current install has uncommitted local edits (a stderr warning lists what was overwritten).
- **Network failure during install** — install is non-atomic by design; on transient failure (download interrupted, extract error), the partial directory is removed before exit and `installed-sources.toml` is **not** updated. Safe to retry.

### Trust gate (v1)

Every install verifies a SHA-256 checksum **before** materializing:

- **Tarball URL** — the CLI fetches `<source>.sha256` (same URL with the `.sha256` suffix), reads the expected digest, and compares against the computed digest of the downloaded archive.
- **Git URL** — the CLI fetches the checksum at the URL with `.git` replaced by `.sha256`. Example:
  - Source: `https://github.com/org/plugin.git#v0.1.0`
  - Checksum: `https://github.com/org/plugin.sha256` (operator publishes a manifest of expected tree-shas keyed by ref)
  - After clone, the framework runs `git rev-parse <ref>^{tree}` and compares against the entry for `<ref>` in the fetched manifest.
  - Operators typically publish this manifest via a GitHub release asset or a separate static-hosted file.
- **Local path** — checksum is optional; if a sibling `<dir>.sha256` exists, it is verified; otherwise the install proceeds (local paths are operator-trusted by convention).

Two flags relax the gate:

- `--no-verify` — skips checksum verification. Emits a stderr warning. Intended for in-development sources served locally over HTTP.
- `--allow-new-source` — required the **first time** a given source URL is installed in this workspace. Establishes "I have inspected this source." Subsequent installs from the same registered source (recorded in `.bwoc/installed-sources.toml`) skip this prompt.

The trust gate matches the SKILLS spec — same flags, same registry file, same semantics. A future Trust v2 (signed envelopes; identity proof) extends both surfaces without breaking the v1 contract.

**Anattā preserved.** There is no central registry, no name-to-URL resolution service, no auto-update mechanism. Every install names its source explicitly. The framework is not a package manager.

### `.bwoc/installed-sources.toml` schema

Shared with SKILLS — a single workspace registry covers both kinds of installs. See [`SKILLS.en.md` — installed-sources schema](SKILLS.en.md#bwocinstalled-sourcestoml-schema) for the full table; plugin entries use `kind = "plugin"` and `target = "modules/plugins/<name>"`.

---

## Scaffolding from template

`bwoc plugin init <name> --kind <kind>` creates a new plugin in `modules/plugins/<name>/` by copying the template at `modules/plugin-template/` and substituting placeholders (including `kind`):

```
modules/plugin-template/
├── manifest.toml          # contains {{pluginName}}, {{pluginVersion}}, {{pluginKind}} placeholders
└── SPEC.md                # Obsidian-formatted; placeholders for plugin name + description
```

Placeholders use the same `{{camelCase}}` convention as `modules/agent-template/` and `modules/skill-template/`. Required substitutions are listed in the template's own [`SPEC.md`](../../modules/plugin-template/SPEC.md).

The `--kind` flag is required — there is no default. Valid values: `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`. Future kinds extend this enum without changing the template layout. The flag forces the operator to declare intent up front and avoids producing a manifest with a missing or wrong `kind` field.

`bwoc plugin init` is the recommended way to start a new plugin — manual creation is supported but bypasses placeholder consistency.

### `init` vs `install` — why `--kind` works differently

`init` and `install` treat `kind` asymmetrically by design:

- **`init <name> --kind <kind>`** — operator declares intent; `--kind` is substituted into the new template manifest. Required because no manifest exists yet to derive kind from.
- **`install <source>`** — `kind` is read from the source's `manifest.toml`. Not overridable — a source manifest declaring `kind = "memory-backend"` is installed with that kind preserved in the manifest, regardless of any flag.

This asymmetry exists because the install flow trusts the source author's declared intent: if the source says it is a `workflow` plugin, it is one. Operators who disagree should refuse the install, not edit the manifest after the fact.

---

## Removal

`bwoc plugin remove <name>`:

1. **Confirms with the user** unless `--yes` was passed. Lists what will be deleted (`modules/plugins/<name>/`) and modified (`workspace.toml [plugins.<name>]`); reports the plugin's `kind` (read from the manifest) for context.
2. **Deletes** `modules/plugins/<name>/` recursively.
3. **Cleans** `workspace.toml` — removes the `[plugins.<name>]` table entirely (not just `enabled = false`).

Idempotent — `remove` on a non-existent target reports "not installed" and exits 0. The `--yes` flag short-circuits the confirmation prompt.

A removed source is not auto-uninstalled from `.bwoc/installed-sources.toml`. Pass `--forget-source` to also drop the source registration.

---

## Verification

`bwoc check` extends to audit `modules/plugins/<name>/` plus the installed-source registry:

| Check | Pass condition |
|---|---|
| Manifest parseable | `manifest.toml` is valid TOML and matches the schema above |
| Name matches directory | `[plugin].name == basename(directory)` |
| Kind valid | `[plugin].kind` is one of `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira` (or a future kind added to the enum) |
| Neutrality | Vendor names only inside `description`; nowhere else |
| `SPEC.md` present | A `SPEC.md` file exists alongside the manifest |
| Required fields | `name`, `kind`, `version`, `description`, `compat`, `entry` all present |
| Compat range valid | `[plugin].compat` parses as a semver range |
| Source registry parseable | `.bwoc/installed-sources.toml` is valid TOML if present |
| No orphan source records | every entry where `kind = "plugin"` in the registry has a matching `modules/plugins/<name>/` directory |
| No orphan installations | every `modules/plugins/<name>/` either has a registry entry OR contains an `.authored-in-place` marker file |
| Registry drift | `installed_hash` in registry matches the current SHA-256 of `modules/plugins/<name>/` (or `bwoc check --update-hashes` was passed to acknowledge drift) |

A failed check exits non-zero on the workspace audit — same surface, same exit semantics as the existing `bwoc check --all`.

---

## What This Spec Does NOT Cover

- **Skills** — see [`SKILLS.en.md`](SKILLS.en.md). Skills are agent-invoked; plugins are framework-loaded.
- **The six declared backends** (`claude`, `antigravity`, `codex`, `kimi`, `ollama`, `openai-compatible`) — they are first-class, not plugins. See [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md).
- **The first reference plugin itself** — see story `BWOC-7` and (once landed) `modules/plugins/memory-tier2-noop/SPEC.md`.
- **Trust v2 / signing of plugin binaries** — deferred. Plugin binaries today are trusted by virtue of being installed under `modules/plugins/`; richer trust gating lands with the broader Trust v2 work.

---

## See Also

- [`SKILLS.en.md`](SKILLS.en.md) — the sibling spec; same substrate, different invoker.
- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — how modules compose with the rest of the framework.
- [`WORKSPACE.en.md`](WORKSPACE.en.md) — `workspace.toml` schema; this spec extends it with `[plugins]`.
- [`HARNESS.en.md`](HARNESS.en.md) — the ollama harness; the pattern future `llm-backend` plugins will follow.
- [`NAMING.en.md`](NAMING.en.md) — file naming and directory conventions.
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — Pali term lookup (Anattā, Samānattatā, Mattaññutā).
