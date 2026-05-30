# SRS — Software Requirements Specification

## Agent Base Profile (โครงตามมรรค 8)

| | |
|---|---|
| **เอกสาร** | SRS.th.md |
| **เวอร์ชัน** | 2.0 |
| **วันที่** | 2026-05-22 |
| **ภาษาคู่** | SRS.en.md |
| **อ้างอิงปรัชญา** | PHILOSOPHY.th.md |
| **อ้างอิง PRD** | PRD.th.md |

> **โครงเอกสาร** Functional Requirements จัดเป็น 8 หมวดตามมรรค 8
> โดยมี cross-cutting principles: โยนิโสมนสิการ, ไตรลักษณ์, มัตตัญญุตา

---

## 0. บทนำ

### 0.1 วัตถุประสงค์
เอกสารนี้กำหนด requirements ทาง software ของ Agent Base Profile ทั้ง functional และ non-functional พร้อม interface และ data contract

### 0.2 ขอบเขต
ระบบที่ spec ครอบคลุมคือ **template repository** — ไม่ใช่ runtime

### 0.3 สัญลักษณ์
- **Priority:** M = Must (จำเป็น), S = Should (ควร), C = Could (อาจมี)
- **Verify:** T = Test, I = Inspection, D = Demo, A = Analysis
- **Requirement ID:** `FR-{มรรค}.{ลำดับ}` เช่น `FR-7.1` = สัมมาสติ ข้อที่ 1

---

## 1. Functional Requirements (จัดตามมรรค 8)

### หมวด 1 — สัมมาทิฏฐิ (Right View) : Persona & Identity

| ID | P | Requirement | V |
|---|---|---|---|
| FR-1.1 | M | `persona/README.md` SHALL กำหนด identity, role, principles, constraints ของ agent | I |
| FR-1.2 | M | Persona SHALL ระบุ scope ของความสามารถ (อัตตัญญุตา) | I |
| FR-1.3 | M | Persona SHALL ระบุสิ่งที่ agent **ไม่ทำ** (มัตตัญญุตา boundaries) | I |
| FR-1.4 | S | Persona SHOULD อ้างอิง PHILOSOPHY.md เพื่อแสดงพื้นฐานการคิด | I |
| FR-1.5 | M | AGENTS.md SHALL เป็น single source of truth | I |

### หมวด 2 — สัมมาสังกัปปะ (Right Intention) : Goal Setting

| ID | P | Requirement | V |
|---|---|---|---|
| FR-2.1 | M | ทุก task SHALL เริ่มด้วยกระบวนการอริยสัจ (ทุกข์→สมุทัย→นิโรธ→มรรค) | A |
| FR-2.2 | M | Task SHALL มี `taskId` และ `goal` ที่วัดได้ | T |
| FR-2.3 | M | Task SHALL ถูก track ใน `task-log.jsonl` (one JSON ต่อบรรทัด) | T |
| FR-2.4 | M | Status field SHALL เป็นหนึ่งใน: `pending`, `in_progress`, `blocked`, `completed`, `failed` | T |
| FR-2.5 | M | `task-log.jsonl` SHALL append-only | A |
| FR-2.6 | S | Task SHOULD declare scope boundaries (มัตตัญญุตา) ก่อนเริ่ม | I |

### หมวด 3 — สัมมาวาจา (Right Speech) : Inter-Agent Communication

| ID | P | Requirement | V |
|---|---|---|---|
| FR-3.1 | M | `interconnect/capabilities.md` SHALL ประกาศ skills แบบ machine-readable | T |
| FR-3.2 | S | `interconnect/coordination.md` SHALL กำหนด phases, messaging, consensus | I |
| FR-3.3 | S | Message ระหว่าง agent SHALL ตรงประเด็น มี context ครบ (ปิยวาจา) | A |
| FR-3.4 | M | Error message SHALL ระบุ root cause และวิธีแก้ ไม่เพียงแค่บอกว่าผิด | A |
| FR-3.5 | C | Agent COULD publish capabilities ไปยัง shared registry | D |
| FR-3.6 | M | เอกสารทุกฉบับ SHALL bilingual (th + en) | I |

