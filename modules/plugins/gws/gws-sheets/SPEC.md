---
title: gws-sheets — Google Sheets (Read + Values Write)
aliases:
  - gws-sheets
tags:
  - group/framework-plugins
  - type/plugin
  - kind/gws
  - domain/integration
  - integration/google-workspace
maturity: L1
---

# gws-sheets — Google Sheets (Read + Values Write)

> [!abstract] A per-service plugin of the `gws` kind — a write-capable Google Sheets adapter. Reads spreadsheet metadata (`get`, `spreadsheets.get`) and cell ranges (`values-get`, `spreadsheets.values.get`), and edits values via `values-update` (`spreadsheets.values.update`) / `values-append` (`spreadsheets.values.append`). Reads project into the normative [[../../../docs/en/PLUGINS.en#Workspace Resource Schema|Google Spreadsheet shape]]. Its write verbs carry the [[../../../docs/en/PLUGINS.en#Write verbs — the operator-confirm gate (normative)|operator-confirm gate]] at the `bwoc gws sheets` CLI boundary. Sources the [[../gws-auth/SPEC|`gws-auth`]] foundation. Requires the `spreadsheets` scope.

## Verbs

| Operation | Direction | Sheets endpoint | Side effect |
|---|---|---|---|
| `get` | read | `GET /v4/spreadsheets/{id}` | None — title + tab list. |
| `values-get` | read | `GET …/values/{range}` | None — a value grid. |
| `values-update` | **write** | `PUT …/values/{range}` | **Durable** — overwrites the range (gated). |
| `values-append` | **write** | `POST …/values/{range}:append` | **Durable** — appends rows after the range (gated). |

> [!warning] `values-update` / `values-append` mutate a live spreadsheet. They carry the operator-confirm gate at the `bwoc gws sheets …` command: interactive `y/N` (default **No**); headless agents pass `--yes`; `--json` requires `--yes`. Writes use `valueInputOption=USER_ENTERED` (Sheets parses types/formulas as if typed).

## How it runs

The CLI (`bwoc gws sheets …`) invokes `gws.sh` with a one-line JSON request on stdin (`BWOC_GWS_OPERATION` / `BWOC_WORKSPACE` / `BWOC_PLUGIN_DIR` / `BWOC_GWS_TOKEN` in env), same channel contract as the sibling `gws-*` plugins.

```jsonc
{"operation":"get","spreadsheet_id":"1AbC"}
{"operation":"values-get","spreadsheet_id":"1AbC","range":"Sheet1!A1:B2"}
{"operation":"values-update","spreadsheet_id":"1AbC","range":"Sheet1!A1","values":[["x","y"]]}
{"operation":"values-append","spreadsheet_id":"1AbC","range":"Sheet1!A1","values":[["x","y"]]}
```

## Authentication & scope

Credentials resolve through `gws-auth`. Requires `https://www.googleapis.com/auth/spreadsheets` (read+write); a `spreadsheets.readonly` token can `get` / `values-get` but not write (a write 403s naming the scope gap).

## Output shapes

`get` → `{ spreadsheet: { spreadsheet_id, title, sheet_count, web_view_link }, sheets: [ { sheet_id, title, index } ] }`.
`values-get` → `{ spreadsheet_id, range, major_dimension, values: [[…]] }`.
`values-update` / `values-append` (write receipt) → `{ spreadsheet_id, updated_range, updated_rows, updated_columns, updated_cells }`. The receipt reports what changed, never echoes the whole sheet.

## Error classes

Same exit taxonomy as the sibling gws plugins: `0` success · `1` missing `jq`/`curl` · `2` usage / no-token (unknown op, missing/invalid `spreadsheet_id` or `range`, non-2-D `values`) · `3` auth/scope (401/403; a read-only token cannot write) · `4` 429 · `5` 404 · `6` transport/unexpected.

## Configuration

```toml
[plugins.gws-sheets]
enabled = true
```

No plugin-local config — the only surface is `enabled`. Credentials come from `gws-auth`.

## Idempotency

`get` / `values-get` are idempotent. `values-update` is idempotent for a fixed range+values; `values-append` is **not** (each call adds rows). The operator-confirm gate exists because both are durable writes.

## Maturity

L1 — `get` + `values-get` + `values-update` + `values-append`. Structural edits (`spreadsheets.batchUpdate` — add sheet, format, charts) are deferred; values cover the common data path.

## Neutrality

Backend-neutral: no LLM, no model, no vendor beyond Google Sheets. A thin, auditable REST adapter.
