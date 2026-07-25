---
title: การตรวจสอบ ISO/IEC 27001 ระบบบริหารความมั่นคงสารสนเทศ
aliases:
  - audit-iso-27001
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-27001
  - status/runtime
maturity: L1
---

# การตรวจสอบ ISO/IEC 27001 ระบบบริหารความมั่นคงสารสนเทศ

> [!abstract] **Runtime แบบ attestation + sample ที่ขับเคลื่อนด้วย SoA (v0.2.0)** เป็น ISMS runtime ตัวสุดท้ายของ EPIC-3 — ปิด epic แต่ละ criterion ประกาศ `expected_evidence_kind` ของตนใน [[criteria]]; runtime อ่าน evidence ที่ operator ให้จาก `.bwoc/workspace.toml` แล้วส่ง `evidence.kind = "attestation"` สำหรับ clauses main-body ของระบบบริหาร (scope, policy, risk assessment, SoA, internal audit) และ `evidence.kind = "sample"` สำหรับ Annex A controls (access, incident, continuity) ตาม [สคีมา BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) 27001 เป็น runtime ตัวเดียวที่ **ประชากรการ sample ถูกประกาศโดย operator**: [Statement of Applicability](https://www.iso.org/standard/27001) (clause 6.1.3) ตัดสินว่า Annex A control ใดอยู่ใน scope และ runtime sample จากชุดนั้น control ที่ถูกยกเว้นอย่างมีเหตุผลส่ง `status = "not_applicable"`; control ที่ไม่มีใน SoA หรือไม่มี sample ส่ง `status = "fail"` ชี้ไปที่ `workspace.toml` แทนที่ stub v0.1.0 จาก EPIC-2

## สถานะและ Roadmap

| รุ่น | วันที่ | การเปลี่ยนแปลง |
|---|---|---|
| v0.1.0 | 2026-05-26 | Stub ประกาศ criteria 8 รายการ; ทุก finding `status = "not_implemented"` สอดคล้องกับสคีมาเท่านั้น — ไม่ตรวจสอบ workspace ลงใน EPIC-2 |
| v0.2.0 | 2026-05-27 | **Runtime แบบ attestation + sample ที่ขับเคลื่อนด้วย SoA** อ่าน `[[plugins.audit-iso-27001.attestations]]`, `[[plugins.audit-iso-27001.soa]]`, และ `[[plugins.audit-iso-27001.samples]]` จาก `workspace.toml`; route แต่ละ criterion ตาม `expected_evidence_kind` (ประกาศใน `criteria.toml`) แล้วส่ง finding แบบ `attestation` หรือ `sample` ที่ผ่านการ gate ด้วย SoA, ส่ง `status = "fail"` + remedy ชี้ไป `workspace.toml` สำหรับ criteria ที่ขาด evidence และ `status = "not_applicable"` สำหรับ Annex A control ที่ถูกยกเว้นอย่างมีเหตุผล ค่า `criterion_id` ทั้ง 8 คงเดิม (สัญญาความคงที่, PLUGINS.en.md §Stability) ไม่ต้องแก้ dispatcher — `crates/bwoc-cli/src/audit.rs` validate `attestation`, `sample`, และสถานะ `not_applicable` อยู่แล้ว (BWOC-28/29) ลงใน EPIC-3 BWOC-34, ปิด epic ดูดีไซน์ที่ [[../../notes/2026-05-27_27001-soa-sampling]] |

## ทำไมมี Runtime ตอนนี้

[[../../notes/2026-05-26_iso-compliance-plugins|บันทึกกรอบความคิด EPIC-2]] อธิบายว่าทำไม 27001 ส่งเป็น stub ก่อน — evidence ของมันคือแนวปฏิบัติบริหารความมั่นคงสารสนเทศ (การประเมินความเสี่ยง, การเลือก control, การซ้อม incident, การฝึกความต่อเนื่อง) ที่ลดทอนเป็น evidence เชิงองค์กรมากกว่า artifact ใน repository และสคีมา v1 แสดงมันไม่ได้ EPIC-3 ปิด gap นั้น:

- [บันทึกดีไซน์ BWOC-26](../../../notes/2026-05-27_iso-runtime-evidence-model.md) ตรึง evidence model ใหม่ (`attestation`, `sample`, ฟิลด์ที่มีกรอบเวลา) และวางกรอบการ sample Annex A ว่า *"เรา sample control A.5.15, A.5.24, A.5.29 (3 จาก 37) โดยไม่พอง finding count ไปเป็น 37"*
- [BWOC-27](../../../docs/en/PLUGINS.en.md#evidence-kinds) ขยายสคีมาด้วย `attestation` (ต้องมี `signer` + `signed_at`) และ `sample` (ต้องมี `sampled_count` + `sampled_of`, มี `window` แบบ optional)
- BWOC-28 สร้าง 9001 attestation runtime และขยาย dispatcher ให้ validate kind ใหม่; BWOC-33 สร้าง 20000-1 attestation + sample runtime
- BWOC-34 (การเปลี่ยนแปลงนี้) สร้าง 27001 runtime — ตัวเดียวที่ประชากรการ sample คือ Statement of Applicability ของ operator ไม่ใช่ตัวเลขที่พิมพ์ด้วยมือ การอนุมาน "องค์กรนี้ได้ทำการประเมินความเสี่ยง" จาก "repo นี้มีไฟล์ `RISKS.md`" ยังคงทำให้ audit เป็นเท็จ (Musāvāda — [PHILOSOPHY.en.md](../../agent-template/docs/en/PHILOSOPHY.en.md) §Sila 5) Attestation evidence ทำให้ clauses main-body ซื่อสัตย์ (operator ลงนามรับรอง พร้อมวันที่และ provenance); sample evidence ที่ gate ด้วย SoA ทำให้ Annex A controls ซื่อสัตย์ (operator บันทึกว่า control ที่อยู่ใน scope ถูก sample รอบนี้ และ SoA ตัดสินว่า control ใดอยู่ใน scope แต่แรก)

## Criteria (v0.2.0)

ISMS criteria หลัก 8 รายการ ดึงจาก clauses หลักและ Annex A controls ของ ISO/IEC 27001:2022[^iso-27001-2022] ลำดับการประกาศใน [[criteria]] คือลำดับรายงาน (PLUGINS.en.md §Audit Findings Schema) ค่า `criterion_id` คงที่ข้ามรุ่น (PLUGINS.en.md §Stability); การเปลี่ยนชื่อเป็น major-version bump คอลัมน์ **Kind** คือ `expected_evidence_kind` ของแต่ละ criterion — ดูเหตุผลการแบ่ง main-body กับ Annex A ที่ [[../../notes/2026-05-27_27001-soa-sampling]]

| `criterion_id` | อ้างอิง | หัวเรื่อง | Severity | Kind |
|---|---|---|---|---|
| `27001-isms-scope` | 4.3 | ขอบเขตของ ISMS | high | attestation |
| `27001-information-security-policy` | 5.2 | นโยบายความมั่นคงสารสนเทศ | high | attestation |
| `27001-risk-assessment` | 6.1.2 | การประเมินความเสี่ยงด้านความมั่นคงสารสนเทศ | critical | attestation |
| `27001-statement-of-applicability` | 6.1.3 | Statement of Applicability | critical | attestation |
| `27001-access-control` | A.5.15 | การควบคุมการเข้าถึง | high | sample |
| `27001-incident-management` | A.5.24 | การวางแผนและเตรียมการบริหาร incident | high | sample |
| `27001-business-continuity` | A.5.29 | ความมั่นคงสารสนเทศระหว่างเหตุขัดข้อง | medium | sample |
| `27001-internal-audit` | 9.2 | การ audit ภายใน | high | attestation |

attestation ห้าตัว + sample สามตัว การแบ่งตามโครงสร้างของ ISO/IEC 27001:2022 เอง: clauses main-body ของระบบบริหาร (4–10 — scope, policy, risk assessment, SoA, internal audit) เป็น conformance แบบ documented-artifact ที่ operator ลงนามรับรอง จึงใช้ `attestation`; Annex A controls เป็นมาตรการเชิงเทคนิค/องค์กรที่ผู้ตรวจ *sample* จึงใช้ `sample` control สามตัวที่ sample มาจาก theme *Organizational* (A.5.15 access, A.5.24 incident, A.5.29 continuity); Annex A controls ที่เหลืออีก 90 ตัวเลื่อนไป — การเพิ่ม criteria เป็น minor-version bump ไม่ใช่การเปลี่ยนชื่อ id เดิม

## วิธีรัน

```bash
bwoc audit run --plugin audit-iso-27001 --json
```

Dispatcher spawns `audit.sh` (ตาม `BWOC-12`) ด้วยสัญญา env มาตรฐาน: `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION` Runtime จะ:

1. อ่าน `criteria.toml` จาก `BWOC_PLUGIN_DIR` สำหรับ criteria ที่ประกาศ (id, severity, **`expected_evidence_kind`**, และ — สำหรับ Annex A criteria — **`annex_control`**)
2. อ่าน `.bwoc/workspace.toml` จาก `BWOC_WORKSPACE` แล้วสร้างตาราง lookup สามตาราง — `[[plugins.audit-iso-27001.attestations]]` และ `[[plugins.audit-iso-27001.samples]]` keyed ด้วย `criterion_id`, และ `[[plugins.audit-iso-27001.soa]]` keyed ด้วย `control` มันคำนวณประชากรที่ขับเคลื่อนด้วย SoA (`K ของ M` ด้านล่าง) ไว้ก่อน
3. สำหรับ criterion แต่ละตัวตามลำดับการประกาศ route ตาม `expected_evidence_kind`:
   - **`attestation`** — มีและครบ (`statement` + `signer` + `signed_at`) → `status = "pass"`, `evidence.kind = "attestation"` พร้อม `valid_through` แบบ optional มีแต่ไม่ครบ หรือไม่มี → `status = "fail"`, `evidence.kind = "file"` ชี้ไป `.bwoc/workspace.toml`
   - **`sample`** — gate ด้วย SoA (ดู [การ sample Annex A ที่ขับเคลื่อนด้วย SoA](#การ-sample-annex-a-ที่ขับเคลื่อนด้วย-soa)) อยู่ใน scope + มีเหตุผล + sample แล้ว → `status = "pass"`, `evidence.kind = "sample"` ยกเว้นอย่างมีเหตุผล → `status = "not_applicable"`, `evidence.kind = "none"` ไม่มีใน SoA, ไม่มีเหตุผล, หรืออยู่ใน scope แต่ไม่มี sample → `status = "fail"`, `evidence.kind = "file"`
4. ออกด้วยรหัส `0` เมื่อสำเร็จ — finding ที่ไม่ pass เป็น *finding* ไม่ใช่ error รหัสออกที่ไม่ใช่ศูนย์สื่อปัญหาฝั่ง framework (อ่าน `criteria.toml` ไม่ได้)

รหัสออกของ process คือจำนวน finding ที่ `fail` (BWOC-12); `not_applicable` **ไม่** นับเป็น fail Runtime **ไม่** กำหนดเกณฑ์เชิงองค์กรกับ sample — sample ที่บันทึกไว้คือ evidence และ pass สรุปแบบมนุษย์อ่านถูก surface ให้ผู้ตรวจตัดสิน

## การ sample Annex A ที่ขับเคลื่อนด้วย SoA

sample ของ 20000-1 อยู่ในตัวเอง: operator พิมพ์ `sampled_count` / `sampled_of` ตรง ๆ ต่อ sample 27001 ต่างออกไป — ประชากรการ sample คือ **scope ที่ operator ประกาศ** ไม่ใช่ตัวเลขที่พิมพ์ด้วยมือ operator จึงไม่เคยพิมพ์ `sampled_count` / `sampled_of` ต่อ Annex A control ทั้งสองค่ามาจาก Statement of Applicability:

- **`M` = `sampled_of`** = จำนวน control ที่อยู่ใน scope ใน SoA (entry ที่ `applicable = true`) นี่คือประชากรการ sample
- **`K` = `sampled_count`** = จำนวน Annex A control *ของปลั๊กอินนี้* สามตัวที่อยู่ใน scope `K ≤ M` โดยโครงสร้าง `K` คำนวณจาก **scope** ไม่ใช่จากความครบของ evidence ดังนั้น `sampled_count` ของ control หนึ่งไม่ขึ้นกับ sample ของพี่น้อง — finding เป็นอิสระต่อกัน (PLUGINS.en.md §schema-rules; BWOC-11 "criterion ผ่านหรือล้มเหลวเป็นหน่วยเดียว") control ที่อยู่ใน scope แต่ขาด sample จะ **fail**; ไม่ลด `K` ของตัวอื่นแบบเงียบ ๆ

นี่ผลิตเรื่องเล่าของ BWOC-26 ("เรา sample 3 จาก 37") เป็นผลรวมของ finding แบบ `K`-ของ-`M` สามตัว — และมันติดตามการตัดสินใจ scope ของ operator อัตโนมัติ: ยกเว้น control แล้ว `M` เล็กลง; ตัวหารที่รายงานก็ตามไป การให้ `sampled_count` / `sampled_of` ด้วยมือถูกปฏิเสธ — มันทำให้ตัวหารที่พิมพ์ drift จากตัวที่ได้จาก SoA ซึ่งทำลายจุดประสงค์

### Pass / not_applicable / fail สำหรับ Annex A criterion

สำหรับ Annex A criterion แต่ละตัว runtime resolve `annex_control` (จาก `criteria.toml`) แล้ว lookup ใน SoA:

| สถานะ SoA ของ control | Status | Evidence | Remedy |
|---|---|---|---|
| ไม่มีใน SoA | `fail` | `file` → `.bwoc/workspace.toml` | "6.1.3 ต้องให้ SoA ครอบคลุมทุก Annex A control ประกาศ `control`, `applicable`, `justification`" |
| `applicable = false`, ไม่มี justification | `fail` | `file` | "6.1.3 ต้องมี justification สำหรับการยกเว้น เพิ่ม `justification`" |
| `applicable = false`, มีเหตุผล | `not_applicable` | `none` | "ยกเว้นตาม SoA: \"…\" ยืนยันซ้ำที่รอบ audit ถัดไป" |
| `applicable = true`, ไม่มี justification | `fail` | `file` | "6.1.3 ต้องมี justification สำหรับการรวมด้วย เพิ่ม `justification`" |
| `applicable = true`, ไม่มี sample | `fail` | `file` | "อยู่ใน scope แต่ไม่มี sample ที่บันทึก เพิ่ม `[[…samples]]` พร้อม `criterion_id` + `summary`" |
| `applicable = true`, sample ขาด `summary` | `fail` | `file` | ระบุฟิลด์ที่ขาด |
| `applicable = true`, มีเหตุผล, มี sample | `pass` | `sample` (`value` = summary, `sampled_count` = K, `sampled_of` = M, `window` แบบ optional) | — |

`not_applicable` คือผล ISO ที่ซื่อสัตย์สำหรับ control ที่ถูกยกเว้นอย่างมีเหตุผล — ไม่ใช่ pass (เราไม่ได้ทดสอบมัน) และไม่ใช่ fail (operator ตัดสินใจ scope อย่างมีเหตุผล) สคีมาอนุญาต `evidence.kind = "none"` เฉพาะกับ `not_applicable` / `not_implemented` และต้องการ `remedy` สำหรับ `not_applicable` — ทั้งคู่ถูกเคารพ

### บทบาทสองอย่างของ SoA

SoA ปรากฏสองครั้ง และการใช้สองแบบเป็น **อิสระ** ต่อกันโดยเจตนา:

1. **attestation `27001-statement-of-applicability` (6.1.3)** — route ผ่าน `[[…attestations]]` เหมือน attestation อื่น; ผู้บริหารระดับสูงรับรองว่า "SoA ถูกจัดตั้งและดูแลรักษา" ลงนามและลงวันที่
2. **array `[[…soa]]`** — การประกาศ in-scope แบบเครื่องอ่านได้ที่ขับเคลื่อนการ sample Annex A

ทั้งสองเสริมกัน (operator ที่ดูแล SoA จริงทำทั้งสองอยู่แล้ว) แต่ไม่ผูกกันในโค้ด — attestation ผ่าน/ล้มเหลวด้วย evidence ของตน; array soa ขับเคลื่อน Annex A criteria สามตัวด้วยตัวมันเอง การผูกทั้งสองถูกปฏิเสธ: มันทำลาย model finding แบน-อิสระ

## ตัวอย่าง Output

attestation criterion (clause main-body):

```json
{
  "criterion_id": "27001-risk-assessment",
  "severity":     "critical",
  "status":       "pass",
  "evidence": {
    "kind":          "attestation",
    "value":         "Information security risk assessment performed 2026-03-10; methodology documented; results comparable and reproducible.",
    "signer":        "CISO: Tonkla K.",
    "signed_at":     "2026-03-10",
    "valid_through": "2027-03-10"
  }
}
```

Annex A control ที่อยู่ใน scope และ sample แล้ว (`K ของ M` ที่ขับเคลื่อนด้วย SoA):

```json
{
  "criterion_id": "27001-access-control",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "sample",
    "value":         "Access reviews completed across in-scope systems; 0 orphaned accounts found.",
    "sampled_count": 1,
    "sampled_of":    3,
    "window":        "2026-Q1"
  }
}
```

Annex A control ที่ถูกยกเว้นอย่างมีเหตุผล:

```json
{
  "criterion_id": "27001-business-continuity",
  "severity":     "medium",
  "status":       "not_applicable",
  "evidence":     { "kind": "none", "value": "" },
  "remedy":       "Control A.5.29 is excluded from the ISMS scope per the Statement of Applicability: \"No formal continuity programme; risk accepted by management for a solo workspace.\". Re-confirm this exclusion remains justified at the next audit cycle."
}
```

criterion ที่ไม่มี evidence จาก operator (ที่นี่คือ Annex A control ที่ไม่มีใน SoA):

```json
{
  "criterion_id": "27001-incident-management",
  "severity":     "high",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": ".bwoc/workspace.toml" },
  "remedy":       "Control A.5.24 is not addressed in the Statement of Applicability. ISO/IEC 27001 6.1.3 requires the SoA to address every Annex A control. Declare it in .bwoc/workspace.toml under [[plugins.audit-iso-27001.soa]] with control=\"A.5.24\", applicable (true/false), and justification."
}
```

`bwoc audit run` ห่อ array ของ finding ไว้ใน envelope หลัก `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`

## การตั้งค่า

```toml
# .bwoc/workspace.toml
[plugins.audit-iso-27001]
enabled = true

# attestation criteria (4.3 scope, 5.2 policy, 6.1.2 risk, 6.1.3 SoA, 9.2 internal audit)
[[plugins.audit-iso-27001.attestations]]
criterion_id  = "27001-risk-assessment"
statement     = "Information security risk assessment performed 2026-03-10; methodology documented."
signer        = "CISO: Tonkla K."
signed_at     = "2026-03-10"
valid_through = "2027-03-10"   # optional

# Statement of Applicability — หนึ่ง entry ต่อ Annex A control ที่ operator
# ประเมินแล้ว 6.1.3 ต้องมี justification สำหรับทั้งการรวมและการยกเว้น
[[plugins.audit-iso-27001.soa]]
control       = "A.5.15"
applicable    = true
justification = "Access control is central to protecting source, credentials, and customer data."

[[plugins.audit-iso-27001.soa]]
control       = "A.5.29"
applicable    = false
justification = "No formal continuity programme; risk accepted by management for a solo workspace."

# บันทึก audit-sample ของ Annex A บางกว่า 20000-1: operator ไม่ให้
# sampled_count/sampled_of — ทั้งคู่ได้จาก SoA (K ของ M)
[[plugins.audit-iso-27001.samples]]
criterion_id = "27001-access-control"
summary      = "Access reviews completed across in-scope systems; 0 orphaned accounts found."
window       = "2026-Q1"   # optional
```

ทั้งสาม block เป็น **array-of-tables** ใต้ block สากล `[plugins.audit-iso-27001]`

ฟิลด์ `[[…attestations]]`:

| ฟิลด์ | จำเป็น | หมายเหตุ |
|---|---|---|
| `criterion_id` | ใช่ | ต้องตรงกับ criterion kind `attestation` ใน `criteria.toml` |
| `statement` | ใช่ | ข้อความ attestation ตามคำ → `evidence.value` TOML string บรรทัดเดียว |
| `signer` | ใช่ | identity แบบ free-text → `evidence.signer` |
| `signed_at` | ใช่ | วันที่ (หรือ datetime) ISO 8601 → `evidence.signed_at` |
| `valid_through` | optional | วันหมดอายุ ISO 8601 → `evidence.valid_through` dispatcher ประทับแต่ไม่บังคับการหมดอายุ (ตาม BWOC-26) |

ฟิลด์ `[[…soa]]` (Statement of Applicability):

| ฟิลด์ | จำเป็น | หมายเหตุ |
|---|---|---|
| `control` | ใช่ | อ้างอิง Annex A control (เช่น `"A.5.15"`) จับคู่กับ `annex_control` ของแต่ละ Annex A criterion ใน `criteria.toml` |
| `applicable` | ใช่ | TOML boolean `true` → อยู่ใน scope (นับเข้า `M`); `false` → ยกเว้น (ส่ง `not_applicable` เมื่อมีเหตุผล) |
| `justification` | ใช่ | 6.1.3 ต้องมี justification สำหรับ **ทั้ง** การรวมและการยกเว้น surface ใน remedy ของ finding `not_applicable` |

ฟิลด์ `[[…samples]]`:

| ฟิลด์ | จำเป็น | หมายเหตุ |
|---|---|---|
| `criterion_id` | ใช่ | ต้องตรงกับ criterion kind `sample` ใน `criteria.toml` |
| `summary` | ใช่ | สรุปสั้นแบบมนุษย์ว่า sample อะไร → `evidence.value` |
| `window` | optional | ช่วงเวลาแบบ free-text → `evidence.window` (เช่น `"2026-Q1"`) |

operator **ไม่** ให้ `sampled_count` / `sampled_of` ที่นี่ (ต่างจาก 20000-1) — ทั้งคู่ได้จาก SoA key แรกที่พบชนะ; runtime ไม่ flag ค่าซ้ำ (เป็นหน้าที่ของ `bwoc check` — BWOC-29) `workspace.toml` ถูกเลือกแทนไฟล์แยกด้วยเหตุผล blast-radius เล็กที่สุดเหมือนแหล่ง evidence ของ 9001/20000-1: operator แตะไฟล์นี้อยู่แล้ว ทุกการเปลี่ยน diff ได้ใน `git log` และ `bwoc check` เดินผ่านมันอยู่แล้ว ดูเหตุผลเต็มที่ [[../../notes/2026-05-27_27001-soa-sampling]]

## Maturity

ประกาศ **L1** — runtime ทำงาน ส่ง finding แบบ attestation + sample ที่ขับเคลื่อนด้วย SoA จริงตาม BWOC-27 เลื่อนเป็น **L2** เมื่อ:

- `bwoc check` validate block `[[attestations]]` / `[[soa]]` / `[[samples]]` ของ workspace กับสคีมา (ขยาย BWOC-29 จาก `criteria.toml` ไป `workspace.toml`) — รวมถึง `control` refs ที่ถูกต้องและ boolean `applicable` ใน SoA
- catalogue ของ Annex A ขยายเกิน A.5 controls สามตัวไปสู่ control ตัวแทนจาก A.6 (People), A.7 (Physical), และ A.8 (Technological) — additive ไม่เปลี่ยนชื่อ id
- traceability control-ต่อ-clause ลง (A.5.24 feeds ทั้ง clause 9 และ clause 10) ผ่าน `evidence.related_criteria` ในอนาคต

## ความเป็นกลาง

ค่าใน manifest ไม่ระบุ LLM backend, vendor หรือ model `kind = "audit"` เป็น enum ของ framework เอง (`BWOC-10`) ปลั๊กอินอ้างถึง "ISO/IEC 27001" เฉพาะใน `description`, ในเนื้อหา SPEC นี้, และใน namespace มาตรฐาน `27001-*` ของ criterion-id — ไม่ใช้ใน key ของ `criteria.toml` หรือในค่า finding นอกเหนือจาก namespace นั้น ผ่านกฎ **Samānattatā**

## แหล่งอ้างอิง

ISMS criteria ดึงจาก clauses หลักของ ISO/IEC 27001:2022 (4 ถึง 10) และชุด Annex A controls ที่เผยแพร่:

- ISO/IEC 27001:2022 — *Information security, cybersecurity and privacy protection — Information security management systems — Requirements.* International Organization for Standardization. ISO catalogue entry: <https://www.iso.org/standard/27001> [^iso-27001-2022]
- ISO/IEC 27002:2022 — *Information security, cybersecurity and privacy protection — Information security controls.* ISO catalogue entry: <https://www.iso.org/standard/75652.html> มาตรฐานเสริมที่ให้แนวทาง implementation สำหรับ Annex A controls
- ISO/IEC JTC 1/SC 27 — *Information security, cybersecurity and privacy protection.* หน้า landing สาธารณะ: <https://www.iso.org/committee/45306.html> คณะกรรมการเทคนิคร่วมที่รับผิดชอบ ISO/IEC 27000 family

[^iso-27001-2022]: ISO/IEC 27001:2022 มีโครงสร้างตาม Annex SL high-level structure (clauses 4 บริบท, 5 ภาวะผู้นำ, 6 การวางแผน, 7 การสนับสนุน, 8 การปฏิบัติการ, 9 การประเมินผลการทำงาน, 10 การปรับปรุง) Annex A มี 93 ควบคุมความมั่นคงสารสนเทศที่จัดเป็น 4 themes — *Organizational* (37 controls, prefix A.5.x), *People* (8 controls, prefix A.6.x), *Physical* (14 controls, prefix A.7.x), และ *Technological* (34 controls, prefix A.8.x) — ปรับโครงสร้างใหม่จากการจัด 114-control / 14-clause ใน ISO/IEC 27001:2013 Criteria ทั้ง 8 ที่นี่ครอบคลุม clauses main-body ห้าตัวบวก Annex A controls สามตัวจาก theme *Organizational* ที่เป็นหัวเรื่องการดำเนินการ ISMS; Annex A controls ที่เหลืออีก 90 ตัวเลื่อนไปขยายใน minor-version ในอนาคตเมื่อ runtime ที่ขับเคลื่อนด้วย SoA sample ได้สอดคล้อง

## ดูเพิ่ม

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — สเปคของ plugin; แถวของ `audit` kind (BWOC-10), Audit Findings Schema (BWOC-11 + BWOC-27 evidence kinds)
- [[../../notes/2026-05-26_iso-compliance-plugins|2026-05-26_iso-compliance-plugins.md]] — บันทึกกรอบความคิด EPIC-2 (ทำไม stub)
- [[../../notes/2026-05-27_iso-runtime-evidence-model|2026-05-27_iso-runtime-evidence-model.md]] — ดีไซน์ evidence-model BWOC-26 (attestation, sample, การวางกรอบ "3 จาก 37 Annex A")
- [[../../notes/2026-05-27_27001-soa-sampling|2026-05-27_27001-soa-sampling.md]] — ดีไซน์ BWOC-34 (การแบ่ง evidence-kind + ประชากรการ sample ที่ขับเคลื่อนด้วย SoA)
- [[../audit-iso-29110/SPEC|audit-iso-29110]] — ปลั๊กอิน audit อ้างอิงที่รันได้จาก BWOC-13
- [[../audit-iso-9001/SPEC|audit-iso-9001]] — ปลั๊กอิน attestation runtime พี่น้อง (BWOC-28); [[../audit-iso-20000-1/SPEC|audit-iso-20000-1]] — ปลั๊กอิน attestation + sample runtime พี่น้อง (BWOC-33)
