# PHILOSOPHY — รากฐานหลักพุทธของ Agent Base Profile (Extended)

| | |
|---|---|
| **เอกสาร** | docs/PHILOSOPHY.th.md |
| **เวอร์ชัน** | 2.0 |
| **วันที่** | 2026-05-22 |
| **ภาษาคู่** | PHILOSOPHY.en.md |
| **สถานะ** | Normative — แกนอ้างอิงของเอกสารทุกฉบับ |

---

## 0. คำนำ

เอกสารนี้เป็น **แกนกลางทางความคิด** ของระบบ Agent Base Profile ทั้งหมด เอกสารอื่นทุกฉบับอ้างอิงและสอดคล้องกับ PHILOSOPHY นี้ ถ้ามีข้อขัดแย้งระหว่างเอกสาร PHILOSOPHY ชี้ขาด

หลักธรรมในที่นี้ใช้เป็น **กรอบความคิดทางวิศวกรรม** ไม่ใช่การตีความศาสนา ผู้สนใจในแง่ธรรมะลึก ขอให้ศึกษาจากตำราพุทธโดยตรง

---

## 0.1 วงรอบ — อุปฺปาท · ฐิติ · วยะ

ก่อนจะถึงหลักธรรม 22 ประการ มีรูปทรงเดียวกันอยู่เบื้องหลัง — ทุกสิ่งที่เป็นสังขตะ (สิ่งปรุงแต่ง) ย่อมมีวงรอบ **อังคุตตรนิกาย 3.47 (สังขตสูตร)** กล่าวถึงลักษณะสามประการของสังขตะ —

> *อุปฺปาโท ปญฺญายติ* — ความเกิดขึ้น ปรากฏ
> *ฐิตสฺส อญฺญถตฺตํ ปญฺญายติ* — ความแปรไปในขณะที่ยังตั้งอยู่ ปรากฏ
> *วโย ปญฺญายติ* — ความดับ ปรากฏ

ในกรอบ BWOC agent (และทุก task, session, worktree ของมัน) คือสังขตะ — จึงดำเนินตามวงรอบนี้

| ระยะ | บาลี | พื้นผิวเชิงวิศวกรรม |
|---|---|---|
| เกิดขึ้น | **อุปฺปาท** | Incarnation (`bwoc new`), การกำหนดบุคลิก, ประกาศความสามารถ (Attanutata), การ resolve manifest |
| ตั้งอยู่ (พร้อมแปรไป) | **ฐิติ** | การทำงาน: วางแผนด้วยอริยสัจ 4, ลงมือด้วยมรรค 8, ความทรงจำด้วยสัมมาสติ, การสื่อสารด้วยพรหมวิหาร 4 การเปลี่ยนแปลง *ภายใน* การคงอยู่ ไม่ใช่ความนิ่งเฉย |
| ดับไป | **วยะ** | การจบ: ล้าง worktree (อนัตตา), ปลดปล่อย branch, ตัดความจำ (มัตตัญญุตา), ปิด task ด้วยการบันทึก |

หลักธรรม 22 ประการในข้อ 1 ทั้งหมดเป็นองค์ประกอบย่อยของวงรอบนี้ ระบุว่าการเกิดขึ้นจะมีหลักการอย่างไร, การคงอยู่พร้อมแปรไปจะมีวินัยอย่างไร, และการดับจะปล่อยวางอย่างสะอาดอย่างไร

---

## 1. หลักธรรมหลัก 22 ประการ

### หมวด A — กรอบกระบวนการ (Process Frameworks)

#### 1. อริยสัจ 4 — โครงคิดแก้ปัญหา
ใช้เป็นโครงของ PRD และ task planning

| ข้อ | ใช้ใน |
|---|---|
| ทุกข์ | นิยามปัญหา |
| สมุทัย | หา root cause |
| นิโรธ | กำหนดเป้าหมายวัดได้ |
| มรรค | วางแผนเป็นขั้นตอน |

#### 2. มรรค 8 — โครงของ Functional Requirements
ใช้เป็น 8 pillars ใน SRS

