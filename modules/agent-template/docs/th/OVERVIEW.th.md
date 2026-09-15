# OVERVIEW — Agent Base Profile

| | |
|---|---|
| **เอกสาร** | docs/OVERVIEW.th.md |
| **เวอร์ชัน** | 1.0 |
| **วันที่** | 2026-05-22 |
| **ภาษาคู่** | OVERVIEW.en.md |

> เอกสารนี้คือ**ประตูเข้า** ใช้เวลาอ่าน 5 นาที จะรู้ว่าระบบนี้คืออะไร อ่านอะไรต่อ

---

## คืออะไร

Agent Base Profile คือ **template สำหรับสร้าง AI coding agent** ที่ออกแบบทั้งระบบตามหลักพุทธ

- **หนึ่งรีโป หนึ่ง agent** — แต่ละ agent อยู่ในรีโปของตัวเอง โคลนจาก template นี้
- **Backend-neutral** — รันได้บน Claude, Antigravity, Codex, Kimi
- **จำได้ + เชื่อมโยงได้** — สะสมความรู้ข้ามเซสชัน
- **ทำงานคู่กันได้** — agent หลายตัวในรีโปเดียวกัน ไม่ชน

---

## ทำไมต้องใช้หลักพุทธ

ไม่ใช่การตกแต่ง แต่หลักพุทธให้ **กรอบความคิดทาง engineering ที่ครบและลึก** สำหรับปัญหาที่ AI agent เจอจริง

| ปัญหา | กรอบพุทธที่ใช้ |
|---|---|
| ออกแบบ requirements | มรรค 8 |
| วางสถาปัตยกรรม | ขันธ์ 5 |
| แก้ปัญหา | อริยสัจ 4 |
| จัดการ state | ไตรลักษณ์ |
| Audit log | กรรม 3 |
| Observability | สติปัฏฐาน 4 |
| Failure analysis | ปฏิจจสมุปบาท |
| Lifecycle | ภาวนา 4 |
| Self-improvement | ปัญญา 3 |
| Threat model | ตัณหา 3 |
| Fleet governance | อปริหานิยธรรม 7 |
| Error UX | พรหมวิหาร 4 |
| Inter-agent trust | กัลยาณมิตร 7 |

ดูรายละเอียดที่ [PHILOSOPHY.th.md](PHILOSOPHY.th.md)

---

## เริ่มต้น

### ผมเป็น Agent Author (จะสร้าง agent ใหม่)
```bash
bwoc new <agent-name>
cd agents/agent-<agent-name>
# แก้ persona/README.md
# ตรวจ config.manifest.json
bwoc check .
```
อ่านต่อ: [LIFECYCLE.th.md](LIFECYCLE.th.md) → กายภาวนา section

### ผมเป็น Agent Operator (จะใช้งาน agent)
อ่านต่อ: [OVERVIEW → SRS section 5 → examples/workflow/](../examples/workflow/)

### ผมเป็น Platform Maintainer (ดูแล template)
อ่าน: [GLOSSARY](GLOSSARY.th.md) → [PHILOSOPHY](PHILOSOPHY.th.md) → ทั้งหมด

### ผมอยากเข้าใจปรัชญาก่อน
อ่าน: [PHILOSOPHY.th.md](PHILOSOPHY.th.md)

---

## แผนผังเอกสาร

