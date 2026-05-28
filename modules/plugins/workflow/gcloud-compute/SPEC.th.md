---
title: gcloud-compute — วงจรชีวิต Instance ของ Google Cloud Compute
aliases:
  - gcloud-compute
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-compute — วงจรชีวิต Instance ของ Google Cloud Compute

> [!abstract] slice GCP ตัวแรกที่ **เขียนได้** (`BWOC-EPIC-9`) สร้างต่อยอดบน foundation ของ EPIC-8 รับผิดชอบ **วงจรชีวิตของ instance** — `list` / `describe` (อ่าน) และ `start` / `stop` (เขียนแบบมีด่าน) การเขียนถูกกั้นด้วยการยืนยันใน CLI `bwoc gcloud compute` ไม่ใช่ในปลั๊กอิน source ฟังก์ชันช่วยจาก [[../gcloud-auth/SPEC.th|`gcloud-auth`]] ที่เป็นพี่น้อง กรอบฉบับเต็ม + ตารางความเสี่ยงของ verb เขียนที่ใช้ซ้ำได้: [[../../../notes/2026-05-28_gcloud-compute-epic9-design|บันทึกออกแบบ EPIC-9]]

## ทำไมต้องสร้างบน foundation

`gcloud-compute` ใช้พื้นผิว auth ที่ `gcloud-auth` วางไว้ซ้ำ — การ resolve credential ถูกนิยามครั้งเดียวและ source ที่นี่ตอน startup เหมือนที่ `gcloud-project` ทำ (บันทึกออกแบบ EPIC-8 §Decision 2) ยังคงเป็น kind `workflow` (ไม่ใช่ kind `gcp` ใหม่): เฟรมเวิร์กไม่ได้ถือ lifecycle แต่เรียก `gcloud` ออกไปแล้วนำเสนอผลลัพธ์ EPIC-9 จงใจเป็น slice วงจรชีวิตที่ **ย้อนกลับได้** เท่านั้น — `start`/`stop` — เพราะ `stop` ที่พลาดสามารถกู้คืนได้ด้วย `start` ส่วน `instances.{delete,reset,create}` อยู่นอกขอบเขต (ย้อนกลับไม่ได้ / blast radius สูงกว่า — เป็น slice อนาคตของตัวเอง)

## คำสั่ง (Verbs)

| คำสั่ง | ทิศทาง | Auth | HTTP / ผลข้างเคียง | ระดับความเสี่ยง | ด่าน |
|---|---|---|---|---|---|
| `list` | อ่าน | ต้องมี | `gcloud compute instances list` (เลือก `--zones` ได้) | T0 | ไม่มี |
| `describe` | อ่าน | ต้องมี | `gcloud compute instances describe <i> --zone <z>` | T0 | ไม่มี |
| `start` | **เขียน (ระยะไกล)** | ต้องมี | `gcloud compute instances start <i> --zone <z>` — ปลุก instance ที่หยุดอยู่ | T1 | **ยืนยัน** (CLI; `--json` ⇒ `--yes`) |
| `stop` | **เขียน (ระยะไกล)** | ต้องมี | `gcloud compute instances stop <i> --zone <z>` — หยุด instance ที่กำลังรัน | T2 | **ยืนยัน + แสดง `project/zone/instance` ที่ resolve แล้ว** (CLI) |

ระดับการยืนยันคือสเกลที่ใช้ซ้ำได้ซึ่งนิยามใน [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|บันทึกออกแบบ §3]]: T0 อ่าน · T1 ย้อนได้/ค่าใช้จ่าย · T2 ย้อนได้/กระทบ availability (แสดงเป้าหมาย) · T3 ย้อนไม่ได้ (พิมพ์ชื่อ) · T4 กระทบความปลอดภัย (ปฏิเสธ + เปิดใช้อย่างชัดแจ้ง) EPIC-9 ใช้ T0/T1/T2; `start`=T1, `stop`=T2

## การทำงาน

CLI `bwoc gcloud compute` เรียก `gcloud.sh` จากไดเรกทอรีนี้:

| ช่องทาง | สิ่งที่ส่ง |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `list` \| `describe` \| `start` \| `stop` — fallback ของ `.operation` |
| `BWOC_WORKSPACE` (env) | รากของ workspace แบบ absolute; path ของ SA JSON พี่น้อง resolve ภายใต้มัน |
| `BWOC_PLUGIN_DIR` (env) | path แบบ absolute ของไดเรกทอรีปลั๊กอินนี้; ใช้หาฟังก์ชันช่วยพี่น้องที่ `../gcloud-auth/gcloud.sh` |
| stdin | JSON บรรทัดเดียว เช่น `{"operation":"describe","instance":"web-1","zone":"us-central1-a"}` |

