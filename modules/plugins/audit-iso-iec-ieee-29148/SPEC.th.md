---
title: ISO/IEC/IEEE 29148 Requirements Engineering Audit
aliases:
  - audit-iso-iec-ieee-29148
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/iso-iec-ieee-29148
  - status/runtime
maturity: L1
---

# ISO/IEC/IEEE 29148 Requirements Engineering Audit

> [!abstract] **Attestation runtime (v0.1.0).** ตรวจ **requirements engineering** ของ workspace เทียบ **ISO/IEC/IEEE 29148:2018** — มาตรฐานที่ **ISO, IEC และ IEEE ร่วมกันประกาศ** (แทน IEEE 830 / 1233 / 1362). อ่าน attestation ที่ operator เซ็นจาก `.bwoc/workspace.toml` ใต้ `[[plugins.audit-iso-iec-ieee-29148.attestations]]` แล้ว emit finding `evidence.kind = "attestation"` (`signer` + `signed_at` + `valid_through` ถ้ามี) ตาม [BWOC-27 schema](../../../docs/th/PLUGINS.th.md#evidence-kinds). criterion ที่ไม่มี attestation → `status = "fail"` ชี้ไป block `workspace.toml`. นี่คือ lane **ISO/IEC/IEEE** แรก — ขยายการรองรับของ audit kind จาก ISO ล้วนไปครอบทั้ง ISO / IEC / IEEE โดยใช้ runtime attestation ร่วมกับ lane ISO 9001 / ISO/IEC 27001.

## ทำไม ISO/IEC/IEEE (ไม่ใช่แค่ ISO)

audit kind เริ่มจากมาตรฐาน ISO ล้วน (ISO 9001) และ **ISO/IEC** ร่วม (27001, 20000-1, 29110). 29148 เป็น **ISO/IEC/IEEE** ร่วมตัวแรกในชุด — สามองค์กรประกาศร่วมเป็นมาตรฐาน requirements engineering หลักของ systems + software. รองรับมันทำให้ coverage ของ audit kind ชัดครบทั้งสามองค์กร; criterion id / ชื่อมาตรฐาน / clause เป็นข้อมูล ไม่ใช่ข้อจำกัด — runtime เป็นกลางต่อองค์กรมาตรฐาน

## Criteria

7 criteria แต่ละอันเป็น attestation ของ operator (clause อ้าง 29148:2018):

| id | clause | severity | ตรวจ |
|---|---|---|---|
| `29148-stakeholder-requirements` | 9.4 | high | ความต้องการ stakeholder บันทึกใน StRS |
| `29148-system-software-requirements` | 9.5/9.6 | high | requirements ระบบ/ซอฟต์แวร์ใน SyRS/SRS สืบจาก StRS |
| `29148-requirement-characteristics` | 5.2.6 | high | แต่ละ requirement จำเป็น/ไม่กำกวม/เดี่ยว/ทำได้/verify ได้/ถูกต้อง/… |
| `29148-requirement-set-characteristics` | 5.2.7 | medium | ชุด requirement ครบ/สอดคล้อง/ทำได้/เข้าใจได้/validate ได้ |
| `29148-verifiability` | 5.2.6 | high | แต่ละ requirement มีวิธี verify (inspection/analysis/demonstration/test) |
| `29148-traceability` | 5.2.8 | high | traceability สองทาง: stakeholder → system → software → verification |
| `29148-requirements-management` | 6.5 | medium | requirements baseline + คุมการเปลี่ยนแปลง |

## วิธีทำงาน

`bwoc audit run` ตั้ง `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION=audit_run` แล้วเรียก `audit.sh`. runtime อ่าน `criteria.toml` + attestation ของ operator, match ด้วย `criterion_id`, emit finding ต่อ criterion. read-only — ตรวจ+รายงาน ไม่ mutate

## Configuration

```toml
[plugins.audit-iso-iec-ieee-29148]
enabled = true

[[plugins.audit-iso-iec-ieee-29148.attestations]]
criterion_id = "29148-traceability"
statement    = "มี requirements traceability matrix เชื่อม StRS need → SyRS/SRS requirement → test case; ทบทวนทุก release."
signer       = "RE Lead: Anong P."
signed_at    = "2026-07-24"
# valid_through = "2027-07-24"
```

criterion ที่ไม่มี attestation ที่ตรง+ถูกรูป → `status = "fail"` พร้อม remedy ชี้ block นี้

## Findings schema

ตาม [PLUGINS.th.md §Audit Findings Schema](../../../docs/th/PLUGINS.th.md#audit-findings-schema): `evidence.kind = "attestation"`, `value` = statement, + `signer` + `signed_at` (+ `valid_through` ถ้ามี). pass = พก attestation; fail = พก remedy

## Exit codes

**plugin** (`audit.sh`) ออก `0` เมื่อสำเร็จ — finding ที่ไม่ pass เป็น *finding* ไม่ใช่ error — และ non-zero เฉพาะ runtime ล้ม (ไม่มี `BWOC_WORKSPACE`, อ่าน `criteria.toml` ไม่ได้). **`bwoc audit run` dispatcher** คำนวณ exit code เอง: จำนวน finding ที่ `fail` (clamp ที่ `254`) หรือ `255` เมื่อ framework/runtime error. audit สะอาดออก `0`, มี N fail ออก `N`, plugin พังออก `255` (`crates/bwoc-cli/src/audit.rs::compute_exit_code`)

## Maturity

L1 — attestation runtime บน 7 criteria ของ 29148. การตรวจลึก (parse SRS/traceability matrix จริงจาก workspace ไม่ใช่แค่ attestation) เป็น slice ถัดไป ตาม roadmap ของ lane ISO 9001 / 27001

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM/model/vendor. ระบุมาตรฐาน (ISO/IEC/IEEE 29148) ไม่ใช่เครื่องมือ. runtime attestation ใช้ร่วมทั้ง audit kind
