---
title: bwoc-harness — รันไทม์ Agent แบบ Self-Hosted
aliases: [harness, ollama-harness, agentic-harness]
tags: [harness, runtime, safety, tools, ollama, self-host]
status: v1 (P1–P5 สมบูรณ์; ดูข้อจำกัด)
canonical-source: crates/bwoc-harness/src/
parent: ภาษาไทย
nav_order: 8
---

# bwoc-harness — รันไทม์ Agent แบบ Self-Hosted

> [!abstract]
> `bwoc-harness` คือ crate ใหม่ที่ทำให้ BWOC กลายเป็น **OpenAI-compatible model-API client และ agentic loop runtime** เพื่อรองรับ LLM backend แบบ self-hosted และเป็นกลางต่อ provider (Ollama เป็นตัวแรก) dep หนัก (tokio, reqwest, keyring) ถูกกักตัวไว้ใน crate นี้เท่านั้น — `bwoc-cli`, `bwoc-agent`, และ `bwoc-core` ยังคงเบา ดังนั้น path เริ่มต้น `bwoc` จะไม่ดึง runtime เลยหากผู้ใช้ไม่ต้องการ

ดูเพิ่ม: [[ARCHITECTURE.th.md]], [[PHILOSOPHY.th.md]], [[GLOSSARY.th.md]]

---

## คืออะไรและทำไมต้องมี

ก่อนมี crate นี้ `bwoc spawn` ทำงานโดย exec vendor agentic CLI (`claude`, `agy`, `codex`, `kimi`) โมเดลนั้นมีช่องว่างพื้นฐาน: **Ollama ไม่มี agentic CLI** การรัน BWOC agent กับ self-hosted model จึงต้องให้ framework จัดการ agentic loop เอง

`bwoc-harness` ปิดช่องว่างนั้นด้วยพันธสัญญาสามข้อ:

1. **ความเป็นกลางต่อ provider (สมานัตตตา)** — harness พูดภาษา OpenAI-compatible `/v1/chat/completions` (tools + SSE streaming) ไม่ใช่ Ollama-native `/api/chat` endpoint ใดที่พูดภาษานั้น (Ollama, vLLM, LM Studio, llama.cpp server หรือ OpenAI เอง) ก็ทำงานได้โดยไม่ต้องเปลี่ยน code

2. **การกักตัว dep** — tokio, reqwest, keyring, และ futures-util อยู่เฉพาะใน `crates/bwoc-harness` ผู้ใช้ที่ไม่รัน self-hosted backend ไม่ต้อง compile หรือ link น้ำหนักนั้น สัญญา "orchestrator ไม่มี dep" ยังคงใช้ได้กับ path เริ่มต้น

3. **Safety ก่อน (ศีล 5 + ตัณหา 3)** — harness บังคับใช้ชั้น guardrail ที่ override ไม่ได้ก่อนที่ tool ใดจะ execute การ deny ถูกส่งกลับไปให้ model เป็น tool result แทนที่จะ panic loop

---

## สถาปัตยกรรม

```
bwoc spawn --backend ollama
  │
  └─▶  bwoc-harness binary
         │
         ├─ โหลด: AGENTS.md (system prompt) + persona + manifest + memory
         ├─ เชื่อมต่อ: OpenAI-compat endpoint (ค่าเริ่มต้น: http://localhost:11434/v1)
         │
         └─ agentic loop (อิทธิบาท 4 — เครื่องยนต์ของงาน)
              ┌──────────────────────────────────────────────────────────┐
              │  สร้าง messages (system + history + tool schemas)          │
              │  → POST /v1/chat/completions stream=true tools=[…]         │
              │  → สะสม SSE token deltas + tool_calls                    │
              │  → สำหรับแต่ละ tool_call:                                │
              │      GUARDRAILS → PERMISSION → SANDBOX → execute           │
              │  → ต่อท้าย assistant(tool_calls) + tool results           │
              │  → วนซ้ำ                                                  │
              │                                                           │
              │  หยุดเมื่อ: ไม่มี tool_calls (คำตอบสุดท้าย)              │
              │           | ถึง max_iterations                            │
              │           | cancel ภายนอก                                 │
              │           | context เต็ม → compact history                │
              └──────────────────────────────────────────────────────────┘
              │
              └─ ส่ง telemetry → session-metrics.jsonl
                   (ในโหมด task) → bwoc task complete
```

### โครงสร้าง crate

