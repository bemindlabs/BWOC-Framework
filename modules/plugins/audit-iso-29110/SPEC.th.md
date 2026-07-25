---
title: การตรวจสอบ ISO/IEC 29110 Basic-Profile
aliases:
  - audit-iso-29110
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-29110
maturity: L1
---

# การตรวจสอบ ISO/IEC 29110 Basic-Profile

> [!abstract] ปลั๊กอินอ้างอิงตัวแรกของ kind `audit` ที่ **รันได้จริง** ตรวจสอบการมีอยู่ของไฟล์ตามรายการ work product ของ Basic profile ที่กำหนดใน **ISO/IEC TR 29110-5-1-2** (วิศวกรรมซอฟต์แวร์ — Lifecycle profiles สำหรับ Very Small Entities — คู่มือบริหารและวิศวกรรม: Generic profile group: Basic profile) ส่ง findings ตามสคีมา BWOC-11 ผ่านการสั่ง `bwoc audit run`

## ทำไมต้องเป็น ISO/IEC 29110 ก่อน

ISO/IEC 29110 เหมาะเป็นปลั๊กอิน `audit` แบบรันได้ตัวแรก ด้วยเหตุผลที่บันทึกไว้ใน [[../../notes/2026-05-26_iso-compliance-plugins|บันทึกกรอบความคิด EPIC-2]]:

- **ขนาดเหมาะสม** Basic profile ออกแบบสำหรับ Very Small Entities (VSE) ขนาด 1–25 คน ตรงกับบริบทของ workspace BWOC ที่มี operator คนเดียวบวกกับทีม agent ขนาดเล็ก ส่วนกรอบ QMS / ITSM / ISMS (`audit-iso-9001`, `audit-iso-20000-1`, `audit-iso-27001`) อธิบายแนวปฏิบัติเชิงองค์กรที่ไม่สามารถลดทอนเป็นการเช็คการมีอยู่ของไฟล์ได้ จึงส่งเป็น stub ใน Sprint 3 และจะได้รับ runtime ใน `BWOC-EPIC-3`
- **หลักฐานที่ตรวจสอบโดยเครื่องได้** กระบวนการ Project Management (PM) และ Software Implementation (SI) ของ Basic profile ระบุ *work product* ที่จับต้องได้ (Project Plan, SRS, Design, Test Plan, Verification Results, Construction Records) ซึ่งสอดคล้องกับไฟล์ใน workspace ของซอฟต์แวร์ทั่วไป การเช็คการมีอยู่ของไฟล์คือ runtime audit ที่เรียบง่ายที่สุด — ปลั๊กอินจึงรันได้โดยไม่ต้องพึ่งเครือข่าย, การเรียก shell ภายนอก, หรือเครื่องมือเพิ่มเติม
- **บังคับให้สคีมาทำงานจริง** การรัน audit จริงกับมาตรฐานจริงเป็นการทดสอบสคีมา findings ของ BWOC-11 ก่อนที่ stub สามตัวใน Sprint 3 จะปรับให้สอดคล้องตามมาตรฐาน เส้นทาง `pass`, `fail`, และ (เมื่อมี) `not_applicable` ทั้งหมดมาจากปลั๊กอินตัวนี้

## Work Product ที่ตรวจสอบ (v0.1.0)

ตรวจ work product ของ Basic profile 6 รายการใน v0.1.0 — เป็นค่ากลางๆ ของช่วง "5–8 criteria" ที่ brief ของ BWOC-13 กำหนด และแต่ละรายการสอดคล้องกับ artifact หนึ่งชื่อที่โครงการระดับ VSE ทั่วไปจะผลิตขึ้น ตาราง Task Output ของ Basic profile ทั้งหมด[^iso-29110-5-1-2] ระบุ work product ~22 รายการใน PM และ SI; v0.1.0 ตั้งใจครอบคลุม *รายการหลัก* ที่ทุกโครงการ Basic profile คาดว่าจะมี เวอร์ชันต่อๆ ไปจะขยายรายการได้โดยไม่ละเมิดสัญญาเสถียรภาพของ BWOC-11 (การเพิ่ม criterion เป็น minor-version bump ตาม `[plugin].version`)

