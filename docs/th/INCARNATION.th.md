---
title: การถือกำเนิด
parent: ภาษาไทย
nav_order: 2
---

# การ Incarnation

วิธีสร้าง BWOC agent ใหม่จาก template — เริ่มต้นจนถึง commit แรกภายในไม่ถึง 30 นาที

เอกสารนี้เป็น **แหล่งความจริงเดียว** สำหรับการ incarnation README และ agent-template README ให้ quickstart ที่ link มาที่นี่

---

## "Incarnation" คืออะไร

agent ใหม่เกิดขึ้นโดยการ copy [`modules/agent-template/`](../../modules/agent-template/) ไปยัง directory ของตน, resolve `{{placeholders}}` ตามตัวตน, แล้ว validate backend neutrality นี่คือระยะ **อุปฺปาท** ของวงรอบ BWOC — สร้างตัวตน, ประกาศความสามารถ, resolve manifest

หลัง incarnation agent คือ repository ที่อยู่ได้ด้วยตนเอง สามารถย้าย version control และทำงานได้อิสระ ไม่มี registry กลาง framework จัดเตรียม recipe — agent เป็นเจ้าของ instance

---

## สิ่งที่ต้องมีก่อน

- `bwoc` CLI บน PATH (ดู [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md))
- `git` บน PATH
- (เสริม) Backend CLI ที่เลือก — `claude`, `agy`, `codex`, `kimi`, หรือ `ollama` (ผ่าน `bwoc-harness`) — ติดตั้งที่จุดที่จะใช้งาน agent

---

## เส้นทาง Canonical

```bash
bwoc new <agent-name>
```

- **`<agent-name>`** — ตัวพิมพ์เล็ก คั่นด้วย hyphen (เช่น `database-schema`); directory จะชื่อ `agent-<agent-name>`
- **`--target <path>`** — ทางเลือก ค่าเริ่มต้น: `<workspace>/agents/agent-<agent-name>` เมื่อ resolve workspace ได้; ไม่เช่นนั้นวางข้าง template หรือใต้ directory ปัจจุบัน

`bwoc new` copy template (auto-detect `modules/agent-template/` หรือใช้ชุดที่ embed ใน binary), เขียน `config.manifest.json` ที่ resolve แล้ว, แทนค่า field ของ manifest ลงใน `AGENTS.md` และ `persona/README.md`, สร้าง backend symlinks (`CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md`, … → `AGENTS.md`), register agent ใน `.bwoc/agents.toml` เมื่อ resolve workspace ได้ แล้วพิมพ์ขั้นตอนถัดไป ไม่รัน `git init` และไม่ commit

---

## การตั้ง Manifest

`bwoc new` รับ field ของ manifest เป็น input, validate, แล้วเขียน manifest ที่ resolve แล้ว โหมด input สองแบบ:

- **Flags** — field ที่จำเป็นทุกตัวมี flag ตัวอย่าง:
  ```bash
  bwoc new <name> \
    --role "database schema reviewer" \
    --primary-model claude-opus-4-8 \
    --fallback-model claude-haiku-4-5 \
    --lint-cmd "cargo clippy" \
    --format-cmd "cargo fmt" \
    --test-cmd "cargo test" \
    --build-cmd "cargo build"
  ```
- **Interactive prompts** — field ที่จำเป็นซึ่งขาดจะ trigger prompt บน TTY พร้อมคำอธิบายของ field จาก `config.manifest.json` `requiredConfig.<field>.description` ใน context ที่ไม่ใช่ TTY (CI) ล้มเหลวทันทีพร้อม list ของ field ที่ขาด

Field ที่จำเป็นและ schema ของมันอยู่ใน `modules/agent-template/config.manifest.json` `requiredConfig` CLI อ่าน schema นั้นตอน incarnation; การเพิ่ม field ที่จำเป็นใหม่คือการเปลี่ยน schema ของ manifest ไม่ใช่การเปลี่ยน CLI

## การแก้ Manifest หลัง Incarnation