```
crates/bwoc-harness/src/
├── main.rs             — entry point: โหลด context, เริ่ม loop
├── provider/
│   ├── mod.rs          — ProviderClient trait + types
│   ├── client.rs       — OpenAI-compat HTTP client (reqwest + SSE)
│   └── types.rs        — ChatMessage, ToolCall, ChatCompletion, …
├── agent_loop.rs       — turn loop, retry, fallback, compaction, telemetry
├── tools/
│   ├── mod.rs          — ToolContext, tool trait
│   ├── registry.rs     — ToolRegistry + dispatch
│   ├── impls.rs        — read_file, write_file, edit_file, list_dir, grep, …
│   ├── extra_tools.rs  — run_gates, bwoc_task, bwoc_send, memory_read/write
│   └── auth.rs         — CredentialBroker (P3)
├── policy/
│   ├── mod.rs          — run_pipeline: guardrails → permission → sandbox
│   ├── guardrails.rs   — กฎ safety แบบ hard (override ไม่ได้)
│   └── permission.rs   — per-tool/per-pattern allow | ask | deny
├── sandbox.rs          — จำกัด path ใน fs, scrub env, scan arg, OsSandbox trait
├── telemetry.rs        — metrics per-turn → session-metrics.jsonl (P3)
├── queue.rs            — async bounded cancellable task queue (P3)
└── eval/
    └── mod.rs          — offline fixture runner + rubric scorer (P4)
```

---

## 8 Component หลัก

| Component | หน้าที่ | กรอบ BWOC | Phase |
|---|---|---|---|
| **Safety guardrails** | กฎ hard ที่รันก่อน permission และ override ไม่ได้ บล็อก `rm -rf` repo root, เขียน secret, ปลอมตัวตน, bypass gate (`--no-verify`, `--force`), privilege escalation (`sudo`/`su`/`doas`) | ศีล 5 + ตัณหา 3 | P2 |
| **Permission system** | `allow \| ask \| deny` per-tool / per-pattern จาก `.bwoc/harness-policy.toml` `ask` ในโหมด non-TTY / autonomous fall back ไปที่ `default_mode` (fail-safe: `deny`) การ deny ถูกส่งกลับเป็น tool result | ตัณหา 3 (ดักตัณหา) | P2 |
| **Sandbox** | จำกัด tool effect ไว้ใน worktree ของ agent reject path-escape (ตรวจ symlink) cwd ของ `run_command` ถูกล็อกไว้ที่ worktree root env scrub ลบ var ที่เป็น credential arg scan บล็อก `curl|sh`, privilege escalation, force-push OS-level confinement คือ **stub trait** ใน v1 (ดูข้อจำกัด) | ศีล 5 + อนัตตา (worktree isolation) | P2 |
| **Tool authentication** | OS keyring credential broker tool ประกาศ credential ที่ต้องการ (`CredentialRequest`) broker inject scoped vars เข้า child-process env ตอน exec เท่านั้น — ไม่เคยอยู่ใน prompt, ไม่เคย log, ไม่เคยอยู่ใน telemetry | ศีล (อทินนาทาน) + กัลยาณมิตร | P3 |
| **Task queue** | async, bounded, cancellable queue integrate กับ `bwoc-core::team` (Saṅgha shared task list) หนึ่ง task in-flight ต่อ worktree; rollback เป็น `pending` ถ้า queue reject หลัง claim | สังฆะ + ปธาน 4 | P3 |
| **Streaming** | SSE token stream จาก model สะสม `content` และ `tool_calls` fragment เป็น `ChatMessage` เดียว เชื่อมใน `agent_loop.rs` ด้วย `stream=true` | สัมมาวาจา (speech โปร่งใส) | P1 |
| **Telemetry** | `TurnMetrics` per-turn (tokens in/out, latency, tool-call count, denial count, gate pass/fail, context tokens) append ไปที่ `session-metrics.jsonl` per session additive กับ schema `AGENTS.md §8b` — reader เดิมที่ไม่รู้จัก key `"harness"` ก็ ignore ได้อย่างปลอดภัย OTEL export optional ผ่าน `--features otel` | สติปัฏฐาน 4 | P3 |
| **Eval framework** | offline fixture runner `task.toml` (prompt + rubric) + `seed/` (initial repo state) + `expected/` (expected outputs) rubric: `file_contains`, `file_matches` (exact bytes), `gates_must_pass` ทุก test ใช้ mock provider — ไม่ต้อง live model หรือ network ใน CI ป้อน retrospective triggers ของ Paññā 3 ใน `session-metrics` | ปัญญา 3 + ภาวนา 4 | P4 |

