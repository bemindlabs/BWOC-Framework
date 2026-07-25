---
title: ISO/IEC/IEEE 12207 Software Life Cycle Processes Audit
aliases:
  - audit-iso-iec-ieee-12207
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-ieee-12207
  - status/runtime
maturity: L1
---

# ISO/IEC/IEEE 12207 Software Life Cycle Processes Audit

> [!abstract] **Attestation runtime (v0.1.0).** ตรวจ **กระบวนการวงจรชีวิตซอฟต์แวร์** ของ workspace เทียบ **ISO/IEC/IEEE 12207:2017** — มาตรฐานที่ **ISO, IEC และ IEEE ร่วมกันประกาศ** (สอดคล้องกับ ISO/IEC/IEEE 15288 ฝั่ง systems). อ่าน attestation ที่ operator เซ็นจาก `.bwoc/workspace.toml` ใต้ `[[plugins.audit-iso-iec-ieee-12207.attestations]]` แล้ว emit finding `evidence.kind = "attestation"` (`signer` + `signed_at` + `valid_through` ถ้ามี) ตาม [BWOC-27 schema](../../../docs/th/PLUGINS.th.md#evidence-kinds). criterion ที่ไม่มี attestation → `status = "fail"` ชี้ไป block `workspace.toml`. ใช้ runtime attestation ร่วมกับ lane ISO 9001 / ISO/IEC 27001 / ISO/IEC/IEEE 29148

## ทำไม ISO/IEC/IEEE 12207

ที่ [29148](../audit-iso-iec-ieee-29148/SPEC.th.md) ครอบวินัย *requirements* ส่วน 12207 ครอบ **วงจรชีวิตซอฟต์แวร์ทั้งหมด** — process framework ที่องค์กรทำตามตั้งแต่ agreement ถึง maintenance. เป็นมาตรฐาน ISO/IEC/IEEE ร่วมตัวที่สองใน audit kind และเป็นคู่หูตามธรรมชาติของ lane requirements: 29148 ถาม "requirements ถูกไหม?", 12207 ถาม "วงจรชีวิตที่ผลิตซอฟต์แวร์ถูกกำกับไหม?". criterion id / ชื่อมาตรฐาน / clause เป็นข้อมูล ไม่ใช่ข้อจำกัด — runtime เป็นกลางต่อองค์กรมาตรฐาน

## Criteria

9 criteria ครอบ process groups ของ 12207:2017 (clause อ้าง 12207:2017):

| id | clause | severity | ตรวจ |
|---|---|---|---|
| `12207-agreement-processes` | 6.1 | medium | acquisition/supply กำกับด้วย agreement ที่นิยาม (scope, deliverable, acceptance) |
| `12207-project-planning` | 6.3.1 | high | project plan นิยาม scope, schedule, resource, และ task ของวงจรชีวิตที่เลือก |
| `12207-project-assessment-control` | 6.3.2 | medium | ประเมินความคืบหน้า/performance เทียบ plan; แก้ไขเมื่อเบี่ยง |
| `12207-configuration-management` | 6.3.5 | high | work product ถูก identify, version-control, baseline; คุมการเปลี่ยนแปลง |
| `12207-requirements-definition` | 6.4.2/6.4.3 | high | ความต้องการ stakeholder + requirements ระบบ/ซอฟต์แวร์ นิยาม/บันทึก/ตกลง |
| `12207-architecture-design` | 6.4.4/6.4.5 | high | architecture/design นิยาม สอดคล้อง + trace ไป requirements ได้ |
| `12207-implementation-integration` | 6.4.6/6.4.7 | medium | implement + integrate ตาม design; verify การ integrate ทีละขั้น |
| `12207-verification-validation` | 6.4.9/6.4.11 | high | verify work product เทียบ spec; validate ระบบเทียบความต้องการ stakeholder |
| `12207-maintenance` | 6.4.13 | medium | จัดการการแก้ไขหลังส่งมอบผ่านกระบวนการ maintenance ที่นิยาม |

## วิธีทำงาน

`bwoc audit run` ตั้ง `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION=audit_run` แล้วเรียก `audit.sh`. runtime อ่าน `criteria.toml` + attestation ของ operator, match ด้วย `criterion_id`, emit finding ต่อ criterion. read-only — ตรวจ+รายงาน ไม่ mutate

## Configuration

```toml
[plugins.audit-iso-iec-ieee-12207]
enabled = true

[[plugins.audit-iso-iec-ieee-12207.attestations]]
criterion_id = "12207-configuration-management"
statement    = "source/docs/build artefact ทั้งหมดอยู่ใน git มี tagged baseline; เปลี่ยนได้ผ่าน PR ที่ review + CI-gate เท่านั้น"
signer       = "Eng Lead: Somchai T."
signed_at    = "2026-07-25"
# valid_through = "2027-07-25"
```

criterion ที่ไม่มี attestation ที่ตรง+ถูกรูป → `status = "fail"` พร้อม remedy ชี้ block นี้

## Findings schema

ตาม [PLUGINS.th.md §Audit Findings Schema](../../../docs/th/PLUGINS.th.md#audit-findings-schema): `evidence.kind = "attestation"`, `value` = statement, + `signer` + `signed_at` (+ `valid_through` ถ้ามี). pass = พก attestation; fail = พก remedy

## Exit codes

**plugin** (`audit.sh`) ออก `0` เมื่อสำเร็จ — finding ที่ไม่ pass เป็น *finding* ไม่ใช่ error — และ non-zero เฉพาะ runtime ล้ม (ไม่มี `BWOC_WORKSPACE`, อ่าน `criteria.toml` ไม่ได้). **`bwoc audit run` dispatcher** คำนวณ exit code เอง: จำนวน finding ที่ `fail` (clamp ที่ `254`) หรือ `255` เมื่อ framework/runtime error. audit สะอาดออก `0`, มี N fail ออก `N`, plugin พังออก `255` (`crates/bwoc-cli/src/audit.rs::compute_exit_code`)

## Maturity

L1 — attestation runtime บน 9 process criteria ของ 12207. การตรวจลึก (parse project plan / CM baseline / traceability จริงจาก workspace ไม่ใช่แค่ attestation) เป็น slice ถัดไป ตาม roadmap ของ lane อื่น

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM/model/vendor. ระบุมาตรฐาน (ISO/IEC/IEEE 12207) ไม่ใช่เครื่องมือ. runtime attestation ใช้ร่วมทั้ง audit kind