| มรรค | ในระบบ |
|---|---|
| สัมมาทิฏฐิ | Persona, Identity |
| สัมมาสังกัปปะ | Goal setting, planning |
| สัมมาวาจา | Inter-agent comms |
| สัมมากัมมันตะ | Worktree, commit |
| สัมมาอาชีวะ | Trust, neutrality |
| สัมมาวายามะ | Verification gates |
| สัมมาสติ | Memory system |
| สัมมาสมาธิ | Focus, session |

#### 3. ขันธ์ 5 — โครงสถาปัตยกรรม
ใช้ใน ARCHITECTURE document

| ขันธ์ | ในระบบ |
|---|---|
| รูป | File layout |
| เวทนา | I/O, hooks |
| สัญญา | Memory, recognition |
| สังขาร | Logic, transformations |
| วิญญาณ | Runtime, awareness |

---

### หมวด B — กรอบสภาวะ (State Frameworks)

#### 4. ไตรลักษณ์ — ฐานคิดเรื่อง state
ทุกสิ่งในระบบมี 3 ลักษณะ การออกแบบต้องสอดคล้อง

| ลักษณะ | ผลกระทบ |
|---|---|
| อนิจจัง | Memory ต้อง prune, timestamp |
| ทุกขัง | Branch ค้าง = ทุกข์ → cleanup |
| อนัตตา | ไม่ยึด branch, worktree |

#### 5. ปฏิจจสมุปบาท — เหตุปัจจัยต่อเนื่อง (ใหม่)
หลักการ "เพราะสิ่งนี้มี สิ่งนี้จึงมี" — ใช้ในการวิเคราะห์ failure modes และ error chains

หลักสำคัญ: **ปัญหาที่เห็น มักไม่ใช่ปัญหาที่ต้องแก้** ต้องสืบเหตุปัจจัยถอยกลับ

ในระบบใช้กับ
- Root cause analysis เมื่อ agent ทำผิด
- Failure propagation tracing
- Cascading failure prevention
- Post-mortem analysis

#### 6. กรรม 3 (กายกรรม วจีกรรม มโนกรรม) — Audit Trail
การกระทำ 3 ทาง — ใช้เป็นโครงของ logging

| กรรม | ในระบบ |
|---|---|
| กายกรรม | File operations, commits (เห็นได้) |
| วจีกรรม | Messages, logs (อ่านได้) |
| มโนกรรม | Decisions, plans (อนุมานได้) |

---

### หมวด C — กรอบการเติบโต (Growth Frameworks)

#### 7. อิทธิบาท 4 — เครื่องยนต์ทำงาน
ใช้เป็น success metrics

| ธรรม | KPI |
|---|---|
| ฉันทะ | Working in declared domain |
| วิริยะ | Task completion rate |
| จิตตะ | Gate compliance |
| วิมังสา | Self-improvement metrics |

#### 8. ภาวนา 4 — Agent Lifecycle (ใหม่)
การเติบโต 4 ขั้น ใช้กับ agent lifecycle management

| ภาวนา | ระยะ | ตัวบ่งชี้ |
|---|---|---|
| กายภาวนา | Incarnation — เกิด | Template materialized, placeholders set |
| สีลภาวนา | Onboarding — เรียนกติกา | Conventions internalized, first task done |
| จิตภาวนา | Operational — ทำงานคล่อง | Stable task completion, low retry |
| ปัญญาภาวนา | Mentorship — สอนผู้อื่น | Patterns extracted, knowledge shared |

#### 9. ปัญญา 3 — Self-Improvement Loop (ใหม่)
ปัญญาเกิดจาก 3 ทาง ใช้เป็นโครงของ self-improvement system

| ปัญญา | ในระบบ |
|---|---|
| สุตมยปัญญา | เรียนจากเอกสาร, conventions, examples |
| จินตามยปัญญา | คิดต่อ — synthesis, planning, pattern extraction |
| ภาวนามยปัญญา | เรียนจากปฏิบัติ — feedback loop, retrospectives |

#### 10. อริยทรัพย์ 7 — Capability Maturity (ใหม่)
ทรัพย์ 7 ของผู้ประเสริฐ ใช้เป็น maturity model

| ทรัพย์ | ในระบบ | ระดับ |
|---|---|---|
| ศรัทธา | Trust in conventions | L1 |
| ศีล | Following rules | L2 |
| หิริ-โอตตัปปะ | Self-awareness of errors | L3 |
| สุตะ | Knowledge depth | L4 |
| จาคะ | Sharing capability | L5 |
| ปัญญา | Independent judgment | L6 |

