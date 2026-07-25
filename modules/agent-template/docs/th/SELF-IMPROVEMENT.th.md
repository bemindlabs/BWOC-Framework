# SELF-IMPROVEMENT — Learning Loop (โครงตามปัญญา 3)

| | |
|---|---|
| **เอกสาร** | docs/SELF-IMPROVEMENT.th.md |
| **ภาษาคู่** | SELF-IMPROVEMENT.en.md |
| **กรอบหลัก** | ปัญญา 3 (Three Roots of Wisdom) |
| **เสริม** | อิทธิบาท 4 (วิมังสา), ภาวนา 4 |

---

## 0. หลักการ

ปัญญาตามพุทธไม่เกิดจากที่เดียว มาจาก 3 ทาง

| ปัญญา | ความหมาย | ในระบบ |
|---|---|---|
| สุตมยปัญญา | ฟัง อ่าน เรียน | Study docs, conventions, examples |
| จินตามยปัญญา | คิดต่อ ใคร่ครวญ | Synthesis, pattern extraction |
| ภาวนามยปัญญา | ปฏิบัติ ทดลอง | Feedback, retrospectives |

> **กฎ:** ปัญญาที่ใช้ได้จริง = 3 ทางครบ ขาดทางใดทางหนึ่ง → ตื้นหรือผิด

---

## 1. สุตมยปัญญา (Sutamayā) — Learn from Study

