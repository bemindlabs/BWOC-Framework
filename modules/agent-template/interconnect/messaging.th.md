---
title: การสื่อสารระหว่าง Agent
aliases:
  - Messaging
  - สาราณียธรรม 6
tags:
  - group/agents
  - type/design
  - meta/template
status: ฉบับร่าง (v2026.5.23 — sender identity + สาราณียธรรม 6)
canonical-source: อังคุตตรนิกาย 6.11–12 (สาราณียสูตร)
---

# การสื่อสารระหว่าง Agent

> [!abstract] การสื่อสาร agent → agent ต่อยอดจากช่อง inbox ที่ใช้ user → agent ([`send.rs`](../../../crates/bwoc-cli/src/send.rs)) ด้วยการระบุตัวตน sender ผู้รับสามารถปฏิเสธโดยอิงโปรไฟล์ความน่าเชื่อถือ Kalyāṇamitta 7 ของผู้ส่ง ([`trust.th.md`](trust.th.md)) ส่วนกติกาการพูดคุยอย่างนอบน้อมมาจาก **สาราณียธรรม 6** (อังคุตตรนิกาย 6.11–12) — 6 เงื่อนไขแห่งความสามัคคีที่แปลงเป็นกฎทางวิศวกรรม

## เหตุผล

`bwoc send` ส่ง envelope โดย hardcode `"from": "user"` ค่าเดียว — ถูกสำหรับช่อง human → agent แต่การประสานระหว่าง agent ต้องให้ผู้ส่งแสดงตัวตน เพื่อให้ผู้รับ:

1. ใช้กลไกตรวจ Kalyāṇamitta 7 (ทำเสร็จแล้วใน `bwoc-agent --serve` — ดู [`trust.th.md`](trust.th.md))
2. แสดงประวัติ inbox ที่มีความหมาย (`bwoc inbox` เห็น sender จริง ไม่ใช่ "user" รวมๆ)
3. ตรวจสอบนโยบายการปฏิเสธได้ — refusals อ้างชื่อ peer จริง ไม่ใช่ placeholder

นอกจากนี้ การมี sender identity ทำให้ framework บังคับ **สาราณียธรรม 6** ได้ — 6 เงื่อนไขแห่งความสามัคคีจากมหาปรินิพพานสูตรและ อังคุตตรนิกาย 6.11 ซึ่งเป็นแนวคำสอนหลักของพุทธะเกี่ยวกับการอยู่ร่วมกันในชุมชน เมื่อแปลงสู่การสื่อสารระหว่าง agent:

| บาลี | ความหมาย | ในระบบ |
|---|---|---|
| Mettā-kāya-kamma | เมตตากายกรรม | API stability — ไม่ทำ schema envelope พังกลางทาง |
| Mettā-vacī-kamma | เมตตาวจีกรรม | เนื้อหา `message` สุภาพ ตรงไปตรงมา ไม่ตะโกน ไม่หยาบคาย ไม่ดูถูก |
| Mettā-mano-kamma | เมตตามโนกรรม | ตีความ envelope ของ peer ในทางดี — malformed ≠ malicious |
| Sādhāraṇa-bhogī | สาธารณโภคี | สถานะมองเห็นได้ — เขียนลง JSONL inbox ไม่มีช่องลับ |
| Sīla-sāmaññatā | สีลสามัญญตา | Sīla 5 baseline + manifest schema เดียวกันทั้งสองฝั่ง |
| Diṭṭhi-sāmaññatā | ทิฏฐิสามัญญตา | อ้างอิงกราฟ `PHILOSOPHY.th.md` เดียวกันเมื่อให้เหตุผล |

ข้อจำกัดการออกแบบ 3 ข้อสำหรับ v1:

1. **Sender identity แสดงตัว ไม่ใช่พิสูจน์** ขั้นที่ 4 ของ trust ได้ ship แบบ v1 ที่ผู้ส่งแสดงเองใน manifest signed envelope (HMAC over workspace-local secret ฯลฯ) เลื่อนไป v2
2. **Trust gating เป็น opt-in ฝั่งผู้รับ** ผู้รับที่ `requiredTrust = []` รับทุก envelope ที่ well-formed ทั้งจาก agent หรือ user — strict-by-default จะทำให้ flow ที่มีอยู่พังตอน rollout
3. **ไม่มี file shape ใหม่** envelope schema เปลี่ยนความหมายของ `from` แต่ JSONL บน disk ยังเหมือนเดิม

