---
title: ISO/IEC 29110 Basic-Profile Audit
aliases:
  - audit-iso-29110
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-29110
maturity: L1
---

# ISO/IEC 29110 Basic-Profile Audit

> [!abstract] First **runnable** reference plugin for the `audit` kind. Runs file-existence checks against the Basic-profile work products defined by **ISO/IEC TR 29110-5-1-2** (Software engineering — Lifecycle profiles for Very Small Entities — Management and engineering guide: Generic profile group: Basic profile). Emits findings conforming to the BWOC-11 schema, dispatched through `bwoc audit run`.

## Why ISO/IEC 29110 First

ISO/IEC 29110 is the right first runnable `audit` plugin for the same reasons captured in [[../../notes/2026-05-26_iso-compliance-plugins|the EPIC-2 framing note]]:

- **Scope match.** The Basic profile targets Very Small Entities (VSEs) of 1–25 people, mapping cleanly onto a single-operator BWOC workspace plus a small fleet of agents. The QMS / ITSM / ISMS frameworks (`audit-iso-9001`, `audit-iso-20000-1`, `audit-iso-27001`) describe organisational practices that do not reduce to file-existence — they ship as stubs in Sprint 3 and earn runtime in `BWOC-EPIC-3`.
- **Mechanisable evidence.** The Basic-profile Project Management (PM) and Software Implementation (SI) processes call out concrete *work products* (Project Plan, SRS, Design, Test Plan, Verification Results, Construction Records) that map onto files in a typical software workspace. File-existence is the simplest possible audit runtime; it lets the plugin ship without network, shell-outs, or external tools.
- **Schema forcing function.** Running a real audit against a real standard exercises the BWOC-11 findings schema before three stub plugins in Sprint 3 conform to it. The `pass`, `fail`, and (where applicable) `not_applicable` paths all surface from this one plugin.

## Audited Work Products (v0.1.0)

Six Basic-profile work products are checked in v0.1.0 — six is the lower-mid of the "5–8 criteria" budget the BWOC-13 brief sets, and each maps to a single named artifact a VSE-grade project would produce. The full Basic-profile Task Output table[^iso-29110-5-1-2] enumerates ~22 work products across PM and SI; v0.1.0 deliberately covers the *headline* outputs that every Basic-profile project is expected to have. Subsequent versions can extend the list without breaking the BWOC-11 stability contract (adding criteria is a minor-version bump per `[plugin].version`).

| `criterion_id` | Process | ISO Work Product | Severity | Primary path |
|---|---|---|---|---|
| `29110-bp-project-plan` | PM | Project Plan | high | `docs/en/PROJECT-PLAN.en.md` |
| `29110-bp-software-requirements-specification` | SI | Software Requirements Specification | high | `docs/en/SRS.en.md` |
| `29110-bp-software-design` | SI | Software Design | medium | `docs/en/DESIGN.en.md` |
| `29110-bp-software-test-plan` | SI | Software Test Plan | medium | `docs/en/TEST-PLAN.en.md` |
| `29110-bp-verification-results` | SI | Verification Results | medium | `docs/en/VERIFICATION.en.md` |
| `29110-bp-software-construction-records` | SI | Software Construction Records | low | `CHANGELOG.md` |

Every criterion declares an ordered `candidates` list in [[criteria]] — alternate paths the audit will accept (e.g. `docs/SRS.md` or `REQUIREMENTS.md` for the SRS work product). The first candidate that exists wins; if none exist the finding is `status = "fail"` and the remedy cites the primary path. `criterion_id` values are stable across releases (PLUGINS.en.md §Stability) — renames are a major version bump on the plugin's own semver.

> [!note] **Severity reflects the criterion's importance, not the outcome.** A `critical` finding with `status = "pass"` is normal — it means "we checked the most important thing and it's fine." Severity is declared once in `criteria.toml`; it is never decided per-run.

## How It Runs

The plugin is dispatched by `bwoc audit run` (per `BWOC-12`):

```bash
bwoc audit run --plugin audit-iso-29110 --json
```

`bwoc audit` spawns `audit.sh` from this directory with these inputs:

| Channel | What it carries |
|---|---|
| `BWOC_WORKSPACE` (env) | Absolute path to the workspace root being audited. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory (where `criteria.toml` lives). |
| `BWOC_AUDIT_OPERATION` (env) | Operation name; v1 always `audit_run`. |
| stdin | `{"operation":"audit_run","workspace":"<abs>","plugin_dir":"<abs>"}` — the same context as the env vars. The script ignores stdin; env vars are the canonical channel. |

The script:

