# 2026-07-22 — gws-sheets: Google Sheets read + values write

Second slice of the gws editor-app series (after `gws-docs`). Adds `gws-sheets`: reads spreadsheet metadata (`spreadsheets.get`) and cell ranges (`spreadsheets.values.get`), and writes values via `spreadsheets.values.update` / `.append`. Reuses the write-verb operator-confirm gate introduced with `gws-docs`. Slides (`gws-slides`) follows as the next PR.

## What changed

- **New plugin** `modules/plugins/gws/gws-sheets/`: `manifest.toml`, `gws.sh` (verbs `get` / `values-get` / `values-update` / `values-append`), `SPEC.md` + `SPEC.th.md`. Sources `gws-auth`; scope `https://www.googleapis.com/auth/spreadsheets`. Writes use `valueInputOption=USER_ENTERED`; append uses `insertDataOption=INSERT_ROWS`.
- **`gws.rs`**: `sheets` command tree + request builders + handlers + render. Read verbs via `run_read_verb`; write verbs via the shared `run_write_verb` gate. New `is_valid_range` (A1 notation, no `/`) + `resolve_sheet_values` (2-D array validation). Module-doc verb table + writes section updated.
- **`check.rs`**: `GwsService::Sheets` + Google Spreadsheet resource shape (`spreadsheet_id` / `title` required, `sheet_count` / `web_view_link` optional).
- **Docs (EN+TH parity)**: `docs/{en,th}/PLUGINS`: Google Spreadsheet resource shape + kind description updated to include `gws-sheets`.
- **Tests**: 5 new (read/write arg parsing incl the values ArgGroup, A1-range validation, `resolve_sheet_values` 2-D check, request builders). `cargo test -p bwoc-cli` = 805. jq projections (get, values-get, update/append receipts) + range validation smoke-tested offline.

## Decisions

- **`values-*` over `spreadsheets.batchUpdate`**: cell values are the common data path; structural edits (add sheet, format, charts) via `batchUpdate` deferred (Mattaññutā).
- **`USER_ENTERED`** value-input (Sheets parses types/formulas as if typed) rather than `RAW` — matches operator expectations when writing "=SUM(...)" or numbers.
- **A1 range is URL-encoded in the plugin** (`jq @uri`) before it enters the request path; the CLI pre-validates the charset (no `/`, no control) so a crafted range can't open a path/query segment.
- **`values-append` is not idempotent** (adds rows each call) — the confirm gate applies to both writes; the prompt names Append vs Overwrite + the row count.

## Status / deferred

- Shipped `gws-sheets` (this PR). **`gws-slides`** (`presentations`, `presentations.batchUpdate`) is the next and final slice of the series.
- `spreadsheets.batchUpdate` (structural) deferred.

## Related (links)

- Series root: #354 / `gws-docs` (`2026-07-22_gws-docs-plugin.md`).
- `modules/plugins/gws/gws-sheets/`; `gws.rs` (`run_sheets_*`, `is_valid_range`, `resolve_sheet_values`), `check.rs` (`GwsService::Sheets`).
