---
title: Loop Engineering
parent: ไทย
nav_order: 8
---

# Loop Engineering

**Goal + ticker loop** คือ agent (หรือ fleet) ที่ทำงานมุ่งสู่ objective ที่คงอยู่ ถูก re-fire ตาม cadence จนกว่าจะถึง Definition-of-Done — แทนที่จะรันครั้งเดียวแล้วจบ เอกสารนี้ระบุ loop-engineering layer ที่ BWOC กำลังสร้าง: 3 objects (**Goal**, **Ticker**, **Gate**), iteration cycle, วิธี internalize pattern ของ *Refinement Loop* ที่ retire ไปแล้ว, catalog ของ use case ที่เปิดใช้ได้, และแผน build แบบ phased

> [!abstract] BWOC มี primitive ดิบทุกชิ้นที่ประกอบเป็น loop อยู่แล้ว — persistent daemon tick, action ต่อ fire (lead drain, mine, health scan), และ gate (budget, plan-approval, peer-review) สิ่งที่ขาดคือ **orchestration envelope** ที่ครอบมัน: Goal แบบ first-class ที่มี done predicate ตรวจด้วยเครื่องได้, Ticker ที่ตั้งค่าได้ (ไม่ใช่ 2 s poll hardcoded), และ gate ที่ re-fire-จน-done Loop engineering คือ envelope นั้น

## ทำไมต้องมี

สองสิ่งพิสูจน์ทั้งความจำเป็นและรูปร่าง:

1. **[Refinement Loop](../../.claude/loop-roadmap.md) ที่ retire แล้ว** ขับงาน doc + implementation ของเฟรมเวิร์กเองเป็นสัปดาห์ ผ่าน external cron × Markdown checklist ที่ maintain ด้วยมือ × "หนึ่ง coherent unit ต่อ fire" มันทำงานได้ แต่ทุกองค์ประกอบเป็น ad-hoc: cron ID ที่ทึบอยู่นอก repo, goal-store 3 อันที่ drift กัน, และ gate `🔒 HELD` แบบ **honor-system** ที่ไม่มีอะไรบังคับ มันจบไม่ใช่เพราะทำเสร็จ แต่เพราะ *supersession* — มนุษย์ re-point ไปทางอื่น
2. **daemon เนทีฟ** (`bwoc-agent --serve`) เป็น persistent tick loop อยู่แล้ว (`crates/bwoc-agent/src/main.rs:283`) แต่ "goal" เดียวที่มีคือ *มีข้อความมา* หรือ *task claim ได้* และทุก cadence เป็นค่าคงที่ hardcoded

Loop engineering ปิดช่องว่าง: เปลี่ยน pattern Markdown honor-system เป็น **typed objects + enforced gates** โดย reuse primitive ที่ผ่านสนามรบมาแล้ว

## 3 objects

### Goal

Objective ที่คงอยู่ บวก **Definition-of-Done ที่ตรวจด้วยเครื่องได้**

```
Goal {
  objective:  string        # เจตนาที่มนุษย์อ่านได้
  dod:        Predicate      # ประเมินทุก fire: goal ถึงหรือยัง?
  budget:     { iterations?, wall_clock?, tokens?, cost? }  # เพดานข้าม iteration
}
```

`Goal` ต่างจาก Saṅgha `task`: task มี *state* (`pending → in_progress → completed`); goal มี *done predicate* ที่ประเมินใหม่ทุก fire (เช่น "task list ของทีม T เป็น `Completed` หมด", "ไม่มี session ที่ยังไม่ mine เก่ากว่า 24 ชม.", "service healthy M ครั้งติด") task คือหน่วยงานที่ goal ย่อยออกมา

### Ticker

สิ่งที่ fire iteration ถัดไป abstraction เดียวครอบ 3 แหล่ง:

```
Ticker =
  | Cron    "0 9 * * MON"      # cadence ตามปฏิทิน
  | Every   Duration           # interval คงที่
  | Event   <source>           # inbox/task-mtime/webhook/A2A push
  | Adaptive { base, backoff } # กว้างขึ้นเมื่อ idle, แคบลงเมื่อ active
```