### หมวด 4 — สัมมากัมมันตะ (Right Action) : Worktree & Commit Discipline

| ID | P | Requirement | V |
|---|---|---|---|
| FR-4.1 | M | ทุก task SHALL execute ใน worktree เฉพาะที่ `{{worktreeBase}}/{{taskId}}` | T |
| FR-4.2 | M | Agent SHALL NOT แชร์ working directory กับ agent อื่น (อนัตตา) | A |
| FR-4.3 | M | Agent SHALL NOT ใช้ `git stash` | A |
| FR-4.4 | M | Agent SHALL NOT switch branch in-place ใช้ worktree แทน | A |
| FR-4.5 | M | Commit SHALL scope เฉพาะไฟล์ที่ agent สร้าง/แก้ในงานนี้ | A |
| FR-4.6 | M | History strategy SHALL เป็น rebase ไม่ใช่ merge | A |
| FR-4.7 | M | Branch naming SHALL เป็น `<type>/{{taskId}}` โดย `<type>` ∈ {`feat fix docs refactor test chore perf style ci`}; กันชนกันหลาย agent ใช้ prefix `agent/{{agentId}}/`. SHALL ไม่มี branch `release/*` หรือ `hotfix/*` — version tag ตัดบน `main` โดยตรง (trunk-based) | T |
| FR-4.8 | M | หลัง merge SHALL ลบ worktree และ branch (อนัตตา = ไม่ยึด) | A |

### หมวด 5 — สัมมาอาชีวะ (Right Livelihood) : Trust & Neutrality

| ID | P | Requirement | V |
|---|---|---|---|
| FR-5.1 | M | `AGENTS.md` SHALL เป็นไฟล์ปกติ ; `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md` SHALL เป็น symlinks ชี้ที่ `AGENTS.md` | T |
| FR-5.2 | M | ไม่มีไฟล์คำสั่งใด SHALL contain backend-specific content ที่ขัด AGENTS.md | A |
| FR-5.3 | M | `check-agent-neutrality.sh` SHALL fail ถ้า symlink พังหรือถูกแทนด้วยไฟล์ปกติ | T |
| FR-5.4 | M | `trust-model.md` SHALL document security posture สำหรับการ clone agent ภายนอก | I |
| FR-5.5 | S | Hook ใน `.claude/settings.json` SHOULD ป้องกัน destructive action | T |
| FR-5.6 | M | ห้ามมี secret ใน memory file (สมานัตตตา + วินัย) | T |
| FR-5.7 | M | ระบบ SHALL รองรับเพิ่ม backend ใหม่ด้วย symlink ใหม่ ไม่ต้องแก้ code | I |

### หมวด 6 — สัมมาวายามะ (Right Effort) : Verification Gates

อิงปธาน 4 — เพียร 4 อย่าง

| ID | P | Requirement | Padhāna | V |
|---|---|---|---|---|
| FR-6.1 | M | Agent SHALL run `{{lintCmd}}` ก่อนประกาศงานเสร็จ | สังวร (กันชั่วใหม่) | T |
| FR-6.2 | M | Agent SHALL run `{{formatCmd}}` ทุก code change | ปหาน (ละชั่วเก่า) | T |
| FR-6.3 | M | Agent SHALL run `{{testCmd}}` ทุก logic change | ภาวนา (สร้างดีใหม่) | T |
| FR-6.4 | M | Agent SHALL run regression test เพื่อรักษา feature ที่มี | อนุรักขนา (รักษาดี) | T |
| FR-6.5 | M | Agent SHALL run `{{buildCmd}}` ก่อน push build-affecting changes | สังวร | T |
| FR-6.6 | M | UI changes SHALL verify against dev server | ภาวนา | D |
| FR-6.7 | M | Work SHALL NOT ประกาศ complete จนกว่า gates ผ่านทั้งหมด | (รวม) | A |

### หมวด 7 — สัมมาสติ (Right Mindfulness) : Memory System

#### 7.1 Tier 1 — File-Based Memory

