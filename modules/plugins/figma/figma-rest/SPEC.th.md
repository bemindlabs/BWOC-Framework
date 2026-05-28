---
title: อะแดปเตอร์ Figma REST
aliases:
  - figma-rest
tags:
  - group/framework-plugins
  - type/plugin
  - kind/figma
  - domain/integration
  - integration/figma
maturity: L1
---

# อะแดปเตอร์ Figma REST

> [!abstract] ปลั๊กอินอ้างอิงของชนิด `figma` — ชนิดที่แปด และเป็นชนิดสุดท้ายของโรดแมปชุดแรก เป็นแบบ **อ่านเป็นหลัก (read-mostly)**: อ่านเมทาดาทาของโหนด Figma, ส่งออก (export) ภาพของโหนดลงในแคชภายในแบบ content-addressable และดึง design token จากสไตล์ของโหนด มัน **ไม่เคยเขียนกลับไปยัง Figma** ถูกเรียกใช้โดย CLI `bwoc figma` (`BWOC-63`) ซึ่งเป็นเจ้าของการแจง argument, ด่านตรวจ auth และการ resolve config ส่วนสคริปต์นี้เป็นเจ้าของชั้น HTTP รายการแมป (mapping entry) เป็นไปตาม [[../../../docs/en/PLUGINS.en#Figma Asset Mapping Schema|Figma Asset Mapping Schema]] (`BWOC-62`)

## ทำไมต้องเป็นชนิด `figma` ไม่ใช่ปลั๊กอิน `workflow`

การผสานรวม `gcloud` (`EPIC-8`) เลือก *ใช้ซ้ำ* ชนิด `workflow` เพราะมันไม่มีสคีมาเชิงบรรทัดฐาน — มันเพียง shell ไปยัง CLI `gcloud` ในเครื่องแล้วส่งต่อ JSON ที่ได้กลับมา แต่ `figma` ต่างออกไป: มันมี **Figma Asset Mapping Schema** เชิงบรรทัดฐาน — ความสัมพันธ์ design→dev ที่ BWOC เป็นเจ้าของและคงทน ที่ผูกโหนด Figma เข้ากับ artifact ที่ส่งออก + design token ซึ่งถูกบริโภคโดย `bwoc check` (`BWOC-65`), แดชบอร์ด และการอ้างอิง token ในเอกสารสเปก สคีมาเชิงบรรทัดฐานนี้เองที่ทำให้ `figma` สมควรเป็นชนิดของตัวเอง เช่นเดียวกับที่ Issue Mapping Schema ทำให้ `jira` ได้เป็นชนิดของตัวเอง เหตุผลฉบับเต็มอยู่ใน [[../../../notes/2026-05-28_figma-plugin-architecture|บันทึกออกแบบ BWOC-61]] §1

`figma` ต่างจาก `jira` ในประเด็นสำคัญหนึ่ง: มันเป็นแบบ **อ่านเป็นหลัก** `jira` อ่านและเขียนระบบบันทึกภายนอก (transition ที่มีด่านกั้น, sync ledger) ส่วน `figma` เพียงอ่าน Figma และเขียน **ภายในเครื่อง** (ภาพที่ส่งออก) มันรับเอาวินัยเรื่อง *สคีมา* ของ jira มา แต่ไม่รับกลไก *การเขียน* ของมันเลย — ไม่มี sync ledger, ไม่มีด่านยืนยันจากผู้ปฏิบัติงาน (operator-confirm), ไม่มีนโยบายแก้ความขัดแย้ง เพราะไม่มีการเขียนกลับไปภายนอกให้ต้องกั้น

## คำกริยา (Verbs)

| ปฏิบัติการ | ทิศทาง | Auth | HTTP | การเขียนในเครื่อง |
|---|---|---|---|---|
| `fetch` | อ่าน | จำเป็น | `GET /v1/files/<key>/nodes?ids=<csv>` | ไม่มี |
| `export` | อ่าน + **เขียนในเครื่อง** | จำเป็น | `GET /v1/files/<key>/nodes` (เวอร์ชัน) แล้ว `GET /v1/images/<key>?ids=<node>` | ภาพที่เรนเดอร์ ลงในแคช |
| `tokens` | อ่าน | จำเป็น | `GET /v1/files/<key>/nodes?ids=<csv>` | ไม่มี |

การอ่านทั้งสามถูกจำกัดขอบเขตด้วยชุด node-id ที่ร้องขอ — อะแดปเตอร์ไม่เคยเดินไล่ทั้งไฟล์แบบไร้ขอบเขต `export` เป็นคำกริยาเดียวที่เขียน และเขียนเฉพาะแคชภายในเครื่องของ workspace เท่านั้น (ไม่เคยเขียนไปยัง Figma)

## วิธีการทำงาน

CLI จะ spawn `figma.sh` จากไดเรกทอรีนี้ (สะท้อนวิธีที่ `bwoc audit`/`bwoc jira` เรียกปลั๊กอินของตน):

| ช่องทาง | สิ่งที่ส่งผ่าน |
|---|---|
| `BWOC_FIGMA_OPERATION` (env) | `fetch` \| `export` \| `tokens` — สำรองเมื่อ stdin `.operation` ว่าง |
| `BWOC_WORKSPACE` (env) | รากของ workspace (absolute); แคช export อยู่ใต้พาธนี้ |
| `BWOC_PLUGIN_DIR` (env) | พาธสัมบูรณ์ของไดเรกทอรีปลั๊กอินนี้ (เพื่อทราบข้อมูล) |
| `BWOC_FIGMA_TOKEN` (env) | personal access token — **ความลับ** (ดู [§การยืนยันตัวตน](#การยืนยันตัวตน)) |
| stdin | คำขอ JSON บรรทัดเดียว เช่น `{"operation":"fetch","file_key":"AbC123","node_ids":["1:2"]}` |

เมื่อสำเร็จ สคริปต์ออกด้วยรหัส `0` และส่ง **อ็อบเจกต์ JSON หนึ่งชิ้น** ออกทาง stdout ให้ CLI แจง เมื่อผิดพลาด มันเขียนคำวินิจฉัยที่มนุษย์อ่านได้ลง **stderr** แล้วออกด้วยรหัสไม่เป็นศูนย์ ซึ่ง CLI จะแสดงเป็น `plugin '<name>' exited <code>` รูปแบบผลลัพธ์:

- `fetch` → `{ "ok": true, "file_key", "assets": [ <Asset Mapping entry>, … ] }`
- `tokens` → เหมือน `fetch` โดยแต่ละรายการมีอ็อบเจกต์ `design_tokens` (ละไว้เมื่อไม่มี token ที่ดึงได้)
- `export` → `{ "ok": true, "cached": <bool>, "asset": <Asset Mapping entry ที่มี exported_path> }`

## การยืนยันตัวตน

Figma ยืนยันตัวตนการเรียก REST ด้วย **personal access token (PAT)** ผ่านเฮดเดอร์ `X-Figma-Token: <PAT>` token resolve จาก (ตัวแรกที่เจอชนะ):

| แหล่ง | ที่ใด | บทบาท |
|---|---|---|
| `BWOC_FIGMA_TOKEN` | environment (แนะนำ — ไม่แตะดิสก์) | PAT — **ความลับ** |
| `.bwoc/secrets.toml` `[figma] token` | gitignored, เจ้าของเท่านั้น (`chmod 600`) | PAT — สำรองบนดิสก์สำหรับการเรียกด้วยมือ |

`figma.sh` อ่าน token ตรงเข้าเฮดเดอร์ `X-Figma-Token` ของ curl — ไม่เคย echo, ไม่เคยเขียนลงไฟล์ และไม่เคยใส่ลงในผลลัพธ์ JSON หรือรายการ Asset Mapping ใด ๆ (**อทินนาทาน** ที่ทุกขอบเขต) ตัวสำรองบนดิสก์จะถูก **ปฏิเสธ** หากไฟล์ secrets อ่านได้โดย group/world หาก token ขาดหาย อะแดปเตอร์จะล้มเหลวทันทีพร้อมคำวินิจฉัย `auth_missing` ที่ชัดเจน

> [!danger] `auth.toml` บรรจุเพียง SHAPE — ไม่เคยมีค่าจริง [[auth|auth.toml]] ประกาศชื่อ env var, พาธ secrets และ scope ที่จำเป็น โดยมี placeholder `token` ที่ **ว่างเปล่า** ไม่มี credential ใดถูก commit การหมุน token ที่ถูกเพิกถอนเป็นเรื่อง config ล้วน (อัปเดต env var / ไฟล์ secrets) `bwoc check` (`BWOC-65`) fail-closed บนฟิลด์ใด ๆ ที่ดูเหมือนมีค่าใน `auth.toml` เช่นเดียวกับด่านของ jira (`BWOC-45`) และ gcloud (`BWOC-55`)

### Scope

PAT ของ Figma มี scope แบบ **เข้าถึงไฟล์ส่วนตัว vs ไลบรารีทีม** token ที่ scope ไปยังไฟล์ของผู้ใช้เองอ่านไลบรารีทีมไม่ได้ และในทางกลับกัน `403` จะถูกแสดงเป็น "token lacks the required scope for `<resource>`" — ระบุช่องว่างที่ขาด ไม่ใช่ความล้มเหลวเปล่า ๆ คำกริยาอ่านต้องการ scope `file_content` ส่วนการอ่านไลบรารีทีม (คำกริยาในอนาคต) ต้องการ `library_content` ดู [[auth|auth.toml]] `[figma.auth.scopes]`

## การจำกัดอัตรา & คลาสของข้อผิดพลาด

Figma REST ใช้การจำกัดอัตราต่อทีม และคืน `429 Too Many Requests` พร้อมเฮดเดอร์ `Retry-After` `figma.sh` แยกแยะคลาสข้อผิดพลาดตามที่ [[../../../notes/2026-05-28_figma-plugin-architecture|บันทึก BWOC-61]] §4 ระบุไว้:

| HTTP | คลาส | พฤติกรรมของอะแดปเตอร์ |
|---|---|---|
| `2xx` | สำเร็จ | แจง + project body |
| `429` | ลองใหม่ได้ | เคารพ `Retry-After` (สำรองด้วยกำลังสองหากไม่มีเฮดเดอร์); สูงสุด 4 ครั้ง แล้วแจ้งข้อผิดพลาดที่ลองใหม่ได้ |
| `401` | auth ร้ายแรง | token ขาด/หมดอายุ/ถูกเพิกถอน — "หมุน `BWOC_FIGMA_TOKEN`" |
| `403` | scope ขาด | "token lacks the required scope for `<resource>`" — ไฟล์ vs ไลบรารีทีมไม่ตรงกัน |
| `404` | ไม่พบ | `file_key` / `node_id` ผิด; แจ้งผู้ปฏิบัติงาน |
| อื่น ๆ / transport | ข้อผิดพลาด | รายงานพร้อม body ที่ตัดสั้น; ออกด้วยรหัสไม่เป็นศูนย์ |

เพราะการเขียนเพียงอย่างเดียวคือภายในเครื่อง + content-addressable (ดู [§การแคช export](#การแคช-export)) การลองใหม่จึงเป็น idempotent

## การใช้ Asset Mapping Schema

`fetch` project แต่ละโหนดเป็นรูปทรง [[../../../docs/en/PLUGINS.en#Figma Asset Mapping Schema|Asset Mapping Schema]] เชิงบรรทัดฐาน — `file_key`, `node_id`, `name`, `type`, `last_modified` `tokens` เพิ่มอ็อบเจกต์ `design_tokens` `export` เพิ่ม `exported_path` (และเมื่อเรนเดอร์ใหม่ จะมี `image_url` ที่ไม่คงทน) `file_key` + `node_id` เป็นกุญแจคงทนเพียงคู่เดียว ฟิลด์อื่นทุกตัวเป็น projection ที่เปลี่ยนแปลงได้ของสถานะ Figma หรือผลการ export ในเครื่อง รีเฟรชทุกครั้งที่เรียก ฟิลด์ที่เป็น optional จะ **ละไว้** เมื่อไม่มี ไม่เคย serialize เป็น `null`

### การดึง token

`tokens` เดินไล่สมบัติสไตล์ของแต่ละโหนดเป็นอ็อบเจกต์ `{ name: value }` — `fills` แบบ solid → ค่า hex `color/fill/<n>`, `cornerRadius` → `radius/corner`, `style` ของข้อความ → `type/font-*` และ `type/line-height`, `itemSpacing` → `spacing/item`, stroke → `border/width` นี่คือสะพาน design→spec ส่วนการทำงานอัตโนมัติที่ลึกขึ้น "ผูก token เข้ากับบรรทัดในเอกสารสเปกแล้วตรวจการคลาดเคลื่อน" ถูกเลื่อนออกไป (สคีมาบรรจุ token ไว้แล้ว ส่วนเครื่องมือเชื่อมโยงตามมาทีหลังได้)

## การแคช export

การ export เป็นแบบ **content-addressable** ชื่อไฟล์แคชคือ `SHA-256(file_key + node_id + version + format)` ภายใต้ `export_dir` ที่กำหนด (ค่าตั้งต้น `figma/exports/`) โดย `version` คือเวอร์ชันปัจจุบันของไฟล์ (สัญญาณ invalidate แคช):

- การ export ซ้ำของโหนดที่ **ไม่เปลี่ยน** (เวอร์ชันเดิม) เป็น **cache hit** — ไฟล์ที่เรนเดอร์มีอยู่แล้ว จึงข้ามการเรนเดอร์ภาพ + ดาวน์โหลดที่หนักและถูกจำกัดอัตรา การ resolve เวอร์ชันปัจจุบันเป็นการอ่านเมทาดาทาที่ถูกเพียงครั้งเดียว เมื่อผู้เรียกถือเวอร์ชัน + เมทาดาทาของโหนดอยู่แล้ว (จาก `fetch` ก่อนหน้า) มันส่งค่าเหล่านั้นมา และ cache hit จะเป็นปฏิบัติการ **ไม่เรียก API เลย (zero-API)**
- โหนดที่ **เปลี่ยน** (เวอร์ชันใหม่) จะ hash เป็นชื่อไฟล์ใหม่ จึงไม่เคยเสิร์ฟภาพเก่าหลังการอัปเดตดีไซน์

สิ่งนี้ทำให้ `bwoc figma export` รันซ้ำได้ถูก และทำให้แคชปลอดภัยที่จะลบ (มันสร้างใหม่ได้) — `figma/exports/` ถูกเพิ่มลงในเทมเพลต gitignore ของ workspace (`BWOC-64`) การ commit ภาพเรนเดอร์แบบไบนารีจะทำให้ repo บวมโดยไม่มีประโยชน์ที่คงทน

## การตั้งค่า

```toml
# workspace.toml
[plugins.figma-rest]
enabled    = true
export_dir = "figma/exports"   # ไม่บังคับ — ที่ตั้งของแคช content-addressable
```

credential **ไม่ใช่** config — token resolve จาก env `BWOC_FIGMA_TOKEN` / `.bwoc/secrets.toml` คีย์ `[config.schema]` ที่ประกาศไว้มีเพียง `export_dir`

## การแมป Lifecycle

ตาม [[../../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]] เจ้าของชนิด `figma` คือ CLI `bwoc figma` ส่วน `init`/`teardown` เป็นแบบต่อการเรียกรอบ `invoke` อะแดปเตอร์ไม่ถือสถานะภายในเครื่องนอกจากแคช content-addressable (สร้างใหม่ได้, gitignored)

| เฟส | สิ่งที่อะแดปเตอร์นี้ทำ |
|---|---|
| `init` | โดยปริยายต่อการเรียก; ตรวจการมีอยู่ของ token |
| `invoke` | อ่านคำขอ, เรียก Figma REST, ส่ง JSON (และสำหรับ `export` เขียนภาพที่แคชไว้) |
| `teardown` | โดยปริยาย; ลบเฉพาะไฟล์ header/body ชั่วคราว |

## Idempotency

- `fetch` และ `tokens` เป็นอ่านอย่างเดียวและลำดับเสถียร
- `export` เป็น content-addressable: คู่ `(file_key, node_id, version, format)` เดิมแมปไปยังไฟล์แคชเดิมเสมอ การเล่นซ้ำหลัง backoff จาก `429` จึงลู่เข้าสู่ artifact เดิม — ไม่มีการดาวน์โหลดซ้ำซ้อน

## ระดับวุฒิภาวะ (Maturity)

ประกาศเป็น **L1** — อะแดปเตอร์ `figma` ที่รันได้ตัวแรก; เส้นทางอ่าน + export ในเครื่องใช้งานได้ จะขยับเป็น L2 เมื่อทดสอบ end-to-end กับไฟล์ Figma จริงด้วย PAT ที่ผู้ปฏิบัติงานให้มา

> [!warning] ช่องว่างการทดสอบจริง การตรวจสอบ end-to-end กับไฟล์ Figma จริง (fetch โหนด, export ภาพ, ดึง token) ถูกกั้นด้วย **PAT ที่ผู้ปฏิบัติงานให้มา** — ด่าน credential ภายนอกเดียวกับที่ jira (`EPIC-6`) และ gcloud (`EPIC-8`) มี v0.1.0 ตรวจสอบด้วย: `bash -n figma.sh`, เส้นทาง auth-missing / unknown-operation ที่ผิดพลาดอย่างสะอาด และ `bwoc check` ยอมรับ manifest `figma` ส่วนการ round-trip REST จริงยังไม่ได้ตรวจสอบจนกว่าจะมี token

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือโมเดลใด `kind = "figma"` เป็นค่า enum ของเฟรมเวิร์กเอง (`BWOC-62`) คำว่า "Figma" ปรากฏเฉพาะใน `description` (ที่ซึ่งชื่อเป้าหมายการผสานรวมเป็นที่ยอมรับได้ตาม [[../../../docs/en/PLUGINS.en#Neutrality constraint (HARD)|PLUGINS.en.md §Neutrality]]) และในเนื้อความของ SPEC นี้ — ไม่เคยอยู่ใน `kind`, `entry` หรือคีย์ config ตรงตาม **สมานัตตตา**

## ดูเพิ่มเติม

- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — สเปกปลั๊กอิน; ชนิด `figma` + Figma Asset Mapping Schema (BWOC-62)
- [[../../../notes/2026-05-28_figma-plugin-architecture|2026-05-28_figma-plugin-architecture.md]] — บันทึกกรอบงาน EPIC-7 (own-kind, auth, rate-limit, การแคช export, สคีมา)
- [[auth|auth.toml]] — สัญญา auth (shape เท่านั้น; ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (สมดุลสองภาษา)
