---
title: ธรรมาภิบาล Fleet — อปริหานิยธรรม 7
aliases:
  - Fleet Governance
  - อปริหานิยธรรม 7
  - หลักไม่เสื่อม
tags:
  - group/governance
  - type/design
  - meta/framework
status: ฉบับร่าง (v2026.5.23 — spec แรก; observable signals วางไว้, automation เลื่อน)
canonical-source: ทีฆนิกาย 16 (มหาปรินิพพานสูตร) §1.4 — โอวาทแก่ชาววัชชี
parent: ภาษาไทย
nav_order: 10
---

# ธรรมาภิบาล Fleet — อปริหานิยธรรม 7

> [!abstract] หลัก 7 ประการแห่งความไม่เสื่อม พระพุทธเจ้าทรงแสดงแก่ชาววัชชี 7 ข้อปฏิบัติเพื่อความยั่งยืนของชุมชน (ทีฆนิกาย 16) BWOC นำหลักนี้มาใช้เป็นชั้น **ธรรมาภิบาล fleet** — กติกาที่ operator ผู้ดูแล agent หลายตัวใน workspace ใช้รักษาสุขภาพของ fleet ในระยะยาว Phase 4 territory: framework ให้กติกา, การยอมรับใน ecosystem เป็นตัวทำให้มันเกิดผลจริง

## ทำไมต้องมี

Phase 1–3 ให้พื้นฐานทางเทคนิคแก่ workspace: incarnation, lifecycle, messaging, trust ไม่มีอะไรตอบคำถาม *"fleet นี้สุขภาพดีอยู่ไหมสัปดาห์นี้?"* โดยตรง — นั่นเป็นคำถามระดับ **ธรรมาภิบาล** ที่ scope ของ workspace + operator ไม่ใช่ของ agent ตัวใดตัวหนึ่ง

โอวาทแก่ชาววัชชี (ทีฆนิกาย 16 §1.4) เป็นแหล่งดั้งเดิมทางพุทธศาสนาสำหรับกติกาเรื่องความยั่งยืนของชุมชน พระพุทธเจ้าทรงตรัส 7 ประการ — เมื่อยังอยู่ ชุมชนจะเจริญ ไม่เสื่อม ทั้ง 7 ข้อ map เข้ากับการทำงาน multi-agent workspace ได้ลงตัวเพราะทั้งสองเผชิญความเสี่ยงเชิงโครงสร้างแบบเดียวกัน: การเบี่ยงเบนเมื่อเวลาผ่านไปโดยไม่มีจุดยึดที่ชัดเจน

ข้อจำกัดการออกแบบ 3 ข้อสำหรับ v1:

1. **สังเกตได้ แต่ยังไม่บังคับ** แต่ละข้อมี *signal* ที่ framework อ่านได้ (รายการใน registry, mtime ของไฟล์, manifest schemaVersion) — ไม่ใช่ hard gate. v2 อาจยกระดับ signal เป็น gate เมื่อ telemetry สนับสนุน
2. **Scope ระดับ workspace** ธรรมาภิบาลใช้กับต้นไม้ `.bwoc/workspace.toml` หนึ่ง การประสานข้าม workspace อยู่นอก scope (เลื่อนไป Phase 4+ vision)
3. **มุ่งสู่ operator** spec นี้อ่านโดยมนุษย์ผู้ดูแล workspace ไม่ใช่ agent แต่ละตัว — agent ใช้สาราณียธรรม 6 (cordiality ระดับ peer); operator ใช้อปริหานิยธรรม 7 (สุขภาพ fleet)

## 7 ข้อ

แต่ละแถว: บาลี → ความหมายดั้งเดิม → การใช้ใน BWOC → signal ที่สังเกตได้ → แนวปฏิบัติของ operator ที่แนะนำ

### 1. หมั่นประชุมเนืองนิตย์ — *abhiṇha-sannipāta*

> ชาววัชชีประชุมกันเป็นประจำและบ่อยครั้ง; agent ต้อง sync เป็นประจำ

