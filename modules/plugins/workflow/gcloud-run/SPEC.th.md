---
title: gcloud-run — Serverless ของ Google Cloud Run
aliases:
  - gcloud-run
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-run — Serverless ของ Google Cloud Run

> [!abstract] slice GCP ตัวที่สามที่เขียนได้ (`BWOC-EPIC-11`) รับผิดชอบ **บริการ Cloud Run** — `list` / `describe` (อ่าน) และ `deploy` (เขียนแบบมีด่าน) `deploy` เป็น **T2** (ยืนยัน + แสดงเป้าหมายที่ resolve แล้ว): การ deploy เปลี่ยนบริการที่กำลังรับ traffic อยู่ แต่ย้อนกลับได้ผ่านการ rollback revision การกั้นการเขียนอยู่ใน CLI `bwoc gcloud run` ไม่ใช่ในปลั๊กอิน source ฟังก์ชันช่วยจาก [[../gcloud-auth/SPEC.th|`gcloud-auth`]] กรอบฉบับเต็ม: [[../../../notes/2026-05-29_gcloud-serverless-epic11-design|บันทึกออกแบบ EPIC-11]]

## ทำไม Cloud Run อย่างเดียว (ไม่มี Cloud Build, ไม่มี delete)

v1 คือ `list`/`describe`/`deploy` ของบริการ `gcloud run deploy --source` trigger การ build ฝั่ง server อยู่แล้ว ดังนั้นปลั๊กอิน `gcloud-build` แยก (`builds submit`) จึง **เลื่อน** เป็น slice ของตัวเอง ส่วน `services delete` (ลบบริการที่ใช้งานอยู่) และการแบ่ง traffic ล้วน ๆ ก็เลื่อนเช่นกัน — `deploy` คือการเขียนที่ blast radius ต่ำสุดที่พิสูจน์รูปแบบ serverless สร้างต่อบน foundation auth ของ EPIC-8; ยังเป็น kind `workflow`

## คำสั่ง (Verbs)

| คำสั่ง | ทิศทาง | Auth | HTTP / ผลข้างเคียง | ระดับความเสี่ยง | ด่าน |
|---|---|---|---|---|---|
| `list` | อ่าน | ต้องมี | `gcloud run services list [--region <r>]` | T0 | ไม่มี |
| `describe` | อ่าน | ต้องมี | `gcloud run services describe <svc> --region <r>` | T0 | ไม่มี |
| `deploy` | **เขียน** | ต้องมี | `gcloud run deploy <svc> --region <r> {--image <img> \| --source <dir>}` | **T2** | **ยืนยัน + แสดง `service/region/source/traffic`** (CLI) |

ระดับคือสเกลที่ใช้ซ้ำได้จาก [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|ตารางความเสี่ยง EPIC-9]] (tier = reversibility × blast radius) `deploy` = T2: ย้อนได้ (rollback revision) แต่มี blast radius ระดับ availability ทั้งบริการ

## การทำงาน

CLI `bwoc gcloud run` เรียก `gcloud.sh` จากไดเรกทอรีนี้:

| ช่องทาง | สิ่งที่ส่ง |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `list` \| `describe` \| `deploy` — fallback ของ `.operation` |
| `BWOC_WORKSPACE` (env) | รากของ workspace แบบ absolute; path ของ SA JSON พี่น้อง resolve ภายใต้มัน |
| `BWOC_PLUGIN_DIR` (env) | path แบบ absolute ของไดเรกทอรีปลั๊กอิน; ใช้หา `../gcloud-auth/gcloud.sh` |
| stdin | JSON บรรทัดเดียว เช่น `{"operation":"deploy","service":"api","region":"us-central1","image":"gcr.io/p/api:v2"}` |

