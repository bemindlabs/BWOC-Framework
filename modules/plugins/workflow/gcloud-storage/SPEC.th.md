---
title: gcloud-storage — อ็อบเจ็กต์ของ Google Cloud Storage
aliases:
  - gcloud-storage
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-storage — อ็อบเจ็กต์ของ Google Cloud Storage

> [!abstract] slice GCP ตัวที่สองที่เขียนได้ (`BWOC-EPIC-10`) และเป็นตัวแรกที่มีการเขียนแบบ **ย้อนกลับไม่ได้** รับผิดชอบ **การจัดการอ็อบเจ็กต์** — `list` / `stat` (อ่าน) และ `put` / `delete` (เขียนแบบมีด่าน) `delete` เป็น **T3** (ยืนยันด้วยการพิมพ์ชื่อ) เพราะการลบอ็อบเจ็กต์เป็นการถาวร; `put` เป็นแบบ **stat ก่อน** (T1 สำหรับ path ใหม่, T2 เมื่อจะเขียนทับ) การเขียนถูกกั้นใน CLI `bwoc gcloud storage` ไม่ใช่ในปลั๊กอิน source ฟังก์ชันช่วยจาก [[../gcloud-auth/SPEC.th|`gcloud-auth`]] กรอบฉบับเต็ม: [[../../../notes/2026-05-29_gcloud-storage-epic10-design|บันทึกออกแบบ EPIC-10]]

## ทำไมระดับอ็อบเจ็กต์เท่านั้น

v1 คืออ่าน + เขียนอ็อบเจ็กต์เดี่ยว **lifecycle ของ bucket** (`buckets create`/`delete` — การลบ bucket ลบทุกอ็อบเจ็กต์พร้อมกัน) และ ops แบบ **recursive/bulk** (`rm -r`, `rsync`) ถูกเลื่อนเป็น slice อนาคตที่มีด่านเข้มกว่า EPIC-10 พิสูจน์รูปแบบการเขียนแบบย้อนกลับไม่ได้ (T3) บน blast radius เล็กสุดก่อน สร้างต่อบน foundation auth ของ EPIC-8 (source `gcloud-auth`); ยังเป็น kind `workflow`

## คำสั่ง (Verbs)

| คำสั่ง | ทิศทาง | Auth | HTTP / ผลข้างเคียง | ระดับความเสี่ยง | ด่าน |
|---|---|---|---|---|---|
| `list` | อ่าน | ต้องมี | `gcloud storage ls gs://<bucket>[/<prefix>]` | T0 | ไม่มี |
| `stat` | อ่าน | ต้องมี | `gcloud storage objects describe gs://<bucket>/<object>` (คืน `exists:false` เมื่อ not-found สะอาด) | T0 | ไม่มี |
| `put` | **เขียน** | ต้องมี | `gcloud storage cp <local> gs://<bucket>/<object>` | T1 / **T2** | **ยืนยัน** (T2 + แสดงอ็อบเจ็กต์เดิมเมื่อเขียนทับ) |
| `delete` | **เขียน (ย้อนไม่ได้)** | ต้องมี | `gcloud storage rm gs://<bucket>/<object>` | **T3** | **ยืนยันด้วยการพิมพ์ชื่อ** (พิมพ์ `gs://bucket/object` ซ้ำ) |

ระดับคือสเกลที่ใช้ซ้ำได้จาก [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|ตารางความเสี่ยง EPIC-9]] EPIC-10 เป็น slice แรกที่ใช้ **T3**

## การทำงาน

CLI `bwoc gcloud storage` เรียก `gcloud.sh` จากไดเรกทอรีนี้:

| ช่องทาง | สิ่งที่ส่ง |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `list` \| `stat` \| `put` \| `delete` — fallback ของ `.operation` |
| `BWOC_WORKSPACE` (env) | รากของ workspace แบบ absolute; path ของ SA JSON พี่น้อง resolve ภายใต้มัน |
| `BWOC_PLUGIN_DIR` (env) | path แบบ absolute ของไดเรกทอรีปลั๊กอิน; ใช้หา `../gcloud-auth/gcloud.sh` |
| stdin | JSON บรรทัดเดียว เช่น `{"operation":"put","bucket":"b","object":"a.txt","local":"./a.txt"}` |

