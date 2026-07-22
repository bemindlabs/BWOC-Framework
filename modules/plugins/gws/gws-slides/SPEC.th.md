---
title: gws-slides — Google Slides (อ่าน + เขียนแก้ในที่)
aliases:
  - gws-slides
tags:
  - group/framework-plugins
  - type/plugin
  - kind/gws
  - domain/integration
  - integration/google-workspace
maturity: L1
---

# gws-slides — Google Slides (อ่าน + เขียนแก้ในที่)

> [!abstract] plugin ต่อบริการของ kind `gws` — adapter Google Slides ที่เขียนได้ อ่าน presentation (`get`, `presentations.get`) และแก้ผ่าน `batch-update` (`presentations.batchUpdate` — verb เขียนทั่วไป) และตัวช่วย `replace-all-text` การอ่าน project เป็น [[../../../docs/th/PLUGINS.th#Workspace Resource Schema|รูป Google Presentation ที่เป็นบรรทัดฐาน]] verb เขียนมี [[../../../docs/th/PLUGINS.th#Write verbs — the operator-confirm gate (normative)|operator-confirm gate]] ที่ขอบ CLI `bwoc gws slides` ดึงจากฐาน [[../gws-auth/SPEC|`gws-auth`]] ต้องใช้ scope `presentations`

## Verbs

| Operation | ทิศทาง | Slides endpoint | ผลข้างเคียง |
|---|---|---|---|
| `get` | read | `GET /v1/presentations/{id}` | ไม่มี — title + จำนวน/id สไลด์ |
| `batch-update` | **write** | `POST /v1/presentations/{id}:batchUpdate` | **ถาวร** — apply `requests[]` ของผู้เรียก (มี gate) |
| `replace-all-text` | **write** | `POST …:batchUpdate` (`replaceAllText` เดียว) | **ถาวร** — ตัวช่วย (มี gate) |

> [!warning] verb เขียนแก้ presentation จริง มี operator-confirm gate ที่ `bwoc gws slides …`: interactive `y/N` (เริ่มต้น **No**); agent headless ส่ง `--yes`; `--json` ต้องมี `--yes` ตัว plugin execute เมื่อถูกเรียก — gate อยู่ที่ CLI

## วิธีทำงาน

CLI เรียก `gws.sh` ด้วย JSON บรรทัดเดียวทาง stdin (`BWOC_GWS_OPERATION` / `BWOC_WORKSPACE` / `BWOC_PLUGIN_DIR` / `BWOC_GWS_TOKEN` ใน env) สัญญา channel เดียวกับ plugin `gws-*` พี่น้อง

```jsonc
{"operation":"get","presentation_id":"1AbC"}
{"operation":"batch-update","presentation_id":"1AbC","requests":[{"createSlide":{}}]}
{"operation":"replace-all-text","presentation_id":"1AbC","find":"{{title}}","replace":"Q3 Review","match_case":false}
```

## Authentication & scope

credential resolve ผ่าน `gws-auth` ต้องใช้ `https://www.googleapis.com/auth/presentations` (อ่าน+เขียน); token `presentations.readonly` `get` ได้แต่เขียนไม่ได้ (เขียนจะ 403 บอก scope ที่ขาด)

## รูปผลลัพธ์

`get` → `{ presentation: { presentation_id, title, slide_count, web_view_link }, slide_ids: [ … ] }`
`batch-update` / `replace-all-text` (ใบเสร็จ) → `{ presentation_id, requests_applied, occurrences_changed, replies: [ … ] }` ใบเสร็จรายงานว่าอะไรเปลี่ยน ไม่ echo เนื้อสไลด์ใหม่

## Error classes

taxonomy เดียวกับ plugin gws พี่น้อง: `0` สำเร็จ · `1` ไม่มี `jq`/`curl` · `2` usage/no-token (op ผิด, `presentation_id` ขาด/ผิด, `requests` ขาด/ว่าง/ไม่ใช่ array, `find` ขาด) · `3` auth/scope (401/403; token read-only เขียนไม่ได้) · `4` 429 · `5` 404 · `6` transport/unexpected

## Configuration

```toml
[plugins.gws-slides]
enabled = true
```

ไม่มี config เฉพาะ plugin — surface เดียวคือ `enabled` credential มาจาก `gws-auth`

## Idempotency

`get` idempotent `batch-update` / `replace-all-text` **ไม่** idempotent — รันซ้ำ apply requests อีกครั้ง gate มีเพราะเป็นการเขียนถาวร

## Maturity

L1 — `get` + `batch-update` + `replace-all-text` verb ตัวช่วยระดับสูง (แก้ต่อ shape, templating layout) เลื่อนไว้; `batch-update` ทั่วไปเปิด surface เขียนของ Slides ทั้งหมด

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM/model/vendor นอกจาก Google Slides — REST adapter บาง ๆ ตรวจสอบได้
