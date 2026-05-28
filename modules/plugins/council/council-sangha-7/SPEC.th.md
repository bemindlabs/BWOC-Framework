---
title: council-sangha-7 — สภาฉันทามติอปริหานิยธรรม 7
aliases:
  - council-sangha-7
tags:
  - group/framework-plugins
  - type/plugin
  - kind/council
  - domain/coordination
maturity: L1
---

# council-sangha-7 — สภาฉันทามติอปริหานิยธรรม 7

> [!abstract] ปลั๊กอินอ้างอิงสาย `council` สำหรับ `BWOC-EPIC-5` และเป็นปลั๊กอินสาย **coordination** ตัวแรกของเฟรมเวิร์ก มัน **บันทึก** การตัดสินใจของฟลีตผ่านโปรโตคอล `propose → discuss → vote → resolve` ภายใต้โมเดลการลงคะแนน **`sangha`** — การตัดสินใจผ่านได้ก็ต่อเมื่อ **องค์ประชุมเห็นพ้องเป็นเอกฉันท์** (อนุญาตให้งดออกเสียง บันทึกความเห็นแย้งไว้) ไม่มีกรณีเสมอ ดังนั้นเมื่อไม่เกิดความพร้อมเพรียงจึงเปิดรอบอภิปรายใหม่ คำสั่ง (ผ่าน [[protocol|protocol.sh]]): `propose`, `discuss`, `vote`, `resolve`, `list`, `show` ทุกคำสั่งทำงานบนเรกคอร์ดที่สอดคล้องกับ [[../../../docs/th/PLUGINS.th#สคีมา Council Decision|Council Decision Schema]] เริ่มต้นด้วย [[decisions|เทมเพลตอปริหานิยธรรม 7]] **อยู่ในเครื่อง/ฟลีตล้วน ๆ** — ไม่มีเครือข่าย ไม่มี credential มัน **บันทึก** การตัดสินใจ ไม่เคย **ลงมือทำ** — ผลแบบ `binding` ถูกบันทึกเป็น `bwoc task` ที่ควรปล่อย ไม่ใช่ให้ปลั๊กอินทำเอง เหตุผลฉบับเต็ม: [[../../../notes/2026-05-28_council-plugin-architecture|บันทึกออกแบบ BWOC-56]]

## ทำไมต้อง kind `council`

`council` เป็น kind สาย **coordination** ตัวแรกของเฟรมเวิร์ก ทุก kind ก่อนหน้านี้กระทำ *ออกไปข้างนอก* สู่ระบบภายนอก (**integration**: `workflow` / `jira`) หรือ *เหนือ workspace* ในรูปรายงาน (**reporting**: `audit` / `okr`) ส่วน `council` กระทำ *ในหมู่ agent ของฟลีตเอง* — มันเป็นคู่ฉบับที่อ่านได้ด้วยเครื่องของการอภิบาลแบบสงฆ์ที่ `bwoc fleet` ปรากฏอยู่แล้ว (สัญญาณอปริหานิยธรรม 7) มันเปลี่ยน "agent ควรเห็นพ้องเรื่อง X" จากเธรด inbox เฉพาะกิจให้เป็นการตัดสินใจที่บันทึกไว้ พร้อมผู้เข้าร่วม รอบอภิปราย คะแนน ผลลัพธ์ และความเห็นแย้งที่เก็บรักษาไว้ เหตุผลฉบับเต็ม (ทำไมไม่ใช่ `workflow` ทำไมไม่ใช่ `audit`): บันทึกออกแบบ decision 1

## โปรโตคอลการตัดสินใจ

```
proposed → discussing → voting → resolved
                    ↘ (องค์ประชุมไม่ครบ) → abandoned
                    ↘ (ไม่พร้อมเพรียง)    → กลับไป discussing (อีกหนึ่งรอบ)
```

โปรโตคอลเป็นแบบ **append-only** ต่อการตัดสินใจหนึ่งเรื่อง: รอบและคะแนนสะสมเพิ่มขึ้น ไม่เคยเขียนทับ — เรกคอร์ดคือร่องรอยตรวจสอบ (สัจจะ ความจริงแห่งการปรึกษา) `participants` และ `options` **ถูกตรึงไว้ตั้งแต่ตอน propose**; การเปลี่ยนอย่างใดอย่างหนึ่งคือการตัดสินใจใหม่ ไม่ใช่การแก้ไข (บันทึก §4, สคีมา §ความเสถียรของฟิลด์)

## โมเดลการลงคะแนน `sangha`

ปลั๊กอินนี้ประกาศ `voting_model = "sangha"` (หนึ่งในสี่โมเดลตามบันทึกออกแบบ §3) การตัดสินใจจะ **resolve** ได้ก็ต่อเมื่อเป็นจริงทั้งสองข้อ:

1. **องค์ประชุม (Quorum)** — จำนวนผู้เข้าร่วมที่ลงคะแนน (การงดออกเสียงนับเป็นการเข้าร่วม) อย่างน้อยเท่ากับ `[council].quorum` ใน manifest คำนวณเทียบกับ `participants` ของการตัดสินใจ (สแนปช็อตรายชื่อทีม) `"2/3"` ของรายชื่อ 4 คนปัดขึ้นเป็น 3
2. **ความพร้อมเพรียง (Concord)** — ผู้ลงคะแนน **ที่ไม่งดออกเสียง** ทุกคนเลือก option **เดียวกัน**

ผลลัพธ์:

| เงื่อนไข | ผล | `status` |
|---|---|---|
| องค์ประชุมไม่ครบ | ยกเลิก | `abandoned` |
| องค์ประชุมครบ **และ** ผู้ไม่งดออกเสียงทุกคนอยู่ option เดียว | ผ่าน; ตั้ง `outcome` | `resolved` |
| องค์ประชุมครบแต่ผู้ไม่งดออกเสียงแตกเสียง (หรือทุกคนงดออกเสียง) | **ไม่พร้อมเพรียง → อีกหนึ่งรอบ** (เปิดใหม่) | `discussing` |

**ไม่มีการตัดสินเสียงเสมอ** — โดยเจตนา การไม่พร้อมเพรียงไม่เคยถูกตัดสินด้วยเสียงชี้ขาด แต่เปิดการปรึกษาใหม่ (อนัตตา: ไม่มีเสียงของ agent ใดมีสิทธิพิเศษ) **การงดออกเสียงที่มี `--rationale`** จะถูกเก็บไว้ตอน resolve เป็น **ความเห็นแย้ง (dissent)** — การ "ยืนเฉย" แบบพระวินัย: ผู้นั้นไม่ขวางความพร้อมเพรียง แต่ข้อสงวนของเขาถูกบันทึกและไม่เคยถูกทิ้ง

## การทำงาน

`protocol.sh` หาคำสั่งจาก argument แรก หรือจาก `$BWOC_COUNCIL_OPERATION` เมื่อไม่ได้ส่ง argument (เส้นทาง dispatcher) CLI `bwoc council` (`BWOC-58`) อาจส่ง JSON request บรรทัดเดียวทาง stdin ได้ด้วย; **argv flags ทับค่าจาก stdin** สคริปต์รันด้วยมือเพื่อ smoke test ได้เต็มที่

| ช่องทาง | สิ่งที่ส่งมา |
|---|---|
| arg 1 | คำสั่ง — `propose` \| `discuss` \| `vote` \| `resolve` \| `list` \| `show` |
| `$BWOC_COUNCIL_OPERATION` (env) | คำสั่งเมื่อไม่ได้ส่ง argument (fallback ของ dispatcher) |
| stdin | JSON request บรรทัดเดียว (ทางเลือก); คำสั่งอ่านพารามิเตอร์จากที่นี่เมื่อมี argv ทับได้ |
| `$BWOC_PLUGIN_DIR` (env) | ไดเรกทอรีนี้; resolve `manifest.toml` + `decisions.toml` ถ้าไม่มีจะ fallback ไปยังไดเรกทอรีของสคริปต์เอง |

**เรกคอร์ดการตัดสินใจ** ถูกเก็บเป็นไฟล์ JSON ไฟล์ละหนึ่งการตัดสินใจ (`<decision_id>.json`) ใต้ไดเรกทอรี records ที่ resolve ตามลำดับ:

1. `$BWOC_COUNCIL_DIR` — override ชัดเจน
2. `$BWOC_WORKSPACE/.bwoc/council` — เมื่อมี workspace อยู่ในบริบท
3. `$BWOC_PLUGIN_DIR/records` — fallback ในตัวปลั๊กอิน (เรียกด้วยมือ / smoke test)

```bash
# propose จากเทมเพลต (seed question + options), resolve participants จากทีม
./protocol.sh propose --decision-id D1 --template ap1-regular-meetings --team design-council

# หรือระบุเองทั้งหมด
./protocol.sh propose --decision-id D1 --question "Adopt convention X?" --options "adopt,defer" \
  --participants "agent-jisoo,agent-jennie,agent-lisa,agent-rose" --effect advisory

./protocol.sh discuss --decision-id D1 --participant agent-jisoo --message-ref msg-20260528T120000Z-a1b2c
./protocol.sh vote    --decision-id D1 --participant agent-jisoo --option adopt
./protocol.sh vote    --decision-id D1 --participant agent-rose  --abstain --rationale "prefers to defer; stands aside"
./protocol.sh resolve --decision-id D1
./protocol.sh list
./protocol.sh show D1
```

## คำสั่ง (Verbs)

| คำสั่ง | อินพุต | เอาต์พุต | ผลข้างเคียง |
|---|---|---|---|
| `propose` | `--decision-id <id>` และ (`--template <tid>` \| `--question <q> --options a,b`); ทางเลือก `--team <id>` \| `--participants a,b`, `--effect advisory\|binding` (ค่าตั้งต้น `advisory`), `--evidence <kind:value>` (ซ้ำได้) | เรกคอร์ดการตัดสินใจใหม่ (`status=proposed`) | สร้าง `<id>.json` ปฏิเสธการเขียนทับ id ที่มีอยู่ |
| `discuss` | `--decision-id <id> --participant <agent> --message-ref <msg-id>` `[--round <n>]` | เรกคอร์ดที่อัปเดต | เพิ่ม turn `{ participant, message_ref }` ลงรอบ; `proposed → discussing` |
| `vote` | `--decision-id <id> --participant <agent>` (`--option <opt>` \| `--abstain`) `[--rationale <text>]` | เรกคอร์ดที่อัปเดต | เพิ่มคะแนน `{ participant, option, abstain }`; `→ voting` ลงซ้ำได้ (อันล่าสุดชนะ) |
| `resolve` | `--decision-id <id>` | `{ record, resolution }` — เรกคอร์ดที่ปิด/อัปเดต พร้อมสรุปผล | นับคะแนนตามกฎ sangha; ตั้ง `outcome` + `dissent` แล้วปิด (`resolved`), หรือ `abandoned`, หรือเปิดใหม่ (`discussing`) |
| `list` | — | JSON array ของสรุปการตัดสินใจ | ไม่มี — อ่านอย่างเดียว ข้าม (ไม่ตายเพราะ) เรกคอร์ดที่ผิดรูป |
| `show` | `--decision-id <id>` (หรือ `show <id>`) | เรกคอร์ดการตัดสินใจฉบับเต็ม | ไม่มี — อ่านอย่างเดียว |

## บันทึก ไม่เคยลงมือทำ

resolution ของการตัดสินใจแบบ `binding` จะมี **`binding_task`** — `bwoc task` ที่ *แนะนำ* ให้ agent ไปทำผลลัพธ์นั้น ปลั๊กอิน **ไม่เคย** แตะ code หรือ config และไม่เคยปล่อย task เอง: `council` เป็น kind สาย coordination ไม่ใช่สาย execution (บันทึกออกแบบ §5) สะพานปล่อย task ถูกเลื่อนออกไป — รากฐานส่งมอบเรกคอร์ด + บันทึกไว้; การต่อสายปล่อย `bwoc task` จริงจะตามมาเมื่อมีสภาแบบ binding ใช้งานจริง

## รูปร่างผลลัพธ์

### `propose` / `discuss` / `vote`

ปล่อยเรกคอร์ดการตัดสินใจฉบับเต็ม (รูปแบบ [[../../../docs/th/PLUGINS.th#สคีมา Council Decision|Council Decision Schema]]):

```json
{
  "decision_id": "D1",
  "status": "voting",
  "question": "Shall the fleet hold standups on a fixed, frequent cadence?",
  "effect": "advisory",
  "participants": ["agent-jisoo","agent-jennie","agent-lisa","agent-rose"],
  "options": ["affirm-cadence","revise-cadence"],
  "rounds": [{ "round": 1, "turns": [{ "participant": "agent-jisoo", "message_ref": "msg-20260528T120000Z-a1b2c" }] }],
  "votes": [{ "participant": "agent-jisoo", "abstain": false, "option": "affirm-cadence" }],
  "evidence_links": [{ "kind": "file", "value": "notes/2026-05-28_council-plugin-architecture.md" }],
  "opened_at": "2026-05-28T12:00:00Z"
}
```

### `resolve` (พร้อมเพรียง)

```json
{
  "record": {
    "decision_id": "D1", "status": "resolved", "outcome": "affirm-cadence",
    "dissent": [{ "participant": "agent-rose", "rationale": "prefers a weekly cadence; stands aside" }],
    "closed_at": "2026-05-28T12:30:00Z", "...": "..."
  },
  "resolution": {
    "resolved": true, "status": "resolved", "concord": true,
    "outcome": "affirm-cadence", "quorum_required": 3, "quorum_voted": 4,
    "dissent": [{ "participant": "agent-rose", "rationale": "prefers a weekly cadence; stands aside" }]
  }
}
```

การตัดสินใจแบบ `binding` จะมี `resolution.binding_task` เพิ่ม (`bwoc task` ที่แนะนำ) เมื่อองค์ประชุมไม่ครบ `resolution` จะเป็น `{ resolved: false, status: "abandoned", reason: "quorum not met", ... }`; เมื่อไม่พร้อมเพรียงจะเป็น `{ resolved: false, status: "discussing", concord: false, reason: "...another round needed", options_chosen: [...] }`

> [!note] ส่วนขยายของสคีมา เรกคอร์ดที่เก็บไว้เพิ่มสามฟิลด์นอกเหนือชุดฟิลด์ที่ตั้งชื่อไว้ใน Council Decision Schema ของ BWOC-57: `question` (คำถามที่มนุษย์อ่าน เพื่อให้ `list`/`show` อ่านง่าย), `effect` (`advisory`\|`binding` จากบันทึกออกแบบ §5) และ `rationale` (ทางเลือก) บน vote (ต้นทางของ dissent ตอน resolve) ทั้งสามอยู่เคียงข้างฟิลด์จำเป็นของสคีมาโดยไม่แก้ไขมัน — เป็นตัวเลือกให้สคีมาดูดซับในรุ่นถัดไป (แจ้งหัวหน้าสเปกแล้ว)

## คลาสข้อผิดพลาด

| Exit | คลาส | ความหมาย |
|---|---|---|
| `0` | สำเร็จ | JSON หนึ่งเอกสารบน stdout |
| `1` | dependency / IO | ไม่มี `jq`, เรกคอร์ดหรือ manifest ผิดรูป, การเขียน/IO ล้มเหลว |
| `2` | usage | คำสั่งไม่รู้จัก, flag หาย/ไม่ถูกต้อง, decision id ไม่รู้จัก, ผู้เข้าร่วมอยู่นอกรายชื่อ, option อยู่นอกชุดที่ประกาศ หรือการตัดสินใจถูกปิดแล้ว |

ทีมที่หาย, เรกคอร์ดที่หาย/ผิดรูป และ manifest ผิดรูป ล้มเหลวอย่าง **สะอาด**: ข้อความ stderr ชัดเจน + ออกด้วยรหัสไม่ใช่ศูนย์; ปลั๊กอินไม่เคย panic (`jq` คือ dependency รันไทม์ตัวเดียว เหมือนปลั๊กอินอ้างอิง `okr/workspace-okrs` และ `workflow/gcloud-*`)

## การตั้งค่า

```toml
# manifest.toml
[council]
voting_model = "sangha"   # โมเดลของปลั๊กอินอ้างอิงนี้
quorum       = "2/3"      # จำนวนเต็ม หรือเศษส่วนของผู้เข้าร่วม

# workspace.toml
[plugins.council-sangha-7]
enabled = true
```

ไม่มี `[config.schema]` — v1 อ่านเทมเพลตจาก `decisions.toml` และเก็บเรกคอร์ดใต้ workspace (หรือ fallback ในตัวปลั๊กอิน) surface ระดับ workspace เพียงตัวเดียวคือคีย์สากล `enabled`

## การจับคู่วงจรชีวิต

ตาม [[../../../docs/th/PLUGINS.th#Lifecycle|PLUGINS.th.md §Lifecycle]] kind `council` ถูกเรียกโดย CLI `bwoc council` (`BWOC-58`) `init`/`teardown` เกิดต่อการเรียกแต่ละครั้งรอบ ๆ `invoke` ปลั๊กอินไม่ถือสถานะใดนอกจากไฟล์ JSON ที่มันอ่านและเขียน

| เฟส | สิ่งที่ปลั๊กอินนี้ทำ |
|---|---|
| `init` | โดยปริยาย; ตรวจว่ามี `jq` บน PATH; resolve `decisions.toml` + ไดเรกทอรี records |
| `invoke` | parse คำสั่ง, อ่าน/เพิ่มลงเรกคอร์ดการตัดสินใจ, ปล่อย JSON |
| `teardown` | โดยปริยาย; ไม่มีสถานะให้คืน |

## Idempotency

- `list` / `show` อ่านอย่างเดียว
- `propose` ปฏิเสธการเขียนทับ decision id ที่มีอยู่ (participants/options ตรึงตั้งแต่ propose) — การเล่นซ้ำด้วย id เดิมถูกปฏิเสธ ไม่ใช่เขียนทับเงียบ ๆ
- `discuss` / `vote` เป็น **append-only**; การเล่นซ้ำเพิ่ม turn/คะแนนอีกอัน ตอนนับคะแนน คะแนน **ล่าสุด** ต่อผู้เข้าร่วมชนะ การลงซ้ำจึงลู่เข้า การเขียนเป็น atomic (ไฟล์ temp + `mv`)
- `resolve` กำหนดได้แน่นอนจากคะแนนปัจจุบัน: รันซ้ำด้วยคะแนนเดิมได้ผลเดิม

## ระดับวุฒิภาวะ (Maturity)

ประกาศ **L1** — ปลั๊กอินอ้างอิง `council/council-sangha-7` ตัวแรกที่รันได้; ทั้งหกคำสั่งทำงานกับเทมเพลต seed รอบ `propose → discuss → vote → resolve` เต็มวงถูกทดสอบ (พร้อมเพรียง, การงดออกเสียงเป็น dissent, แตกเสียง→อีกรอบ, องค์ประชุมไม่ครบ→ยกเลิก, ลงซ้ำ→อันล่าสุดชนะ) จะขยับเป็น **L2** เมื่อการตรวจเชิงลึกของ `bwoc check` (`BWOC-60`) ครอบคลุม manifest `[council]` + Decision Schema และ CLI `bwoc council` (`BWOC-58`) ทดสอบ verb แบบ end-to-end กับฟลีตจริง ≥2 agent (เกณฑ์ L2 ตามบันทึก §Status)

> [!note] การแบ่งงานตรวจสอบ ปลั๊กอินนี้ทำให้ `bwoc check` ยอมรับ kind `council` ในระดับ basic-well-formedness (kind อยู่ในชุดที่รองรับ; ตาราง `[council]` ถูกมองข้าม ไม่ถูกปฏิเสธ) การตรวจสอบเฉพาะ council เชิงลึก — `voting_model` ∈ สี่โมเดล, `quorum` เป็น int-หรือ-เศษส่วน และการตรวจฟิลด์ Decision Schema — อยู่ใน `BWOC-60` (เจ้าของ: `agent-rose`) ซึ่งถูกบล็อกโดยสตอรีนี้

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือ model ใด `kind = "council"` และ `voting_model = "sangha"` เป็นค่า enum ของเฟรมเวิร์กเอง; `sangha` / `อปริหานิยธรรม` เป็นคำอภิบาลภาษาบาลี ไม่ใช่ชื่อ vendor ไม่มีชื่อ vendor ปรากฏใน `kind`, `entry` หรือคีย์ config ใด สอดคล้องกับ **สมานัตตตา**

## ดูเพิ่มเติม

- [[../../../notes/2026-05-28_council-plugin-architecture|บันทึกออกแบบ BWOC-56]] — framing ฉบับเต็ม (decisions 1–7)
- [[../../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `council` + Council Decision Schema
- [[decisions|decisions.toml]] — เทมเพลตอปริหานิยธรรม 7 (ข้อมูล seed)
- [[protocol|protocol.sh]] — การ implement โปรโตคอล
- `bwoc fleet` / `crates/bwoc-cli/src/fleet.rs` — สัญญาณอภิบาลอปริหานิยธรรม 7 ที่ปลั๊กอินนี้ทำให้เป็นการตัดสินใจที่บันทึกไว้
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (ความเท่าเทียมสองภาษา)
