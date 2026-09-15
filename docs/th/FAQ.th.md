---
title: คำถามที่พบบ่อย
parent: ภาษาไทย
nav_order: 9
---

# FAQ

คำถามที่ผู้มาใหม่ถามในไม่กี่ชั่วโมงแรกกับ BWOC คำตอบกระชับ พร้อม cross-reference ไปยังสเปกสำหรับรายละเอียด

---

## เชิงแนวคิด

### ต้องรู้พุทธศาสนาก่อนหรือไม่?

ไม่ ศัพท์บาลีเป็น **ป้ายชื่อ** สำหรับ concept เชิงวิศวกรรม เนื้อหาเป็นเทคนิคล้วน [`GLOSSARY.th.md`](GLOSSARY.th.md) ให้ความหมายเชิงวิศวกรรมหนึ่งบรรทัดสำหรับศัพท์บาลีทุกตัวในกรอบ — ผู้อ่านส่วนใหญ่พบว่าพอเพียง

### BWOC เป็นกรอบทางศาสนาหรือไม่?

ไม่ BWOC ใช้กรอบพุทธเป็น **เครื่องช่วยคิดเชิงวิศวกรรม** ไม่ใช่คำสอนทางศาสนา จุดยืนที่ไม่ใช่ศาสนาถูกบังคับใช้ — ดู [`VISION.th.md` §สิ่งที่ไม่ใช่เป้าหมาย](../../VISION.th.md#สิ่งที่ไม่ใช่เป้าหมาย) และหมายเหตุการ frame ของ [`CODE_OF_CONDUCT.md`](../../CODE_OF_CONDUCT.md) ผู้ contribute จากภูมิหลังใด ๆ ศรัทธาใด ๆ หรือไม่มีศรัทธาก็ยินดี

### ทำไมจึงเป็นกรอบพุทธโดยเฉพาะ?

กรอบวิศวกรรมตะวันตก (DDD, Clean Architecture, SOLID, Hexagonal) แม่นยำเรื่องโครงสร้างและการพึ่งพา แต่บางในเรื่อง **อนิจจังของสถานะ, การสาวความผิดพลาด, วงรอบ, ความเชื่อใจระหว่าง agent, และการประเมินภัย** — มิติที่ระบบ agent ล้มเหลวพอดี กรอบพุทธบังเอิญมีศัพท์ที่แม่นยำเป็นพิเศษสำหรับมิติเหล่านั้น ดู [`VISION.th.md` §ช่องว่าง](../../VISION.th.md#ช่องว่าง)

### ขัดกับ DDD / Clean Architecture / SOLID หรือไม่?

ไม่ BWOC **ขยาย** กรอบเหล่านั้นเข้าสู่มิติที่กรอบเหล่านั้นไม่ได้ออกแบบมาจัดการ พวกเขาจัดการโครงสร้าง; BWOC จัดการวงรอบ, เจตนา, ความเชื่อใจ, และวินัย ใช้ทั้งคู่

### ใช้ BWOC โดยไม่มี framing แบบพุทธได้หรือไม่?

ได้ เก็บโครงเทคนิคไว้ — manifest, neutrality, lifecycle, threat model, CLI surface คุณสูญเสีย "why" ที่รวมเป็นหนึ่งเดียวอยู่เบื้องหลังการตัดสินใจ และศัพท์ร่วม แต่ไม่มีอะไรใน implementation ที่ต้องการ framing

---

## กลไกของ Project

### Phase กับ Version ต่างกันอย่างไร?

**Phase** อธิบาย milestone ของการ implement (Phase 1 v2.0 = รากฐาน อุปฺปาท, Phase 2 = การปฏิบัติ ฐิติ, Phase 3 = วยะ + interconnect, Phase 4 = reference agent + fleet) **Version** อธิบาย identity ของ release (SemVer) Phase หนึ่งอาจครอบคลุม SemVer release หลายครั้ง ดู [`VERSION.md` §Phase vs Version](../../VERSION.md#phase-vs-version)

### ทำไมเอกสารถูกเขียนก่อน code?

Documents-first คือหลักการ Code ตามสเปก ไม่ใช่ตรงข้าม การ map กรอบ 22 ประการ, วงรอบ (อุปฺปาท · ฐิติ · วยะ), โครงสร้าง workspace, และ CLI surface ถูก spec ใน Markdown ทั้งหมดก่อน Rust workspace ถูก scaffold วินัยคือ **โยนิโสมนสิการ** — ตรวจสอบเจตนาก่อนการกระทำ

### `Software-Version` กับ `Document-Version` ต่างกันอย่างไร?

วิวัฒน์อย่างอิสระ `Software-Version` อยู่ใน `Cargo.toml` ติดตามการเปลี่ยน code (`.rs` / `.toml` edit bump มัน) `Document-Version` อยู่ใน `VERSION.md` ติดตามการเปลี่ยนเอกสาร (`.md` edit bump มัน) ทั้งคู่ auto-bumped ทุก edit ของ Claude Code โดย `.claude/hooks/auto-version.sh` ดู [`VERSION.md`](../../VERSION.md)

---

## Setup

### สร้าง agent ใหม่อย่างไร?

```bash
bwoc new <agent-name>
```

แล้วตรวจ manifest, กำหนด persona, รัน `bwoc check` เป้าหมาย: commit แรกภายในไม่ถึง 30 นาที walkthrough เต็มใน [`INCARNATION.th.md`](INCARNATION.th.md)

### Agent ที่ incarnate แล้วอยู่ที่ใดบนดิสก์?

ที่ใดก็ได้ที่คุณต้องการ แต่ละ agent เป็น repository ที่อยู่ได้ด้วยตนเอง copy จาก template layout ที่แนะนำคือภายใน **workspace** ที่ `<workspace>/agents/agent-<name>/` แต่คุณวาง agent ที่ใดก็ได้ที่ filesystem และ workflow version-control ของคุณชอบ ไม่มี registry กลาง ดู [`WORKSPACE.th.md`](WORKSPACE.th.md)

### *Workspace* คืออะไรและจำเป็นต้องมีหรือไม่?

Workspace คือ directory ที่ CLI ใช้เป็นบ้านสำหรับงาน BWOC ของคุณ มี `.bwoc/` marker (`workspace.toml`, `agents.toml`), memory ระดับ workspace (ทางเลือก), และ directory `agents/` สำหรับ agent ที่ incarnate **คุณต้องมี** เพื่อใช้คำสั่ง CLI เชิงปฏิบัติการ (`bwoc spawn`, `bwoc list` ฯลฯ) — พวกมันปฏิเสธรันหาก workspace ไม่ครบถ้วน รัน `bwoc init` เพื่อสร้าง ดู [`WORKSPACE.th.md`](WORKSPACE.th.md)

### อะไรอยู่ใน `~/.bwoc/`?

State ระดับ user ระดับเครื่องที่อิสระจาก workspace ใด: `config.toml` (default backend, default language, default workspace), `memory/` (central memory แชร์โดยทุก agent ที่คุณรันบนเครื่องนี้), `workspaces.toml` (registry ของ workspace ที่ CLI เห็น), และ `logs/` (log การเรียกใช้ CLI) ดู [`WORKSPACE.th.md` §Central Memory](WORKSPACE.th.md#central-memory--bwoc)

---

## Multi-Language และ Multi-Backend

### เพิ่ม LLM backend ใหม่อย่างไร?

คำสั่งเดียว ไม่ต้องเปลี่ยน code:

```bash
ln -s AGENTS.md <BACKEND>.md
```

Backend อ่าน `AGENTS.md` ผ่าน symlink ของตน; ไม่มี instruction แยกตาม backend โดยการออกแบบ (สมานัตตตา — การปฏิบัติเท่าเทียม) แล้วรัน `bwoc check` ใหม่เพื่อยืนยัน ดู [`INCARNATION.th.md` §เพิ่ม Backend](INCARNATION.th.md#เพิ่ม-backend)

### เพิ่มภาษามนุษย์ใหม่สำหรับเอกสารอย่างไร?

```bash
mkdir docs/<lang>          # <lang> = BCP 47 / ISO 639-1 (เช่น "ja", "zh", "de")
# แปลแต่ละ docs/en/<NAME>.en.md ไปเป็น docs/<lang>/<NAME>.<lang>.md
```

ภาษาอังกฤษเป็น canonical; ภาษาอื่นเป็นคำแปล Framework root, agent template, และ CLI ทั้งหมดใช้รูปแบบ `<lang>` เดียวกัน (`docs/<lang>/<NAME>.<lang>.md` + `FILENAME.md` ↔ `FILENAME.<lang>.md` ที่ root + `crates/bwoc-cli/locales/<lang>/cli.ftl`) ดู [`ARCHITECTURE.th.md` §โครงสร้างหลายภาษา](ARCHITECTURE.th.md#โครงสร้างหลายภาษา)

### เปลี่ยนภาษาของ output ของ CLI อย่างไร?

ลำดับความสำคัญ: flag `--lang <code>` → env `BWOC_LANG` → env `$LANG` → fallback `en` CLI ship ด้วย TH และ EN ตั้งแต่เริ่ม; การเพิ่มภาษาที่สามคือการวาง folder ใน `crates/bwoc-cli/locales/`

---

## Convention

### ใช้รูปแบบใดตั้งชื่อไฟล์ Markdown ใหม่?

แหล่งความจริงเดียวคือ [`NAMING.th.md`](NAMING.th.md) — 12 หมวดพร้อม decision tree สรุปเร็ว:

- Metadata ระดับ project (มาตรฐาน OSS) → `UPPERCASE.md`
- เอกสารสเปก → `UPPERCASE.<lang>.md` ใน `docs/<lang>/`
- prose ของ template → `lowercase-hyphen.md`
- Slot README → `README.md` (รูปแบบ Obsidian)
- Crate README → `README.md` (plain Markdown)
- Skill → `SKILL.md`
- ดัชนี memory → `MEMORY.md`; รายการ → `<type>_<slug>.md`
- **Note → `YYYY-MM-DD_<title>.md`**
- คำแปลของไฟล์ root → `FILENAME.<lang>.md` (เช่น `VISION.th.md`)

### Session note หรือ decision record ควรไว้ที่ใด?

Note ตามรูปแบบ `YYYY-MM-DD_<title>.md` ที่อยู่ถูกต้องสามที่: `<repo>/notes/` (ระดับ project), `<workspace>/.bwoc/notes/` (scope workspace), `~/.bwoc/notes/` (per-user) เลือก scope ที่ตรงกับผู้รับของ note ดู [`NAMING.th.md` §Note](NAMING.th.md#yyyy-mm-dd_title-md--note-ใหม่)

---

## การปฏิบัติการ

### รัน agent หลายตัวพร้อมกันได้หรือไม่?

ได้ แต่ละ agent เป็น repo ที่อยู่ได้ด้วยตนเอง พร้อม subprocess ของ backend ของตน Phase 1 spawn แยกกัน; Phase 2 เพิ่ม control socket ของ `bwoc-agent` ให้ CLI supervise; Phase 3 เพิ่ม inter-agent messaging สำหรับการประสานงาน (สัมมาวาจา + Sāraṇīyadhamma 6) ดู [`ROADMAP.th.md`](ROADMAP.th.md)

### เกิดอะไรขึ้นเมื่อ agent จบงาน?

เข้าสู่ **วยะ** — ระยะดับ Worktree ถูกล้าง, branch ถูกปล่อย, memory ถูกตัด, task ถูกปิด Phase 1 ปล่อยให้ทำเอง; Phase 3 แนะนำ `bwoc retire <name>` ที่ทำ cleanup เต็มรูปแบบในครั้งเดียว วินัยคือ **อนัตตา** — ไม่ยึดมั่น

### ถ้าเจอประเด็นความปลอดภัยทำอย่างไร?

อย่าเปิด issue สาธารณะ ส่ง email ไปที่ **info@bemind.tech** พร้อมรายละเอียด ดู [`SECURITY.md`](../../SECURITY.md) สำหรับขั้นตอนการเปิดเผยและ [`THREAT-MODEL.th.md`](../../modules/agent-template/docs/th/THREAT-MODEL.th.md) สำหรับ surface ภัยเต็มรูปแบบ

---

## การ Contribute

### Contribute อย่างไร?

ดู [`CONTRIBUTING.md`](../../CONTRIBUTING.md) สำหรับ workflow, commit style, และ PR checklist ผู้ contribute ใหม่อ่านตามลำดับนี้: [`VISION.th.md`](../../VISION.th.md) → [`GLOSSARY.th.md`](GLOSSARY.th.md) → [`PHILOSOPHY.th.md`](../../modules/agent-template/docs/th/PHILOSOPHY.th.md) (หมวด A–F) → [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) → พื้นที่ที่ต้องการทำงาน

### Contribute เป็นภาษาอื่นนอกจากอังกฤษได้หรือไม่?

การ contribute คำแปลยินดีต้อนรับ เอกสารสเปกตามกฎ bilingual: ทุก EN doc มีคำแปลที่จับคู่ใน `docs/<lang>/<NAME>.<lang>.md` เปิด PR พร้อมทั้งการแก้ EN และการแก้คำแปลที่จับคู่; hook bilingual-reminder จะ flag การไม่จับคู่

### ถ้าหลักการของ framework กับ use case ของฉันไม่ตรงกัน?

เปิด issue อธิบาย friction Framework เป็น normative แต่ไม่ infallible หลักการวิวัฒน์ผ่านวงจรอริยสัจ 4 (ทุกข์ → สมุทัย → นิโรธ → มรรค) เดียวกับที่ขอจาก agent

---

## ดูเพิ่ม

- [`VISION.th.md`](../../VISION.th.md) — ทำไม BWOC ถึงมีอยู่
- [`PHILOSOPHY.th.md`](../../modules/agent-template/docs/th/PHILOSOPHY.th.md) — แกนความคิดเต็มรูปแบบ
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — ค้นหาคำบาลี
- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — ส่วนประกอบทำงานร่วมกันอย่างไร
- [`INCARNATION.th.md`](INCARNATION.th.md) — สร้าง agent ใหม่อย่างไร
- [`WORKSPACE.th.md`](WORKSPACE.th.md) — โครงสร้าง workspace และ central memory
- [`NAMING.th.md`](NAMING.th.md) — มาตรฐานการตั้งชื่อไฟล์ Markdown
- [`ROADMAP.th.md`](ROADMAP.th.md) — แผน phase
