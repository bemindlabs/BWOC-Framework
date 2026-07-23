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

> [!abstract] A `workflow`-kind plugin adapting the **Bemind Accounting Open API** (v2.3.2, `https://accounting.bemind.tech/api/v1`). Reads financial **reports** and records **purchases + expenses**: create then fill a purchase document (the 2-step `/purchase-docs` `POST → PATCH` bill flow) and post an expense. Every write **auto-posts a double-entry GL entry** server-side. Bearer-key auth (operator-supplied, never committed) + a **required** User-Agent header. This is the first slice; the `bwoc accounting` CLI (which carries the write-verb operator-confirm gate) is a follow-up.

## Verbs

| Operation | Direction | Endpoint | Scope | Side effect |
|---|---|---|---|---|
| `report` | read | `GET /reports/<name>` | `reports:read` | None — returns the report JSON. |
| `bill-create` | **write** | `POST /purchase-docs` | `purchases:write` | Creates a draft purchase doc → `{id, number}`. GL-posting on finalize. |
| `bill-update` | **write** | `PATCH /purchase-docs/{id}` | `purchases:write` | Fills/finalizes the doc (date, supplier, items, vat). |
| `expense-create` | **write** | `POST /expenses` | `expenses:write` | Records an expense. Auto-posts GL. |

Report names (`<name>`): `pnl` · `balance-sheet` · `cashflow` · `trial-balance` · `vat` · `wht` · `ap-aging` · `ar-aging` · `expenses` · `sales-by-channel` · `mrr` · `product-margin` · `asset-register`.

> [!warning] The write verbs mutate an external **system of record** and auto-post GL — durable, hard-to-reverse. Their operator-confirm gate belongs at the `bwoc accounting` CLI (PLUGINS §Write verbs), a follow-up slice — not this plugin. Until that CLI ships, invoke the write verbs deliberately.

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

- `report` → `{ ok, plugin, operation:"report", report:<name>, data:<report JSON> }`.
- `bill-create` → `{ ok, operation:"bill-create", document_id, number, type }`.
- `bill-update` → `{ ok, operation:"bill-update", document_id, number, status }` (a write receipt — never the full doc).
- `expense-create` → `{ ok, operation:"expense-create", expense_id, number }`.

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

L1 — first slice: reports + purchase-doc bill flow + expense. Sales / cashbook / stock domains, and the `bwoc accounting` CLI with the write-confirm gate, are follow-up slices. Grounded in the live OpenAPI (v2.3.2).

## Neutrality

Backend-neutral: no LLM, no model, no vendor beyond the accounting API itself. A thin, auditable REST adapter.
