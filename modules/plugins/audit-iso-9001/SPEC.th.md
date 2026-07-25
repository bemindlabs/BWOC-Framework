---
title: การตรวจสอบ ISO 9001 ระบบบริหารคุณภาพ
aliases:
  - audit-iso-9001
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-9001
  - status/runtime
maturity: L1
---

# การตรวจสอบ ISO 9001 ระบบบริหารคุณภาพ

> [!abstract] **Attestation runtime (v0.2.0)** อ่าน attestation ที่ operator เซ็นจาก `.bwoc/workspace.toml` ใต้ `[[plugins.audit-iso-9001.attestations]]` และส่ง finding ที่ `evidence.kind = "attestation"` (`signer` + `signed_at` + `valid_through` แบบ optional) ตาม [การขยาย schema ของ BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) Criterion ที่ไม่มี attestation จากผู้ดำเนินการจะส่ง `status = "fail"` ชี้ไปที่ block ใน `workspace.toml` แทนที่ stub v0.1.0 จาก EPIC-2

## สถานะและ Roadmap

| Version | วันที่ | การเปลี่ยนแปลง |
|---|---|---|
| v0.1.0 | 2026-05-26 | Stub ประกาศ criteria 8 รายการ ทุก finding `status = "not_implemented"` สอดคล้องกับ schema เท่านั้น — ไม่ตรวจสอบ workspace ลงใน EPIC-2 |
| v0.2.0 | 2026-05-27 | **Attestation runtime** อ่าน `[[plugins.audit-iso-9001.attestations]]` จาก `workspace.toml`; ส่ง `kind = "attestation"` สำหรับ criterion ที่มี attestation จาก operator และ `status = "fail"` ที่ชี้ไปที่ `workspace.toml` สำหรับที่เหลือ `criterion_id` ทั้ง 8 ยังคงเดิม (สัญญา stability, PLUGINS.en.md §Stability) มีการเปลี่ยนแปลงคู่กันใน `crates/bwoc-cli/src/audit.rs` ที่ขยาย schema validator ของ dispatcher ให้ยอมรับ attestation + sample evidence kinds และส่ง sub-field ผ่าน — เป็น runtime-side companion ของ BWOC-27 ฝั่ง docs ลงใน EPIC-3 BWOC-28 ดู [[../../notes/2026-05-27_9001-runtime-attestation-source]] สำหรับ design |

## ทำไมจึงเป็น Runtime ตอนนี้

[[../../notes/2026-05-26_iso-compliance-plugins|บันทึกกรอบความคิด EPIC-2]] อธิบายว่าทำไม 9001 ส่งเป็น stub ก่อน — evidence ของ 9001 เป็นเชิงองค์กร ไม่ใช่รูปแบบการมีอยู่ของไฟล์ และ schema v1 ไม่สามารถแสดง attestation ได้ EPIC-3 ปิดช่องว่างนั้น:

- [บันทึก design BWOC-26](../../../notes/2026-05-27_iso-runtime-evidence-model.md) ตรึง evidence model ใหม่ (`attestation`, `sample`, time-bounded fields)
- [BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) ขยาย schema ด้วย `attestation` (ต้องการ `signer` + `signed_at`) และ `valid_through` ที่เป็น optional แบบ orthogonal
- BWOC-28 (การเปลี่ยนแปลงนี้) implement runtime ของ 9001 ตาม schema ใหม่ Operator ให้ attestation ใน `workspace.toml`; plugin ส่ง finding หนึ่งตัวต่อ criterion ซื่อสัตย์ว่า criterion ใดมี coverage และ criterion ใดไม่มี การอนุมาน "องค์กรนี้มีนโยบายคุณภาพที่เป็นเอกสาร" จาก "repo นี้มีไฟล์ `POLICY.md`" จะยังคงทำให้ audit เป็นเท็จ (Musāvāda — [PHILOSOPHY.en.md](../../agent-template/docs/en/PHILOSOPHY.en.md) §Sila 5) Attestation evidence รักษาความซื่อสัตย์ของ audit โดยกำหนดให้ operator วาง attestation พร้อมวันที่และ provenance