---

## Safety Pipeline

ทุก tool call ผ่านสามชั้นเรียงกัน **ลำดับตายตัวและเปลี่ยนไม่ได้**

```
GUARDRAILS  (ศีล 5 + ตัณหา 3 — hard, override ไม่ได้)
  ↓ ผ่าน
PERMISSION  (policy per-tool / per-pattern จาก harness-policy.toml)
  ↓ ผ่าน
SANDBOX     (worktree confinement + env scrub + arg scan)
  ↓ ผ่าน
  execute
```

การบล็อกที่ชั้นใดก็ตามจะส่งคืนเหตุผลเป็น tool result message ให้ model ปรับตัว **loop ไม่ panic หรือหยุดเมื่อถูก deny**

> [!warning]
> Pipeline **fail-safe โดยค่าเริ่มต้น** ถ้าไม่มี policy file `default_mode = "deny"` agent ที่ไม่มี `.bwoc/harness-policy.toml` อ่านไฟล์ได้แต่เขียนหรือรัน command ไม่ได้ เว้นแต่ policy จะระบุชัดเจน

### กฎ Guardrail

แต่ละกฎ map กับศีลหรือรากตัณหา:

| Rule ID | trigger เมื่อ | ศีล |
|---|---|---|
| `sila_panatatipata` | `rm -rf` ที่ `/` หรือ worktree root; `git clean -f*` | ปาณาติบาต (ไม่ทำลาย) |
| `sila_adinnadana` | เขียน PEM key, GitHub PAT, AWS key, `password=`, `token=` ฯลฯ ลงไฟล์ที่ tracked | อทินนาทาน (ไม่ขโมย) |
| `sila_musavada` | field `from`/`sender` มีคำ `spoof`/`impersonate`/`fake` ใน `bwoc_send` หรือ `bwoc_task` | มุสาวาท (ไม่โกหก) |
| `sila_surameraya` | `--no-verify` บนคำสั่งใด; `git push --force`/`-f`/`--force-with-lease` | สุราเมรยะ (ไม่ประมาท) |
| `bhava_tanha_escalation` | `sudo`, `su`, `doas` เป็น binary ของคำสั่ง | ภวตัณหา (ขยาย privilege) |

---

## ชุด Tool

Tool ทุกตัวลงทะเบียนใน `tools/registry.rs` และถูก dispatch ผ่าน safety pipeline ก่อน execute ทุก tool เคารพ `ToolContext::workdir` ในการ resolve path

| Tool | คำอธิบาย |
|---|---|
| `read_file` | อ่านไฟล์จาก worktree |
| `write_file` | เขียน / overwrite ไฟล์ |
| `edit_file` | แทนที่ string แบบเจาะจง (`old_string` → `new_string`) |
| `list_dir` | แสดงเนื้อหา directory |
| `grep` | ค้นหาใน file contents ด้วย regex pattern |
| `run_command` | รัน shell command (sandboxed: cwd ล็อก, env scrub, arg scan) |
| `git` | git operation แบบ structured (`subcommand` + `args` array) |
| `run_gates` | รัน lint / fmt / test / build gates จาก manifest |
| `bwoc_task` | claim / complete task ใน Saṅgha team list |
| `bwoc_send` | ส่ง message ไป agent อื่นผ่าน `interconnect/` |
| `memory_read` | อ่านจาก `memories/` ของ agent |
| `memory_write` | เขียนไปที่ `memories/` ของ agent |

---

## Schema ของ `.bwoc/harness-policy.toml`

วางไฟล์นี้ที่ workspace root ของ agent harness โหลดตอน startup ถ้าไม่มีไฟล์ `default_mode = "deny"` ใช้งาน (fail-safe)

```toml
# mode default สำหรับ tool หรือ pattern ที่ไม่ได้ระบุไว้
# ค่าที่ใช้ได้: "allow" | "ask" | "deny"
# ค่าเริ่มต้นถ้าไม่มีไฟล์: "deny" (fail-safe)
default_mode = "allow"

# override per-tool คีย์ = ชื่อ tool ที่แน่นอน
[tools]
read_file   = "allow"
list_dir    = "allow"
write_file  = "ask"     # prompt operator บน TTY; deny ใน non-TTY/autonomous
run_command = "deny"

# pattern rules — match กับ JSON arguments string ครบ
# กฎประเมินตามลำดับ; match แรกชนะ
[[patterns]]
pattern = "git push"
mode    = "deny"
reason  = "git push ต้องให้มนุษย์ตรวจก่อน"

[[patterns]]
pattern = "cargo test"
mode    = "allow"
```

