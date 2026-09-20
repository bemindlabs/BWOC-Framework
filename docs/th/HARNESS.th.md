---
title: bwoc-harness — รันไทม์ Agent แบบ Self-Hosted
aliases: [harness, ollama-harness, agentic-harness]
tags: [harness, runtime, safety, tools, ollama, self-host]
status: shipped (ดูข้อจำกัด)
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

## เริ่มต้นเร็ว: `bwoc` ใน repository ใดก็ได้

รัน `bwoc` โดยไม่ใส่ argument ใน terminal ที่ directory ใดก็ได้ มันจะเปิด chat TUI เป็น coding session ตรงนั้นทันที ไม่ต้องมี workspace, agent ที่ลงทะเบียน หรือ manifest ถ้าไม่ได้อยู่ใน terminal (pipe, script, CI) `bwoc` เปล่า ๆ ยังพิมพ์ banner เหมือนเดิมทุกไบต์ และ `bwoc about` ใช้พิมพ์ banner บน terminal

**Provider และ key** chat TUI ทำงานบน API key หรือ model ในเครื่อง:

| Backend | Key |
|---|---|
| `anthropic` | `bwoc auth set anthropic` หรือ `ANTHROPIC_API_KEY` |
| `openrouter` | `bwoc auth set openrouter` หรือ `OPENROUTER_API_KEY` |
| `litellm` | ไม่บังคับ: `bwoc auth set litellm` หรือ `LITELLM_API_KEY` |
| `openai-compatible` | ไม่บังคับ: `bwoc auth set openai-compatible` (ไม่มี env var เพื่อไม่ให้ key ทั่วไปถูกส่งไป endpoint ใดก็ได้) |
| `ollama` | ไม่ต้องใช้ |

**CLI แบบ subscription** เมื่อใช้ backend ที่เป็น vendor CLI (`claude`, `codex`, `agy`, `kimi`, `grok`, `copilot`) `bwoc` เปล่า ๆ จะ exec CLI ตัวนั้นใน directory ปัจจุบัน และส่ง `--model` ต่อให้ถ้ากำหนดไว้ CLI ใช้ login ของตัวเอง subscription ของ Claude Code หรือ Codex จึงไม่ต้องมี API key และ CLI รัน tool กับ permission prompt ของตัวเอง tool ของ bwoc-harness, capability gate และ trust gate จึงไม่มีผล `bwoc` จะพิมพ์ข้อความหนึ่งบรรทัดบอกเรื่องนี้ก่อนส่ง terminal ให้ CLI ค่า `endpoint`, `max_tokens` และ `max_context` ถูกข้ามสำหรับ backend กลุ่มนี้ `cli` ไม่ใช่ backend ของ session ให้ระบุชื่อ vendor CLI แทน

`bwoc auth set <provider>` อ่าน key จาก stdin (ซ่อนตัวอักษรเมื่ออยู่ใน terminal) หรือจาก `--from-env VAR` แล้วเขียน `[<provider>] api_key` ลง `~/.bwoc/secrets.toml` โดยสร้างไฟล์เป็น `0600` ถ้าไฟล์เดิมให้ group หรือ world อ่านได้ จะไม่ยอมเขียน (ให้ `chmod 600` ก่อน) และ section อื่นยังอยู่ครบ `bwoc auth status` แสดงว่า provider ไหนมี key และ key มาจากที่ใด ไม่เคยพิมพ์ key หรือความยาวของ key

**การเลือก runtime** เรียงจากลำดับความสำคัญสูงสุด:

1. flag: `bwoc --backend … --model … --endpoint …`
2. env: `BWOC_BACKEND`, `BWOC_MODEL`, `BWOC_ENDPOINT`
3. `.bwoc/config.toml` ใน directory นั้น หรือ directory แม่ขึ้นไปจนถึง git root
4. `~/.bwoc/config.toml`
5. ตรวจหาเอง: มี Anthropic key ใช้ `anthropic`; ไม่มีก็ใช้ `ollama` กับ model แรกถ้า Ollama ตอบที่ `localhost:11434`; ไม่มีทั้งคู่ `bwoc` จะพิมพ์วิธีตั้งค่า พร้อมบอก vendor CLI ที่เจอใน `PATH` แล้วจบด้วย exit `2` โดยไม่เลือก vendor CLI ให้เอง