| `criterion_id` | Process | ISO Work Product | Severity | Path หลัก |
|---|---|---|---|---|
| `29110-bp-project-plan` | PM | Project Plan | high | `docs/en/PROJECT-PLAN.en.md` |
| `29110-bp-software-requirements-specification` | SI | Software Requirements Specification | high | `docs/en/SRS.en.md` |
| `29110-bp-software-design` | SI | Software Design | medium | `docs/en/DESIGN.en.md` |
| `29110-bp-software-test-plan` | SI | Software Test Plan | medium | `docs/en/TEST-PLAN.en.md` |
| `29110-bp-verification-results` | SI | Verification Results | medium | `docs/en/VERIFICATION.en.md` |
| `29110-bp-software-construction-records` | SI | Software Construction Records | low | `CHANGELOG.md` |

ทุก criterion ประกาศ `candidates` แบบเรียงลำดับใน [[criteria]] — รายการ path สำรองที่ audit จะยอมรับ (เช่น `docs/SRS.md` หรือ `REQUIREMENTS.md` สำหรับ work product SRS) candidate ตัวแรกที่มีอยู่จริงจะชนะ; ถ้าไม่มีเลย finding จะเป็น `status = "fail"` และ remedy อ้างถึง path หลัก ค่า `criterion_id` คงที่ข้ามรุ่น (PLUGINS.en.md §Stability) — การเปลี่ยนชื่อเป็น major-version bump ตาม semver ของปลั๊กอินเอง

> [!note] **Severity สะท้อนความสำคัญของ criterion ไม่ใช่ผลลัพธ์** finding ที่มี `critical` กับ `status = "pass"` เป็นเรื่องปกติ — หมายถึง "เราเช็คสิ่งสำคัญที่สุดแล้ว มันโอเค" Severity ประกาศครั้งเดียวใน `criteria.toml` ไม่ตัดสินใจต่อรอบ

## วิธีรันงาน

ปลั๊กอินถูกเรียกผ่าน `bwoc audit run` (ตาม `BWOC-12`):

```bash
bwoc audit run --plugin audit-iso-29110 --json
```

`bwoc audit` spawns `audit.sh` จาก directory นี้ด้วย input ดังนี้:

| Channel | สิ่งที่ส่ง |
|---|---|
| `BWOC_WORKSPACE` (env) | Path แบบ absolute ของ workspace root ที่กำลังถูก audit |
| `BWOC_PLUGIN_DIR` (env) | Path แบบ absolute ของ directory ปลั๊กอินตัวนี้ (ที่ `criteria.toml` อยู่) |
| `BWOC_AUDIT_OPERATION` (env) | ชื่อ operation; v1 จะเป็น `audit_run` เสมอ |
| stdin | `{"operation":"audit_run","workspace":"<abs>","plugin_dir":"<abs>"}` — context เดียวกับ env vars สคริปต์ไม่อ่าน stdin; env vars เป็น channel หลัก |

สคริปต์จะ:

1. อ่าน `criteria.toml` จาก `BWOC_PLUGIN_DIR` (รูปทรง TOML แบบจำกัด — scalar และ array บรรทัดเดียว — parse ด้วย `awk`)
2. สำหรับ criterion แต่ละตัวตามลำดับการประกาศ ตรวจสอบรายการ `candidates` กับ `BWOC_WORKSPACE`
3. ส่ง finding หนึ่งรายการต่อ criterion ออกไปทาง stdout เป็น JSON array ที่สอดคล้องกับ [PLUGINS.en.md §Audit Findings Schema](../../../docs/en/PLUGINS.en.md#audit-findings-schema):
   - Candidate แรกที่มีอยู่ → `status = "pass"`, `evidence = { kind: "file", value: <found path> }`, ไม่มี `remedy`
   - ไม่มีเลย → `status = "fail"`, `evidence = { kind: "file", value: <primary path> }`, `remedy = "Create <primary> … (or one of: <alts>)"`

สคริปต์ออกด้วยรหัส `0` เมื่อสำเร็จ **finding ที่ไม่ใช่ pass เป็น finding ไม่ใช่ error** การออกด้วยรหัสไม่ใช่ศูนย์บ่งบอกถึงปัญหาฝั่ง framework (env var หาย, อ่าน `criteria.toml` ไม่ได้) ซึ่ง dispatcher ของ BWOC-12 จะถือว่าเป็น bug ของปลั๊กอิน — ดู [PLUGINS.en.md §Audit Findings Schema](../../../docs/en/PLUGINS.en.md#audit-findings-schema)

## ตัวอย่าง Output

สำหรับ workspace ที่มี `docs/en/SRS.en.md` และ `docs/en/ARCHITECTURE.en.md` แต่ขาดที่เหลือ ปลั๊กอินจะส่งออกประมาณนี้:

```json
[
  {
    "criterion_id": "29110-bp-project-plan",
    "severity": "high",
    "status": "fail",
    "evidence": { "kind": "file", "value": "docs/en/PROJECT-PLAN.en.md" },
    "remedy": "Create docs/en/PROJECT-PLAN.en.md documenting the Project Plan work product (or one of: docs/PROJECT-PLAN.md, PROJECT-PLAN.md). Project plan capturing scope, schedule, resources, risks, and acceptance criteria."
  },
  {
    "criterion_id": "29110-bp-software-requirements-specification",
    "severity": "high",
    "status": "pass",
    "evidence": { "kind": "file", "value": "docs/en/SRS.en.md" }
  },
  {
    "criterion_id": "29110-bp-software-design",
    "severity": "medium",
    "status": "pass",
    "evidence": { "kind": "file", "value": "docs/en/ARCHITECTURE.en.md" }
  }
]
```

`bwoc audit run` ห่อ output นี้ไว้ใน envelope หลัก `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }` — เป็นความรับผิดชอบของ dispatcher ไม่ใช่ของปลั๊กอิน Findings serialise ตาม **ลำดับการประกาศของ criterion** ซึ่งคือลำดับแถวของ `criteria.toml` (PLUGINS.en.md §Audit Findings Schema)

## การตั้งค่า

```toml
# workspace.toml
[plugins.audit-iso-29110]
enabled = true
```

ปลั๊กอินไม่ประกาศ `[config.schema]` ใน manifest — interface ระดับ workspace มีแค่ key `enabled` แบบสากล เวอร์ชันต่อไปอาจเพิ่ม key `profile` (Entry / Basic / Intermediate) เมื่อ scope ของ 29110 profile อื่นๆ ชัดเจน

## การจับคู่ Lifecycle

ตาม [PLUGINS.en.md §Lifecycle](../../../docs/en/PLUGINS.en.md#lifecycle) เจ้าของ kind `audit` คือ CLI `bwoc audit`; `init` กับ `teardown` เกิดขึ้นรอบ `invoke` แต่ละครั้ง ปลั๊กอินนี้ไม่ถือ **state ภายนอก** — ทุก phase เป็น idempotent โดยปริยาย:

| Phase | สิ่งที่ปลั๊กอินนี้ทำ |
|---|---|
| `init` | (ทำโดยปริยายต่อการเรียก; ไม่ต้องตั้งค่าอะไรก่อน `invoke`) |
| `invoke` | อ่าน `criteria.toml`, probe `BWOC_WORKSPACE`, ส่ง findings JSON ออก stdout |
| `teardown` | (ทำโดยปริยายต่อการเรียก; ไม่ต้องคืนทรัพยากร) |

รันซ้ำกับ workspace เดียวกันให้ผล findings array เหมือนเดิม — `[file -e ...]` เป็น read-only และเรียงลำดับคงที่

## Maturity

ประกาศ **L1** — ปลั๊กอิน audit ที่รันได้ตัวแรก, 6 criteria, ยังไม่มี operator ในชีวิตจริงยืนยัน เลื่อนเป็น L2 เมื่อใช้ end-to-end กับ workspace ของ operator BWOC อย่างน้อยหนึ่งคนแล้ว; เป็น L3 เมื่อ integration test ใน `crates/bwoc-cli/tests/` เรียกปลั๊กอินนี้เป็น fixture

## ความเป็นกลาง

ค่าใน manifest ไม่ระบุ LLM backend, vendor หรือ model `kind = "audit"` เป็น enum ของ framework เอง (`BWOC-10`) ปลั๊กอินอ้างถึง "ISO/IEC 29110" เฉพาะใน `description` (ที่ยอมให้ใช้ชื่อแบบ vendor ตาม PLUGINS.en.md §Neutrality constraint) และในเนื้อหา SPEC นี้ — ไม่ใช้ใน key ของ `criteria.toml`, file path หรือ `criterion_id` ของ finding นอกเหนือจาก namespace มาตรฐาน `29110-bp-*` ผ่านกฎ **Samānattatā**

## สถานะ & Roadmap

- **v0.1.0** (เวอร์ชันนี้): work product ของ Basic profile 6 รายการ, เช็คการมีอยู่ของไฟล์, bash entry
- **v0.2.0** (วางแผน): ขยาย coverage ให้ครบตาราง Task Output ของ Basic profile (~22 work products); เพิ่มเส้นทาง `not_applicable` สำหรับ VSE profile แต่ละแบบ (Entry vs Basic vs Intermediate)
- **v0.3.0** (วางแผน): เช็ค `evidence.kind = "content"` เพื่อดูว่า section ที่จำเป็นมีอยู่ใน work product (เช่น "SRS มี § Non-functional requirements")
- ที่ไม่อยู่ใน scope ของ `BWOC-EPIC-2`: conformance เชิงลึก ("Project Plan อ้างถึง SRS หรือไม่", "Verification Results record ถูก sign off แล้วหรือยัง") ต้องการ evidence model ที่รวยกว่าซึ่งเลื่อนไปที่ `BWOC-EPIC-3`

## แหล่งอ้างอิง

รายการ work product ของ Basic profile มาจากแหล่งอ้างอิงสาธารณะดังนี้:

- ISO/IEC TR 29110-5-1-2:2011 — *Software engineering — Lifecycle profiles for Very Small Entities (VSEs) — Part 5-1-2: Management and engineering guide: Generic profile group: Basic profile.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/51153.html> [^iso-29110-5-1-2]
- ISO/IEC 29110 — *Standards and guides for Very Small Entities (VSEs).* หน้า landing สาธารณะ: <https://www.iso.org/committee/4909141.html>
- ISO/IEC 29110-4-1:2018 — *Profile specifications: Generic profile group.* ISO catalogue entry: <https://www.iso.org/standard/62711.html>
- Laporte, C. Y., O'Connor, R. V., García Paucar, L. H. (2015). "The Implementation of ISO/IEC 29110 Software Engineering Standards and Guides in Very Small Entities." *Lecture Notes in Computer Science*, vol. 599 สรุประดับ operator ที่มีประโยชน์ของ work product ใน Basic profile

[^iso-29110-5-1-2]: ตาราง Task Output ของ Basic profile ทั้งหมดอยู่ใน ISO/IEC TR 29110-5-1-2 §8 ("Software Implementation") และ §7 ("Project Management") criteria ทั้ง 6 ที่นี่ครอบคลุม work product หลักที่ทุกโครงการ Basic profile คาดว่าจะผลิต; work product ที่เหลือ ~16 รายการ (Acceptance Record, Change Request, Correction Register, Software Configuration, ฯลฯ) เลื่อนไปขยายใน v0.2.0

## ดูเพิ่ม

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — สเปคของ plugin; แถวของ `audit` kind (BWOC-10), Audit Findings Schema (BWOC-11)
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — บันทึกกรอบความคิด EPIC-2 (ทำไม `audit`, ทำไม 29110 ก่อน)
- [[../memory-tier2-noop/SPEC|memory-tier2-noop]] — ปลั๊กอินอ้างอิงพี่น้อง (kind ต่างกัน, substrate เดียวกัน)
- [[../../crates/bwoc-cli/src/audit|crates/bwoc-cli/src/audit.rs]] — dispatcher ที่เรียกปลั๊กอินตัวนี้
