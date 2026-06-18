# 2026-06-18 — README trim + CONTRIBUTING cleanup (task #13)

A Mattaññutā/Yoniso-Manasikāra pass over the two front-door docs: cut stale and
duplicative prose, fix dangling references. Conservative on scope — the
doctrinal identity sections (the 22 frameworks, the stack diagram, the five
principles) are the project's signature, not bloat, and were left fully intact.

## What changed

**README.md**
- **Status section condensed.** It pinned "latest release v2026.6.1-0 (2.19.0),
  2026-06-01" while HEAD is **2.31.0 / v2026.6.15-0** — stale by a dozen
  releases. Dropped the version-by-version "In development since 2.19.0" +
  "Recent — v2.8.0/v2.7.0/v2.6.0" history (that's CHANGELOG's job, and it was
  wrong), and folded the current-phase paragraph down to Phases 1–5 done +
  Phase 6 in progress, pointing at ROADMAP for per-phase detail.
- Unpinned the C4 diagram title (`Container view (v2.6.0)` → `Container view`)
  so it can't go stale again.
- Fixed `bwoc-framwork/` → `bwoc-framework/` in the repo-layout tree.
- Added the two existing sections (Environment Variables, Infrastructure &
  Datastores) to the table of contents — they were rendered but unlisted.

**CONTRIBUTING.md**
- Fixed the broken Discussions URL — it pointed at a non-existent
  `github.com/bmt-bwol-ops/bwoc-framwork` (wrong org + typo) → corrected to
  `github.com/bemindlabs/BWOC-Framework`.
- Removed the dangling `[CODEOWNERS](.github/CODEOWNERS)` link. The framework
  CLAUDE.md explicitly records that file as "referenced but not present" — the
  link 404'd. Kept the surrounding sentence.

## Decisions

- **Fix staleness > shrink line count.** README only dropped 507→499 lines, but
  the point was truthfulness (Yoniso Manasikāra): a front door that advertises a
  release twelve versions old misleads. Removing a wrong claim beats removing a
  long one.
- **Left the framework-identity sections alone.** Three representations of the
  22 frameworks (the "why" table, the A–F grouping, the stack diagram) is
  arguably redundant, but each serves a different reader and they are the
  project's doctrinal core — trimming them is a separate, opinionated call, not
  a cleanup. Mattaññutā cuts bloat, not identity.

## Status / deferred

- Did not touch the triple "no Docker / no DB / no ports" overlap across the
  Architecture intro, Infrastructure & Datastores, and Tech Stack footer — a
  real redundancy, but consolidating it rewrites three sections and risks the
  C4/datastore detail. Left for a dedicated pass if desired.

## Related

- task #13; `README.md`, `CONTRIBUTING.md`; CLAUDE.md (the CODEOWNERS-absent note).
