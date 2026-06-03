---
title: Workspace
parent: ภาษาไทย
nav_order: 3
---

# Workspace

**Workspace** คือ directory ที่บรรจุ BWOC agent หนึ่งตัวหรือมากกว่า พร้อม metadata ที่ CLI ต้องใช้ทำงานกับ agent เหล่านั้นอย่างประสานกัน CLI รับ workspace path ที่ user ระบุ **คำสั่งเชิงปฏิบัติการจะปฏิเสธการรันจนกว่า workspace จะมีโครงสร้างครบถ้วน** (fail-fast พร้อมข้อความที่ระบุการแก้ไขที่ทำได้)

เอกสารนี้นิยาม concept ของ workspace, โครงสร้างบน disk, กฎ validation, central memory ของ user ที่ `~/.bwoc/`, และวิธีที่ CLI resolve workspace ที่จะใช้

---

## Concept

| ศัพท์ | ความหมาย |
|---|---|
| **Workspace** | directory ที่ user กำหนดให้เป็นบ้านสำหรับงาน BWOC ของตน อาจบรรจุ agent หลายตัว |
| **Workspace marker** | directory `.bwoc/` ที่ root ของ workspace การมีอยู่ + content ที่ valid ทำให้ directory เป็น workspace |
| **Agent** | BWOC agent ที่ incarnate แล้ว — sub-directory ของตนเองที่อยู่ได้ด้วยตนเอง ภายใน (หรือภายนอก) workspace |
| **Central memory** | memory ระดับ user ที่ `~/.bwoc/memory/` แชร์โดยทุก agent ที่ user รันบนเครื่องนี้ |

Workspace ให้ user จัดระเบียบ agent จำนวนมากภายใต้หลังคาเดียว — pin model, share memory, config ระดับเครื่อง — โดยไม่ผูกกับ git repository เดียว

---

## โครงสร้าง Workspace (ที่ต้องมี)

```
<workspace>/
├── .bwoc/                    # workspace marker + metadata   (ต้องมี)
│   ├── workspace.toml        # workspace config              (ต้องมี)
│   ├── agents.toml           # ดัชนี agent ที่ดูแลอัตโนมัติ   (ต้องมี — สร้างโดย CLI)
│   └── memory/               # memory ระดับ workspace         (ทางเลือก)
│       ├── MEMORY.md
│       └── *.md
├── agents/                   # agent ที่ incarnate แล้ว        (ที่อยู่ที่แนะนำ)
│   ├── agent-foo/
│   └── agent-bar/
└── ...                       # ไฟล์อื่น ๆ ของ user (workspace อยู่ร่วมกับอะไรก็ได้)
```

### `.bwoc/workspace.toml` — Field ที่ต้องมี

```toml
[workspace]
name = "my-workspace"            # ต้องมี, slug
version = "0.1.0"                # ต้องมี, SemVer ของ BWOC framework ที่สอดคล้อง
created = "2026-05-22T05:50:00Z" # ต้องมี, ISO 8601 UTC

[defaults]
backend = "claude"               # ทางเลือก: claude | agy | codex | kimi | ollama
lang = "th"                      # ทางเลือก: BCP 47 / ISO 639-1
agents_dir = "agents"            # ทางเลือก, default "agents" (สัมพัทธ์กับ workspace root)
```

### `.bwoc/agents.toml` — ดูแลอัตโนมัติ

```toml
# Update โดย `bwoc new` และ `bwoc retire` การแก้ไขด้วยมือยอมรับได้แต่ไม่แนะนำ

[[agent]]
id = "agent-foo"
path = "agents/agent-foo"
backend = "claude"
incarnated = "2026-05-22T05:51:00Z"
status = "active"

[[agent]]
id = "agent-bar"
path = "agents/agent-bar"
backend = "agy"
incarnated = "2026-05-22T05:52:00Z"
status = "active"
```

---

## กฎ Validation — "ครบถ้วนก่อนทำงาน"

workspace **ครบถ้วน** เมื่อ:

1. directory `.bwoc/` มีอยู่
2. `.bwoc/workspace.toml` มีอยู่, parse เป็น TOML ได้, และมี field `[workspace]` ที่จำเป็น (`name`, `version`, `created`)
3. `.bwoc/agents.toml` มีอยู่และ parse เป็น TOML ได้ (`[[agent]]` array ว่างยอมรับได้ — workspace ใหม่)
4. `agents_dir` ที่ระบุใน `workspace.toml` (หรือ default) มีอยู่ แม้จะว่าง
5. field `version` ใน `workspace.toml` parse เป็น SemVer ได้