> [!note]
> mode `ask` ใน context non-TTY หรือ autonomous (CI, background agent ที่ถูก spawn โดย `bwoc spawn`) fall back ไปที่ `default_mode` ซึ่งเริ่มต้นเป็น `deny` นี่คือพฤติกรรม fail-safe ที่ตั้งใจไว้

---

## วิธีใช้งาน Backend

### Spawn Ollama agent

```bash
# ตรวจให้แน่ใจว่า Ollama-compatible model กำลังรันอยู่
# จากนั้น spawn agent ด้วย ollama backend:
bwoc spawn --backend ollama --path agents/my-agent
```

`bwoc spawn` ตรวจจับ backend `ollama` และ launch binary `bwoc-harness` แทน vendor CLI harness จะ:

1. อ่าน `AGENTS.md` (ผ่าน symlink `OLLAMA.md → AGENTS.md`) เป็น system prompt
2. อ่าน `config.manifest.json` เพื่อชื่อ model และ `context_limit`
3. เชื่อมต่อ `http://localhost:11434/v1` (หรือ `$OLLAMA_BASE_URL` ถ้าตั้งไว้)
4. ตรวจสอบว่า model มีอยู่จริงบน Ollama instance ก่อน turn แรก
5. รัน agentic loop

### Spawn agent แบบ OpenAI-compatible

```bash
# endpoint ใด ๆ ที่พูด OpenAI-compatible (vLLM, LM Studio, llama.cpp server, remote):
bwoc spawn --backend openai-compatible --path agents/my-agent
```

ตั้งค่า `"baseUrl"` ใน `config.manifest.json` ของ agent ให้ชี้ไปยัง endpoint — **จำเป็น** สำหรับ `openai-compatible` (`ollama` ใช้ค่า default `http://localhost:11434/v1` และถือว่า `baseUrl` เป็น optional) `bwoc spawn` ส่งค่านี้ให้ harness ผ่าน `--endpoint` ลงทะเบียน backend ด้วย symlink `OPENAI.md → AGENTS.md` โดย provider client ไม่เปลี่ยน (ใช้ path `/v1/chat/completions` แบบ OpenAI-compatible เดิม)

### การบังคับใช้ vetted-model

`--vetted-mode off | warn | enforce` (default `warn`) ควบคุมว่า loop จะจัดการ model ที่**ไม่**อยู่ใน allowlist `vetted_models` อย่างไร:

- `off` — ไม่ตรวจ
- `warn` — log warning แล้วทำต่อ (พฤติกรรมเดิม, backward-compatible default)
- `enforce` — ปฏิเสธไม่รัน **primary** model ที่ไม่ vetted (error ก่อน turn แรก)

allowlist `vetted_models` ว่าง = ไม่จำกัด ไม่ว่า mode ใด

### เพิ่ม symlink OLLAMA.md ใน agent ที่มีอยู่

```bash
cd agents/my-agent
ln -s AGENTS.md OLLAMA.md
```

ไม่ต้องเปลี่ยนอะไรอีก harness อ่าน `AGENTS.md` เดียวกับที่ backend อื่นอ่าน

### ตั้งค่า model ใน `config.manifest.json`

```json
{
  "primaryModel": "gemma4",
  "fallbackModel": "qwen2.5-coder:7b"
}
```

`fallbackModel` จะถูกลองใช้ถ้า primary model สร้าง tool call ผิดรูปแบบซ้ำๆ เกินสองครั้ง (ส่วน history compaction และ context limit ต่อ model ตั้งบน `LoopConfig` ของ harness ไม่ใช่ field ใน `config.manifest.json`)

สำหรับ endpoint แบบ OpenAI-compatible ที่ serve GPT-5.5 ให้ใช้ model ชัดเจน
หรือ pool สำหรับเลือกตอน runtime:

```json
{
  "backend": "openai-compatible",
  "baseUrl": "https://api.openai.com/v1",
  "primaryModel": "auto",
  "autoModels": ["gpt-5.5", "gpt-5.5-pro", "gpt-5.4", "gpt-5.4-mini"],
  "reasoningEffort": "medium"
}
```