เมื่อสำเร็จ: exit `0`, ออบเจ็กต์ JSON หนึ่งตัวบน stdout เมื่อผิดพลาด: ข้อความวินิจฉัยบน stderr + exit ไม่เป็นศูนย์

> [!warning] ด่านกัน option-injection (#92) ค่าทุกตัวที่ผู้ดูแลส่งจะถึง `gcloud` ในรูป `--flag=value` (ผูกค่าไว้ ไม่ถูก parse ซ้ำ) หรือเป็น positional **หลังตัวคั่น `--` (end-of-options)** ดังนั้น instance id ที่ขึ้นต้นด้วย `-` จะไม่มีวันถูกอ่านเป็น flag ของ gcloud อีกทั้ง CLI ยัง validate charset ของ instance/zone ก่อน dispatch

## การยืนยันตัวตน (Authentication)

ปลั๊กอินนี้ **ไม่อ่านค่า credential ใด ๆ** มัน source ฟังก์ชันช่วยจากพี่น้อง [[../gcloud-auth/SPEC.th#การยืนยันตัวตน (Authentication)|`gcloud-auth`]] (`gcloud_assert_cli`, `gcloud_assert_authenticated`) ตอน startup แล้วถาม `gcloud` เรื่องสถานะ compute [[auth|auth.toml]] ประกาศสัญญา auth **เดียวกัน** (เฉพาะรูปร่าง) กับพี่น้องเพื่อให้ `bwoc check` ตรวจได้รายปลั๊กอิน และผู้ดูแลเห็นโมเดลทั้งหมดที่นี่

ลำดับความสำคัญของ credential เหมือน [[../gcloud-auth/SPEC.th#การยืนยันตัวตน (Authentication)|`gcloud-auth`]]: ADC → `.bwoc/secrets/gcloud-sa.json` → env `BWOC_GCLOUD_*` ปลั๊กอินล้มเหลวเร็วพร้อมข้อความชัดเจนถ้าไม่มี credential ที่ใช้งานอยู่

> [!danger] **ศีล — อทินนาทาน** ไม่มี token ใดเข้าสู่ address space ของปลั๊กอินนี้ เราเพียงเรียก `gcloud` และนำเสนอผลลัพธ์ การเขียนวงจรชีวิตไม่ส่งอะไรนอกจาก instance/zone/project ไปยัง `gcloud` CLI ในเครื่อง

## รูปแบบผลลัพธ์ (Output shapes)

### `list`

```json
{
  "ok": true,
  "plugin": "gcloud-compute",
  "operation": "list",
  "total": 2,
  "instances": [
    { "name": "web-1", "zone": "us-central1-a", "status": "RUNNING",    "machine_type": "e2-medium", "internal_ip": "10.0.0.2" },
    { "name": "batch", "zone": "us-central1-b", "status": "TERMINATED", "machine_type": "e2-small",  "internal_ip": "10.0.0.3" }
  ]
}
```

### `describe`

```json
{
  "ok": true,
  "plugin": "gcloud-compute",
  "operation": "describe",
  "name": "web-1",
  "zone": "us-central1-a",
  "status": "RUNNING",
  "machine_type": "e2-medium",
  "internal_ip": "10.0.0.2",
  "external_ip": "34.x.x.x",
  "create_time": "2026-05-01T08:00:00Z"
}
```

### `start` / `stop`

```json
{
  "ok": true,
  "plugin": "gcloud-compute",
  "operation": "stop",
  "instance": "web-1",
  "zone": "us-central1-a",
  "status": "TERMINATED"
}
```

`gcloud` รอให้การทำงานวงจรชีวิตเสร็จ จากนั้นปลั๊กอินอ่าน instance ซ้ำและรายงาน `status` ที่ **เกิดขึ้นจริง** (สัจจะ — รายงานสิ่งที่เป็นจริง ไม่ใช่สถานะที่ตั้งใจ) `status` เป็น `null` ถ้าการอ่านหลังการทำงานล้มเหลว

## ชั้นความผิดพลาด (Error classes)

| Exit | ชั้น | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | ออบเจ็กต์ JSON หนึ่งตัวบน stdout |
| `1` | dependency | ไม่มี `jq`, ไม่มีพี่น้อง `gcloud-auth/gcloud.sh`, หรือไม่มี `gcloud` |
| `2` | usage | operation ไม่รู้จัก หรือ `describe`/`start`/`stop` ถูกเรียกโดยไม่มี `.instance` + `.zone` |
| `3` | not-authenticated | ไม่มี credential `gcloud` ที่ใช้งานอยู่ |
| `6` | gcloud-error | คำสั่ง `gcloud` ที่อยู่ข้างใต้ล้มเหลว; ข้อความวินิจฉัยที่ตัดทอนอยู่บน stderr |

ไม่มี `gcloud` CLI จะล้มเหลว **อย่างนุ่มนวล** — ข้อความ stderr ชัดเจน + exit ไม่เป็นศูนย์ ไม่เคย panic

## การตั้งค่า (Configuration)

```toml
# workspace.toml
[plugins.gcloud-compute]
enabled = true
```

ไม่มี `[config.schema]` — สถานะ compute ถูก query สด พื้นผิวระดับ workspace มีเพียงคีย์ `enabled` สากล

## การแมป Lifecycle

ตาม [[../../../docs/th/PLUGINS.th#Lifecycle|PLUGINS.th.md §Lifecycle]] เจ้าของของ kind `workflow` คือ **เอเจนต์/ผู้ดูแล** ที่เรียกออกผ่าน CLI `bwoc gcloud compute` ปลั๊กอินไม่ถือ **state ในเครื่อง** เกินกว่าที่ `gcloud` แคชเอง

| เฟส | ปลั๊กอินทำอะไร |
|---|---|
| `init` | โดยปริยาย; source ฟังก์ชันช่วยพี่น้อง; ตรวจ `jq` บน PATH |
| `invoke` | อ่าน request, เรียก `gcloud compute instances ...`, ส่ง JSON |
| `teardown` | โดยปริยาย; ไม่มี state ให้ปล่อย |

## Idempotency

- `list` และ `describe` อ่านอย่างเดียว
- `start` / `stop` เป็น idempotent: การ start instance ที่รันอยู่ (หรือ stop ตัวที่หยุดแล้ว) ลู่เข้าสู่สถานะเดิม และ `status` ที่อ่านซ้ำสะท้อนสถานะปลายทาง การ replay หลัง error ชั่วคราวของ `gcloud` ลู่เข้า

## Maturity

ประกาศ **L1** — ปลั๊กอินอ้างอิง `workflow/gcloud-compute` ที่รันได้ตัวแรก; ทั้งสี่ verb ทำงานได้ จะขยับเป็น **L2** เมื่อ smoke test รัน verb แบบ end-to-end กับ `gcloud` ในเครื่องที่ยืนยันตัวตนแล้วพร้อม instance ทดสอบที่หยุดได้

> [!warning] ช่องว่างการทดสอบสด การทดสอบ end-to-end กับ instance GCP จริงถูกกั้นด้วย sandbox ที่ผู้ดูแลจัดให้ (SA JSON ที่ `.bwoc/secrets/gcloud-sa.json` + instance ทดสอบที่หยุดได้ — บันทึก §Status) v0.1.0 ตรวจด้วย: `bash -n gcloud.sh`, เส้นทางไม่มีพี่น้องที่ error อย่างสะอาด, เส้นทางไม่ยืนยันตัวตนที่คืนข้อความ `not-authenticated`, verb อ่านกับ `gcloud` จริง และ `bwoc check` ยอมรับ manifest

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือโมเดลใด `kind = "workflow"` เป็นค่า enum ของเฟรมเวิร์กเอง "gcloud" / "Google Cloud" ปรากฏเฉพาะใน `description` (ที่ยอมให้มีชื่อระบบเป้าหมายตาม [[../../../docs/th/PLUGINS.th#ข้อจำกัดความเป็นกลาง (HARD)|PLUGINS.th.md §ความเป็นกลาง]]) และในเนื้อความ SPEC นี้ — ไม่เคยอยู่ใน `kind`, `entry`, หรือคีย์ config สอดคล้อง **สมานัตตตา**

## ดูเพิ่ม

- [[../../../notes/2026-05-28_gcloud-compute-epic9-design|บันทึกออกแบบ EPIC-9]] — กรอบ + ตารางความเสี่ยงของ verb เขียนที่ใช้ซ้ำได้
- [[../gcloud-auth/SPEC.th|gcloud-auth SPEC]] — ปลั๊กอินพี่น้อง (สถานะ credential); source ฟังก์ชันช่วยที่นี่
- [[../gcloud-project/SPEC.th|gcloud-project SPEC]] — ปลั๊กอิน foundation อีกตัว
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `workflow`
- [[auth|auth.toml]] — สัญญา auth (เฉพาะรูปร่าง; ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (parity สองภาษา)