## Criteria (v0.2.0)

QMS criteria หลัก 8 รายการ ดึงจาก clauses หลักของ ISO 9001:2015[^iso-9001-2015] ลำดับการประกาศใน [[criteria]] คือลำดับรายงาน (PLUGINS.en.md §Audit Findings Schema) ค่า `criterion_id` คงที่ข้ามรุ่น (PLUGINS.en.md §Stability); การเปลี่ยนชื่อเป็น major-version bump

| `criterion_id` | Clause | หัวเรื่อง | Severity |
|---|---|---|---|
| `9001-context-of-organization` | 4 | บริบทขององค์กร | high |
| `9001-leadership-and-policy` | 5.2 | ภาวะผู้นำและนโยบายคุณภาพ | high |
| `9001-risks-and-opportunities` | 6.1 | การดำเนินการกับความเสี่ยงและโอกาส | high |
| `9001-competence-and-awareness` | 7.2 | สมรรถนะและความตระหนัก | medium |
| `9001-documented-information` | 7.5 | สารสนเทศที่เป็นเอกสาร | medium |
| `9001-internal-audit` | 9.2 | การ audit ภายใน | high |
| `9001-management-review` | 9.3 | การทบทวนของผู้บริหาร | high |
| `9001-corrective-action` | 10.2 | ความไม่สอดคล้องและการดำเนินการแก้ไข | medium |

Criteria 8 รายการครอบคลุมหนึ่งแนวปฏิบัติต่อ clause หลัก (4 ถึง 10) ที่องค์กรเชิงสอดคล้อง QMS ทุกที่คาดว่าจะดำเนินการ Sub-clause ที่เหลือ (ทรัพยากร, โครงสร้างพื้นฐาน, การติดตามและวัดผลของกระบวนการปฏิบัติการ, แบบสำรวจความพึงพอใจของลูกค้า ฯลฯ) ยังไม่ได้ประกาศ — การเพิ่ม criteria ใน minor-version bump ในอนาคตคือทางเดิน

## วิธีรันงานวันนี้

```bash
bwoc audit run --plugin audit-iso-9001 --json
```

Dispatcher spawns `audit.sh` (ตาม `BWOC-12`) ด้วยสัญญา env มาตรฐาน: `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION` Runtime จะ:

1. อ่าน `criteria.toml` จาก `BWOC_PLUGIN_DIR` สำหรับ criteria ที่ประกาศ (id, severity)
2. อ่าน `.bwoc/workspace.toml` จาก `BWOC_WORKSPACE` และเดินผ่าน `[[plugins.audit-iso-9001.attestations]]` สร้างตาราง attestation ที่ index ด้วย `criterion_id`
3. สำหรับ criterion แต่ละตัวตามลำดับการประกาศ:
   - **มี attestation และครบถ้วน** (`statement` + `signer` + `signed_at`) → `status = "pass"`, `evidence.kind = "attestation"` พร้อม `value = statement`, `signer`, `signed_at`, `valid_through` แบบ optional
   - **มี attestation แต่ไม่ครบ** → `status = "fail"`, `evidence.kind = "file"` ชี้ไปที่ `.bwoc/workspace.toml`, remedy ระบุ required field(s) ที่ขาด
   - **ไม่มี attestation** → `status = "fail"`, `evidence.kind = "file"` ชี้ไปที่ `.bwoc/workspace.toml`, remedy ระบุ criterion_id และ required fields
4. ออกด้วยรหัส `0` เมื่อสำเร็จ — finding ที่ไม่ pass เป็น *finding* ไม่ใช่ error การออกด้วยรหัสไม่เป็นศูนย์บ่งบอกปัญหาฝั่ง framework (`criteria.toml` อ่านไม่ได้)