---

### หมวด D — กรอบความสัมพันธ์ (Relational Frameworks)

#### 11. สัปปุริสธรรม 7 — รู้บริบท 7 มิติ
ใช้ใน PRD stakeholder analysis และก่อนทำงาน

| ธรรม | รู้ |
|---|---|
| ธัมมัญญุตา | เหตุ, หลักการ |
| อัตถัญญุตา | ผล, เป้าหมาย |
| อัตตัญญุตา | ตน, ขีดจำกัด |
| มัตตัญญุตา | ประมาณ, ขอบเขต |
| กาลัญญุตา | กาล, เวลา |
| ปริสัญญุตา | ชุมชน, บริบท |
| ปุคคลัญญุตา | บุคคล, ผู้ใช้ |

#### 12. สังคหวัตถุ 4 — UX Principles
| ธรรม | ในระบบ |
|---|---|
| ทาน | Generous defaults, scaffolds |
| ปิยวาจา | Clear, helpful error messages |
| อัตถจริยา | Beneficial action, not just done |
| สมานัตตตา | Equal treatment across backends |

#### 13. สาราณียธรรม 6 — Inter-Agent Harmony
- เมตตา 3 ทาง (กาย วาจา ใจ) ต่อ agent อื่น
- สาธารณโภคี — แบ่ง resource ยุติธรรม
- สีลสามัญญตา — กติกาเดียวกัน
- ทิฏฐิสามัญญตา — เป้าหมายตรงกัน

#### 14. พรหมวิหาร 4 — Error UX (ใหม่)
หลัก 4 ประการของผู้ประเสริฐ ใช้กับการตอบสนองผู้ใช้และจัดการ error

| วิหาร | ในระบบ |
|---|---|
| เมตตา | Friendly tone in messages |
| กรุณา | Suggest fixes, not just report errors |
| มุทิตา | Celebrate user wins, learn from them |
| อุเบกขา | **Stay even when user frustrated** — ไม่ react แรง |

#### 15. กัลยาณมิตร 7 — Inter-Agent Trust (ใหม่)
คุณสมบัติของ "เพื่อนดี" ใช้กำหนดว่า agent ไหนเป็น trusted peer

| คุณสมบัติ | ในระบบ |
|---|---|
| ปิโย | น่าสนใจในการ delegate |
| ครุ | น่าเคารพในความสามารถ |
| ภาวนีโย | ช่วยให้เราเก่งขึ้น |
| วัตตา | บอกในสิ่งที่เป็นประโยชน์ |
| วจนักขโม | รับฟังคำท้วงได้ |
| คัมภีรัญจะ กถัง กัตตา | อธิบายเรื่องลึกได้ |
| โน จัฏฐาเน นิโยชเย | ไม่ชักนำผิดทาง |

---

### หมวด E — กรอบความระมัดระวัง (Discipline Frameworks)

#### 16. โยนิโสมนสิการ — Verify Before Act
คิดแยบคาย สืบเหตุปัจจัย ก่อนกระทำ

#### 17. อจินไตย 4 — Scope Discipline
เรื่องที่ไม่ควรเก็บมาคิด (จำกัดที่ scope งานจริง ๆ ไม่ใช่ scope ทั่วไป)

| อจินไตย | ในระบบ (เฉพาะกรณีที่ตรง) |
|---|---|
| พุทธวิสัย | ไม่คาดเดาเจตนา LLM provider |
| ฌานวิสัย | ไม่อ้างเหตุผลจาก model internals |
| กรรมวิบาก | ไม่คาดผลทางธุรกิจระยะยาว |
| โลกจินตา | ไม่ออกแบบนอก scope ของระบบนี้ |

> *Note: ใน v1 เคยใช้อจินไตยกับเรื่อง task scope — เปลี่ยนมาใช้ มัตตัญญุตา (รู้ประมาณ) จะตรงกว่า*

#### 18. สติปัฏฐาน 4 — Observability (ใหม่ - ขยายเต็ม)
ใช้เป็นโครงของ observability system

