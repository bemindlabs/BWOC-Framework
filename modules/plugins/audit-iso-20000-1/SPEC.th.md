---
title: การตรวจสอบ ISO/IEC 20000-1 ระบบบริหารบริการ IT
aliases:
  - audit-iso-20000-1
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-20000-1
  - status/runtime
maturity: L1
---

# การตรวจสอบ ISO/IEC 20000-1 ระบบบริหารบริการ IT

> [!abstract] **Attestation + sample runtime (v0.2.0)** runtime ตัวแรกที่ผสม evidence สองชนิด แต่ละ criterion ประกาศ `expected_evidence_kind` ใน [[criteria]]; runtime อ่าน evidence ที่ operator ให้จาก `.bwoc/workspace.toml` แล้วส่ง `evidence.kind = "attestation"` สำหรับ clause ที่เป็นเอกสาร (scope, policy, catalogue) และ `evidence.kind = "sample"` สำหรับ clause เชิงอัตรา (SLA, change, incident, problem, improvement) ตาม [schema ของ BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) Criterion ที่ไม่มี evidence จาก operator จะส่ง `status = "fail"` ชี้ไปที่ `workspace.toml` แทนที่ stub v0.1.0 จาก EPIC-2

## สถานะและ Roadmap

| Version | วันที่ | การเปลี่ยนแปลง |
|---|---|---|
| v0.1.0 | 2026-05-26 | Stub ประกาศ criteria 8 รายการ ทุก finding `status = "not_implemented"` สอดคล้องกับ schema เท่านั้น — ไม่ตรวจสอบ workspace ลงใน EPIC-2 |
| v0.2.0 | 2026-05-27 | **Attestation + sample runtime** อ่าน `[[plugins.audit-iso-20000-1.attestations]]` และ `[[plugins.audit-iso-20000-1.samples]]` จาก `workspace.toml`; route แต่ละ criterion ตาม `expected_evidence_kind` (ประกาศใน `criteria.toml`) แล้วส่ง finding แบบ `attestation` หรือ `sample` พร้อม `status = "fail"` + remedy ที่ชี้ไปที่ `workspace.toml` สำหรับ criterion ที่ไม่มี evidence `criterion_id` ทั้ง 8 ยังคงเดิม (สัญญา stability, PLUGINS.en.md §Stability) ไม่ต้องแก้ dispatcher — `crates/bwoc-cli/src/audit.rs` validate ทั้งสองชนิดอยู่แล้ว (ขยายใน BWOC-28) ลงใน EPIC-3 BWOC-33 ดู [[../../notes/2026-05-27_20000-1-sample-source]] สำหรับ design |

## ทำไมจึงเป็น Runtime ตอนนี้

[[../../notes/2026-05-26_iso-compliance-plugins|บันทึกกรอบความคิด EPIC-2]] อธิบายว่าทำไม 20000-1 ส่งเป็น stub ก่อน — evidence ของมันคือแนวปฏิบัติบริหารบริการ (incident, change, SLA, policy) ที่อยู่ในเครื่องมือ ITSM ไม่ใช่ไฟล์ workspace และ schema v1 แสดงมันไม่ได้ EPIC-3 ปิดช่องว่างนั้น:

- [บันทึก design BWOC-26](../../../notes/2026-05-27_iso-runtime-evidence-model.md) ตรึง evidence model ใหม่ (`attestation`, `sample`, time-bounded fields)
- [BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) ขยาย schema ด้วย `attestation` (ต้องการ `signer` + `signed_at`) และ `sample` (ต้องการ `sampled_count` + `sampled_of`, optional `window`)
- BWOC-28 สร้าง attestation runtime ของ 9001 และขยาย dispatcher ให้ validate ทั้งสองชนิดใหม่
- BWOC-33 (การเปลี่ยนแปลงนี้) สร้าง runtime ของ 20000-1 — ตัวแรกที่ผสมทั้งสองชนิด การอนุมาน "องค์กรนี้จัดการ incident ภายใน SLA" จาก "repo นี้มีไฟล์ `INCIDENTS.md`" จะยังทำให้ audit เป็นเท็จ (Musāvāda — [PHILOSOPHY.en.md](../../agent-template/docs/en/PHILOSOPHY.en.md) §Sila 5) Attestation รักษาความซื่อสัตย์ของ clause เชิงเอกสาร (operator วาง sign-off พร้อมวันที่และ provenance); sample รักษาความซื่อสัตย์ของ clause เชิงปฏิบัติการ (operator บันทึกอัตราที่วัดได้จากเครื่องมือ ITSM)

## Criteria (v0.2.0)

