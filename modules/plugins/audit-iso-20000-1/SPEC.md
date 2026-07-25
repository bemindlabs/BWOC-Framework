---
title: ISO/IEC 20000-1 IT Service Management System Audit
aliases:
  - audit-iso-20000-1
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-20000-1
  - status/runtime
maturity: L1
---

# ISO/IEC 20000-1 IT Service Management System Audit

> [!abstract] **Attestation + sample runtime (v0.2.0).** The first runtime that mixes two evidence kinds. Each criterion declares its `expected_evidence_kind` in [[criteria]]; the runtime reads operator-provided evidence from `.bwoc/workspace.toml` and emits `evidence.kind = "attestation"` for documented-artifact clauses (scope, policy, catalogue) and `evidence.kind = "sample"` for operational-rate clauses (SLAs, changes, incidents, problems, improvement), per the [BWOC-27 schema](../../../docs/en/PLUGINS.en.md#evidence-kinds). Criteria without operator evidence emit `status = "fail"` pointing at `workspace.toml`. Replaces the v0.1.0 stub from EPIC-2.

## Status & Roadmap

| Version | Date | Change |
|---|---|---|
| v0.1.0 | 2026-05-26 | Stub. Eight criteria declared; every finding `status = "not_implemented"`. Schema conformance only — no workspace inspection. Landed in EPIC-2. |
| v0.2.0 | 2026-05-27 | **Attestation + sample runtime.** Reads `[[plugins.audit-iso-20000-1.attestations]]` and `[[plugins.audit-iso-20000-1.samples]]` from `workspace.toml`; routes each criterion by its `expected_evidence_kind` (declared in `criteria.toml`) and emits `attestation` or `sample` findings, with `status = "fail"` + a `workspace.toml`-pointing remedy for criteria lacking evidence. The eight `criterion_id`s carry over unchanged (stability contract, PLUGINS.en.md §Stability). No dispatcher change was needed — `crates/bwoc-cli/src/audit.rs` already validates both kinds (extended in BWOC-28). Landed in EPIC-3 BWOC-33. See [[../../notes/2026-05-27_20000-1-sample-source]] for the design. |

## Why a Runtime Now

[[../../notes/2026-05-26_iso-compliance-plugins|The EPIC-2 framing note]] explained why 20000-1 shipped as a stub first — its evidence is service-management practice (incidents, changes, SLAs, policy) that lives in ITSM tooling, not workspace files, and the v1 schema could not express it. EPIC-3 closed that gap:

- The [BWOC-26 design note](../../../notes/2026-05-27_iso-runtime-evidence-model.md) pinned the new evidence model (`attestation`, `sample`, time-bounded fields).
- [BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) extended the schema with `attestation` (required `signer` + `signed_at`) and `sample` (required `sampled_count` + `sampled_of`, optional `window`).
- BWOC-28 built the 9001 attestation runtime and extended the dispatcher to validate both new kinds.
- BWOC-33 (this change) builds the 20000-1 runtime — the first to mix both kinds. Inferring "this organization runs incident management within SLA" from "this repo contains `INCIDENTS.md`" would still falsify the audit (Musāvāda — [PHILOSOPHY.en.md](../../agent-template/docs/en/PHILOSOPHY.en.md) §Sila 5). Attestation evidence keeps documented-artifact clauses honest (an operator vouches, dated, with provenance); sample evidence keeps operational clauses honest (the operator records the measured rate from their ITSM tool).

## Criteria (v0.2.0)

Eight headline ITSM criteria, drawn from the main clauses of ISO/IEC 20000-1:2018[^iso-20000-1-2018]. Declaration order in [[criteria]] is the report order (PLUGINS.en.md §Audit Findings Schema). `criterion_id` values are stable across releases (PLUGINS.en.md §Stability); renames are a major version bump. The **Kind** column is each criterion's `expected_evidence_kind` — see [[../../notes/2026-05-27_20000-1-sample-source]] for the documented-artifact-vs-operational-rate split rationale.

| `criterion_id` | Clause | Title | Severity | Kind |
|---|---|---|---|---|
| `20000-1-service-management-system-scope` | 4.3 | Scope of the service management system | high | attestation |
| `20000-1-service-policy-and-objectives` | 5.2 | Service management policy and objectives | high | attestation |
| `20000-1-service-catalogue` | 8.3.1 | Service catalogue | medium | attestation |
| `20000-1-service-level-management` | 8.3.3 | Service level management | high | sample |
| `20000-1-change-management` | 8.5.1 | Change management | high | sample |
| `20000-1-incident-management` | 8.6.1 | Incident management | high | sample |
| `20000-1-problem-management` | 8.6.3 | Problem management | medium | sample |
| `20000-1-continual-improvement` | 10.2 | Continual improvement | medium | sample |

Three attestation + five sample. The split follows one rule: documented-artifact criteria (a scope, a policy, a catalogue the operator vouches for) use `attestation`; operational-rate criteria (N of M items met the bar over a window) use `sample`. Sub-clauses 8.2 (service portfolio), 8.4 (supply & demand), and 8.7 (service assurance) remain deferred — adding criteria is a minor-version bump, never a rename of an existing id.

## How It Runs

```bash
bwoc audit run --plugin audit-iso-20000-1 --json
```

The dispatcher spawns `audit.sh` (per `BWOC-12`) with the standard env contract: `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION`. The runtime:

1. Reads `criteria.toml` from `BWOC_PLUGIN_DIR` for the declared criteria (id, severity, **`expected_evidence_kind`**).
2. Reads `.bwoc/workspace.toml` from `BWOC_WORKSPACE` and builds two lookup tables keyed by `criterion_id` — one from `[[plugins.audit-iso-20000-1.attestations]]`, one from `[[plugins.audit-iso-20000-1.samples]]`.
3. For each criterion in declaration order, routes by `expected_evidence_kind`:
   - **`attestation`** — present and complete (`statement` + `signer` + `signed_at`) → `status = "pass"`, `evidence.kind = "attestation"` with optional `valid_through`. Present but incomplete, or absent → `status = "fail"`, `evidence.kind = "file"` pointing at `.bwoc/workspace.toml`.
   - **`sample`** — present and valid (`summary` + integer `sampled_count` + integer `sampled_of`, `0 ≤ count ≤ of`) → `status = "pass"`, `evidence.kind = "sample"` with optional `window`. Present but incomplete/invalid, or absent → `status = "fail"`, `evidence.kind = "file"`.
4. Exits `0` on success — non-pass findings are *findings*, not errors. A non-zero exit signals a framework-side problem (unreadable `criteria.toml`).

The runtime does **not** impose an SLA threshold on samples: a recorded sample is evidence and passes, and the rate (`"49 of 50 …"`) is surfaced for the human auditor to judge. Thresholds are organisational policy — the runtime surfaces reproducible evidence, downstream tooling decides (per BWOC-26). The dispatcher's process exit code is the count of `fail` findings (BWOC-12), so a workspace with no evidence exits `8`, one with four evidenced criteria exits `4`, and one with all eight exits `0`.

## Sample Output

A sample criterion with a recorded rate:

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

An attestation criterion:

```json
{
  "criterion_id": "20000-1-service-policy-and-objectives",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "attestation",
    "value":         "Service management policy v2.1 ratified 2026-01-15; objectives reviewed quarterly.",
    "signer":        "Service Owner: Tonkla K.",
    "signed_at":     "2026-01-15",
    "valid_through": "2027-01-15"
  }
}
```

A criterion without operator evidence (here a sample criterion):

```json
{
  "criterion_id": "20000-1-service-level-management",
  "severity":     "high",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": ".bwoc/workspace.toml" },
  "remedy":       "Provide a recorded sample in .bwoc/workspace.toml under [[plugins.audit-iso-20000-1.samples]] with criterion_id=\"20000-1-service-level-management\", summary, sampled_count, and sampled_of (integers) to satisfy this criterion."
}
```

`bwoc audit run` wraps the findings array in the canonical envelope `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`.

## Configuration

```toml
# .bwoc/workspace.toml
[plugins.audit-iso-20000-1]
enabled = true

# attestation criteria (4.3 scope, 5.2 policy, 8.3.1 catalogue)
[[plugins.audit-iso-20000-1.attestations]]
criterion_id  = "20000-1-service-policy-and-objectives"
statement     = "Service management policy v2.1 ratified 2026-01-15; objectives reviewed quarterly."
signer        = "Service Owner: Tonkla K."
signed_at     = "2026-01-15"
valid_through = "2027-01-15"   # optional

# sample criteria (8.3.3 SLA, 8.5.1 change, 8.6.1 incident, 8.6.3 problem, 10.2 improvement)
[[plugins.audit-iso-20000-1.samples]]
criterion_id  = "20000-1-incident-management"
summary       = "49 of 50 incidents resolved within SLA"
sampled_count = 49
sampled_of    = 50
window        = "2026-Q1"   # optional
```

Both blocks are **array-of-tables** under the universal `[plugins.audit-iso-20000-1]` block.

`[[…attestations]]` fields:

| Field | Required | Notes |
|---|---|---|
| `criterion_id` | yes | Must match an `attestation`-kind criterion in `criteria.toml`. |
| `statement` | yes | Verbatim attestation text → `evidence.value`. Single-line basic TOML string. |
| `signer` | yes | Free-text identity → `evidence.signer`. |
| `signed_at` | yes | ISO 8601 date (or datetime) → `evidence.signed_at`. |
| `valid_through` | optional | ISO 8601 expiry date → `evidence.valid_through`. The dispatcher stamps but does not enforce expiry (per BWOC-26). |

`[[…samples]]` fields:

| Field | Required | Notes |
|---|---|---|
| `criterion_id` | yes | Must match a `sample`-kind criterion in `criteria.toml`. |
| `summary` | yes | Short human summary → `evidence.value` (e.g. `"49 of 50 incidents resolved within SLA"`). |
| `sampled_count` | yes | Integer N actually measured → `evidence.sampled_count`. |
| `sampled_of` | yes | Integer M population → `evidence.sampled_of`. Must be `≥ sampled_count`. |
| `window` | optional | Free-text time period → `evidence.window` (e.g. `"2026-Q1"`, `"last 90 days"`). |

The first occurrence of a given `criterion_id` wins; the runtime does not flag duplicates (that is `bwoc check`'s job — BWOC-29). The operator transcribes the sample rate from their ITSM tool (incident tracker, change board, SLA dashboard) into the committed block — v0.2.0 does not query the ticket system directly. `workspace.toml` was chosen over a separate file or env path for the same smallest-blast-radius reasons as the 9001 attestation source: operators already touch this file, every change is diff-able in `git log`, and `bwoc check` already walks it. See [[../../notes/2026-05-27_20000-1-sample-source]] for the full rationale.

## Maturity

Declared **L1** — runtime operational, emits real attestation + sample findings per BWOC-27. Bumps to **L2** when:

- `bwoc check` validates the workspace `[[attestations]]` / `[[samples]]` blocks against the schema (extends BWOC-29 from `criteria.toml` to `workspace.toml`).
- Live ITSM-tool adapters (Jira Service Management, ServiceNow, CSV exports) answer sample queries directly, removing the manual transcription step.
- A `bwoc audit report --below <pct>` flag thresholds sample rates against an operator-declared target.

## Neutrality

Manifest values name no LLM backend, vendor, or model. `kind = "audit"` is the framework's own enum (`BWOC-10`). The plugin references "ISO/IEC 20000-1" exclusively in `description`, in this SPEC's prose, and in the standardized `20000-1-*` criterion-id namespace — never in `criteria.toml` keys or in finding values beyond that namespace. Satisfies the **Samānattatā** rule.

## Sources

The ITSM criteria are drawn from the published ISO/IEC 20000-1:2018 main-body clauses (4 through 10):

- ISO/IEC 20000-1:2018 — *Information technology — Service management — Part 1: Service management system requirements.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/70636.html>. [^iso-20000-1-2018]
- ISO/IEC JTC 1/SC 40 — *IT service management and IT governance.* The joint technical committee responsible for the ISO/IEC 20000 family: <https://www.iso.org/committee/5013818.html>.
- ISO/IEC 20000-10:2018 — *Concepts and vocabulary* (publicly excerpted in ISO's online browsing platform for terminology only): useful for distinguishing "service", "SMS", and "SLA" definitions.

[^iso-20000-1-2018]: ISO/IEC 20000-1:2018 follows the Annex SL high-level structure (clauses 4 Context, 5 Leadership, 6 Planning, 7 Support, 8 Operation, 9 Performance evaluation, 10 Improvement). Clause 8 (Operation of the SMS) is the substantive ITSM-practice clause and is itself broken into seven sub-clauses (8.1 operational planning, 8.2 service portfolio, 8.3 relationship & agreement, 8.4 supply & demand, 8.5 service design build & transition, 8.6 resolution & fulfilment, 8.7 service assurance). The eight criteria here cover one headline practice from each of clauses 4, 5, 8.3, 8.5, 8.6, and 10; the residual sub-clauses (8.2, 8.4, 8.7, plus several 8.3 and 8.6 children) are deferred to a future minor-version expansion.

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `audit` kind row (BWOC-10), Audit Findings Schema (BWOC-11 + BWOC-27 evidence kinds).
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — EPIC-2 framing note (why stubs).
- [[../../notes/2026-05-27_iso-runtime-evidence-model|2026-05-27_iso-runtime-evidence-model.md]] — BWOC-26 evidence-model design (attestation, sample, time-bounded fields).
- [[../../notes/2026-05-27_20000-1-sample-source|2026-05-27_20000-1-sample-source.md]] — BWOC-33 design (evidence-kind split + sample source mechanism).
- [[../audit-iso-29110/SPEC|audit-iso-29110]] — the runnable reference audit plugin from BWOC-13.
- [[../audit-iso-9001/SPEC|audit-iso-9001]] — sibling attestation runtime (BWOC-28); [[../audit-iso-27001/SPEC|audit-iso-27001]] — sibling stub (runtime scheduled next).