| ฐาน | สังเกตอะไร | ในระบบ |
|---|---|---|
| กายานุปัสสนา | กาย (รูปธรรม) | File state, working directory, process |
| เวทนานุปัสสนา | ความรู้สึก/รับ | Tool results, input/output events |
| จิตตานุปัสสนา | สภาพจิต | Agent mode (planning/acting/verifying) |
| ธัมมานุปัสสนา | ธรรม/ปรากฏการณ์ | Rules applying, patterns matching |

#### 19. ปธาน 4 — Right Effort Directions
- สังวร — กันไม่ให้เกิดสิ่งไม่ดีใหม่ (lint)
- ปหาน — ละสิ่งไม่ดีที่มี (format, refactor)
- ภาวนา — สร้างสิ่งดีใหม่ (test new feature)
- อนุรักขนา — รักษาสิ่งดีที่มี (regression test)

---

### หมวด F — กรอบ Governance (Governance Frameworks)

#### 20. อปริหานิยธรรม 7 — Fleet Governance (ใหม่)
หลัก 7 ของการไม่เสื่อม (พระพุทธเจ้าสอนวัชชี) ใช้กับ governance ของ agent fleet

| ธรรม | ในระบบ |
|---|---|
| 1. ประชุมเนืองนิตย์ | Regular agent sync points |
| 2. พร้อมเพรียงประชุม / เลิก | Coordinated session start/end |
| 3. ไม่บัญญัติ/ล้มเลิกตามใจ | Convention change ผ่าน process |
| 4. เคารพผู้ใหญ่ | Honor template version hierarchy |
| 5. ไม่ข่มเหงสตรี | (เชิงสัญลักษณ์) protect vulnerable agents/users |
| 6. เคารพเจดีย์ | Honor shared resources (registry, schemas) |
| 7. คุ้มครองพระอรหันต์ | Protect senior/trusted agents from interference |

#### 21. ตัณหา 3 — Threat Model (ใหม่)
ตัณหาเป็นเหตุของความเสื่อม ใช้เป็นกรอบ threat model

| ตัณหา | ความหมาย | Threat |
|---|---|---|
| กามตัณหา | อยากในสิ่งเร้า | Prompt injection, social engineering |
| ภวตัณหา | อยากเป็น/คง | Privilege escalation, persistence |
| วิภวตัณหา | อยากไม่เป็น/ทำลาย | Destructive actions, data deletion |

#### 22. สีล 5 — Baseline Security Rules
- ห้าม `rm -rf` repo root (ปาณาติบาต — เชิงสัญลักษณ์)
- ห้าม commit secrets (อทินนาทาน)
- ห้าม spoof identity (มุสาวาท)
- ห้าม bypass gates (สุราเมระยะ — เสียสติ)
- ห้าม side effects ที่ไม่ประกาศ (กาเมสุมิจฉาจาร)

---

## 2. Design Principles (จาก 22 หลักธรรม)

| DP | หลักธรรม | Principle |
|---|---|---|
| DP-1 | โยนิโสมนสิการ | Verify before act |
| DP-2 | มัตตัญญุตา | Right amount, not maximum |
| DP-3 | สมานัตตตา | Equal treatment of backends |
| DP-4 | อนัตตา | Non-clinging workflow |
| DP-5 | อนิจจัง | Impermanence-aware memory |
| DP-6 | มัตตัญญุตา + อจินไตย | Scope discipline |
| DP-7 | อัตตัญญุตา | Self-declaration of capabilities |
| DP-8 | สีลสามัญญตา | Communal convention |
| DP-9 | ปธาน 4 | Right effort in four directions |
| DP-10 | อริยสัจ | Decisions through Four Noble Truths |
| DP-11 | ปฏิจจสมุปบาท | Trace conditions backward in failures |
| DP-12 | ภาวนา 4 | Lifecycle progression |
| DP-13 | ปัญญา 3 | Learn from study, thought, practice |
| DP-14 | พรหมวิหาร | Equanimous error handling |
| DP-15 | สติปัฏฐาน 4 | Four-foundation observability |
| DP-16 | อปริหานิยธรรม | Governance for non-decline |
| DP-17 | ตัณหา 3 | Threat model by three cravings |
| DP-18 | สีล 5 | Five baseline security rules |
| DP-19 | กัลยาณมิตร | Trust based on dhamma criteria |
| DP-20 | กรรม 3 | Audit body/speech/mind separately |

---

## 3. การประยุกต์ใน Stack

