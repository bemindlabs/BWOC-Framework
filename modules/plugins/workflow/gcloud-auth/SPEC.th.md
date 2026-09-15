---
title: gcloud-auth — สถานะข้อมูลรับรอง Google Cloud
aliases:
  - gcloud-auth
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-auth — สถานะข้อมูลรับรอง Google Cloud

> [!abstract] หนึ่งในสองปลั๊กอินอ้างอิงสาย `workflow` สำหรับ foundation ของ GCP (`BWOC-EPIC-8`) รับผิดชอบ **สถานะข้อมูลรับรอง** — แหล่งข้อมูลรับรองที่กำลังใช้งาน (ADC vs service-account vs env), อีเมลบัญชีที่ active และการมี/ไม่มีข้อมูลรับรอง คำสั่ง: `status` (อ่านอย่างเดียว; **ไม่เคยพิมพ์ค่า credential**) และ `login` (ผู้ดูแลขับเคลื่อน ใช้ shell-out ไปยัง `gcloud auth login`) จับคู่กับ [[../gcloud-project/SPEC.th|`gcloud-project`]] สำหรับการสำรวจ project รวมแล้วทั้งสองตัวประกอบเป็น foundation auth+context ที่ปลั๊กอิน GCP ในอนาคตต่อยอด เหตุผลฉบับเต็ม: [[../../../notes/2026-05-28_gcloud-workflow-plugin-architecture|บันทึกออกแบบ BWOC-51]]

## ทำไมต้องสองปลั๊กอิน ทำไม kind `workflow`

`gcloud-auth` กับ `gcloud-project` ส่งมาเป็น **สอง** ปลั๊กอินเพื่อให้ slice ในอนาคต (`gcloud-compute`, `gcloud-storage`, …) นำ surface auth ที่ประกาศไว้ที่นี่ไปใช้ใหม่ได้โดยไม่ต้องสืบทอด verb ฝั่ง project ที่ไม่ต้องใช้ ส่วนการใช้ kind `workflow` (แทนที่จะออก kind `gcp` ใหม่) ถูกต้องเพราะเฟรมเวิร์กไม่ได้ถือ lifecycle: เอเจนต์เรียก `gcloud` CLI ในเครื่องและอ่านผลลัพธ์ของมัน ไม่มีอะไรใน BWOC ที่ถือ sync ledger หรือ schema GCP ที่เป็นบรรทัดฐาน เหตุผลฉบับเต็ม: บันทึกออกแบบ §1 + 2

## คำสั่ง (Verbs)

| คำสั่ง | ทิศทาง | Auth | ผลข้างเคียง |
|---|---|---|---|
| `status` | อ่าน | ไม่ต้องมี | ไม่มี — อ่านเฉพาะ config ในเครื่องและการมีอยู่ของไฟล์ **ไม่เคยพิมพ์ค่า token หรือ credential** |
| `login` | เขียน (ในเครื่อง) | ไม่ต้องมี | สตรีม `gcloud auth login` ไปยัง TTY ของผู้ดูแล **ผู้ดูแลขับเคลื่อนเท่านั้น** ไม่เคยให้เอเจนต์เรียกเอง (BWOC-54 / บันทึก §Decision 5) |

`status` คือ verb อ่านหลักสำหรับเอเจนต์ ส่วน `login` เป็น pass-through บาง ๆ ไปยัง `gcloud` CLI

## การทำงาน

CLI `bwoc gcloud` (`BWOC-52`) เรียก `gcloud.sh` จากไดเรกทอรีนี้ (สะท้อนวิธีที่ `bwoc audit` เรียกปลั๊กอิน `audit` และ `bwoc jira` เรียก `jira-cloud-rest`):