**คำสั่งเชิงปฏิบัติการ** (`bwoc spawn`, `bwoc new`, `bwoc check`, `bwoc list`, `bwoc retire`) เรียก validation ก่อนทำงาน เมื่อล้มเหลว exit ด้วย code `2` พร้อมข้อความที่ระบุชื่อส่วนที่ขาดหรือผิดรูปแบบ **ไม่มีงาน agent ใดรันกับ workspace ที่ไม่ครบ**

**คำสั่งตรวจสอบ** (`bwoc workspace info`, `bwoc workspace validate`) รายงานสถานะโดยไม่ทำงาน `bwoc init` สร้างโครงสร้างเมื่อยังไม่มี

---

## CLI Surface

| คำสั่ง | จุดประสงค์ | Phase |
|---|---|---|
| `bwoc init [path]` | สร้างโครงสร้าง workspace ที่ `path` (default: cwd) Idempotent — ปฏิเสธการทับ `workspace.toml` ที่มีอยู่ | Phase 1 v2.0 |
| `bwoc workspace info [path]` | พิมพ์ workspace path ที่ resolve ได้, config, และจำนวน agent | Phase 1 v2.0 |
| `bwoc workspace validate [path]` | รันกฎ validation ทั้งหมด; พิมพ์ผล; exit 0 ถ้าครบ, 2 ถ้าไม่ครบ | Phase 1 v2.0 |
| `bwoc new <name>` | Incarnate agent ใหม่ลงใน workspace (ใช้ `agents_dir`) `--project <path>` derive agent ที่ผูกกับ base project (ตั้ง `worktreeBase` + seed build gates) | Phase 1 v2.0 |
| `bwoc list` | แสดง agent ที่ register ใน workspace (จาก `agents.toml`) | Phase 1 v2.0 |
| `bwoc spawn <name>` | Validate workspace แล้ว exec backend ของ agent | Phase 1 v2.0 |
| `bwoc debase list \| show <agent> \| set <agent> <project>` | จัดการ binding agent → base project ที่ถือไว้โดย `worktreeBase` `list`/`show` read-only (`--json`); `set` เป็น write ที่มี gate ชี้ `worktreeBase` ของ agent ไปที่ `<project>/worktrees` | Phase 3 |
| `bwoc remote link \| list \| status \| unlink` | Link agent เข้ากับ **remote-control session** (เช่น Claude Code Remote Control) และจัดการ link เหล่านั้น เป็น bookkeeping ฝั่ง bwoc แบบ backend-neutral บันทึกที่ `.bwoc/remote/<agentId>.json`; `link` เขียน, `list`/`status` อ่าน (`--json`), `unlink` เป็น remove ที่มี gate | Phase 3 |

### binding แบบ debase

**base project** ของ agent — project ที่มัน derive มาและ build — ถูกบันทึกเชิงฟังก์ชันด้วย `worktreeBase` ใน manifest: agent ที่ผูกกับ project `P` ถือ `worktreeBase = <P>/worktrees` และ framework วาง task worktree ที่ `<worktreeBase>/<agentId>/<taskId>` `bwoc debase` ทำความสัมพันธ์นี้ให้เป็น surface ที่ตรวจดูได้แบบ first-class และ `bwoc new --project <path>` สร้าง binding นี้ตอน incarnate (พร้อม seed gate build/test/lint/format จาก stack ที่ detect ได้ของ project) agent ที่ไม่มี `worktreeBase` คือ **unbound** — มันทำงาน *กับ* target แทนที่จะ build project (เช่น agent monitoring / audit)

### link ของ remote-control session

**remote-control session** ทำให้ session แบบ interactive ของ agent ถูกควบคุมจากที่อื่นได้ — เช่น Claude Code Remote Control ที่เข้าถึงจาก claude.ai หรือมือถือ `bwoc remote` เป็น **bookkeeping ฝั่ง bwoc แบบ backend-neutral** บนความสัมพันธ์นั้น: บันทึกต่อ agent ว่า link กับ control session ภายนอกตัวไหน (ไม่ได้เปิดหรือ proxy session เอง) แต่ละ link เก็บที่ `.bwoc/remote/<agentId>.json` — `{ agentId, backend, kind, sessionRef, url?, linkedAt, note? }` — ตามแบบ convention ของ marker `.bwoc/sessions/` ฟิลด์ `kind` ระบุกลไก: `claude-remote-control` เป็นตัวแรก และ backend ใดก็ประกาศ kind ของตัวเองได้ (Samānattatā — Claude เป็น implementation แรก ไม่ใช่กรณีพิเศษ) `link` default `backend` จาก workspace registry (`agents.toml` ซึ่งบันทึกไว้เสมอ) และ `kind` เป็น `claude-remote-control`

### การ Resolve Workspace

ลำดับความสำคัญ — ตัวแรกที่ตรงชนะ:

