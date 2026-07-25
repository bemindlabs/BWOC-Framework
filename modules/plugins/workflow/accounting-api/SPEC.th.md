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

> [!abstract] plugin kind `workflow` เชื่อม **Bemind Accounting Open API** (v2.3.2, `https://accounting.bemind.tech/api/v1`) อ่าน **รายงาน** การเงิน และบันทึก **ซื้อ + ค่าใช้จ่าย**: สร้างแล้วเติมเอกสารซื้อ (flow 2 ขั้น `/purchase-docs` `POST → PATCH`) และลงค่าใช้จ่าย การเขียน **post GL double-entry** ฝั่ง server — purchase doc ตอน finalize (`bill-update`), expense ตอน create. auth ด้วย Bearer key (operator ใส่เอง ไม่ commit) + **ต้องมี** User-Agent header. `bwoc accounting` CLI ถือ write gate (standing opt-in `writes_enabled` + per-write confirm); plugin นี้ทำงานเมื่อถูก invoke

## Verbs

| Operation | ทิศทาง | Endpoint | Scope | ผลข้างเคียง |
|---|---|---|---|---|
| `report` | read | `GET /reports/<name>` | `reports:read` | ไม่มี — คืน JSON รายงาน |
| `bill-create` | **write** | `POST /purchase-docs` | `purchases:write` | สร้าง draft → `{document_id, number}` (GL post ตอน finalize) |
| `bill-update` | **write** | `PATCH /purchase-docs/{id}` | `purchases:write` | เติม/ปิดเอกสาร (date, supplier, items, vat) |
| `expense-create` | **write** | `POST /expenses` | `expenses:write` | ลงค่าใช้จ่าย auto-post GL |

ชื่อรายงาน: `pnl` · `balance-sheet` · `cashflow` · `trial-balance` · `vat` · `wht` · `ap-aging` · `ar-aging` · `expenses` · `sales-by-channel` · `mrr` · `product-margin` · `asset-register`

> [!warning] verb เขียนแก้ **system of record** ภายนอก + auto-post GL — ถาวร ย้อนยาก. gate อยู่ที่ `bwoc accounting` CLI (standing opt-in `writes_enabled` + per-write confirm) ไม่ใช่ที่ plugin. เรียก plugin ตรง ๆ = bypass gate; เขียนผ่าน `bwoc accounting` เท่านั้น

## วิธีทำงาน

framework เรียก `accounting.sh` ด้วย JSON บรรทัดเดียวทาง stdin. API key resolve ตอน runtime (env / secrets file) ไม่เคยพิมพ์ออก

```jsonc
{"operation":"report","report":"pnl","params":{"from":"2026-01-01","to":"2026-03-31"}}
{"operation":"bill-create","type":"bill"}
{"operation":"bill-update","document_id":"PI-123","payload":{"date":"2026-07-24","supplier":{"name":"ACME"},"items":[{"description":"widget","quantity":2,"unit":"ea","unitPrice":100}],"vat":7}}
{"operation":"expense-create","payload":{"date":"2026-07-24","description":"taxi","amount":150}}
```

flow บิลปกติ = 2 call: `bill-create` (ได้ `document_id`) → `bill-update` (เติม)

## Authentication & scope

key = personal API key ผูก **1 seller** resolve จาก **`BWOC_ACCOUNTING_KEY`** (env, ก่อน) หรือ **`<workspace>/.bwoc/secrets/accounting-key`** (file, gitignored, `chmod 600`). shape-only ใน `auth.toml`; **ไม่ commit ไม่พิมพ์**. scope ต่อ domain (`domain:write` ครอบ `:read`): `reports:read`, `purchases:write`, `expenses:write`. 403 บอก scope ที่ขาด. **ต้องมี User-Agent** (ไม่งั้น Cloudflare 1010) — plugin ส่ง UA ของตัวเองเสมอ

## รูปผลลัพธ์

ทุก response มี envelope `{ ok, plugin:"accounting-api", operation, … }`:

`report` → `{ ok, plugin, operation, report:<name>, data:<รายงาน> }` · `bill-create` → `{ ok, plugin, operation, document_id, number, type }` · `bill-update` → `{ ok, plugin, operation, document_id, number, status }` (ใบเสร็จ ไม่คืนเอกสารเต็ม) · `expense-create` → `{ ok, plugin, operation, expense_id, number }`

## Error classes

`0` สำเร็จ · `1` ไม่มี jq/curl · `2` usage/no-key · `3` auth/scope (401/403) · `4` 429 · `5` 404 · `6` transport

## Configuration

```toml
[plugins.accounting-api]
enabled = true
```

## Idempotency

`report` idempotent. `bill-create`/`expense-create` **ไม่** (สร้าง record ใหม่ทุกครั้ง); `bill-update` idempotent สำหรับ payload คงที่. gate (ที่ `bwoc accounting` CLI) มีเพราะเป็นการเขียนถาวร post-GL

## Maturity

L1 — reports + purchase-doc bill flow + expense ผ่าน `bwoc accounting` CLI พร้อม write gate (`writes_enabled` opt-in + per-write confirm). sales/cashbook/stock เป็น follow-up. อิงจาก OpenAPI จริง (v2.3.2)

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM/model/vendor นอกจาก accounting API — REST adapter บาง ๆ ตรวจสอบได้