1. Reads `criteria.toml` from `BWOC_PLUGIN_DIR` (a constrained TOML shape — single-line scalars and arrays — parsed with `awk`).
2. For each criterion in declaration order, probes the `candidates` list against `BWOC_WORKSPACE`.
3. Emits one finding per criterion to stdout, in a JSON array, conforming to [PLUGINS.en.md §Audit Findings Schema](../../../docs/en/PLUGINS.en.md#audit-findings-schema):
   - First existing candidate → `status = "pass"`, `evidence = { kind: "file", value: <found path> }`, no `remedy`.
   - None exist → `status = "fail"`, `evidence = { kind: "file", value: <primary path> }`, `remedy = "Create <primary> … (or one of: <alts>)"`.

The script exits `0` on success. **Non-pass findings are findings, not errors.** A non-zero exit signals a framework-side problem (missing env var, unreadable `criteria.toml`) which the BWOC-12 dispatcher then treats as a plugin bug — see [PLUGINS.en.md line 59](../../../docs/en/PLUGINS.en.md#audit-findings-schema).

## Sample Output

For a workspace that has `docs/en/SRS.en.md` and `docs/en/ARCHITECTURE.en.md` but is missing the rest, the plugin emits something like:

```json
[
  {
    "criterion_id": "29110-bp-project-plan",
    "severity": "high",
    "status": "fail",
    "evidence": { "kind": "file", "value": "docs/en/PROJECT-PLAN.en.md" },
    "remedy": "Create docs/en/PROJECT-PLAN.en.md documenting the Project Plan work product (or one of: docs/PROJECT-PLAN.md, PROJECT-PLAN.md). Project plan capturing scope, schedule, resources, risks, and acceptance criteria."
  },
  {
    "criterion_id": "29110-bp-software-requirements-specification",
    "severity": "high",
    "status": "pass",
    "evidence": { "kind": "file", "value": "docs/en/SRS.en.md" }
  },
  {
    "criterion_id": "29110-bp-software-design",
    "severity": "medium",
    "status": "pass",
    "evidence": { "kind": "file", "value": "docs/en/ARCHITECTURE.en.md" }
  }
]
```

`bwoc audit run` wraps this into its canonical envelope `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }` — the dispatcher's responsibility, not the plugin's. Findings serialise in **criterion-declaration order**, which is `criteria.toml`'s row order (PLUGINS.en.md line 84).

## Configuration

```toml
# workspace.toml
[plugins.audit-iso-29110]
enabled = true
```

The plugin declares no `[config.schema]` in its manifest — the only workspace-level surface is the universal `enabled` key. A future version may add a `profile` key (Entry / Basic / Intermediate) once the other 29110 profiles are scoped in.

## Lifecycle Mapping

Per [PLUGINS.en.md §Lifecycle](../../../docs/en/PLUGINS.en.md#lifecycle), the `audit` kind's owner is the `bwoc audit` CLI; `init` and `teardown` happen per-invocation around `invoke`. This plugin holds **no external state** — every phase is trivially idempotent:

| Phase | What this plugin does |
|---|---|
| `init` | (Implicit per invocation; nothing to set up before `invoke`.) |
| `invoke` | Read `criteria.toml`, probe `BWOC_WORKSPACE`, emit findings JSON to stdout. |
| `teardown` | (Implicit per invocation; nothing to release.) |

A re-run with the same workspace produces the same findings array — `[file -e ...]` is read-only and order-stable.

## Maturity

Declared **L1** — first runnable audit plugin, six criteria, no real-world operator confirmation yet. Bumps to L2 once it has been exercised end-to-end against at least one BWOC operator's workspace; to L3 once an integration test in `crates/bwoc-cli/tests/` invokes it as a fixture.

## Neutrality

Manifest values name no LLM backend, vendor, or model. `kind = "audit"` is the framework's own enum (`BWOC-10`). The plugin references "ISO/IEC 29110" exclusively in `description` (where vendor-style names are tolerated per PLUGINS.en.md §Neutrality constraint) and in this SPEC's prose — never in `criteria.toml` keys, file paths, or finding `criterion_id` values beyond the standardised `29110-bp-*` namespace. Satisfies the **Samānattatā** rule.

## Status & Roadmap

- **v0.1.0** (this version): six Basic-profile work products, file-existence checks, bash entry.
- **v0.2.0** (planned): broaden coverage to the full Basic-profile Task Output table (~22 work products); add a `not_applicable` path for VSE profile variants (Entry vs Basic vs Intermediate).
- **v0.3.0** (planned): `evidence.kind = "content"` checks for required-section presence inside the work products (e.g. "SRS contains a § Non-functional requirements section").
- Out of scope for `BWOC-EPIC-2`: deeper conformance ("does the Project Plan reference the SRS?", "is the Verification Results record signed off?"). Those need the richer evidence model deferred to `BWOC-EPIC-3`.

## Sources

The Basic-profile work-product list is drawn from the following public references:

- ISO/IEC TR 29110-5-1-2:2011 — *Software engineering — Lifecycle profiles for Very Small Entities (VSEs) — Part 5-1-2: Management and engineering guide: Generic profile group: Basic profile.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/51153.html>. [^iso-29110-5-1-2]
- ISO/IEC 29110 — *Standards and guides for Very Small Entities (VSEs).* Public landing page: <https://www.iso.org/committee/4909141.html>.
- ISO/IEC 29110-4-1:2018 — *Profile specifications: Generic profile group.* ISO catalogue entry: <https://www.iso.org/standard/62711.html>.
- Laporte, C. Y., O'Connor, R. V., García Paucar, L. H. (2015). "The Implementation of ISO/IEC 29110 Software Engineering Standards and Guides in Very Small Entities." *Lecture Notes in Computer Science*, vol. 599. Useful operator-facing summary of the Basic-profile work products.

[^iso-29110-5-1-2]: The full Basic-profile Task Output table appears in ISO/IEC TR 29110-5-1-2 §8 ("Software Implementation") and §7 ("Project Management"). The six criteria here cover the headline work products that every Basic-profile project is expected to produce; the residual ~16 work products (Acceptance Record, Change Request, Correction Register, Software Configuration, etc.) are deferred to a v0.2.0 expansion.

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `audit` kind row (BWOC-10), Audit Findings Schema (BWOC-11).
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — EPIC-2 framing note (why `audit`, why 29110 first).
- [[../memory-tier2-noop/SPEC|memory-tier2-noop]] — sibling reference plugin (a different kind, same substrate).
- [[../../crates/bwoc-cli/src/audit|crates/bwoc-cli/src/audit.rs]] — the dispatcher that invokes this plugin.
