# 2026-07-24 — audit kind → ISO / IEC / IEEE (add 29148 lane)

Generalises the `audit` plugin kind from "ISO" framing to the full **ISO / IEC / IEEE** standards family, and adds the first jointly-**ISO/IEC/IEEE** lane: `audit-iso-iec-ieee-29148` (Requirements Engineering).

## What changed

- **New plugin** `modules/plugins/audit-iso-iec-ieee-29148/` (kind `audit`, L1): `manifest.toml`, `criteria.toml` (7 criteria — StRS / SyRS-SRS / requirement + set characteristics / verifiability / traceability / requirements-management, clause-referenced to 29148:2018), `audit.sh` (reuses the shared attestation runtime of the ISO 9001 / ISO/IEC 27001 lanes), `SPEC.md` + `SPEC.th.md`.
- **Framing (EN+TH parity)**: `docs/{en,th}/PLUGINS` — the `audit` kinds-table row + the kind rationale now say the kind spans **ISO / IEC / IEEE** (ISO 9001 · ISO/IEC 20000-1/27001/29110 · ISO/IEC/IEEE 29148), with the accurate joint designations (20000-1, 27001 are ISO/IEC).

## Decisions

- **The runtime was already body-agnostic** — a criterion id / standard designation / clause reference is data the `audit` runtime reads, not a constraint (Rust has no "iso-only" check; `criterion_id` is kebab-case). So "support IEC/IEEE" is (a) making the *framing* explicit + accurate, and (b) shipping a concrete jointly-published standard to prove it.
- **29148 is the right first IEEE lane** — ISO, IEC, *and* IEEE co-publish it as the canonical requirements-engineering standard for systems + software (it supersedes IEEE 830 / 1233 / 1362), and requirements engineering is directly relevant to what BWOC agents produce.
- **Reuse, not reinvent** — the 29148 runtime is the ISO-9001 attestation runtime, name-parameterised; the shared attestation evidence vocabulary holds across the whole kind.
- **Did not rename the `audit-iso-*` dirs** — the historical prefix stays (renames break references); the joint standards are named accurately in descriptions + docs instead.

## Status / deferred

- L1 attestation runtime over the 7 criteria. Deeper checks (parsing a real SRS / traceability matrix from the workspace) are a future slice, matching the 9001/27001 roadmap. More IEEE/IEC lanes (e.g. ISO/IEC/IEEE 12207 life-cycle, IEEE 1012 V&V) can follow the same template.

## Related (links)

- `modules/plugins/audit-iso-iec-ieee-29148/`; kin `modules/plugins/audit-iso-{9001,27001,20000-1,29110}/`; `docs/en/PLUGINS.en.md` §Plugin Kinds.