`primaryModel: "auto"` รักษาความเป็นกลางต่อ backend ของ BWOC และให้ harness
เลือกจาก model ที่ provider ปัจจุบัน serve จริง ใส่ model capability สูงสุดไว้
ก่อน และ fallback ที่ถูกกว่า/latency ต่ำกว่าไว้ทีหลัง; resolver ใช้ลำดับนี้
เป็นแกน cost หลังตรวจ availability และ context-fit แล้ว

OpenAI แนะนำ GPT-5.5 สำหรับงาน coding และ agent workflow ที่ต้อง reasoning มาก
โดยใช้ `medium` reasoning effort เป็นจุดเริ่มต้นที่สมดุล และลอง effort ต่ำกว่า
ก่อนปิด reasoning ทั้งหมด `reasoningEffort` เป็น optional; ถ้าตั้งค่าไว้
harness จะส่งเป็น `reasoning_effort` ใน request แบบ OpenAI-compatible completion
ปัจจุบัน BWOC harness ยังพูดผ่าน surface OpenAI-compatible `/v1/chat/completions`
เพื่อให้รันได้ทั้ง Ollama และ provider compatible อื่นๆ adapter แบบ native
Responses API คือขั้นถัดไปสำหรับ control reasoning ของ GPT-5.5 แบบเต็ม ระหว่างนี้
ให้ `AGENTS.md` เน้น outcome, ลด prompt scaffolding แบบบอกขั้นตอนละเอียดเกินจำเป็น,
และระบุ completion criteria ให้ชัดเจน

---

## การออกแบบ Dep-Quarantine

> [!tip]
> นี่คือการรับประกันเชิงโครงสร้างที่ทำให้ `bwoc-harness` เป็น optional ไม่ใช่บังคับ

```
crates/bwoc-core    — เบา: serde, serde_json, toml, thiserror เท่านั้น
crates/bwoc-cli     — เบา: clap + bwoc-core + ratatui; ไม่มี tokio, ไม่มี HTTP
crates/bwoc-agent   — เบา: bwoc-core + fluent-bundle; ไม่มี tokio
crates/bwoc-harness — หนัก: tokio, reqwest, futures-util, keyring, async-trait
```

`bwoc-harness` depend บน `bwoc-core` (data types เบา) แต่ `bwoc-core`, `bwoc-cli`, และ `bwoc-agent` **ไม่** depend บน `bwoc-harness` ผู้ใช้ที่รัน `bwoc spawn --backend claude` เท่านั้นไม่ต้อง compile หรือ link harness

นี่รักษาตัวตน "zero-dep orchestrator" ของ VISION สำหรับ path เริ่มต้น ในขณะที่เปิดใช้ self-hosted production ผ่าน crate opt-in

---

## ผลการ Validate จริง (2026-05-23)

harness ถูก validate end-to-end กับ Ollama จริงก่อนเขียนเอกสาร

**Model: `gemma4:latest` (8B)**

- Turn 1: model เรียก `read_file` (read-before-edit เกิดขึ้นเอง ไม่ได้สั่ง)
- Turn 2: model เรียก `write_file` พร้อม Python code ถูกต้องและ Thai Unicode ถูกต้อง (`สวัสดี, Pi`)
- Turn 3: model ให้คำตอบสุดท้าย
- รัน `greet("Pi")` จาก output คืนค่า `สวัสดี, Pi` ไฟล์ถูกต้องและรันได้

**ไม่มี policy file (fail-safe deny):**

- การเขียนถูก deny ถูกต้อง
- เหตุผล deny ถูกส่งกลับให้ model เป็น tool result
- model ปรับตัวและให้คำตอบสุดท้ายอธิบายว่าทำงานไม่ได้เพราะอะไร

**มี `.bwoc/harness-policy.toml` แบบ permissive (`default_mode = "allow"`):**

- การเขียนสำเร็จ end-to-end

**Model: `llama3.2:3b`**

- mechanism รันถูกต้อง (tool calls ถูก dispatch) แต่ model บิด unicode output ข้าม read และทำลายโครงสร้างไฟล์
- ยืนยัน design ของ vetted-model gate: model เล็กไม่ควรใช้แก้ code โดยไม่ validate

---

## ยังไม่มี / ข้อจำกัด v1

> [!warning]
> ระบุตรงๆ ว่าอะไรพร้อม production และอะไรยังไม่พร้อม

