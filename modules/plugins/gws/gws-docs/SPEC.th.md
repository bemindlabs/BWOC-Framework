---
title: gws-docs — Google Docs (อ่าน + เขียนแก้ในที่)
aliases:
  - gws-docs
tags:
  - group/framework-plugins
  - type/plugin
  - kind/gws
  - domain/integration
  - integration/google-workspace
maturity: L1
---

# gws-docs — Google Docs (อ่าน + เขียนแก้ในที่)

> [!abstract] plugin ต่อบริการของ kind `gws` (`BWOC-354`) — เป็น **บริการ `gws` ตัวแรกที่เขียนได้**. อ่าน **Google Doc** (`get`, Docs `documents.get`) และแก้ไขในที่ผ่าน `batch-update` (`documents.batchUpdate` — verb เขียนแบบทั่วไป) และตัวช่วย `replace-all-text`. การอ่าน project เป็น [[../../../docs/th/PLUGINS.th#Workspace Resource Schema|รูป Google Doc ที่เป็นบรรทัดฐาน]]. verb เขียนของมันมี [[../../../docs/th/PLUGINS.th#Write verbs — the operator-confirm gate (normative)|operator-confirm gate]] ที่ขอบเขต CLI `bwoc gws docs`. ดึง credential helper จากฐาน [[../gws-auth/SPEC|`gws-auth`]] จึงไม่มีโค้ด auth ของตัวเอง. ต้องใช้ scope `documents`. กรอบเต็ม: [[../../../notes/2026-05-28_google-workspace-plugin-architecture|BWOC-72 design note]].

## Verbs

| Operation | ทิศทาง | Docs endpoint | ผลข้างเคียง |
|---|---|---|---|
| `get` | read | `GET /v1/documents/{documentId}` (`documents.get`) | ไม่มี — metadata + ดึงข้อความ body แบบมีขอบเขต |
| `batch-update` | **write** | `POST /v1/documents/{documentId}:batchUpdate` | **ถาวร** — apply `requests[]` ของผู้เรียก (เส้นทางเขียน Docs ทั่วไป) มี gate |
| `replace-all-text` | **write** | `POST …:batchUpdate` (`replaceAllText` เดียว) | **ถาวร** — ตัวช่วยครอบ `replaceAllText` เดียว มี gate |

> [!warning] verb เขียน (`batch-update`, `replace-all-text`) แก้เอกสารจริงแบบย้อนกลับไม่ได้ จึงมี operator-confirm gate ที่คำสั่ง `bwoc gws docs …`: operator แบบ interactive ตอบ `y/N` (ค่าเริ่มต้น **No**); agent แบบ headless ต้องส่ง `--yes` และเฉพาะเมื่อ operator อนุมัติการแก้นั้นจริง. `--json` ต้องมี `--yes`. ตัว plugin เอง execute เมื่อถูกเรียก — gate อยู่เหนือขึ้นไปที่ CLI

## วิธีทำงาน

CLI (`bwoc gws docs …`) ค้น plugin ที่ enabled ตัวนี้ ใช้ confirm gate สำหรับ verb เขียน แล้วเรียก `gws.sh` ด้วย JSON บรรทัดเดียวทาง stdin:

| Channel | บรรจุอะไร |
|---|---|
| `BWOC_GWS_OPERATION` (env) | `get` \| `batch-update` \| `replace-all-text` — fallback ของ `.operation` เมื่อ stdin ว่าง |
| `BWOC_WORKSPACE` (env) | workspace root absolute (resolve token file ผ่าน sibling) |
| `BWOC_PLUGIN_DIR` (env) | path absolute ของ plugin นี้ — ใช้หา `../gws-auth/gws.sh` |
| `BWOC_GWS_TOKEN` (env) | OAuth2 access token — **ความลับ** ใช้โดย sibling helper |
| stdin | JSON request บรรทัดเดียว — ดูตัวอย่างสัญญาด้านล่าง |

```jsonc
{"operation":"get","document_id":"1AbC_dEf"}
{"operation":"batch-update","document_id":"1AbC_dEf","requests":[{"insertText":{"location":{"index":1},"text":"Hello"}}]}
{"operation":"replace-all-text","document_id":"1AbC_dEf","find":"March 31","replace":"In stock","match_case":false}
```

## Authentication & scope

Credential resolve ผ่านฐาน `gws-auth` (`BWOC_GWS_TOKEN` env / `<workspace>/.bwoc/secrets/gws-token.json`) ไม่ใช่จาก workspace config. ต้องใช้ scope `https://www.googleapis.com/auth/documents` — ต่างจากบริการ read-mostly ตรงที่นี่คือ scope **อ่าน+เขียน** ของ Docs. token ที่ consent แค่ `documents.readonly` จะ `get` ได้แต่เขียนไม่ได้; 403 ตอนเขียนจะบอก scope ที่ขาด

## รูปผลลัพธ์

### `get`

```json
{ "ok": true, "plugin": "gws-docs", "operation": "get",
  "document": { "document_id": "1AbC_dEf", "title": "Q3 Plan",
                "revision_id": "ALm37…", "web_view_link": "https://docs.google.com/document/d/1AbC_dEf/edit" },
  "text": "Q3 Plan\n…", "text_truncated": false }
```

### `batch-update` / `replace-all-text` (ใบเสร็จการเขียน)

```json
{ "ok": true, "plugin": "gws-docs", "operation": "replace-all-text",
  "document_id": "1AbC_dEf", "revision_id": "ALm38…",
  "requests_applied": 1, "occurrences_changed": 3, "replies": [ … ] }
```

ใบเสร็จการเขียนไม่ echo body เอกสารกลับ — รายงานว่าอะไรเปลี่ยน ไม่ใช่เนื้อหาใหม่

## Error classes

| Exit | Class | ความหมาย |
|---|---|---|
| `0` | success | JSON object เดียวบน stdout |
| `1` | dependency | ไม่มี `jq` หรือ `curl` ใน PATH |
| `2` | usage / no-token | operation ผิด/ขาด, ขาด `.document_id`, id ไม่ถูก, `.requests` ขาด/ว่าง/ไม่ใช่ array, ขาด `.find`, หรือไม่มี token |
| `3` | auth / scope | HTTP 401 (token ใช้ไม่ได้) หรือ 403 (ขาด scope `documents`; token read-only เขียนไม่ได้) |
| `4` | rate-limited | HTTP 429 หลังหมด budget backoff |
| `5` | not-found | HTTP 404 (ไม่มีเอกสาร) |
| `6` | transport / unexpected | network ล้ม หรือ HTTP status ที่ไม่ได้ map |

## Configuration

```toml
# workspace.toml
[plugins.gws-docs]
enabled = true
```

ไม่มี config เฉพาะ plugin — surface เดียวคือ `enabled`. credential มาจาก `gws-auth`

## Lifecycle mapping

| Phase | ทำอะไร |
|---|---|
| `init` | โดยปริยายต่อการเรียก; ตรวจ `jq` + `curl` ใน PATH และ sibling helper มีอยู่ |
| `invoke` | อ่าน request; สำหรับการเขียน CLI ยืนยันไว้แล้ว เรียก Docs ผ่าน `gws_curl` ของ sibling; project การอ่านเป็น Doc entry หรือการเขียนเป็นใบเสร็จ |
| `teardown` | โดยปริยาย; ไม่มี state ต้องปล่อย |

## Idempotency

`get` idempotent. `batch-update` และ `replace-all-text` **ไม่** idempotent โดยเนื้อแท้ — รันซ้ำ apply requests อีกครั้ง (เช่น `insertText` แทรกสองครั้ง). operator-confirm gate มีอยู่ก็เพราะสิ่งเหล่านี้เป็นการเขียนถาวรที่ไม่ idempotent

## Maturity

L1 — slice แรก: `get` + `batch-update` + `replace-all-text`. verb ตัวช่วยระดับสูง (แก้ตารางแบบมีโครงสร้าง, update named-range) เลื่อนไว้โดยตั้งใจ; `batch-update` ทั่วไปเปิด surface เขียนของ Docs ทั้งหมดแล้ว

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM, ไม่มี model, ไม่มี vendor นอกจาก Google Docs เอง. plugin เป็น REST adapter บาง ๆ ตรวจสอบได้