| ID | P | Requirement | V |
|---|---|---|---|
| FR-7.1 | M | Memory files SHALL อยู่ภายใต้ `{{memoryPath}}` (default `memories/`) | I |
| FR-7.2 | M | Memory file SHALL มี YAML front-matter: `name`, `description`, `type`, `created`, `updated` | T |
| FR-7.3 | M | `type` SHALL เป็น: `user`, `feedback`, `project`, `reference` | T |
| FR-7.4 | M | `feedback` และ `project` memories SHALL มี **Why** และ **How to apply** section | I |
| FR-7.5 | M | Agent SHALL แปลง relative date (เช่น "Thursday") เป็น ISO absolute date | T |
| FR-7.6 | M | `MEMORY.md` (index) SHALL NOT เกิน 200 บรรทัด (มัตตัญญุตา) | T |
| FR-7.7 | M | Agent SHALL verify memory claims กับ current code ก่อนเชื่อ (โยนิโสมนสิการ) | A |
| FR-7.8 | S | Agent SHOULD save memories จากทั้ง failure และ success | A |
| FR-7.9 | S | Agent SHOULD NOT save อะไรที่ derive ได้จาก code, git history, AGENTS.md (มัตตัญญุตา) | A |
| FR-7.10 | M | Memory SHALL ถูก prune ตาม policy (อนิจจัง) | A |

#### 7.2 Tier 2 — Deep Memory Backend (Optional)

| ID | P | Requirement | V |
|---|---|---|---|
| FR-7.11 | S | ระบบ SHOULD เปิด placeholder `{{deepMemoryCmd}}` ใน config | I |
| FR-7.12 | S | Deep memory backend SHALL รองรับ verbs: `wake-up`, `search <query>`, `mine <path>` | T |
| FR-7.13 | C | Agent COULD invoke `wake-up` ที่ session start | D |
| FR-7.14 | C | Agent COULD invoke `mine` ที่ session end | D |
| FR-7.15 | M | Tier 2 SHALL optional ; ถ้าไม่มี ระบบ MUST ทำงานได้ปกติ | T |

#### 7.3 Session Lifecycle

| ID | P | Requirement | V |
|---|---|---|---|
| FR-7.16 | M | Session start SHALL load `MEMORY.md`, relevant memories, `task-log.jsonl` | T |
| FR-7.17 | M | Session start SHALL verify memory claims กับ current code | A |
| FR-7.18 | M | Session end SHALL update `task-log.jsonl` | T |
| FR-7.19 | M | Session end SHALL persist new discoveries เป็น Tier 1 memories | A |
| FR-7.20 | S | Session end SHOULD remove stale memories (อนิจจัง) | A |

### หมวด 8 — สัมมาสมาธิ (Right Concentration) : Focus & Session Stability

| ID | P | Requirement | V |
|---|---|---|---|
| FR-8.1 | M | `config.manifest.json` SHALL declare required placeholders ทั้งหมด | T |
| FR-8.2 | M | Validation SHALL fail ถ้า required placeholder ไม่ถูกแทนที่ | T |
| FR-8.3 | M | Default config SHALL include: `agentId`, `model`, `fallbackModel`, `maxConcurrentTasks`, `worktreeIsolation`, `worktreeBase`, `memory.*` | I |
| FR-8.4 | M | `scripts/incarnate.sh <agent-name>` SHALL clone template เป็น agent ใหม่ | T |
| FR-8.5 | M | `scripts/check-agent-neutrality.sh` SHALL ตรวจ structural conformance | T |
| FR-8.6 | M | Scripts SHALL exit non-zero on failure | T |
| FR-8.7 | S | Scripts SHOULD print human-readable summary | D |
| FR-8.8 | S | `.claude/commands/new-agent` SHOULD invoke `incarnate.sh` จากใน Claude Code | T |

---

## 2. Cross-Cutting Principles

### 2.1 โยนิโสมนสิการ — Verify Before Act
ทุก FR ที่อ่านจาก memory ต้องผ่านการ verify กับ current state ก่อนใช้ — ครอบคลุม FR-7.7, FR-7.17