**การใช้:** fleet ที่ไม่มี check-in สม่ำเสมอจะเบี่ยงเบน `task-log.jsonl` ของแต่ละ agent เป็น log ส่วนตัว แต่ workspace เองต้องมีจังหวะ sync เต็ม fleet — ติดต่อทุก agent, รู้ทุก status

**Signal:** `bwoc list --json` คืน `status`, `running` flag, และ timestamp `incarnated` ล่าสุดของแต่ละ agent. workspace ที่ agent ตัวใดไม่ได้ถูกแตะใน N สัปดาห์ = warning ไม่ใช่ violation

**ปฏิบัติ:** รัน `bwoc list --json | jq '.[] | select(.status == "active")'` ตามจังหวะ (รายวัน / รายสัปดาห์) ดู agent ที่ `inbox` มี envelope ยังไม่อ่าน หรือ daemon ไม่ได้ ping ภายใน N วัน TUI `bwoc dashboard` เป็น surface ธรรมชาติ

### 2. เริ่ม-เลิกพร้อมกัน — *samaggā sannipatanti*

> เข้ามาประชุมพร้อมเพรียงกัน และเลิกพร้อมเพรียงกัน

**การใช้:** เมื่อ workspace ถูกเปิด/ปิด daemon ของทุก agent ควร start/stop *พร้อมกัน* ไม่ใช่ทีละตัว workspace ที่หยุด = daemon ทุกตัวหยุด; workspace ทำงาน = daemon ที่คาดหวังทุกตัวขึ้น สถานะกึ่งกลาง (บางตัวขึ้น บางตัวลงโดยไม่มีเจตนาของ operator) = ความเสี่ยงเรื่อง dispersion

**Signal:** `bwoc workspace prune --apply` reconcile drift ระหว่าง registry status กับสภาพบน disk. `bwoc doctor` sweep `agent.pid` / `agent.sock` ที่ stale ทั้งคู่จับ agent ที่ *คิด* ว่ายังทำงานอยู่แต่จริงๆ ไม่ใช่ หรือกลับด้าน

**ปฏิบัติ:** ห่อ `bwoc start --all` / `bwoc stop --all` (surface ที่มีอยู่แล้ว) ใน playbook ของ operator หลังหยุด workspace รัน `bwoc doctor --auto` เพื่อล้าง stale-PID / stale-socket / stale-cursor ถ้า `bwoc list` เห็น agent ตัวเดียวยังทำงานในขณะที่ตัวอื่นหยุด — สอบถามก่อนเริ่มงานต่อ สำหรับการ "มาพร้อมกัน" แบบ *โต้ตอบ* ใช้ `bwoc fleet term` เปิด tmux หนึ่ง pane ต่อหนึ่ง agent ในเซสชันเดียว (จัดเรียงด้วย `--layout grid|columns|rows|main-vertical|main-horizontal` หรือ `--print` เพื่อพิมพ์คำสั่ง attach โดยไม่ต้อง attach) — เทียบได้กับ `start --all` แต่สำหรับ fleet ที่มีมนุษย์คอยดูแล ชื่อเซสชันเป็นค่าเฉพาะต่อ workspace โดยปริยาย จึงไม่ชนกันเมื่อเปิดหลาย fleet พร้อมกัน

### 3. ไม่บัญญัติ/ไม่ยกเลิกกติกาตามอำเภอใจ — *appaññattaṃ na paññāpenti*

> ไม่บัญญัติสิ่งที่ไม่ได้บัญญัติไว้, ไม่ยกเลิกสิ่งที่บัญญัติไว้แล้ว ตามอำเภอใจ

**การใช้:** การเปลี่ยน schema (manifest, workspace.toml, agents.toml, envelope JSONL) เป็น convention ที่ทุก agent ใน fleet พึ่งพา การเปลี่ยน schema ฝ่ายเดียวจาก agent ตัวหนึ่งทำให้ peer ทุกตัวพัง วินัย: schema evolution ต้องผ่าน spec doc ของ framework ไม่ใช่ ad-hoc edit ของ agent