## Envelope Schema

Envelope บน disk = 1 บรรทัด JSONL ต่อหนึ่งข้อความใน `<recipient>/.bwoc/inbox.jsonl` — schema:

```json
{
  "ts":        "<ISO 8601 UTC>",
  "messageId": "msg-<utc-slug>-<5hex>",
  "from":      "user" | "agent-<sender-name>",
  "to":        "agent-<recipient-name>",
  "message":   "<UTF-8 text>",
  "replyTo":   "msg-..."            // optional — เฉพาะเมื่อเป็นการตอบกลับ
}
```

ความหมายของแต่ละฟิลด์:

| ฟิลด์ | จำเป็น? | จุดประสงค์ |
|---|---|---|
| `ts` | ใช่ | ISO 8601 UTC ของการส่ง |
| `messageId` | ใช่ (ตั้งแต่ v2.0.x) | id ที่เสถียรสำหรับ threading + audit รูปแบบ: `msg-YYYYMMDDTHHMMSSZ-<5hex>` `bwoc send` สร้างให้ — caller ไม่ต้องส่งเอง |
| `from` | ใช่ | identity ผู้ส่ง (ตามตารางด้านล่าง) |
| `to` | ใช่ | ผู้รับ `agent-<name>` |
| `message` | ใช่ | UTF-8 body |
| `replyTo` | optional | `messageId` ของ envelope ก่อนหน้าเมื่อ send นี้เป็นการตอบกลับ stamp ผ่าน `bwoc send --reply-to <id>` — ขาดหายในรอบแรก |

Backward compatibility: reader เก่าที่ parse envelope เป็น `serde_json::Value` จะ ignore `messageId` กับ `replyTo` แบบเงียบๆ — ไม่มี behavior change สำหรับ inbox watch และ refusal path ของ daemon มัตตัญญุตา — เพิ่มแบบ additive ไม่กระทบ required fields

ความหมายของ `from` ตามค่า:

| ค่า `from` | ความหมาย | Trust gate |
|---|---|---|
| `"user"` | มนุษย์ผู้ใช้งาน (`bwoc send` default) | ผ่านเสมอ (ผู้รับปฏิเสธ user ไม่ได้) |
| `"agent-<name>"` | agent อื่นในเดียวกัน workspace | อยู่ภายใต้ `requiredTrust` ของผู้รับเมื่อ gating เปิด |
| อื่นๆ | สงวนไว้สำหรับ identity source ในอนาคต (signed external sender ฯลฯ) | ถูกปฏิเสธด้วย `reason: "unknown_sender"` |

ฝั่ง runtime (daemon poll + refusal logic) จัดการทั้ง 3 กรณีแล้วตั้งแต่ trust step 4 spec นี้แค่ระบุ contract เป็นทางการ

## CLI Surface

```
bwoc send <to> <message>                          # from=user (default)
bwoc send <to> <message> --from <agent>           # from=agent-<name>
bwoc send <to> <message> --reply-to <msg-id>      # ตอบกลับแบบ thread
bwoc send <to> <message> --no-wakeup              # ข้าม tmux ping
bwoc send --all <message>                          # broadcast ถึงทุก agent ใน workspace
bwoc send --team <team> <message>                 # broadcast ถึงสมาชิกทีม Saṅgha
bwoc outbox [list]                                # ข้อความ durable-retry ที่ค้างอยู่
bwoc outbox flush [--peer <id>]                   # ส่งซ้ำข้อความที่ spool ไว้
```