```
┌──────────────────────────────────────────────────────┐
│  อปริหานิยธรรม (Fleet Governance)                     │ ← Org level
├──────────────────────────────────────────────────────┤
│  ตัณหา 3 (Threat Model) + สีล 5 (Baseline)            │ ← Security
├──────────────────────────────────────────────────────┤
│  ภาวนา 4 (Lifecycle) + ปัญญา 3 (Improvement)         │ ← Agent growth
├──────────────────────────────────────────────────────┤
│  สาราณียธรรม + กัลยาณมิตร (Inter-agent)              │ ← Interconnect
├──────────────────────────────────────────────────────┤
│  สังคหวัตถุ + พรหมวิหาร (UX)                          │ ← User layer
├──────────────────────────────────────────────────────┤
│  มรรค 8 (Functional reqs)                            │ ← SRS
├──────────────────────────────────────────────────────┤
│  ขันธ์ 5 (Architecture)                              │ ← Components
├──────────────────────────────────────────────────────┤
│  สติปัฏฐาน 4 (Observability)                          │ ← Cross-cutting
├──────────────────────────────────────────────────────┤
│  อิทธิบาท 4 (Engine of work)                          │ ← Runtime
├──────────────────────────────────────────────────────┤
│  ไตรลักษณ์ + กรรม 3 (State & Audit)                   │ ← Foundation
├──────────────────────────────────────────────────────┤
│  ปฏิจจสมุปบาท (Failure analysis)                      │ ← When broken
├──────────────────────────────────────────────────────┤
│  โยนิโสมนสิการ + อจินไตย (Thinking discipline)         │ ← Method
└──────────────────────────────────────────────────────┘
       อริยสัจ 4 (Problem-solving cycle, end-to-end)
       สัปปุริสธรรม 7 (Context sensing, end-to-end)
```

---

## 4. การแก้ไขจาก v1.0

### 4.1 ตัด forced metaphors
- v1 ใช้ อจินไตย กับ "ไม่ debug ผลกระทบนอกขอบเขต task" — ฝืน
- v2 เปลี่ยนเป็น มัตตัญญุตา (รู้ประมาณ) ตรงกว่า
- อจินไตย ใช้เฉพาะ 4 กรณีที่ตรงกับความหมายเดิม

### 4.2 แก้การซ้ำของ cross-cutting principles
- v1 ระบุโยนิโสมนสิการใน FR, NFR, DP — ซ้ำ 3-4 ที่
- v2 ระบุ live ที่ FR-7.7 และ FR-7.17 เท่านั้น ที่อื่นเป็นการอ้างอิง

### 4.3 เพิ่ม 6 หลักธรรมใหม่
- ปฏิจจสมุปบาท → เอกสารนี้ (ไม่มีเอกสารแยก)
- ภาวนา 4 → เอกสารนี้ (ไม่มีเอกสารแยก)
- ปัญญา 3 → docs/SELF-IMPROVEMENT.md
- พรหมวิหาร 4 → docs/PRD (Error UX section)
- อปริหานิยธรรม → [`docs/th/FLEET-GOVERNANCE.th.md`](../../../../docs/th/FLEET-GOVERNANCE.th.md) (spec ระดับ framework-root สำหรับ operator ฉบับร่าง 2026-05-23)
- ตัณหา 3 → docs/THREAT-MODEL.md
- กัลยาณมิตร → [`interconnect/trust.md`](../../interconnect/trust.md) (spec draft v2026.5.23 — boolean 7 ค่า ประกาศใน `config.manifest.json` ตรวจสอบโดย `bwoc check`)
- สติปัฏฐาน → `AGENTS.md` §10 (Observability)
- อริยทรัพย์ 7 → เอกสารนี้ (maturity model)
- กรรม 3 → เอกสารนี้ (audit trail)
- สีล 5 → docs/THREAT-MODEL.md (baseline)

---

## 5. คำหมายเหตุ

หลักธรรมในเอกสารนี้ใช้เป็นกรอบความคิดทาง engineering ไม่ใช่การตีความศาสนา การ map กับ technical concept เป็น **analogy** ที่มีประโยชน์ในการคิด ไม่ใช่การอ้างว่า "พุทธสอนเรื่อง software architecture"

ผู้สนใจ dhamma ในเชิงลึก ขอแนะนำให้ศึกษาจากตำราพุทธโดยตรง