**Signal:** ทุกไฟล์ที่มี schema ต้องมีฟิลด์ `schemaVersion` (เป็นจริงแล้วใน `trust.schemaVersion`; ควรขยายไป `workspace.toml`, `agents.toml`, envelope shape ใน v2) framework แจ้งเตือนเมื่อ agent ตัวใด `schemaVersion` ตามหลังพื้น workspace

**ปฏิบัติ:** ตั้งพื้น schemaVersion ระดับ workspace ใน `workspace.toml` (ฟิลด์ที่เสนอใน v2) framework serialize `trust.schemaVersion: 1` อยู่แล้ว — วินัยเดียวกันควรขยายไป schema อื่นบน disk การเพิ่ม / ถอนฟิลด์ manifest ที่ required = migration ระดับ workspace ไม่ใช่ per-agent change

### 4. เคารพลำดับชั้นเวอร์ชันของ template — *ye te bhikkhū vuḍḍhā vuḍḍhataravā*

> เคารพภิกษุผู้เถระ ฟังคำของท่าน

**การใช้:** template ของ agent คือผู้อาวุโส agent ที่ incarnate จาก template ใหม่ได้ประโยชน์จาก improvement; agent ที่ติดอยู่กับ fork เก่าพลาดสิ่งเหล่านั้น เคารพผู้อาวุโส = ทำให้ agent sync กับ template ที่ตัวเอง incarnate มา

**Signal:** `config.manifest.json::version` บันทึก template version ตอน incarnate `bwoc check` เปรียบเทียบกับ version ปัจจุบันของ template ได้ — flag agent ที่ตามหลัง major version

**ปฏิบัติ:** รัน `bwoc check --all` เป็นระยะ เทียบ `manifest.version` ของแต่ละ agent กับ `modules/agent-template/config.manifest.json::version` ตามหลัง major = signal สำหรับวางแผน migration; ตามหลัง minor / patch = informational framework ไม่ auto-migrate — operator เลือกว่า agent ตัวใดจะ re-incarnate หรืออัปเกรดบางส่วน

### 5. คุ้มครอง agent / user ที่เปราะบาง — *parihāra*

> ไม่ฉุดคร่าข่มเหง; คุ้มครองผู้เปราะบางในหมู่ตน

**การใช้:** agent บางตัวใน fleet แข็งแกร่งกว่า (skill มาก memory มาก compute มาก) ตัวอื่น Trust gating ([`trust.th.md`](../../modules/agent-template/interconnect/trust.th.md)) คุ้มครองผู้รับแต่ละคนจาก message ของ peer ตามอำเภอใจ เวอร์ชันระดับ fleet: ไม่มี agent ตัวใด — แข็งหรืออ่อน — ควรได้รับอนุญาตให้บังคับหรือ override การตัดสินใจของตัวอื่นโดยไม่ยินยอม

**Signal:** refusal record ใน `inbox.refusals.jsonl` คือ audit trail. fleet ที่มี refusal มากมายจาก sender ตัวเดียว = signal เรื่องการบังคับ — operator ควรสอบสวนพฤติกรรมของ sender ไม่กดดันผู้รับให้ผ่อน `requiredTrust`

**ปฏิบัติ:** ถือ refusal ของผู้รับเป็นสิทธิ์อันชอบธรรม แม้ operator อยากให้ peer รับ ไม่ override `requiredTrust` เพื่อ "ทำให้ flow ผ่าน" — refusal คือชั้นป้องกัน สอบสวนหลักฐาน `trust.declared` ของผู้ส่ง (บ่อยครั้ง manifest อ้าง quality ที่ไม่มี signal ใน repo มารองรับ) ให้ผู้ส่งได้มาด้วยการกระทำ

### 6. เคารพทรัพยากรร่วม — *cetiya / สถาน*

> ทำสักการะแก่ปูชนียสถาน ทั้งภายในและภายนอก

