---
title: ความไว้วางใจระหว่าง Agent
aliases:
  - Trust Model (TH)
  - Kalyāṇamitta 7
tags:
  - group/agents
  - type/design
  - meta/template
status: active (v2026.5.24 — Trust v2 warn-mode implement แล้ว)
canonical-source: AN 7.36 (มิตตสูตร) — อังคุตตรนิกาย 7.36
---

# ความไว้วางใจระหว่าง Agent

> [!abstract] แต่ละ agent ประกาศ boolean 7 ค่าจาก Kalyāṇamitta-7 ("คุณสมบัติ 7 ของกัลยาณมิตร") ตาม AN 7.36 agent อื่นอ่าน boolean เหล่านั้นแล้วเลือกปฏิเสธข้อความจากผู้ส่งที่ขาดคุณสมบัติที่ผู้รับต้องการ ประกาศเองตอน incarnate ตรวจสอบด้วย `bwoc check` ไม่เลื่อนชั้นอัตโนมัติตอน runtime

## แรงจูงใจ

การส่งข้อความระหว่าง agent → agent (Sammā-vācā Phase 1) ต้องมีวิธีให้ agent หนึ่งปฏิเสธข้อความจาก peer ที่ไม่ผ่านมาตรฐานความน่าเชื่อถือพื้นฐาน รากฐานของ BWOC คือหลักพุทธ — trust model จึงใช้รายการ canonical ที่มีอยู่แล้วแทนการประดิษฐ์ schema ใหม่ คือ **Kalyāṇamitta 7** จาก *มิตตสูตร* (AN 7.36) ซึ่งอ้างอิงไว้แล้วใน [`PHILOSOPHY.en.md#15. Kalyāṇamitta 7`](../docs/en/PHILOSOPHY.en.md#15-kaly%C4%81%E1%B9%87amitta-7--inter-agent-trust-new)

สามข้อจำกัด design ที่เลือกสำหรับ v1:

1. **Boolean ประกาศเอง ไม่ใช่ score ที่ได้มา** แต่ละ agent อ้างว่าตัวเองมีคุณสมบัติข้อใดบ้างใน 7 ข้อ ไม่มี telemetry runtime ที่ "เลื่อนชั้น" profile (ซึ่งหลอกง่าย) Hybrid model แบบลดเท่านั้น เลื่อนไป v2
2. **`bwoc check` ตรวจสอบหลักฐาน** แต่ละ boolean มีกฎ "อะไรนับเป็นหลักฐาน" การประกาศที่ไม่มีหลักฐาน = check violation
3. **ปฏิเสธโดยฝ่ายผู้รับ** agent ที่ต้องการ peer เข้มงวด ตั้ง `requiredTrust: [...]` ใน manifest ของตน ข้อความจากผู้ส่งที่ขาดคุณสมบัติใด ๆ ที่ require จะถูกปฏิเสธที่ inbox layer

นี่คือ **model ที่ง่ายที่สุดที่รองรับการปฏิเสธอย่างมีหลัก** ในขณะที่ยัง auditable ได้ ไม่ใช่คำตอบสุดท้าย — trust ระหว่าง agent จะรวมพฤติกรรมที่สังเกตได้เข้ามาในภาพเมื่อ Phase 3 พัฒนาต่อ

## คุณสมบัติ 7 ข้อ (AN 7.36)

มิตตสูตรระบุคุณสมบัติ 7 ของกัลยาณมิตร (บาลี → ความหมายตรง → ความหมายระดับระบบ) คอลัมน์ที่สามคือการ adapt ของ BWOC ตามที่บันทึกใน [`PHILOSOPHY.en.md`](../docs/en/PHILOSOPHY.en.md)

| บาลี | ความหมายตรง | ในระบบ | Manifest key |
|---|---|---|---|
| Piyo | น่าพอใจ / น่ารัก | Delegate ให้ง่าย | `piyo` |
| Garu | น่าเคารพ / หนักแน่น | น่าเคารพในความสามารถ | `garu` |
| Bhāvanīyo | น่ายกย่อง / น่าเจริญรอยตาม | ช่วยให้เราพัฒนา | `bhavaniyo` |
| Vattā | ผู้กล่าว | พูดความจริงที่เป็นประโยชน์ | `vatta` |
| Vacanakkhamo | ผู้ฟังด้วยขันติ | รับ feedback ได้ | `vacanakkhamo` |
| Gambhīrañca kathaṃ kattā | ผู้พูดเรื่องลึก | อธิบายเรื่องลึกได้ | `gambhira` |
| No caṭṭhāne niyojaye | ไม่ชักนำในทางไม่สมควร | ไม่ชักนำไปทางผิด | `noCatthana` |

Manifest key เป็น **camelCase** เพื่อความเข้ากันได้กับ style ของ `config.manifest.json` เดิม ไม่ใส่เครื่องหมายกำกับ (หลีกเลี่ยงปัญหา encoding ข้าม backend)

## Manifest Schema

block `trust` ใหม่ระดับ top ใน `config.manifest.json` ทั้งสองครึ่งเป็น optional — ไม่มี block = ทุกอย่าง false (ไม่ประกาศคุณสมบัติใด ไม่ require คุณสมบัติใด)

```json
{
  "agentId": "agent-{{name}}",
  "role": "{{agentRole}}",
  "trust": {
    "schemaVersion": 1,
    "declared": {
      "piyo": true,
      "garu": false,
      "bhavaniyo": true,
      "vatta": true,
      "vacanakkhamo": true,
      "gambhira": false,
      "noCatthana": true
    },
    "requiredTrust": ["vatta", "noCatthana"],
    "mode": "warn"
  }
}
```

> [!note] `declared` อธิบาย **สิ่งที่ agent นี้อ้างเกี่ยวกับตัวเอง**; `requiredTrust` อธิบาย **สิ่งที่ agent นี้ต้องการจาก peer ที่จะส่งข้อความถึงตน** สองอย่างเป็นอิสระ — agent หนึ่ง require คุณสมบัติที่ตัวเองไม่อ้างได้ และนั่นถูกต้อง (ผู้รับมีสิทธิ์ตั้งมาตรฐานของตน)

### field `mode` (Trust v2)

field `"mode"` ใน trust block เป็น optional ควบคุมการตอบสนองของ daemon เมื่อ envelope ขาคุณสมบัติที่ require ค่าที่รับได้: `"off"` | `"warn"` | `"refuse"`

**กฎ backward-compat:** เมื่อ `mode` ไม่มี framework คำนวณ effective mode ด้วยกฎ v1 — `requiredTrust` ว่าง → `off`; ไม่ว่าง → `refuse` หมายความว่า manifest เก่าที่ไม่มี field นี้ไม่ถูกเปลี่ยนจาก refuse เป็น warn โดยเงียบ ๆ `warn` เป็น opt-in เท่านั้น

เพื่อเปิด warn-mode บน agent ใด เพิ่ม `"mode": "warn"` ใน trust block พร้อมกับ `requiredTrust` ที่ไม่ว่าง

### `schemaVersion`

block `trust` มี `schemaVersion: 1` integer ชัดเจน เพื่อให้ spec รุ่นถัดไปเพิ่ม field ได้โดยไม่ทำให้ผู้รับพังเงียบ ๆ **กฎสำหรับ field ที่ขาดใน `declared`: ถือเป็น `false`** ถ้า v2 เพิ่มคุณสมบัติ `mudu` และ manifest agent v1 ไม่ประกาศ ผู้รับที่รัน v2 เห็น `declared.mudu === undefined` framework ถือเป็น `false` logic การปฏิเสธของผู้รับใช้ตามปกติ — ไม่มี silent accept ไม่มี silent refuse peer v1 ทุกตัว

daemon v1 ที่อ่าน manifest v2 จะ ignore field ที่ไม่รู้จักทั้งหมด (forward-compat ผ่านการไม่รู้) นี่คือ **Anicca** — วาง seam ก่อนการเปลี่ยน

### Scaffolding ของ manifest

template `incarnate.sh` และ (ในอนาคต) `bwoc new` จะ seed block `trust` ด้วย floor ที่สมเหตุสมผล: `requiredTrust: ["vatta", "noCatthana"]` — "พูดความจริงที่เป็นประโยชน์" + "ไม่ชักนำผิดทาง" สองคุณสมบัตินี้ Pi ระบุเป็น **minimum civic floor** ที่ peer ทุกตัวควรเรียกร้องตามเหตุผล agent ที่ incarnate ใหม่จึงเป็น "strict-ish out of the box" ในขณะที่ default ระดับ framework ยังคงเป็น permissive (agent เก่าไม่ได้รับผลกระทบ; การยอมรับผ่าน scaffold ≠ การพลิก default) หลีกเลี่ยง **vestigial-feature risk** ของกลไกปฏิเสธที่ไม่มีใคร opt-in

## กฎหลักฐาน (สิ่งที่ `bwoc check` ตรวจ)

คุณสมบัติที่ประกาศจะ **valid ก็ต่อเมื่อ** มีหลักฐานที่สอดคล้องกัน `bwoc check` อ่าน manifest แล้วตรวจการประกาศ `true` แต่ละข้อต่อกฎด้านล่าง `true` ที่ไม่มีหลักฐาน → check **violation** (exit 1) `false` valid เสมอ (ไม่ต้องมีหลักฐาน)

| Quality | กฎหลักฐาน (สิ่งที่ `bwoc check` มองหา) |
|---|---|
| `piyo` | persona scope ไม่ว่าง AND อธิบาย task ที่ delegate ได้อย่างเป็นรูปธรรม (`persona/README.md` section "Scope" ถูกกรอก) การ delegate ต้องมีจุดจับที่ชัดถึงจะรู้สึกน่าพอใจ |
| `garu` | มี skill หรือ mindset stub อย่างน้อย 1 ตัวใต้ `skills/` หรือ `mindsets/` ความน่าเคารพต้องมี surface ความสามารถที่สาธิตได้บ้าง |
| `bhavaniyo` | `mindsets/` มี entry ที่ชื่อหรือเนื้อหาพูดถึง improvement / verification / right-amount (tag Yoniso Manasikāra / Mattaññutā) การช่วย peer พัฒนาต้องมีกรอบ improvement ที่ชัดเจน |
| `vatta` | anti-scope (out-of-scope) ของ persona ไม่ว่าง การพูดความจริงที่เป็นประโยชน์ต้องซื่อสัตย์ว่าตัวเอง *ไม่ทำ* อะไรบ้าง anti-scope ว่าง = ไม่มี commitment ต่อการปฏิเสธอย่างซื่อสัตย์ |
| `vacanakkhamo` | มี inbox flow ที่ใช้แล้วอย่างน้อย 1 ครั้ง (`.bwoc/inbox.jsonl` มี และไม่ว่าง OR `interconnect/feedback.md` documenting วิธีที่ agent จัดการ feedback) |
| `gambhira` | มี doc file ใต้ agent root อย่างน้อย 1 ตัวที่ ≥ 50 บรรทัด AND มี wikilink `[[PHILOSOPHY.en.md]]` (หรือ `[[PHILOSOPHY.th.md]]`) ความลึกต้อง *เชื่อมโยงกับ canon* — backlink เข้าหา graph เชิงปรัชญาพิสูจน์ว่าความลึกประสานกับ framework ไม่ใช่แค่โรย Pali keyword (draft แรกรับ "Pali term mention" เป็นหลักฐาน Pi ชี้ว่าเป็นเกณฑ์ที่ pad ง่ายที่สุดในตาราง — keyword sniff โดยไม่มี commitment เชิงโครงสร้าง backlink ปลอมยากกว่า: link จะใช้ได้ก็ต่อเมื่อ `PHILOSOPHY.en.md` มีอยู่ที่ relative path ที่ถูก ซึ่งบังคับให้ doc อยู่ใน tree ที่ถูก) |
| `noCatthana` | `persona/README.md` section "Anti-scope" มี AND มีรายการ "จะปฏิเสธ" อย่างชัดเจนอย่างน้อย 1 รายการ การปฏิเสธ request ที่ไม่เหมาะสมคือฐานของการไม่ชักนำไปทางผิด |

กฎเหล่านี้เป็นกลไกอย่างเจตนา — ไม่ได้วัดความน่าเชื่อถือ *จริง* แค่วัดว่า agent มีโครงสร้างพื้นฐานที่จะลองทำคุณสมบัติได้หรือไม่ การอ้างที่ซื่อสัตย์ยังขึ้นกับ human operator บทบาทของ framework คือ **จับคำโกหกที่ชัดเจน** (อ้าง `gambhira` โดยไม่มี doc) ไม่ใช่รับรองคุณธรรม

## Read API

```
bwoc trust <agent>              # human table: 7 booleans + รายการ requiredTrust
bwoc trust <agent> --json       # { "declared": {…}, "requiredTrust": […] }
```

สถานะ: คำสั่ง **ยังไม่ implement** spec อย่างเดียว

อ่าน trust profile ของ agent อื่นจาก script:
```bash
bwoc trust agent-beta --json | jq -r '.declared.vatta'
# → true | false
```

## Semantics การปฏิเสธ

เมื่อ `bwoc send <recipient> <message>` (หรือ agent-originated send ในอนาคต) append envelope JSONL:

1. daemon ของผู้รับอ่าน envelope ใน poll ครั้งถัดไป
2. ถ้าผู้รับมี array `trust.requiredTrust` ที่ไม่ว่าง daemon resolve manifest ของ **ผู้ส่ง** และอ่าน `trust.declared`
3. ถ้าคุณสมบัติ require **ใด ๆ** ขาดหรือ `false` ในการประกาศของผู้ส่ง daemon:
   - mark envelope เป็น `refused` (ไม่ลบ — auditability สำคัญ)
   - เขียน field `refused: { reason: "missing_trust", missing: [qualities] }` บน envelope
   - process envelope อื่น ๆ ต่อไปตามปกติ
4. ผู้ส่ง **ไม่** ได้รับการแจ้งเตือนอัตโนมัติว่าถูกปฏิเสธ ถ้าสนใจสามารถ `bwoc inbox <recipient> --json | jq '.[] | select(.refused)'`

ผู้ส่ง == `user` เป็นกรณีพิเศษ: ข้อความจาก user ผ่านเสมอ (user อยู่เหนือ trust gate โดยนิยาม) Trust gate คุม agent→agent messaging เท่านั้น

พฤติกรรม default — `trust.requiredTrust` ว่างหรือไม่มี — คือ **ไม่ gating** Framework ship แบบ permissive โดย default; ผู้รับ opt-in เข้ามาที่ refusal agent ที่ incarnate ใหม่จะได้รับ default ที่ไม่ว่างจาก template scaffold (`["vatta", "noCatthana"]`) — ดู [Scaffolding ของ manifest](#scaffolding-ของ-manifest) — เพื่อให้ feature ไม่เป็น vestigial ในทางปฏิบัติ ในขณะที่ permissive ในระดับนโยบาย

### Mode การปฏิเสธ (Trust v2 — implement แล้ว)

daemon รองรับ 3-state refusal mode ควบคุมด้วย field `mode` ใน trust block ของผู้รับ effective mode กำหนดสิ่งที่เกิดขึ้นเมื่อ envelope ของผู้ส่งขาดคุณสมบัติที่ require:

| Mode | Effective เมื่อ | สิ่งที่ daemon ทำ |
|---|---|---|
| `off` | `mode: "off"` ชัดเจน หรือ `mode` ไม่มีและ `requiredTrust` ว่าง | envelope ผ่าน; ไม่ log |
| `warn` | `mode: "warn"` ชัดเจน (opt-in เท่านั้น) | envelope **ผ่าน** (ส่งตามปกติ) AND daemon emit log line `trust_warn`: `bwoc-agent: trust_warn ← <sender>: missing=["quality", ...]` ผู้รับ monitor `bwoc log -f` ตัดสินใจ upgrade เป็น `refuse` ได้ |
| `refuse` | `mode: "refuse"` ชัดเจน หรือ `mode` ไม่มีและ `requiredTrust` ไม่ว่าง (default v1) | envelope mark `refused` ใน `inbox.refusals.jsonl` พร้อม block `refused: { reason: "missing_trust", missing: [...] }` ไม่ลบ |

**Can't-verify paths refuse เสมอโดยไม่คำนึง mode** เมื่อ daemon resolve manifest ของผู้ส่งไม่ได้ (`no_workspace`, `registry_unreadable`, `unknown_sender`, `sender_manifest_unreadable`) envelope ถูกปฏิเสธ ผู้ส่งที่ตรวจสอบไม่ได้ไม่สามารถ warn-pass ได้

**`from: "user"` ผ่านเสมอ** โดยไม่คำนึง mode หรือคุณสมบัติที่ require Trust gate คุม agent→agent messaging เท่านั้น

เหตุผลของ design 3-state: strict-by-default สำหรับ trust model ที่ self-declared (ไม่ sign) คือ **security theater** — adversary โกหก manifest ของตัวเองได้ `warn` ให้ผู้รับมีข้อมูลตัดสินใจโดยไม่ commit framework กับ semantics false-strict `warn` เป็น opt-in อย่างเคร่งครัดเพื่อให้ agent เก่าที่มี `requiredTrust` ไม่ว่าง ยัง refuse ตาม v1 เดิม (Anicca — ไม่มี silent security regression)

## สิ่งที่ spec นี้ไม่ครอบคลุม

- **การปรับ runtime** v1 ประกาศอย่างเดียวเข้มงวด ไม่มีการเปลี่ยน score จาก telemetry Hybrid model เลื่อนไป v2
- **การ sign / proof ตัวตน** Clone ที่ malicious โกหกใน `config.manifest.json` ได้ การพิสูจน์ตัวตน (manifest ที่ sign แล้ว ฯลฯ) เป็น Phase 3 work item แยก — spec นี้สมมติว่า agent ภายในขอบเขต workspace ประกาศซื่อสัตย์ **นี่คือเหตุผลที่ `requiredTrust` ต้องไม่ strict-by-default ระดับ framework:** การปฏิเสธอย่างเข้มงวดต่อ manifest ที่ไม่ sign คือ security theater การ scaffold floor (`["vatta", "noCatthana"]`) บน agent ใหม่ยอมรับได้เพราะ agent ใหม่ OPT IN ที่เวลา incarnate; นโยบายไม่ได้บังคับใช้ทั้ง framework
- **Reputation ข้าม workspace** Trust ต่อ workspace agent ที่ trust ใน workspace A เป็นคนแปลกหน้าใน workspace B จนกว่าจะ incarnate ที่นั่น
- **การแจ้งผู้ส่งกลับเมื่อถูกปฏิเสธ** ตัดออกอย่างจงใจ — การปฏิเสธเป็นสิทธิ์ของผู้รับ ไม่ใช่สัญญาที่จะต้องแจ้งผู้ส่ง การฟังเป็นหน้าที่ของผู้ส่ง

## ประวัติการแก้ไข spec

- **v1 / 2026-05-23 (draft แรก):** boolean 7 ค่า + array `requiredTrust` ตาราง evidence-rule semantics การปฏิเสธ default permissive ไม่มี scaffolding floor
- **v1.1 / 2026-05-23 (review Oracle + Pi):**
  - กฎ evidence ของ `gambhira` เขียนใหม่จาก "≥50 บรรทัด + Pali term mention" → "≥50 บรรทัด + wikilink `[[PHILOSOPHY.en.md]]`" ตาม Pi (จับ loophole keyword-sniff reuse infrastructure wikilink ที่มีอยู่)
  - เพิ่ม `trust.schemaVersion: 1`; document กฎ "field ที่ขาดใน `declared` → `false`" ตาม Pi (seam Anicca สำหรับ field v2 ในอนาคต)
  - clause scaffolding: incarnate.sh / `bwoc new` seed `requiredTrust: ["vatta", "noCatthana"]` ตาม Pi (ป้องกัน vestigial-feature โดยไม่พลิก default ของ framework)
  - section "Mode การปฏิเสธ" เพิ่ม วางแผน 3-state (`off` / `warn` / `refuse`) สำหรับ v2 ตาม Oracle (warn-by-default หลีกเลี่ยง security theater ในขณะที่เก็บข้อมูล)
- **v2 / 2026-05-24 (Trust v2 warn-mode — Oracle, GH #6 / WS5):**
  - เพิ่ม enum `RefusalMode` (`off` | `warn` | `refuse`) ใน `bwoc-core::manifest::TrustBlock`
  - เพิ่ม field `"mode"` optional บน trust block (serde key `"mode"`) Backward-compat: field ไม่มี → effective mode จากกฎ v1 (required ว่าง → `Off`; ไม่ว่าง → `Refuse`) `warn` เป็น opt-in อย่างเคร่งครัด
  - `evaluate()` return `TrustOutcome` (`Pass` / `Warn { from, missing }` / `Refuse(Refusal)`) แทน `Option<Refusal>`
  - daemon caller จัดการ `Warn`: ส่ง envelope ตามปกติ + emit `trust_warn` log line ผ่าน `announce_warned` ไม่บันทึก refusal บน `Warn`
  - Can't-verify paths (`no_workspace`, `registry_unreadable`, `unknown_sender`, `sender_manifest_unreadable`) produce `Refuse` เสมอโดยไม่คำนึง mode

## ลำดับ Implementation

1. `bwoc-core::Manifest`: deserialize block `trust` Backward-compatible: missing block = default ✓ done (v1)
2. `bwoc check`: เพิ่ม 7 verification check ตาม `evidence-rules` ด้านบน surface เป็น PASS / WARN / FAIL ต่อ quality
3. `bwoc trust <agent>` คำสั่ง read: table + `--json`
4. `bwoc-agent --serve`: ตอน poll inbox resolve `trust.declared` ของผู้ส่ง เทียบกับ `requiredTrust` ของตัวเอง mark envelope ที่ refused ✓ done (v1)
5. Trust v2 warn-mode: enum `RefusalMode` + field `mode` + `TrustOutcome` 3-state + `announce_warned` ✓ done (v2, GH #6)
6. row CHANGELOG + ROADMAP cross-reference + bilingual TH parity

Step 4+ อยู่หลัง env opt-in `BWOC_TRUST_GATING=1`

## อ้างอิงข้าม

- [`PHILOSOPHY.en.md` #15. Kalyāṇamitta 7](../docs/en/PHILOSOPHY.en.md) — การ map เชิงปรัชญาที่ spec นี้ implement
- [`capabilities.md`](capabilities.md) — การประกาศ capability (skill registry); trust ประกอบกับ capability (peer ที่มี skill ที่ใช่ AND trust ที่ require)
- `interconnect/feedback.md` (เสนอ ยังไม่ draft) — โครงสร้างหลักฐาน `vacanakkhamo`
- AN 7.36 มิตตสูตร — แหล่ง canonical ([SuttaCentral](https://suttacentral.net/an7.36))
