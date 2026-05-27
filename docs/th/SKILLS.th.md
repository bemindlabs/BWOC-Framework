---
title: Skills
parent: ไทย
nav_order: 11
---

# Framework Skills (ทักษะระดับเฟรมเวิร์ก)

**Framework skill** คือความสามารถที่เฟรมเวิร์กแนะนำให้เป็น baseline ที่ agent สามารถเลือก opt-in ได้ เป็น "standard library" ของพฤติกรรม agent — มี contract ที่ชัดเจน, ทดสอบได้, เป็นกลางต่อทุก backend, ค้นพบได้ผ่าน manifest format เดียวกัน

เอกสารนี้กำหนดรูปแบบ manifest, สัญญาการ invoke, กลไกการค้นพบ, และ verification gates Reference skill ตัวแรก (`worktree-discipline`) จะลงพร้อมกับ spec นี้ — ทั้ง spec และ implementation พิสูจน์รูปแบบไปด้วยกัน

> [!abstract] สถานะ: scaffold เริ่มต้น ตาราง manifest และ lifecycle hook ด้านล่างเป็น normative; ส่วน prose อาจปรับเมื่องาน story BWOC-1..3 ทำให้ contract ละเอียดขึ้น Reference skill ตัวแรกจะมาใน BWOC-6

---

## คำว่า "Skill" ใช้ 3 ที่ ไม่ซ้อนทับกัน

| ชั้น | Path | ผู้ใช้ | ผู้เรียก |
|---|---|---|---|
| **Framework skill** (spec นี้) | `modules/skills/<name>/` | ผู้สร้าง agent | Agent เองในระหว่างทำงาน |
| **Agent skill** ([SPEC](../../modules/agent-template/skills/SPEC.md)) | `<agent>/skills/<name>.md` | Agent ตัวเดียว | Logic ของ agent ตัวนั้น |
| **Claude Code skill** | `.claude/skills/<name>/SKILL.md` | Claude Code session ของ repo นี้ | คำสั่ง `/<name>` |

Framework skill = *baseline ที่แนะนำ*. Agent skill = *capability ที่ประกาศ*. Claude Code skill = *tool invocation*. การเลือกชั้นที่ถูกต้องคือคำตัดสินใจแรกเมื่อเพิ่มอะไรก็ตามที่เรียกตัวเองว่า "skill"

---

## โครงสร้างไดเรกทอรี

```
modules/skills/
└── <name>/
    ├── manifest.toml       # บังคับ — contract
    ├── SPEC.md             # บังคับ — รายละเอียดสกิลในรูปแบบ Obsidian
    └── ...                 # implementation (Rust crate, shell script, ฯลฯ) — ไม่บังคับ
```

`<name>` เป็น `kebab-case` หนึ่ง skill ต่อหนึ่งไดเรกทอรี

---

## Manifest — `manifest.toml`

```toml
[skill]
name        = "worktree-discipline"             # บังคับ — ต้องตรงกับชื่อไดเรกทอรี
version     = "0.1.0"                           # บังคับ — semver
description = "Create, isolate, cleanup worktrees per Anatta."   # บังคับ — สรุปหนึ่งประโยค
maturity    = "L1"                              # บังคับ — ดู "Maturity"

[contract]
requires         = []                              # ไม่บังคับ (default []) — framework skill อื่นที่ skill นี้พึ่งพา
requires_plugins = []                              # ไม่บังคับ (default []) — KIND ของ plugin ที่ skill นี้ต้องการให้ enable
exposes          = ["claim_task", "release_task"]  # บังคับ — operations ที่ skill เปิดให้ผู้เรียก
                                                   #   (ต้องเป็น array ที่ไม่ว่าง; ถ้าว่างคือ skill ไม่เปิดอะไรเลย
                                                   #    ก็ไม่ควรมีอยู่ — ดู "อ้างอิงฟิลด์")

[gates]
verify      = "bwoc skill verify worktree-discipline"   # ไม่บังคับ — shell command; exit 0 ถ้า skill ทำงานได้
```

### อ้างอิงฟิลด์