ITSM criteria หลัก 8 รายการ ดึงจาก clauses หลักของ ISO/IEC 20000-1:2018[^iso-20000-1-2018] ลำดับการประกาศใน [[criteria]] คือลำดับรายงาน (PLUGINS.en.md §Audit Findings Schema) ค่า `criterion_id` คงที่ข้ามรุ่น (PLUGINS.en.md §Stability); การเปลี่ยนชื่อเป็น major-version bump คอลัมน์ **Kind** คือ `expected_evidence_kind` ของแต่ละ criterion — ดู [[../../notes/2026-05-27_20000-1-sample-source]] สำหรับเหตุผลการแบ่ง documented-artifact กับ operational-rate

| `criterion_id` | Clause | หัวเรื่อง | Severity | Kind |
|---|---|---|---|---|
| `20000-1-service-management-system-scope` | 4.3 | ขอบเขตของระบบบริหารบริการ | high | attestation |
| `20000-1-service-policy-and-objectives` | 5.2 | นโยบายและวัตถุประสงค์การบริหารบริการ | high | attestation |
| `20000-1-service-catalogue` | 8.3.1 | Service catalogue | medium | attestation |
| `20000-1-service-level-management` | 8.3.3 | การบริหารระดับการบริการ | high | sample |
| `20000-1-change-management` | 8.5.1 | การบริหารการเปลี่ยนแปลง | high | sample |
| `20000-1-incident-management` | 8.6.1 | การบริหาร incident | high | sample |
| `20000-1-problem-management` | 8.6.3 | การบริหาร problem | medium | sample |
| `20000-1-continual-improvement` | 10.2 | การปรับปรุงอย่างต่อเนื่อง | medium | sample |

attestation 3 รายการ + sample 5 รายการ การแบ่งใช้กฎเดียว: criterion ที่เป็นเอกสาร (scope, policy, catalogue ที่ operator รับรอง) ใช้ `attestation`; criterion เชิงอัตรา (N จาก M รายการผ่านเกณฑ์ตามช่วงเวลา) ใช้ `sample` Sub-clauses 8.2 (service portfolio), 8.4 (supply & demand), และ 8.7 (service assurance) ยังเลื่อน — การเพิ่ม criteria เป็น minor-version bump ไม่ใช่การเปลี่ยนชื่อ id ที่มีอยู่

## วิธีรันงาน

```bash
bwoc audit run --plugin audit-iso-20000-1 --json
```

Dispatcher spawns `audit.sh` (ตาม `BWOC-12`) ด้วยสัญญา env มาตรฐาน: `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION` Runtime จะ:

1. อ่าน `criteria.toml` จาก `BWOC_PLUGIN_DIR` สำหรับ criteria ที่ประกาศ (id, severity, **`expected_evidence_kind`**)
2. อ่าน `.bwoc/workspace.toml` จาก `BWOC_WORKSPACE` และสร้างตาราง lookup สองชุดที่ index ด้วย `criterion_id` — ชุดหนึ่งจาก `[[plugins.audit-iso-20000-1.attestations]]` อีกชุดจาก `[[plugins.audit-iso-20000-1.samples]]`
3. สำหรับ criterion แต่ละตัวตามลำดับการประกาศ route ตาม `expected_evidence_kind`:
   - **`attestation`** — มีและครบ (`statement` + `signer` + `signed_at`) → `status = "pass"`, `evidence.kind = "attestation"` พร้อม `valid_through` แบบ optional มีแต่ไม่ครบ หรือไม่มี → `status = "fail"`, `evidence.kind = "file"` ชี้ไปที่ `.bwoc/workspace.toml`
   - **`sample`** — มีและถูกต้อง (`summary` + integer `sampled_count` + integer `sampled_of`, `0 ≤ count ≤ of`) → `status = "pass"`, `evidence.kind = "sample"` พร้อม `window` แบบ optional มีแต่ไม่ครบ/ไม่ถูกต้อง หรือไม่มี → `status = "fail"`, `evidence.kind = "file"`
4. ออกด้วยรหัส `0` เมื่อสำเร็จ — finding ที่ไม่ pass เป็น *finding* ไม่ใช่ error การออกด้วยรหัสไม่เป็นศูนย์บ่งบอกปัญหาฝั่ง framework (`criteria.toml` อ่านไม่ได้)

Runtime **ไม่** กำหนด threshold SLA กับ sample: sample ที่บันทึกไว้คือ evidence และ pass ส่วนอัตรา (`"49 of 50 …"`) ถูกแสดงให้ผู้ตรวจที่เป็นมนุษย์พิจารณา Threshold เป็นนโยบายองค์กร — runtime แสดง evidence ที่ทำซ้ำได้ tooling ปลายทางตัดสิน (ตาม BWOC-26) Process exit code ของ dispatcher คือจำนวน `fail` findings (BWOC-12) ดังนั้น workspace ที่ไม่มี evidence จะออกด้วย `8`, ที่มี evidence 4 criterion จะออกด้วย `4`, และที่ครบทั้ง 8 จะออกด้วย `0`

