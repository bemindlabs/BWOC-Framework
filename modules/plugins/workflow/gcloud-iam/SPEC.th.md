---
title: gcloud-iam — IAM project bindings ของ Google Cloud
aliases:
  - gcloud-iam
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-iam — IAM project bindings ของ Google Cloud

> [!abstract] slice GCP ตัวที่สี่และ **ตัวสุดท้าย** ที่เขียนได้ (`BWOC-EPIC-12`) — blast radius สูงสุด จึงสร้างเป็นตัวสุดท้ายโดยตั้งใจ รับผิดชอบ **IAM policy ระดับโปรเจกต์** — `get` (อ่าน) และ `add` / `remove` ของ binding `(member, role)` (เขียนแบบมีด่าน) การเขียนทั้งสองเป็น **T4 — ปฏิเสธโดยปริยาย + เปิดใช้แบบตั้งค่าถาวร ซ้อนบนด่านยืนยันแบบพิมพ์ชื่อ (T3)**: การแก้ IAM เปลี่ยน *ใครทำอะไรได้* และช่วงเวลาที่เปิดช่องระหว่างการ grant ผิดนั้นย้อนคืนไม่ได้ การกั้นการเขียนอยู่ใน CLI `bwoc gcloud iam` ทั้งหมด ไม่ใช่ในปลั๊กอิน source ฟังก์ชันช่วยจาก [[../gcloud-auth/SPEC.th|`gcloud-auth`]] กรอบฉบับเต็ม: [[../../../notes/2026-05-29_gcloud-iam-epic12-design|บันทึกออกแบบ EPIC-12]]

> [!danger] **T4 — ยอดของตารางความเสี่ยง** `add`/`remove` จะรันก็ต่อเมื่อ (1) workspace เปิดใช้การเขียน IAM อย่างชัดเจนด้วย `[plugins.gcloud-iam] writes_enabled = true` **และ** (2) ผู้ดูแลผ่านด่านยืนยันแบบพิมพ์ชื่อ (พิมพ์ `member role` ที่ resolve แล้วซ้ำ) principal สาธารณะ (`allUsers` / `allAuthenticatedUsers`) ถูก **ปฏิเสธเด็ดขาด** บทบาทสิทธิ์สูง (`owner` / `editor` / `*.admin` / `iam.*`) อนุญาตได้แต่ถูกตั้งธงเตือนความเสี่ยงสูง

## ทำไมเฉพาะ binding ระดับโปรเจกต์ (ไม่มี set-policy, ไม่มี SA key)

v1 คือ `get` ระดับโปรเจกต์ + `add`/`remove` ของ binding เดียว **เลื่อนออกไป — แต่ละอย่างอันตรายกว่า binding เดียวอย่างชัดเจน:** `set-iam-policy` (แทนที่ policy ทั้งก้อน; etag เก่าค่าเดียวทับทุก binding — `add`/`remove` คือ primitive ที่ผ่าตัดเฉพาะจุดและ atomic ฝั่ง server), **การสร้าง service-account key** (mint credential อายุยาว — ขัดกฎ Adinnādāna ที่ตั้งไว้ น่าจะเลื่อนตลอดไป), การจัดการ custom role, การสร้าง/ลบ SA และ IAM ระดับทรัพยากรที่ไม่ใช่โปรเจกต์ (bucket / Cloud Run / instance) สร้างต่อบน foundation auth ของ EPIC-8; ยังเป็น kind `workflow`

## คำสั่ง (Verbs)

| คำสั่ง | ทิศทาง | Auth | HTTP / ผลข้างเคียง | ระดับความเสี่ยง | ด่าน |
|---|---|---|---|---|---|
| `get` | อ่าน | ต้องมี | `gcloud projects get-iam-policy <project>` | T0 | ไม่มี (แต่ไม่เปิดให้ skill เลย — เผยสถานะความปลอดภัย) |
| `add` | **เขียน** | ต้องมี | `gcloud projects add-iam-policy-binding <project> --member=<m> --role=<r>` | **T4** | **`writes_enabled` ถาวร + ยืนยันพิมพ์ `member role`** (CLI) |
| `remove` | **เขียน** | ต้องมี | `gcloud projects remove-iam-policy-binding <project> --member=<m> --role=<r>` | **T4** | **`writes_enabled` ถาวร + ยืนยันพิมพ์ `member role`** (CLI) |

