---
title: ISO/IEC 27001 Information Security Management System Audit
aliases:
  - audit-iso-27001
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-27001
  - status/runtime
maturity: L1
---

# ISO/IEC 27001 Information Security Management System Audit

> [!abstract] **Attestation + SoA-driven sample runtime (v0.2.0).** The last EPIC-3 ISMS runtime — closes the epic. Each criterion declares its `expected_evidence_kind` in [[criteria]]; the runtime reads operator-provided evidence from `.bwoc/workspace.toml` and emits `evidence.kind = "attestation"` for the main-body management-system clauses (scope, policy, risk assessment, SoA, internal audit) and `evidence.kind = "sample"` for the Annex A controls (access, incident, continuity), per the [BWOC-27 schema](../../../docs/en/PLUGINS.en.md#evidence-kinds). 27001 is the only runtime whose sampling **population is operator-declared**: the [Statement of Applicability](https://www.iso.org/standard/27001) (clause 6.1.3) decides which Annex A controls are in scope, and the runtime samples from that set. A justifiably-excluded control emits `status = "not_applicable"`; a control absent from the SoA or lacking a sample emits `status = "fail"` pointing at `workspace.toml`. Replaces the v0.1.0 stub from EPIC-2.

## Status & Roadmap

| Version | Date | Change |
|---|---|---|
| v0.1.0 | 2026-05-26 | Stub. Eight criteria declared; every finding `status = "not_implemented"`. Schema conformance only — no workspace inspection. Landed in EPIC-2. |
| v0.2.0 | 2026-05-27 | **Attestation + SoA-driven sample runtime.** Reads `[[plugins.audit-iso-27001.attestations]]`, `[[plugins.audit-iso-27001.soa]]`, and `[[plugins.audit-iso-27001.samples]]` from `workspace.toml`; routes each criterion by its `expected_evidence_kind` (declared in `criteria.toml`) and emits `attestation` or SoA-gated `sample` findings, with `status = "fail"` + a `workspace.toml`-pointing remedy for criteria lacking evidence and `status = "not_applicable"` for justifiably-excluded Annex A controls. The eight `criterion_id`s carry over unchanged (stability contract, PLUGINS.en.md §Stability). No dispatcher change was needed — `crates/bwoc-cli/src/audit.rs` already validates `attestation`, `sample`, and the `not_applicable` status (BWOC-28/29). Landed in EPIC-3 BWOC-34, closing the epic. See [[../../notes/2026-05-27_27001-soa-sampling]] for the design. |

## Why a Runtime Now

[[../../notes/2026-05-26_iso-compliance-plugins|The EPIC-2 framing note]] explained why 27001 shipped as a stub first — its evidence is information-security management practice (risk assessments, control selection, incident drills, continuity exercises) that reduces to organisational evidence rather than repository artifacts, and the v1 schema could not express it. EPIC-3 closed that gap:

- The [BWOC-26 design note](../../../notes/2026-05-27_iso-runtime-evidence-model.md) pinned the new evidence model (`attestation`, `sample`, time-bounded fields) and framed Annex A sampling as *"we sampled controls A.5.15, A.5.24, A.5.29 (3 of 37) without inflating the finding count to 37."*
- [BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) extended the schema with `attestation` (required `signer` + `signed_at`) and `sample` (required `sampled_count` + `sampled_of`, optional `window`).
- BWOC-28 built the 9001 attestation runtime and extended the dispatcher to validate the new kinds; BWOC-33 built the 20000-1 attestation + sample runtime.
- BWOC-34 (this change) builds the 27001 runtime — the only one whose sample population is the operator's Statement of Applicability, not a hand-typed number. Inferring "this organization has run an information security risk assessment" from "this repo contains `RISKS.md`" would still falsify the audit (Musāvāda — [PHILOSOPHY.en.md](../../agent-template/docs/en/PHILOSOPHY.en.md) §Sila 5). Attestation evidence keeps the main-body clauses honest (an operator vouches, dated, with provenance); SoA-gated sample evidence keeps the Annex A controls honest (the operator records that the in-scope control was sampled this cycle, and the SoA decides which controls are in scope at all).

## Criteria (v0.2.0)

Eight headline ISMS criteria, drawn from the main clauses and the Annex A controls of ISO/IEC 27001:2022[^iso-27001-2022]. Declaration order in [[criteria]] is the report order (PLUGINS.en.md §Audit Findings Schema). `criterion_id` values are stable across releases (PLUGINS.en.md §Stability); renames are a major version bump. The **Kind** column is each criterion's `expected_evidence_kind` — see [[../../notes/2026-05-27_27001-soa-sampling]] for the main-body-vs-Annex-A split rationale.

| `criterion_id` | Reference | Title | Severity | Kind |
|---|---|---|---|---|
| `27001-isms-scope` | 4.3 | Scope of the ISMS | high | attestation |
| `27001-information-security-policy` | 5.2 | Information security policy | high | attestation |
| `27001-risk-assessment` | 6.1.2 | Information security risk assessment | critical | attestation |
| `27001-statement-of-applicability` | 6.1.3 | Statement of Applicability | critical | attestation |
| `27001-access-control` | A.5.15 | Access control | high | sample |
| `27001-incident-management` | A.5.24 | Incident management planning and preparation | high | sample |
| `27001-business-continuity` | A.5.29 | Information security during disruption | medium | sample |
| `27001-internal-audit` | 9.2 | Internal audit | high | attestation |

Five attestation + three sample. The split tracks the structural divide in ISO/IEC 27001:2022 itself: the main-body management-system clauses (4–10 — scope, policy, risk assessment, SoA, internal audit) are documented-artifact conformance the operator vouches for, so they use `attestation`; the Annex A controls are the technical/organisational measures an auditor *samples*, so they use `sample`. The three sampled controls are from the *Organizational* theme (A.5.15 access, A.5.24 incident, A.5.29 continuity); the remaining 90 Annex A controls are deferred — adding criteria is a minor-version bump, never a rename of an existing id.

## How It Runs

```bash
bwoc audit run --plugin audit-iso-27001 --json
```

The dispatcher spawns `audit.sh` (per `BWOC-12`) with the standard env contract: `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION`. The runtime:

1. Reads `criteria.toml` from `BWOC_PLUGIN_DIR` for the declared criteria (id, severity, **`expected_evidence_kind`**, and — for Annex A criteria — **`annex_control`**).
2. Reads `.bwoc/workspace.toml` from `BWOC_WORKSPACE` and builds three lookup tables — `[[plugins.audit-iso-27001.attestations]]` and `[[plugins.audit-iso-27001.samples]]` keyed by `criterion_id`, and `[[plugins.audit-iso-27001.soa]]` keyed by `control`. It computes the SoA-driven population (`K of M`, below) up front.
3. For each criterion in declaration order, routes by `expected_evidence_kind`:
   - **`attestation`** — present and complete (`statement` + `signer` + `signed_at`) → `status = "pass"`, `evidence.kind = "attestation"` with optional `valid_through`. Present but incomplete, or absent → `status = "fail"`, `evidence.kind = "file"` pointing at `.bwoc/workspace.toml`.
   - **`sample`** — SoA-gated (see [SoA-Driven Annex A Sampling](#soa-driven-annex-a-sampling)). In scope + justified + sampled → `status = "pass"`, `evidence.kind = "sample"`. Justifiably excluded → `status = "not_applicable"`, `evidence.kind = "none"`. Absent from the SoA, unjustified, or in scope without a sample → `status = "fail"`, `evidence.kind = "file"`.
4. Exits `0` on success — non-pass findings are *findings*, not errors. A non-zero exit signals a framework-side problem (unreadable `criteria.toml`).

The dispatcher's process exit code is the count of `fail` findings (BWOC-12); `not_applicable` does **not** count as a fail. The runtime does **not** impose an organisational threshold on samples — a recorded sample is evidence and passes, and the human summary is surfaced for the auditor to judge.

## SoA-Driven Annex A Sampling

20000-1's samples are self-contained: the operator types `sampled_count` / `sampled_of` directly per sample. 27001 is different — the sampling population is **operator-declared scope**, not a hand-typed number, so the operator never types `sampled_count` / `sampled_of` for an Annex A control. Both are derived from the Statement of Applicability:

- **`M` = `sampled_of`** = the number of in-scope controls in the SoA (entries with `applicable = true`). This is the sampling population.
- **`K` = `sampled_count`** = the number of *this plugin's* three Annex A controls that are in scope. `K ≤ M` by construction. `K` is computed by **scope**, not by evidence completeness, so one control's `sampled_count` never depends on a sibling's sample — findings stay independent (PLUGINS.en.md §schema-rules; BWOC-11 "a criterion passes or fails as a unit"). An in-scope control missing its sample **fails**; it does not silently deflate `K` on the others.

This reproduces the BWOC-26 narrative ("we sampled 3 of 37") as the aggregate of three `K`-of-`M` findings — and it tracks the operator's scope decisions automatically: exclude a control and `M` shrinks; the reported denominator follows. Supplying `sampled_count` / `sampled_of` by hand was rejected — it would let the operator's typed denominator drift from the SoA-derived one, defeating the point.

### Pass / not_applicable / fail for an Annex A criterion

For each Annex A criterion, the runtime resolves its `annex_control` (from `criteria.toml`) and looks it up in the SoA:

| SoA state for the control | Status | Evidence | Remedy |
|---|---|---|---|
| Absent from SoA | `fail` | `file` → `.bwoc/workspace.toml` | "6.1.3 requires the SoA to address every Annex A control. Declare `control`, `applicable`, `justification`." |
| `applicable = false`, no justification | `fail` | `file` | "6.1.3 requires a justification for exclusions. Add `justification`." |
| `applicable = false`, justified | `not_applicable` | `none` | "Excluded per SoA: \"…\". Re-confirm at the next audit cycle." |
| `applicable = true`, no justification | `fail` | `file` | "6.1.3 requires a justification for inclusions too. Add `justification`." |
| `applicable = true`, no sample entry | `fail` | `file` | "In scope but no recorded sample. Add `[[…samples]]` with `criterion_id` + `summary`." |
| `applicable = true`, sample missing `summary` | `fail` | `file` | names the missing field. |
| `applicable = true`, justified, sample present | `pass` | `sample` (`value` = summary, `sampled_count` = K, `sampled_of` = M, optional `window`) | — |

`not_applicable` is the honest ISO outcome for a justifiably-excluded control — it is neither a pass (we did not test it) nor a fail (the operator made a defensible scoping decision). The schema permits `evidence.kind = "none"` only with `not_applicable` / `not_implemented`, and requires a `remedy` for `not_applicable` — both honoured.

### The SoA's two roles

The SoA appears twice, and the two uses are intentionally **independent**:

1. **The `27001-statement-of-applicability` attestation (6.1.3)** — routed through `[[…attestations]]` like any other attestation; top management attests "the SoA is established and maintained," signed and dated.
2. **The `[[…soa]]` array** — the machine-readable in-scope declarations that drive Annex A sampling.

They are complementary (an operator who maintains a real SoA naturally does both) but not coupled in code — the attestation passes/fails on its own evidence; the soa array drives the three Annex A criteria on its own. Coupling them was rejected: it would break the flat, independent-finding model.

## Sample Output

An attestation criterion (main-body clause):

```json
{
  "criterion_id": "27001-risk-assessment",
  "severity":     "critical",
  "status":       "pass",
  "evidence": {
    "kind":          "attestation",
    "value":         "Information security risk assessment performed 2026-03-10; methodology documented; results comparable and reproducible.",
    "signer":        "CISO: Tonkla K.",
    "signed_at":     "2026-03-10",
    "valid_through": "2027-03-10"
  }
}
```

An in-scope, sampled Annex A control (SoA-driven `K of M`):

```json
{
  "criterion_id": "27001-access-control",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "sample",
    "value":         "Access reviews completed across in-scope systems; 0 orphaned accounts found.",
    "sampled_count": 1,
    "sampled_of":    3,
    "window":        "2026-Q1"
  }
}
```

A justifiably-excluded Annex A control:

```json
{
  "criterion_id": "27001-business-continuity",
  "severity":     "medium",
  "status":       "not_applicable",
  "evidence":     { "kind": "none", "value": "" },
  "remedy":       "Control A.5.29 is excluded from the ISMS scope per the Statement of Applicability: \"No formal continuity programme; risk accepted by management for a solo workspace.\". Re-confirm this exclusion remains justified at the next audit cycle."
}
```

A criterion without operator evidence (here an Annex A control absent from the SoA):

```json
{
  "criterion_id": "27001-incident-management",
  "severity":     "high",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": ".bwoc/workspace.toml" },
  "remedy":       "Control A.5.24 is not addressed in the Statement of Applicability. ISO/IEC 27001 6.1.3 requires the SoA to address every Annex A control. Declare it in .bwoc/workspace.toml under [[plugins.audit-iso-27001.soa]] with control=\"A.5.24\", applicable (true/false), and justification."
}
```

`bwoc audit run` wraps the findings array in the canonical envelope `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`.

## Configuration

```toml
# .bwoc/workspace.toml
[plugins.audit-iso-27001]
enabled = true

# attestation criteria (4.3 scope, 5.2 policy, 6.1.2 risk, 6.1.3 SoA, 9.2 internal audit)
[[plugins.audit-iso-27001.attestations]]
criterion_id  = "27001-risk-assessment"
statement     = "Information security risk assessment performed 2026-03-10; methodology documented."
signer        = "CISO: Tonkla K."
signed_at     = "2026-03-10"
valid_through = "2027-03-10"   # optional

# Statement of Applicability — one entry per Annex A control the operator has
# assessed. 6.1.3 requires a justification for BOTH inclusions and exclusions.
[[plugins.audit-iso-27001.soa]]
control       = "A.5.15"
applicable    = true
justification = "Access control is central to protecting source, credentials, and customer data."

[[plugins.audit-iso-27001.soa]]
control       = "A.5.29"
applicable    = false
justification = "No formal continuity programme; risk accepted by management for a solo workspace."

# Annex A audit-sample records. Thinner than 20000-1: the operator does NOT
# supply sampled_count/sampled_of — they are SoA-derived (K of M).
[[plugins.audit-iso-27001.samples]]
criterion_id = "27001-access-control"
summary      = "Access reviews completed across in-scope systems; 0 orphaned accounts found."
window       = "2026-Q1"   # optional
```

All three blocks are **array-of-tables** under the universal `[plugins.audit-iso-27001]` block.

`[[…attestations]]` fields:

| Field | Required | Notes |
|---|---|---|
| `criterion_id` | yes | Must match an `attestation`-kind criterion in `criteria.toml`. |
| `statement` | yes | Verbatim attestation text → `evidence.value`. Single-line basic TOML string. |
| `signer` | yes | Free-text identity → `evidence.signer`. |
| `signed_at` | yes | ISO 8601 date (or datetime) → `evidence.signed_at`. |
| `valid_through` | optional | ISO 8601 expiry date → `evidence.valid_through`. The dispatcher stamps but does not enforce expiry (per BWOC-26). |

`[[…soa]]` fields (the Statement of Applicability):

| Field | Required | Notes |
|---|---|---|
| `control` | yes | Annex A control reference (e.g. `"A.5.15"`). Matched against each Annex A criterion's `annex_control` in `criteria.toml`. |
| `applicable` | yes | TOML boolean. `true` → in scope (counts toward `M`); `false` → excluded (emits `not_applicable` when justified). |
| `justification` | yes | 6.1.3 requires a justification for **both** inclusions and exclusions. Surfaced in the finding's remedy for `not_applicable`. |

`[[…samples]]` fields:

| Field | Required | Notes |
|---|---|---|
| `criterion_id` | yes | Must match a `sample`-kind criterion in `criteria.toml`. |
| `summary` | yes | Short human summary of what was sampled → `evidence.value`. |
| `window` | optional | Free-text time period → `evidence.window` (e.g. `"2026-Q1"`). |

The operator does **not** supply `sampled_count` / `sampled_of` here (contrast 20000-1) — they are SoA-derived. The first occurrence of a given key wins; the runtime does not flag duplicates (that is `bwoc check`'s job — BWOC-29). `workspace.toml` was chosen over a separate file for the same smallest-blast-radius reasons as the 9001/20000-1 evidence sources: operators already touch this file, every change is diff-able in `git log`, and `bwoc check` already walks it. See [[../../notes/2026-05-27_27001-soa-sampling]] for the full rationale.

## Maturity

Declared **L1** — runtime operational, emits real attestation + SoA-driven sample findings per BWOC-27. Bumps to **L2** when:

- `bwoc check` validates the workspace `[[attestations]]` / `[[soa]]` / `[[samples]]` blocks against the schema (extends BWOC-29 from `criteria.toml` to `workspace.toml`) — including well-formed `control` refs and `applicable` booleans in the SoA.
- The Annex A catalogue grows beyond the three A.5 controls to representative controls from A.6 (People), A.7 (Physical), and A.8 (Technological) — additive, no id renames.
- Control-to-clause traceability lands (A.5.24 feeds both clause 9 and clause 10) via a future `evidence.related_criteria`.

## Neutrality

Manifest values name no LLM backend, vendor, or model. `kind = "audit"` is the framework's own enum (`BWOC-10`). The plugin references "ISO/IEC 27001" exclusively in `description`, in this SPEC's prose, and in the standardized `27001-*` criterion-id namespace — never in `criteria.toml` keys or in finding values beyond that namespace. Satisfies the **Samānattatā** rule.

## Sources

The ISMS criteria are drawn from the published ISO/IEC 27001:2022 main-body clauses (4 through 10) and the Annex A control set:

- ISO/IEC 27001:2022 — *Information security, cybersecurity and privacy protection — Information security management systems — Requirements.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/27001>. [^iso-27001-2022]
- ISO/IEC 27002:2022 — *Information security, cybersecurity and privacy protection — Information security controls.* ISO catalogue entry: <https://www.iso.org/standard/75652.html>. Companion standard providing implementation guidance for the Annex A controls.
- ISO/IEC JTC 1/SC 27 — *Information security, cybersecurity and privacy protection.* Public landing page: <https://www.iso.org/committee/45306.html>. The joint technical committee responsible for the ISO/IEC 27000 family.

[^iso-27001-2022]: ISO/IEC 27001:2022 follows the Annex SL high-level structure (clauses 4 Context, 5 Leadership, 6 Planning, 7 Support, 8 Operation, 9 Performance evaluation, 10 Improvement). Annex A contains 93 information security controls organized into four themes — *Organizational* (37 controls, prefix A.5.x), *People* (8 controls, prefix A.6.x), *Physical* (14 controls, prefix A.7.x), and *Technological* (34 controls, prefix A.8.x) — restructured from the 114-control / 14-clause arrangement in ISO/IEC 27001:2013. The eight criteria here cover five main-body clauses plus three *Organizational*-theme Annex A controls that headline ISMS operations; the remaining 90 Annex A controls are deferred to a future minor-version expansion once the SoA-driven runtime samples them coherently.

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `audit` kind row (BWOC-10), Audit Findings Schema (BWOC-11 + BWOC-27 evidence kinds).
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — EPIC-2 framing note (why stubs).
- [[../../notes/2026-05-27_iso-runtime-evidence-model|2026-05-27_iso-runtime-evidence-model.md]] — BWOC-26 evidence-model design (attestation, sample, the "3 of 37 Annex A" framing).
- [[../../notes/2026-05-27_27001-soa-sampling|2026-05-27_27001-soa-sampling.md]] — BWOC-34 design (evidence-kind split + SoA-driven sampling population).
- [[../audit-iso-29110/SPEC|audit-iso-29110]] — the runnable reference audit plugin from BWOC-13.
- [[../audit-iso-9001/SPEC|audit-iso-9001]] — sibling attestation runtime (BWOC-28); [[../audit-iso-20000-1/SPEC|audit-iso-20000-1]] — sibling attestation + sample runtime (BWOC-33).
