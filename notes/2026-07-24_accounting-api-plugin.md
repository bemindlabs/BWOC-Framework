# 2026-07-24 — `accounting-api` workflow plugin (first slice)

Adds a `workflow`-kind plugin (`modules/plugins/workflow/accounting-api/`) adapting the **Bemind Accounting Open API** (v2.3.2). First slice: read reports + record purchases/expenses. Deferred earlier (during the skill batch) as "belongs as a plugin, not a skill" — now built. Enables an agent (e.g. the accountant, kaew) to record bills/expenses + read financials.

## What changed

- New `modules/plugins/workflow/accounting-api/`: `manifest.toml`, `accounting.sh`, `SPEC.md` + `SPEC.th.md`, `auth.toml` (shape-only).
- Verbs: `report` (read, `GET /reports/<name>`), `bill-create` (`POST /purchase-docs {type}` → draft), `bill-update` (`PATCH /purchase-docs/{id} {payload}`), `expense-create` (`POST /expenses {payload}`). Every write auto-posts a double-entry GL entry server-side.
- Auth: operator API key (bound to one seller) from `BWOC_ACCOUNTING_KEY` env / `.bwoc/secrets/accounting-key` (gitignored) — never committed, never printed. A **User-Agent header is required** (Cloudflare 1010 without it).

## Decisions / grounding

- **Grounded in the live OpenAPI, not memory.** Pulled the app's `openapi.json` from the server (Accounting Open API v2.3.2, 77 paths) and confirmed the real endpoints — the fleet memory's `/purchase-docs` 2-step flow was correct, and I verified the report-name set (`pnl`/`balance-sheet`/`cashflow`/`trial-balance`/`vat`/`wht`/`ap-aging`/`ar-aging`/`expenses`/`sales-by-channel`/`mrr`/`product-margin`/`asset-register`) against it. (Yoniso Manasikāra.)
- **workflow-kind, not a new kind.** External-system integration → the `workflow` kind (like the `gcloud-*` family). A dedicated `bwoc accounting` CLI + the write-verb operator-confirm gate is the **next slice** (mirrors how `gws`/`gcloud` ship the plugin + the gated CLI separately). Until then the plugin executes writes when invoked; the SPEC flags this.
- **Report-name allowlist + id charset guard** — a crafted `report`/`document_id` can never inject a path segment (validated against the OpenAPI's report set / `[A-Za-z0-9_-]`).
- **Write receipt, not the full doc** — writes return `{id, number, status}`, never echo the record back.
- **No key to test end-to-end** (none in the fleet secrets), so verified the shell offline: header/URL construction, report-name validation (bash word-split), the 2-step payload building, and envelope (`{ok, data}`) projection with mocked responses. The operator supplies the key at runtime (same as `gws` needs `BWOC_GWS_TOKEN`).

## Status / deferred

- Shipped the plugin (reports + purchase-doc bill flow + expense). **Deferred:** the `bwoc accounting` CLI with the write-confirm gate; the sales / cashbook / stock domains; a possible normative BWOC schema (would argue for promoting to an `accounting` own-kind).

## Related (links)

- `modules/plugins/workflow/accounting-api/`; pattern kin: `modules/plugins/workflow/gcloud-*`, `modules/plugins/gws/`.
- Source of truth: the app's OpenAPI (`~/apps/accounting`, v2.3.2) on the bemind server.
