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

- Shell (bash, zsh, หรือ PowerShell + Git Bash บน Windows)
- `git` บน PATH
- `rsync`, `ln`, `python3` บน PATH (ใช้โดย `incarnate.sh` และ `check-agent-neutrality.sh`)
- (เสริม) Backend CLI ที่เลือก — `claude`, `agy`, `codex`, `kimi`, หรือ `ollama` (ผ่าน `bwoc-harness`) — ติดตั้งที่จุดที่จะใช้งาน agent

`bwoc` Rust CLI กำลังอยู่ใน Phase 1 v2.0 เส้นทาง canonical วันนี้ใช้ shell scripts ที่แนบมากับ template เมื่อ `bwoc new` port logic ของ script เสร็จ คำสั่งจะเหลือเพียง invocation เดียว

---

## เส้นทาง Canonical (วันนี้)

จาก framework root:

```bash
cd modules/agent-template
./scripts/incarnate.sh <agent-name> [target-path]
```

ค่าเริ่มต้น:

- **`<agent-name>`** — ตัวพิมพ์เล็ก คั่นด้วย hyphen (เช่น `agent-database-schema`)
- **`[target-path]`** — ทางเลือก ค่าเริ่มต้น: `../agent-<agent-name>/` ที่สัมพัทธ์กับ template

Script จะ copy template, สร้าง backend symlinks (`CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md` → `AGENTS.md`), init git, commit แรก, แล้วรัน neutrality check Output จะระบุทุกขั้นและจบด้วย block "Next steps"

---

## การตั้ง Manifest — ปัจจุบัน vs แผน

**วันนี้ (ผ่าน `incarnate.sh`):** script copy template แล้วหยุด คุณแก้ `config.manifest.json` เองเพื่อ resolve placeholder

**กับ `bwoc new` (Phase 1 v2.0 กำลังดำเนินการ):** CLI รับ field ของ manifest เป็น input, validate, แล้วเขียน manifest ที่ resolve แล้วแบบ atomic โหมด input สองแบบ:

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

### 1. รัน `incarnate.sh`

```bash
./scripts/incarnate.sh agent-foo
```

ผลลัพธ์:

```
+ CLAUDE.md -> AGENTS.md
+ AGY.md    -> AGENTS.md
+ CODEX.md  -> AGENTS.md
+ KIMI.md   -> AGENTS.md
+ OLLAMA.md -> AGENTS.md
+ git initialized
...
Done in 3s
```

directory ใหม่ `../agent-foo/` มี agent ที่ใช้ได้แต่ยังไม่ได้กำหนดค่า Symlinks เป็นของจริง manifest ยังมี `{{placeholders}}`

### 2. แก้ `config.manifest.json`

```bash
cd ../agent-foo
$EDITOR config.manifest.json
```

Resolve ทุก placeholder ที่จำเป็น อย่างน้อย:

- `agentId` — ตรงกับชื่อ directory ไม่มี prefix `agent-`
- `agentRole` — คำอธิบาย role หนึ่งบรรทัด (เช่น `database schema reviewer`)
- `primaryModel` / `fallbackModel` — key ของ model selector ที่เป็นกลางต่อ backend (backend CLI จะ resolve เป็นชื่อ native ของตน)
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

ตัวอย่าง persona ที่ดี: ดู `modules/agent-template/docs/README.md` (ปัจจุบันชื่อผิด — จะเปลี่ยนเป็น `examples/persona-good.md`)

### 5. Verify Backend Neutrality

```bash
./scripts/check-agent-neutrality.sh
```

ต้อง exit 0 Script ตรวจ:

- `AGENTS.md` เป็น plain Markdown (ไม่มี YAML frontmatter, ไม่มี wikilinks)
- Backend symlinks มีอยู่และชี้ไปที่ `AGENTS.md`
- `config.manifest.json` parse เป็น JSON ที่ถูกต้อง
- ไม่มี model IDs ที่ hardcode หรือคำพูดเฉพาะ vendor ใน `AGENTS.md`

ทุก FAIL บรรทัดจะระบุการละเมิด แก้แล้วรันใหม่

### 6. Commit แรก

```bash
git add -A
git commit -m "feat(agent): incarnate agent-foo from BWOC template v2"
```

`incarnate.sh` ได้สร้าง commit scaffold แรกไปแล้ว นี่คือ commit แรกที่ **กำหนดค่าแล้ว** ของคุณ

**เป้าหมาย: ขั้น 1–6 ภายในไม่ถึง 30 นาที**

---

## เพิ่ม Backend

5 backends เริ่มต้น (Claude, Antigravity, Codex, Kimi, Ollama) แนบมาเป็น symlinks การเพิ่มตัวที่หกใช้คำสั่งเดียว:

```bash
ln -s AGENTS.md <BACKEND>.md
```

ไม่ต้องเปลี่ยนอะไรอีก รัน `check-agent-neutrality.sh` เพื่อยืนยัน

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

- [ ] `./scripts/check-agent-neutrality.sh` exit 0
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