Process exit code ของ dispatcher คือจำนวน `fail` findings (BWOC-12) ดังนั้น workspace ที่ไม่มี attestation จะออกด้วย `8`, ที่มี attestation 2 ตัวจะออกด้วย `6`, และที่มีครบทั้ง 8 จะออกด้วย `0`

## ตัวอย่าง Output

Criterion ที่มี attestation:

```json
{
  "criterion_id": "9001-management-review",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "attestation",
    "value":         "Management review held 2026-04-15 covering Q1 QMS performance, customer feedback, internal audit results, improvement opportunities.",
    "signer":        "Quality Manager: Tonkla K.",
    "signed_at":     "2026-04-15",
    "valid_through": "2027-04-15"
  }
}
```

Criterion ที่ไม่มี attestation:

```json
{
  "criterion_id": "9001-internal-audit",
  "severity":     "high",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": ".bwoc/workspace.toml" },
  "remedy":       "Provide a signed attestation in .bwoc/workspace.toml under [[plugins.audit-iso-9001.attestations]] with criterion_id=\"9001-internal-audit\", statement, signer, and signed_at (ISO 8601 date) to satisfy this criterion."
}
```

`bwoc audit run` ห่อ findings array ใน envelope หลัก `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`

## การตั้งค่า

```toml
# .bwoc/workspace.toml
[plugins.audit-iso-9001]
enabled = true

[[plugins.audit-iso-9001.attestations]]
criterion_id  = "9001-management-review"
statement     = "Management review held 2026-04-15 covering Q1 QMS performance, customer feedback, internal audit results, improvement opportunities."
signer        = "Quality Manager: Tonkla K."
signed_at     = "2026-04-15"
valid_through = "2027-04-15"   # optional

[[plugins.audit-iso-9001.attestations]]
criterion_id = "9001-leadership-and-policy"
statement    = "Quality policy v1.2 ratified 2026-01-10 — aligned with strategic direction."
signer       = "Top Management: Tonkla K."
signed_at    = "2026-01-10"
```

`[[plugins.audit-iso-9001.attestations]]` แต่ละรายการเป็น **array-of-tables** ใต้ block สากล `[plugins.audit-iso-9001]` Fields:

| Field | จำเป็น | หมายเหตุ |
|---|---|---|
| `criterion_id` | ใช่ | ต้องตรงกับรายการใน `criteria.toml` (หนึ่งใน 8 `9001-*` ids ที่ประกาศ) |
| `statement` | ใช่ | ข้อความ attestation แบบ verbatim กลายเป็น `evidence.value` บน finding v0.2.0 รองรับ TOML basic string บรรทัดเดียว |
| `signer` | ใช่ | ตัวตนแบบ free-text กลายเป็น `evidence.signer` ตัวอย่าง: `"Quality Manager: Tonkla K."` |
| `signed_at` | ใช่ | ISO 8601 date (หรือ datetime) กลายเป็น `evidence.signed_at` |
| `valid_through` | optional | ISO 8601 date เมื่อ attestation หยุดเป็น authoritative กลายเป็น `evidence.valid_through` Dispatcher ประทับแต่ไม่บังคับ expiry — เป็นงานของ tooling ปลายทาง (ตาม BWOC-26) |

การประกาศ `criterion_id` ครั้งแรกชนะ; runtime ไม่ flag duplicate (เป็นงานของ `bwoc check` — ดู BWOC-29)

เลือก `workspace.toml` มากกว่าไฟล์ `attestations/9001.toml` แยก หรือ env path `BWOC_AUDIT_ATTESTATIONS` เพราะเป็น mechanism ที่มี blast radius เล็กที่สุด: operator แตะไฟล์นี้อยู่แล้วเพื่อเปิด plugin, การเปลี่ยนแปลงทุกครั้งสามารถ diff ได้ใน `git log`, และ `bwoc check` เดินผ่านไฟล์นี้อยู่แล้ว ดู [[../../notes/2026-05-27_9001-runtime-attestation-source]] สำหรับเหตุผลเต็ม