### 1.1 Inputs
- AGENTS.md (และ symlinks)
- conventions/*.md
- docs/* (PHILOSOPHY → ARCHITECTURE)
- examples/*
- Skill files จาก agent อื่น (capabilities.md)
- Tier 2 memory (cross-agent insights)

### 1.2 Activities
- **Session start:** load relevant docs ก่อนทำงาน
- **Pre-task:** read related memories
- **On unknown:** ค้นหา skill files และ knowledge base

### 1.3 Storage
Memory files type: `reference-*`

```markdown
# memories/reference-postgres-naming.md
---
type: reference
source: conventions/database.md#naming
date: 2026-05-22
verifiedAgainst: schema.sql@abc123
---

PostgreSQL naming conventions used here:
- Tables: snake_case plural
- Columns: snake_case
- Indexes: idx_<table>_<columns>
```

### 1.4 Quality Check
- Source traceable? (มี link/reference)
- Verified date? (เคยตรวจกับ code จริงไหม)
- Selective? (ไม่ใช่ดูดมาทั้งหมด → มัตตัญญุตา)

---

## 2. จินตามยปัญญา (Cintāmayā) — Learn from Reflection

### 2.1 Activities
- **Pattern extraction:** หลังทำหลาย tasks คล้ายกัน → ดูว่ามี pattern อะไร
- **Decision rationale:** ก่อนตัดสินใจใหญ่ → เขียน rationale, alternatives
- **Synthesis:** เชื่อม sutta หลายแหล่งเข้าด้วยกัน
- **Mental simulation:** "ถ้าทำแบบนี้ จะเกิดอะไร" — โยนิโสมนสิการ

### 2.2 Storage
Memory files type: `project-*` หรือ `decision-*`

```markdown
# memories/decision-2026-05-22-caching-strategy.md
---
type: decision
date: 2026-05-22
status: active
references:
  - reference-redis-cluster.md
  - feedback-PROJ-30-cache-thrashing.md
---

## ตัดสินใจ
ใช้ Redis Sentinel แทน Cluster mode

## ทางเลือกที่พิจารณา
- A: Redis Cluster — ความซับซ้อนสูงเกินสำหรับ scale ปัจจุบัน
- B: Redis Sentinel — เลือก
- C: Memcached — ขาด persistence

## เหตุผล
อ้างอิง reference-redis-cluster.md + feedback-PROJ-30
Sentinel ตรงกับ scale และ availability ที่ต้องการ

## เงื่อนไขที่ทำให้ revisit
- Scale เกิน 50k req/s
- Multi-region requirement
```

### 2.3 Quality Check
- Alternatives พิจารณาหรือยัง
- Rationale อ้างอิงสุตอะไรบ้าง
- Revisit conditions ระบุชัด

### 2.4 Core Reflection — พิธีกรรม Like / Wish / Next Step

หัวใจภาคปฏิบัติที่ทำซ้ำได้ของจินตามยปัญญา: **retrospective สามบรรทัด** ทำตอนจบทุกงานและทุก session. เป็น reflection ที่เล็กที่สุดที่ยังเปลี่ยนประสบการณ์ดิบเป็นความรู้ได้ — ถูกพอจะทำ *ทุกครั้ง* (**มัตตัญญุตา**) มีโครงพอจะทบต้นได้

ตั้งอยู่บน **ปัจจเวกขณะ (Paccavekkhaṇa)** — การพิจารณาทบทวน; พุทธโอวาทแก่ราหุล (MN 61) ให้ทบทวนทุกการกระทำ *ก่อน ระหว่าง และหลัง*. Core Reflection คือ "หลัง" อย่างมีวินัย

**สามง่าม:**

| ง่าม | ถามว่า | ได้ผลเป็น | ป้อนเข้า |
|---|---|---|---|
| **Like** | อะไรเวิร์ก — ควรเก็บ/ทำซ้ำ? | pattern ที่ถูกตอกย้ำ | สุต (`reference-*`) หรือ capability |
| **Wish** | อะไรยังขาด — ช่องว่างระหว่างที่คาดกับที่เกิดจริง? | *ช่องว่างที่ตั้งชื่อได้* (ไม่ใช่การบ่น) | จินตา (`decision-*`) หรือ `feedback-*` |
| **Next step** | สิ่งเดียวที่มีประโยชน์สุดที่จะเปลี่ยนครั้งหน้า? | ข้อผูกพันรูปธรรมหนึ่งข้อที่รับผิดชอบเอง (**อธิษฐาน**) | ภาวนา (hypothesis ของงานถัดไป) |

**กฎที่ทำให้มันซื่อสัตย์:**
- **บรรทัดละหนึ่ง** ถ้าง่ามไหนต้องเขียนเป็นย่อหน้า นั่นคือ `decision-*` ไม่ใช่บรรทัด reflection — เลื่อนขั้นมัน
- **Wish ตั้งชื่อช่องว่าง ไม่ใช่คำบ่น** "wish ว่าน่าจะลอง migration บน copy ก่อน" — ทำได้จริง ไม่ใช่ "schema มันห่วย"
- **Next step มีข้อเดียวเป๊ะ** หลายข้อ = ไม่มีเลย เลือกข้อ leverage สูงสุด ที่เหลือรอก่อน
- **Next step รับผิดชอบเอง + รูปธรรม** — มีกริยาและประธานที่เราคุมได้

**Template** (ต่อท้าย `feedback-*` ของงาน หรือ session note):

```markdown
## Core Reflection
- Like: <อะไรที่เวิร์ก เก็บไว้>
- Wish: <ช่องว่าง — ที่คาดกับที่เกิดจริง ทำได้จริง>
- Next step: <การเปลี่ยนรูปธรรมหนึ่งข้อ รับผิดชอบเอง>
```

**ตัวอย่าง:**

```markdown
## Core Reflection
- Like: เขียน rollback script ก่อน migration — ทำให้รันพลาดกลายเป็นเรื่องเล็ก
- Wish: ทดสอบ migration กับข้อมูลรูปทรงเหมือน prod ช้าไป สมมติฐานจำนวน row พังตอน scale เท่านั้น
- Next step: เพิ่มขั้น "test บน copy ขนาดเท่า prod" ใน checklist migration ก่อน ALTER ทุกครั้ง
```

**มันร้อยทุก stage อย่างไร** — Core Reflection คือเนื้อเยื่อเชื่อมของ Wisdom Loop (§4). ทำที่รอยต่อของแต่ละ stage แล้วมันแปลง output ของ stage นั้นเป็น input ของ stage ถัดไป:

| Stage | Core Reflection ถามตรงนั้นว่า | Next step กลายเป็น |
|---|---|---|
| หลัง **Suta** (ศึกษา) | Like: source ไหนช่วยจริง? · Wish: อะไรขาด/เก่าไป? | source ที่ดีกว่าให้โหลดครั้งหน้า (หรือ verification ที่ต้องรัน) |
| หลัง **Cintā** (ตัดสินใจ) | Like: ทางเลือกไหนอยู่ตัว? · Wish: สมมติฐานไหนอ่อนสุด? | เงื่อนไข `Revisit If` บน `decision-*` |
| หลัง **Bhāvanā** (ปฏิบัติ) | Like: อะไรเวิร์กตอนทำ? · Wish: ช่องว่างที่คาดกับที่เกิด? | hypothesis ของครั้งถัดไป (→ สุตใหม่หลัง curation) |

**การเลื่อนขั้น:** เมื่อ Wish *เดิม* เกิดซ้ำ 3+ ครั้ง แปลว่า Next step ของมันยังไม่ลง — ดันขึ้น Curation Pipeline (§5): จากบรรทัด reflection ส่วนตัว → `decision-*` → capability ที่แชร์ → convention. Wish ที่เกิดซ้ำคือฟลีตกำลังบอกว่า *habit* ไม่ใช่ *task* ที่ต้องเปลี่ยน

---

## 3. ภาวนามยปัญญา (Bhāvanāmayā) — Learn from Practice

### 3.1 Activities
- **Post-task feedback:** บันทึกผลจริง vs ที่คาด
- **Post-mortem:** ปฏิจจสมุปบาท chain analysis
- **Retrospective:** Weekly review
- **A/B observation:** ผลของ pattern ในการใช้งานจริง

### 3.2 Storage
Memory files type: `feedback-*`

```markdown
# memories/feedback-PROJ-42-schema-migration.md
---
type: feedback
date: 2026-05-22
task: PROJ-42
outcome: success-with-issues
---

## คาดไว้
Migration จะใช้เวลา < 30 นาที, no downtime

## เกิดจริง
- 47 นาที (เกิน estimate 50%)
- Brief lock บน users table 2 วินาที

## เพราะอะไร (ปฏิจจสมุปบาท สั้น)
- อวิชชา: ไม่รู้ขนาดจริงของ users (ดูแค่ count, ไม่ดู indexes)
- สังขาร: estimate ผิด → ไม่ได้วาง schedule ตอน low-traffic

## บทเรียน
- เพิ่ม pre-migration size check ใน skill file
- Update reference-schema-migration.md

## เกี่ยวข้องกับ
- ปรับ feedback ใน convention หรือไม่: ใช่ → ส่ง CCP
```

### 3.3 Quality Check
- คาด vs จริง ระบุชัด
- ห่วงโซ่เหตุผล (ไม่ใช่แค่ "เกิดอะไร")
- Action items concrete

---

## 4. The Wisdom Loop

```
       ┌─────────────────────────────┐
       │  สุต (Study)                 │
       │  reference-*.md             │
       └──────────┬──────────────────┘
                  │ informs
                  ▼
       ┌─────────────────────────────┐
       │  จินตา (Reflect)             │
       │  decision-*.md, project-*.md│
       └──────────┬──────────────────┘
                  │ becomes hypothesis
                  ▼
       ┌─────────────────────────────┐
       │  ภาวนา (Practice)            │
       │  feedback-*.md              │
       └──────────┬──────────────────┘
                  │
                  │ feeds back as new สุต
                  │ (after curation)
                  ▼
           Updated สุต (via Tier 2 mining)
```

**พิธีกรรมที่ขับ loop นี้คือ Core Reflection (§2.4).** ทำ retro Like / Wish / Next Step ที่ลูกศรทุกอัน: *Like* ตอกย้ำ stage ที่เพิ่งออกมา, *Wish* ตั้งชื่อช่องว่างที่พาไป stage ถัดไป, และ *Next step* คือ hypothesis ที่พกเข้าไป. ไม่มีพิธีกรรม loop ก็เป็นแค่ไดอะแกรม; มีมัน ทุกงานหมุนวงล้อไปหนึ่งรอบ

---

## 5. Curation Pipeline

ไม่ใช่ทุก feedback เป็น knowledge ต้อง curate

### ระดับ 1 — Personal (Tier 1)
- Agent เก็บใน memories/ ของตัวเอง
- Verify ใน next session

### ระดับ 2 — Pattern Detected
- หลังเจอ pattern เดิม 3+ ครั้ง → mine เข้า decision-*.md
- เริ่ม share ใน capabilities.md

### ระดับ 3 — Cross-Agent (Tier 2)
- เมื่อ pattern เป็นประโยชน์กว่าหนึ่ง agent
- Mine เข้า Tier 2 memory
- ใส่ใน skill files

### ระดับ 4 — Convention
- เมื่อ pattern เป็น best practice ของ fleet
- เสนอผ่าน CCP (FLEET-GOVERNANCE §3)

---

## 6. Self-Improvement Metrics

### 6.1 สุต metrics
- Source diversity: อ้าง source หลายแห่งใน decisions
- Verification rate: % ของ references ที่ verified
- Reading depth: time spent loading docs

### 6.2 จินตา metrics
- Decision quality: alternatives considered per decision
- Synthesis count: cross-references in memory files
- Revisit accuracy: เมื่อ revisit, decision ถูกแก้บ้างไหม

### 6.3 ภาวนา metrics
- Post-mortem completion rate
- Action item closure rate
- Pattern detection latency: เจอ pattern หลังกี่ครั้ง

### 6.4 Combined: วิมังสา (Iddhipāda 4)
- Improvement velocity: feedback → action time
- Knowledge half-life: memory ใช้ได้นานแค่ไหนก่อนต้อง verify

---

## 7. Anti-Patterns

| Pattern | ขาดทางไหน |
|---|---|
| Memorizing docs without testing | ขาด ภาวนา |
| Patching without analysis | ขาด จินตา |
| Reinventing patterns | ขาด สุต |
| Endless reflection, no action | ขาด ภาวนา |
| Cargo-culting from other agents | ขาด จินตา + ภาวนา |

---

## 8. Triggers สำหรับ Self-Improvement

### Trigger 1 — Task Failure
→ Post-mortem (ภาวนา) + check existing สุต

### Trigger 2 — Repeated Same Issue
→ Pattern extraction (จินตา) + mine to Tier 2

### Trigger 3 — Promotion Eligibility
→ Demonstrate ทั้ง 3 ปัญญา (Ariya-dhana L5/L6)

### Trigger 4 — Convention Update
→ Re-read affected สุต, update references

### Trigger 5 — Fleet Sync
→ Share insights (สาราณียธรรม + จาคะ)

---

## 9. ความสัมพันธ์กับเอกสารอื่น

| เอกสาร | เชื่อมอย่างไร |
|---|---|
| PHILOSOPHY | ปัญญา 3 (DP-13), อิทธิบาท วิมังสา |
| LIFECYCLE | L4 → L5 ต้องครบ ปัญญา 3 |
| FAILURE-MODES | Post-mortem feeds ภาวนา |
| OBSERVABILITY | Rule application logs = สุต source |
| FLEET-GOVERNANCE | Mined patterns → CCP |
| SRS | FR-8 (Sammā-samādhi) memory system |
