---
title: ISO/IEC/IEEE 12207 Software Life Cycle Processes Audit
aliases:
  - audit-iso-iec-ieee-12207
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-ieee-12207
  - status/runtime
maturity: L1
---

# ISO/IEC/IEEE 12207 Software Life Cycle Processes Audit

> [!abstract] **Attestation runtime (v0.1.0).** Audits a workspace's **software life cycle processes** against **ISO/IEC/IEEE 12207:2017** — a standard jointly published by **ISO, IEC, and IEEE** (harmonised with ISO/IEC/IEEE 15288 for systems). Reads operator-signed attestations from `.bwoc/workspace.toml` under `[[plugins.audit-iso-iec-ieee-12207.attestations]]` and emits `evidence.kind = "attestation"` findings (`signer` + `signed_at` + optional `valid_through`) per the [BWOC-27 schema extension](../../../docs/en/PLUGINS.en.md#evidence-kinds). Criteria without an operator attestation emit `status = "fail"` pointing at the `workspace.toml` block. It reuses the shared attestation runtime of the ISO 9001 / ISO/IEC 27001 / ISO/IEC/IEEE 29148 lanes.

## Why ISO/IEC/IEEE 12207

Where [29148](../audit-iso-iec-ieee-29148/SPEC.md) covers the *requirements* discipline, 12207 covers the **whole software life cycle** — the process framework an organization follows from agreement through maintenance. It is the second ISO/IEC/IEEE joint standard in the audit kind, and the natural complement to a requirements lane: 29148 asks "are the requirements right?", 12207 asks "is the life cycle that produces the software governed?". A crafted criterion id, standard designation, or clause reference is data, not a constraint — the runtime is body-agnostic.

## Criteria

Nine criteria across the 12207:2017 process groups (clause references are to 12207:2017):

| id | clause | severity | checks |
|---|---|---|---|
| `12207-agreement-processes` | 6.1 | medium | Acquisition/supply governed by defined agreements (scope, deliverables, acceptance). |
| `12207-project-planning` | 6.3.1 | high | A project plan defines scope, schedule, resources, and selected life-cycle tasks. |
| `12207-project-assessment-control` | 6.3.2 | medium | Progress/process performance assessed against plan; corrective action taken. |
| `12207-configuration-management` | 6.3.5 | high | Work products identified, version-controlled, baselined; changes controlled. |
| `12207-requirements-definition` | 6.4.2/6.4.3 | high | Stakeholder needs + system/software requirements defined, recorded, agreed. |
| `12207-architecture-design` | 6.4.4/6.4.5 | high | Architecture/design defined, satisfying + traceable to requirements. |
| `12207-implementation-integration` | 6.4.6/6.4.7 | medium | Elements implemented + integrated per design; integration verified incrementally. |
| `12207-verification-validation` | 6.4.9/6.4.11 | high | Work products verified against specs; system validated against stakeholder needs. |
| `12207-maintenance` | 6.4.13 | medium | Post-delivery modifications managed through a defined maintenance process. |

## How it runs

The `bwoc audit run` dispatcher sets `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, and `BWOC_AUDIT_OPERATION=audit_run`, then invokes `audit.sh`. The runtime reads `criteria.toml` (this plugin's declared criteria) and the operator's attestations, matches by `criterion_id`, and emits one finding per criterion. Read-only — it inspects the workspace and emits a report; it never mutates.

## Configuration

Enable the plugin, then declare an attestation per criterion the workspace claims to meet:

```toml
[plugins.audit-iso-iec-ieee-12207]
enabled = true

[[plugins.audit-iso-iec-ieee-12207.attestations]]
criterion_id = "12207-configuration-management"
statement    = "All source, docs, and build artefacts are in git with tagged baselines; changes land only via reviewed, CI-gated PRs."
signer       = "Eng Lead: Somchai T."
signed_at    = "2026-07-25"
# valid_through = "2027-07-25"   # optional
```

A criterion with no matching, well-formed attestation emits `status = "fail"` with the remedy pointing at this block.

## Findings schema

Per [PLUGINS.en.md §Audit Findings Schema](../../../docs/en/PLUGINS.en.md#audit-findings-schema): `evidence.kind = "attestation"`, `value` = the statement, plus `signer` + `signed_at` (+ optional `valid_through`). A pass carries the attestation; a fail carries the remedy.

## Exit codes

The **plugin** (`audit.sh`) exits `0` on success — non-pass findings are *findings*, not errors — and non-zero only on a runtime failure (missing `BWOC_WORKSPACE`, unreadable `criteria.toml`). The **`bwoc audit run` dispatcher** then derives its own exit code from the run: the number of `fail` findings (clamped to `254`), or `255` on a framework/runtime error. So a clean audit exits `0`, an audit with N fails exits `N`, and a broken plugin exits `255` (`crates/bwoc-cli/src/audit.rs::compute_exit_code`).

## Maturity

L1 — attestation runtime over the nine 12207 process criteria. Deeper checks (parsing an actual project plan / CM baseline / traceability from the workspace, not just an attestation) are a future slice, matching the roadmap of the other audit lanes.

## Neutrality

Backend-neutral: no LLM, no model, no vendor. Names a standard (ISO/IEC/IEEE 12207), not a tool. The attestation runtime is shared across the audit kind's standards.