แทนที่ `TASK_POLL_EVERY = 2s` และ sleep 100 ms ที่ hardcoded (`crates/bwoc-agent/src/main.rs:254`) **prompt/objective ที่ steer ถูกแนบกับ ticker** เหมือนที่ Refinement Loop แนบ prompt กับแต่ละ cron — การ re-aim loop คือสลับ objective ไม่ใช่สลับกลไก

### Gate

สิ่งที่ตัดสินทุก fire ว่าจะ act, pause, หรือ stop ประกอบจาก gate ที่ ship อยู่แล้ว:

| Gate | ความหมาย | หนุนโดย |
|---|---|---|
| **DoD met** | goal predicate จริง → stop (สำเร็จ) | predicate check ใหม่ |
| **HELD** | ต้องการ user policy → surface ไม่ auto-act | plan-approval (Pavāraṇā) `crates/bwoc-cli/src/sangha.rs`, trust posture |
| **Budget** | เพดาน iteration/wall-clock/token/cost → stop | `crates/bwoc-harness/src/budget.rs`; rolling-window pattern จาก `crates/bwoc-cli/src/supervise.rs` |

convention `🔒 HELD` ที่ Refinement Loop เคารพด้วยมือ กลายเป็น gate ที่ **บังคับจริง**: HELD item route ไปที่ plan-approval flow และ auto-action ไม่ได้

## Iteration cycle

แต่ละ ticker fire รันหนึ่งรอบ: **ประเมิน DoD → ถ้าถึง stop → ถ้ายัง เลือกหนึ่ง coherent unit → execute → log/discover → re-gate**

- **หนึ่ง coherent unit ต่อ fire** (*Mattaññutā*) — หนึ่ง lead drain, หนึ่ง warm-harness turn, หนึ่ง mine — ไม่ใช่ burst หลายขั้นตอน นี่คือวินัยแกนของ Refinement Loop ตอนนี้เป็น design invariant
- **Discover → schedule** — งานที่เจอกลาง loop (bug, follow-up) ถูกจับเป็น Saṅgha task ใหม่ผ่าน `bwoc task add` แทน "Discovered" append-log ของ Refinement Loop นี่คือ **trigger → task bridge**: signal กลายเป็นหน่วยงานที่ scheduled + gated ไม่ใช่ note prose

## Grounding — จาก loop ad-hoc สู่ layer ที่บังคับได้

ทุกองค์ประกอบของ Refinement Loop ที่ retire แล้ว map ไปยัง BWOC-native equivalent ที่แทนกลไก ad-hoc ของมัน:

| Refinement Loop (ad-hoc) | Loop-engineering equivalent (บังคับได้) |
|---|---|
| cron ID + `/loop` skill | **Ticker** บน daemon idle loop |
| Markdown tiered checklist | Saṅgha task queue (`tasks.jsonl`) + **Goal / DoD** object |
| "หนึ่ง coherent unit ต่อ fire" (Mattaññutā) | หนึ่ง lead drain / หนึ่ง harness turn ต่อ fire — design invariant |
| "Discovered" append-log | retro triggers → `bwoc task add` (**trigger → task bridge**) |
| `🔒 HELD` honor-system | plan-approval gate (Pavāraṇā) + trust posture — **บังคับได้** |
| CHANGELOG + git + version ledger | task states + retro metrics + budget accounting |

## Use-case catalog

Loop ที่ layer นี้เปิดใช้ ทั้งหมดใช้ **core ที่ขาดเหมือนกัน** (`Goal + Ticker + Gate`); คอลัมน์ "net-new" คือสิ่งที่แต่ละอันต้องการเพิ่มจาก core

### Internal / fleet

| Goal | Ticker | action ต่อ fire | net-new นอกจาก core |
|---|---|---|---|
| **ดันงานทีม T ให้เป็น `Completed` หมด** | task-mtime event | หนึ่ง `run_lead` drain (`crates/bwoc-harness/src/lead.rs:152`) | DoD predicate + re-fire wrapper — *ถูกสุด* |
| **คง fleet-health conditions ให้เขียว** | interval | `bwoc fleet health` → `bwoc doctor --auto` เมื่อ Warn | timer glue + auto-fixable-class policy |
| **คง Tier-2 memory ของแต่ละ agent ให้ทันสมัย** | adaptive / nightly | `bwoc memory mine <sessions> <agent>` | session cursor + scheduler entry |
| **retro/report หนึ่งฉบับต่อ period** | cron | `bwoc retro new` (metrics-prefill) + `bwoc report` | calendar trigger + idempotency หนึ่งต่อ period |
| **Framework self-improvement** (loop เดิม productized) | run-end event | retro `Trigger` → `bwoc task add` → lead drain | trigger→task bridge + multi-run DoD |
| **Ship release** | operator kick | `bwoc run` gates → tag → notes | release orchestration (tag / semver / changelog) |