Broadcast (`--all` / `--team`) ยิงข้อความเดียวกันออกไปหาผู้รับแต่ละคนผ่าน transport ของผู้รับเอง (local inbox / MQTT / gateway) โดยใช้ single-send path ต่อผู้รับ — signing + routing เหมือนกันทุกประการ หมายเหตุ:
- ผู้รับมาจาก flag ข้อความจึงเป็น positional เดียว: `bwoc send --all "text"` `--all` กับ `--team` ใช้ร่วมกันไม่ได้ และ `--reply-to` ไม่มีความหมายกับ fan-out (ถูกปฏิเสธ)
- broadcast ที่มี `--from <agent>` จะไม่ส่งหา agent นั้นเอง
- `--dry-run` (broadcast) resolve + พิมพ์รายชื่อผู้รับโดยไม่ส่งอะไรจริง
- การส่ง fail รายผู้รับ **ถูก label แต่ไม่ทำให้ทั้ง run ล้ม** (peer ที่ offline จะถูก spool — ดูด้านล่าง) — เหมือน `bwoc ping --all` มีเพียง resolution error (ไม่มี workspace, ไม่รู้จักทีม, set ว่าง) และ hard error รายผู้รับเท่านั้นที่คืนค่า non-zero

### Durable offline delivery (outbox)

`bwoc send` ถึง **remote peer ที่ offline/ติดต่อไม่ได้** (gateway/MQTT relay ล้ม) จะไม่ทำข้อความหาย: envelope ที่ signed แล้วถูก spool ไปที่ `<workspace>/.bwoc/outbox/<recipientId>.jsonl` และรายงานว่า *spooled* (exit 0) ไม่ใช่ล้มเหลว — durable อยู่รอด restart ต่างจาก in-memory park ของ gateway

- **`bwoc outbox`** (หรือ `list`) — จำนวนที่ค้างต่อ peer
- **`bwoc outbox flush [--peer <id>]`** — ส่งซ้ำ แต่ละ envelope ถูก **replay ตรง ๆ** (messageId + signature เดิม) ดังนั้น inbox dedup ของผู้รับ (`inbox::append_envelope_deduped`) ทำให้ at-least-once retry กลายเป็น **effectively-once** ข้อความที่ส่งสำเร็จถูกลบจาก spool, peer ที่ยัง offline คงค้างไว้รอ flush รอบหน้า, ส่วน hard error (route ค้าง, gateway ไม่ signed) คงบรรทัดไว้และแจ้ง error

กฎ resolution ของ `--from`:
- รับชื่อ agent (หรือ `agentId` เต็ม) prefix `agent-` เติมให้ถ้าไม่มี — mirror กับ `--to`
- ผู้ส่งที่ระบุ **ต้อง** มีอยู่ใน `agents.toml` ของ workspace ที่ครอบคลุม ถ้าไม่พบ → exit 2 พร้อม error ชัดเจน
- `config.manifest.json` ของผู้ส่ง **ต้อง** อ่านได้ ถ้าอ่านไม่ได้ → exit 1

Daemon ฝั่งผู้รับ resolve manifest ของผู้ส่งใหม่ตอน envelope มาถึง — ฉะนั้นผู้ส่งที่เปลี่ยน declaration ระหว่าง `send` กับ `inbox poll` จะถูกประเมินด้วยสถานะ *ปัจจุบัน* (trust เป็น property ของ claim ปัจจุบัน ไม่ใช่ของเวลาที่ส่ง)

กฎ resolution ของ `--reply-to`:
- ค่าคือ `messageId` ของ envelope ก่อนหน้า (รับ string ใดก็ได้ที่ recipient เคย emit — framework ไม่ validate ว่ามีอยู่จริง ผู้รับ thread จาก lookup ไม่ใช่ foreign-key constraint)
- ถูก stamp ลงใน envelope ใหม่เป็น `replyTo` Stop hook (ดู §Wakeup & Auto-Reply) ใช้ฟิลด์นี้ปิด loop ของ request/response

`--no-wakeup` ปิด tmux send-keys ping แบบ best-effort ตามที่อธิบายในหัวข้อถัดไป CI, daemons และ auto-reply hook เองตั้ง flag นี้เพื่อไม่ให้ caller แบบ non-interactive ไป side-effect TUI session ที่ไม่ได้ขอให้ขัดจังหวะ ผลแบบ process-wide ได้จาก env `BWOC_DISABLE_TMUX_WAKEUP` (test suite ใช้ตัวนี้)

## Wakeup & Auto-Reply

Daemon poll ของผู้รับ surface envelope ตาม cadence ของตัวเอง แต่มี 2 กรณีที่ต้องการ latency ต่ำ: peer ที่รออันตอบโต้เชิง interactive และ multi-agent flow ที่ orchestrator ต้องการให้ prompt ของมันเข้าใน assistant turn ถัดไปแทนที่จะรอ poll interval

