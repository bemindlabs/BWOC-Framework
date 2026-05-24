---
title: การตั้งชื่อ
parent: ภาษาไทย
nav_order: 4
---

# มาตรฐานการตั้งชื่อไฟล์ Markdown

convention เดียวที่สอดคล้องสำหรับทุกไฟล์ `*.md` ใน BWOC framework, agent template, และ agent ที่ incarnate แล้ว กฎด้านล่างกำหนดว่าไฟล์ใหม่แต่ละไฟล์จะถูกตั้งชื่ออะไรและอยู่ที่ไหน

---

## หมวด

| # | หมวด | ที่อยู่ | รูปแบบ | ตัวอย่าง |
|---|---|---|---|---|
| 1 | Metadata ระดับ project | repo root | `UPPERCASE.md` | `README.md` · `LICENSE` · `CHANGELOG.md` · `VERSION.md` · `VISION.md` · `SECURITY.md` · `CODE_OF_CONDUCT.md` · `CONTRIBUTING.md` |
| 2 | คำแปลระดับ project | repo root | `UPPERCASE.<lang>.md` | `VISION.th.md` |
| 3 | เอกสารสเปก | `docs/<lang>/` | `UPPERCASE.<lang>.md` | `PHILOSOPHY.en.md` · `GLOSSARY.th.md` · `ARCHITECTURE.en.md` · `INCARNATION.th.md` · `WORKSPACE.en.md` |
| 4 | prose ของ template / module | `modules/<x>/` | `lowercase-hyphen.md` | `conventions.md` · `neutrality.md` · `trust-model.md` |
| 5 | Landing ของ slot (Obsidian) | `modules/<x>/<slot>/` | `README.md` | `memories/README.md` · `persona/README.md` |
| 6 | เอกสารของ crate | `crates/<crate>/` | `README.md` | `crates/bwoc-cli/README.md` |
| 7 | นิยาม skill | `.claude/skills/<name>/` | `SKILL.md` | `.claude/skills/incarnate/SKILL.md` |
| 8 | ดัชนี memory | `<memory-scope>/` | `MEMORY.md` | `~/.bwoc/memory/MEMORY.md` · `<agent>/memories/MEMORY.md` |
| 9 | รายการ memory | `<memory-scope>/` | `<type>_<slug>.md` | `feedback_policy_docs.md` · `user_role.md` |
| 10 | บันทึก (note) | `notes/` (scope ใดก็ได้) | `YYYY-MM-DD_<title>.md` | `notes/2026-05-22_workspace-design.md` |
| 10a | Retrospective | `retrospectives/` (scope ใดก็ได้) | `YYYY-MM-DD_<title>.md` | `retrospectives/2026-05-24_sprint-1.md` |
| 10b | เอกสารวิจัย (research) | `research/` (scope ใดก็ได้) | `YYYY-MM-DD_<title>.md` | `research/2026-05-24_llm-caching.md` |
| 10c | Doc kind กำหนดเอง | `dir` ที่ประกาศใน `.bwoc/doc-kinds.toml` | `YYYY-MM-DD_<title>.md` | `decisions/2026-05-24_use-postgres.md` |
| 11 | คำสั่ง Claude Code | repo root | `CLAUDE.md`, `CLAUDE.local.md` | `CLAUDE.md` · `CLAUDE.local.md` |
| 12 | คำสั่ง agent (backend-neutral) | repo root ของ agent | `AGENTS.md` + symlinks | `AGENTS.md` · `CLAUDE.md → AGENTS.md` |

---

## นิยามกฎ

### `UPPERCASE.md` — ไฟล์มาตรฐาน open-source

ใช้สำหรับ: metadata ระดับ project ที่ GitHub และชุมชน OSS ถือเป็น artifact ที่รู้จัก (`README`, `LICENSE`, `CHANGELOG`, `CONTRIBUTING`, `CODE_OF_CONDUCT`, `SECURITY`) บวกกับเอกสาร canonical ระดับ project ของ BWOC (`VISION`, `VERSION`)

เหตุผล: GitHub render ใน UI; ชุมชนคาดหวัง; filesystem ที่ case-insensitive บน macOS ปฏิบัติได้สม่ำเสมอ

### `UPPERCASE.<lang>.md` — สเปก + คำแปล bilingual

สองการใช้ รูปแบบเดียวกัน:

- ภายใน `docs/<lang>/` สำหรับเอกสารสเปกทุกฉบับ (`PHILOSOPHY.en.md`, `GLOSSARY.th.md`)
- ที่ repo root สำหรับคำแปลของเอกสารระดับ project (`VISION.th.md`)

`<lang>` คือรหัส BCP 47 / ISO 639-1 ตัวพิมพ์เล็ก (`en`, `th`, `ja`, `zh` ...) ภาษาอังกฤษเป็น canonical