Manifest เป็นของ agent หลัง incarnation **แก้ `config.manifest.json` โดยตรง** ด้วย editor ของคุณ — นั่นคือเส้นทาง canonical

Phase 2 อาจเพิ่ม `bwoc manifest set <key> <value>` และ `bwoc manifest get <key>` ถ้าการแก้โดยตรงกลายเป็น friction ในทางปฏิบัติ Framework ไม่เพิ่มคำสั่งเหล่านี้แบบเก็งกำไร (มัตตัญญุตา)

## ทีละขั้น

### 1. รัน `bwoc new`

```bash
bwoc new foo
```

directory ใหม่ (เช่น `agents/agent-foo/`) มี agent ที่กำหนดค่าแล้ว: symlinks เป็นของจริง และ manifest มีค่าที่คุณกรอกหรือยอมรับ

### 2. ตรวจ `config.manifest.json`

```bash
cd agents/agent-foo
$EDITOR config.manifest.json
```

ยืนยัน field ที่ resolve แล้ว อย่างน้อย:

- `agentId` — `agent-<name>` ตรงกับชื่อ directory
- `agentRole` — คำอธิบาย role หนึ่งบรรทัด (เช่น `database schema reviewer`)
- `primaryModel` — model ID ที่ backend ใช้ หรือ `auto` เพื่อเลือกจาก `autoModels` ส่วน `fallbackModel` เป็น metadata เท่านั้น (ไม่ใช่ runtime fallback)
- `memoryPath`, `deepMemoryCmd` — ถ้าใช้ memory Tier 2 (ดู [`memories/README.md`](../../modules/agent-template/memories/README.md))

เอกสาร schema อยู่ที่ [`modules/agent-template/conventions.md`](../../modules/agent-template/conventions.md)

### 3. กรอกส่วน Identity ของ `AGENTS.md`

เปิด `AGENTS.md` แก้ Section 1 (`Identity`):

- `{{agentId}}` → ID ของ agent
- `{{agentRole}}`, `{{primaryCapability}}`, `{{scopeDescription}}`, `{{outOfScope}}` — คำอธิบายที่ชัดเจนว่า agent ทำอะไรและไม่ทำอะไร (อัตตัญญุตา — รู้ตน)

สิ่งเหล่านี้ผูก persona ของ agent ระบุชัด scope ที่คลุมเครือทำให้เกิด capability spoofing (Threat T-1.4)

### 4. กำหนด Persona

แก้ [`persona/README.md`](../../modules/agent-template/persona/README.md) ด้วย:

- Identity (ชื่อ, ID, repo, maintainer)
- Domains (file paths ที่ระบุไว้ว่าจะแตะ)
- Principles (กรอบ BWOC ที่ใช้บ่อยที่สุด)
- ขอบเขตกับ agent อื่น

ตัวอย่าง persona: [`persona-example.good.md`](../../modules/agent-template/docs/persona-example.good.md) (ตัวอย่างที่ดี) · [`persona-example.bad.md`](../../modules/agent-template/docs/persona-example.bad.md) (anti-pattern)

### 5. Verify Backend Neutrality

```bash
bwoc check .
```

ต้อง exit 0 ตรวจ (บางส่วน):

- `AGENTS.md` เป็น plain Markdown (ไม่มี YAML frontmatter, ไม่มี wikilinks)
- Backend symlinks มีอยู่และชี้ไปที่ `AGENTS.md`
- `config.manifest.json` parse เป็น JSON ที่ถูกต้อง
- ไม่มี `{{placeholders}}` ค้างใน `AGENTS.md` (ยกเว้น `{{taskId}}` ที่ใช้ตอน runtime)

ทุก FAIL บรรทัดจะระบุการละเมิด แก้แล้วรันใหม่

### 6. Commit แรก

```bash
git add -A
git commit -m "feat(agent): incarnate agent-foo from BWOC template v2"
```