1. flag `--workspace <path>` global
2. environment variable `BWOC_WORKSPACE`
3. directory บรรพบุรุษที่ใกล้สุดของ `cwd` ที่มี `.bwoc/` (เดินขึ้นไป)
4. cwd ถ้าตัวมันมี `.bwoc/`
5. **ไม่มี workspace** → คำสั่งเชิงปฏิบัติการล้มเหลวด้วย code `2` พร้อมคำแนะนำให้รัน `bwoc init`

---

## Central Memory — `~/.bwoc/`

ไม่ขึ้นกับ workspace ใด CLI ดูแล directory **ระดับ user** ที่ `~/.bwoc/` นี่คือ memory ระดับ user ที่แชร์โดยทุก BWOC agent ที่ user รันบนเครื่องนี้

```
~/.bwoc/
├── config.toml               # config ระดับ user (default lang, default backend ฯลฯ)
├── memory/                   # central memory (รูปแบบ Tier 1)
│   ├── MEMORY.md             # ดัชนี (≤ 200 บรรทัด — มัตตัญญุตา)
│   └── *.md                  # memory แบบมีประเภท (user, feedback, project, reference)
├── workspaces.toml           # registry ของ workspace ที่รู้จัก (ดูแลอัตโนมัติ)
└── logs/                     # log การเรียกใช้ CLI (rotated)
```

### `~/.bwoc/config.toml` — Default ระดับ User

```toml
[defaults]
backend = "claude"
lang = "th"
workspace = "/Users/lps/bwoc"    # ทางเลือก: workspace path default

[memory]
cap_lines = 200                  # MEMORY.md index cap
```

### `~/.bwoc/memory/` — รูปแบบ Memory

รูปแบบ two-tier เหมือน memory ของ agent (ดู [`modules/agent-template/memories/README.md`](../../modules/agent-template/memories/README.md)) ดัชนี `MEMORY.md` capped ที่ 200 บรรทัด (มัตตัญญุตา) ไฟล์ memory แต่ละไฟล์ใช้ 4 ประเภท: `user`, `feedback`, `project`, `reference`

Agent เข้าถึง central memory ผ่าน `deepMemoryCmd` ของตน หรือใน Phase 2+ ผ่าน `bwoc-agent` runtime ที่ expose API memory แบบรวมครอบคลุมทั้ง 3 scope

### Scope ของ Memory (อธิบาย)

| Scope | Path | มองเห็นได้โดย |
|---|---|---|
| **Per-agent** | `<agent>/memories/` | agent เดียว |
| **Per-workspace** | `<workspace>/.bwoc/memory/` | agent ทุกตัวใน workspace นี้ (ทางเลือก) |
| **Per-user (central)** | `~/.bwoc/memory/` | agent ทุกตัวที่ user นี้รันบนเครื่องนี้ |
| **Tier 2 (deep)** | pluggable backend | ทุก scope (vector DB, semantic search ฯลฯ) |

Scope ที่สูงกว่า **อ่านได้แบบแชร์** โดย default; **การ write** ต้องมีเจตนาที่ชัดเจน เพื่อไม่ให้ agent เปลี่ยน context นอก scope ของตนโดยไม่ตั้งใจ

---

## Lifecycle — Workspace กับวงรอบ

Workspace มีส่วนร่วมในทุกระยะของวงรอบ BWOC:

| ระยะ | การกระทำ |
|---|---|
| **อุปฺปาท** | `bwoc init` สร้าง workspace; `bwoc new` เพิ่ม agent ลง workspace (register ใน `agents.toml`) |
| **ฐิติ** | `bwoc spawn` validate workspace ก่อน แล้ว exec backend ของ agent ใน directory ของ agent workspace อยู่ยาวนาน — คงอยู่ผ่านการ operate agent หลายครั้ง |
| **วยะ** | `bwoc retire <agent>` ถอด agent จาก `agents.toml`, archive directory ถ้าต้องการ; workspace ยังอยู่ `bwoc workspace prune` (Phase 3) จะเก็บกวาด entry ที่ลอย |

---

## ดูเพิ่ม

- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — CLI, workspace, agent, runtime ทำงานร่วมกันอย่างไรใน runtime
- [`INCARNATION.th.md`](INCARNATION.th.md) — สร้าง agent ทีละขั้น (Phase 1 ยังใช้ `incarnate.sh` ของ template; Phase 2+ หุ้มด้วย `bwoc new` ที่ write `agents.toml`)
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — ค้นหาคำบาลี
- [`modules/agent-template/memories/README.md`](../../modules/agent-template/memories/README.md) — spec รูปแบบ memory (ใช้กับ per-agent, per-workspace, per-user memory)
- [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) — วิธี install CLI และสถานะคำสั่งปัจจุบัน
