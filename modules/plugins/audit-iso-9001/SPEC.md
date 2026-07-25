---
title: ISO 9001 Quality Management System Audit
aliases:
  - audit-iso-9001
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-9001
  - status/runtime
maturity: L1
---

# ISO 9001 Quality Management System Audit

> [!abstract] **Attestation runtime (v0.2.0).** Reads operator-signed attestations from `.bwoc/workspace.toml` under `[[plugins.audit-iso-9001.attestations]]` and emits `evidence.kind = "attestation"` findings (`signer` + `signed_at` + optional `valid_through`) per the [BWOC-27 schema extension](../../../docs/en/PLUGINS.en.md#evidence-kinds). Criteria without an operator attestation emit `status = "fail"` pointing at the `workspace.toml` block. Replaces the v0.1.0 stub from EPIC-2.

## Status & Roadmap

| Version | Date | Change |
|---|---|---|
| v0.1.0 | 2026-05-26 | Stub. Eight criteria declared; every finding `status = "not_implemented"`. Schema conformance only — no workspace inspection. Landed in EPIC-2. |
| v0.2.0 | 2026-05-27 | **Attestation runtime.** Reads `[[plugins.audit-iso-9001.attestations]]` from `workspace.toml`; emits `kind = "attestation"` for criteria with an operator attestation and `status = "fail"` with a `workspace.toml`-pointing remedy for the rest. The eight `criterion_id`s carry over unchanged (stability contract, PLUGINS.en.md §Stability). Companion change in `crates/bwoc-cli/src/audit.rs` extends the dispatcher's schema validator to accept attestation + sample evidence kinds and pass through sub-fields — runtime-side companion to the BWOC-27 doc-side schema bump. Landed in EPIC-3 BWOC-28. See [[../../notes/2026-05-27_9001-runtime-attestation-source]] for the design. |

## Why a Runtime Now

[[../../notes/2026-05-26_iso-compliance-plugins|The EPIC-2 framing note]] explained why 9001 shipped as a stub first — its evidence is organisational, not file-existence-shaped, and the v1 schema could not express attestation. EPIC-3 closed that gap:

- The [BWOC-26 design note](../../../notes/2026-05-27_iso-runtime-evidence-model.md) pinned the new evidence model (`attestation`, `sample`, time-bounded fields).
- [BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) extended the schema with `attestation` (required `signer` + `signed_at`) and the orthogonal optional `valid_through`.
- BWOC-28 (this change) implements the 9001 runtime against the new schema. The operator provides attestations in `workspace.toml`; the plugin emits one finding per criterion, honest about which are covered and which are not. Inferring "this organization has a documented quality policy" from "this repo contains `POLICY.md`" would still falsify the audit (Musāvāda — [PHILOSOPHY.en.md](../../agent-template/docs/en/PHILOSOPHY.en.md) §Sila 5). Attestation evidence keeps the audit honest by requiring an operator to vouch for the practice, dated, with provenance.

## Criteria (v0.2.0)

Eight headline QMS criteria, drawn from the main clauses of ISO 9001:2015[^iso-9001-2015]. Declaration order in [[criteria]] is the report order (PLUGINS.en.md §Audit Findings Schema). `criterion_id` values are stable across releases (PLUGINS.en.md §Stability); renames are a major version bump.

| `criterion_id` | Clause | Title | Severity |
|---|---|---|---|
| `9001-context-of-organization` | 4 | Context of the organization | high |
| `9001-leadership-and-policy` | 5.2 | Leadership and quality policy | high |
| `9001-risks-and-opportunities` | 6.1 | Actions to address risks and opportunities | high |
| `9001-competence-and-awareness` | 7.2 | Competence and awareness | medium |
| `9001-documented-information` | 7.5 | Documented information | medium |
| `9001-internal-audit` | 9.2 | Internal audit | high |
| `9001-management-review` | 9.3 | Management review | high |
| `9001-corrective-action` | 10.2 | Nonconformity and corrective action | medium |

The eight criteria cover one practice per major clause (4 through 10) that every QMS-conformant organization is expected to operate. The residual sub-clauses (resources, infrastructure, monitoring & measurement of operational processes, customer satisfaction surveys, etc.) remain deferred — adding criteria is a minor-version bump, never a rename of an existing id.

## How It Runs (Today)

```bash
bwoc audit run --plugin audit-iso-9001 --json
```

The dispatcher spawns `audit.sh` (per `BWOC-12`) with the standard env contract: `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION`. The runtime:

1. Reads `criteria.toml` from `BWOC_PLUGIN_DIR` for the declared criteria (id, severity).
2. Reads `.bwoc/workspace.toml` from `BWOC_WORKSPACE` and walks `[[plugins.audit-iso-9001.attestations]]`, building an attestation table keyed by `criterion_id`.
3. For each criterion in declaration order:
   - **Attestation present and complete** (`statement` + `signer` + `signed_at`) → `status = "pass"`, `evidence.kind = "attestation"` with `value = statement`, `signer`, `signed_at`, optional `valid_through`.
   - **Attestation present but incomplete** → `status = "fail"`, `evidence.kind = "file"` pointing at `.bwoc/workspace.toml`, remedy names the missing required field(s).
   - **Attestation absent** → `status = "fail"`, `evidence.kind = "file"` pointing at `.bwoc/workspace.toml`, remedy names the criterion_id and the required fields.
4. Exits `0` on success — non-pass findings are *findings*, not errors. A non-zero exit signals a framework-side problem (unreadable `criteria.toml`).

The dispatcher's process exit code is the count of `fail` findings (BWOC-12), so a workspace with no attestations exits `8`, one with two attestations exits `6`, and one with all eight exits `0`.

## Sample Output

A criterion with an attestation:

```json
{
  "criterion_id": "9001-management-review",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "attestation",
    "value":         "Management review held 2026-04-15 covering Q1 QMS performance, customer feedback, internal audit results, improvement opportunities.",
    "signer":        "Quality Manager: Tonkla K.",
    "signed_at":     "2026-04-15",
    "valid_through": "2027-04-15"
  }
}
```

A criterion without an attestation:

```json
{
  "criterion_id": "9001-internal-audit",
  "severity":     "high",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": ".bwoc/workspace.toml" },
  "remedy":       "Provide a signed attestation in .bwoc/workspace.toml under [[plugins.audit-iso-9001.attestations]] with criterion_id=\"9001-internal-audit\", statement, signer, and signed_at (ISO 8601 date) to satisfy this criterion."
}
```

`bwoc audit run` wraps the findings array in the canonical envelope `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`.

## Configuration

```toml
# .bwoc/workspace.toml
[plugins.audit-iso-9001]
enabled = true

[[plugins.audit-iso-9001.attestations]]
criterion_id  = "9001-management-review"
statement     = "Management review held 2026-04-15 covering Q1 QMS performance, customer feedback, internal audit results, improvement opportunities."
signer        = "Quality Manager: Tonkla K."
signed_at     = "2026-04-15"
valid_through = "2027-04-15"   # optional

[[plugins.audit-iso-9001.attestations]]
criterion_id = "9001-leadership-and-policy"
statement    = "Quality policy v1.2 ratified 2026-01-10 — aligned with strategic direction."
signer       = "Top Management: Tonkla K."
signed_at    = "2026-01-10"
```

Each `[[plugins.audit-iso-9001.attestations]]` entry is an **array-of-tables** under the universal `[plugins.audit-iso-9001]` block. Fields:

| Field | Required | Notes |
|---|---|---|
| `criterion_id` | yes | Must match an entry in `criteria.toml` (one of the eight declared `9001-*` ids). |
| `statement` | yes | Verbatim attestation text. Becomes `evidence.value` on the finding. v0.2.0 expects a single-line basic TOML string. |
| `signer` | yes | Free-text identity. Becomes `evidence.signer`. Example: `"Quality Manager: Tonkla K."`. |
| `signed_at` | yes | ISO 8601 date (or datetime). Becomes `evidence.signed_at`. |
| `valid_through` | optional | ISO 8601 date when the attestation stops being authoritative. Becomes `evidence.valid_through`. The dispatcher stamps but does not enforce expiry — that is downstream tooling's job (per BWOC-26). |

The first occurrence of a given `criterion_id` wins; the runtime does not flag duplicates (that is `bwoc check`'s job — see BWOC-29).

`workspace.toml` was chosen over a separate `attestations/9001.toml` file or a `BWOC_AUDIT_ATTESTATIONS` env path because it is the smallest-blast-radius mechanism: operators already touch this file to enable the plugin, every change is diff-able in `git log`, and `bwoc check` already walks it. See [[../../notes/2026-05-27_9001-runtime-attestation-source]] for the full rationale.

## Maturity

Declared **L1** — runtime operational, emits real attestation findings per BWOC-27. Bumps to **L2** when:

- `bwoc check` validates the workspace `[[attestations]]` block against the schema (BWOC-29).
- A `bwoc audit report --expired` flag surfaces `valid_through` expiry as warnings.
- Multi-line `statement` strings are supported (TOML triple-quoted form).

## Neutrality

Manifest values name no LLM backend, vendor, or model. `kind = "audit"` is the framework's own enum (`BWOC-10`). The plugin references "ISO 9001" exclusively in `description`, in this SPEC's prose, and in the standardised `9001-*` criterion-id namespace — never in `criteria.toml` keys or in finding values beyond that namespace. Satisfies the **Samānattatā** rule.

## Sources

The QMS criteria are drawn from the published ISO 9001:2015 main-body clauses (4 through 10):

- ISO 9001:2015 — *Quality management systems — Requirements.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/62085.html>. [^iso-9001-2015]
- ISO/TC 176/SC 2 — *Quality management and quality assurance — Quality systems.* Public landing page: <https://www.iso.org/committee/53896.html>. The technical committee responsible for the ISO 9000 family.
- ISO — *Quality management principles* (publicly available brochure summarising the seven QMS principles that underpin the 2015 revision): <https://www.iso.org/publication/PUB100080.html>.

[^iso-9001-2015]: ISO 9001:2015 is structured around the Annex SL high-level structure (clauses 4 Context, 5 Leadership, 6 Planning, 7 Support, 8 Operation, 9 Performance evaluation, 10 Improvement). The eight criteria here cover one headline practice per main clause that every QMS-conformant organization is expected to operate; the residual sub-clauses (7.1 Resources, 8.5 Production and service provision, 9.1.2 Customer satisfaction, etc.) are not yet declared — adding criteria in a future minor-version bump is the path forward.

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `audit` kind row (BWOC-10), Audit Findings Schema (BWOC-11 + BWOC-27 evidence kinds).
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — EPIC-2 framing note (why stubs).
- [[../../notes/2026-05-27_iso-runtime-evidence-model|2026-05-27_iso-runtime-evidence-model.md]] — BWOC-26 evidence-model design (attestation, sample, time-bounded fields).
- [[../../notes/2026-05-27_9001-runtime-attestation-source|2026-05-27_9001-runtime-attestation-source.md]] — BWOC-28 design (attestation source mechanism + dispatcher reach).
- [[../audit-iso-29110/SPEC|audit-iso-29110]] — the runnable reference audit plugin from BWOC-13.
- [[../audit-iso-20000-1/SPEC|audit-iso-20000-1]], [[../audit-iso-27001/SPEC|audit-iso-27001]] — sibling stubs (runtimes scheduled for S5).
