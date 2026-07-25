---
title: IEEE 1012 Verification and Validation Audit
aliases:
  - audit-ieee-1012
tags:
  - group/framework-plugins
  - type/plugin
  - kind/audit
  - domain/compliance
  - standard/ieee-1012
  - status/runtime
maturity: L1
---

# IEEE 1012 Verification and Validation Audit

> [!abstract] **Attestation runtime (v0.1.0).** ตรวจ **verification and validation (V&V)** ของ workspace เทียบ **IEEE 1012:2016** — *IEEE Standard for System, Software, and Hardware Verification and Validation*. อ่าน attestation ที่ operator เซ็นจาก `.bwoc/workspace.toml` ใต้ `[[plugins.audit-ieee-1012.attestations]]` แล้ว emit finding `evidence.kind = "attestation"` (`signer` + `signed_at` + `valid_through` ถ้ามี) ตาม [BWOC-27 schema](../../../docs/th/PLUGINS.th.md#evidence-kinds). criterion ที่ไม่มี attestation → `status = "fail"` ชี้ไป block `workspace.toml`. ใช้ runtime attestation ร่วมกับ lane ISO 9001 / ISO/IEC 27001 / ISO/IEC/IEEE 29148 / 12207

## ทำไม IEEE 1012 — lane IEEE เดี่ยวตัวแรก

audit kind ครอบ ISO ล้วน (9001), ISO/IEC (27001, 20000-1, 29110), และ ISO/IEC/IEEE ร่วม (29148, 12207). IEEE 1012 เป็นมาตรฐาน **IEEE เดี่ยว** ตัวแรกในชุด — IEEE ประกาศเองไม่ร่วมองค์กรอื่น. รองรับมันพิสูจน์ว่า audit kind เป็นกลางต่อองค์กรมาตรฐานจริง: ตรวจตาม *criteria* ของมาตรฐาน ไม่ว่าจะหนึ่ง สอง หรือสามองค์กรร่วมประกาศ. V&V ยังเป็นคู่ verification ตามธรรมชาติของ life-cycle ใน 12207 และ requirements ใน 29148 — สามอันรวมเป็นชุด assurance ของ systems/software ที่สอดคล้องกัน

## Criteria

8 criteria ครอบกระบวนการ V&V ของ IEEE 1012:2016 (clause อ้าง 1012:2016):

| id | clause | severity | ตรวจ |
|---|---|---|---|
| `1012-integrity-levels` | 5 | high | แต่ละ element กำหนด integrity level ที่ปรับระดับความเข้มของ V&V |
| `1012-vv-planning` | 7 | high | V&V Plan (SVVP) นิยาม scope, activity, task, method, การปรับตาม integrity |
| `1012-independence` | 4 | medium | ความเป็นอิสระของ V&V (technical/managerial/financial) เหมาะกับ integrity level |
| `1012-requirements-vv` | Table 1 | high | verify requirements (ถูก/สอดคล้อง/ครบ/trace ได้) + validate เทียบความต้องการ |
| `1012-design-vv` | Table 1 | high | verify design ว่าถูก + trace ไป requirements; ประเมิน testability |
| `1012-implementation-vv` | Table 1 | medium | verify code/implementation เทียบ design + coding standard (review, static analysis) |
| `1012-test-vv` | Table 1 | high | verify test plan/design/case/procedure/result ว่าพอเพียง + trace ได้ |
| `1012-anomaly-reporting` | 7 | medium | กิจกรรม V&V ออก report; anomaly ถูกบันทึก จัดหมวด ติดตามจนแก้ |

## วิธีทำงาน

`bwoc audit run` ตั้ง `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION=audit_run` แล้วเรียก `audit.sh`. runtime อ่าน `criteria.toml` + attestation ของ operator, match ด้วย `criterion_id`, emit finding ต่อ criterion. read-only — ตรวจ+รายงาน ไม่ mutate

## Configuration

```toml
[plugins.audit-ieee-1012]
enabled = true

[[plugins.audit-ieee-1012.attestations]]
criterion_id = "1012-test-vv"
statement    = "ทุก requirement map ไป automated test อย่างน้อยหนึ่ง; CI รัน suite ทุก PR และ block merge เมื่อ fail"
signer       = "QA Lead: Naruemon K."
signed_at    = "2026-07-25"
# valid_through = "2027-07-25"
```

criterion ที่ไม่มี attestation ที่ตรง+ถูกรูป → `status = "fail"` พร้อม remedy ชี้ block นี้

## Findings schema

ตาม [PLUGINS.th.md §Audit Findings Schema](../../../docs/th/PLUGINS.th.md#audit-findings-schema): `evidence.kind = "attestation"`, `value` = statement, + `signer` + `signed_at` (+ `valid_through` ถ้ามี). pass = พก attestation; fail = พก remedy

## Exit codes

**plugin** (`audit.sh`) ออก `0` เมื่อสำเร็จ — finding ที่ไม่ pass เป็น *finding* ไม่ใช่ error — และ non-zero เฉพาะ runtime ล้ม (ไม่มี `BWOC_WORKSPACE`, อ่าน `criteria.toml` ไม่ได้). **`bwoc audit run` dispatcher** คำนวณ exit code เอง: จำนวน finding ที่ `fail` (clamp ที่ `254`) หรือ `255` เมื่อ framework/runtime error. audit สะอาดออก `0`, มี N fail ออก `N`, plugin พังออก `255` (`crates/bwoc-cli/src/audit.rs::compute_exit_code`)

## Maturity

L1 — attestation runtime บน 8 criteria V&V ของ 1012. การตรวจลึก (parse SVVP / test-traceability matrix จริงจาก workspace ไม่ใช่แค่ attestation) เป็น slice ถัดไป ตาม roadmap ของ lane อื่น

## Neutrality

เป็นกลางต่อ backend: ไม่มี LLM/model/vendor. ระบุมาตรฐาน (IEEE 1012) ไม่ใช่เครื่องมือ. runtime attestation ใช้ร่วมทั้ง audit kind