> [!warning] ด่านกัน option-injection (#92) ค่าจากผู้ดูแลถึง `gcloud` ในรูป `--flag=value` (region/image/source ผูกค่า) หรือเป็น positional **หลังตัวคั่น `--`** (ชื่อ service); ค่าที่ขึ้นต้นด้วย `-` parse เป็น flag ไม่ได้ CLI ยัง validate ชื่อ service/region และแปลง `--source` เป็น path แบบ absolute ก่อน dispatch

## การยืนยันตัวตน (Authentication)

ปลั๊กอินนี้ **ไม่อ่านค่า credential ใด ๆ** มัน source ฟังก์ชันช่วยจากพี่น้อง [[../gcloud-auth/SPEC.th#การยืนยันตัวตน (Authentication)|`gcloud-auth`]] แล้วถาม `gcloud` เรื่องสถานะ Cloud Run [[auth|auth.toml]] ประกาศสัญญาเฉพาะรูปร่างเดียวกัน ลำดับ: ADC → `.bwoc/secrets/gcloud-sa.json` → env `BWOC_GCLOUD_*`

> [!danger] **ศีล — อทินนาทาน** ไม่มี token เข้าสู่ address space ของปลั๊กอินนี้ `deploy` ส่งเพียง service/region/image/source ไปยัง `gcloud` CLI ในเครื่อง

## รูปแบบผลลัพธ์ (Output shapes)

### `list`

```json
{ "ok": true, "plugin": "gcloud-run", "operation": "list", "total": 1,
  "services": [ { "name": "api", "region": "us-central1", "url": "https://api-xxxx.run.app", "ready": "True" } ] }
```

### `describe`

```json
{ "ok": true, "plugin": "gcloud-run", "operation": "describe",
  "service": "api", "region": "us-central1", "url": "https://api-xxxx.run.app",
  "latest_ready_revision": "api-00007-abc", "latest_created_revision": "api-00007-abc",
  "traffic": [ { "revision": "api-00007-abc", "percent": 100, "latest": true } ] }
```

### `deploy`

```json
{ "ok": true, "plugin": "gcloud-run", "operation": "deploy",
  "service": "api", "region": "us-central1", "url": "https://api-xxxx.run.app",
  "latest_ready_revision": "api-00008-def" }
```

`gcloud run deploy` รันด้วย `--quiet` (CLI ของ BWOC เป็นเจ้าของด่านยืนยัน T2); envelope รายงาน URL + revision ที่พร้อมใหม่

## ชั้นความผิดพลาด (Error classes)

| Exit | ชั้น | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | ออบเจ็กต์ JSON หนึ่งตัวบน stdout |
| `1` | dependency | ไม่มี `jq`, ไม่มีพี่น้อง `gcloud-auth/gcloud.sh`, หรือไม่มี `gcloud` |
| `2` | usage | operation ไม่รู้จัก; `describe`/`deploy` ไม่มี `.service`+`.region`; `deploy` ไม่มีหรือมีทั้ง `.image`/`.source` |
| `3` | not-authenticated | ไม่มี credential `gcloud` ที่ใช้งานอยู่ |
| `6` | gcloud-error | คำสั่ง `gcloud` ที่อยู่ข้างใต้ล้มเหลว; ข้อความวินิจฉัยที่ตัดทอนบน stderr |

## การตั้งค่า (Configuration)

```toml
# workspace.toml
[plugins.gcloud-run]
enabled = true
```

ไม่มี `[config.schema]` — สถานะบริการถูก query สด มีเพียงคีย์ `enabled` สากล

## การแมป Lifecycle

เจ้าของ kind `workflow` คือผู้ดูแลผ่าน CLI `bwoc gcloud run`; ไม่มี state ในเครื่องเกินกว่าที่ `gcloud` แคช `init` source ฟังก์ชันช่วย + ตรวจ `jq`; `invoke` รัน verb; `teardown` โดยปริยาย

## Idempotency

- `list` / `describe` อ่านอย่างเดียว
- `deploy` idempotent เชิงผล: การ deploy image+config เดิมซ้ำลู่เข้าสู่ revision ที่ให้บริการเทียบเท่ากัน การ deploy แบบ `--source` จะ rebuild; revision ที่ได้เทียบเท่ากันเชิงฟังก์ชันสำหรับ source เดิม

## Maturity

ประกาศ **L1** — ปลั๊กอิน `workflow/gcloud-run` ที่รันได้ตัวแรก; ทั้งสาม verb ทำงานได้ ขยับเป็น **L2** เมื่อ smoke test รัน verb แบบ end-to-end กับ `gcloud` ที่ยืนยันตัวตนแล้วพร้อมบริการที่ deploy ได้

> [!warning] ช่องว่างการทดสอบสด end-to-end กับบริการ Cloud Run จริงถูกกั้นด้วย sandbox ที่ผู้ดูแลจัดให้ (บันทึก §Status) v0.1.0 ตรวจด้วย `bash -n` + shellcheck, เส้นทางไม่มีพี่น้อง/ไม่ยืนยันตัวตนที่ error สะอาด และ `bwoc check` ยอมรับ manifest

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือโมเดล `kind = "workflow"` เป็น enum ของเฟรมเวิร์ก "gcloud" / "Cloud Run" ปรากฏเฉพาะใน `description` + เนื้อความ SPEC นี้ — ไม่อยู่ใน `kind`, `entry`, หรือคีย์ config สอดคล้อง **สมานัตตตา**

## ดูเพิ่ม

- [[../../../notes/2026-05-29_gcloud-serverless-epic11-design|บันทึกออกแบบ EPIC-11]] — กรอบ + เหตุผล T2 ของ deploy
- [[../gcloud-auth/SPEC.th|gcloud-auth SPEC]] — ปลั๊กอินพี่น้อง (สถานะ credential); source ฟังก์ชันช่วยที่นี่
- [[../gcloud-compute/SPEC.th|gcloud-compute SPEC]] — พี่น้อง EPIC-9; ตารางความเสี่ยงที่ slice นี้ใช้ซ้ำ
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `workflow`
- [[auth|auth.toml]] — สัญญา auth (เฉพาะรูปร่าง; ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (parity สองภาษา)
