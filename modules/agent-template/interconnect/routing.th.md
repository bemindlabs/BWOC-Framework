---
title: การกำหนดเส้นทางข้าม Workspace
aliases:
  - Routing
  - Interconnect Routing
  - อนัตตา / ไม่มีศูนย์กลาง
tags:
  - group/agents
  - type/design
  - meta/template
status: draft (v2026.5.23 — spec เท่านั้น; ยังไม่มีโค้ด)
canonical-source: SN 22.59 (อนัตตลักขณสูตร) — สังยุตตนิกาย 22.59
---

# การกำหนดเส้นทางข้าม Workspace

> [!abstract] วันนี้ `bwoc send` ส่งถึง agent ได้เฉพาะใน registry ของ workspace **เดียวกัน** Routing เพิ่มตารางที่ workspace ประกาศเอง (`.bwoc/interconnect/routes.toml`) เพื่อให้ข้อความถึง agent ใน workspace **peer** ได้ — โดยไม่มี broker กลาง แต่ละ workspace ประกาศ peer ของตัวเอง ไม่มีใครเป็นศูนย์กลางที่พิเศษกว่า นี่คือการอ่าน **อนัตตา** เชิงโครงสร้าง: routing mesh ที่ไม่มีตัวตนควบคุมถาวร

## แรงจูงใจ

นิยามของเสร็จ (DoD) ของ Phase 3 มีสองครึ่ง: *ชีวิตของ agent จบลงอย่างสะอาด* (วยะ — ดู [`retire`](../../../crates/bwoc-cli/src/retire.rs)) และ *agent ประสานงานโดยไม่มีศูนย์กลาง* Trust ([`trust.md`](trust.md)) กับ messaging ([`messaging.md`](messaging.md)) ให้ช่องทางที่ verify ได้ **ภายใน** workspace เดียว แต่ข้ามขอบเขต workspace ไม่ได้: `send` resolve ผู้รับจาก `AgentsRegistry::load(&workspace)` อันเดียว แล้ว append ลง tree ของ workspace นั้น ([`send.rs:88-124`](../../../crates/bwoc-cli/src/send.rs))

"ไม่มีศูนย์กลาง" คือข้อจำกัดการออกแบบที่ตัดทางแก้แบบตรงไปตรงมา (directory ของ agent กลางที่ทุก workspace มาดู) ออกไป แทนที่ด้วยการที่แต่ละ workspace **ประกาศ peer ที่ตัวเองรู้จัก** และ routing คือ union ของการประกาศ local เหล่านั้น — ไม่ใช่แผนที่อันเดียวที่ถูกถือครอง นี่คือหลักการไม่ถือครองเดียวกันที่ framework ใช้กับ branch และ worktree อยู่แล้ว (อนัตตาในความหมายวยะ) หันมาใช้กับ topology: **ไม่มี node ไหนเป็นเจ้าของ mesh**