ชั้นที่ระบุ backend ต่างจาก backend ที่ชนะ จะไม่ส่งค่าอื่นใดมาเลย model ที่เขียนไว้สำหรับ provider หนึ่งจึงไม่ถูกส่งไปอีก provider

```toml
schema_version = 3

[runtime]
backend    = "ollama"
model      = "<model>"
endpoint   = "http://localhost:11434/v1"   # ไม่บังคับ
max_tokens = 8192                          # ไม่บังคับ
max_context = 32768                        # ไม่บังคับ: context window ของ model
```

ถ้าไฟล์ไม่มี `[runtime] backend` จะใช้ `[defaults] backend` (ค่าเริ่มต้นของ fleet สำหรับ agent ใหม่) ในไฟล์เดียวกันแทน ไม่มี `schema_version` ถือเป็น legacy ถ้าเป็น revision ที่ใหม่กว่าจะถูกปฏิเสธพร้อม error ที่ระบุ `schema_version` key ที่ไม่รู้จักจะถูกข้าม

**สิ่งที่ session ใส่ใน system prompt** ตามลำดับ:

1. preamble สั้น ๆ สำหรับ coding agent ที่ฝังมาในตัว: tool ของ harness, สำรวจ → ลงมือ → ตรวจสอบ และถามก่อนทำสิ่งที่ย้อนกลับไม่ได้
2. บล็อก environment: working directory, OS/arch, วันที่ UTC และสถานะ git (branch, สะอาดหรือมีการเปลี่ยนแปลงที่ยังไม่ commit) ถ้าไม่มี `git` หรือ `git` ค้าง จะแค่ได้รายละเอียดน้อยลง
3. คำสั่งของโปรเจกต์: `AGENTS.md` (ถ้าไม่มีใช้ `CLAUDE.md`) จากทุก directory ระหว่าง git root ถึง working directory เรียงจาก root ก่อน ใกล้ที่สุดอยู่ท้าย จำกัดรวม 32 KB โดยตัด directory ที่ไกลที่สุดก่อน

บทสนทนาถูกบันทึกแยกตาม directory ไว้ใต้ `~/.bwoc/sessions/` ไม่เคยเขียนลงใน repository หนึ่ง directory มีได้หลายบทสนทนา: `bwoc` เปล่า ๆ จะเปิดต่อบทสนทนาที่เขียนล่าสุด `bwoc --new` เริ่มบทสนทนาใหม่ และ `bwoc --session <id>` (id เต็มหรือ prefix ที่ไม่ซ้ำ) เปิดต่อบทสนทนาที่ระบุ `bwoc session list` แสดงบทสนทนาของ directory นี้ (`--all` สำหรับทุก directory, `--json` สำหรับ script) `bwoc session fork [<id>]` คัดลอกบทสนทนาเป็น session ใหม่ (หลังจากนั้น `bwoc` เปล่า ๆ จะเปิดต่อที่ fork ส่วนต้นฉบับคงเดิม) และ `bwoc session rm <id>` ลบบทสนทนา บทสนทนาจาก 3.2 จะถูกย้ายเข้าโครงสร้างใหม่ครั้งแรกที่เปิด directory นั้น file tool ถูกจำกัดอยู่ใน working directory และการเขียน แก้ไฟล์ หรือรันคำสั่งจะถามก่อน (chat default policy) เว้นแต่ `.bwoc/harness-policy.toml` กำหนดไว้เป็นอย่างอื่น

**ระหว่างที่ turn กำลังทำงาน** กด `Esc` เพื่อยกเลิก harness จะรอ tool call ที่ค้างอยู่ให้จบก่อน (บทสนทนาจะได้ไม่เสียรูป) แล้วจบ turn นั้น (`Cancel` บนสาย) ถ้าไม่มี turn ทำงานอยู่ `Esc` จะไม่ทำอะไร `/model` แสดงชื่อ model ปัจจุบัน และ `/model <name>` เปลี่ยน model ที่ turn ถัดไปใช้ เมื่อ tool เขียนหรือแก้ไฟล์ใน working directory session จะแสดง unified diff ของไฟล์นั้น (ไม่เกิน 8 KB และระบุไว้เมื่อถูกตัด) เป็นการแสดงผลอย่างเดียวเพราะ model เห็นผลจาก tool อยู่แล้ว ส่วนแถบสถานะจะแสดงค่าใช้จ่ายเฉพาะเมื่อ provider รายงานมาเท่านั้น (เช่น OpenRouter) ไม่มีการประมาณราคาจากตารางในเครื่อง