### 2.2 ไตรลักษณ์ — State Philosophy

| ลักษณะ | ผลกระทบต่อ FR |
|---|---|
| อนิจจัง | FR-7.10 (prune), FR-4.8 (cleanup), FR-7.2 (timestamps) |
| ทุกขัง | FR-4.8 (no stale branch), FR-7.20 (no stale memory) |
| อนัตตา | FR-4.2 (no shared dir), FR-4.3 (no stash), FR-4.8 (release) |

### 2.3 มัตตัญญุตา — Scope Discipline
- ไม่ทำงานนอก scope ของ task (FR-4.5)
- ไม่เก็บข้อมูลที่ derive ได้ (FR-7.9)
- ไม่ debug ผลกระทบนอกขอบเขต (FR-1.3)

---

## 3. Non-Functional Requirements

### 3.1 Portability (สมานัตตตา)

| ID | Requirement |
|---|---|
| NFR-1.1 | ระบบ SHALL ทำงานบน Linux, macOS, Windows (ผ่าน WSL/Git Bash) |
| NFR-1.2 | LLM-agnostic — พฤติกรรมเหมือนกันทุก backend ตาม FR-5.1 |

### 3.2 Performance (มัตตัญญุตา — รู้ประมาณ)

| ID | Requirement |
|---|---|
| NFR-2.1 | `incarnate.sh` SHALL complete ≤ 5 วินาทีบน developer laptop |
| NFR-2.2 | `check-agent-neutrality.sh` SHALL complete ≤ 2 วินาที |
| NFR-2.3 | Session start memory load SHALL complete ≤ 1 วินาทีเมื่อ MEMORY.md ≤ 200 บรรทัด |

### 3.3 Reliability (สัมมาสมาธิ — ตั้งมั่น)

| ID | Requirement |
|---|---|
| NFR-3.1 | Scripts SHALL idempotent เว้นที่ explicit destructive |
| NFR-3.2 | Failed incarnation SHALL ไม่ทิ้ง partial directory |
| NFR-3.3 | Worktree creation failure SHALL rollback คลีน |

### 3.4 Maintainability (สีลสามัญญตา)

| ID | Requirement |
|---|---|
| NFR-4.1 | ทุกไฟล์ instruction ข้าม backend SHALL เป็น symlink (no duplication) |
| NFR-4.2 | Conventions SHALL document ใน `conventions.md` |
| NFR-4.3 | Requirements ทุกข้อ SHALL traceable ผ่าน matrix ใน Appendix C |

### 3.5 Security (สัมมาอาชีวะ)

| ID | Requirement |
|---|---|
| NFR-5.1 | ห้าม commit secrets ใน memory file |
| NFR-5.2 | Trust model SHALL document และ enforceable ผ่าน hooks |
| NFR-5.3 | Hooks SHALL deny `rm -rf` ของ repository root |

### 3.6 Usability (สังคหวัตถุ 4)

| ID | Requirement |
|---|---|
| NFR-6.1 | Agent ใหม่ SHALL ready to commit ภายใน 30 นาที |
| NFR-6.2 | Error message SHALL ระบุ placeholder/symlink ที่เป็นปัญหาโดยชื่อ |

### 3.7 Scalability

| ID | Requirement |
|---|---|
| NFR-7.1 | ระบบ SHALL รองรับ `maxConcurrentTasks` ≥ 3 default |
| NFR-7.2 | Worktree isolation SHALL allow N concurrent tasks bounded only by disk/CPU |

### 3.8 Auditability (วิมังสา)

| ID | Requirement |
|---|---|
| NFR-8.1 | `task-log.jsonl` SHALL append-only audit trail |
| NFR-8.2 | Memory file SHALL มี `created` และ `updated` ISO timestamps |

---

## 4. External Interfaces

### 4.1 LLM CLI Interfaces

| Backend | Entry File | Mechanism |
|---|---|---|
| Claude | `CLAUDE.md` → `AGENTS.md` | Symlink |
| Antigravity | `AGY.md` → `AGENTS.md` | Symlink |
| Codex | `CODEX.md` → `AGENTS.md` | Symlink |
| Kimi | `KIMI.md` → `AGENTS.md` | Symlink |
| Generic | `AGENTS.md` | Direct |

