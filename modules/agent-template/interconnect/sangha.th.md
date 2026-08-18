---
title: สังฆะ — ทีม & รายการงานร่วม
aliases:
  - Saṅgha
  - Agent Teams
  - สังคหวัตถุ 4
tags:
  - group/agents
  - type/design
  - meta/template
status: draft (v2026.5.23 — Phase A: สมาชิก + รายการงานร่วม + self-claim)
canonical-source: สังคหวัตถุ (AN 4.32, DN 31) · สังฆกรรม (พระวินัย มหาวรรค)
---

# สังฆะ — ทีม & รายการงานร่วม

> [!abstract] **ทีม** (สังฆะ) จัดกลุ่ม agent ส่วนหนึ่งของ workspace ให้ใช้รายการงานร่วมกันหนึ่งรายการ ผู้ดำเนินการที่เป็นมนุษย์คือ lead โดยปริยาย สมาชิก **หยิบงานเอง (self-claim)** เฉพาะงานที่ pending และไม่ติด dependency; advisory file lock ทำให้การ claim แต่ละครั้งเป็น "สังฆกรรม" — การกระทำร่วมที่สมาชิกเพียงคนเดียวเป็นผู้ปิดได้ ส่งแล้ว: รากฐาน CLI + ไฟล์ (Phase A), daemon task-watch พร้อม opt-in wakeup + auto-claim (Phase B/B+), task hooks, และ plan approval (ปวารณา, Phase C) ส่วน dashboard pane ที่รู้จักทีม กับ lead-agent ที่กำหนด เป็นงานในอนาคต

## แรงจูงใจ

BWOC มีชิ้นส่วนให้ agent *คุยกัน* อยู่แล้ว — inbox ([`send.rs`](../../../crates/bwoc-cli/src/send.rs)), sender identity ที่ตรวจสอบได้ + trust gate Kalyāṇamitta 7 ([[trust]]), และกฎความนุ่มนวล Sāraṇīyadhamma 6 ([[messaging]]) สิ่งที่ยังขาดคือที่ให้ agent *ประสานงานกัน*: รายการงานร่วมที่หยิบ claim, complete, และ gate กันได้

ต่างจาก subagent แบบ one-shot (ที่รายงานกลับอย่างเดียว) ทีมใช้รายการงานร่วมและให้สมาชิกหยิบงานได้อิสระ — เป็นเส้นแบ่งเดียวกับที่ Claude Agent Teams แยกระหว่าง subagent กับ teammate จุดต่างของ BWOC: state การประสานงานเป็นไฟล์ธรรมดาใน workspace (`.bwoc/teams/`) เป็นกลางต่อ transport และ backend ใดก็ตามที่รันผ่าน `bwoc spawn` อ่านได้

## รากฐานพุทธธรรม

| แนวคิด | ที่มา | การประยุกต์ใน BWOC |
|---|---|---|
| **สังฆะ** | หมู่ผู้ปฏิบัติ | ทีม — เซตของ agent ที่มีชื่อและขอบเขต ทำงานในรายการร่วม ≥1 สมาชิก; มนุษย์เป็น lead ไม่นับเป็นสมาชิก |
| **สังคหวัตถุ 4** | AN 4.32 — 4 ฐานของความสามัคคี | กฎที่ทีมยึด: **ทาน** (แบ่งสิ่งที่ค้นพบ ไม่กั๊ก), **ปิยวาจา** (ชื่องาน + ข้อความที่นุ่มนวล — ดู [[messaging]]), **อัตถจริยา** (หยิบงานที่ช่วยทีม ไม่ใช่แค่ตัวเอง), **สมานัตตตา** (ทุกสมาชิกเท่ากันต่อหน้ารายการงาน — ไม่มีผู้ claim อภิสิทธิ์) |
| **สังฆกรรม** | พระวินัย มหาวรรค — การกระทำร่วมอย่างเป็นทางการ | โปรโตคอลการ claim: งานเปลี่ยนสถานะไปยังสมาชิกหนึ่งคนภายใต้ lock เพื่อให้สองคนไม่ claim งานเดียวกัน lock *คือ* องค์ประชุมที่ทำให้กรรมนั้นสมบูรณ์ |

