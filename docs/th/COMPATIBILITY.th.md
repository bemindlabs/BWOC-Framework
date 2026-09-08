---
title: ความเข้ากันได้
aliases:
  - Compatibility Policy
  - Support Window
tags:
  - group/framework
  - type/process
  - meta/operations
parent: ภาษาไทย
nav_order: 8
---

# ความเข้ากันได้ (Compatibility)

> [!abstract] BWOC สัญญาอะไรไว้เรื่องการทำของพัง และไม่ได้สัญญาอะไร

นี่คือ contract ที่อยู่เบื้องหลังแถว MAJOR ใน [`VERSION.md`](../../VERSION.md#cargo-semver-bump-rules)
มันมีอยู่เพราะ 2.x ไม่เคยมี contract แบบนี้: breaking change ถูกบันทึกไว้ใน
`CHANGELOG.md` แล้วที่เหลือปล่อยให้ไปเจอเอาเอง

---

## อะไรคือ public surface

มีสามอย่าง breaking change ต่อสิ่งใดในนี้คือ MAJOR bump นอกนั้นไม่ใช่

| Surface | อยู่ที่ไหน | "breaking" แปลว่าอะไร |
|---|---|---|
| **Specification** | [`AGENTS.md`](../../modules/agent-template/AGENTS.md) ซึ่ง version ถูก mirror ไว้ใน `VERSION.md` §Specification | section ที่ `AGENTS.md` ของ agent เดิมต้องมีเพิ่ม หรือ section ที่ความหมายเปลี่ยน |
| **Schema บนดิสก์** | `config.manifest.json` และ control plane ใน `.bwoc/` — ดู [Artifact ที่มี version](#artifact-ที่มี-version) | field ที่กลายเป็นบังคับ เปลี่ยนความหมาย ย้ายที่ หรือถูกลบ |
| **CLI** | ทุก subcommand / flag / รูป `--json` / exit code ของ `bwoc` ที่มีเอกสาร | คำสั่งหรือ flag ที่ถูกลบหรือเปลี่ยนชื่อ; key ใน `--json` ที่หายไปหรือเปลี่ยน type; exit code ที่เปลี่ยนความหมาย |

**ที่ไม่ใช่ public อย่างชัดเจน:** Rust API — crate ไม่ได้ publish ขึ้น crates.io,
module ของ `bwoc-core` เป็น `pub` เพื่อ binary ใน workspace นี้เท่านั้น และ MINOR release
รื้อมันได้อิสระ ถ้าวันหนึ่งเรื่องนี้เปลี่ยน ตารางนี้จะได้แถวที่สี่ก่อน

ที่ไม่ใช่ public เช่นกัน: layout ภายใต้ `target/`, รูปแบบ log, ถ้อยคำของ output
แบบ human-readable (ที่ไม่ใช่ `--json`) และอะไรก็ตามที่เอกสารระบุว่าเป็น experimental

---

## Artifact ที่มี version

ทุก format ที่ BWOC เป็นเจ้าของประกาศว่า revision ไหนเป็นคนเขียน — `schema_version`
ใน TOML, `schemaVersion` ใน JSON (แต่ละไฟล์ใช้ casing ของตัวเอง) กติกาถูก implement
ไว้ที่เดียวใน [`bwoc-core::schema`](../../crates/bwoc-core/src/schema.rs)

| Artifact | Marker |
|---|---|
| `.bwoc/workspace.toml` | `schema_version` |
| `.bwoc/agents.toml` | `schema_version` |
| `.bwoc/interconnect/routes.toml` | `schema_version` |
| `.bwoc/harness-policy.toml` | `schema_version` |
| `.bwoc/peers.toml` | `schema_version` |
| `config.manifest.json` | `version` (เวอร์ชัน specification — `3.0`) และ `trust.schemaVersion` สำหรับ sub-spec Kalyāṇamitta-7 ซึ่งมี version แยกของตัวเอง |

**ไม่มี marker = schema 2** — ทุกอย่างที่ BWOC 2.x เขียนไว้ นี่คือสิ่งที่ทำให้การ upgrade
ไม่ทำลายของเดิม และเป็นเหตุผลที่ marker เป็น `#[serde(default)]` ไม่ใช่ required

ที่ตั้งใจไม่ใส่ version เพราะไม่มีผู้อ่านคนไหนทำอะไรกับ marker ได้: `.bwoc/doc-kinds.toml`
(เป็น additive; ถ้า parse ไม่ได้ก็ degrade เป็น "ไม่มี custom kind" อยู่แล้ว),
`.bwoc/secrets.toml` (ที่เก็บ secret), `.bwoc/installed-sources.toml` และ
`.bwoc/teams/*.toml` (ถูกเขียนทับทั้งไฟล์โดยคำสั่งที่เป็นเจ้าของ) และทุก `*.jsonl`
ที่เป็น append-only stream

---

## หน้าต่างการรองรับ (support window)

**เหลื่อมกันหนึ่ง major**

- release ในสาย **3.x** อ่านได้ทั้ง schema 2 และ schema 3 การอ่าน artifact schema 2
  เป็น warning ไม่ใช่ error เสมอ และ warning นั้นบอกชื่อ `bwoc migrate`
- **4.0** ตัด schema 2 ทิ้ง workspace ที่ยังไม่ migrate จะ load ไม่ผ่าน แทนที่จะถูกอ่านผิดเงียบ ๆ

หน้าต่างจึงกินตั้งแต่วันที่ 3.0 ออก จนถึงวันที่ 4.0 ออก — ไม่ใช่จำนวนเดือนตายตัว
เพราะ BWOC ไม่ได้สัญญาความถี่ของ release ไว้ และการสัญญาเป็นหน่วยเวลาแทนหน่วยเวอร์ชัน
คือการสัญญาในสิ่งที่โครงการนี้รักษาไม่ได้

### การอ่านไปข้างหน้า

Artifact ที่ประกาศ revision **ใหม่กว่า** build ที่รันอยู่จะถูกปฏิเสธ ไม่ใช่เดา
สำหรับ `.bwoc/harness-policy.toml` เป็น error ส่วน `.bwoc/peers.toml` peer จะกลายเป็น
unpinned ทั้งคู่ fail closed โดยตั้งใจ: ไฟล์เหล่านี้ตัดสินว่า turn หนึ่งทำอะไรได้บ้าง
และ signature ของใครนับว่า verified ฉะนั้น key ที่ build นี้ตีความไม่ได้จะกลายเป็น
การให้สิทธิ์ที่ผู้ดูแลไม่เคยเขียนไว้

### การอ่านย้อนหลัง (downgrade)

การกลับไปใช้ `bwoc` รุ่นเก่าทำได้ตราบใดที่ *เนื้อหา* ยังเข้ากันได้: ไม่มี struct ไหน
ใน workspace ใช้ `deny_unknown_fields` build เก่าจึงข้าม `schema_version` ที่ไม่รู้จัก
แล้วทำงานต่อได้ นี่เป็นความเอื้อเฟื้อของ implementation ไม่ใช่คำสัญญา — schema revision
ที่เปลี่ยน *ความหมาย* ของ field จะทำให้ downgrade พังไม่ว่า parser จะใจกว้างแค่ไหน

---

## Deprecation

public surface จะถูกลบก็ต่อเมื่อผ่าน release ที่ทั้งยังให้มันทำงานได้ และบอกว่ามันกำลังจะหายไป

1. **ประกาศ** — entry ใน `CHANGELOG.md` ของ release ที่ deprecate บอกว่าอะไรถูก deprecate
   อะไรมาแทน และ major ไหนจะลบมันออก
2. **เตือนในเครื่องมือ** — `bwoc check` รายงานเป็น warning ไม่ใช่ violation เพราะ `check`
   exit non-zero เมื่อมี violation และการทำให้ installation เดิมแดงในวันที่มีคน upgrade
   ไม่ใช่ deprecation แต่คือการลบทิ้งที่มีขั้นตอนเพิ่ม
3. **ลบที่ major ถัดไป** ซึ่งมันจะกลายเป็น violation หรือ error

ปลั๊กอินมี contract รุ่นของตัวเอง: `[plugin].compat` เป็น semver range ที่มีขอบบน
บังคับใช้ตั้งแต่ 3.0 ดู [`PLUGINS.th.md` §Stability](PLUGINS.th.md#stability)

---

## เวอร์ชันที่รองรับ

รองรับเฉพาะ release ล่าสุด ไม่มี maintenance branch: fix ลง `main` แล้วออกใน release ถัดไป
และ security fix เป็นเหตุผลให้ *ตัด release* ไม่ใช่ให้ backport (ดู [`SECURITY.md`](../../SECURITY.md))

นี่เป็นผลที่ตั้งใจของโมเดล release — tag ถูกตัดบน `main` โดยตรง และ `CONTRIBUTING.md`
ห้าม branch `release/*` การรองรับสายเก่าจะต้องใช้ branch ที่โครงการนี้เลือกจะไม่มี

---

## ดูเพิ่ม

- [`VERSION.md`](../../VERSION.md) — namespace ของ version และกติกาการ bump
- [`MIGRATION.th.md`](MIGRATION.th.md) — ย้าย installation 2.x ไป 3.0
- [`RELEASING.th.md`](RELEASING.th.md) — วิธีตัด release
- [`PLUGINS.th.md`](PLUGINS.th.md) — compatibility surface ฝั่งปลั๊กอิน