```
docs/
├── PHILOSOPHY.{th,en}.md          ← รากฐานหลักพุทธ (อ่านก่อน)
├── OVERVIEW.{th,en}.md            ← ไฟล์นี้
├── GLOSSARY.{th,en}.md            ← ศัพท์บาลีและเทคนิค
│
├── PRD.{th,en}.md                 ← Product (อริยสัจ 4)
├── SRS.{th,en}.md                 ← Requirements (มรรค 8)
├── ARCHITECTURE.{th,en}.md        ← Architecture (ขันธ์ 5)
│
├── LIFECYCLE.{th,en}.md           ← Agent lifecycle (ภาวนา 4)
├── OBSERVABILITY.{th,en}.md       ← Monitoring (สติปัฏฐาน 4)
├── FAILURE-MODES.{th,en}.md       ← Failures (ปฏิจจสมุปบาท)
├── SELF-IMPROVEMENT.{th,en}.md    ← Learning (ปัญญา 3)
│
├── COORDINATION-PROTOCOL.{th,en}.md  ← Inter-agent (กัลยาณมิตร)
├── FLEET-GOVERNANCE.{th,en}.md       ← Org (อปริหานิยธรรม)
├── THREAT-MODEL.{th,en}.md           ← Security (ตัณหา 3)
│
└── ANTIPATTERNS.{th,en}.md        ← ทางผิดของแต่ละมรรค

examples/
├── persona/                       ← ตัวอย่าง persona ดี/แย่
├── memory/                        ← ตัวอย่าง memory file
├── capabilities/                  ← ตัวอย่าง capabilities.md
├── task-log/                      ← ตัวอย่าง task-log.jsonl
└── workflow/                      ← ขั้นตอนตัวอย่าง
```

---

## ลำดับการอ่าน (แนะนำ)

### 🟢 Path 1 — เร่งด่วน (30 นาที)
1. OVERVIEW (ที่นี่)
2. examples/workflow/incarnation.md
3. examples/workflow/first-task.md

### 🟡 Path 2 — ทำความเข้าใจ (2 ชั่วโมง)
1. OVERVIEW
2. PHILOSOPHY (skim หมวด A-F)
3. PRD
4. SRS
5. ARCHITECTURE

### 🔴 Path 3 — ลึก (วันเดียว)
อ่านครบทุกไฟล์ตามลำดับใน docs/ + examples/

---

## หลักการ 5 ข้อที่ต้องรู้

จากทั้งหมด 22 หลักธรรม นี่คือ 5 ข้อที่ใช้บ่อยที่สุด

### 1. โยนิโสมนสิการ — Verify Before Act
Memory คือคำกล่าวอ้างในอดีต ตรวจกับสภาพปัจจุบันก่อนเชื่อ

### 2. มัตตัญญุตา — Right Amount
MEMORY.md ไม่เกิน 200 บรรทัด เพื่อบังคับให้เลือกแต่ที่สำคัญ

### 3. อนัตตา — Non-Clinging
Task เสร็จ → cleanup worktree → ลบ branch ไม่ยึด

### 4. สมานัตตตา — Equal Treatment
ทุก backend เท่าเทียม ผ่าน symlink ชี้ AGENTS.md เดียว

### 5. สีลสามัญญตา — Communal Convention
ทุก agent ใต้กติกาเดียวกัน ผ่าน conventions.md + neutrality check

---

## คำถามที่พบบ่อย

**Q: ต้องเป็นพุทธหรือเข้าใจพุทธไหม?**
A: ไม่ต้อง คุณใช้เป็น engineering framework ได้เลย คำบาลีเป็นชื่อหัวข้อ เนื้อหาเป็นเทคนิค

**Q: ทำไมไม่ใช้ DDD, Clean Architecture, SOLID?**
A: ใช้ได้ครับ และไม่ขัดกัน หลักพุทธให้กรอบเพิ่ม โดยเฉพาะเรื่อง state, failure, lifecycle ที่กรอบตะวันตกไม่ครอบคลุม

**Q: เอกสารเยอะมาก ต้องอ่านทั้งหมดไหม?**
A: ไม่ต้อง อ่าน OVERVIEW + PHILOSOPHY ก่อน เอกสารอื่นเปิดตามต้องการ

**Q: ถ้าไม่ชอบหลักพุทธ ใช้ template นี้ได้ไหม?**
A: ได้ — ตัด PHILOSOPHY ออก ใช้เฉพาะโครงเทคนิคได้ แต่จะเสียคำอธิบาย "ทำไม" ของแต่ละ design decision

---

## สถานะปัจจุบัน

| ส่วน | สถานะ |
|---|---|
| Core docs (PHILOSOPHY, PRD, SRS, ARCH) | ✅ พร้อม |
| Lifecycle, Observability, Failure, Improvement | ✅ พร้อม |
| Coordination, Governance, Threat | ✅ พร้อม |
| Examples | ✅ พร้อม |
| Reference agents | ⏳ Phase 4 |
| Fleet dashboard | ⏳ Phase 4 |