Framework ให้ครึ่งหนึ่งเป็น native Rust (`bwoc send`) และอีกครึ่งเป็น Claude Code Stop hook ที่ bundle ไปกับ template ทั้งสองเป็น opt-in โดยธรรมชาติ — ทำงานเมื่อเงื่อนไขเป็นจริง, degrade เป็น no-op เงียบๆ เมื่อไม่ใช่

### 1. Native tmux wakeup (`bwoc send`)

หลังจาก `bwoc send` append ลง `inbox.jsonl` ของผู้รับ มันจะลอง tmux send-keys ping แบบ best-effort:

| เงื่อนไข | พฤติกรรม |
|---|---|
| ผู้รับเป็น `agent-<x>` และ `tmux has-session -t <x>` สำเร็จ | ส่ง marker เป็น input → sleep 200 ms → ส่ง `Enter` |
| ผู้รับเป็น `user` หรือค่าอื่นที่ไม่ใช่ `agent-*` | ข้ามเงียบๆ |
| ไม่มี binary `tmux` ใน `PATH` | ข้ามเงียบๆ |
| caller ส่ง `--no-wakeup` หรือมี env `BWOC_DISABLE_TMUX_WAKEUP` | ข้ามเงียบๆ |

Marker line เป็น:

```
[bwoc inbox <messageId> from <sender>] <message body>
```

Convention: codename `agent-<x>` แมปไปยัง tmux session `<x>` Operator ที่ต้องการ feature นี้ wrap `bwoc spawn` ด้วย tmux session ที่ชื่อเดียวกับ bare name ของ agent (ไม่มี prefix `agent-`) เมื่อไม่มี wrap → wakeup เป็น no-op — daemon poll ก็ยัง deliver envelope อยู่ดี

การส่งแบบ 2 ขั้น (text → 200 ms → Enter) จำเป็น: `send-keys -l "text\n"` ครั้งเดียวจะถูก TUI input layer ของ Claude Code drop ทิ้ง — workaround เดียวกับ pattern ต้นทาง `it-app-workspace/bin/agent-send` ที่ port มา

### 2. Stop hook auto-reply (Claude Code, bundle กับ template)

