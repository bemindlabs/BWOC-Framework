# 2026-07-25 — accounting sales / stock / cashbook domains

Extends `bwoc accounting` + the `accounting-api` plugin with the three remaining
accounting domains, grounded in the live Accounting Open API (fetched, not
recalled). Closes gap #4 of the "do it all" set.

## What changed

- **Plugin** `modules/plugins/workflow/accounting-api/accounting.sh` — 12 new
  operations behind three DRY helpers (`_do_get`, `_do_post_payload`,
  `_do_post_empty`) + a generic `_valid_path_id`: sales (`sales-open-invoices`,
  `quick-sales-{list,show,create,convert}`), stock (`stock-{balance,low,movements,
  receipt,adjust}`), cashbook (`gl-journals-list`, `gl-journal-{show,create}`).
- **CLI** `crates/bwoc-cli/src/accounting.rs` — `Sales` / `Stock` / `Cashbook`
  subcommand trees + four shared arg structs (`ReadArgs`, `IdReadArgs`,
  `PayloadWriteArgs`, `IdWriteArgs`) driven by four generic runners (`run_read`,
  `run_id_read`, `run_payload_write`, `run_id_write`). Writes reuse the existing
  `financial_write_gate`; a local `is_valid_path_id` pre-check mirrors the plugin.
- SPEC.md/th (a Domains table + updated maturity) + manifest description.
- 4 new e2e tests; the stub plugin gained the new op branches.

## Decisions

- **Cashbook = GL journals.** The API has no literal `/cashbook`; the cashbook
  concept (the record of cash/bank movements) maps to `/gl/journals` (the
  double-entry journal) + the existing `report cashflow`. Named the subcommand
  `cashbook` (the domain the user asked for) over the journal endpoint.
- **Every domain write is financial → the same gate.** quick-sales create,
  stock receipt/adjust, and a manual GL journal all post to the live books, so
  they get the identical `writes_enabled` + per-write-confirm gate as
  bill/expense — no weaker path. Reads (`open-invoices`, `low`, `movements`,
  `journals`, …) are free.
- **Generic runners over per-verb code.** 12 verbs would be ~400 lines of
  duplicated resolve→gate→require→dispatch. Four runners keyed by shape
  (read / id-read / payload-write / id-write) collapse it and keep every verb's
  behaviour identical (Mattaññutā).
- **Grounded in the fetched OpenAPI, not memory.** Pulled
  `https://accounting.bemind.tech/api/v1/openapi.json` (77 paths) and used the
  real endpoints/methods (`/quick-sales/{id}/convert-to-invoice`, `/stock/receipts`,
  `/gl/journals`, …) — Yoniso Manasikāra, not recalled shapes.

## Status / deferred

- Shipped 2.42.0 (`v2026.7.25-4`). Gap #4 done.
- Deferred (from the "do it all" set): #6 accounting-api plugin *enable* on the
  fleets (needs the operator seller key at `.bwoc/secrets/accounting-key`);
  #7 kla.life gateway prod deploy (awaiting explicit confirmation); #8 per-host
  keyring / daemon auto-drain (design-heavy). #5 (audit lanes) + #6-soul done.

## Related

- `crates/bwoc-cli/src/accounting.rs`, `modules/plugins/workflow/accounting-api/`
- `notes/2026-07-24_accounting-api-plugin.md`, `notes/2026-07-25_accounting-cli-write-gate.md`