> [!warning] ด่านกัน option-injection (#92) ค่าทุกตัวที่ผู้ดูแลส่งจะถึง `gcloud` ในรูป `--flag=value` หรือเป็น positional **หลังตัวคั่น `--`** (URL `gs://…` และ path ในเครื่อง) ดังนั้นค่าที่ขึ้นต้นด้วย `-` จึงไม่ถูก parse เป็น flag อีกทั้ง CLI validate ชื่อ bucket/object ก่อน dispatch

## การยืนยันตัวตน (Authentication)

ปลั๊กอินนี้ **ไม่อ่านค่า credential ใด ๆ** มัน source ฟังก์ชันช่วยจากพี่น้อง [[../gcloud-auth/SPEC.th#การยืนยันตัวตน (Authentication)|`gcloud-auth`]] แล้วถาม `gcloud` เรื่องสถานะอ็อบเจ็กต์ [[auth|auth.toml]] ประกาศสัญญาเฉพาะรูปร่างเดียวกัน ลำดับ: ADC → `.bwoc/secrets/gcloud-sa.json` → env `BWOC_GCLOUD_*`

> [!danger] **ศีล — อทินนาทาน** ไม่มี token เข้าสู่ address space ของปลั๊กอินนี้ `put`/`delete` ส่งเพียง bucket/object/path-ในเครื่อง ไปยัง `gcloud` CLI ในเครื่อง

## รูปแบบผลลัพธ์ (Output shapes)

### `list`

```json
{ "ok": true, "plugin": "gcloud-storage", "operation": "list", "bucket": "b",
  "total": 2,
  "objects": [
    { "url": "gs://b/a.txt", "size": 12, "updated": "2026-05-29T08:00:00Z" },
    { "url": "gs://b/logs/", "size": null, "updated": null }
  ] }
```

### `stat`

```json
{ "ok": true, "plugin": "gcloud-storage", "operation": "stat",
  "exists": true, "bucket": "b", "object": "a.txt",
  "size": 12, "updated": "2026-05-29T08:00:00Z", "content_type": "text/plain", "storage_class": "STANDARD" }
```

not-found ที่สะอาดคืน `{ "ok": true, "exists": false, "bucket": …, "object": … }` (exit `0`) — นี่คือสิ่งที่ `put` ใน CLI อ่านเพื่อเลือก T1 (ใหม่) หรือ T2 (เขียนทับ)

### `put` / `delete`

```json
{ "ok": true, "plugin": "gcloud-storage", "operation": "put",  "bucket": "b", "object": "a.txt", "source": "./a.txt" }
{ "ok": true, "plugin": "gcloud-storage", "operation": "delete","bucket": "b", "object": "a.txt", "deleted": true }
```

## ชั้นความผิดพลาด (Error classes)

| Exit | ชั้น | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | ออบเจ็กต์ JSON หนึ่งตัวบน stdout (รวม `stat` ที่ `exists:false`) |
| `1` | dependency | ไม่มี `jq`, ไม่มีพี่น้อง `gcloud-auth/gcloud.sh`, หรือไม่มี `gcloud` |
| `2` | usage | operation ไม่รู้จัก; `stat`/`put`/`delete` ไม่มี `.bucket`+`.object`; `put` ไม่มี/หาไม่เจอ `.local` |
| `3` | not-authenticated | ไม่มี credential `gcloud` ที่ใช้งานอยู่ |
| `6` | gcloud-error | คำสั่ง `gcloud` ที่อยู่ข้างใต้ล้มเหลว (error จริง ต่างจาก not-found สะอาดของ `stat`) |

## การตั้งค่า (Configuration)

```toml
# workspace.toml
[plugins.gcloud-storage]
enabled = true
```

ไม่มี `[config.schema]` — สถานะอ็อบเจ็กต์ถูก query สด มีเพียงคีย์ `enabled` สากล

## การแมป Lifecycle

เจ้าของ kind `workflow` คือผู้ดูแลผ่าน CLI `bwoc gcloud storage`; ไม่มี state ในเครื่องเกินกว่าที่ `gcloud` แคช `init` source ฟังก์ชันช่วย + ตรวจ `jq`; `invoke` รัน verb; `teardown` โดยปริยาย

## Idempotency

- `list` / `stat` อ่านอย่างเดียว
- `put` เป็น idempotent (อัปโหลด bytes เดิมซ้ำลู่เข้า)
- `delete` idempotent เชิงผล (gone→gone); การ `delete` อ็อบเจ็กต์ที่ไม่มีจะคืน error ของ `gcloud` (exit 6) ไม่ใช่สำเร็จลวง

## Maturity

ประกาศ **L1** — ปลั๊กอิน `workflow/gcloud-storage` ที่รันได้ตัวแรก; ทั้งสี่ verb ทำงานได้ ขยับเป็น **L2** เมื่อ smoke test รัน verb แบบ end-to-end กับ `gcloud` ที่ยืนยันตัวตนแล้วพร้อม test bucket + อ็อบเจ็กต์ทิ้งได้

> [!warning] ช่องว่างการทดสอบสด end-to-end กับ bucket จริงถูกกั้นด้วย sandbox ที่ผู้ดูแลจัดให้ (บันทึก §Status) v0.1.0 ตรวจด้วย `bash -n` + shellcheck, เส้นทางไม่มีพี่น้อง/ไม่ยืนยันตัวตนที่ error สะอาด และ `bwoc check` ยอมรับ manifest

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือโมเดล `kind = "workflow"` เป็น enum ของเฟรมเวิร์ก "gcloud" / "Google Cloud Storage" ปรากฏเฉพาะใน `description` + เนื้อความ SPEC นี้ — ไม่อยู่ใน `kind`, `entry`, หรือคีย์ config สอดคล้อง **สมานัตตตา**

## ดูเพิ่ม

- [[../../../notes/2026-05-29_gcloud-storage-epic10-design|บันทึกออกแบบ EPIC-10]] — กรอบ + การใช้ T3
- [[../gcloud-auth/SPEC.th|gcloud-auth SPEC]] — ปลั๊กอินพี่น้อง (สถานะ credential); source ฟังก์ชันช่วยที่นี่
- [[../gcloud-compute/SPEC.th|gcloud-compute SPEC]] — พี่น้อง EPIC-9; ตารางความเสี่ยงที่ slice นี้ขยายไป T3
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `workflow`
- [[auth|auth.toml]] — สัญญา auth (เฉพาะรูปร่าง; ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (parity สองภาษา)