### 4.2 Deep Memory Backend Interface (Tier 2)

```
{{deepMemoryCmd}} wake-up                     # emit session-start context to stdout
{{deepMemoryCmd}} search "<query>"            # emit ranked results to stdout
{{deepMemoryCmd}} mine <path> --mode <mode>   # persist learnings
```

Exit codes: 0 success, non-zero failure

### 4.3 Project Submodule Interface
Project repos mounted via `git submodule add <url> projects/<name>`

---

## 5. Data Schemas

### 5.1 Memory File Schema

```yaml
---
name: <descriptive name>             # required
description: <one-line hook>         # required
type: user|feedback|project|reference  # required
created: <ISO 8601>                  # required
updated: <ISO 8601>                  # required
---

<content body>

**Why:** <motivation>                # required for feedback, project
**How to apply:** <when/where>       # required for feedback, project
```

### 5.2 Task Log Record Schema

```json
{
  "taskId":        "string",         // required, unique
  "moduleName":    "string",         // required
  "branchName":    "string",         // required, FR-4.7 pattern
  "worktreePath":  "string",         // required, absolute
  "status":        "string",         // required, FR-2.4 enum
  "startedAt":     "ISO-8601",       // required
  "lastAction":    "string",         // required
  "completedAt":   "ISO-8601",       // optional
  "blockedReason": "string"          // optional, required if status=blocked
}
```

### 5.3 Config Manifest Schema

```json
{
  "agentId":            "agent-{{name}}",
  "model":              "{{primaryModel}}",
  "fallbackModel":      "{{fallbackModel}}",
  "maxConcurrentTasks": 3,
  "worktreeIsolation":  true,
  "worktreeBase":       "/tmp",
  "memory": {
    "fileBasedPath":      "{{memoryPath}}",
    "deepMemoryCmd":      "{{deepMemoryCmd}}",
    "wakeUpOnStart":      true,
    "maxMemoryIndexLines": 200
  }
}
```

### 5.4 Required Placeholders

| Placeholder | Type | Required | Resolved By |
|---|---|---|---|
| `{{name}}` | string | yes | `incarnate.sh` argument |
| `{{agentId}}` | string | yes | derived from `{{name}}` |
| `{{primaryModel}}` | string | yes | user edit |
| `{{fallbackModel}}` | string | no | user edit |
| `{{memoryPath}}` | path | yes | default `memories/` |
| `{{deepMemoryCmd}}` | string | no | user edit |
| `{{lintCmd}}` | string | yes | user edit |
| `{{testCmd}}` | string | yes | user edit |
| `{{buildCmd}}` | string | yes | user edit |
| `{{formatCmd}}` | string | yes | user edit |
| `{{worktreeBase}}` | path | no | default `/tmp` |
| `{{taskId}}` | string | runtime | task assignment |

---

## 6. Verification & Validation

### 6.1 Automated Checks (`check-agent-neutrality.sh`)
1. `AGENTS.md` มีอยู่และเป็น regular file
2. `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md` เป็น symlink ชี้ `AGENTS.md`
3. Required placeholders ถูกแทนหมด
4. `config.manifest.json` parse JSON ได้
5. `task-log.jsonl` เป็น valid JSONL
6. Memory file ทุกไฟล์มี valid front-matter และ required `type`
7. `MEMORY.md` ≤ 200 บรรทัด
8. AGENTS.md ไม่มี backend-specific lock-in

### 6.2 Acceptance Criteria (per Magga)

| Magga | Acceptance |
|---|---|
| สัมมาทิฏฐิ | Persona ผ่าน checklist; วิสัยทัศน์ชัด |
| สัมมาสังกัปปะ | Task ทั้งหมดมี `taskId` และ goal |
| สัมมาวาจา | 2 agents complete consensus exchange |
| สัมมากัมมันตะ | 3 concurrent agents, 0 collisions |
| สัมมาอาชีวะ | 6 backends, equivalent behavior |
| สัมมาวายามะ | 100% gates pass on merged PRs |
| สัมมาสติ | Prior-decision test ≥ 95% |
| สัมมาสมาธิ | Incarnation ≤ 5s ; check ≤ 2s |