### `lowercase-hyphen.md` — prose ของ module / template

ใช้สำหรับ: prose ภายใน `modules/agent-template/` และพื้นที่คล้ายกันที่ไฟล์เป็น implementation detail มากกว่า artifact มาตรฐานชุมชน ใช้ hyphen เป็นตัวคั่นคำ; ไม่มีช่องว่าง, ไม่มี underscore ในหมวดนี้

### `README.md` — Landing ของ subdirectory

ใช้ภายใน slot (`memories/`, `persona/`, `interconnect/`, `mindsets/`, `skills/`) และภายในแต่ละ Rust crate (`crates/<x>/README.md`)

Slot README **format Obsidian** (YAML frontmatter + callouts) — เป็นไฟล์ spec ของ slot ไม่ใช่ OSS landing page

Crate README **plain Markdown** — convention ของ Rust แสดงบน crates.io

### `SKILL.md` — Skill ของ Claude Code

ชื่อตายตัวตาม convention ของ Claude Code อยู่ภายใน `.claude/skills/<skill-name>/SKILL.md`

### `MEMORY.md` — ดัชนี memory

ชื่อตายตัว อยู่ภายใน memory scope ใด ๆ (`<agent>/memories/`, `<workspace>/.bwoc/memory/`, `~/.bwoc/memory/`) capped ที่ 200 บรรทัด (มัตตัญญุตา)

### `<type>_<slug>.md` — รายการ memory

`<type>` คือหนึ่งใน `user`, `feedback`, `project`, `reference` `<slug>` คือ kebab-case แต่ใช้ underscore คั่นระหว่าง type และ slug เพื่อความอ่านง่าย (`feedback_policy_docs.md` ไม่ใช่ `feedback-policy-docs.md` หรือ `feedback.policy-docs.md`)

### `YYYY-MM-DD_<title>.md` — Note (ใหม่)

สำหรับ note ของ session, note การออกแบบ, decision record, และอะไรก็ตามที่เรียงตามเวลา ไม่มี identity เสถียรนอกเหนือจากวันที่

- `YYYY-MM-DD` คือ ISO 8601 (เดือน/วัน เติม zero)
- Underscore เดียวคั่น date และ title
- `<title>` ตัวพิมพ์เล็ก คั่นด้วย hyphen บรรยายชัด
- เรียงตามเวลาเมื่อ list

ตัวอย่าง:

```
notes/2026-05-22_workspace-design.md
notes/2026-05-22_naming-standard-rollout.md
~/.bwoc/notes/2026-05-23_user-config-cleanup.md
```

#### ที่อยู่ของ Note

Note อยู่ได้ที่:

| Scope | Path | เมื่อใด |
|---|---|---|
| ระดับ project | `<repo>/notes/` | การตัดสินใจเกี่ยวกับ framework หรือ repo นี้ |
| Workspace | `<workspace>/.bwoc/notes/` | การตัดสินใจที่อยู่ใน scope ของ workspace (Phase 2+) |
| Per-user | `~/.bwoc/notes/` | note session ส่วนบุคคลที่ข้าม workspace |

### `YYYY-MM-DD_<title>.md` — Retrospective

สำหรับรีวิวแบบ Paññā-3 ของ sprint, session หรือ milestone รูปแบบ date-slug เดียวกับ note

หัวข้อ: **Sutamayā** (สิ่งที่ข้อมูล/เอกสารบอก), **Cintāmayā** (การสังเคราะห์/รูปแบบที่เกิด), **Bhāvanāmayā** (การกระทำที่ทำ), **Metrics**

`bwoc retro new "<title>"` สร้างไฟล์ใน `retrospectives/` ด้วย template ที่ built-in

### `YYYY-MM-DD_<title>.md` — Research

สำหรับการสำรวจเชิงวิจัย: คำถาม, ขอบเขต, แหล่งข้อมูล, ผลการค้นพบ และคำแนะนำ commit ลง repo เหมือน note และ retrospective

`bwoc research new "<title>"` สร้างไฟล์ใน `research/` ด้วย template ที่ built-in

### `YYYY-MM-DD_<title>.md` — Doc kind กำหนดเอง (ใหม่)

Workspace สามารถประกาศ document kind เพิ่มเติมใน `.bwoc/doc-kinds.toml`:

```toml
[[kind]]
name = "decision"
dir = "decisions"
committed = true
template = "# YYYY-MM-DD — <Decision>\n\n## Context\n\n## Decision\n\n## Consequences\n"
# หรือ: template_file = ".bwoc/templates/decision.md"
```

ฟิลด์: `name` (ชื่อ kind ใน CLI), `dir` (directory สัมพัทธ์กับ workspace), `committed` (default `true`), และ `template` (inline string) หรือ `template_file` (path สัมพัทธ์กับ workspace root) อย่างใดอย่างหนึ่ง

