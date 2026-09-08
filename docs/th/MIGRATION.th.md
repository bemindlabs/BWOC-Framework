---
title: การย้ายเวอร์ชัน
aliases:
  - Upgrade Guide
  - การย้ายไป 3.0
tags:
  - group/framework
  - type/process
  - meta/operations
parent: ภาษาไทย
nav_order: 9
---

# การย้ายเวอร์ชัน

> [!abstract] การย้าย installation ข้าม major version

หนึ่ง section ต่อหนึ่ง major เริ่มที่ section ที่ตรงกับ release ที่กำลังจะอัปเกรด *ไปหา*

---

# 2.x → 3.0

> [!abstract] อัปเกรด installation 2.x — ส่วนใหญ่จบด้วยคำสั่งเดียว

3.0 อ่านทุกอย่างที่ 2.x เขียนไว้ได้ ฉะนั้น **ไม่มีอะไรพังในวินาทีที่อัปเกรด binary**
การ migrate เป็นสิ่งที่ทำเมื่อสะดวก ก่อนที่ 4.0 จะตัด schema เก่าออก ดูหน้าต่างการรองรับ
ที่ [`COMPATIBILITY.th.md`](COMPATIBILITY.th.md#หน้าต่างการรองรับ-support-window)

---

## ฉบับสั้น

```bash
bwoc migrate --all --dry-run   # ดูว่าจะเปลี่ยนอะไรบ้าง
bwoc migrate --all             # ลงมือ; ต้นฉบับถูกเก็บไว้ใน .bwoc/migrate-backup/
bwoc check --all               # ยืนยัน: ไม่เหลือ warning เรื่อง schema
```

ถ้าดูแลปลั๊กอินอยู่ ให้ใส่ `compat` range ที่มีขอบบนให้แต่ละตัวด้วย — ดู [ปลั๊กอิน](#ปลั๊กอิน) ด้านล่าง

---

## อะไรเปลี่ยนบ้าง

### 1. ทุก artifact ที่ BWOC เป็นเจ้าของประกาศ schema ของตัวเอง

`workspace.toml`, `agents.toml`, `routes.toml`, `harness-policy.toml` และ
`peers.toml` ได้ key ระดับบนสุด `schema_version = 3` artifact ที่ไม่มี key นี้จะถูกอ่าน
เป็น schema 2 — ยังใช้ได้ใน 3.x, มี warning, และถูกตัดออกใน 4.0

ผลในทางปฏิบัติ: BWOC แยกไฟล์เก่ากับไฟล์ใหม่ออกจากกันได้แล้ว ใน 2.x มันแยกไม่ได้
ซึ่งแปลว่าการเปลี่ยน format ใด ๆ ในอนาคตจะพังแบบเงียบ ๆ ได้ทางเดียว

### 2. Specification เป็น 3.0

`config.manifest.json` ของ agent ประกาศ `"version": "3.0"` โดยมีแถว
`| **Version** | 3.0 |` ใน `AGENTS.md` เป็นกระจกสะท้อน key นี้มีมาตั้งแต่ 2.x
แต่ไม่เคยถูกอ่าน — ตอนนี้ `bwoc check` validate มันแล้ว

**ไม่มี section ไหนใน `AGENTS.md` เปลี่ยน** เวอร์ชัน specification ขยับเพราะ
*contract รอบตัวมัน* เปลี่ยน (มันถูกบังคับใช้แล้ว) ไม่ใช่เพราะ agent ต้องถูกเขียนใหม่
การ migrate แก้สองบรรทัดต่อ agent

### 3. `[plugin].compat` ถูกบังคับใช้

`PLUGINS.th.md` บอกมาตลอดว่าเฟรมเวิร์กปฏิเสธการโหลดปลั๊กอินที่ `compat` range
ไม่ครอบเวอร์ชันเฟรมเวิร์กที่รันอยู่ ซึ่งมันไม่เคยทำ ตอนนี้ทำแล้ว และ range ต้องมีขอบบน

### 4. ไฟล์ control-plane สองตัว fail closed เมื่อเจอ schema ใหม่กว่า

`harness-policy.toml` และ `peers.toml` จะถูกปฏิเสธถ้าประกาศ schema ที่ build นี้ไม่รู้จัก
แทนที่จะอ่านโดยข้ามส่วนที่ไม่รู้จัก เรื่องนี้กระทบก็ต่อเมื่อคุณ downgrade binary
ลงมาใต้ workspace ที่ binary ใหม่กว่าเขียนไว้

---

## วิธี migrate

### ทั้ง workspace

```bash
bwoc migrate --all --dry-run
```

รายงานทีละ artifact ว่าจะกลายเป็นอะไร โดยไม่เขียนอะไรเลย จากนั้น:

```bash
bwoc migrate --all
```

- **Idempotent** — รันซ้ำครั้งที่สองไม่เปลี่ยนอะไร
- **ไม่ทำลายของเดิม** — ต้นฉบับถูกคัดลอกไปที่
  `<root>/.bwoc/migrate-backup/<timestamp>/` โดยคง path เดิมไว้ การกู้คืนคือ `cp -r`
  ทับกลับลง root ใส่ `--no-backup` ถ้าไม่ต้องการ
- **รักษาคอมเมนต์** — ไฟล์ถูกแก้แบบข้อความ ไม่ใช่ reserialize ฉะนั้นคอมเมนต์
  ลำดับ key และ key ใด ๆ ที่ BWOC ไม่ได้ model ไว้ (`[plugins.*]` ใน `workspace.toml`,
  `skills.framework[]` ใน manifest) รอดครบ

### ทีละ agent หรือทีละ workspace

```bash
bwoc migrate ./agents/agent-foo     # ไดเรกทอรีของ agent
bwoc migrate /path/to/workspace     # root ของ workspace
```

`bwoc migrate` ตรวจเองว่าเป็นอะไร: workspace root มี `.bwoc/workspace.toml`
ส่วน agent มี `config.manifest.json`

### ใน script

```bash
bwoc migrate --all --json --yes
```

`--json` ต้องใช้คู่กับ `--yes` เพราะมันเขียนไฟล์โดยไม่ถาม รูปของรายงาน:

```json
{
  "targets": [ { "path": "...", "from": "2", "to": "3", "action": "migrated",
                 "backup": "..." } ],
  "summary": { "migrated": 6, "already_current": 0, "failed": 0, "ahead": 0,
               "dry_run": false },
  "schema": { "current": 3, "oldest_supported": 2, "spec": "3.0" }
}
```

Exit code: `0` สำเร็จหรือไม่มีอะไรต้องทำ · `1` มีไฟล์ที่อ่านหรือเขียนไม่ได้ ·
`2` ไม่เจอ workspace หรือเรียกผิด · `3` มี artifact ที่ประกาศ schema ใหม่กว่า build นี้
(ให้อัปเกรด `bwoc`)

---

## ปลั๊กอิน

ทุกปลั๊กอินต้องมี `compat` range ที่ **มีขอบบน**:

```toml
compat = ">=3.0.0, <4.0.0"     # ไม่ใช่ ">=3.0.0"
```

range ที่เปิดปลายอ้างว่าเข้ากันได้กับทุก major ในอนาคต รวมถึง major ที่ทำให้ปลั๊กอินพัง
`bwoc check` เตือนเรื่องนี้ ส่วน range ที่ไม่ครอบเวอร์ชันเฟรมเวิร์กที่รันอยู่จะทำให้
ปลั๊กอินปฏิเสธการโหลด พร้อมบอก path ของ manifest และ range ที่ไม่ตรงใน error

`bwoc migrate` **ไม่** แก้ `compat` ให้ การประกาศว่าปลั๊กอินใช้กับเฟรมเวิร์กรุ่นไหนได้
เป็นคำยืนยันของผู้เขียนปลั๊กอิน การให้เครื่องมือปลอมคำยืนยันนั้นให้ก็ทำลายเหตุผลที่ถามตั้งแต่แรก

---

## การตรวจสอบ

```bash
bwoc check --all       # ไม่เหลือ warning "specification version 2.0"
bwoc doctor            # สุขภาพ workspace
bwoc list              # agent ยัง resolve ได้
```

workspace ที่ migrate แล้วยังถูกอ่านได้ด้วย binary 2.x — ไม่มีอะไรใน BWOC ใช้
`deny_unknown_fields` build เก่าจึงข้าม key `schema_version` ไป ถือว่านี่เป็นตาข่ายรองรับ
กรณีอัปเกรดพลาด ไม่ใช่ configuration ที่รองรับอย่างเป็นทางการ

---

## ถ้ามีอะไรผิดพลาด

**กู้จาก backup** — การรันแต่ละครั้งเขียนไดเรกทอรีที่มี timestamp หนึ่งชุดต่อ root:

```bash
cp -r <root>/.bwoc/migrate-backup/<timestamp>/. <root>/
```

**มีไฟล์ migrate ไม่สำเร็จ** (`action: "failed"`) รายงานบอกเหตุผลไว้ กรณีที่พบบ่อยที่สุด
คือไฟล์ที่เสียอยู่ก่อนแล้ว — `migrate` ปฏิเสธที่จะเขียนอะไรที่มัน re-parse ไม่ได้
ฉะนั้นไฟล์ที่มันแก้ไม่ได้คือไฟล์ที่มันไม่ได้แตะ

**มี artifact ที่ "ahead"** (exit 3) มันถูกเขียนโดย `bwoc` ที่ใหม่กว่าตัวที่คุณรันอยู่
ให้อัปเกรด binary อย่าแก้ไฟล์ให้ต่ำลงด้วยมือ

---

## ดูเพิ่ม

- [`COMPATIBILITY.th.md`](COMPATIBILITY.th.md) — หน้าต่างการรองรับ และอะไรนับเป็น breaking
- [`PLUGINS.th.md`](PLUGINS.th.md#stability) — compatibility surface ฝั่งปลั๊กอิน
- [`WORKSPACE.th.md`](WORKSPACE.th.md) — ไฟล์แต่ละตัวใน workspace มีไว้ทำอะไร
- [`CHANGELOG.md`](../../CHANGELOG.md) — entry ของ release 3.0
