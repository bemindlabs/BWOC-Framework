---
title: สถาปัตยกรรม
parent: ภาษาไทย
nav_order: 1
---

# สถาปัตยกรรม

อธิบายว่า framework, agent template, agent ที่ incarnate แล้ว, CLI, และ runtime ประกอบกันอย่างไร — ในระดับไฟล์, process, และการไหลของข้อมูล

สำหรับ stack **เชิงแนวคิด** (กรอบพุทธ 22 ประการ) ดูที่ [`PHILOSOPHY.th.md`](../../modules/agent-template/docs/th/PHILOSOPHY.th.md) เอกสารนี้พูดถึง **implementation**

---

## Implementation Stack

```
┌──────────────────────────────────────────────────────┐
│  Framework repository (repo นี้)                     │  ← spec + tooling
│  - Markdown specification                            │
│  - Rust workspace (crates/)                          │
│  - Claude Code hooks, skills, memory                 │
└──────────────────────────────────────────────────────┘
                       │ จัดเตรียม
                       ▼
┌──────────────────────────────────────────────────────┐
│  Agent template (modules/agent-template/)            │  ← พิมพ์เขียว
│  - AGENTS.md (แหล่งความจริงเดียว)                    │
│  - Backend symlinks (CLAUDE/AGY/CODEX/KIMI.md)       │
│  - ช่องเสียบ: persona, memories, interconnect, ...   │
│  - bwoc-agent binary (แนบไปกับทุก agent)             │
└──────────────────────────────────────────────────────┘
                       │ clone ด้วย `bwoc new`
                       ▼
┌──────────────────────────────────────────────────────┐
│  Agent ที่ incarnate แล้ว (อยู่ที่ใดบนดิสก์)         │  ← หนึ่ง repo ต่อหนึ่ง agent
│  - หนึ่ง directory ต่อหนึ่ง agent — ไม่มี registry กลาง │
│  - {{placeholders}} ถูก resolve ตอน incarnate        │
└──────────────────────────────────────────────────────┘
                       │ จัดการโดย ↓
┌──────────────────────────────────────────────────────┐
│  bwoc CLI (crates/bwoc-cli/)                         │  ← orchestrator ของวงรอบ
│  - อุปฺปาท:  new, check                              │
│  - ฐิติ:     spawn, list, status, log, send          │
│  - วยะ:      stop, retire                            │
│  - แสดงผลตามภาษา (TH · EN; เพิ่มภาษาด้วยการวาง folder) │
└──────────────────────────────────────────────────────┘
                       │ `bwoc spawn` exec ↓
┌──────────────────────────────────────────────────────┐
│  การ execute backend                                 │  ← LLM runtime
│  - Subprocess: claude · agy · codex · kimi CLI       │
│    หรือ bwoc-harness (ollama / OpenAI-compatible)    │
│  - Backend อ่าน AGENTS.md ผ่าน symlink ของตน         │
│  - bwoc-agent runtime (Phase 2+) สำหรับ control socket │
└──────────────────────────────────────────────────────┘
```

---

## ชั้น (Layers)

### 1. Framework Repository

repo นี้ บรรจุ —

- สเปก Markdown — `AGENTS.md`, การ map กรอบ 22 ประการใน `PHILOSOPHY.th.md`, `PRD`, `SRS`, `THREAT-MODEL` ฯลฯ
- Rust workspace ภายใต้ `crates/` — `bwoc-core`, `bwoc-cli`, `bwoc-agent`
- เครื่องมือ Claude Code ภายใต้ `.claude/` — skills (`/incarnate`, `/check-neutrality`, `/check-bilingual`, `/task-log`), bilingual-reminder hook, project memory

ไม่มี agent ใด *อยู่* ที่นี่ framework เป็นผู้จัดเตรียม recipe

### 2. Agent Template — `modules/agent-template/`

พิมพ์เขียวที่ถูก copy ไปยังทุก agent ใหม่ บรรจุ —