> [!note] สังคหวัตถุ 4 เป็น **norm ไม่ใช่ gate** ใน Phase A — `bwoc` ไม่บังคับชื่องานนุ่มนวลหรือการ claim ที่ไม่เห็นแก่ตัว มันอยู่ที่นี่เพื่อให้ agent ที่ incarnate ซึมซับเอง เหมือนที่ [[messaging]] บรรจุ Sāraṇīyadhamma 6

## โครงสร้างข้อมูล

ทีม = สมาชิก + รายการงาน

```toml
# .bwoc/teams/<team-id>.toml
id = "squad"
members = ["agent-pi", "agent-oracle"]
created_at = "2026-05-23T06:47:15Z"
```

```jsonl
# .bwoc/teams/<team-id>/tasks.jsonl  — หนึ่งงานต่อบรรทัด
{"id":"t1","title":"design schema","state":"completed","created_at":"…","claimed_by":"agent-pi","completed_at":"…"}
{"id":"t2","title":"implement","state":"in_progress","deps":["t1"],"created_at":"…","claimed_by":"agent-oracle"}
```

- **ไม่มี field `lead`** ผู้ดำเนินการที่เป็นมนุษย์คือ lead โดยปริยาย — เป็นผู้สร้างทีม, เพิ่มงาน, และสรุปผล (lead ที่เป็น *agent* เป็นส่วนขยาย v2 ที่เป็นไปได้ ไม่ใช่ v1)
- **`deps`** คือรายการ task id ที่ต้อง `completed` ก่อนงานนี้จะ claim ได้ ละไว้เมื่อว่าง
- **`claimed_by`** ตั้งตอน claim และคงไว้จน complete (audit trail ว่าใครทำงาน)

### State machine ของงาน

```
pending ──claim──▶ in_progress ──complete──▶ completed
```

- **claim**: งานต้อง `pending` และทุก dependency `completed` ตั้ง `in_progress` + `claimed_by`
- **complete**: งานต้อง `in_progress` และผู้กระทำต้องเป็นผู้ claim ตั้ง `completed` + `completed_at`
- งานไม่เคยถูกลบ (งานที่ completed คงไว้เป็น audit trail) ดังนั้น auto-id (`t1`, `t2`, …) จึง monotonic

## CLI surface

```bash
# ทีม
bwoc team create <id> --members a,b,c     # นิยามทีม
bwoc team list                            # ทีม + จำนวนสมาชิก/งาน
bwoc team retire <id> [--yes]             # ลบสมาชิก + รายการงาน (ทำลาย)

# งาน (ทำกับทีมเดียว)
bwoc task add <team> "<title>" [--deps t1,t2] [--id <custom>]
bwoc task list <team>                     # id · state · ผู้ claim · title
bwoc task claim <team> <task> --as <agent>      # หยิบเอง (เฉพาะสมาชิก)
bwoc task complete <team> <task> --as <agent>   # เฉพาะผู้ claim
```

ทุกคำสั่งรับ `--workspace` (resolution มาตรฐาน: flag → `BWOC_WORKSPACE` → ancestor walk → cwd) และ `--json` สำหรับ output แบบ structured

> [!example] agent ที่รันใน `bwoc spawn` หยิบงานเองด้วย id ของตัวเอง:
> ```bash
> bwoc task claim squad t2 --as agent-oracle
> ```
> การ claim งานที่ติด dependency หรือถูก claim ไปแล้ว exit `2` พร้อมข้อความที่ทำตามได้; ไฟล์งานไม่ถูกแตะ

## Concurrency — lock

`bwoc task add/claim/complete` จะ acquire advisory lock (`.bwoc/teams/<id>/tasks.lock`) ก่อน read-modify-write lock เป็นไฟล์ `O_CREAT | O_EXCL` แบบไม่พึ่ง dependency เก็บ PID ของผู้ถือ; lock ที่ stale (PID ตาย ตรวจด้วย signal-0) จะถูกยึด สอง agent แข่งกัน claim งานเดียวกันจะถูก serialize: คนหนึ่งชนะ (`in_progress`) อีกคนอ่านงานที่เป็น `in_progress` แล้วถูกปฏิเสธ ตรวจสอบจริงด้วยสอง process `bwoc task claim` พร้อมกัน

## การปฏิเสธ & exit code

