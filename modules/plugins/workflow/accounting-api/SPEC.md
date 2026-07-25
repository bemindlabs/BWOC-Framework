---
title: accounting-api — Bemind Accounting Open API
aliases:
  - accounting-api
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/accounting
maturity: L1
---

# accounting-api — Bemind Accounting Open API

> [!abstract] A `workflow`-kind plugin adapting the **Bemind Accounting Open API** (v2.3.2, `https://accounting.bemind.tech/api/v1`). Reads financial **reports** and records **purchases + expenses**: create then fill a purchase document (the 2-step `/purchase-docs` `POST → PATCH` bill flow) and post an expense. Writes **post a double-entry GL entry** server-side — a purchase doc on finalize (`bill-update`), an expense on create. Bearer-key auth (operator-supplied, never committed) + a **required** User-Agent header. The `bwoc accounting` CLI carries the write gate (a `writes_enabled` standing opt-in + a per-write operator confirm); this plugin executes when invoked.

## Verbs

| Operation | Direction | Endpoint | Scope | Side effect |
|---|---|---|---|---|
| `report` | read | `GET /reports/<name>` | `reports:read` | None — returns the report JSON. |
| `bill-create` | **write** | `POST /purchase-docs` | `purchases:write` | Creates a draft purchase doc → `{document_id, number}`. GL posts on finalize (`bill-update`). |
| `bill-update` | **write** | `PATCH /purchase-docs/{id}` | `purchases:write` | Fills/finalizes the doc (date, supplier, items, vat). |
| `expense-create` | **write** | `POST /expenses` | `expenses:write` | Records an expense. Auto-posts GL. |

Report names (`<name>`): `pnl` · `balance-sheet` · `cashflow` · `trial-balance` · `vat` · `wht` · `ap-aging` · `ar-aging` · `expenses` · `sales-by-channel` · `mrr` · `product-margin` · `asset-register`.

### Sales / stock / cashbook domains

| Operation | Direction | Endpoint | CLI |
|---|---|---|---|
| `sales-open-invoices` | read | `GET /sales/open-invoices` | `bwoc accounting sales open-invoices` |
| `quick-sales-list` / `quick-sales-show` | read | `GET /quick-sales[/{id}]` | `bwoc accounting sales quick list` / `show <id>` |
| `quick-sales-create` | **write** | `POST /quick-sales` | `bwoc accounting sales quick create --payload` |
| `quick-sales-convert` | **write** | `POST /quick-sales/{id}/convert-to-invoice` | `bwoc accounting sales quick convert <id>` |
| `stock-balance` / `stock-low` / `stock-movements` | read | `GET /stock/balance/{productId}` · `/stock/low` · `/stock/movements` | `bwoc accounting stock balance <id>` / `low` / `movements` |
| `stock-receipt` / `stock-adjust` | **write** | `POST /stock/receipts` · `/stock/adjustments` | `bwoc accounting stock receipt --payload` / `adjust --payload` |
| `gl-journals-list` / `gl-journal-show` | read | `GET /gl/journals[/{id}]` | `bwoc accounting cashbook journals` / `journal show <id>` |
| `gl-journal-create` | **write** | `POST /gl/journals` | `bwoc accounting cashbook journal create --payload` |

Every write in these domains is financial (GL-posting) and carries the same `bwoc accounting` gate as `bill`/`expense`. Reads are free.

> [!warning] The write verbs mutate an external **system of record** and auto-post GL — durable, hard-to-reverse. Their gate lives at the `bwoc accounting` CLI (PLUGINS §Write verbs) — a standing `writes_enabled` opt-in plus a per-write operator confirm — not this plugin. Invoking the plugin directly bypasses that gate; drive writes through `bwoc accounting`.

## How it runs

The framework invokes `accounting.sh` with a one-line JSON request on stdin. The API key resolves at runtime (env / secrets file) and is never printed.

| Channel | What it carries |
|---|---|
| `BWOC_WORKSPACE` (env) | Absolute workspace root (secrets-file resolution). |
| `BWOC_ACCOUNTING_KEY` (env) | The API key — **secret**. First-precedence source. |
| stdin | One-line JSON request — see the contract examples below. |

```jsonc
{"operation":"report","report":"pnl","params":{"from":"2026-01-01","to":"2026-03-31"}}
{"operation":"bill-create","type":"bill"}
{"operation":"bill-update","document_id":"PI-123","payload":{"date":"2026-07-24","supplier":{"name":"ACME"},"items":[{"description":"widget","quantity":2,"unit":"ea","unitPrice":100}],"vat":7}}
{"operation":"expense-create","payload":{"date":"2026-07-24","description":"taxi","amount":150}}
```

The typical bill flow is two calls: `bill-create` (get the `document_id`) → `bill-update` (fill it).

## Authentication & scope

The key is an operator personal API key **bound to one seller**, resolved from **`BWOC_ACCOUNTING_KEY`** (env, first) or **`<workspace>/.bwoc/secrets/accounting-key`** (file, gitignored, `chmod 600`). Shape-only in `auth.toml`; **never committed, never printed**. Scopes are per domain (`domain:write` covers `:read`, `*` = all): `reports:read`, `purchases:write`, `expenses:write`. A 403 names the scope gap.

A **`User-Agent` header is required** — the edge (Cloudflare) returns error 1010 without one. The plugin always sends its own UA.

## Output shapes

Every response carries the envelope `{ ok, plugin:"accounting-api", operation, … }`:

- `report` → `{ ok, plugin, operation:"report", report:<name>, data:<report JSON> }`.
- `bill-create` → `{ ok, plugin, operation:"bill-create", document_id, number, type }`.
- `bill-update` → `{ ok, plugin, operation:"bill-update", document_id, number, status }` (a write receipt — never the full doc).
- `expense-create` → `{ ok, plugin, operation:"expense-create", expense_id, number }`.

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout. |
| `1` | dependency | `jq` or `curl` missing. |
| `2` | usage / no-key | Unknown/missing operation, bad report name, missing `document_id`/`payload`, invalid id, or no resolvable key. |
| `3` | auth / scope | HTTP 401 (key invalid) or 403 (scope gap). |
| `4` | rate-limited | HTTP 429. |
| `5` | not-found | HTTP 404. |
| `6` | transport / unexpected | Network failure or an unmapped HTTP status. |

## Configuration

```toml
# workspace.toml
[plugins.accounting-api]
enabled = true
```

No plugin-local config — the only surface is `enabled`. Credentials come from env / the secrets file.

## Idempotency

`report` is idempotent. `bill-create` / `expense-create` are **not** (each call creates a new record); `bill-update` is idempotent for a fixed payload. The operator-confirm gate (at the future CLI) exists because these are durable GL-posting writes.

## Maturity

L1 — reports + purchase-doc bill flow + expense + the **sales / stock / cashbook** domains (quick-sales, stock balances/movements/receipts/adjustments, GL journals), all fronted by the `bwoc accounting` CLI with the write gate (`writes_enabled` opt-in + per-write confirm). Grounded in the live OpenAPI (v2.3.2).

## Neutrality

Backend-neutral: no LLM, no model, no vendor beyond the accounting API itself. A thin, auditable REST adapter.