## Maturity

ประกาศ **L1** — runtime ใช้งานได้ ส่ง attestation findings จริงตาม BWOC-27 เลื่อนเป็น **L2** เมื่อ:

- `bwoc check` validate block `[[attestations]]` ของ workspace ตาม schema (BWOC-29)
- มี flag `bwoc audit report --expired` ที่แสดง `valid_through` expiry เป็น warning
- รองรับ `statement` แบบหลายบรรทัด (TOML triple-quoted form)

## ความเป็นกลาง

ค่าใน manifest ไม่ระบุ LLM backend, vendor หรือ model `kind = "audit"` เป็น enum ของ framework เอง (`BWOC-10`) ปลั๊กอินอ้างถึง "ISO 9001" เฉพาะใน `description`, ในเนื้อหา SPEC นี้, และใน namespace มาตรฐาน `9001-*` ของ criterion-id — ไม่ใช้ใน key ของ `criteria.toml` หรือในค่า finding นอกเหนือจาก namespace นั้น ผ่านกฎ **Samānattatā**

## แหล่งอ้างอิง

QMS criteria ดึงจาก clauses หลักของ ISO 9001:2015 (4 ถึง 10) ที่เผยแพร่:

- ISO 9001:2015 — *Quality management systems — Requirements.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/62085.html> [^iso-9001-2015]
- ISO/TC 176/SC 2 — *Quality management and quality assurance — Quality systems.* หน้า landing สาธารณะ: <https://www.iso.org/committee/53896.html> คณะกรรมการเทคนิคที่รับผิดชอบ ISO 9000 family
- ISO — *Quality management principles* (โบรชัวร์เปิดเผยที่สรุปหลักการ QMS ทั้ง 7 ที่รองรับการแก้ไขปี 2015): <https://www.iso.org/publication/PUB100080.html>

[^iso-9001-2015]: ISO 9001:2015 มีโครงสร้างตาม Annex SL high-level structure (clauses 4 บริบท, 5 ภาวะผู้นำ, 6 การวางแผน, 7 การสนับสนุน, 8 การปฏิบัติการ, 9 การประเมินผลการทำงาน, 10 การปรับปรุง) Criteria ทั้ง 8 ที่นี่ครอบคลุมหนึ่งแนวปฏิบัติหลักต่อ clause หลักที่องค์กรเชิงสอดคล้อง QMS ทุกที่คาดว่าจะดำเนินการ; sub-clauses ที่เหลือ (7.1 ทรัพยากร, 8.5 การจัดหาผลิตภัณฑ์และบริการ, 9.1.2 ความพึงพอใจของลูกค้า ฯลฯ) ยังไม่ได้ประกาศ — การเพิ่ม criteria ใน minor-version bump ในอนาคตคือทางเดิน

## ดูเพิ่ม

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — สเปคของ plugin; แถวของ `audit` kind (BWOC-10), Audit Findings Schema (BWOC-11 + evidence kinds BWOC-27)
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — บันทึกกรอบความคิด EPIC-2 (ทำไม stub)
- [[../../notes/2026-05-27_iso-runtime-evidence-model|2026-05-27_iso-runtime-evidence-model.md]] — design ของ BWOC-26 (attestation, sample, time-bounded fields)
- [[../../notes/2026-05-27_9001-runtime-attestation-source|2026-05-27_9001-runtime-attestation-source.md]] — design ของ BWOC-28 (mechanism แหล่ง attestation + dispatcher reach)
- [[../audit-iso-29110/SPEC|audit-iso-29110]] — ปลั๊กอิน audit อ้างอิงที่รันได้จาก BWOC-13
- [[../audit-iso-20000-1/SPEC|audit-iso-20000-1]], [[../audit-iso-27001/SPEC|audit-iso-27001]] — stub พี่น้อง (runtime กำหนดสำหรับ S5)