| Section | Field | บังคับ | ชนิดข้อมูล | ความหมาย |
|---|---|---|---|---|
| `[skill]` | `name` | ใช่ | string (kebab-case) | ชื่อ skill; ต้องตรงกับชื่อไดเรกทอรีใต้ `modules/skills/` |
| `[skill]` | `version` | ใช่ | string (semver) | Semver ของ skill เอง แยกจากเวอร์ชันเฟรมเวิร์ก |
| `[skill]` | `description` | ใช่ | string | สรุปหนึ่งประโยค; ใช้แสดงใน `bwoc skill list` |
| `[skill]` | `maturity` | ใช่ | enum `L1`..`L7` | ระดับ maturity ปัจจุบัน (ดู [Maturity](#maturity-levels)); ประกาศตามจริง — `bwoc check` บังคับ |
| `[contract]` | `requires` | ไม่ (default `[]`) | array of strings | ชื่อ framework **skill** ที่ติดตั้งแล้วและ skill นี้พึ่งพา; resolve ตอน spawn agent |
| `[contract]` | `requires_plugins` | ไม่ (default `[]`) | array of strings | **kind** ของ plugin ที่ skill นี้ต้องการให้ enable ใน workspace; resolve ตอน spawn agent (ดู [Skill-on-plugin dependency](#skill-on-plugin-dependency)) |
| `[contract]` | `exposes` | ใช่ (ไม่ว่าง) | array of strings | operations ที่ skill เปิดให้ผู้เรียก; array ว่างจะไม่ผ่าน `bwoc check` |
| `[gates]` | `verify` | ไม่ | string (shell command) | คำสั่งที่ `bwoc skill verify <name>` รัน; exit 0 ถ้า skill ทำงานได้ในสภาพแวดล้อมนี้ |

### ข้อจำกัดเรื่องความเป็นกลาง (HARD)

ค่าใน manifest **ต้องไม่** ระบุ vendor, model, หรือ backend CLI เฉพาะ Skill ที่ทำงานเฉพาะกับ backend ใด backend หนึ่งควรอยู่เป็น integration plugin ของ backend นั้น ไม่ใช่ framework skill กฎ **Samānattatā** เดียวกันกับที่ `bwoc check` บังคับใช้กับ `AGENTS.md` อยู่แล้ว

---

## สัญญาการ Invoke

Skill เปิด **operations** ที่มีชื่อ (ประกาศใน `[contract] exposes`) เมื่อ agent opt-in skill ตัวหนึ่ง operations เหล่านั้นจะเข้าถึงได้จาก logic ของ agent — *วิธี* route call เป็นเรื่องของ agent *สิ่งที่เรียกได้* เป็น contract ของ skill

### Lifecycle

```
init  → invoke (หนึ่งครั้งหรือมากกว่า) → teardown
```

- **`init`** — เรียกครั้งเดียวเมื่อ skill ถูกโหลดเข้า runtime ของ agent **Idempotent** อ่าน config ฝั่ง agent ที่ skill ต้องการ
- **`invoke`** — เรียกต่อ operation **Idempotent ที่ระดับ operation**: เรียก `claim_task("t-1")` สองครั้งสำหรับ task เดียวกันต้องไม่ทำให้ claim ซ้ำ
- **`teardown`** — เรียกครั้งเดียวเมื่อ agent retire หรือ release skill **Idempotent** เป็นการ cleanup เท่านั้น; ต้องไม่ block บนสถานะภายนอก

Idempotency เป็น **ข้อกำหนดบังคับทุกเฟส** — agent อาจ retry, restart, หรือ replay Skill ที่พังบน replay จะทำลายเรื่อง recovery ของ agent

### สัญญา Hook — success, failure, partial state

Skill ถูก *invoke* ไม่ใช่ *import* Runtime ของ agent resolve ชื่อ skill ไปยัง manifest ที่ติดตั้ง รัน `init` ครั้งเดียว แล้ว dispatch operations ไม่มี global registry; การ resolve เป็นแบบ per-workspace (ดู [Discovery](#discovery))

Skill เป็น abstraction แบบ in-process ดังนั้น contract แสดงผ่าน return / throw semantics (ไม่ใช้ exit code — exit code เป็นเรื่องของ plugin ดู [`PLUGINS.th.md`](PLUGINS.th.md#สัญญา-hook--success-failure-partial-state))

| Hook | Success คือ | Failure คือ | Partial state |
|---|---|---|---|
| `init` | Return; agent spawn ดำเนินต่อ | Throw typed error ระบุชื่อ skill; agent spawn ถูกปฏิเสธ | init ต้องเสร็จเต็มหรือ roll back ก่อน throw — caller ถือว่า init ที่ล้มเหลวเหมือนไม่เคยรัน |
| `invoke` | Return ผลลัพธ์ของ operation | Throw typed error ระบุชื่อ operation; caller ตัดสินใจว่าจะ retry หรือไม่ | ทุก operation ต้อง durable-or-discarded — ไม่ apply ครึ่ง ๆ การ retry ตกบน path ที่ idempotent |
| `teardown` | Return; slot ถูก release | Log แล้วทำต่อ — การ retire ของ agent ต้องไม่ block ที่ teardown | Idempotent บน replay agent อาจ crash กลาง retire; session ถัดไปเรียก teardown ซ้ำ |

### ตัวอย่างต่อเฟส

```text
# init — โหลด config ฝั่ง agent ครั้งเดียวตอน spawn
init():
  cfg = read("<agent>/config.manifest.json")
  cache cfg.worktreeBase           # ใช้ทุกครั้งใน invoke()
  # ไม่มี remote call — Anattā

# invoke — idempotent ที่ระดับ operation
claim_task("t-1"):
  if worktree_exists("t-1"):
    return existing_path           # replay-safe
  path = create_worktree("t-1")
  register(path)
  return path

# teardown — cleanup เท่านั้น, idempotent
teardown():
  drop_in_memory_caches()
  # ไม่ prune worktree — เป็นหน้าที่ของ release_task
  # ปลอดภัยที่จะเรียกซ้ำ
```

---

## Discovery

Skills เป็น **opt-in ต่อ agent** ไม่ใช่ใช้ได้ทั่ว `config.manifest.json` ของ agent ประกาศว่าจะใช้ framework skill ใดบ้าง:

```json
{
  "skills": {
    "framework": [
      { "name": "worktree-discipline", "version": ">=0.1.0", "enabled": true }
    ]
  }
}
```

Schema ของแต่ละ entry ใน `skills.framework[]`:

- `name` (string, บังคับ) — ชื่อไดเรกทอรีของ skill ที่ติดตั้งใต้ `modules/skills/`
- `version` (string, บังคับ) — semver constraint ที่ agent ยอมรับ; resolve กับ `[skill].version` ใน manifest ของ skill
- `enabled` (bool, บังคับ) — กำหนดว่า skill จะถูกโหลดตอน spawn agent หรือไม่ ตั้ง `false` เพื่อเก็บ entry ไว้แสดงเจตนาแต่ไม่ load สอดคล้องกับ pattern `workspace.toml [plugins.<name>] enabled` ใน [`PLUGINS.th.md`](PLUGINS.th.md); ใช้ `bwoc skill disable <name>` เพื่อ flip โดยไม่ลบ entry

Entry ที่ไม่มีฟิลด์ `enabled` ถือเป็น manifest error — `bwoc check` จะปฏิเสธ ไม่มี default โดยปริยาย; เจตนาที่ชัดเจนคือ contract

เมื่อ spawn agent เฟรมเวิร์กจะ:

1. อ่านรายการ `skills.framework` จาก manifest ของ agent
2. กรองเฉพาะ entry ที่ `enabled` เป็น `true` Entry ที่ `enabled = false` ยังอยู่ใน manifest (เป็นเจตนาที่บันทึกไว้) แต่ถูกข้ามตอนโหลด
3. Resolve แต่ละรายการกับไดเรกทอรี `modules/skills/<name>/` ของ workspace
4. Resolve `[contract] requires` (skill อื่น) และ `[contract] requires_plugins` (**kind** ของ plugin) ของแต่ละ skill กับ workspace — skill ที่ระบุทุกตัวต้องติดตั้ง-และ-enable และ kind ที่ระบุทุกตัวต้องมี `workspace.toml [plugins.<name>]` ที่ enable และเป็น kind นั้น (ดู [Skill-on-plugin dependency](#skill-on-plugin-dependency))
5. โหลด manifest ของแต่ละ skill และรัน `init`
6. ปฏิเสธการ spawn ถ้า required skill ไม่มี, plugin kind ที่ต้องการไม่ถูก enable, หรือ verify gate ใดไม่ผ่าน

ไม่มี central index Workspace รู้จัก skill ตัวหนึ่งเพราะมันอยู่ใต้ `modules/skills/` เท่านั้น Source สามารถมาจาก remote (git / tarball / local path — ดู [Sources & Installation](#sources--installation)) แต่ **การ resolve ตอน spawn เป็นแบบ local ต่อ workspace เสมอ** — ไม่มี network call ตอน runtime **Anattā** ยังคงอยู่

---

## Skill-on-plugin dependency

Skill อาจพึ่งพาไม่เพียงแค่ **skill** อื่น (`[contract] requires`) แต่พึ่งพา **plugin** ได้ (`[contract] requires_plugins`) Skill เป็น capability ของ agent; plugin เป็น integration ของเฟรมเวิร์ก ([`PLUGINS.th.md`](PLUGINS.th.md#skill-กับ-plugin)) เมื่อ skill เป็นเพียง wrapper บาง ๆ ด้วยคำศัพท์ scrum/agent บน integration ที่ workspace โหลดอยู่แล้ว มันจะประกาศ integration นั้นเป็น dependency แทนที่จะ reimplement ใหม่ Skill แบบนี้ตัวแรกคือ [`scrum-via-jira`](../../modules/skills/scrum-via-jira/SPEC.md) ซึ่งห่อ verb `bwoc jira` ของ plugin kind `jira`

```toml
[contract]
requires         = ["worktree-discipline"]   # ชื่อ framework SKILL
requires_plugins = ["jira"]                   # KIND ของ plugin — ไม่ใช่ชื่อ plugin
exposes          = ["transition-story", "..."]
```

### kind ไม่ใช่ name

`requires_plugins` ระบุ **kind** ของ plugin (enum ใน [`PLUGINS.th.md` §ประเภทของปลั๊กอิน](PLUGINS.th.md#ประเภทของปลั๊กอิน): `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`, …) ไม่ใช่ `name` ของ plugin เฉพาะตัว Skill ที่ต้องการ Jira พึ่งพา adapter kind `jira` *ตัวใดก็ได้* ที่ enable อยู่ ไม่ใช่ `jira-cloud-rest` โดยเฉพาะ — ดังนั้นสลับ adapter ได้โดยไม่แตะ skill และ skill ยังเป็นกลาง นี่คือเหตุผลที่ใช้ฟิลด์เฉพาะแทนการ overload `requires`: `requires` resolve กับชื่อ skill, `requires_plugins` resolve กับ enum ของ kind การรวมสอง namespace จะทำให้ `"jira"` เปล่า ๆ กำกวม (skill ชื่อ `jira`? หรือ kind `jira`?)

### ทิศทางเป็นทางเดียว: skill → plugin

Skill เรียก verb ของ plugin; plugin ไม่รู้จัก skill และใช้งานได้เต็มที่โดยไม่มี skill (operator รัน CLI ของ plugin ตรง ๆ) plugin จึงไม่เคยประกาศ dependency บน skill — ลูกศร dependency ชี้ skill → plugin เสมอ

### การ Resolve

- **ตอน spawn agent** — สำหรับทุก kind ใน `requires_plugins` เฟรมเวิร์กตรวจว่า workspace มี plugin kind นั้นที่ **enable** อยู่ (`workspace.toml [plugins.<name>]` ที่ `kind` ตรงกัน) ถ้าไม่มีตัวไหน enable, spawn ล้มเหลวทันทีพร้อม diagnostic ระบุ kind ที่ขาด; agent ไม่ถูกต่อสายครึ่ง ๆ (Discovery ขั้นที่ 4)
- **เร็วกว่านั้น ผ่าน `bwoc skill verify <name>`** — รันการตรวจเดียวกันก่อนถึงเวลา spawn เพื่อให้ช่องว่างปรากฏใน CI / pre-flight ไม่ใช่ตอน runtime
- **แบบ static ผ่าน `bwoc check`** — validate ว่าทุกค่าใน `requires_plugins` เป็น kind enum ที่ valid (ดู [Verification](#verification)) ไม่ต้องการให้ plugin enable ตอน check — การ enable เป็นเรื่องของ spawn ต่อ agent, ความ valid ของ kind เป็นเรื่องของ manifest

---

## CLI Surface

Surface แบบ read-only (ไม่มี side-effect กับ workspace):

```
bwoc skill list                     # ลิสต์ framework skill ที่ติดตั้ง
bwoc skill list --enabled           # กรองเฉพาะที่เปิดบน agent ปัจจุบัน
bwoc skill list --json              # machine-readable

bwoc skill show <name>              # manifest + spec เต็มของ skill หนึ่งตัว
bwoc skill show <name> --json

bwoc skill verify <name>            # รันคำสั่งจาก [gates]
bwoc skill verify --all             # verify ทุก skill ที่ติดตั้ง
```

Surface สำหรับ lifecycle (write — รายละเอียดดูในหัวข้อที่อ้างถึง):

```
bwoc skill init <name>              # scaffold skill ใหม่จาก modules/skill-template/
                                    #   (ดู "Scaffolding from template")

bwoc skill install <source>         # ติดตั้งจาก local path / git URL / tarball URL
                                    #   (ดู "Sources & Installation")

bwoc skill enable <name>            # ตั้ง enabled=true ใน config.manifest.json ของ agent ปัจจุบัน
bwoc skill disable <name>           # ตั้ง enabled=false (เก็บ entry ไว้)

bwoc skill remove <name>            # ลบ modules/skills/<name>/ และ clean manifest ของ
                                    #   ทุก agent ที่ใช้ skill นี้ (ดู "Removal")
```

คำสั่ง read-only ทั้งหมดมี `--json` คู่ คำสั่ง lifecycle emit JSON ที่มีโครงสร้างเมื่อใส่ `--json` (event-shape ต่อคำสั่ง) `verify` exit non-zero เมื่อ gate ไม่ผ่าน; `install` exit non-zero เมื่อ trust-gate ไม่ผ่าน; `remove` exit non-zero เมื่อ target ไม่มี เว้นแต่ใส่ `--yes`

### Resolve "current agent"

`enable`, `disable`, และ `remove` ต้องรู้ว่าจะแก้ `config.manifest.json` ของ agent ตัวไหน เฟรมเวิร์ก resolve ตามลำดับนี้ หยุดที่ตัวที่ match ก่อน:

1. **flag `--agent <name>`** — override ชัดเจน ชนะเสมอ
2. **environment variable `BWOC_AGENT`** — มีประโยชน์สำหรับ shell session ที่ scope ไป agent ตัวเดียว
3. **Working directory** — ถ้า cwd อยู่ใต้ `<workspace>/agents/<id>/` ใช้ `<id>` นั้น
4. **อื่น ๆ** — error: `no agent context; pass --agent <name> or run from within an agent directory`

`bwoc skill remove --all-agents <name>` ข้าม resolution นี้และ apply กับทุก agent ที่ใช้ skill ใน workspace (ยัง confirm กับ user เว้นแต่ใส่ `--yes`)

---

## Sources & Installation

Framework skill เข้าสู่ workspace ได้ 2 ทาง — เขียนเองใต้ `modules/skills/<name>/` หรือ install จาก 3 ประเภท source:

| ประเภท Source | ตัวอย่าง | การตรวจจับ |
|---|---|---|
| **Local path** | `bwoc skill install ./vendor/my-skill/` | argument ขึ้นต้นด้วย `./`, `../`, หรือ `/` และ resolve เป็นไดเรกทอรี |
| **Git URL** | `bwoc skill install https://github.com/org/skill.git#v0.1.0` | argument มี scheme `http(s)://` หรือ `git://` และลงท้าย `.git` (`#<ref>` เลือก branch / tag / sha) |
| **Tarball URL** | `bwoc skill install https://example.com/skill-0.1.0.tar.gz` | argument มี scheme `http(s)://` และลงท้าย `.tar.gz` หรือ `.tgz` |

กลไกการ install:

1. Resolve ประเภท source จาก argument
2. **Pre-flight** — ถ้า source ไม่มี `manifest.toml` ที่ root → refuse พร้อม error `source missing manifest.toml; cannot resolve name or kind` ไม่ fetch / extract / write อะไร
3. **Trust gate** (ดูข้างล่าง) — fetch และ verify SHA-256 checksum
4. Materialize source ลง `modules/skills/<name>/` (copy สำหรับ local; clone-แล้ว-ทิ้ง-`.git` สำหรับ git; extract สำหรับ tarball)
5. Validate manifest ที่ติดตั้งด้วย `bwoc check`
6. บันทึก install ลง `.bwoc/installed-sources.toml` (schema ด้านล่าง) เขียน registry record เฉพาะเมื่อสำเร็จเท่านั้น
7. **ไม่ auto-enable** Skill ที่ติดตั้งแล้ว dormant จนกว่าจะเรียก `bwoc skill enable <name>` บน agent ที่จะใช้

### Re-install และการจัดการความล้มเหลว

- **Target มีอยู่แล้ว** — ถ้า `modules/skills/<name>/` มีอยู่แล้ว default behavior คือ refuse พร้อม `<name> already installed at version X; pass --upgrade to replace`
  - `--upgrade` — แทนที่ในที่เดิม เก็บ record ใน `installed-sources.toml` (update `last_hash` และ `installed_at`)
  - `--force` — แทนที่ไม่มีเงื่อนไข แม้ install ปัจจุบันมี local edit ที่ยังไม่ commit (stderr warning ระบุสิ่งที่ถูก overwrite)
- **Network failure ระหว่าง install** — install ไม่ atomic by design; เมื่อล้มเหลวชั่วคราว (download ขาด, extract error) ไดเรกทอรีบางส่วนถูกลบก่อน exit และ `installed-sources.toml` **ไม่** ถูก update ปลอดภัยที่จะ retry

### Trust gate (v1)

ทุกการ install verify SHA-256 checksum **ก่อน** materialize:

- **Tarball URL** — CLI fetch `<source>.sha256` (URL เดียวกัน + suffix `.sha256`) อ่าน digest ที่คาดหวัง และเทียบกับ digest ของ archive ที่ดาวน์โหลด
- **Git URL** — CLI fetch checksum ที่ URL โดยแทนที่ `.git` ด้วย `.sha256` ตัวอย่าง:
  - Source: `https://github.com/org/skill.git#v0.1.0`
  - Checksum: `https://github.com/org/skill.sha256` (operator เผยแพร่ manifest ของ tree-sha ที่คาดหวังตาม ref)
  - หลัง clone เฟรมเวิร์กรัน `git rev-parse <ref>^{tree}` และเทียบกับ entry สำหรับ `<ref>` ใน manifest ที่ fetch มา
  - Operator มักจะเผยแพร่ manifest นี้ผ่าน GitHub release asset หรือไฟล์ static-hosted แยก
- **Local path** — checksum เป็น optional ถ้ามีไฟล์ `<dir>.sha256` ข้างไดเรกทอรีจะ verify ถ้าไม่มีก็ install ต่อ

มี 2 flag ที่ผ่อนปรน gate:

- `--no-verify` — ข้าม checksum verification emit stderr warning ใช้กับ source ที่ develop อยู่และ serve local ผ่าน HTTP
- `--allow-new-source` — บังคับ **ครั้งแรก** ที่ install source URL หนึ่งในเวิร์กสเปซนี้ เป็นการระบุว่า "ฉันได้ตรวจสอบ source นี้แล้ว" Install ครั้งถัดไปจาก source เดิม (บันทึกใน `.bwoc/installed-sources.toml`) จะข้าม prompt นี้

Trust gate v1 จงใจให้ minimal Trust v2 ในอนาคต (signed envelopes; identity proof) ขยาย surface นี้โดยไม่ break v1 contract — flag `--no-verify` / `--allow-new-source` ยังใช้งานต่อไปได้

**Anattā ยังคงอยู่** ไม่มี central registry ไม่มี service สำหรับ resolve name-to-URL ไม่มี auto-update ทุกการ install ระบุ source อย่างชัดเจน เฟรมเวิร์กไม่ใช่ package manager

### Schema ของ `.bwoc/installed-sources.toml`

Registry ของการ install ต่อ workspace สร้างตอน install ครั้งแรก เฟรมเวิร์กไม่ลบให้ (cleanup ด้วยมือผ่าน `bwoc skill remove --forget-source` หรือ hand-edit)

```toml
# Key ด้วย source-key = SHA-256 hex ของ source argument ที่ normalize แล้ว
# Hash เป็น stable identifier — URL อาจเปลี่ยน (เช่น ref เปลี่ยน)
# โดยไม่เสีย history ของ install ที่ผ่านมา

["abc123def456..."]
url             = "https://github.com/org/skill.git#v0.1.0"   # argument เดิม
kind            = "skill"                                      # "skill" | "plugin"
name            = "worktree-discipline"                        # จาก manifest ที่ติดตั้ง
target          = "modules/skills/worktree-discipline"          # path relative ของ workspace
installed_at    = "2026-05-26T10:23:00Z"                       # ISO 8601 UTC
installed_hash  = "<SHA-256 ของ tree ที่ติดตั้ง>"                # สำหรับ drift detection
last_verified   = "2026-05-26T10:23:00Z"                       # set โดย bwoc check
acknowledged_by = "pituk.kae"                                  # ผู้ใส่ --allow-new-source
```

Field ทั้งหมดเฟรมเวิร์กดูแล — operator ไม่ควร hand-edit เว้นแต่ลบ entry ที่ stale `bwoc check` validate registry กับ filesystem ทุกครั้งที่รัน (ดู [Verification](#verification))

---

## Scaffolding from template

`bwoc skill init <name>` สร้าง skill ใหม่ใน `modules/skills/<name>/` โดย copy template ที่ `modules/skill-template/` และแทนที่ placeholder:

```
modules/skill-template/
├── manifest.toml          # มี placeholder {{skillName}}, {{skillVersion}}
└── SPEC.md                # Obsidian-formatted; placeholder สำหรับชื่อ skill + description
```

Placeholder ใช้ convention `{{camelCase}}` เดียวกับ `modules/agent-template/` Substitution ที่บังคับระบุไว้ใน [`SPEC.md`](../../modules/skill-template/SPEC.md) ของ template เอง

`bwoc skill init` เป็นวิธีที่แนะนำสำหรับเริ่ม skill ใหม่ — สร้างเองด้วยมือทำได้แต่ข้าม consistency ของ placeholder

---

## Removal

`bwoc skill remove <name>`:

1. **ลิสต์ผู้ใช้** — agent ทุกตัวที่ `config.manifest.json` อ้างถึง skill นี้ (`skills.framework[].name == <name>`)
2. **Confirm กับ user** เว้นแต่ใส่ `--yes` แสดงสิ่งที่จะลบ (ไดเรกทอรี) และแก้ (manifest ของแต่ละ agent ที่ใช้)
3. **ลบ** `modules/skills/<name>/` แบบ recursive
4. **Clean** `skills.framework[]` ใน `config.manifest.json` ของทุก agent ที่ใช้ — ลบ entry ทั้งหมด ไม่ใช่แค่ตั้ง `enabled = false`

Idempotent — `remove` กับ target ที่ไม่มีจะรายงาน "not installed" และ exit 0 flag `--yes` ข้าม confirmation prompt; operator รับผิดชอบผลที่ตามมา

Source ที่ถูกลบไม่ถูก auto-uninstall จาก `.bwoc/installed-sources.toml` — registry นั้นเก็บอยู่เพื่อให้การ re-install จาก source เดิมไม่ trigger `--allow-new-source` ใหม่ ใส่ `--forget-source` เพื่อลบ source registration ด้วย

---

## Maturity Levels

ใช้สเกล [Ariya-dhana 7](../../modules/agent-template/skills/SPEC.md#maturity-levels-ariya-dhana-7) เดียวกับ agent-template skill slot Framework skill ประกาศระดับปัจจุบันอย่างซื่อสัตย์; การ over-claim เป็น violation ของ `bwoc check`

| Level | ความหมาย |
|---|---|
| L1 | ใช้สำเร็จครั้งแรก; ยังไม่ verify |
| L2 | ใช้หลายครั้ง; verify แบบไม่เป็นทางการ |
| L3 | Verification gates ผ่านสม่ำเสมอ |
| L4 | ทนทานต่อโหมดความล้มเหลวที่พบบ่อย |
| L5 | Mentorship — skill หนึ่งใช้นำทางการออกแบบ skill อื่น |
| L6 | Cross-domain transfer — ใช้ได้นอก context เดิม |
| L7 | Canonical — skill อื่นรับมาเป็นต้นแบบ |

Skill ขยับ maturity ในการเปลี่ยน `version` ของตัวเอง; ฟิลด์ `version` และ `maturity` เลื่อนไปด้วยกัน ไม่แยกกัน

---

## Verification

`bwoc check` ขยายไปตรวจสอบ `modules/skills/<name>/` รวมถึง registry ของ source ที่ติดตั้ง:

| Check | เงื่อนไขผ่าน |
|---|---|
| Manifest parseable | `manifest.toml` เป็น TOML ที่ valid และตรง schema ด้านบน |
| ชื่อตรงกับไดเรกทอรี | `[skill].name == basename(directory)` |
| Neutrality | ไม่มีชื่อ vendor / model ID / backend CLI ในค่า manifest |
| มี `SPEC.md` | ไฟล์ `SPEC.md` อยู่ข้าง manifest |
| ฟิลด์บังคับครบ | `name`, `version`, `description`, `maturity`, `[contract] exposes` ครบ |
| kind ใน `requires_plugins` valid | ทุกค่าใน `[contract] requires_plugins` (ถ้ามี) เป็น kind enum ของ plugin ที่ valid (ไม่ต้องการให้ plugin enable — นั่นเป็นการตรวจตอน spawn) |
| Source registry parseable | `.bwoc/installed-sources.toml` เป็น TOML ที่ valid ถ้ามี |
| ไม่มี orphan source record | ทุก entry ที่ `kind = "skill"` ใน registry มี `modules/skills/<name>/` ที่ match |
| ไม่มี orphan installation | ทุก `modules/skills/<name>/` มี registry entry หรือมี marker file `.authored-in-place` (skill ที่เขียนเองในที่เลือก opt out จาก registry tracking) |
| Registry drift | `installed_hash` ใน registry match SHA-256 ปัจจุบันของ `modules/skills/<name>/` (หรือใส่ `bwoc check --update-hashes` เพื่อรับทราบ drift) |

Check ที่ไม่ผ่าน exit non-zero ใน workspace audit — surface เดียวกัน, exit semantics เดียวกันกับ `bwoc check --all` ที่มีอยู่

---

## สิ่งที่ Spec นี้ไม่ครอบคลุม

- **Per-agent skill slot** — ดู [`modules/agent-template/skills/SPEC.md`](../../modules/agent-template/skills/SPEC.md) คนละชั้น คนละ contract
- **Claude Code session skill** — ดู `.claude/skills/` แนวคิดฝั่ง tool ไม่ใช่ความกังวลของเฟรมเวิร์ก
- **การโหลด plugin** — ดู [`PLUGINS.th.md`](PLUGINS.th.md) Skill ถูก agent invoke; plugin ถูก framework load
- **Reference skill ตัวแรกเอง** — ดู story `BWOC-6` และ (เมื่อลงแล้ว) `modules/skills/worktree-discipline/SPEC.md`

---

## ดูเพิ่ม

- [`PLUGINS.th.md`](PLUGINS.th.md) — spec พี่น้อง; substrate เดียวกัน, invoker ต่างกัน
- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — modules ประกอบกับส่วนอื่นของเฟรมเวิร์กอย่างไร
- [`modules/agent-template/skills/SPEC.md`](../../modules/agent-template/skills/SPEC.md) — per-agent skill slot; เพื่อนบ้านที่ใกล้ที่สุดของ spec นี้
- [`NAMING.th.md`](NAMING.th.md) — แนวทางตั้งชื่อไฟล์และไดเรกทอรี
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — คำบาลี (Anattā, Samānattatā, Mattaññutā)
