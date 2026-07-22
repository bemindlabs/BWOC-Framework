---
title: gws-sheets — Google Sheets (อ่าน + เขียนค่า)
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

# gws-sheets — Google Sheets (อ่าน + เขียนค่า)

> [!abstract] plugin ต่อบริการของ kind `gws` — adapter Google Sheets ที่เขียนได้ อ่าน metadata (`get`, `spreadsheets.get`) และช่วงเซลล์ (`values-get`, `spreadsheets.values.get`) และแก้ค่าผ่าน `values-update` (`spreadsheets.values.update`) / `values-append` (`spreadsheets.values.append`) การอ่าน project เป็น [[../../../docs/th/PLUGINS.th#Workspace Resource Schema|รูป Google Spreadsheet ที่เป็นบรรทัดฐาน]] verb เขียนมี [[../../../docs/th/PLUGINS.th#Write verbs — the operator-confirm gate (normative)|operator-confirm gate]] ที่ขอบ CLI `bwoc gws sheets` ดึงจากฐาน [[../gws-auth/SPEC|`gws-auth`]] ต้องใช้ scope `spreadsheets`

## Verbs

| Operation | ทิศทาง | Sheets endpoint | ผลข้างเคียง |
|---|---|---|---|
| `get` | read | `GET /v4/spreadsheets/{id}` | ไม่มี — title + รายการ tab |
| `values-get` | read | `GET …/values/{range}` | ไม่มี — grid ค่า |
| `values-update` | **write** | `PUT …/values/{range}` | **ถาวร** — เขียนทับช่วง (มี gate) |
| `values-append` | **write** | `POST …/values/{range}:append` | **ถาวร** — เพิ่มแถวต่อท้าย (มี gate) |

> [!warning] `values-update` / `values-append` แก้ spreadsheet จริง มี operator-confirm gate ที่คำสั่ง `bwoc gws sheets …`: interactive `y/N` (เริ่มต้น **No**); agent headless ส่ง `--yes`; `--json` ต้องมี `--yes` การเขียนใช้ `valueInputOption=USER_ENTERED` (Sheets parse ชนิด/สูตรเหมือนพิมพ์เอง)

## วิธีทำงาน

CLI (`bwoc gws sheets …`) เรียก `gws.sh` ด้วย JSON บรรทัดเดียวทาง stdin (`BWOC_GWS_OPERATION` / `BWOC_WORKSPACE` / `BWOC_PLUGIN_DIR` / `BWOC_GWS_TOKEN` ใน env) สัญญา channel เดียวกับ plugin `gws-*` พี่น้อง

```jsonc
{"operation":"get","spreadsheet_id":"1AbC"}
{"operation":"values-get","spreadsheet_id":"1AbC","range":"Sheet1!A1:B2"}
{"operation":"values-update","spreadsheet_id":"1AbC","range":"Sheet1!A1","values":[["x","y"]]}
{"operation":"values-append","spreadsheet_id":"1AbC","range":"Sheet1!A1","values":[["x","y"]]}
```

## Authentication & scope

credential resolve ผ่าน `gws-auth` ต้องใช้ `https://www.googleapis.com/auth/spreadsheets` (อ่าน+เขียน); token `spreadsheets.readonly` `get` / `values-get` ได้แต่เขียนไม่ได้ (เขียนจะ 403 บอก scope ที่ขาด)

## รูปผลลัพธ์

`get` → `{ spreadsheet: { spreadsheet_id, title, sheet_count, web_view_link }, sheets: [ { sheet_id, title, index } ] }`
`values-get` → `{ spreadsheet_id, range, major_dimension, values: [[…]] }`
`values-update` / `values-append` (ใบเสร็จ) → `{ spreadsheet_id, updated_range, updated_rows, updated_columns, updated_cells }` ใบเสร็จรายงานว่าอะไรเปลี่ยน ไม่ echo ทั้งชีต

## Error classes

taxonomy เดียวกับ plugin gws พี่น้อง: `0` สำเร็จ · `1` ไม่มี `jq`/`curl` · `2` usage/no-token (op ผิด, `spreadsheet_id`/`range` ขาด/ผิด, `values` ไม่ใช่ 2-D) · `3` auth/scope (401/403; token read-only เขียนไม่ได้) · `4` 429 · `5` 404 · `6` transport/unexpected

## Configuration

```toml
[plugins.gws-sheets]
enabled = true
```

ไม่มี config เฉพาะ plugin — surface เดียวคือ `enabled` credential มาจาก `gws-auth`

## Idempotency

`get` / `values-get` idempotent `values-update` idempotent สำหรับ range+values คงที่; `values-append` **ไม่** (แต่ละครั้งเพิ่มแถว) gate มีเพราะทั้งคู่เป็นการเขียนถาวร

## Maturity

L1 — `get` + `values-get` + `values-update` + `values-append` การแก้เชิงโครงสร้าง (`spreadsheets.batchUpdate` — เพิ่มชีต, จัดรูปแบบ, ชาร์ต) เลื่อนไว้; values ครอบเส้นทางข้อมูลทั่วไป

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM/model/vendor นอกจาก Google Sheets — REST adapter บาง ๆ ตรวจสอบได้