**การใช้:** ทรัพยากรร่วมระดับ workspace — `agents.toml` registry, `workspace.toml` config, `notes/` ของ workspace, template ใต้ `modules/agent-template/` — เป็นปูชนียสถาน ทุก agent อ่านพวกมัน; เฉพาะ operator (หรือ migration ที่ประสานงาน) เขียน agent ที่ขีดเขียนใส่ state ร่วมเองตามใจ = ลบหลู่สถาน

**Signal:** `bwoc workspace prune` ตรวจ drift ระหว่าง registry กับ disk อยู่แล้ว git history ของ `.bwoc/` และ `modules/agent-template/` คือ audit trail ระยะยาว การเปลี่ยนแปลงไฟล์ร่วมที่ไม่ระบุที่มาบ่อยๆ = signal ว่ามีการเขียนที่ไม่ประสาน

**ปฏิบัติ:** ถือ `agents.toml`, `workspace.toml`, และ template directory เป็น operator-owned. agent อ่านได้แต่ไม่แก้นอก incarnation ของตน ใช้ `bwoc retire` / `bwoc new` สำหรับ mutation ใน registry ไม่ใช่แก้ไฟล์ตรง ตรวจ git diff บน path เหล่านี้ด้วยความระมัดระวังพิเศษ

### 7. คุ้มครอง agent อาวุโส / น่าเชื่อถือ — *arahantesu rakkhāvaraṇa-gutti*

> คุ้มครองพระอรหันต์ เพื่อให้มีท่านเสด็จมาเพิ่มขึ้น

**การใช้:** agent อาวุโส — มี memory ลึก, trust สูง, capability หายาก — มีค่าต่อ fleet อย่างไม่สมส่วน การสูญเสียพวกเขา = ความถอยหลังเชิงโครงสร้าง ธรรมาภิบาล fleet รวมถึงการคุ้มครองอย่างชัดเจน: backup, แผนสืบทอด, ไม่ `bwoc retire` agent ที่ trust สูงตามใจ

**Signal:** `bwoc trust <agent> --json` คืน declared block; `bwoc check` ตรวจหลักฐาน agent "อาวุโส" ใน fleet = ผู้ที่มี peer ที่ `requiredTrust = []` พึ่งพา การถอด agent แบบนี้ควรต้องการการยืนยันจาก operator มากกว่า `--yes` มาตรฐาน

**ปฏิบัติ:** ก่อน `bwoc retire <agent>` ของ agent ที่ประกาศ trust quality ให้รัน `bwoc inbox <every-other-agent>` แล้ว grep หา agent-id นั้นใน traffic ที่ผ่านมา ถ้า peer พึ่งพา agent ตัวนั้น — วางแผนการ retire: archive `memories/` (ใช้ `bwoc retire --keep-memory`), แจ้ง peer (operator ส่งข้อความเอง), และ migrate ความรับผิดชอบใดๆ ไม่ retire แบบเงียบๆ

## สุขภาพ Fleet ที่สังเกตได้

Signal เหล่านี้รวมกันให้มุมมองสุขภาพ fleet แก่ operator ตอนนี้ `bwoc fleet health` รวมทั้งเจ็ดข้อไว้ใน sweep แบบอ่านอย่างเดียวคำสั่งเดียวแล้ว ส่วน query รายเงื่อนไขด้านล่างยังมีประโยชน์สำหรับเจาะดูทีละ signal

| ข้อ | Query | การอ่านที่สุขภาพดี |
|---|---|---|
| 1. ประชุมเนืองนิตย์ | `bwoc list --json` รายวัน | ไม่มี agent ไม่ถูกแตะ > N วัน |
| 2. เริ่ม-เลิกพร้อมกัน | `bwoc doctor --auto` หลังหยุด | ไม่มี stale-PID / stale-socket |
| 3. กติกามีกระบวนการ | `git log -- .bwoc/ modules/agent-template/` | schema bump ประสาน operator ลงนาม |
| 4. เคารพ template version | `bwoc check --all` | `manifest.version` ตรงกับ template version |
| 5. คุ้มครองผู้เปราะบาง | `bwoc inbox <agent> --json \| jq '.[] \| select(.refused)'` | refusal คงที่หรือลดลงตามเวลา |
| 6. เคารพทรัพยากรร่วม | `git blame .bwoc/agents.toml` | เฉพาะการเปลี่ยนที่ operator เขียน |
| 7. คุ้มครอง agent อาวุโส | audit ผ่าน `bwoc trust <agent> --json` | agent อาวุโสมี backup + แผนสืบทอด |