`modules/agent-template/.claude/hooks/inbox-auto-reply.sh` เป็น Claude Code [`Stop` hook](https://docs.anthropic.com/en/docs/claude-code/hooks) ที่ wire ผ่าน `.claude/settings.json` ของ template เมื่อ turn จบ มันจะ:

1. Loop-guard ด้วย `stop_hook_active`
2. เดินขึ้นจาก `cwd` หา `config.manifest.json` ของ agent ตัวเอง อ่าน `agentId`
3. Scan transcript หา user message ล่าสุด match regex `\[bwoc inbox (msg-[\w.-]+) from ([\w-]+)\]`
4. รวบ assistant text ล่าสุดหลัง user prompt ที่ทำเครื่องหมาย (cap 4000 ตัวอักษร)
5. Shell ออกไป `bwoc send --from <self> --reply-to <id> --no-wakeup <sender> "<reply>"`

ข้ามเงียบๆ เมื่อ: ไม่มี marker, sender เป็น `user`, อ่าน manifest ไม่ได้, หรือไม่มี assistant text การ fail ของ `bwoc send` ถูกเก็บไว้ (`|| true` ใน wrapper) — hook ไม่บล็อกการจบ turn ของ Claude Code

### Backend neutrality

Hook เป็น Claude-specific เพราะ `Stop` event surface เป็น Claude-specific Contract เดียวกัน — *parse inbox marker, post reply ด้วย `--reply-to`* — ใช้กับ backend อื่นได้ equivalents จะ land ใน hook configuration ของแต่ละตัว:

| Backend | event analog | hook target |
|---|---|---|
| Claude (Anthropic) | `Stop` hook | `.claude/hooks/inbox-auto-reply.sh` (ship แล้ว) |
| Antigravity (Google) | tbd — ตาม hook surface ของ Antigravity | `.antigravity/hooks/...` (เลื่อน) |
| Codex (OpenAI) | tbd — ตาม hook surface ของ Codex | เลื่อน |
| Kimi (Moonshot) | tbd — ตาม hook surface ของ Kimi | เลื่อน |

สมานัตตตา — equal treatment เชิงเจตนา Backend อื่นรับ protocol เดียวกันทันทีที่ hook surface ของมัน land — ไม่มี special-casing ใน `bwoc-cli`

## สาราณียธรรม 6 — กฎเชิงวิศวกรรม

6 เงื่อนไขไม่ได้ถูก framework บังคับวันนี้ มันเป็น **norms** ที่ `AGENTS.md` §3 (Communication / Sammā-vācā) ของ template ควรสะท้อน เจตนาคือให้ agent ซึมซับเป็นแนวทาง ไม่ใช่ให้ `bwoc check` gate

### 1. เมตตากายกรรม — API stability

> การกระทำทางกายด้วยเมตตา: ไม่เปลี่ยนพื้นใต้เท้า peer

- JSONL envelope schema เป็น **append-only** — เพิ่มฟิลด์ optional ได้ แต่ฟิลด์เดิมความหมายเดิมเสมอ ฟิลด์ required ไม่ถอด
- Path ที่ spec นี้เปิดเผย (`.bwoc/inbox.jsonl`, `.bwoc/inbox.refusals.jsonl`) เป็น contract — ย้ายมัน = breaking change
- Protocol changes ของ Unix socket daemon (`PING`/`STATUS`/`STOP`) ใช้ discipline เดียวกัน

### 2. เมตตาวจีกรรม — พูดดี

> วจีกรรมด้วยเมตตา: เนื้อหา `message` ต้องอ่านเหมือนคำชี้แนะแบบเพื่อนร่วมงาน ไม่ใช่การตะโกน

- ใช้ประโยคบอกเล่ามากกว่า imperative ("กรุณาทำ X" ดีกว่า "ทำ X เดี๋ยวนี้")
- ห้าม ALL CAPS, คำหยาบ, คำดูถูก — framework ไม่บังคับ ผู้ตรวจ + operator บังคับ
- "ฉันทำไม่ได้" ที่ซื่อสัตย์ ดีกว่า "OK" ที่หลอก — ดู Vattā ใน [trust spec](trust.th.md)

### 3. เมตตามโนกรรม — คิดดี

> มโนกรรมด้วยเมตตา: ตีความ envelope ของ peer ในทางดี

- JSON malformed ≠ malicious — daemon poll จัดการ parse failure ด้วย warning + continue ไม่ใช่ระแวงว่าโจมตี
- ฟิลด์ optional ที่ขาด ≠ peer ไม่ปฏิบัติตาม — ใช้ default ตาม spec
- Sender ที่ไม่รู้จัก (`from: agent-x` ไม่อยู่ใน registry) ได้ structured refusal `reason: "unknown_sender"` ไม่ใช่ silent drop

### 4. สาธารณโภคี — แบ่งปันสิ่งที่ได้

> แบ่งปันทรัพยากรอย่างเป็นธรรม: state ต้องมองเห็นได้

- traffic ของ inbox ทั้งหมดอยู่ใน `inbox.jsonl` (version-able, grep-able, replay-able)
- refusals อยู่ใน `inbox.refusals.jsonl` (audit ได้ — ไม่เคยถูกลบ; merge ตอน read)
- ไม่มี agent ซ่อนข้อความใน private channel ที่ workspace มองไม่เห็น

### 5. สีลสามัญญตา — ศีลเสมอกัน

> precept ชุดเดียวกันทั้งสองฝั่ง

- ทั้ง sender และ recipient แสดงการปฏิบัติตาม Sīla 5 ([`AGENTS.md` §9](../AGENTS.md))
- ทั้งคู่ผ่าน `bwoc check` ก่อนเข้าร่วม flow ระหว่าง agent
- ทั้งคู่ใช้ manifest schema version ที่ compatible (mismatch `schemaVersion` เป็น refusal reason ใน trust v2 — ผ่อนปรนใน v1)

### 6. ทิฏฐิสามัญญตา — เห็นตรงกัน

> เป้าหมายตรงกัน: อ้างอิงปรัชญาเดียวกัน

- ข้อความ `message` ที่อ้าง Buddhist framework **ควร** link ไปที่รายการ canonical ใน `PHILOSOPHY.th.md` — เป็น convention ไม่ใช่ข้อบังคับใน wire format
- spec ระหว่าง agent (ไฟล์นี้, `trust.th.md`, `capabilities.md`) อยู่ใต้ `interconnect/` ฉะนั้น template ทุกตัว ship ที่ path เดียวกัน

## Backward Compatibility

- `bwoc send <to> <message>` แบบเดิมยังเขียน `from: "user"` เหมือนเดิม — ไม่มี behavior change
- `--from` default = `user` เมื่อไม่ใส่ — scripts ที่ไม่ผ่าน flag ใหม่ไม่ได้รับผลกระทบ
- Envelope เก่า (ก่อน spec) ที่มี `from: "user"` deserialize เหมือนเดิม — codepath user-passthrough ของ daemon ฝั่งผู้รับยังเป็นเส้นทางเดิม

## ลำดับการ Implement

1. ✓ `bwoc-agent --serve` daemon-side refusal สำหรับ sender ที่ไม่ใช่ `user` — **ship แล้วใน trust step 4** ฝั่ง runtime เสร็จแล้ว; spec นี้แค่บันทึก contract
2. `bwoc send --from <agent>` — flag sender identity ใน `bwoc-cli` (iter นี้)
3. Tests + live verification ของ flow agent → agent กับ trust gate (iter นี้)
4. CHANGELOG + ROADMAP cross-reference (iter นี้)
5. **เลื่อน (v2):** signed envelopes, sender identity proof, cross-workspace *trust* (broadcast `--all` / `--team` และ durable outbox retry **ship แล้ว** — ดู §CLI Surface)

## สิ่งที่ Spec นี้ ไม่ ครอบคลุม

- **Signed envelopes / identity proof** Workspace-local secret HMAC ครอบ envelope JSON เป็นแนวทาง v2 ที่ชัดเจน threat model วันนี้ยอมรับว่า clone ที่ประสงค์ร้ายเขียน `from: agent-bob` ได้ทั้งๆ ที่เป็น agent ตัวอื่น — การตรวจ trust วันนี้ทำกับ *manifest* ของผู้ส่งซึ่งเป็นไฟล์ต่อ agent บน disk
- **Cross-workspace messaging** Trust เป็น per-workspace ([`trust.th.md`](trust.th.md)) — envelope ที่ส่งหา agent ใน workspace อื่นเป็น undefined behavior ใน v1
- **Routing ผ่านตัวกลาง** การส่งทั้งหมดเป็น point-to-point (broadcast คือ fan-out ของ point-to-point ไม่ใช่ relay) — agent ที่อยาก relay ต้องอ่านจาก inbox ตัวเองแล้วส่งต่อโดยชัดแจ้ง

## ประวัติการแก้ Spec

- **v2 / 2026-08-08:** Broadcast fan-out (`bwoc send --all` / `--team`, #417) และ durable offline delivery (outbox spool + `bwoc outbox flush`, #418) ship แล้วและ document ใน §CLI Surface + §Durable offline delivery
- **v1 / 2026-05-23 (ฉบับร่างแรก):** Envelope schema + `--from <agent>` CLI surface + แผนที่ สาราณียธรรม 6 → กฎเชิงวิศวกรรม Trust gate integration ใช้งานได้แล้วจาก trust step 4 ที่ ship ก่อนหน้านี้ในวันเดียวกัน

## เอกสารอ้างอิง

- [`trust.th.md`](trust.th.md) — Kalyāṇamitta 7 trust model — refusal gate ทำงานบนฟิลด์ `from` ที่ spec นี้กำหนด
- [`capabilities.md`](capabilities.md) — capability declaration — peer ที่มี skill ที่ใช่ + trust ที่ต้องการ + posture สาราณียธรรมที่สะอาด = ภาพรวมที่สมบูรณ์
- [`AGENTS.md` §3 (Communication)](../AGENTS.md) — หลัก Sammā-vācā กับการพูดต่อ user — กฎเดียวกันใช้ระหว่าง peer
- [`PHILOSOPHY.th.md` #13. สาราณียธรรม 6](../docs/th/PHILOSOPHY.th.md) — อ้างอิง canonical สำหรับ 6 เงื่อนไข
- อังคุตตรนิกาย 6.11–12 — แหล่งดั้งเดิม ([SuttaCentral AN 6.11](https://suttacentral.net/an6.11), [AN 6.12](https://suttacentral.net/an6.12))