| ช่องทาง | สิ่งที่ส่งมา |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `status` \| `login` — fallback สำหรับ `.operation` เมื่อ stdin ว่าง |
| `BWOC_WORKSPACE` (env) | path สัมบูรณ์ของ workspace; ใช้ resolve path ของ service-account JSON |
| `BWOC_PLUGIN_DIR` (env) | path สัมบูรณ์ของไดเรกทอรีปลั๊กอินนี้ (เพื่อข้อมูล) |
| `BWOC_GCLOUD_ACCOUNT` / `BWOC_GCLOUD_PROJECT` / `BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT` (env, ทางเลือก) | ค่า override จากผู้ดูแล (precedence ต่ำสุด) |
| stdin | คำขอ JSON บรรทัดเดียว เช่น `{"operation":"status"}` หรือ `{"operation":"login","account":"me@example.com"}` |

เมื่อสำเร็จ: ออกด้วยรหัส `0`, ปล่อย JSON หนึ่งอ็อบเจกต์ทาง stdout เมื่อผิดพลาด: ข้อความวินิจฉัยทาง stderr + ออกด้วยรหัสไม่ใช่ศูนย์ (CLI จะรายงานเป็น `plugin '<name>' exited <code>`)

## การยืนยันตัวตน

ปลั๊กอินนี้ **ไม่เคยอ่านค่า credential ใด ๆ** มันเพียงตรวจการมีอยู่ของไฟล์และถาม `gcloud` ถึงสถานะ ลำดับความสำคัญ (แหล่งแรกที่ resolve ได้ชนะ; สะท้อนใน `status.active_source`):

1. **Application Default Credentials (ADC)** — `~/.config/gcloud/application_default_credentials.json` (หรือ `$CLOUDSDK_CONFIG/application_default_credentials.json` หากตั้งไว้) เป็นค่าเริ่มต้นสำหรับ session นักพัฒนาที่เป็นมนุษย์ สร้างโดย `gcloud auth application-default login`
2. **Service-account JSON** — `${BWOC_WORKSPACE}/.bwoc/secrets/gcloud-sa.json` ถูก gitignore ที่ระดับ workspace (เพิ่มในสตอรี่นี้), `chmod 600`, ไม่เคย commit เหมาะสำหรับ CI / agent แบบ headless
3. **ตัวแปร environment** — `BWOC_GCLOUD_ACCOUNT`, `BWOC_GCLOUD_PROJECT`, ทางเลือก `BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT` precedence ต่ำสุด — transient ที่สุด

[[auth|auth.toml]] ประกาศ **รูปร่าง** — แหล่งใดถูกพิจารณา, env vars ใดสำคัญ, file paths ใดคาดหวัง — โดย **ไม่มีค่า** `bwoc check` (`BWOC-55`) จะปฏิเสธหากปรากฏฟิลด์ที่ดูเหมือนเป็นค่า

> [!danger] **ศีล — อทินนาทาน** `auth.toml` ที่ malformed ไม่สามารถทำให้ token รั่วได้ เพราะไม่มี token ใดเข้ามาใน address space ของปลั๊กอินเลย การดำเนินการที่ใกล้ความลับที่สุดของปลั๊กอินคือ (ก) ตรวจว่ามีไฟล์ `gcloud-sa.json` หรือไม่ และ (ข) ให้ `gcloud` อ่านไฟล์นั้นเอง token ไม่เคยถูก echo, ไม่เคยถูก log และไม่เคยปรากฏใน JSON output ใด ๆ verb `status` ตั้งใจเปิดเผยข้อมูลเมทาดาทา (paths, email, source) — **ไม่เคย** เปิดเผยค่า credential

## รูปร่างผลลัพธ์

### `status`

```json
{
  "ok": true,
  "plugin": "gcloud-auth",
  "operation": "status",
  "gcloud_cli_present": true,
  "active_source": "adc",
  "account_email": "me@example.com",
  "has_credential": true,
  "sources": {
    "adc":             { "present": true,  "path": "/Users/me/.config/gcloud/application_default_credentials.json" },
    "service_account": { "present": false, "path": null },
    "env":             { "present": false, "vars": ["BWOC_GCLOUD_ACCOUNT","BWOC_GCLOUD_PROJECT","BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT"] }
  }
}
```