**ในช่องพิมพ์** `/` เปิดเมนูคำสั่ง และ `@` เติมชื่อไฟล์ในโปรเจกต์ ใช้ `↑`/`↓` เลือก `Tab` เติมคำ `Esc` ซ่อนเมนู `/help` แสดงคำสั่งทั้งหมด `/clear` ลืมบทสนทนานี้ (ลบประวัติที่บันทึกไว้ เหมือน Forget ใน palette ของ fleet) `/mode [default|accept_edits|bypass]` แสดงหรือตั้ง permission mode (เหมือน `F2`) และ `/quit` หรือ `/exit` จบ session คำสั่งเหล่านี้ TUI จัดการเองและไม่ส่งอะไรไปให้ model `/name` ที่ไม่รู้จักจะแจ้ง error แทนการส่ง ส่วนบรรทัดที่คำแรกเป็น path เช่น `/etc/hosts` จะถูกส่งเป็นข้อความตามปกติ ตอนส่ง ทุก `@path` ที่ชี้ไปยังไฟล์ข้อความใน working directory จะแนบเนื้อหาไฟล์นั้นไปกับข้อความ (ไม่เกิน 32 KB ต่อไฟล์ และระบุไว้เมื่อถูกตัด) path ที่อยู่นอก directory หรือไฟล์ binary จะแจ้งว่าไม่ได้แนบ และ `@name` ที่ไม่ใช่ไฟล์จะคงเป็นข้อความธรรมดา

**Session ใช้เส้นทางเดียวกับ batch** การเรียก provider จะ retry เมื่อเจอ error ชั่วคราว และถ้า tool call ผิดรูปแบบซ้ำ ๆ จะย้ายไปใช้ fallback ถัดไปจาก `autoModels` server ของ `--mcp` / `--mcp-http` ถูกลงทะเบียนเหมือนใน `bwoc run` ทุก tool call ผ่าน capability gate, guardrail และ permission policy แล้วรันผ่าน turn executor และ OS sandbox ยกเว้น tool เฉพาะ chat สามตัวด้านล่าง (`webfetch`, `todo`, `subagent`) ที่รันใน process หลักหลังผ่านการตรวจชุดเดียวกัน เมื่อมีเนื้อหาที่ไม่น่าเชื่อถือ (output ของ tool หรือข้อความจาก connector) อยู่ในบทสนทนาแล้ว call ที่ถูก gate เช่น `run_command` ต้องได้ Allow อย่างชัดแจ้งแม้อยู่ใน bypass mode ส่วน session แบบ `--headless` จะปฏิเสธเหมือน batch การ compact คำนวณจาก context window ของ model: `max_context` (`--max-context`) ถ้าไม่มีใช้ค่าที่ provider รายงาน ถ้าไม่มีใช้ 200k token สำหรับ `anthropic` ไม่เช่นนั้นใช้ 8,000 token ข้อความ reasoning ที่ provider stream มาจะแสดงเป็นบรรทัดสีจางและยุบลงเมื่อคำตอบเริ่ม

**Agent session ไม่เปลี่ยน** เมื่อ workdir มี `config.manifest.json` (`bwoc chat <agent>`) prompt ยังคงเป็น `AGENTS.md` ของ agent ตามด้วย index ของ `MEMORY.md` และตอนนี้ต่อท้ายด้วยบล็อก persona และ mindsets แบบย่อ: เนื้อหา `persona/README.md` และชื่อกับย่อหน้าแรกของแต่ละ mindset จำกัด 8 KB ส่วน public workdir ของ bwoc-connect (`.bwoc/public/…`) จะไม่อ่านคำสั่งจาก directory ที่อยู่เหนือตัวเองเลย

---

## สถาปัตยกรรม