- **`AGENTS.md`** — แหล่งความจริงเดียวที่เป็นกลางต่อ backend
- **Backend symlinks** — `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md`, `OPENAI.md` ล้วนชี้ไปที่ `AGENTS.md`
- **`config.manifest.json`** — schema ของ placeholder (`{{agentId}}`, `{{primaryModel}}` ฯลฯ)
- **ช่องเสียบ (slots)** — `persona/`, `memories/`, `interconnect/`, `mindsets/`, `skills/`
- **`scripts/`** — `incarnate.sh`, `check-agent-neutrality.sh`
- **`bwoc-agent`** binary — แนบไปกับทุก agent ที่ incarnate (Phase 1: stub บอกว่ายังมีชีวิตอยู่)

### 3. Agent ที่ Incarnate แล้ว

สร้างโดย `bwoc new <name>` ซึ่ง copy template ไปยัง directory ใหม่และ resolve placeholder หลัง incarnate —

- agent คือ repo ที่อยู่ได้ด้วยตนเอง สามารถย้าย fork และ version control แยกได้
- **ไม่มี registry กลาง** `bwoc list` ค้นหา agent โดย scan ตาม search path ที่กำหนด (Phase 1: ใช้ convention ของ filesystem; Phase 2 อาจเพิ่ม cache แบบ opt-in)
- `AGENTS.md` ของ agent นั้นเป็นของตัวมันเอง การแตกต่างจาก template เป็นเรื่องคาดหวังและยอมรับได้

### 4. CLI — `crates/bwoc-cli/`

binary ชื่อ `bwoc` ไฟล์เดียวที่ใช้ได้กับ macOS · Linux · Windows

- **Orchestrator บาง** ไม่มี LLM client ฝังในตัว สื่อสารกับ backend CLI ผ่าน subprocess
- **แสดงผลตามภาษา** TH และ EN พร้อมใช้ตั้งแต่เริ่ม เพิ่มภาษาใด ๆ ในอนาคตด้วยการวาง folder ใน `crates/bwoc-cli/locales/`
- **จัดตามวงรอบ** คำสั่งถูกจัดตามสามระยะ (อุปฺปาท · ฐิติ · วยะ)

ดู [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) สำหรับวิธี install และสถานะแต่ละคำสั่ง

### 5. การ Execute Backend

โมเดล Phase 1: **`bwoc spawn` ทำการ exec backend CLI ที่กำหนด** (Claude Code, Antigravity CLI, Codex CLI, Kimi CLI) หรือ **`bwoc-harness`** (`bwoc spawn --backend ollama` — self-hosted Ollama / endpoint ที่ compatible กับ OpenAI) ภายใน directory ของ agent backend อ่าน `AGENTS.md` ผ่าน backend file ของมัน (ซึ่ง symlink ไป) แล้วทำงานตามสเปก ดู [`docs/th/HARNESS.th.md`](HARNESS.th.md) สำหรับสเปกของ harness

Phase 2+ เพิ่ม `bwoc-agent` runtime ทำงานคู่กัน เปิด control socket เพื่อให้ `bwoc status` และ `bwoc send` สื่อสารกับ agent ที่กำลังทำงานอยู่

---

## การไหลของข้อมูล — `bwoc spawn agent-foo`

```
1. User                  bwoc spawn agent-foo
2. CLI                   resolve `agent-foo` เป็น directory บนดิสก์
                         (search path: cwd → ~/bwoc-agents/ → $BWOC_PATH)
3. CLI                   อ่าน agent-foo/config.manifest.json
                         resolve {{primaryBackend}}, {{primaryModel}}
4. CLI                   `cd agent-foo && exec <backend-cli>`
                         (เช่น `exec claude code`)
5. Backend CLI           อ่าน AGENTS.md (ผ่าน entry file ที่ symlink)
                         ใช้ persona, manifest, capabilities
6. Agent                 ทำงานตามวงจรอริยสัจ 4
                         เพิ่มรายการลงใน task-log.jsonl
7. Agent                 เมื่อจบ exit; หรือ
                         (Phase 2) bwoc-agent อยู่ต่อบน socket
```