| ความสามารถ | สถานะ |
|---|---|
| **OS-level sandbox** (macOS `sandbox-exec`, Linux landlock/seccomp) | **Stub** trait `OsSandbox` มีอยู่และ pluggable แต่ implement เดียวคือ `NoopOsSandbox` worktree+allowlist confinement active; OS-level syscall isolation ยังไม่มี |
| **Streaming** | เชื่อมและทำงานได้ (SSE delta accumulation test ผ่าน) token count ใน `usage` ไม่มีบน streaming path (provider ไม่ return `usage` ใน SSE delta) |
| **Vetted-model list** | เล็ก ขณะนี้รู้ว่า `gemma4` และ `qwen2.5-coder:7b` ใช้ tool calling ได้ดี model ที่ไม่ได้ vetted จะ warning แต่ไม่ hard-block |
| **Context compaction** | Active (truncate-with-marker strategy) LLM-summarise คือ upgrade path ที่ชัดเจนแต่ยังไม่ implement ใน v1 |
| **Tool authentication broker** | Implement แล้ว (P3) แต่ยังไม่ wire เข้าทุก tool โดยค่าเริ่มต้น tool ที่ต้องการ OS keyring credential ต้อง declare `CredentialRequest` เอง |
| **Concurrent tool execution** | Sequential ใน P1/P2 parallel tool dispatch เป็น P3 item |
| **Identity spoofing detection** | Conservative: fire เฉพาะเมื่อ field `from`/`sender` มีคำว่า `spoof`, `impersonate`, หรือ `fake` ตรงๆ ระบบ agent-identity proof ที่แท้จริงเป็น v2 item |
| **Platform support** | **Unix-first ใน v1** (macOS + Linux) crate *build* บน Windows ได้ แต่ tool layer shell out ไปยัง POSIX shell และ test ของ sandbox / `run_command` สมมติ Unix command (`pwd`, `rm -rf`, …) harness จึง **ไม่ถูกเทสต์บน Windows** — CI exclude `bwoc-harness` ใน Windows job การรองรับ Windows เป็น follow-up; ส่วนที่เหลือของ toolkit ยัง cross-platform เต็มที่ |

---

## การ Map กับกรอบ BWOC

การออกแบบ map แต่ละ component กับกรอบพุทธใน [[PHILOSOPHY.th.md]]:

| Component | กรอบ | เหตุผล |
|---|---|---|
| Safety guardrails | ศีล 5 | ศีลห้าประการกลายเป็น constraint ของ code ที่เปลี่ยนไม่ได้ |
| Permission system | ตัณหา 3 | permission ดักสามรากตัณหา (กาม, ภว, วิภว) ก่อนกลายเป็น tool call |
| Sandbox confinement | อนัตตา + ศีลข้อ 1 | ไม่มีการกระทำที่อยู่เกิน worktree; worktree คือขอบเขต conditioned ของ agent |
| Denial-as-tool-result | พรหมวิหาร 4 (กรุณา) | การบอกเหตุผลให้ model ปรับตัวได้ แทนที่จะ silent fail |
| Agentic loop | อิทธิบาท 4 | ฉันทะ, วิริยะ, จิต, วิมังสา map กับ: ตั้งเป้า, retry effort, model call, rubric scoring |
| Telemetry | สติปัฏฐาน 4 | ฐานสี่ของสติ apply กับ operation ของ harness เอง (กาย=process, เวทนา=I/O, จิต=tool calls, ธรรม=denials/gates) |
| Eval framework | ปัญญา 3 + ภาวนา 4 | offline fixture ป้อน wisdom practices สาม (สุตมยา, จินตามยา, ภาวนามยา) และ right efforts สี่ |
| Task queue | สังฆะ + ปธาน 4 | queue integrate กับ shared task list ของ agent team (สังฆะ) และบังคับ right effort ในการ schedule |
| ความเป็นกลาง backend | สมานัตตตา | endpoint OpenAI-compatible ทุกตัวได้รับการปฏิบัติเหมือนกัน ไม่มี provider ใดได้รับการโปรดปราน |

---

## ดูเพิ่ม

- [[ARCHITECTURE.th.md]] — ตำแหน่งของ `bwoc-harness` ใน implementation stack
- [[PHILOSOPHY.th.md]] — กรอบ BWOC 22 ประการที่อ้างถึง
- [[GLOSSARY.th.md]] — ค้นหาศัพท์บาลี
- `crates/bwoc-harness/src/agent_loop.rs` — loop implementation พร้อม annotation
- `crates/bwoc-harness/src/policy/guardrails.rs` — การ implement กฎ guardrail และ test
- `notes/2026-05-23_ollama-agentic-harness-design.md` — การตัดสินใจทางสถาปัตยกรรมก่อน implement