ระดับคือสเกลที่ใช้ซ้ำได้จาก [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|ตารางความเสี่ยง EPIC-9]] (tier = reversibility × blast radius) การเขียน IAM = **T4**: ย้อนได้ (`remove`/`add` ที่จับคู่กันแก้ binding คืน) แต่มี blast radius ระดับ **ความปลอดภัย** — ช่วงเวลาที่เปิดช่องย้อนคืนไม่ได้ — ดังนั้นการย้อนได้จึงไม่ลดระดับ

## การทำงาน

CLI `bwoc gcloud iam` เรียก `gcloud.sh` จากไดเรกทอรีนี้:

| ช่องทาง | สิ่งที่ส่ง |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `get` \| `add` \| `remove` — fallback ของ `.operation` |
| `BWOC_WORKSPACE` (env) | รากของ workspace แบบ absolute; path ของ SA JSON พี่น้อง resolve ภายใต้มัน |
| `BWOC_PLUGIN_DIR` (env) | path แบบ absolute ของไดเรกทอรีปลั๊กอิน; ใช้หา `../gcloud-auth/gcloud.sh` |
| stdin | JSON บรรทัดเดียว เช่น `{"operation":"add","project":"p","member":"user:x@y.com","role":"roles/viewer"}` |

> [!warning] ด่านกัน option-injection (#92) ค่าจากผู้ดูแลถึง `gcloud` ในรูป `--flag=value` (member/role ผูกค่า) หรือเป็น positional **หลังตัวคั่น `--`** (project id); ค่าที่ขึ้นต้นด้วย `-` parse เป็น flag ไม่ได้ CLI ยัง validate project id, รูปแบบ IAM-principal ของ member และรูปร่างของ role, ปฏิเสธ principal สาธารณะ และตั้งธงบทบาทสิทธิ์สูง **ก่อน** dispatch

## การยืนยันตัวตน (Authentication)

ปลั๊กอินนี้ **ไม่อ่านค่า credential ใด ๆ** มัน source ฟังก์ชันช่วยจากพี่น้อง [[../gcloud-auth/SPEC.th#การยืนยันตัวตน (Authentication)|`gcloud-auth`]] แล้วถาม `gcloud` เรื่องสถานะ IAM [[auth|auth.toml]] ประกาศสัญญาเฉพาะรูปร่างเดียวกัน ลำดับ: ADC → `.bwoc/secrets/gcloud-sa.json` → env `BWOC_GCLOUD_*`

> [!danger] **ศีล — อทินนาทาน** ไม่มี token เข้าสู่ address space ของปลั๊กอินนี้ `add`/`remove` ส่งเพียง project/member/role ไปยัง `gcloud` CLI ในเครื่อง ปลั๊กอินไม่เคย mint credential (ไม่มีการสร้าง SA key — อยู่นอกขอบเขตโดยการออกแบบ)

## รูปแบบผลลัพธ์ (Output shapes)

### `get`

```json
{ "ok": true, "plugin": "gcloud-iam", "operation": "get", "project": "my-proj",
  "bindings": [ { "role": "roles/viewer", "members": ["user:x@y.com"] } ] }
```

### `add` / `remove`

```json
{ "ok": true, "plugin": "gcloud-iam", "operation": "add",
  "project": "my-proj", "member": "user:x@y.com", "role": "roles/viewer", "present": true }
```

`present` สะท้อนว่าคู่ `(member, role)` อยู่ใน policy ที่การแก้คืนกลับมาหรือไม่ — `true` หลัง `add` สำเร็จ, `false` หลัง `remove` ตัด `etag` และ `auditConfigs` ทิ้ง (เราใช้ primitive `add`/`remove` แบบ atomic จึงไม่ต้องใช้ etag แบบ read-modify-write)

## ชั้นความผิดพลาด (Error classes)

| Exit | ชั้น | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | ออบเจ็กต์ JSON หนึ่งตัวบน stdout |
| `1` | dependency | ไม่มี `jq`, ไม่มีพี่น้อง `gcloud-auth/gcloud.sh`, หรือไม่มี `gcloud` |
| `2` | usage | operation ไม่รู้จัก; `get` ไม่มี project ที่ resolve ได้; `add`/`remove` ขาด `.project`/`.member`/`.role` |
| `3` | not-authenticated | ไม่มี credential `gcloud` ที่ใช้งานอยู่ |
| `6` | gcloud-error | คำสั่ง `gcloud` ที่อยู่ข้างใต้ล้มเหลว; ข้อความวินิจฉัยที่ตัดทอนบน stderr |

## การตั้งค่า (Configuration)

```toml
# workspace.toml
[plugins.gcloud-iam]
enabled = true
# จำเป็นเพื่ออนุญาตการเขียน IAM ใด ๆ หากไม่มี `bwoc gcloud iam add/remove`
# จะปฏิเสธโดยปริยาย (ด่านเปิดใช้ถาวร T4) การอ่าน (`get`) ไม่ต้องใช้
writes_enabled = true
```

ด่าน `writes_enabled` อ่านโดย **CLI** ไม่ใช่ปลั๊กอิน — ปลั๊กอินเห็นเพียง request ที่ผ่านการตรวจแล้ว การอ่านทำงานได้ด้วย `enabled = true` อย่างเดียว

## การแมป Lifecycle

เจ้าของ kind `workflow` คือผู้ดูแลผ่าน CLI `bwoc gcloud iam`; ไม่มี state ในเครื่องเกินกว่าที่ `gcloud` แคช `init` source ฟังก์ชันช่วย + ตรวจ `jq`; `invoke` รัน verb; `teardown` โดยปริยาย

## Idempotency

- `get` อ่านอย่างเดียว
- `add` idempotent: การเพิ่มคู่ `(member, role)` ที่มีอยู่แล้วเป็น no-op ฝั่ง server
- `remove` idempotent: การลบคู่ `(member, role)` ที่ไม่มีอยู่ ทำให้ policy ไม่เปลี่ยน

## Maturity

ประกาศ **L1** — ปลั๊กอิน `workflow/gcloud-iam` ที่รันได้ตัวแรก; ทั้งสาม verb ทำงานได้ ขยับเป็น **L2** เมื่อ smoke test รัน verb แบบ end-to-end กับ `gcloud` ที่ยืนยันตัวตนแล้วพร้อมโปรเจกต์ทิ้งขว้าง + principal ทดสอบ

> [!warning] ช่องว่างการทดสอบสด end-to-end กับ IAM policy ของโปรเจกต์จริงถูกกั้นด้วย sandbox ที่ผู้ดูแลจัดให้ (โปรเจกต์ทิ้งขว้าง + principal ทิ้งได้) — การเขียน IAM ไม่เคยรันกับโปรเจกต์จริงใน CI (บันทึก §Status) v0.1.0 ตรวจด้วย `bash -n` + shellcheck, เส้นทางไม่มีพี่น้อง/ไม่ยืนยันตัวตนที่ error สะอาด และ `bwoc check` ยอมรับ manifest

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือโมเดล `kind = "workflow"` เป็น enum ของเฟรมเวิร์ก "gcloud" / "IAM" ปรากฏเฉพาะใน `description` + เนื้อความ SPEC นี้ — ไม่อยู่ใน `kind`, `entry`, หรือคีย์ config สอดคล้อง **สมานัตตตา**

## ดูเพิ่ม

- [[../../../notes/2026-05-29_gcloud-iam-epic12-design|บันทึกออกแบบ EPIC-12]] — กรอบ + เหตุผล T4 (การใช้ tier สูงสุดของตารางครั้งแรก)
- [[../gcloud-auth/SPEC.th|gcloud-auth SPEC]] — ปลั๊กอินพี่น้อง (สถานะ credential); source ฟังก์ชันช่วยที่นี่
- [[../gcloud-storage/SPEC.th|gcloud-storage SPEC]] — พี่น้อง EPIC-10; slice T3 (พิมพ์ชื่อ) ตัวแรกที่ T4 ซ้อน opt-in ทับ
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `workflow`
- [[auth|auth.toml]] — สัญญา auth (เฉพาะรูปร่าง; ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (parity สองภาษา)