`bwoc new` ไม่ commit ให้; รัน `git init` ก่อนถ้า directory ของ agent ยังไม่อยู่ใน repository

**เป้าหมาย: ขั้น 1–6 ภายในไม่ถึง 30 นาที**

---

## เพิ่ม Backend

5 backends เริ่มต้น (Claude, Antigravity, Codex, Kimi, Ollama) แนบมาเป็น symlinks การเพิ่มตัวที่หกใช้คำสั่งเดียว:

```bash
ln -s AGENTS.md <BACKEND>.md
```

ไม่ต้องเปลี่ยนอะไรอีก รัน `bwoc check` เพื่อยืนยัน

นี่คือ **สมานัตตตา** — การปฏิบัติเท่าเทียม — บังคับใช้ที่ระดับ filesystem

---

## การตั้งค่าหลายภาษา

Template แนบมากับคู่ `docs/en/` และ `docs/th/` สำหรับทุก `docs/en/*.en.md` มี `docs/th/*.th.md` ที่จับคู่ เมื่อแก้ตัวหนึ่ง ต้องแก้อีกตัว

เพิ่มภาษาที่สาม (เช่น ญี่ปุ่น ISO 639-1 `ja`):

```bash
mkdir docs/ja
# แปลแต่ละ docs/en/<NAME>.en.md ไปเป็น docs/ja/<NAME>.ja.md
```

`<lang>` คือ BCP 47 / ISO 639-1 convention อยู่ใน [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md#โครงสร้างหลายภาษา) ไม่ต้องเปลี่ยน code

---

## Checklist การตรวจสอบ

ก่อนประกาศว่า agent พร้อม:

- [ ] `bwoc check` exit 0
- [ ] `config.manifest.json` ไม่มี `{{placeholders}}` ที่ยังไม่ resolve
- [ ] `AGENTS.md` Section 1 สะท้อน agent นี้ (ไม่ใช่ค่าเริ่มต้นของ template)
- [ ] `persona/README.md` ระบุ domains และขอบเขต
- [ ] `task-log.jsonl` มีอยู่ (ว่างได้ — รายการจะเข้ามาตอน task แรก)
- [ ] ทุกไฟล์ `docs/en/*.en.md` มี `docs/th/*.th.md` ที่จับคู่ (ถ้า agent ของคุณส่งเอกสาร bilingual)
- [ ] Backend CLI ที่เลือกอยู่บน PATH และรู้จัก directory ของ agent

---

## หลัง Incarnation — เส้นทางการอ่าน

สำหรับ session แรกของ operator ของ agent:

1. [`AGENTS.md`](../../modules/agent-template/AGENTS.md) — ชุด instruction เต็มของ agent
2. [`docs/th/OVERVIEW.th.md`](../../modules/agent-template/docs/th/OVERVIEW.th.md) — orientation 5 นาที
3. [`docs/th/PHILOSOPHY.th.md`](../../modules/agent-template/docs/th/PHILOSOPHY.th.md) — กรอบ 22 ประการ (หมวด A–F)
4. [`docs/th/PRD.th.md`](../../modules/agent-template/docs/th/PRD.th.md) และ [`SRS.th.md`](../../modules/agent-template/docs/th/SRS.th.md) — product และ requirements
5. [`docs/th/THREAT-MODEL.th.md`](../../modules/agent-template/docs/th/THREAT-MODEL.th.md) — ตัณหา 3 + ศีล 5

---

## ดูเพิ่ม

- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — ส่วนประกอบทำงานร่วมกันอย่างไรใน runtime
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — ค้นหาคำบาลี → ความหมายเชิงวิศวกรรม
- [`VISION.th.md`](../../VISION.th.md) — เหตุที่ incarnation ถูก model เป็น อุปฺปาท
- [`modules/agent-template/conventions.md`](../../modules/agent-template/conventions.md) — schema ของ placeholder และกฎ YAML
- [`modules/agent-template/neutrality.md`](../../modules/agent-template/neutrality.md) — เหตุที่บังคับใช้ neutrality
