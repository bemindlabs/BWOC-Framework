---
title: อะแดปเตอร์ Jira Cloud REST v3
aliases:
  - jira-cloud-rest
tags:
  - group/framework-plugins
  - type/plugin
  - kind/jira
  - domain/integration
  - integration/jira-cloud
maturity: L1
---

# อะแดปเตอร์ Jira Cloud REST v3

> [!abstract] ปลั๊กอินอ้างอิงตัวแรกที่ **เขียนข้อมูลได้** — อะแดปเตอร์ของ kind `jira` อ่าน issue ผ่าน JQL ที่จำกัดขอบเขตเฉพาะ project และทำการเปลี่ยนสถานะ (transition) แบบ **มีด่านยืนยัน** กับ Atlassian Cloud REST v3 ถูกเรียกใช้โดย CLI `bwoc jira` (`BWOC-42`) โดยอะแดปเตอร์รับผิดชอบเฉพาะส่วน HTTP ส่วน CLI รับผิดชอบการแยกวิเคราะห์อาร์กิวเมนต์, สมุดบัญชี sync (ledger), ด่านตรวจ auth และด่านยืนยันการเขียน รายการ mapping เป็นไปตาม [[../../docs/th/PLUGINS.th#สคีมา Jira Issue Mapping|สคีมา Jira Issue Mapping]] (`BWOC-41`)

## ทำไมจึงเป็น kind `jira` ไม่ใช่ปลั๊กอิน `workflow`

ทุก kind ที่ส่งมาก่อนหน้านี้ (`audit` และ kind สาย reporting ที่วางแผนไว้) ล้วน **อ่าน** workspace แล้วปล่อยผลลัพธ์ออกมา — ไม่เคยแก้ไขสถานะภายนอก แต่ `jira` ทั้ง **อ่านและเขียน** ระบบบันทึกภายนอก ได้แก่ การเปลี่ยนสถานะ issue และ (ในอนาคต) การอัปเดต field/sprint คุณสมบัติข้อนี้ — ผลข้างเคียงต่อระบบภายนอกที่ถาวรและย้อนกลับยากตอน `invoke` — คือเหตุผลที่มันได้ kind ของตัวเอง และเป็นที่มาของด่านยืนยันการเขียนกับนโยบายแก้ความขัดแย้ง เหตุผลฉบับเต็มอยู่ใน [[../../notes/2026-05-27_jira-plugin-architecture|บันทึกออกแบบ BWOC-40]] §1

## คำสั่ง (Verbs)

CLI `bwoc jira` มอบหมายให้อะแดปเตอร์นี้เพียงสามคำสั่งที่ต้องต่อเครือข่าย ส่วนคำสั่งออฟไลน์ (`status`, `link`, `unlink`) ไม่เคยมาถึงอะแดปเตอร์ — มันแตะเฉพาะ ledger

| คำสั่ง | ทิศทาง | Auth | HTTP | ด่าน |
|---|---|---|---|---|
| `query` | อ่าน | ต้องมี | `GET /rest/api/3/search` (JQL จำกัด project, แบ่งหน้าแบบมีขอบเขต) | ไม่มี — การอ่านไม่มีต้นทุน |
| `transition` | **เขียน** | ต้องมี | `GET …/transitions` แล้ว `POST …/transitions` | ยืนยันโดยผู้ดูแล (ใน CLI) |
| `sync` | อ่าน/**เขียน** | ต้องมี | อ่าน ledger; `--dry-run` แสดงตัวอย่าง | การ apply มีด่าน (ใน CLI) |

`query` และครึ่งอ่านของ `sync` คือเส้นทาง read-mostly ที่ทำงานได้จริงใน v0.1.0 ส่วน `transition` เป็นการเขียนที่มีโครงสร้างและ idempotent ส่วน `sync` เป็นโครงร่างที่มีโครงสร้าง (ดู [§Sync](#sync--โครงร่างที่มีโครงสร้าง))

## การทำงาน

CLI เรียก `jira.sh` จากไดเรกทอรีนี้ (สะท้อนวิธีที่ `bwoc audit` เรียกปลั๊กอิน `audit`):

| ช่องทาง | สิ่งที่ส่งมา |
|---|---|
| `BWOC_JIRA_OPERATION` (env) | `query` \| `transition` \| `sync` |
| `BWOC_WORKSPACE` (env) | path สัมบูรณ์ของ workspace; `sync` อ่าน `.scrum/jira-sync.json` ใต้นี้ |
| `BWOC_PLUGIN_DIR` (env) | path สัมบูรณ์ของไดเรกทอรีปลั๊กอินนี้ |
| `BWOC_JIRA_EMAIL` / `BWOC_JIRA_TOKEN` / `BWOC_JIRA_BASE_URL` (env) | ข้อมูลรับรอง ส่งทอดผ่าน env (ดู [§การยืนยันตัวตน](#การยืนยันตัวตน)) |
| `BWOC_JIRA_PROJECT` (env, ทางเลือก) | project key สำหรับจำกัดขอบเขต JQL (ดู [§JQL](#jql--การจำกัดขอบเขต-project)) |
| stdin | คำขอ JSON บรรทัดเดียว เช่น `{"operation":"query","jql":"…","start_at":0,"max_results":50}` |

เมื่อสำเร็จ สคริปต์จะออกด้วยรหัส `0` และปล่อย **อ็อบเจกต์ JSON หนึ่งตัว** ทาง stdout ให้ CLI แยกวิเคราะห์ เมื่อผิดพลาด มันจะเขียนข้อความวินิจฉัยทาง **stderr** และออกด้วยรหัสไม่ใช่ศูนย์ ซึ่ง CLI จะรายงานเป็น `plugin '<name>' exited <code>` รูปแบบผลลัพธ์ที่ CLI ใช้:

- `query` → `{ "total": <n>, "issues": [ … ], "start_at", "max_results" }`
- `transition` → `{ "ok": true, "issue", "to_status", "transitioned": <bool> }`
- `sync` → `{ "summary": { "push", "pull", "noop", "conflict" }, "dry_run" }` — ค่า `conflict` ที่ไม่ใช่ศูนย์ทำให้ CLI ออกด้วยรหัส `3`

## การยืนยันตัวตน

Atlassian Cloud ยืนยัน REST v3 ด้วย **HTTP Basic = `email:api_token`** ต่อ base URL ของไซต์ อินพุตสามตัวถูก resolve จาก environment (เจอตัวแรกชนะ):

| อินพุต | ตัวแปร env | บทบาท |
|---|---|---|
| อีเมลบัญชี | `BWOC_JIRA_EMAIL` | ครึ่ง username ของ Basic-auth |
| API token | `BWOC_JIRA_TOKEN` | ครึ่ง password ของ Basic-auth — **ความลับ** |
| URL ไซต์ | `BWOC_JIRA_BASE_URL` | เช่น `https://<site>.atlassian.net` |

ลำดับการ resolve: **ตัวแปร environment** (แนะนำ — ไม่มีอะไรลงดิสก์) จากนั้นไฟล์ **`.bwoc/secrets.toml`** ที่ gitignore และเปิดอ่านได้เฉพาะเจ้าของ (`chmod 600`; ตาราง `[jira]`) token ถูกอ่านเข้าสู่ `curl -u` โดยตรง — ไม่เคยถูก echo, ไม่เคยถูกเขียนลงไฟล์ และไม่เคยปรากฏในผลลัพธ์ JSON ใด ๆ (**อทินนาทาน** ที่ทุกขอบเขต)

> [!danger] `auth.toml` ส่งมาเฉพาะ "รูปร่าง" เท่านั้น — ไม่เคยมีค่า [[auth|auth.toml]] ประกาศว่ามีคีย์อะไรบ้างและแต่ละคีย์ผูกกับตัวแปร env ใด พร้อม placeholder `email`/`token`/`base_url` ที่ **ว่างเปล่า** ไม่มีข้อมูลรับรองใดถูก commit เลย การหมุนเวียน token ที่ถูกเพิกถอนทำได้ที่ระดับ config เท่านั้น (อัปเดตตัวแปร env / ไฟล์ secrets) watermark `last_synced` ใน ledger ไม่ขึ้นกับข้อมูลรับรอง ดังนั้น `401`/`403` จะถูกรายงานว่า "ยืนยันตัวตนใหม่ / หมุน token" ไม่ใช่ความขัดแย้งของ sync

หากขาดตัวใดตัวหนึ่งในสามตัว อะแดปเตอร์จะล้มเหลวทันทีพร้อมข้อความ `auth_missing` ที่ระบุชื่อตัวแปรที่ขาด (CLI ก็มีด่านนี้ก่อนจะ spawn อะแดปเตอร์ด้วย — ป้องกันสองชั้น)

## JQL — การจำกัดขอบเขต project

`query` รับ JQL แต่อะแดปเตอร์จำกัดขอบเขตให้ (**มัตตัญญุตา** ที่ขอบเขต API):

- **จำกัดเฉพาะ project** เมื่อมีการตั้ง `BWOC_JIRA_PROJECT` และ JQL ยังไม่ระบุ project อะแดปเตอร์จะห่อเป็น `project = "<P>" AND (<jql>)` — ไม่เผลออ่านข้าม project ใน v0.1.0 นี่เป็นการจำกัดขอบเขตแบบ best-effort ที่ขับด้วย env ส่วนการจำกัดขอบเขตที่ขับด้วย config จะตามมาเมื่อ CLI ส่งค่า config `[plugins.jira-cloud-rest].project` ที่ resolve แล้วมาให้
- **ชุดผลลัพธ์มีขอบเขต** `maxResults` ถูกจำกัดที่ ≤ 100 และเคารพ `startAt` — อะแดปเตอร์ไม่เคยดึงข้อมูลแบบไม่มีขอบเขต นี่เป็นแนวป้องกัน rate limit ด่านแรก
- **อ่านอย่างเดียวโดยธรรมชาติ** JQL ไม่เคยเป็นเส้นทางเขียน การเขียนไปผ่านคำสั่ง `transition` ที่มีรูปแบบชัดเจน

## การจำกัดอัตรา (rate limit) และคลาสของข้อผิดพลาด

Atlassian Cloud ใช้การจำกัดอัตราแบบอิงต้นทุนและคืน `429 Too Many Requests` พร้อมเฮดเดอร์ `Retry-After` `jira.sh` แยกแยะคลาสข้อผิดพลาดที่ [[../../notes/2026-05-27_jira-plugin-architecture|บันทึก BWOC-40]] §3 ระบุไว้:

| HTTP | คลาส | พฤติกรรมของอะแดปเตอร์ |
|---|---|---|
| `2xx` | สำเร็จ | แยกวิเคราะห์ + แปลง body |
| `429` | ลองใหม่ได้ | เคารพ `Retry-After` (ถ้าไม่มีใช้ exponential fallback); สูงสุด 4 ครั้ง แล้วรายงานข้อผิดพลาดที่ลองใหม่ได้ |
| `401` / `403` | auth ร้ายแรง | "หมุน `BWOC_JIRA_TOKEN` / ตรวจ `BWOC_JIRA_EMAIL`" — **ไม่ใช่** ความขัดแย้งของ sync |
| `404` | mapping เคลื่อน | issue ที่ผูกไว้ถูกย้าย/ลบ; รายงานต่อผู้ดูแล ไม่สร้างใหม่เงียบ ๆ |
| อื่น ๆ / transport | ข้อผิดพลาด | รายงานพร้อม body ที่ตัดสั้น; ออกด้วยรหัสไม่ใช่ศูนย์ |

การลองใหม่ปลอดภัยเพราะการเขียนเป็น idempotent (ดู [§Idempotency](#idempotency))

## การใช้สคีมา Issue Mapping

`query` แปลงแต่ละ Jira issue ให้อยู่ในรูป [[../../docs/th/PLUGINS.th#สคีมา Jira Issue Mapping|สคีมา Jira Issue Mapping]] ที่เป็นบรรทัดฐาน — `issue_key`, `project`, `summary`, `status`, `assignee` (ละไว้เมื่อไม่มีผู้รับผิดชอบ ไม่ใช่ `null`) ส่วน `sync` อ่าน/เขียนรายการเดียวกันใน `.scrum/jira-sync.json` ซึ่งเป็น ledger เดียว (อะแดปเตอร์เป็นหนึ่งในผู้เขียนผ่าน CLI) `issue_key` เป็นคีย์ถาวรเพียงตัวเดียว ส่วน field อื่นเป็น projection ที่เปลี่ยนได้ รีเฟรชทุกครั้งที่ sync และเทียบทีละ field กับ watermark `last_synced` เพื่อตรวจความขัดแย้ง

## Sync — โครงร่างที่มีโครงสร้าง

`sync` ใน v0.1.0 วาง **สัญญา, auth และเส้นทางอ่าน**: มันอ่าน ledger นับ issue ที่ map ไว้ และปล่อยรูป envelope `summary.{push,pull,noop,conflict}` สุดท้าย โดยรายงาน issue ที่ map ไว้ทุกตัวเป็น **no-op** — ไม่มีการเขียน เครื่องยนต์แก้ความขัดแย้งแบบ last-writer-wins รายฟิลด์ พร้อมการยืนยันความขัดแย้งจริงโดยผู้ดูแล (บันทึก BWOC-40 §4) ถูก **เลื่อนไปยังเครื่องยนต์ sync ของ EPIC-6** รูป envelope ที่นี่เป็นฉบับสุดท้ายแล้ว เพื่อให้สัญญากับ CLI นิ่งตั้งแต่ตอนนี้

## การตั้งค่า

```toml
# workspace.toml
[plugins.jira-cloud-rest]
enabled = true
project = "BWOC"   # project key ของ Jira ที่จำกัดขอบเขตการอ่าน
```

ข้อมูลรับรอง **ไม่ใช่** config — มัน resolve จาก env `BWOC_JIRA_*` / `.bwoc/secrets.toml` คีย์ `[config.schema]` ที่ประกาศมีเพียง `project`

## การจับคู่วงจรชีวิต

ตาม [[../../docs/th/PLUGINS.th#วงจรชีวิต|PLUGINS.th.md §วงจรชีวิต]] เจ้าของ kind `jira` คือ CLI `bwoc jira`; `init`/`teardown` เกิดต่อการเรียกแต่ละครั้งรอบ ๆ `invoke` อะแดปเตอร์ไม่ถือสถานะภายในใด ๆ นอกจาก ledger ที่ใช้ร่วมกัน

| เฟส | สิ่งที่อะแดปเตอร์นี้ทำ |
|---|---|
| `init` | โดยปริยายต่อการเรียก; ตรวจว่ามี auth ครบ |
| `invoke` | อ่านคำขอ, เรียก REST v3, ปล่อย JSON |
| `teardown` | โดยปริยาย; ลบเฉพาะไฟล์ header/body ชั่วคราว |

## Idempotency

- `query` อ่านอย่างเดียวและลำดับคงที่
- `transition` ตรวจสถานะปัจจุบันของ issue ก่อน: หากเท่ากับเป้าหมายอยู่แล้ว จะเป็น no-op ที่สำเร็จ การเล่นซ้ำหลัง backoff จาก `429` จึงลู่เข้าสู่สถานะเดียวกัน
- `sync` ไม่เขียนอะไรใน v0.1.0 (โครงร่าง) จึง idempotent โดยปริยาย

## ระดับวุฒิภาวะ (Maturity)

ประกาศ **L1** — อะแดปเตอร์ `jira` ตัวแรกที่รันได้; เส้นทางอ่านทำงานได้ เส้นทางเขียนมีโครงสร้าง จะขยับเป็น L2 เมื่อทดสอบ end-to-end กับ Jira Cloud จริงด้วย token ที่ผู้ดูแลจัดหามา

> [!warning] ช่องว่างการทดสอบจริง การยืนยัน end-to-end กับไซต์ Jira Cloud จริงต้องรอ token sandbox จากผู้ดูแล (เป็นความเสี่ยงของ `BWOC-EPIC-6`) v0.1.0 ยืนยันด้วย: `bash -n jira.sh`, เส้นทาง auth-missing ที่ error อย่างสะอาด และ `bwoc check` ที่ยอมรับ manifest `jira` ส่วนการ round-trip REST จริงยังไม่ได้ยืนยันจนกว่าจะมี token

## ความเป็นกลาง (Neutrality)

ค่าใน manifest ไม่ระบุ LLM backend หรือ model ใด `kind = "jira"` เป็นค่า enum ของเฟรมเวิร์กเอง (`BWOC-41`) คำว่า "Jira" / "Atlassian" ปรากฏเฉพาะใน `description` (ที่อนุญาตชื่อเป้าหมายการเชื่อมต่อตาม [[../../docs/th/PLUGINS.th#ข้อจำกัดความเป็นกลาง (HARD)|PLUGINS.th.md §ความเป็นกลาง]]) และในเนื้อความ SPEC นี้ — ไม่เคยอยู่ใน `kind`, `entry` หรือคีย์ config สอดคล้องกับ **สมานัตตตา**

## ดูเพิ่มเติม

- [[../../docs/th/PLUGINS.th|PLUGINS.th.md]] — สเปกปลั๊กอิน; แถว kind `jira` + สคีมา Jira Issue Mapping (BWOC-41)
- [[../../notes/2026-05-27_jira-plugin-architecture|2026-05-27_jira-plugin-architecture.md]] — บันทึกกรอบงาน EPIC-6 (auth, JQL, rate-limit, นโยบายความขัดแย้ง)
- [[../../crates/bwoc-cli/src/jira|crates/bwoc-cli/src/jira.rs]] — CLI `bwoc jira` ที่เรียกใช้อะแดปเตอร์นี้
- [[auth|auth.toml]] — สัญญา auth (รูปร่างเท่านั้น ไม่มีค่า)
- [[SPEC|SPEC.md]] — คู่ฉบับภาษาอังกฤษ (ความเท่าเทียมสองภาษา)