| สถานการณ์ | Exit | รูปข้อความ |
|---|---|---|
| ติด dependency | 2 | `task 't2' is blocked: dependency 't1' is not completed` |
| ถูก claim แล้ว / สถานะผิด | 2 | `task 't1' is in_progress — only pending tasks can be claimed` |
| ผู้ไม่ใช่สมาชิก claim | 2 | `agent 'x' is not a member of this team (members: …)` |
| complete โดยผู้ที่ไม่ได้ claim | 2 | `task 't1' is claimed by 'agent-pi', not 'agent-oracle'` |
| lock contention timeout | 1 | `could not acquire task lock (… remove tasks.lock if stale)` |

## Phase B — daemon task-watch (ส่งแล้ว)

`bwoc-agent --serve` ที่รันอยู่จะเฝ้ารายการงานร่วมของทุกทีมที่ agent เป็นสมาชิก และประกาศงานที่ claim ได้ใหม่ไปยัง stderr — รูปแบบเดียวกับ inbox watch:

```text
bwoc-agent: task available ← squad/t3: implement the parser
```

"claim ได้" = `pending` ที่ทุก dependency `completed` ในทีมที่เป็นสมาชิก daemon snapshot งานที่เปิดอยู่แล้วตอน startup (ไม่ replay — เหมือน inbox cursor เริ่มที่ EOF) และ poll ที่ cadence 2 วินาที (งานเปลี่ยนไม่บ่อย) ปรับได้ด้วย `BWOC_TASK_POLL_SECS` (default `2`, floor 1s) — fleet ใหญ่เพิ่มค่าเพื่ออ่าน team file ถี่น้อยลง, ทีมเร็วลดค่าเพื่อ pickup ไวขึ้น inert เมื่อ agent ไม่อยู่ทีมใดหรือไม่มี workspace ดู [`crates/bwoc-agent/src/task_watch.rs`](../../../crates/bwoc-agent/src/task_watch.rs)

**Wakeup แบบ opt-in** (`BWOC_TASK_WAKEUP=1`): เมื่อมี task ที่ claim ได้ใหม่ daemon จะ ping tmux session ของ agent (`agent-<x>` → session `<x>`) ด้วย marker `[bwoc task <team>/<id>] <title>` — กลไก two-step send-keys best-effort เดียวกับ inbox Claude session ที่รัน agent อยู่จะเห็นแล้ว `bwoc task claim` ได้ agent ยังคุมเอง: daemon ไม่ mutate รายการ default ปิด (announce-only)

**Auto-claim แบบ opt-in** (`BWOC_AUTO_CLAIM=1`): โหมดทำงานเป็นทีมอัตโนมัติ เมื่อมี task ที่ claim ได้ใหม่ daemon จะ claim ให้ agent ของตัวเอง — ผ่าน path `bwoc task claim` ที่มี lock จึง serialize กับสมาชิกอื่นได้ (แพ้ race ก็แค่ log `auto-claim … skipped`) — แล้วปลุก agent ให้ทำงาน นี่คือโหมดเสี่ยงที่สุด (daemon mutate shared state) จึง gate แยกจาก `wakeup` และปิด default loop เต็ม: `bwoc task add` → daemon เห็น → claim ให้ agent → ปลุก agent ดู [`crates/bwoc-agent/src/task_watch.rs`](../../../crates/bwoc-agent/src/task_watch.rs)

## Task hooks (ส่งแล้ว)

shell hook ระดับ workspace แบบ optional ทำงานตาม lifecycle ของงาน mirror `TaskCreated` / `TaskClaimed` / `TaskCompleted` ของ Claude Agent Teams hook คือ executable ที่ `<workspace>/.bwoc/hooks/<event>`; ไฟล์ที่ไม่มี — หรือบน Unix ไฟล์ที่มีอยู่แต่ไม่ executable — เป็น no-op เงียบ (hook เป็น opt-in) คอลัมน์ตัวแปรด้านล่างลิสต์ **`BWOC_*` context** ที่แต่ละ event เพิ่มให้; environment เต็มที่ hook เห็นคือ context นั้นบวกกับ safe base ที่ถูก scrub (ดู *Contract*)