`active_source` เป็นหนึ่งใน `adc | service-account | env | none` `account_email` เป็น `null` เมื่อไม่ได้ตั้งบัญชี `gcloud_cli_present` เป็น `false` เมื่อไม่มี `gcloud` บน PATH — ในโหมดนั้น `account_email` ก็เป็น `null` และ envelope ยังถูกปล่อยออกมาอย่างสะอาด (ไม่ panic)

### `login`

`login` สตรีม `gcloud auth login` ไปยัง TTY ของผู้ดูแล หลังจาก `gcloud` ออกสำเร็จ ปลั๊กอินจะปล่อยบรรทัด telemetry บรรทัดเดียวทาง stdout:

```json
{ "ok": true, "plugin": "gcloud-auth", "operation": "login", "account_email": "me@example.com" }
```

`login` ไม่ลองใหม่อัตโนมัติ ไม่จับ OAuth flow และไม่เก็บ token เอง — `gcloud` ถือ store ของ credential ที่ `~/.config/gcloud/`

## ฟังก์ชันช่วย (Shared helpers)

`gcloud.sh` export ฟังก์ชันช่วย 4 ตัวเพื่อให้ปลั๊กอินพี่น้อง `workflow/gcloud-*` source ใช้ (บันทึก §Decision 2):

| ฟังก์ชัน | คืนค่า / ผลข้างเคียง |
|---|---|
| `gcloud_assert_cli` | คืน `127` (พร้อม stderr ที่ชัดเจน) เมื่อไม่มี `gcloud` บน PATH |
| `gcloud_active_source` | echo `adc \| service-account \| env \| none` (precedence เดียวกับ `status`) |
| `gcloud_account_email` | echo อีเมลบัญชีที่ active; ว่างเมื่อยังไม่ยืนยันตัวตน **ไม่เคยพิมพ์ token** |
| `gcloud_assert_authenticated` | คืน `3` (พร้อม stderr ที่ชัดเจน) เมื่อไม่มี credential ที่ active |

ตัว dispatcher (`_gcloud_auth_main`) ถูก guard ด้วย `BASH_SOURCE` ดังนั้นการ source ไฟล์นี้เป็นการ import ที่บริสุทธิ์ — ไม่กิน stdin, ไม่รัน verb [[../gcloud-project/gcloud|`gcloud-project/gcloud.sh`]] source ไฟล์นี้ผ่าน `$BWOC_PLUGIN_DIR/../gcloud-auth/gcloud.sh` (พร้อม fallback ที่อิง path ของสคริปต์ เพื่อให้ปลั๊กอินยังทดสอบได้นอก dispatcher ของเฟรมเวิร์ก)

## คลาสข้อผิดพลาด

| Exit | คลาส | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | JSON หนึ่งอ็อบเจกต์บน stdout |
| `1` | dependency / login-blocker | ไม่มี `jq` บน PATH หรือเรียก `login` โดยไม่มี `gcloud` บน PATH |
| `2` | usage | operation ไม่รู้จัก / ไม่ได้ระบุ |
| `3` | not-authenticated | caller ใช้ `gcloud_assert_authenticated` แต่ไม่มี credential ที่ active `status` เองไม่เคยคืนรหัสนี้ |
| `127` | helper-only | คืนจาก `gcloud_assert_cli` ไปยัง caller ของมัน; dispatcher แปลงเป็น exit `1` |

การไม่มี `gcloud` CLI ล้มเหลวอย่าง **สะอาด**: ข้อความ stderr ชัดเจน + envelope `status` ที่มีโครงสร้าง (มี `gcloud_cli_present: false`); ปลั๊กอินไม่เคย panic หรือทิ้งกระบวนการในสถานะกึ่ง

## การตั้งค่า

```toml
# workspace.toml
[plugins.gcloud-auth]
enabled = true
```

ไม่มี `[config.schema]` — การ resolve credential ขับด้วย environment ไม่ใช่ workspace config (บันทึก §Decision 3) surface ระดับ workspace เพียงตัวเดียวคือคีย์สากล `enabled`

## การจับคู่วงจรชีวิต