| หลักการ | ในระบบ | แมปกับ |
|---|---|---|
| **อนัตตา** (ไม่มีตัวตนศูนย์กลาง) | ไม่มี directory กลาง ไม่มี broker; แต่ละ workspace เป็น locus ของตัวเอง | [`PHILOSOPHY.en.md` #4 Tilakkhaṇa](../docs/en/PHILOSOPHY.en.md) |
| **สมานัตตตา** (ยืนเท่ากัน) | ทุก peer workspace เท่ากัน — routing ไม่ให้สิทธิพิเศษกับใคร | [`PHILOSOPHY.en.md` #12 Saṅgahavatthu 4](../docs/en/PHILOSOPHY.en.md) |

> [!note] operator ยืนยัน mapping แล้ว (2026-05-23): canonical anchor คือ **SN 22.59 / อนัตตา** (ไม่มีตัวตนศูนย์กลาง → ไม่มี broker กลาง) โดยมี สมานัตตตา เป็นหลักการเสริม พิจารณาแล้วไม่เลือก: ซ้อนใต้ Saṅgaha ([`sangha.md`](sangha.md)) หรือ Aparihāniya-dhamma ([`FLEET-GOVERNANCE.en.md`](../../../docs/en/FLEET-GOVERNANCE.en.md))

ข้อจำกัดการออกแบบสามข้อสำหรับ v1:

1. **ประกาศเอง ไม่ใช่ค้นพบ** workspace เข้าถึงได้เฉพาะ peer ที่ list ไว้ ไม่มี broadcast ไม่มี gossip ไม่มี service registry การเพิ่ม peer คือการแก้ local อย่างชัดแจ้ง
2. **Additive — พฤติกรรมเดิมเป็น fallback** local registry lookup ยังเป็น fast path และถูกลองก่อน; routing ถูกเรียกเฉพาะตอน miss ไม่มี flow single-workspace เดิมเปลี่ยน
3. **Local filesystem เท่านั้น** v1 ส่งถึง peer workspace ที่เข้าถึงได้บน local filesystem network transport (ssh, http) เลื่อนออกไปพร้อมงาน [Trust v2](trust.md) cross-workspace

## ตาราง Routing

`.bwoc/interconnect/routes.toml`, หนึ่งไฟล์ต่อ workspace ไม่มีไฟล์ ≡ ไม่มี peer ≡ พฤติกรรมวันนี้

```toml
# แต่ละ route บอก `bwoc send` ว่าจะส่งข้อความสำหรับผู้รับ
# ที่ไม่อยู่ใน local registry ไปที่ไหน ไม่มี directory กลาง —
# ไฟล์นี้คือการประกาศของ workspace เองว่าเอื้อมถึงใครได้

[[route]]
agent = "agent-neo"                 # recipient id แบบ exact
workspace = "/abs/path/to/peer-ws"  # peer workspace root (local FS)

[[route]]
namespace = "team-b"                # หรือ prefix: route ผู้รับ `team-b-*` ใดก็ได้
workspace = "/abs/path/to/team-b-ws"
```

route หนึ่งจะเป็น `agent` (id แบบ exact) **หรือ** `namespace` (prefix) — ไม่ใช่ทั้งสอง `workspace` คือ root directory ของ peer (อันที่ถือ `.bwoc/agents.toml` ของมัน)

## ลำดับการ Resolve

`send` resolve ผู้รับใน 3 ขั้น match แรกชนะ; พฤติกรรมเป็น additive ล้วน

1. **Local registry** — `AgentsRegistry::load(&workspace)`, `find(id == lookup_id)` fast path เดิม hit → ส่ง local เหมือนวันนี้
2. **ตาราง routing** — เมื่อ local miss ให้ load `routes.toml`:
   - exact `agent` match → resolve peer `workspace`, load registry **ของ peer นั้น**, หาผู้รับ, append ลง `<agent>/.bwoc/inbox.jsonl` ของ peer
   - ไม่งั้น match `namespace` prefix ที่ยาวสุด → เช่นเดียวกัน
3. **ไม่ match** — error `NotFound { name, workspace }` เดิม ไม่เปลี่ยน

> [!example] `bwoc send agent-neo "ping" --from agent-oracle` เมื่อ `agent-neo` ไม่อยู่ local แต่ `routes.toml` มี `agent="agent-neo", workspace="/srv/ws-b"` → envelope ลงที่ `/srv/ws-b/<agent-neo path>/.bwoc/inbox.jsonl`, `from = "agent-oracle"`

## การ Compose กับ Trust — ทำไม Routing ship ก่อน Trust v2

cross-workspace delivery กับ trust gate compose กันเป็น **safe default** โดยไม่ต้องเพิ่มโค้ดใน gate เลย:

- trust check ของ daemon ฝั่งรับ resolve `from` ใน envelope กับ registry **ของตัวเอง** ([`trust.md` §Refusal Semantics](trust.md))
- sender ข้าม workspace ไม่อยู่ใน registry ของผู้รับ → resolve เป็น `unknown_sender` → refused ลง `inbox.refusals.jsonl` (envelope ถูกเก็บไว้ ไม่ถูกลบ)
- ดังนั้นเมื่อ `BWOC_TRUST_GATING=1` ข้อความข้าม workspace จาก sender ที่ไม่รู้จักจะ **ถูก refuse โดย default** — เป็นท่าทีอนุรักษ์นิยมที่ต้องการพอดีก่อนที่ identity จะพิสูจน์ได้ ถ้าไม่ได้ตั้ง `=1` (default ของ framework คือ gating แบบ warn เท่านั้น) quality gate จะไม่ refuse

ดังนั้น routing **ไม่** block บน [Trust v2](trust.md): สอง feature นี้ orthogonal และ interaction ถูกต้องตามที่เป็นอยู่

> [!warning] seam ที่ฝากไว้ให้ Trust v2 envelope `from` เป็น bare id (`agent-oracle`) ข้าม workspace แล้ว bare id กำกวมและพิสูจน์ไม่ได้ v2 ควรใส่ identity ที่ workspace-qualified และ signed (เช่น `agent-oracle@ws-b`) เพื่อให้ sender ข้าม workspace ถูก *trust* ได้ ไม่ใช่แค่ถูก *refuse* v1 คง `from` เป็น bare และ mark seam นี้ไว้ — อย่าขยาย envelope schema เพื่อ routing อย่างเดียว

## สิ่งที่ Spec นี้ไม่ครอบคลุม

- **Network transport** peer บน local filesystem เท่านั้น transport แบบ ssh/http/queue เลื่อนออก (Trust v2 cross-workspace)
- **Discovery** ไม่มี peer discovery อัตโนมัติ, broadcast, หรือ gossip peer ถูกประกาศด้วยมือ
- **Cross-workspace trust** routing ส่งให้; การ *trust* sender ข้าม workspace เป็นเรื่องของ Trust v2 (ดู seam ข้างบน) จนกว่าจะถึงตอนนั้น sender ข้าม workspace คือคนแปลกหน้า (ถูก refuse เมื่อ `BWOC_TRUST_GATING=1`)
- **การป้องกัน loop / cycle** v1 ทำ single hop (local → peer หนึ่งตัว) การ forward หลาย hop (A→B→C) อยู่นอก scope; route resolve ไปยัง workspace ปลายทาง ไม่ใช่ตาราง routing อีกอัน
- **routing ฝั่งอ่าน** `bwoc inbox` ยังอ่าน inbox ของ agent local ตัวเอง routing คุม `send` (delivery) ไม่ใช่การอ่านข้าม workspace

## ประวัติการแก้ไข Spec

- **v1 / 2026-05-23 (ร่างแรก, Oracle):** schema `routes.toml` (`agent` | `namespace` → peer `workspace`), resolution 3 ขั้นแบบ additive, scope v1 local-FS, safe-default จากการ compose กับ trust, seam สำหรับ Trust v2 mapping (อนัตตา / SN 22.59) operator ยืนยันแล้ว 2026-05-23

## ลำดับการ Implement (เมื่อเริ่มงานโค้ด)

1. `bwoc-core`: type `Routes` ที่ deserialize `routes.toml` (`Vec<Route>`; แต่ละ `Route` เป็น `agent` xor `namespace` บวก `workspace`) ไม่มีไฟล์ → routes ว่าง validation: reject route ที่มีทั้งสอง key หรือไม่มีเลย
2. `send.rs`: หลัง local-registry miss (ระหว่าง [`send.rs:99`](../../../crates/bwoc-cli/src/send.rs) กับการ return `NotFound`) ให้ consult routes; เมื่อ hit peer ให้ load registry ของ peer แล้ว retarget `inbox_path` path ของ local hit ไม่แตะ
3. Tests: local hit ไม่เปลี่ยน; exact-agent peer route; namespace prefix route; error validation both-keys; no-match `NotFound`; trust-gated peer send → refusal `unknown_sender` ที่ผู้รับ
4. แถว CHANGELOG + cross-reference ROADMAP + TH parity (`routing.th.md` mirror ไฟล์นี้)

แต่ละขั้น merge อิสระได้ ขั้น 2 เป็นขั้นเดียวที่แตะพฤติกรรม `send` จริง และต้องคง path ของ local เหมือนเดิมแบบ byte-for-byte

## อ้างอิงข้าม

- [`PHILOSOPHY.en.md` #4 Tilakkhaṇa](../docs/en/PHILOSOPHY.en.md) — อนัตตา หลักการที่ spec นี้สื่อ (ไม่มีตัวตนศูนย์กลาง → ไม่มี broker กลาง)
- [`trust.md`](trust.md) — gate ฝั่งรับที่ routing compose ด้วย; refusal `unknown_sender` คือสิ่งที่ทำให้ cross-ws ปลอดภัยโดย default
- [`messaging.md`](messaging.md) — envelope sender-identity (`from`) ที่ routing พาข้ามขอบเขต
- [`sangha.md`](sangha.md) — shared task list; task ของ peer workspace เอื้อมถึงได้ก็ต่อเมื่อ routing ลง
- SN 22.59 อนัตตลักขณสูตร — canonical source ([SuttaCentral](https://suttacentral.net/sn22.59))