### External / product

| Goal | Ticker | action ต่อ fire | หมายเหตุ |
|---|---|---|---|
| **คง repo ให้เขียว + ทันสมัย** (CI-babysit) | interval / CI webhook | trusted headless turn: bump → build → open PR | productize งาน release-PR |
| **Resolve inbound request** | message arrival + follow-ups | warm per-sender turn ผ่าน `AutoProcessor` | ต้องมี *trust tier กลาง* (act-as-user) |
| **Watch source, alert เมื่อ trip** | cron / interval | fetch → predicate → `bwoc send` alert | **flagship**: ที่ขาดคือ scheduler เดียว |
| **ส่ง recurring digest** | cron | aggregate → render → deliver | idempotency หนึ่งต่อ period |
| **Delegate sub-goal ให้ peer** | poll / A2A push | `message/send` → `tasks/get` จน `Completed` | ต้องมี driver loop + join |
| **ดัน incident สู่ recovery** | alert → cadence แคบลง | read-only diagnose → notify → verify | dynamic cadence + recovery gate |

## แผน build (phased)

1. **Phase L1 — Goal loop รอบ lead** *(ROI สูงสุด, risk ต่ำสุด)* ครอบ `run_lead` ที่ hardened แล้วด้วย `Goal + Ticker + Gate`: re-fire เมื่อ task-list เปลี่ยน, DoD = list เป็น `Completed` หมด, HELD เมื่อ task เป็น `requires_plan`, budget = ผลรวม worker budget validate ทั้ง layer บน primitive ที่ทดสอบมากสุด
2. **Phase L2 — Ticker-driven fleet loops** เพิ่ม ticker `Cron`/`Every`/`Adaptive` บน daemon idle loop แล้ว wire loop health-remediation กับ Tier-2-mining
3. **Phase L3 — Product loops** scheduled monitoring/alerting (flagship external case), ตามด้วย inbound-service กับ A2A-delegation loop — พวกนี้ต้องมี **trust tier กลาง** (act-as-authenticated-user ระหว่าง trusted-headless กับ untrusted-read-only วันนี้) และ **idempotency/dedup** primitive

## Non-goals & safety

- **ไม่ใช่ autopilot ที่รันตลอด** ทุก loop ต้องมี stop ที่พิสูจน์ได้: DoD, budget, หรือ HELD surface loop ที่ไม่มี terminal condition (จุดพังของ BabyAGI/AutoGPT) ถูกปฏิเสธตั้งแต่ spec — budget gate บังคับ
- **HELD บังคับได้ ไม่ใช่แค่แนะนำ** งาน policy-bearing route ไป plan-approval; loop draft หรือ action มันไม่ได้
- **คง trust posture** untrusted inbound loop คง read-only; effectful loop คง trusted-หรือ-approved product loop ใหม่ที่ "act as user" ต้องใช้ L3 middle tier — ไม่ได้เปิดโดยขยาย binary เดิม
- **Durability เลื่อนไว้** L1–L2 loop host บน daemon (crash-restart ผ่าน `bwoc supervise`); durable pause/resume ข้าม restart (แบบ Temporal) เป็นเรื่องทีหลัง ติดตามเมื่อมี loop จริงต้องรอด restart กลาง goal

## เอกสารอ้างอิง

- [Refinement Loop (retired)](../../.claude/loop-roadmap.md) — prototype ad-hoc ที่ layer นี้ internalize
- [`ROADMAP.th.md`](ROADMAP.th.md) — ที่ L1–L3 จะถูก ticket
- [`FLEET-GOVERNANCE.th.md`](FLEET-GOVERNANCE.th.md) — fleet-health conditions ที่ monitoring loop ขับ
- Saṅgha teams + task queue: [`sangha.th.md`](../../modules/agent-template/interconnect/sangha.th.md)
