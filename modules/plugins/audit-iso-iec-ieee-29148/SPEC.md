---
title: ISO/IEC/IEEE 29148 Requirements Engineering Audit
aliases:
  - audit-iso-iec-ieee-29148
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-ieee-29148
  - status/runtime
maturity: L1
---

# ISO/IEC/IEEE 29148 Requirements Engineering Audit

> [!abstract] **Attestation runtime (v0.1.0).** Audits a workspace's **requirements engineering** against **ISO/IEC/IEEE 29148:2018** — a standard jointly published by **ISO, IEC, and IEEE** (it supersedes IEEE 830 / IEEE 1233 / IEEE 1362). Reads operator-signed attestations from `.bwoc/workspace.toml` under `[[plugins.audit-iso-iec-ieee-29148.attestations]]` and emits `evidence.kind = "attestation"` findings (`signer` + `signed_at` + optional `valid_through`) per the [BWOC-27 schema extension](../../../docs/en/PLUGINS.en.md#evidence-kinds). Criteria without an operator attestation emit `status = "fail"` pointing at the `workspace.toml` block. This is the first **ISO/IEC/IEEE** lane — it extends the audit kind beyond pure-ISO to the full ISO / IEC / IEEE standards family, reusing the shared attestation runtime of the ISO 9001 / ISO/IEC 27001 lanes.

## Why ISO/IEC/IEEE (not just ISO)

The `audit` kind began with pure-ISO standards (ISO 9001) and joint **ISO/IEC** ones (27001, 20000-1, 29110). 29148 is the first **ISO/IEC/IEEE** joint standard in the set — the three bodies co-publish it as the canonical requirements-engineering standard for systems + software. Supporting it makes the audit kind's coverage explicit across all three standards bodies; a crafted criterion id, standard designation, or clause reference is data, not a constraint — the runtime is body-agnostic.

## Criteria

Seven criteria, each an operator attestation (clause references are to 29148:2018):

| id | clause | severity | checks |
|---|---|---|---|
| `29148-stakeholder-requirements` | 9.4 | high | Stakeholder needs recorded in a StRS. |
| `29148-system-software-requirements` | 9.5/9.6 | high | System/software requirements in a SyRS/SRS derived from the StRS. |
| `29148-requirement-characteristics` | 5.2.6 | high | Each requirement is necessary/unambiguous/singular/feasible/verifiable/correct/… |
| `29148-requirement-set-characteristics` | 5.2.7 | medium | The set is complete/consistent/feasible/comprehensible/validatable. |
| `29148-verifiability` | 5.2.6 | high | Each requirement has a verification method (inspection/analysis/demonstration/test). |
| `29148-traceability` | 5.2.8 | high | Bidirectional traceability: stakeholder → system → software → verification. |
| `29148-requirements-management` | 6.5 | medium | Requirements baselined + change-controlled. |

## How it runs

The `bwoc audit run` dispatcher sets `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, and `BWOC_AUDIT_OPERATION=audit_run`, then invokes `audit.sh`. The runtime reads `criteria.toml` (this plugin's declared criteria) and the operator's attestations, matches by `criterion_id`, and emits one finding per criterion. Read-only — it inspects the workspace and emits a report; it never mutates.

## Configuration

Enable the plugin, then declare an attestation per criterion the workspace claims to meet:

```toml
[plugins.audit-iso-iec-ieee-29148]
enabled = true

[[plugins.audit-iso-iec-ieee-29148.attestations]]
criterion_id = "29148-traceability"
statement    = "A requirements traceability matrix links every StRS need → SyRS/SRS requirement → test case; reviewed each release."
signer       = "RE Lead: Anong P."
signed_at    = "2026-07-24"
# valid_through = "2027-07-24"   # optional
```

A criterion with no matching, well-formed attestation emits `status = "fail"` with the remedy pointing at this block.

## Findings schema

Per [PLUGINS.en.md §Audit Findings Schema](../../../docs/en/PLUGINS.en.md#audit-findings-schema): `evidence.kind = "attestation"`, `value` = the statement, plus `signer` + `signed_at` (+ optional `valid_through`). A pass carries the attestation; a fail carries the remedy.

## Exit codes

The **plugin** (`audit.sh`) exits `0` on success — non-pass findings are *findings*, not errors — and non-zero only on a runtime failure (missing `BWOC_WORKSPACE`, unreadable `criteria.toml`). The **`bwoc audit run` dispatcher** then derives its own exit code from the run: the number of `fail` findings (clamped to `254`), or `255` on a framework/runtime error. So a clean audit exits `0`, an audit with N fails exits `N`, and a broken plugin exits `255` (`crates/bwoc-cli/src/audit.rs::compute_exit_code`).

## Maturity

L1 — attestation runtime over the seven 29148 criteria. Deeper checks (parsing an actual SRS/traceability matrix from the workspace, not just an attestation) are a future slice, matching the roadmap of the ISO 9001 / 27001 lanes.

## Neutrality

Backend-neutral: no LLM, no model, no vendor. Names a standard (ISO/IEC/IEEE 29148), not a tool. The attestation runtime is shared across the audit kind's standards.