## ตัวอย่าง Output

Sample criterion ที่มีอัตราบันทึกไว้:

```json
{
  "criterion_id": "20000-1-incident-management",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "sample",
    "value":         "49 of 50 incidents resolved within SLA",
    "sampled_count": 49,
    "sampled_of":    50,
    "window":        "2026-Q1"
  }
}
```

Attestation criterion:

```json
{
  "criterion_id": "20000-1-service-policy-and-objectives",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "attestation",
    "value":         "Service management policy v2.1 ratified 2026-01-15; objectives reviewed quarterly.",
    "signer":        "Service Owner: Tonkla K.",
    "signed_at":     "2026-01-15",
    "valid_through": "2027-01-15"
  }
}
```

Criterion ที่ไม่มี evidence จาก operator (ในที่นี้เป็น sample criterion):

```json
{
  "criterion_id": "20000-1-service-level-management",
  "severity":     "high",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": ".bwoc/workspace.toml" },
  "remedy":       "Provide a recorded sample in .bwoc/workspace.toml under [[plugins.audit-iso-20000-1.samples]] with criterion_id=\"20000-1-service-level-management\", summary, sampled_count, and sampled_of (integers) to satisfy this criterion."
}
```

`bwoc audit run` ห่อ findings array ใน envelope หลัก `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`

## การตั้งค่า

```toml
# .bwoc/workspace.toml
[plugins.audit-iso-20000-1]
enabled = true

# attestation criteria (4.3 scope, 5.2 policy, 8.3.1 catalogue)
[[plugins.audit-iso-20000-1.attestations]]
criterion_id  = "20000-1-service-policy-and-objectives"
statement     = "Service management policy v2.1 ratified 2026-01-15; objectives reviewed quarterly."
signer        = "Service Owner: Tonkla K."
signed_at     = "2026-01-15"
valid_through = "2027-01-15"   # optional

# sample criteria (8.3.3 SLA, 8.5.1 change, 8.6.1 incident, 8.6.3 problem, 10.2 improvement)
[[plugins.audit-iso-20000-1.samples]]
criterion_id  = "20000-1-incident-management"
summary       = "49 of 50 incidents resolved within SLA"
sampled_count = 49
sampled_of    = 50
window        = "2026-Q1"   # optional
```

ทั้งสอง block เป็น **array-of-tables** ใต้ block สากล `[plugins.audit-iso-20000-1]`

Fields ของ `[[…attestations]]`:

| Field | จำเป็น | หมายเหตุ |
|---|---|---|
| `criterion_id` | ใช่ | ต้องตรงกับ criterion ชนิด `attestation` ใน `criteria.toml` |
| `statement` | ใช่ | ข้อความ attestation แบบ verbatim → `evidence.value` TOML basic string บรรทัดเดียว |
| `signer` | ใช่ | ตัวตนแบบ free-text → `evidence.signer` |
| `signed_at` | ใช่ | ISO 8601 date (หรือ datetime) → `evidence.signed_at` |
| `valid_through` | optional | ISO 8601 date หมดอายุ → `evidence.valid_through` Dispatcher ประทับแต่ไม่บังคับ expiry (ตาม BWOC-26) |

Fields ของ `[[…samples]]`:

| Field | จำเป็น | หมายเหตุ |
|---|---|---|
| `criterion_id` | ใช่ | ต้องตรงกับ criterion ชนิด `sample` ใน `criteria.toml` |
| `summary` | ใช่ | สรุปสั้นสำหรับมนุษย์ → `evidence.value` (เช่น `"49 of 50 incidents resolved within SLA"`) |
| `sampled_count` | ใช่ | จำนวน N ที่วัดจริง → `evidence.sampled_count` |
| `sampled_of` | ใช่ | จำนวนประชากร M → `evidence.sampled_of` ต้อง `≥ sampled_count` |
| `window` | optional | ช่วงเวลาแบบ free-text → `evidence.window` (เช่น `"2026-Q1"`, `"last 90 days"`) |

การประกาศ `criterion_id` ครั้งแรกชนะ; runtime ไม่ flag duplicate (เป็นงานของ `bwoc check` — BWOC-29) Operator คัดลอกอัตรา sample จากเครื่องมือ ITSM (incident tracker, change board, SLA dashboard) มาใส่ใน block ที่ commit ไว้ — v0.2.0 ไม่ query ระบบ ticket โดยตรง เลือก `workspace.toml` มากกว่าไฟล์แยกหรือ env path ด้วยเหตุผล blast radius เล็กที่สุดเดียวกับแหล่ง attestation ของ 9001: operator แตะไฟล์นี้อยู่แล้ว, การเปลี่ยนแปลงทุกครั้ง diff ได้ใน `git log`, และ `bwoc check` เดินผ่านไฟล์นี้อยู่แล้ว ดู [[../../notes/2026-05-27_20000-1-sample-source]] สำหรับเหตุผลเต็ม

