---
title: IEEE 1012 Verification and Validation Audit
aliases:
  - audit-ieee-1012
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/ieee-1012
  - status/runtime
maturity: L1
---

# IEEE 1012 Verification and Validation Audit

> [!abstract] **Attestation runtime (v0.1.0).** Audits a workspace's **verification and validation (V&V)** against **IEEE 1012:2016** — *IEEE Standard for System, Software, and Hardware Verification and Validation*. Reads operator-signed attestations from `.bwoc/workspace.toml` under `[[plugins.audit-ieee-1012.attestations]]` and emits `evidence.kind = "attestation"` findings (`signer` + `signed_at` + optional `valid_through`) per the [BWOC-27 schema extension](../../../docs/en/PLUGINS.en.md#evidence-kinds). Criteria without an operator attestation emit `status = "fail"` pointing at the `workspace.toml` block. It reuses the shared attestation runtime of the ISO 9001 / ISO/IEC 27001 / ISO/IEC/IEEE 29148 / 12207 lanes.

## Why IEEE 1012 — the first IEEE-standalone lane

The audit kind spans pure-ISO (9001), ISO/IEC (27001, 20000-1, 29110), and ISO/IEC/IEEE joint standards (29148, 12207). IEEE 1012 is the first **IEEE-only** (standalone) standard in the set — IEEE publishes it alone, not jointly. Supporting it proves the audit kind is genuinely body-agnostic: it audits against a standard by its *criteria*, regardless of whether one, two, or three bodies co-publish it. V&V is also the natural verification counterpart to 12207's life-cycle processes and 29148's requirements — the three form a coherent systems/software-assurance trio.

## Criteria

Eight criteria across the IEEE 1012:2016 V&V processes (clause references are to 1012:2016):

| id | clause | severity | checks |
|---|---|---|---|
| `1012-integrity-levels` | 5 | high | Each element assigned an integrity level scaling the required V&V rigour. |
| `1012-vv-planning` | 7 | high | A V&V Plan (SVVP) documents scope, activities, tasks, methods, integrity scaling. |
| `1012-independence` | 4 | medium | V&V independence (technical/managerial/financial) commensurate with integrity level. |
| `1012-requirements-vv` | Table 1 | high | Requirements verified (correct/consistent/complete/traceable) + validated vs needs. |
| `1012-design-vv` | Table 1 | high | Design verified for correctness + traceability; testability evaluated. |
| `1012-implementation-vv` | Table 1 | medium | Code/implementation verified vs design + coding standards (review, static analysis). |
| `1012-test-vv` | Table 1 | high | Test plans/designs/cases/procedures/results verified for adequacy + traced. |
| `1012-anomaly-reporting` | 7 | medium | V&V reports produced; anomalies recorded, classified, tracked to resolution. |

## How it runs

The `bwoc audit run` dispatcher sets `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, and `BWOC_AUDIT_OPERATION=audit_run`, then invokes `audit.sh`. The runtime reads `criteria.toml` (this plugin's declared criteria) and the operator's attestations, matches by `criterion_id`, and emits one finding per criterion. Read-only — it inspects the workspace and emits a report; it never mutates.

## Configuration

Enable the plugin, then declare an attestation per criterion the workspace claims to meet:

```toml
[plugins.audit-ieee-1012]
enabled = true

[[plugins.audit-ieee-1012.attestations]]
criterion_id = "1012-test-vv"
statement    = "Every requirement maps to at least one automated test; CI runs the suite on each PR and blocks merge on failure."
signer       = "QA Lead: Naruemon K."
signed_at    = "2026-07-25"
# valid_through = "2027-07-25"   # optional
```

A criterion with no matching, well-formed attestation emits `status = "fail"` with the remedy pointing at this block.

## Findings schema

Per [PLUGINS.en.md §Audit Findings Schema](../../../docs/en/PLUGINS.en.md#audit-findings-schema): `evidence.kind = "attestation"`, `value` = the statement, plus `signer` + `signed_at` (+ optional `valid_through`). A pass carries the attestation; a fail carries the remedy.

## Exit codes

The **plugin** (`audit.sh`) exits `0` on success — non-pass findings are *findings*, not errors — and non-zero only on a runtime failure (missing `BWOC_WORKSPACE`, unreadable `criteria.toml`). The **`bwoc audit run` dispatcher** then derives its own exit code from the run: the number of `fail` findings (clamped to `254`), or `255` on a framework/runtime error. So a clean audit exits `0`, an audit with N fails exits `N`, and a broken plugin exits `255` (`crates/bwoc-cli/src/audit.rs::compute_exit_code`).

## Maturity

L1 — attestation runtime over the eight 1012 V&V criteria. Deeper checks (parsing an actual SVVP / test-traceability matrix from the workspace, not just an attestation) are a future slice, matching the roadmap of the other audit lanes.

## Neutrality

Backend-neutral: no LLM, no model, no vendor. Names a standard (IEEE 1012), not a tool. The attestation runtime is shared across the audit kind's standards.
