# 2026-07-22 — gws-docs: Google Docs read + in-place write (first gws write path)

Closes #354. Adds `gws-docs`, the first **write-capable** `gws` service: reads a Google Doc (`documents.get`) and edits it in place via `documents.batchUpdate` (the general write verb) plus the convenience `replace-all-text`. Introduces the gws kind's first use of the operator-confirm gate. First of a doc/sheet/slide series — Sheets and Slides follow as separate PRs (one concern per PR).

## What changed

- **New plugin** `modules/plugins/gws/gws-docs/`: `manifest.toml`, `gws.sh` (verbs `get` / `batch-update` / `replace-all-text`), `SPEC.md` + `SPEC.th.md`. Sources the sibling `gws-auth` helpers (Bearer + refresh + rate-limit live once); scope `https://www.googleapis.com/auth/documents` (read+write). Reads project into the normative Google Doc shape; writes return a receipt (never the new body).
- **`crates/bwoc-cli/src/gws.rs`**: `docs get|batch-update|replace-all-text` command tree, request builders, handlers, render. The **write-verb operator-confirm gate** (`run_write_verb` + `confirm` + `json_write_blocked`) — the gws kind's first — mirroring jira: default No, interactive `y/N`, `--yes` for headless, `--json` requires `--yes`, refused write → `EXIT_USAGE` reporting "no change". Module-doc verb table + writes section updated.
- **`crates/bwoc-cli/src/check.rs`**: `GwsService::Docs` + Google Doc resource shape (`document_id` / `title` / `revision_id` required, `web_view_link` optional) for the optional captured-`resources/` audit.
- **Docs (EN+TH parity)**: `docs/{en,th}/PLUGINS`: Google Doc resource shape + the gws-kind description now says "read-mostly for Drive/Gmail/Calendar; gws-docs adds the first write path".
- **Tests**: 6 new (arg parsing incl the requests ArgGroup, `resolve_docs_requests` array validation, request builders, `json_write_blocked` gate). `cargo test -p bwoc-cli` = 800. jq projections smoke-tested offline.

## Decisions

- **Gate at the CLI, not the plugin** (PLUGINS §Write verbs): one confirmation point per write; the plugin executes when invoked. `gws.sh` never re-implements nor bypasses the gate.
- **`batch-update` is the general write path**; `replace-all-text` is sugar over a single `replaceAllText` request. Higher-level structured verbs deferred — `batch-update` already exposes the full Docs write surface (Mattaññutā).
- **`--requests` (inline JSON) XOR `--requests-file`** via a clap ArgGroup(required); the CLI validates non-empty-array before spawning.
- **Write receipt, not resource entry**: writes return `{document_id, revision_id, requests_applied, occurrences_changed, replies}` — reports what changed, never echoes the document body (Adinnādāna: minimal surface).
- **One controller identity / one scope**: `documents` (read+write) rather than splitting readonly; a `documents.readonly` token still works for `get` and 403s on write with the scope named.

## Alternatives considered

- Separate `documents.readonly` + `documents` scopes — rejected; one read+write scope is simpler and the 403 path already names the gap.
- Building `batch-update` requests from high-level flags — rejected for the first slice; raw `requests[]` is the honest, complete surface.

## Status / deferred

- Shipped `gws-docs` (this PR). **Sheets** (`gws-sheets`, `spreadsheets`) and **Slides** (`gws-slides`, `presentations`) are the next two PRs, same template + gate.
- **Secondary (#354): canonical plugin install source** — the reference gws plugins ship in-tree under `modules/plugins/gws/`; a resolvable git-URL/tarball registry (BWOC-75/76) is a separate infra item, still deferred.

## Related (links)

- Issue #354; `modules/plugins/gws/gws-docs/`; `crates/bwoc-cli/src/gws.rs` (`run_write_verb`), `check.rs` (`GwsService::Docs`).
- Pattern reuse: `gws-drive` (read template), `jira` (write-confirm gate).
