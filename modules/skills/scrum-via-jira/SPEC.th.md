---
title: Scrum ผ่าน Jira
aliases:
  - scrum-via-jira
tags:
  - group/framework-skills
  - type/skill
  - domain/scrum
  - domain/integration
maturity: L1
---

# Scrum ผ่าน Jira (Scrum via Jira)

> [!abstract] Skill แบบ **skill-on-plugin** ตัวแรกของเฟรมเวิร์ก ให้ agent มี scrum operation ระดับสูง 6 ตัว — propose / open / close สปรินต์, transition story, sync backlog, list สปรินต์ที่ active — แต่ละตัวเป็นการเรียก verb ของ `bwoc jira` ผ่าน plugin kind `jira` แบบบาง ๆ Skill เป็นเจ้าของ *ความหมายของ scrum*; plugin เป็นเจ้าของ *การ integrate* (REST, auth, JQL, rate limit, sync ledger) Skill พึ่งพา plugin kind `jira` ผ่านฟิลด์ contract [`requires_plugins`](#dependency-model--requires_plugins)

## Skill นี้ทำอะไร

ห่อ CLI `bwoc jira` ที่ operator ใช้ (plugin kind `jira`, [[../../docs/th/PLUGINS.th#ประเภทของปลั๊กอิน|PLUGINS.th.md §ประเภทของปลั๊กอิน]]) ด้วยคำศัพท์ scrum เพื่อให้ agent รันสปรินต์กับ issue tracker ภายนอกได้โดยไม่ต้อง derive การ integrate ใหม่ มี operation ที่เปิด 6 ตัว แต่ละตัวบอกว่า *เกิดอะไรในเชิง scrum* และ delegate *วิธีไปถึง Jira* ให้ verb ของ `bwoc jira` ภายใต้ฝากระโปรง

- **`propose-sprint`** — รวบรวม backlog story ที่เป็นตัวเลือกแล้ว emit องค์ประกอบสปรินต์ที่เสนอ **อ่านอย่างเดียว** ใช้ `bwoc jira query` (JQL ที่ scope ตามโปรเจกต์) ภายใต้ฝากระโปรง; ไม่เขียนภายนอก
- **`open-sprint`** — เปิดใช้สปรินต์ที่ตกลงกัน: map แต่ละ story ที่เลือกให้เรียบร้อย แล้ว push การ assign สปรินต์ ใช้ `bwoc jira link` (map story ↔ issue) แล้ว `bwoc jira sync` (push การ assign) ภายใต้ฝากระโปรง **เขียนแบบ gated**
- **`transition-story`** — ย้าย story หนึ่งตัวข้าม lifecycle ของ scrum (`backlog → in_progress → done`) ใช้ `bwoc jira transition` ภายใต้ฝากระโปรง **เขียนแบบ gated**
- **`sync-backlog`** — reconcile backlog ของ scrum กับ Jira (pull การเปลี่ยนแปลงภายนอก, push การเปลี่ยนแปลงในเครื่อง) ใช้ `bwoc jira sync` ภายใต้ฝากระโปรง — `--dry-run` แสดง plan การ resolve, การ apply เปล่า ๆ ถูก gate **เขียนแบบ gated**
- **`close-sprint`** — ปิดสปรินต์: transition story ที่เหลือและบันทึกการปิด ใช้ `bwoc jira transition` + `bwoc jira sync` ภายใต้ฝากระโปรง **เขียนแบบ gated**
- **`list-active-sprints`** — รายงานสปรินต์ที่เปิดอยู่ของโปรเจกต์ที่ตั้งค่าไว้ **อ่านอย่างเดียว** ใช้ `bwoc jira query` ภายใต้ฝากระโปรง

## ทำไมจึงมี Skill นี้

การ bundle scrum operation เข้าไปใน plugin `jira` จะบังคับให้ทุก agent ที่อยากรันสปรินต์ต้องแบก REST, auth, rate-limit, และเรื่อง conflict ไปด้วย — ผิดชั้นสำหรับ capability ของ agent ([[../../notes/2026-05-27_jira-plugin-architecture|บันทึกออกแบบ BWOC-40]] §5) การแยกออกทำให้ผู้บริโภค *หลายราย* — skill นี้, skill ในอนาคต, หรือการใช้ `bwoc jira` ตรง ๆ ของ operator — นั่งบน plugin **ตัวเดียว** ได้ และทำให้ plugin ถูก install, ตั้งค่า, audit, และทดสอบแยกจาก agent ใด ๆ Substrate เดียวกัน, ผู้เรียกต่างกัน — แกน [[../../docs/th/PLUGINS.th#Skill กับ Plugin|Skill กับ Plugin]] นำมาใช้ตรง ๆ

## ขอบเขต Skill ↔ Plugin

นี่คือเส้นแบ่งที่สำคัญที่สุด ต้องคมชัด

| | `scrum-via-jira` (skill นี้) | plugin kind `jira` (เช่น `jira-cloud-rest`) |
|---|---|---|
| ชั้น | Capability ของ agent | Integration ของเฟรมเวิร์ก |
| Opt-in ผ่าน | `<agent>/config.manifest.json` (`skills.framework[]`) | `workspace.toml [plugins.<name>]` |
| ผู้เรียก | Agent เองในระหว่างทำงาน | Runtime ของเฟรมเวิร์ก / CLI `bwoc jira` |
| รู้ | ความหมายของ scrum — สปรินต์, story, backlog, lifecycle `backlog → in_progress → done` | REST v3, HTTP Basic auth, ไวยากรณ์ JQL, `429`/`Retry-After`, Issue Mapping schema, sync ledger |
| **ไม่** รู้ | REST, auth, ไวยากรณ์ JQL, rate limit, รูปแบบ ledger | อะไรก็ตามเกี่ยวกับ scrum |

กฎสองข้อที่ตามมาจากตารางนี้ และ **ต่อรองไม่ได้**:

1. **การพึ่งพาเป็นทางเดียว: skill → plugin** Skill เรียก verb ของ plugin; plugin ไม่รู้จัก skill เลย plugin kind `jira` ใช้งานได้เต็มที่โดยไม่มี skill (operator รัน `bwoc jira` ตรง ๆ)
2. **Skill ไม่แตะ `.scrum/jira-sync.json` ตรง ๆ และไม่ถือ credential** มันเข้าถึง sync ledger *ผ่าน plugin โดยอ้อมเท่านั้น* — plugin เป็นผู้เขียน ledger รายเดียว ([[../../notes/2026-05-27_jira-plugin-architecture|บันทึก BWOC-40]] §6), และ credential resolve ภายใน plugin จาก `BWOC_JIRA_*` env / `.bwoc/secrets.toml` ไม่ผ่าน skill เลย

## สัญญา Operation

แต่ละ operation ประกอบจาก verb ของ `bwoc jira` หนึ่งตัวหรือมากกว่า gate ของ verb นั้นสืบทอดมา — verb ที่เขียนพก gate operator-confirmation ของ plugin ([[../../modules/plugins/jira-cloud-rest/SPEC|jira-cloud-rest SPEC.md]] §Verbs); skill ไม่เพิ่ม gate ที่สอง และไม่ถอดออก

| Operation | เจตนาเชิง scrum | verb `bwoc jira` ภายใต้ฝากระโปรง | ทิศทาง | Gate |
|---|---|---|---|---|
| `propose-sprint` | ร่างสปรินต์จาก backlog story ที่เป็นตัวเลือก | `query` | อ่าน | ไม่มี — อ่านฟรี |
| `open-sprint` | เปิดสปรินต์; assign story | `link` → `sync` | เขียน | operator confirmation (ใน plugin) |
| `transition-story` | เลื่อนสถานะ story หนึ่งตัว | `transition` | เขียน | operator confirmation (ใน plugin) |
| `sync-backlog` | reconcile backlog ↔ Jira | `sync` (`--dry-run` แสดงตัวอย่าง) | อ่าน/เขียน | การ apply ถูก gate (ใน plugin) |
| `close-sprint` | ปิดสปรินต์ | `transition` → `sync` | เขียน | operator confirmation (ใน plugin) |
| `list-active-sprints` | ลิสต์สปรินต์ที่เปิดของโปรเจกต์ | `query` | อ่าน | ไม่มี — อ่านฟรี |

verb ledger แบบ offline `bwoc jira status` รองรับการ introspect ฝั่งอ่านข้าม operation (เช่น resolve การ map story ↔ issue ปัจจุบันก่อน `transition`) ทุก operation ถูกสังเกตด้วย `Kāyānupassanā` (สถานะ ledger/filesystem ที่ plugin รายงาน) และ `Dhammānupassanā` (gate ใดมีผล) ความล้มเหลวเปิดเผย operation, root cause, และวิธีแก้ — ไม่ใช่แค่ "ล้มเหลว"

## Dependency Model — `requires_plugins`

Skill นี้คือ **skill-on-plugin dependency ตัวแรก** ของเฟรมเวิร์ก แสดงผ่านฟิลด์ contract เฉพาะ แยกจาก array `requires` ที่เป็นชื่อ skill:

```toml
[contract]
requires         = []          # ชื่อ framework SKILL (ฟิลด์เดิม)
requires_plugins = ["jira"]    # KIND ของ plugin ที่ skill นี้ต้องการให้ enable (ฟิลด์ใหม่)
```

- **`requires_plugins` ระบุ _kind_ ของ plugin ไม่ใช่ _ชื่อ_ plugin** `"jira"` คือค่า enum ของ kind จาก [[../../docs/th/PLUGINS.th#ประเภทของปลั๊กอิน|PLUGINS.th.md]] — ดังนั้น skill พึ่งพา adapter kind `jira` *ตัวใดก็ได้* ที่ enable อยู่ (`jira-cloud-rest` วันนี้, ตัวอื่นวันหน้า) ไม่ผูกกับ implementation ของ vendor เฉพาะ ซึ่งทำให้ skill เป็นกลางและสลับ adapter ได้โดยไม่แตะ skill
- **Resolve ตอน spawn agent** ถ้า `scrum-via-jira` ถูก enable บน agent แต่ไม่มี plugin kind `jira` enable ใน workspace, spawn จะล้มเหลวทันทีพร้อม diagnostic ที่ชัดเจนระบุ kind ที่ขาด — agent ไม่ถูกต่อสายครึ่ง ๆ
- **ตรวจจับได้เร็วกว่าด้วย `bwoc skill verify scrum-via-jira`** ซึ่งตรวจการพึ่งพาเดียวกันก่อนถึงเวลา spawn

Dependency model เต็ม — ทำไมฟิลด์เฉพาะดีกว่า overload `requires`, และการ resolve ตอน spawn ทำงานอย่างไร — ระบุใน [[../../docs/th/SKILLS.th#Skill-on-plugin dependency|SKILLS.th.md §Skill-on-plugin dependency]]

## การ Map Lifecycle

ตาม [[../../docs/th/SKILLS.th#Lifecycle|SKILLS.th.md §Lifecycle]]:

```
init       → resolve การพึ่งพา plugin kind jira; cache handle dispatch ของ bwoc jira
             ไม่มี REST call, ไม่อ่าน credential — Anattā
invoke     → แต่ละ operation ประกอบ verb ของ bwoc jira หนึ่งตัวหรือมากกว่า; verb ที่เขียน
             สืบทอด gate operator-confirmation ของ plugin Idempotent ที่ระดับ operation:
             transition ที่ replay ซึ่ง plugin apply ไปแล้วจะ converge เป็น no-op
teardown   → no-op Skill ไม่ถือ external state — sync ledger เป็นของ plugin
```

Skill ไม่ถือ global state ระหว่างการ invoke ปลอดภัยต่อการ replay

## Maturity

ประกาศ **L1** — spec + scaffold; สัญญา operation ถูกตรึงแล้วแต่ยังไม่ verify end-to-end การ verify จริงกับ Jira Cloud site จริงถูก gate ด้วย sandbox token ที่ operator ให้ (ความเสี่ยง `BWOC-EPIC-6` เดียวกับที่ [[../../modules/plugins/jira-cloud-rest/SPEC|plugin jira-cloud-rest]] พกอยู่) ขยับเป็น L2 เมื่อมี agent อย่างน้อยหนึ่งตัวขับสปรินต์ผ่านทั้ง 6 operation แบบ end-to-end; เป็น L3 เมื่อ `bwoc skill verify scrum-via-jira` ถูกต่อสายและผ่านใน CI

## ความเป็นกลาง

ค่าใน manifest ไม่ระบุ LLM backend, model, หรือ vendor CLI `requires_plugins = ["jira"]` อ้างถึงค่า enum ของ plugin-**kind** ของเฟรมเวิร์กเอง ไม่ใช่ vendor — เหมือนกับที่ [[../../modules/plugins/jira-cloud-rest/SPEC#ความเป็นกลาง (Neutrality)|plugin jira-cloud-rest]] ตั้ง `kind = "jira"` "Jira" / "Atlassian" ปรากฏเฉพาะใน `description` และ prose ของ SPEC นี้ในฐานะชื่อเป้าหมายการ integrate ซึ่งยอมรับได้ภายใต้กฎเดียวกับที่ปล่อยให้ plugin ตั้งชื่อเป้าหมายของมัน `bwoc jira` และ `bwoc skill verify` เป็นคำสั่งของเฟรมเวิร์ก ไม่ใช่คำสั่งของ backend สอดคล้องกับ **Samānattatā**

## ดูเพิ่ม

- [[../../docs/th/SKILLS.th|SKILLS.th.md]] — spec ที่ skill นี้ทำตาม; dependency model แบบ skill-on-plugin
- [[../../modules/plugins/jira-cloud-rest/SPEC|jira-cloud-rest SPEC.md]] — plugin kind `jira` ที่ skill นี้ขับ; สัญญา verb ภายใต้ฝากระโปรง
- [[../../docs/th/PLUGINS.th|PLUGINS.th.md]] — แถว kind `jira` + แกน Skill กับ Plugin
- [[../../notes/2026-05-27_jira-plugin-architecture|2026-05-27_jira-plugin-architecture.md]] — กรอบ EPIC-6 (§5 การแยก plugin-vs-skill, §6 ledger ผู้เขียนรายเดียว)
- [[../worktree-discipline/SPEC|worktree-discipline]] — framework skill อ้างอิงตัวแรก; รูปทรงที่ skill นี้ทำตาม
- [[SPEC|SPEC.md]] — ฉบับภาษาอังกฤษ (bilingual parity)