ตาม [[../../../docs/th/PLUGINS.th#วงจรชีวิต|PLUGINS.th.md §วงจรชีวิต]] เจ้าของ kind `workflow` คือ **เอเจนต์** ที่เรียกออกไป (ผ่าน CLI `bwoc gcloud`) `init`/`teardown` เกิดต่อการเรียกแต่ละครั้งรอบ ๆ `invoke` ปลั๊กอินไม่ถือสถานะภายในใด ๆ นอกจากที่ `gcloud` CLI แคชไว้ใน `~/.config/gcloud/`

| เฟส | สิ่งที่ปลั๊กอินนี้ทำ |
|---|---|
| `init` | โดยปริยายต่อการเรียก; ตรวจว่ามี `jq` บน PATH (และ `gcloud` สำหรับ verb ที่ต้องใช้) |
| `invoke` | อ่านคำขอ, query `gcloud` (สำหรับ `login`) หรือสถานะไฟล์ในเครื่อง (สำหรับ `status`), ปล่อย JSON |
| `teardown` | โดยปริยาย; ไม่มีสถานะให้คืน |

## Idempotency

- `status` อ่านอย่างเดียว ลำดับคงที่ทั่วทุกการเล่นซ้ำ
- `login` ขับเคลื่อนโดยผู้ดูแล; การเล่นซ้ำลู่เข้าสู่สถานะที่ยืนยันแล้วเดียวกัน (หรือไม่เปลี่ยนถ้าผู้ดูแลยกเลิก) ตัวปลั๊กอินเองเป็น pass-through บาง ๆ

## ระดับวุฒิภาวะ (Maturity)

ประกาศ **L1** — ปลั๊กอินอ้างอิง `workflow/gcloud-auth` ตัวแรกที่รันได้; ทั้งสอง verb ทำงาน จะขยับเป็น **L2** เมื่อส่วนขยาย `bwoc check` (`BWOC-55`) และ smoke test ทดสอบ end-to-end กับ `gcloud` จริงในเครื่องโดยใช้ credential ของผู้ดูแล

> [!warning] ช่องว่างการทดสอบจริง การยืนยัน end-to-end กับ project GCP จริงต้องรอ SA JSON sandbox ที่ผู้ดูแลจัดหามาวางที่ `.bwoc/secrets/gcloud-sa.json` (บันทึก §Status) v0.1.0 ยืนยันด้วย: `bash -n gcloud.sh`, เส้นทาง gcloud-หายไปที่ปล่อย envelope `gcloud_cli_present:false` ที่สะอาด, เส้นทาง jq-หายไปที่ error อย่างสะอาด, `bwoc check` ที่ยอมรับ manifest และ verb `status` ที่คืน envelope ถูกต้องบน workstation เปล่าที่ยังไม่ตั้งค่า

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือ model ใด `kind = "workflow"` เป็นค่า enum ของเฟรมเวิร์กเอง คำว่า "gcloud" / "Google Cloud" ปรากฏเฉพาะใน `description` (ที่อนุญาตชื่อเป้าหมายการเชื่อมต่อตาม [[../../../docs/th/PLUGINS.th#ข้อจำกัดความเป็นกลาง (HARD)|PLUGINS.th.md §ความเป็นกลาง]]) และในเนื้อความ SPEC นี้ — ไม่เคยอยู่ใน `kind`, `entry` หรือคีย์ config สอดคล้องกับ **สมานัตตตา**

## ดูเพิ่มเติม

- [[../../../notes/2026-05-28_gcloud-workflow-plugin-architecture|บันทึกออกแบบ BWOC-51]] — framing ฉบับเต็มสำหรับ foundation ของ EPIC-8 (decisions 1–7)
- [[../gcloud-project/SPEC.th|gcloud-project SPEC.th]] — ปลั๊กอินพี่น้อง (บริบท project); source helpers จากปลั๊กอินนี้
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `workflow`
- [[auth|auth.toml]] — สัญญา auth (รูปร่างเท่านั้น ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (ความเท่าเทียมสองภาษา)