| Event — `<workspace>/.bwoc/hooks/…` | ทำงานเมื่อ | `BWOC_*` context ที่เพิ่ม |
|---|---|---|
| `task-created` | `bwoc task add` กำลังจะ persist งาน | `BWOC_TASK_EVENT`, `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_TASK_TITLE` |
| `task-claimed` | `bwoc task claim` กำลังจะ persist การ claim | `BWOC_TASK_EVENT`, `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_AGENT`, `BWOC_WORKTREE_BASE` |
| `task-completed` | `bwoc task complete` กำลังจะ persist การ complete | `BWOC_TASK_EVENT`, `BWOC_TEAM`, `BWOC_TASK_ID`, `BWOC_AGENT` |

**Contract.** **exit ไม่เป็นศูนย์ block การทำงาน** — ไฟล์งานไม่ถูกแตะ และ stderr บรรทัดแรกของ hook โผล่ให้ operator (exit 2) hook รันโดยตั้ง cwd เป็น workspace และ **environment ที่ถูก scrub**: เหลือแค่ base ที่ปลอดภัย (`PATH`, `HOME`, …) กับ context `BWOC_*` ข้างบน — secret รอบตัวของ operator (token, SSH agent, cloud creds) **ไม่** ถูก inherit ดังนั้น hook ที่ต้องการมากกว่านี้ต้องอ่านจากไฟล์ ชื่อ event ถูก validate ให้เป็น kebab segment เดียว ใช้ hook สำหรับ quality gate: เช่น hook `task-completed` ที่รัน `cargo test` แล้ว exit ไม่เป็นศูนย์เพื่อปฏิเสธ completion จนกว่า test จะผ่าน

## Plan approval — ปวารณา (ส่งแล้ว)

สำหรับงานเสี่ยงหรือกระทบกว้าง task บังคับให้ lead เซ็นรับ plan ก่อน complete ได้ — map กับ **ปวารณา** (ภิกษุเชื้อเชิญสงฆ์ให้ว่ากล่าวตักเตือนเมื่อออกพรรษา: ยอมให้ตรวจสอบก่อนดำเนินต่อ)

- `bwoc task add <team> "<title>" --requires-plan` — gate งานนี้ด้วย plan approval
- `bwoc task plan <team> <task> --as <agent> --plan "<text>"` (หรือ `--plan-file`) — ผู้ claim ส่ง/แก้ plan (task ต้อง `in_progress`) ส่งใหม่ reset verdict กลับเป็น pending
- `bwoc task plan <team> <task>` (ไม่มี `--as`/`--plan`) — แสดง plan + verdict ปัจจุบัน
- `bwoc task approve <team> <task>` / `bwoc task reject <team> <task>` — verdict ของ lead (ไม่มี `--as`; มนุษย์คือ lead) reject ส่งกลับไปแก้
- `bwoc task complete` บน task ที่ `requires_plan` ถูกปฏิเสธจนกว่า `plan_approved == true` — gate อยู่ใน `bwoc-core::team::complete_task` จึงคงอยู่ไม่ว่า surface ใดเรียก complete

task ที่ไม่ใช้ plan (`requires_plan` default false) complete ได้เหมือนเดิม — gate เป็น opt-in ต่อ task

## เลื่อนไป phase ถัดไป

- **Dashboard รู้จักทีม** — task pane ใน `bwoc dashboard` (detail pane แสดงสถานะทีมต่อ agent แล้ว; pane team/task เต็มเป็นก้าวถัดไป)
- **Lead agent ที่กำหนด** — field `lead` + การกระทำเฉพาะ lead เฉพาะเมื่อโมเดล human-implicit-lead พิสูจน์ว่าจำกัดเกินไป

## ที่เกี่ยวข้อง

- [[trust]] — Kalyāṇamitta 7; gate *ว่าใคร* ที่ teammate รับข้อความได้
- [[messaging]] — Sāraṇīyadhamma 6; ช่องทางที่ teammate คุยกัน
- [`crates/bwoc-core/src/team.rs`](../../../crates/bwoc-core/src/team.rs) — โครงสร้างข้อมูล + กฎ transition
- [`crates/bwoc-cli/src/sangha.rs`](../../../crates/bwoc-cli/src/sangha.rs) — CLI + lock
- ธรรมาภิบาลระดับ fleet (มุมมอง operator ต่อ agent หลายตัว): `docs/th/FLEET-GOVERNANCE.th.md` (อปริหานิยธรรม 7)