Custom kind ใช้รูปแบบ `YYYY-MM-DD_<title>.md` เดียวกันใน directory ที่ประกาศไว้

ลำดับการ resolve: built-in kind ชนะก่อน แล้วตาม `.bwoc/doc-kinds.toml` kind ที่ไม่รู้จัก → error แสดง kind ทั้งหมดที่มี

**CLI:** `bwoc doc <kind> new "<title>"` — resolve kind ใดก็ได้ (built-in หรือ custom) ผ่าน engine เดียวกัน

```
bwoc doc decision new "use postgres"   # custom kind
bwoc doc notes new "my note"           # built-in ผ่าน generic command
bwoc notes new "my note"               # built-in alias (ไม่เปลี่ยน)
```

alias ที่มีชื่อ (`bwoc notes`, `bwoc retro`, `bwoc research`) ยังคงเป็น thin wrapper และไม่ถูกยกเลิก

---

## ต้องห้าม / สงวน

- Case ผสมที่ไม่ใช่รูปแบบที่ documented (เช่น `MyFile.md`, `getting_started.md`)
- ช่องว่างใน filename
- คำแปลของ `README.md` ที่ repo root (ใช้ `docs/<lang>/<NAME>.<lang>.md` แทน)
- `README.<lang>.md` ที่ใด ๆ — คำแปลอยู่ใน `docs/<lang>/` ด้วยชื่อของตน
- Date stamp ภายใน body ของไฟล์เพื่อจุดประสงค์การตั้งชื่อ — filename นำ date สำหรับ note

---

## Decision Tree เร็ว

```
เป็นไฟล์ระดับ project มาตรฐาน GitHub/OSS (README, LICENSE, CHANGELOG, ...)?
├── ใช่ → UPPERCASE.md ที่ repo root  (คำแปล: UPPERCASE.<lang>.md)
└── ไม่
    ├── เป็นเอกสารสเปก?
    │   └── ใช่ → UPPERCASE.<lang>.md ใน docs/<lang>/
    ├── เป็น Rust crate README?
    │   └── ใช่ → README.md ใน crates/<crate>/
    ├── เป็น Claude Code skill?
    │   └── ใช่ → SKILL.md ใน .claude/skills/<name>/
    ├── เป็นรายการ memory?
    │   ├── ดัชนี → MEMORY.md
    │   └── รายการ → <type>_<slug>.md
    ├── เป็น note หรือ decision record ที่เรียงตามเวลา?
    │   └── ใช่ → YYYY-MM-DD_<title>.md ใน notes/
    └── อื่น ๆ (prose ของ module, slot landing)
        ├── slot landing → README.md (Obsidian format)
        └── prose       → lowercase-hyphen.md
```

---

## Audit

Skill `/check-naming` และ workflow `.github/workflows/docs.yml` รันการตรวจ 3 อย่างเดียวกัน Audit ด้วยมือได้ด้วย:

```bash
# A) Root-level: UPPERCASE.md, UPPERCASE.<lang>.md, หรือ CLAUDE.local.md
find . -maxdepth 1 -name '*.md' \
  | grep -vE '^\./(README|LICENSE|CHANGELOG|CONTRIBUTING|CODE_OF_CONDUCT|SECURITY|VISION|VERSION|CLAUDE|AGENTS)(\.local|\.[a-z]{2,3})?\.md$'

# B) ไฟล์ใน docs/<lang>/: UPPERCASE.<lang>.md (mindepth 2 ข้าม root ของ docs/;
#    Slot README เช่น memories/README.md ยกเว้นตาม category 5)
find docs modules/agent-template/docs -mindepth 2 -type f -name '*.md' \
  | grep -vE '/[A-Z]+(-[A-Z]+)*\.(en|th|[a-z]{2,3})\.md$' \
  | grep -v '/README'

# C) Note: YYYY-MM-DD_<title>.md
find . -path '*/notes/*.md' \
  | grep -vE '/[0-9]{4}-[0-9]{2}-[0-9]{2}_[a-z0-9-]+\.md$'
```

output ใด ๆ จากการตรวจใดก็เป็นการละเมิด CI exit non-zero พร้อม annotation `::error::`

---

## ดูเพิ่ม

- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — โครงสร้างหลายภาษาข้าม docs, metadata ระดับ root, และ CLI locales
- [`WORKSPACE.th.md`](WORKSPACE.th.md) — ที่อยู่ของ note และ memory ระดับ workspace
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — ค้นหาคำบาลี
- [`modules/agent-template/conventions.md`](../../modules/agent-template/conventions.md) — เอกสาร convention เดิม จะถูก update ให้ชี้มาที่นี่