## Maturity

ประกาศ **L1** — runtime ใช้งานได้ ส่ง attestation + sample findings จริงตาม BWOC-27 เลื่อนเป็น **L2** เมื่อ:

- `bwoc check` validate block `[[attestations]]` / `[[samples]]` ของ workspace ตาม schema (ขยาย BWOC-29 จาก `criteria.toml` ไปยัง `workspace.toml`)
- Adapter ของเครื่องมือ ITSM จริง (Jira Service Management, ServiceNow, CSV exports) ตอบ sample query โดยตรง ลบขั้นตอนการคัดลอกด้วยมือ
- flag `bwoc audit report --below <pct>` threshold อัตรา sample เทียบกับเป้าหมายที่ operator ประกาศ

## ความเป็นกลาง

ค่าใน manifest ไม่ระบุ LLM backend, vendor หรือ model `kind = "audit"` เป็น enum ของ framework เอง (`BWOC-10`) ปลั๊กอินอ้างถึง "ISO/IEC 20000-1" เฉพาะใน `description`, ในเนื้อหา SPEC นี้, และใน namespace มาตรฐาน `20000-1-*` ของ criterion-id — ไม่ใช้ใน key ของ `criteria.toml` หรือในค่า finding นอกเหนือจาก namespace นั้น ผ่านกฎ **Samānattatā**

## แหล่งอ้างอิง

ITSM criteria ดึงจาก clauses หลักของ ISO/IEC 20000-1:2018 (4 ถึง 10) ที่เผยแพร่:

- ISO/IEC 20000-1:2018 — *Information technology — Service management — Part 1: Service management system requirements.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/70636.html> [^iso-20000-1-2018]
- ISO/IEC JTC 1/SC 40 — *IT service management and IT governance.* คณะกรรมการเทคนิคร่วมที่รับผิดชอบ ISO/IEC 20000 family: <https://www.iso.org/committee/5013818.html>
- ISO/IEC 20000-10:2018 — *Concepts and vocabulary* (excerpted ใน online browsing platform ของ ISO สำหรับ terminology เท่านั้น): มีประโยชน์ในการแยกแยะคำว่า "service", "SMS", และ "SLA"

[^iso-20000-1-2018]: ISO/IEC 20000-1:2018 มีโครงสร้างตาม Annex SL high-level structure (clauses 4 บริบท, 5 ภาวะผู้นำ, 6 การวางแผน, 7 การสนับสนุน, 8 การปฏิบัติการ, 9 การประเมินผลการทำงาน, 10 การปรับปรุง) Clause 8 (Operation of the SMS) เป็น clause แนวปฏิบัติ ITSM หลัก และมี 7 sub-clauses (8.1 operational planning, 8.2 service portfolio, 8.3 relationship & agreement, 8.4 supply & demand, 8.5 service design build & transition, 8.6 resolution & fulfilment, 8.7 service assurance) Criteria ทั้ง 8 ที่นี่ครอบคลุมหนึ่งแนวปฏิบัติหลักจาก clauses 4, 5, 8.3, 8.5, 8.6, และ 10; sub-clauses ที่เหลือ (8.2, 8.4, 8.7, รวมทั้ง 8.3 และ 8.6 child หลายตัว) เลื่อนไปขยายใน minor-version ถัดไป

## ดูเพิ่ม

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — สเปคของ plugin; แถวของ `audit` kind (BWOC-10), Audit Findings Schema (BWOC-11 + evidence kinds BWOC-27)
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — บันทึกกรอบความคิด EPIC-2 (ทำไม stub)
- [[../../notes/2026-05-27_iso-runtime-evidence-model|2026-05-27_iso-runtime-evidence-model.md]] — design ของ BWOC-26 (attestation, sample, time-bounded fields)
- [[../../notes/2026-05-27_20000-1-sample-source|2026-05-27_20000-1-sample-source.md]] — design ของ BWOC-33 (การแบ่ง evidence kind + mechanism แหล่ง sample)
- [[../audit-iso-29110/SPEC|audit-iso-29110]] — ปลั๊กอิน audit อ้างอิงที่รันได้จาก BWOC-13
- [[../audit-iso-9001/SPEC|audit-iso-9001]] — attestation runtime พี่น้อง (BWOC-28); [[../audit-iso-27001/SPEC|audit-iso-27001]] — stub พี่น้อง (runtime กำหนดถัดไป)