---

## Appendix A — Branch Naming Grammar

```
branch       ::= category "/" identifier
category     ::= "feature" | "fix" | "refactor" | "release" | "hotfix"
               | "agent/" agent-id
agent-id     ::= [a-z0-9-]+
identifier   ::= task-id | version
task-id      ::= [A-Z]+ "-" [0-9]+
version      ::= "v" [0-9]+ "." [0-9]+ "." [0-9]+
```

## Appendix B — Worktree State Machine

```
[*] --> Create        : task assigned (สัมมาสังกัปปะ)
Create --> Work       : worktree ready
Work --> Verify       : code complete
Verify --> Fix        : gates fail (สัมมาวายามะ)
Fix --> Verify        : retry
Verify --> Land       : gates pass
Land --> Cleanup      : merged
Cleanup --> [*]       : (อนัตตา — release)
```

## Appendix C — Traceability Matrix

| PRD Section | SRS Requirements |
|---|---|
| Part 4 — Magga / สัมมาทิฏฐิ | FR-1.1–1.5 |
| Part 4 — Magga / สัมมาสังกัปปะ | FR-2.1–2.6 |
| Part 4 — Magga / สัมมาวาจา | FR-3.1–3.6 |
| Part 4 — Magga / สัมมากัมมันตะ | FR-4.1–4.8 |
| Part 4 — Magga / สัมมาอาชีวะ | FR-5.1–5.7 |
| Part 4 — Magga / สัมมาวายามะ | FR-6.1–6.7 |
| Part 4 — Magga / สัมมาสติ | FR-7.1–7.20 |
| Part 4 — Magga / สัมมาสมาธิ | FR-8.1–8.8 |
| Part 7 — Iddhipāda | NFR-1 ถึง NFR-8 |
| Part 8 — Tilakkhaṇa | Cross-cutting §2.2 |
| ภาค 9 Out of scope | Cross-cutting §2.3 (มัตตัญญุตา) |

---

## ภาคผนวก — บันทึกการเปลี่ยนแปลง (Changelog)

### v2.0 (2026-05-22)
- **แก้ไข forced metaphors:** เปลี่ยน `อจินไตย` → `มัตตัญญุตา` ในจุดที่หมายถึง "รู้ประมาณของขอบเขตงาน" (อจินไตย คงไว้เฉพาะ 4 กรณีต้นฉบับ — Buddha-visaya, Jhāna-visaya, Kamma-vipāka, Loka-cintā)
- **เพิ่มเอกสารคู่ขนาน:**
  - `FAILURE-MODES.md` (ปฏิจจสมุปบาท) — failure analysis
  - `LIFECYCLE.md` (ภาวนา 4 + อริยทรัพย์ 7) — agent lifecycle
  - `OBSERVABILITY.md` (สติปัฏฐาน 4 + กรรม 3) — monitoring + audit
  - `COORDINATION-PROTOCOL.md` (กัลยาณมิตร 7 + สาราณียธรรม 6) — inter-agent
  - `FLEET-GOVERNANCE.md` (อปริหานิยธรรม 7) — org-level governance
  - `SELF-IMPROVEMENT.md` (ปัญญา 3) — learning loop
  - `THREAT-MODEL.md` (ตัณหา 3 + สีล 5) — security
  - `ANTIPATTERNS.md` (มิจฉาตามมรรค 8) — wrong-path catalog
  - `GLOSSARY.md` — Pali + technical terms reference
  - `OVERVIEW.md` — entry-point document
- **ขยาย PHILOSOPHY.md** ให้ครอบคลุม 22 หลักธรรม (เดิม 13) ใน 6 หมวด

### v1.0 (2026-05-22)
- เอกสารเริ่มต้น 4 ฉบับ (PHILOSOPHY, PRD, SRS, ARCHITECTURE) แบบ bilingual
