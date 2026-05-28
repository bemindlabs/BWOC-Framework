---
title: workspace-okrs — ตัวติดตาม Objectives + Key Results
aliases:
  - workspace-okrs
tags:
  - group/framework-plugins
  - type/plugin
  - kind/okr
  - domain/reporting
maturity: L1
---

# workspace-okrs — ตัวติดตาม Objectives + Key Results

> [!abstract] ปลั๊กอินอ้างอิงสาย `okr` สำหรับ `BWOC-EPIC-4` ติดตาม Objectives + Key Results ที่ผู้ดูแลเขียนเองในไฟล์ TOML สองไฟล์ในเครื่อง ([[objectives|objectives.toml]] + [[key_results|key_results.toml]]) แล้วปล่อยรายงานความคืบหน้าแบบ normative คำสั่ง: `track` (อัปเดตค่า `current` ของ key result — เป็นการเขียนเพียงอย่างเดียว และแตะเฉพาะไฟล์ในเครื่องของผู้ดูแลเอง จึง **ไม่มี** ด่านยืนยัน), `check-progress` (สถานะต่อ KR + สรุปยอดต่อ objective), `report` (JSON ตาม [[../../../docs/th/PLUGINS.th#สคีมา OKR Progress|OKR Progress Schema]] ฉบับเต็ม) **อยู่ในเครื่องล้วน ๆ** — ไม่มีเครือข่าย ไม่มี credential ไม่มี system of record ภายนอก เหตุผลฉบับเต็ม: [[../../../notes/2026-05-28_okr-plugin-architecture|บันทึกออกแบบ BWOC-46]]

## ทำไมต้อง kind `okr`

`okr` เป็น kind สาย **reporting** ตัวที่สามของเฟรมเวิร์ก เคียงข้าง `audit` ในขณะที่ `audit` ตรวจ *workspace* เทียบมาตรฐานภายนอกแล้วปล่อย findings, `okr` ติดตาม *เป้าหมายที่ผู้ดูแลเขียนเอง* แล้วปล่อยความคืบหน้า มันไม่ใช่ kind `workflow`: ไม่เอื้อมไปยังระบบภายนอก ไม่ถือ credential และการเขียนเพียงอย่างเดียวของมันแตะ TOML ในเครื่องของผู้ดูแลเอง — ไม่ใช่ system of record — จึงไม่มีด่านยืนยันจากผู้ดูแล การนำ [[../../../docs/th/PLUGINS.th#Evidence kinds|Evidence kinds]] ของ audit กลับมาใช้ (แทนที่จะคิด kind เฉพาะ OKR ขึ้นใหม่) ทำให้มี vocabulary ของ evidence เพียงชุดเดียวทั่วทั้งเฟรมเวิร์ก เหตุผลฉบับเต็ม: บันทึกออกแบบ decisions 1, 3, 4

## รูปร่างข้อมูล

ไฟล์ที่ผู้ดูแลเขียนเองสองไฟล์ส่งมาในไดเรกทอรีนี้เป็นตัวอย่าง seed ในตัว (objective ของ `BWOC-EPIC-4` ที่ติดตามการส่งมอบของตัวเอง):

### `objectives.toml` — ตัว Objectives

| ฟิลด์ | ชนิด | จำเป็น | ความหมาย |
|---|---|---|---|
| `objective_id` | string | ใช่ | id ที่เสถียร; ถูกอ้างโดย `key_results.objective_id` |
| `title` | string | ใช่ | คำกล่าวหนึ่งบรรทัดของ objective |
| `owner` | string | ใช่ | ใครเป็นเจ้าของ (agent id หรือบุคคล) |
| `period` | string | ใช่ | ช่วงเวลาที่ครอบคลุม เช่น `2026-Q2` |
| `parent` | string | ไม่ | `objective_id` แม่สำหรับต้นไม้ objective (`""` เมื่ออยู่ระดับบนสุด; การ rollup หลายชั้นถูกเลื่อนออกไป — บันทึก §Status) |

### `key_results.toml` — ตัว Key Results

| ฟิลด์ | ชนิด | จำเป็น | ความหมาย |
|---|---|---|---|
| `key_result_id` | string | ใช่ | id ที่เสถียร ไม่ซ้ำภายในปลั๊กอิน |
| `objective_id` | string | ใช่ | **เชิงอ้างอิง** — ต้อง resolve ไปยัง id ใน `objectives.toml` (บังคับโดย `bwoc check`, BWOC-50) |
| `description` | string | ใช่ | สิ่งที่ key result วัด |
| `target` | number | ใช่ | ค่าเป้าหมาย |
| `current` | number | ใช่ | ค่าล่าสุดที่ติดตาม (เขียนโดย `track`) |
| `unit` | enum | ใช่ | `count` \| `percent` \| `currency` \| `ratio` \| `boolean` (boolean ใช้ `0`/`1`) |
| `confidence` | enum | ใช่ | `high` \| `medium` \| `low` — การอ่านเชิงคุณภาพว่าเส้นทางจะคงอยู่หรือไม่ |
| `evidence` | inline table | ใช่ | `{ kind, value }` — นำ Evidence kinds ของ audit กลับมาใช้ (`file` \| `content` \| `command` \| `attestation` \| `sample` \| `none`) |
| `as_of` | string | ไม่ | วันที่ ISO-8601 ที่ `current` ถูกติดตามล่าสุด; ละไว้เมื่อยังไม่เคยติดตาม |

## คำสั่ง (Verbs)

| คำสั่ง | อินพุต | เอาต์พุต | ผลข้างเคียง |
|---|---|---|---|
| `track` | `--key-result <id> --current <value> [--evidence <kind:value>]` | KR ที่อัปเดตเป็น progress entry หนึ่งรายการ | เขียน `current` (+ `evidence`, + `as_of`) กลับไปยัง `key_results.toml` **เขียนไฟล์ในเครื่องเท่านั้น** — ไม่มีด่าน (บันทึก §3) |
| `check-progress` | — | สถานะต่อ KR + สรุปยอดต่อ objective | ไม่มี — อ่านอย่างเดียว |
| `report` | — | JSON array ตาม OKR Progress Schema ฉบับเต็มสำหรับทุก KR | ไม่มี — อ่านอย่างเดียว |

### ฮิวริสติกของ `check-progress`

`attainment = current / target` (สำหรับ `boolean`, `current ≥ target → 1` ไม่งั้น `0`; `target` ที่เป็น `0` ถือว่าบรรลุ) จากนั้นสถานะคือ:

- **on-track** — `attainment ≥ 0.7` **หรือ** `confidence == high`
- **at-risk** — ไม่งั้น เมื่อ `confidence == medium`
- **off-track** — ไม่งั้น (`confidence == low`)

เส้น `0.7` คือเกณฑ์ "เขียว" ตามแบบแผน OKR ฮิวริสติกตั้งใจให้เรียบง่ายและมีเอกสารกำกับ (บันทึก §3): attainment ถือสัญญาณเชิงปริมาณ ส่วน `confidence` ถือสัญญาณเชิงคุณภาพ โมเดล "attainment ที่คาดหวังตามสัดส่วนเวลาที่ผ่านไป" แบบอิงเวลาถูก **เลื่อนออกไป** — v1 ใช้เส้นคงที่ สถานะต่อ objective rollup ไปยังสถานะ KR ที่ **แย่ที่สุด** (off-track > at-risk > on-track)

## การทำงาน

CLI `bwoc okr` (`BWOC-48`) เรียก `okr.sh` จากไดเรกทอรีนี้ สะท้อนวิธีที่ `bwoc audit run` เรียก `audit.sh`: มันตั้ง `BWOC_OKR_OPERATION` + `BWOC_PLUGIN_DIR` และส่ง JSON request บรรทัดเดียวทาง stdin สคริปต์นี้รันด้วยมือ (argv flags) เพื่อ smoke test ได้เช่นกัน

| ช่องทาง | สิ่งที่ส่งมา |
|---|---|
| `BWOC_OKR_OPERATION` (env) | คำสั่ง — `report` \| `track` (เส้นทาง dispatcher; เป็น fallback ของคำสั่งเมื่อไม่ได้ส่ง argument ด้วย) |
| stdin | JSON request บรรทัดเดียวของ dispatcher เช่น `{"operation":"track","key_result_id":"O1-KR2","current":2,"evidence":"file:..."}` `track` อ่านพารามิเตอร์จากที่นี่เมื่อมี |
| arg 1 | คำสั่งสำหรับการเรียกด้วยมือ — `track` \| `check-progress` \| `report` |
| `BWOC_PLUGIN_DIR` (env) | path สัมบูรณ์ของไดเรกทอรีนี้; resolve `objectives.toml` / `key_results.toml` ถ้าไม่มีจะ fallback ไปยังไดเรกทอรีของสคริปต์เอง |

CLI เรียกปลั๊กอินด้วย `report` (มันอนุมาน `list` / `show` / การ rollup objective จากผลของ report) และ `track` เท่านั้น `check-progress` เป็น verb อ่านอย่างเดียวที่เป็นของปลั๊กอินเอง ใช้เรียกด้วยมือ เมื่อสำเร็จ: ออกด้วยรหัส `0`, ปล่อย JSON หนึ่งเอกสารทาง stdout เมื่อผิดพลาด: ข้อความวินิจฉัยทาง stderr + ออกด้วยรหัสไม่ใช่ศูนย์

```bash
# เรียกด้วยมือ (argv)
./okr.sh report
./okr.sh check-progress
./okr.sh track --key-result O1-KR2 --current 2 --evidence "file:crates/bwoc-cli/src/okr.rs"

# เส้นทาง dispatcher (stdin JSON)
echo '{"operation":"track","key_result_id":"O1-KR2","current":2}' | BWOC_OKR_OPERATION=track ./okr.sh
```

## รูปร่างผลลัพธ์

### `report`

JSON array ของ progress entry ที่สอดคล้องกับ [[../../../docs/th/PLUGINS.th#สคีมา OKR Progress|OKR Progress Schema]]:

```json
[
  {
    "objective_id": "O1",
    "key_result_id": "O1-KR1",
    "target": 1,
    "current": 1,
    "unit": "count",
    "confidence": "high",
    "evidence": { "kind": "file", "value": "docs/en/PLUGINS.en.md" },
    "as_of": "2026-05-28"
  }
]
```

`as_of` ถูกละไว้ (ไม่ใช่ `null`) สำหรับ key result ที่ยังไม่เคยติดตาม

### `check-progress`

```json
{
  "plugin": "workspace-okrs",
  "operation": "check-progress",
  "expected_attainment": 0.7,
  "key_results": [
    { "key_result_id": "O1-KR1", "objective_id": "O1", "attainment": 1.0, "status": "on-track" },
    { "key_result_id": "O1-KR2", "objective_id": "O1", "attainment": 0.0, "status": "at-risk" }
  ],
  "objectives": [
    { "objective_id": "O1", "title": "Ship the OKR plugin kind (BWOC-EPIC-4)", "status": "at-risk",
      "counts": { "on_track": 2, "at_risk": 2, "off_track": 0, "total": 4 } }
  ]
}
```

### `track`

ปล่อย key result ที่อัปเดตเพียงตัวเดียวเป็น progress entry (รูปร่างเดียวกับสมาชิกใน `report`) สะท้อนค่า `current` / `evidence` / `as_of` ใหม่

## คลาสข้อผิดพลาด

| Exit | คลาส | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | JSON หนึ่งเอกสารบน stdout |
| `1` | dependency / IO | ไม่มี `jq`, ไฟล์ข้อมูลหาย หรือ TOML ผิดรูป (`target`/`current` ไม่ใช่ตัวเลข, ฟิลด์จำเป็นหาย) |
| `2` | usage | คำสั่งไม่รู้จัก, flag หาย/ไม่ถูกต้อง, kind ของ `--evidence` ไม่ถูกต้อง หรือ `--key-result` ไม่พบ |

TOML ที่หายหรือผิดรูปล้มเหลวอย่าง **สะอาด**: ข้อความ stderr ชัดเจน + ออกด้วยรหัสไม่ใช่ศูนย์; ปลั๊กอินไม่เคย panic (`jq` คือ dependency รันไทม์ตัวเดียว เหมือนปลั๊กอินอ้างอิง `workflow/gcloud-*`)

## การตั้งค่า

```toml
# workspace.toml
[plugins.workspace-okrs]
enabled = true
```

ไม่มี `[config.schema]` — v1 อ่าน Objectives + Key Results จากไฟล์ TOML พี่น้อง surface ระดับ workspace เพียงตัวเดียวคือคีย์สากล `enabled`

## การจับคู่วงจรชีวิต

ตาม [[../../../docs/th/PLUGINS.th#Lifecycle|PLUGINS.th.md §Lifecycle]] kind `okr` ถูกเรียกโดย CLI `bwoc okr` `init`/`teardown` เกิดต่อการเรียกแต่ละครั้งรอบ ๆ `invoke` ปลั๊กอินไม่ถือสถานะใดนอกจากไฟล์ TOML สองไฟล์ที่มันอ่าน (และสำหรับ `track` คือเขียน)

| เฟส | สิ่งที่ปลั๊กอินนี้ทำ |
|---|---|
| `init` | โดยปริยาย; ตรวจว่ามี `jq` บน PATH; resolve ไฟล์ข้อมูล |
| `invoke` | parse คำสั่ง, อ่าน `objectives.toml` / `key_results.toml`, ปล่อย JSON (และสำหรับ `track` คือเขียน `key_results.toml` ใหม่) |
| `teardown` | โดยปริยาย; ไม่มีสถานะให้คืน |

## Idempotency

- `report` และ `check-progress` อ่านอย่างเดียว
- `track` เป็น idempotent ระดับ operation: การติดตาม key result ไปยัง `current` + `evidence` เดิมเขียน byte เดิม (มีเพียง `as_of` ที่ขยับเป็นวันนี้) การเล่นซ้ำหลังความล้มเหลวชั่วคราวลู่เข้า — การเขียนเป็น atomic (ไฟล์ temp + `mv`)

## ระดับวุฒิภาวะ (Maturity)

ประกาศ **L1** — ปลั๊กอินอ้างอิง `okr/workspace-okrs` ตัวแรกที่รันได้; ทั้งสาม verb ทำงานกับข้อมูล seed จะขยับเป็น **L2** เมื่อส่วนขยาย `bwoc check` (`BWOC-50`) ตรวจ manifest + Progress Schema + referential integrity และ CLI `bwoc okr` (`BWOC-48`) ทดสอบ verb แบบ end-to-end

> [!note] การแบ่งงานตรวจสอบ ปลั๊กอินนี้ทำให้ `bwoc check` ยอมรับ kind `okr` ในระดับ basic-well-formedness (kind อยู่ในชุดที่รองรับ) การตรวจสอบเฉพาะ okr เชิงลึก — referential integrity ระหว่าง `objectives.toml` ↔ `key_results.toml`, การบังคับ enum `confidence` และการตรวจฟิลด์ Progress Schema — อยู่ใน `BWOC-50` (เจ้าของ: `agent-rose`) ซึ่งถูกบล็อกโดยสตอรีนี้

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือ model ใด `kind = "okr"` เป็นค่า enum ของเฟรมเวิร์กเอง ไม่มีชื่อ vendor ปรากฏใน `kind`, `entry` หรือคีย์ config ใด สอดคล้องกับ **สมานัตตตา**

## ดูเพิ่มเติม

- [[../../../notes/2026-05-28_okr-plugin-architecture|บันทึกออกแบบ BWOC-46]] — framing ฉบับเต็ม (decisions 1–6)
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `okr` + OKR Progress Schema
- [[objectives|objectives.toml]] / [[key_results|key_results.toml]] — ข้อมูลที่ผู้ดูแลเขียนเอง (ตัวอย่าง seed)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (ความเท่าเทียมสองภาษา)