```
bwoc spawn --backend ollama
  │
  └─▶  bwoc-harness binary
         │
         ├─ โหลด: AGENTS.md (หรือ CLAUDE.md) + index MEMORY.md + Tier-2 wake-up (ถ้ามี)
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
| **Worker result envelope** | worker ของ Saṅgha เขียนผลลัพธ์แบบมีโครงสร้างลง worktree (`.bwoc/worker-result.json`: จำนวน turn, model ที่ตอบ, diff summary ของการเปลี่ยนแปลงใน working-tree, และข้อความผลลัพธ์แบบจำกัดความยาว) แทน exit code เปล่า ๆ; lead อ่านก่อน teardown แล้ว log สรุปบรรทัดเดียว เป็น best-effort — worker ที่ไม่เขียน envelope จะ degrade ไปใช้ exit code | สังฆะ + กัลยาณมิตร | P3 |
| **Peer-review gate** | เมื่อใช้ `--lead --reviewer <agent>` (หรือ field `reviewer` ของทีม) diff ของ worker ที่สำเร็จจะถูกส่งให้ reviewer agent (รัน `bwoc-harness` ใน worktree) ตรวจก่อน complete: APPROVE → complete; REJECT → คืน task เข้า queue พร้อมเก็บ worktree + feedback เป็น fail-safe — spawn/timeout/verdict อ่านไม่ได้ = reject; self-review ถูกข้าม | สังฆะ + กัลยาณมิตร | P3 |
| **Team chat broadcast** | session แบบ `--chat --team-chat <path>` ใช้ `chat.jsonl` แบบ append-only ร่วมกันของทีม: ข้อความจากเพื่อนร่วมทีมที่โพสต์ตั้งแต่ turn ก่อนถูก inject เป็น system note "Team conversation" ก่อนแต่ละ turn แล้ว append คำตอบของ agent ต่อท้ายหลัง turn เรียกใช้ผ่าน `bwoc chat <agent> --tui --team <id>` (ตรวจ membership); ข้อความเพื่อนร่วมทีมยังถูกแสดงใน TUI เป็นบรรทัด `📢` ผ่าน event `TeamMessage` เป็น opt-in — ไม่มี flag = session เดี่ยว; agent ไม่เห็นข้อความของตัวเอง echo กลับ | สังฆะ + กัลยาณมิตร | P3 |
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
| `edit_file` | แทนที่ string แบบเจาะจง (`old_string` → `new_string`); `replace_all` แทนที่ทุกจุดที่ตรงแบบ exact |
| `multi_edit` | ชุดการแทนที่แบบ `edit_file` หลายรายการบนไฟล์เดียว ตามลำดับ เขียนไฟล์ก็ต่อเมื่อทุกรายการสำเร็จ |
| `list_dir` | แสดงเนื้อหา directory |
| `grep` | ค้นหาใน file contents ด้วย regex; มี `fixed_strings`, `case_insensitive` และตัวกรองไฟล์ `glob` ข้ามไฟล์ binary และ directory ที่ซ่อนอยู่; regex ที่ไม่ถูกต้องจะค้นแบบ literal แทน |
| `glob` | หาไฟล์ด้วย glob (`*`, `**`, `?`, `[..]`, `{a,b}`) อ่านอย่างเดียว ข้าม directory ที่ซ่อนอยู่ และไม่อ่าน `.gitignore` |
| `run_command` | รัน shell command (sandboxed: cwd ล็อก, env scrub, arg scan) `timeout_secs` (ค่าเริ่มต้น 120, สูงสุด 600) kill ทั้ง process group ของคำสั่ง |
| `git` | git operation แบบ structured (`subcommand` + `args` array) |
| `run_gates` | รัน lint / fmt / test / build gates จาก manifest |
| `bwoc_task` | claim / complete task ใน Saṅgha team list |
| `bwoc_send` | ส่ง message ไป agent อื่นผ่าน `interconnect/` |
| `memory_read` | อ่านจาก `memories/` ของ agent |
| `memory_write` | เขียนไปที่ `memories/` ของ agent |
| `memory_search` | ค้นหาเชิงความหมายใน Tier 2 deep-memory store (ลงทะเบียนเฉพาะเมื่อ manifest กำหนด `deepMemoryCmd`; อ่านอย่างเดียว) |

session แบบ `--chat` และ `--headless` ลงทะเบียน tool อีกสามตัวที่ไม่อยู่ใน `default_registry` ทั้งสามรันใน process เดียวกัน และไม่ถูกส่งไปยัง turn-executor child:

| Tool | คำอธิบาย | Chat default policy | Plan mode |
|---|---|---|---|
| `webfetch` | GET URL แบบ http(s) แล้วคืนข้อความ (แปลง HTML เป็นข้อความ) timeout 30 วินาที, body 1 MB, redirect ไม่เกิน 5 ครั้ง; ปฏิเสธ localhost และ address แบบ loopback, private, link-local และ CGNAT รวมถึงตอน redirect | `ask` (ออก network) | ถูกบล็อก |
| `todo` | รายการงานของ session เก็บในหน่วยความจำ (`read` / `write`) | `allow` | ถูกบล็อก |
| `subagent` | child session แบบอ่านอย่างเดียว: provider และ model เดียวกัน, context ใหม่, ใช้ได้เฉพาะ `read_file` / `list_dir` / `grep` / `glob`, เรียก model ไม่เกิน 15 ครั้ง, เปิด subagent ต่อไม่ได้ คืนคำตอบสุดท้าย | `allow` | ถูกบล็อก |

`webfetch`, `todo` และ `subagent` ไม่อยู่ใน plan mode เพราะ session สาธารณะของ connector รันใน plan mode และทุก tool ใน plan mode ต้องผ่าน capability gate บน turn ที่ไม่น่าเชื่อถือ ซึ่งมีแค่ `PURE_READ_TOOLS` ที่ผ่าน และไม่มี `apply_patch`: `multi_edit` ครอบคลุมการแก้หลายจุดโดยไม่ต้องมีไวยากรณ์ patch

---

## Tier 2 Deep Memory (HV3-1)

เมื่อ manifest ของ agent ตั้ง `deepMemoryCmd` (เครื่องมือใดก็ได้ที่พูด contract ของ `bwoc-core::deep_memory` — ตัวอ้างอิงคือ `bwoc-deep-memory`) harness จะปิดวงจรความจำรอบทุก session:

1. **wake-up** — ตอนเริ่ม session (ทั้ง batch และ `--chat`) output ของ `<cmd> wake-up` ถูกต่อท้าย system prompt เป็นบล็อก *"Prior context (Tier 2 memory)"*
2. **memory_search** — register tool แบบ read-only (`<cmd> search "<q>"`) ให้โมเดล recall การตัดสินใจเก่ากลางงาน; ผ่าน pipeline guardrails → permission เหมือน tool อื่นทุกตัว; policy default ของ chat อนุญาตคู่กับ `memory_read`
3. **mine** — ตอนจบ session ตัว session กลายเป็นความจำ: `--chat` mine `.bwoc/chat-session.json` ที่ persist ไว้; batch run ที่สำเร็จกลั่น *task → outcome* ลง `.bwoc/last-run.md` แล้ว mine (checkpoint ถูกลบไปแล้วตอนสำเร็จ); run ที่ล้มเหลว mine checkpoint ที่ยังอยู่ (run ที่พังคือสิ่งที่ควรจำที่สุด)

ทั้งหมด **opt-in, best-effort และมีขอบเขตเวลา**: ไม่มี/เป็น placeholder `deepMemoryCmd` = ปิดทั้งสามอย่าง (Tier 1 ทำงานต่อ); ความล้มเหลว degrade เป็น warning; ทุก subprocess มี timeout (wake-up 10 วิ, search 15 วิ, mine 60 วิ) — memory backend ที่ค้างทำให้ run สะดุดไม่ได้ *(สติ — agent จำข้าม session)*

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
2. อ่าน `config.manifest.json` เพื่อชื่อ model (`context_limit` ไม่ใช่ field ใน manifest: harness hardcode เป็น `0` คือไม่ compact และมี limit ต่อ model เฉพาะจากการเลือกแบบ `auto`)
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

`fallbackModel` เป็น metadata เท่านั้น — harness ไม่อ่านค่านี้ fallback chain ที่ลองเมื่อ tool call ผิดรูปแบบซ้ำมาจาก `autoModels` เมื่อ `primaryModel` เป็น `"auto"` (ส่วน history compaction และ context limit ต่อ model ตั้งบน `LoopConfig` ของ harness ไม่ใช่ field ใน `config.manifest.json`)

สำหรับ endpoint แบบ OpenAI-compatible ที่ serve GPT-5.5 ให้ใช้ model ชัดเจน
หรือ pool สำหรับเลือกตอน runtime:

```json
{
  "backend": "openai-compatible",
  "baseUrl": "https://api.openai.com/v1",
  "primaryModel": "auto",
  "autoModels": ["gpt-5.5", "gpt-5.5-pro", "gpt-5.4", "gpt-5.4-mini"],
  "reasoningEffort": "medium",
  "maxTokens": 32000,
  "promptCache": true,
  "thinking": false
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
**และ** เป็น `output_config.effort` บน path ของ Claude native — effort จึงถึงทุก HTTP
backend ที่รองรับ. `maxTokens` ก็ optional เช่นกัน: Messages API ของ Claude
บังคับต้องมี `max_tokens` ค่านี้จึง override ค่า default ของ harness ที่นั่น; ส่วน
backend แบบ OpenAI-compatible จะส่งเมื่อ set เท่านั้น ไม่งั้นใช้ default ของ provider.
`promptCache` **เปิดโดยดีฟอลต์** (`false` = ปิด): path ของ Claude native จะ mark
prefix ของ system prompt ที่เสถียรด้วย `cache_control` — agentic loop ที่ resend
ทุก turn จึงจ่าย cache-read (~0.1×) แทน full input (block `tools` ที่ render ก่อน
system ถูกครอบด้วย breakpoint เดียวกัน). ต่ำกว่า min cacheable size ของ provider =
no-op เงียบ ๆ. `thinking` **ปิดโดยดีฟอลต์** (เปิดด้วย `true`): path ของ Claude native
จะขอ adaptive extended thinking บน completion ทั้งแบบ non-streaming และ streaming
แล้ว preserve + replay thinking blocks ข้าม turn — จำเป็นสำหรับ tool path เพราะ
Messages API reject `tool_result` ที่ thinking block นำหน้าถูกตัดทิ้ง. บน path streaming
thinking block (พร้อม signature) จะถูกประกอบกลับจาก SSE delta แล้วแนบไปกับ assistant
message ที่ accumulate ไว้ — replay จึงทำงานเหมือนกัน. **multimodal image input**
เป็นแบบ provider-neutral: `ChatMessage` แนบรูป base64 ได้ (`with_images`) โดย render
เป็น Anthropic `image` block บน path native และเป็น OpenAI `image_url` data-URI part
บน path OpenAI-compat — ข้อความที่เป็น text อย่างเดียวยังเหมือนเดิมทุก byte. (การต่อสาย
screenshot จาก `computer` *ผ่าน re-exec turn-executor boundary* เข้า field นี้
เป็น follow-up แยกที่ต้อง verify บน bemind.) ปัจจุบัน BWOC harness ยังพูดผ่าน surface OpenAI-compatible `/v1/chat/completions`
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
| **OS-level sandbox** (macOS `sandbox-exec`, Linux landlock/seccomp) | **Ship แล้ว (2.3.0)** landlock จริง (Linux ≥ 5.13) + `sandbox-exec` (macOS) ผ่าน `make_os_sandbox()` degrade เป็น worktree-only บน kernel ที่ไม่รองรับ *(แถวนี้เคยบอก "stub" — stale ตั้งแต่ 2.3.0)* |
| **Streaming** | เชื่อมและทำงานได้ (SSE delta accumulation test ผ่าน) token count ใน `usage` ไม่มีบน streaming path (provider ไม่ return `usage` ใน SSE delta) |
| **Vetted-model list** | เล็ก ขณะนี้รู้ว่า `gemma4` และ `qwen2.5-coder:7b` ใช้ tool calling ได้ดี model ที่ไม่ได้ vetted จะ warning แต่ไม่ hard-block |
| **Context compaction** | **engine รวม (HV3-2)** summarize ก่อนด้วย provider หนึ่ง call, fallback เป็น truncate-with-marker เมื่อ summarizer ล้ม — policy เดียวทั้ง batch loop และ `--chat` เนื้อหาที่ถูกพับถูก mine ลง Tier 2 เมื่อ manifest ตั้ง `deepMemoryCmd` |
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