ไม่มีอันใดเป็น gate ในวันนี้ ทั้งหมดเป็น *practice* ที่ operator รันตามจังหวะที่เหมาะกับขนาดและความเสี่ยงของ fleet

## สิ่งที่ Spec นี้ ไม่ ครอบคลุม

- **ธรรมาภิบาลข้าม workspace** spec นี้ scope ต่อต้นไม้ `.bwoc/workspace.toml` หนึ่ง federation หลาย workspace เป็น design problem ของตัวเอง (Phase 4+ vision territory — ดู [`VISION.md`](../../VISION.md))
- **การบังคับแบบอัตโนมัติ** v1 spec เป็น descriptive: ตั้งชื่อข้อ, signal, ปฏิบัติ การยกระดับ signal เป็น hard gate ("CI fail ถ้า agent ตัวใด `manifest.version` ตามหลัง > 2 major release") เกิดทีละขั้นเมื่อ telemetry สนับสนุน
- **ธรรมาภิบาลของทีมมนุษย์** อปริหานิยธรรม 7 เดิมใช้กับชุมชนมนุษย์; BWOC ปรับมาใช้กับ agent fleet framework ไม่กำหนดว่าทีมมนุษย์ของ operator จะประสานงานกันรอบ fleet อย่างไร — นั่นเป็นทางเลือกของ operator
- **การ adopt framework โดย ecosystem** DoD ของ Phase 4 รวมถึง "Reference agent 3+ ตัวในโลกจริง สร้างโดย maintainer นอกผู้เขียนต้นฉบับ" และ "BWOC vocabulary ปรากฏใน codebase ที่ไม่เกี่ยวกับ project นี้" สิ่งเหล่านั้นเกิดจาก maintainer ภายนอกรับ framework ไปใช้ ไม่ใช่จาก spec ที่เราเขียน spec นี้ทำให้การ adopt *เป็นไปได้* โดยให้ operator มี vocabulary ของธรรมาภิบาลที่ coherent; spec เองไม่ทำให้ Phase 4 บรรลุได้คนเดียว

## ประวัติการแก้ Spec

- **v1 / 2026-05-23 (ฉบับร่างแรก):** 7 ข้อ map ไปยังการทำงาน fleet ของ BWOC. ตั้งชื่อ observable signals; เลื่อน automation parity TH กับ [`FLEET-GOVERNANCE.en.md`](../en/FLEET-GOVERNANCE.en.md)

## เอกสารอ้างอิง

- [`modules/agent-template/docs/en/PHILOSOPHY.en.md` #20. Aparihāniya-dhamma 7](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) — แผนที่เชิงปรัชญาที่ spec นี้ทำให้ใช้งานได้
- [`modules/agent-template/interconnect/trust.md`](../../modules/agent-template/interconnect/trust.md) — Kalyāṇamitta 7 peer trust (ระดับ per-agent); ธรรมาภิบาล fleet ประกอบกับมัน
- [`modules/agent-template/interconnect/messaging.md`](../../modules/agent-template/interconnect/messaging.md) — สาราณียธรรม 6 peer cordiality; คู่หูระดับ agent ของธรรมาภิบาลระดับ operator
- [`WORKSPACE.th.md`](WORKSPACE.th.md) — รูปแบบ workspace `.bwoc/` ที่ธรรมาภิบาลนี้ทำงานบน
- [`ROADMAP.th.md` Phase 4](ROADMAP.th.md) — ที่ spec นี้อยู่ในแผน phase ใหญ่
- ทีฆนิกาย 16 §1.4 — แหล่งดั้งเดิม ([SuttaCentral DN 16](https://suttacentral.net/dn16))