ไม่มี daemon กลาง ไม่มี shared state ข้าม agent การประสานงาน (Phase 3) ไหลผ่านไฟล์ `interconnect/` และข้อความ `bwoc send` ที่ชัดเจน — ไม่ผ่าน global state

---

## Backend Neutrality

`AGENTS.md` เป็น *ที่เดียว* ที่ instruction อาศัยอยู่ การเพิ่ม backend ใหม่ใช้คำสั่งเดียว —

```bash
ln -s AGENTS.md <BACKEND>.md
```

ไม่ต้องเปลี่ยนอะไรอีก flag `--backend` ของ CLI เลือก backend CLI ที่จะ invoke ตอน spawn สเปกที่ backend อ่านไม่เปลี่ยน

ดู [`modules/agent-template/neutrality.md`](../../modules/agent-template/neutrality.md) สำหรับกฎ validation และ `/check-neutrality` สำหรับ audit ที่รันได้

---

## โครงสร้างหลายภาษา

สามรูปแบบที่ขนานกัน ใช้ key เป็น BCP 47 / ISO 639-1 —

| พื้นผิว | รูปแบบ path | ตัวอย่าง |
|---|---|---|
| เอกสารระดับ framework root | `docs/<lang>/<NAME>.<lang>.md` | `docs/en/GLOSSARY.en.md`, `docs/th/GLOSSARY.th.md` |
| Metadata ระดับ root | `FILENAME.md` (EN canonical) + `FILENAME.<lang>.md` | `VISION.md` + `VISION.th.md` |
| สตริง CLI | `crates/bwoc-cli/locales/<lang>/cli.ftl` | `locales/en/cli.ftl`, `locales/th/cli.ftl` |

ภาษาอังกฤษเป็น canonical ในทั้งสาม การเพิ่มภาษาใหม่คือการวาง folder/file — ไม่ต้องเปลี่ยน code

---

## ขอบเขตความเชื่อใจ (Trust Boundaries)

จุดที่ input ที่ไม่น่าเชื่อใจเข้าระบบ และ threat model ครอบคลุมอะไร —

| ขอบเขต | ประเภทภัย | อ้างอิง |
|---|---|---|
| User → CLI args | Command injection ใน payload ของ `bwoc spawn` | [`THREAT-MODEL.th.md`](../../modules/agent-template/docs/th/THREAT-MODEL.th.md) |
| CLI → Backend subprocess | Untrusted args ส่งต่อไป LLM context | THREAT-MODEL §1 |
| Backend → `AGENTS.md` | Direct prompt injection | THREAT-MODEL §1.1 |
| Backend → อ่านเนื้อหาไฟล์ | Indirect prompt injection | THREAT-MODEL §1.2 |
| Agent → memory files | Social engineering ผ่าน memory ที่ปลูกไว้ | THREAT-MODEL §1.3 |
| Agent ↔ Agent (Phase 3) | Capability spoofing | THREAT-MODEL §1.4 |

การกระทำที่ห้ามพื้นฐาน (ศีล 5) และประเภทภัยตามตัณหา (ตัณหา 3) เป็นฐานทางหลักการ ดู `SECURITY.md` สำหรับขั้นตอนรายงาน

---

## ดูเพิ่ม

- [`PHILOSOPHY.th.md`](../../modules/agent-template/docs/th/PHILOSOPHY.th.md) — stack เชิงแนวคิดและการ map กรอบ 22 ประการ
- [`PHILOSOPHY.th.md §0.1`](../../modules/agent-template/docs/th/PHILOSOPHY.th.md#01-วงรอบ--อุปฺปาท--ฐิติ--วยะ) — วงรอบ (อุปฺปาท · ฐิติ · วยะ) ที่ใช้จัด CLI commands
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — ค้นหาเร็วสำหรับศัพท์บาลีในเอกสารนี้
- [`THREAT-MODEL.th.md`](../../modules/agent-template/docs/th/THREAT-MODEL.th.md) — threat model เต็มรูปแบบ
- `INCARNATION.th.md` (วางแผน) — สร้าง agent ทีละขั้น
- `ROADMAP.th.md` (วางแผน) — ไทม์ไลน์ phase
