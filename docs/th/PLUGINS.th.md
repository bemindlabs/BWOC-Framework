---
title: Plugins
parent: ไทย
nav_order: 12
---

# Framework Plugins (ปลั๊กอินระดับเฟรมเวิร์ก)

**Framework plugin** ขยายเฟรมเวิร์กด้วยความสามารถที่ไม่ควรอยู่ใน agent ทุกตัว แต่ควรพร้อมใช้สำหรับ agent และ workspace ที่ต้องการ ปลั๊กอินถูกโหลดโดย **framework runtime** — เป็นเรื่องของ operator ไม่ใช่ของ agent

เอกสารนี้กำหนดประเภทของปลั๊กอิน, รูปแบบ manifest, lifecycle hooks, กลไกการโหลด, และ verification gates Reference plugin ตัวแรก (`memory-tier2-noop`) ลงพร้อมกับ spec นี้ — ทั้ง spec และ implementation พิสูจน์รูปแบบไปด้วยกัน

> [!abstract] สถานะ: scaffold เริ่มต้น ตาราง manifest และ lifecycle hook ด้านล่างเป็น normative; ส่วน prose อาจปรับเมื่องาน story BWOC-1..3 ทำให้ contract ละเอียดขึ้น Reference plugin ตัวแรกจะมาใน BWOC-7

---

## Skill กับ Plugin

Skill กับ plugin ใช้ substrate ร่วมกัน (TOML manifest, neutrality gate, opt-in per workspace) และต่างกันที่ **ใครเป็นคน invoke**

| | Skill | Plugin |
|---|---|---|
| Spec | [`SKILLS.th.md`](SKILLS.th.md) | เอกสารนี้ |
| ผู้ใช้ | ผู้สร้าง agent | Operator ของ workspace |
| Opt-in ผ่าน | `<agent>/config.manifest.json` | `workspace.toml [plugins]` |
| ผู้เรียก | Agent เองในระหว่างทำงาน | Framework runtime |
| ตัวอย่าง | worktree discipline, bilingual parity check | Tier 2 memory backend, LLM backend เพิ่ม |
| ขอบเขต lifecycle | Per-agent | Per-workspace |

เลือกชั้นที่ตรงกับ *ใครเปิดใช้งานมัน* ถ้า logic ของ agent ตัวใดตัวหนึ่งเรียก = skill ถ้า workspace โหลดครั้งเดียวสำหรับทุกคน = plugin

---

## ประเภทของปลั๊กอิน

ปลั๊กอินทุกตัวประกาศ `kind` Kind กำหนด lifecycle hook ที่เฟรมเวิร์กจะเรียก spec นี้ส่งแปดประเภท:

| Kind | สิ่งที่ขยาย | ผู้รับ lifecycle |
|---|---|---|
| `memory-backend` | Tier 2 memory (semantic search, vector store, deep-memory CLI) | Memory subsystem ของ agent |
| `llm-backend` | Backend นอกเหนือจากห้าตัวที่ประกาศ (`claude`, `antigravity`, `codex`, `kimi`, `ollama`) | `bwoc spawn` |
| `workflow` | Integration กับระบบภายนอก (issue tracker, code review, CI) | โค้ดของ agent ที่เรียกออก |
| `audit` | การตรวจสอบ workspace ตามมาตรฐานภายนอก (ISO/IEC 29110, ISO 9001, ISO 20000-1, ISO 27001) หรือ audit ที่ operator เขียนเอง (license header, doc parity, secret scan) | `bwoc audit` CLI |
| `jira` | Sync สองทิศทางกับ issue tracker ภายนอก (Jira Cloud) — อ่าน issue ผ่าน JQL และ **เขียน** status transition, การแก้ field, และการ assign sprint กลับไปยัง tracker | `bwoc jira` CLI |
| `okr` | ติดตาม Objectives + Key Results ที่ operator เขียนเอง — อ่าน `objectives.toml` / `key_results.toml`, บันทึกความคืบหน้า, และส่งรายงานความคืบหน้าที่เป็น normative | `bwoc okr` CLI |
| `council` | decision protocol แบบมีโครงสร้างระหว่าง agent ในฟลีต — agent ใดก็เปิด decision ได้, ผู้ร่วมถกใน rounds และโหวต, บันทึก outcome พร้อม evidence + dissent | `bwoc council` CLI |
| `figma` | Integration แบบอ่านเป็นหลักกับ Figma REST API — ดึง metadata ของ frame/node, export รูป, query component library, และ surface design token; เชื่อม design→dev | `bwoc figma` CLI |

ปลั๊กอินตั้ง `kind` ครั้งเดียว ปลั๊กอินข้าม kind ไม่รองรับ — แยกออกเป็นหลายตัว

ประเภท `audit` เพิ่มเข้ามาใน `BWOC-EPIC-2`; เหตุผล (ทำไมเลือก `audit` ไม่ใช่ `compliance` หรือ `policy`) และโรดแมป ISO ที่เป็นแรงจูงใจ ดู [BWOC-19 design note](../../notes/2026-05-26_iso-compliance-plugins.md)

ประเภท `jira` เพิ่มเข้ามาใน `BWOC-EPIC-6` เป็น **plugin kind ตัวแรกที่เขียนได้ (write-capable)** ของเฟรมเวิร์ก — เป็น *integration adapter* ไม่ใช่ reporting kind ทุก kind เหนือมัน (`audit` รวมถึง reporting kind ที่วางแผนไว้) เพียง **อ่าน** workspace แล้วส่งรายงานออก; `jira` ทั้ง **อ่านและเขียน** external system of record คุณสมบัติเดียวนี้ — side-effect ภายนอกที่ durable และย้อนกลับยากตอน `invoke` — คือสิ่งที่ทำให้มันต่าง: มันเก็บ sync ledger (`.scrum/jira-sync.json`), gate write verb ไว้หลัง operator confirmation, และพก [สคีมา Jira Issue Mapping](#สคีมา-jira-issue-mapping) ที่เป็น normative เหตุผลว่าทำไมเป็น kind แยกแทนที่จะเป็น `workflow` plugin, auth model, ขอบเขต JQL/rate-limit, และนโยบาย conflict สองทิศทาง ดู [BWOC-40 design note](../../notes/2026-05-27_jira-plugin-architecture.md) — spec นี้ประกาศ kind และ mapping schema เท่านั้น ไม่ทำซ้ำ rationale นั้น

ประเภท `okr` เพิ่มเข้ามาใน `BWOC-EPIC-4` เป็น **reporting kind** ตัวที่สามของเฟรมเวิร์ก เคียงข้าง `audit` ขณะที่ `audit` ตรวจ *workspace* ตามมาตรฐานภายนอกแล้วส่ง findings, `okr` ติดตาม Objectives + Key Results ที่ *operator เขียนเอง* (`objectives.toml` / `key_results.toml`) แล้วส่งความคืบหน้า มัน**ไม่ใช่** `workflow` kind: ไม่เอื้อมไประบบภายนอก, ไม่ถือ credential, และ write เดียวของมัน — `track` ที่อัปเดตค่า `current` ของ key result — แตะ TOML ในเครื่องของ operator เอง ไม่ใช่ system of record จึง**ไม่มี** operator-confirmation gate มันพก [สคีมา OKR Progress](#สคีมา-okr-progress) ที่เป็น normative และ **reuse [Evidence kinds](#evidence-kinds) ของ audit** แทนที่จะสร้างของตัวเอง — evidence vocabulary เดียวทั่วเฟรมเวิร์ก เหตุผลว่าทำไม `okr` เป็น kind แยกแทนที่จะเป็น `audit` หรือ `workflow` plugin, รูปร่างข้อมูล, สัญญา verb `track` / `check-progress` / `report`, และการตัดสินใจ `confidence` เป็น enum ดู [BWOC-46 design note](../../notes/2026-05-28_okr-plugin-architecture.md) — spec นี้ประกาศ kind และ progress schema เท่านั้น ไม่ทำซ้ำ rationale นั้น

ประเภท `council` เพิ่มเข้ามาใน `BWOC-EPIC-5` เป็น **coordination kind** ตัวแรกของเฟรมเวิร์ก — มันไม่ได้เอื้อมออกไประบบภายนอก (อย่าง `workflow`/`jira`) และไม่ได้รายงานเหนือ workspace (อย่าง `audit`/`okr`) แต่ทำงาน **ในหมู่ agent ของฟลีตเอง** decision ของ council เดินตาม protocol หลายขั้น — `propose` → `discuss` (rounds) → `vote` → `resolve` — มี quorum gate และ voting model ที่ประกาศ (`simple-majority` / `consensus` / `weighted` / `sangha`); ดึงผู้ร่วมจาก `bwoc team`, route discussion turn ผ่าน `bwoc send`, และเก็บ [สคีมา Council Decision](#สคีมา-council-decision) ที่เป็น normative พร้อม outcome และ dissent ที่ถูกรักษาไว้ มัน **บันทึก** decision ไม่ใช่ execute — outcome ที่ `binding` จะ emit `bwoc task` แทนที่จะไปแก้อะไรเอง รายละเอียด protocol, voting model, quorum + tie-break, ความต่าง binding-vs-advisory, และ reference `council-sangha-7` (โมเดลตาม Aparihaniya-dhamma 7) ดู [BWOC-56 design note](../../notes/2026-05-28_council-plugin-architecture.md) — spec นี้ประกาศ kind และ decision schema เท่านั้น ไม่ทำซ้ำ rationale นั้น

ประเภท `figma` เพิ่มเข้ามาใน `BWOC-EPIC-7` เป็น integration แบบ **อ่านเป็นหลัก (read-mostly)** กับ Figma REST API เช่นเดียวกับ `jira` (และต่างจาก `gcloud` ที่ reuse `workflow`) มันได้ kind ของตัวเองเพราะพก [สคีมา Figma Asset Mapping](#สคีมา-figma-asset-mapping) ที่เป็น normative — ความสัมพันธ์ที่ BWOC เป็นเจ้าของ ผูก Figma node กับ artifact ที่ export + design token; กฎคือ **ได้ kind ของตัวเองเมื่อ BWOC นิยาม normative schema เหนือ integration, reuse `workflow` เมื่อเป็น passthrough ที่ไม่มี shape ที่ BWOC เป็นเจ้าของ** ต่างจาก `jira`, `figma` ไม่เคยเขียนกลับไประบบภายนอก: ทุก verb อ่าน Figma (`fetch` / `tokens` / `status`) หรือเขียน **ในเครื่อง** (`export` วางรูป content-addressable ใต้ `figma/exports/`) จึงพกวินัย schema ของ jira แต่ไม่มี bidirectional-sync machinery — ไม่มี ledger, conflict policy, operator-confirm gate Auth เป็น personal access token ของ operator (`BWOC_FIGMA_TOKEN` env / `.bwoc/secrets.toml`, shape-only ใน `auth.toml`, ไม่เคย commit) auth model, ขอบเขต file-vs-team-library, การจัดการ REST rate-limit, และกลยุทธ์ export caching ดู [BWOC-61 design note](../../notes/2026-05-28_figma-plugin-architecture.md) — spec นี้ประกาศ kind และ asset schema เท่านั้น ไม่ทำซ้ำ rationale นั้น

### สิ่งที่ Plugin ไม่ใช่

- **ไม่ใช่ช่องโหว่สำหรับ logic เฉพาะ vendor ใน framework** ห้า backend ที่ประกาศเป็น first-class อยู่ใน spec ไม่ใช่ plugin คำพูดเฉพาะ vendor ใน `AGENTS.md` ยังห้ามอยู่ (**Samānattatā**)
- **ไม่ใช่ที่สำหรับสคริปต์ครั้งเดียวจบ** สคริปต์เหล่านั้นอยู่กับ agent ที่ใช้
- **ไม่ใช่ skill ที่ซับซ้อนกว่าเดิม** ถ้า agent เรียกในระหว่างทำงานของตัวเอง = skill (ดู [`SKILLS.th.md`](SKILLS.th.md))

---

## สคีมา Findings สำหรับ Audit

`invoke` ของปลั๊กอินประเภท `audit` ทุกตัวคืน list ของ **findings** สคีมาด้านล่างเป็น normative — ทั้งปลั๊กอินที่รันได้และ stub ต้องส่ง findings ตามรูปนี้ และ envelope `bwoc audit run --json` จาก `BWOC-12` สร้างทับสคีมานี้โดยตรง เฟรมเวิร์ก validate enum แบบปิดที่ขอบเขตของ `invoke` ทุกครั้ง; ค่าที่ไม่รู้จักคือ bug ของปลั๊กอินที่ทำให้ audit run fail ไม่ใช่ finding ที่ operator ต้อง triage

### ฟิลด์

| ฟิลด์ | ชนิดข้อมูล | บังคับ | ความหมาย |
|---|---|---|---|
| `criterion_id` | string, kebab-case | ใช่ | ตัวระบุของ criterion ที่ตรวจ **Scope ภายในปลั๊กอิน** — unique ภายในปลั๊กอินตัวเดียว ไม่ใช่ทั่วทั้งระบบ ต้องตรงกับ entry ในรายการ criteria ของปลั๊กอินที่ประกาศ **คงที่ข้ามรีลีส** — เปลี่ยนชื่อ `criterion_id` คือ breaking change ของ contract ของปลั๊กอิน (ดู [Stability](#stability)) |
| `severity` | enum แบบปิด: `info` \| `low` \| `medium` \| `high` \| `critical` | ใช่ | ความรุนแรงในตัวของ criterion ประกาศครั้งเดียวใน criteria list ของปลั๊กอิน — **ไม่ใช่** สิ่งที่ตัดสินต่อ run finding ที่ `critical` กับ `status = "pass"` เป็นเรื่องปกติ หมายถึง "เราตรวจสิ่งที่สำคัญที่สุดและมันโอเค" Severity บอกความสำคัญของ criterion ไม่ใช่ผลลัพธ์ |
| `status` | enum แบบปิด: `pass` \| `fail` \| `not_applicable` \| `not_implemented` | ใช่ | ผลของการตรวจบน workspace นี้ `not_applicable` ใช้กับ criterion ที่ไม่ใช้กับ profile ของ workspace นี้ (เช่น clause multi-tenant กับ workspace แบบ solo) `not_implemented` เป็นสถานะของ stub plugin — ใช้โดย `audit-iso-9001`, `audit-iso-20000-1`, และ `audit-iso-27001` จนกว่า runtime จะ land ใน `BWOC-EPIC-3` ค่า status แบบ free-text เป็น bug ของปลั๊กอิน |
| `evidence` | structured: `{ kind, value, ...sub-field ตาม kind }` โดย `kind ∈ { "file", "content", "command", "attestation", "sample", "none" }` และ `value` เป็น string บาง kind มี sub-field บังคับเพิ่ม (ดู [Evidence kinds](#evidence-kinds)) ฟิลด์เสริม 2 ตัวใช้ได้กับทุก kind: `as_of` (ISO 8601 date — วันที่ evidence ยังเป็นปัจจุบัน) และ `valid_through` (ISO 8601 date — วันหมดอายุที่ operator ประกาศ) | ใช่ | ที่ที่ปลั๊กอินไปดู `kind` บังคับเสมอ; `value` บังคับยกเว้น `kind = "none"` Evidence ต้องทำซ้ำได้ — operator ที่รันการตรวจเดียวกันด้วยมือต้องเจอ artifact เดียวกัน นี่คือการ์ด **Musāvāda**: ไม่มีคำกล่าวอ้างใดที่ไม่มี referent dispatcher ประทับ `as_of` / `valid_through` ถ้ามี; ไม่บังคับ semantics ของวันหมดอายุ — tooling ปลายทางตัดสินใจ ดู [Evidence kinds](#evidence-kinds) |
| `remedy` | string, ข้อความปกติ | conditional | ขั้นถัดไปที่ลงมือทำได้ **บังคับ** เมื่อ `status` เป็น `fail`, `not_applicable`, หรือ `not_implemented` ("ทำไมสถานะนี้ และต้องทำอะไรต่อ") **ตัดออก** เมื่อ `status = "pass"` เฟรมเวิร์กปฏิเสธ finding ที่ใส่ `remedy` กับ `pass` และ finding ที่ไม่ใส่กับ status อื่น |

### Evidence kinds

| `evidence.kind` | ความหมายของ `evidence.value` | Sub-field บังคับ | ใช้เมื่อ |
|---|---|---|---|
| `file` | Path สัมพัทธ์กับ workspace root (เช่น `docs/en/PROJECT-PLAN.en.md`) ไฟล์มีอยู่จริงที่ path นั้น | — | Criterion คือ "artifact นี้มีอยู่" |
| `content` | Path พร้อม locator (เช่น `Cargo.toml#workspace.package.license`, `docs/en/SRS.en.md:§3.2`) ปลั๊กอินพบเนื้อหาที่คาดหวังที่ locator นั้น | — | Criterion คือ "artifact นี้มี/ประกาศ X" |
| `command` | คำสั่ง shell-safe ที่ operator รันซ้ำได้ (เช่น `bwoc check --all`) ปลั๊กอินรันคำสั่งและสังเกต exit code | — | Criterion คือ "คำสั่งนี้ผ่านบน workspace นี้" |
| `attestation` | ข้อความ free-text แบบ verbatim — multi-line ได้ Artefact เป็นการลงนามของ operator ไม่ใช่ไฟล์ใน workspace | `signer` (string — ตัวตน free-text เช่น `"CISO: สุชาดา น."`), `signed_at` (ISO 8601 date หรือ datetime) | Criterion ลดเป็น "X เกิดขึ้น, นี่คือผู้ลงนามและเวลา" ใช้โดย ISO 9001 (clause ส่วนใหญ่), ISO/IEC 27001 (5.2 / 6.1.2 / 6.1.3), ISO/IEC 20000-1 (5.2 service policy) |
| `sample` | ข้อความสรุปสั้น (เช่น `"49 of 50 incidents resolved within SLA"`) | `sampled_count` (จำนวนเต็ม N), `sampled_of` (จำนวนเต็ม M), เสริม `window` (free-text ช่วงเวลา เช่น `"2026-Q1"`, `"last 90 days"`) | Criterion เป็นเชิงสถิติ — "N จาก M รายการผ่านเกณฑ์ในช่วงเวลานี้" ใช้โดย ISO/IEC 20000-1 (อัตรา incident/change, SLA performance), ISO/IEC 27001 (Annex A sampling, SoA-driven scope) |
| `none` | String ว่าง | — | `status = "not_applicable"` (ไม่ต้องตรวจ) หรือ `status = "not_implemented"` (runtime ถูกเลื่อน) ห้ามปรากฏกับ `status = "pass"` หรือ `"fail"` — สอง status นั้นต้องมี referent เสมอ |

**`attestation` และ `sample` เป็นการเพิ่มในรอบนี้ (additive)** — `file`, `content`, `command`, และ `none` ไม่เปลี่ยน producer และ consumer v1 ยังคง validate ผ่าน ดู [บันทึกออกแบบ 2026-05-27_iso-runtime-evidence-model](../../notes/2026-05-27_iso-runtime-evidence-model.md) สำหรับ rationale การจับคู่แต่ละมาตรฐาน

### กติกาของสคีมา

- **Enum แบบปิด ไม่ใช่ string อิสระ** `severity`, `status`, และ `evidence.kind` validate ตอน load ปลั๊กอินและที่ขอบเขตของ `invoke` ทุกครั้ง ค่าที่ไม่รู้จักคือ bug ของปลั๊กอินที่ทำให้ audit run fail
- **ไม่มี finding แบบ nested** Criterion ผ่านหรือ fail เป็นหน่วยเดียว Sub-check แยกเป็น `criterion_id` ของตัวเอง รายงานยังคงแบนและ machine-parseable
- **ลำดับ serialize คงที่** Findings serialize ตาม **ลำดับการประกาศ criterion** — ลำดับใน criteria list ของปลั๊กอิน — ไม่ใช่ลำดับการรัน Diff ข้าม run มีความหมายเมื่อทำตามนี้เท่านั้น
- **JSON เป็นรูปแบบ wire ที่ canonical** `bwoc audit run --json` (ตาม `BWOC-12`) ส่ง envelope หนึ่งต่อปลั๊กอิน: `{ plugin, version, started_at, finished_at, findings: [...] }` Output แบบ human-readable คือ renderer ที่ทำงานบนรูปนี้; JSON เป็น normative

### Stability

ค่า `criterion_id` เป็น public surface ของปลั๊กอิน การเพิ่ม criterion คือ minor-version bump ใน semver ของปลั๊กอินเอง **การเปลี่ยนชื่อหรือลบ** `criterion_id` คือ major-version bump (แยกจากเวอร์ชันเฟรมเวิร์กใน `[plugin].compat`) — consumer ปลายทาง (เครื่องมือ diff, archive ของรายงาน, dashboard) อ้างอิงตัวระบุเหล่านี้

### ตัวอย่าง

Finding ที่ผ่านไม่ใส่ `remedy`:

```json
{
  "criterion_id": "29110-bp-project-plan-exists",
  "severity":     "high",
  "status":       "pass",
  "evidence":     { "kind": "file", "value": "docs/en/PROJECT-PLAN.en.md" }
}
```

Finding ที่ fail มี `remedy`:

```json
{
  "criterion_id": "29110-bp-traceability-matrix",
  "severity":     "medium",
  "status":       "fail",
  "evidence":     { "kind": "file", "value": "docs/en/TRACEABILITY.en.md" },
  "remedy":       "สร้าง docs/en/TRACEABILITY.en.md เชื่อม requirement ใน SRS แต่ละข้อกับ design element และ test case"
}
```

Stub plugin (`audit-iso-9001`, `audit-iso-20000-1`, `audit-iso-27001` ตาม `BWOC-EPIC-2`) ส่ง `status = "not_implemented"` พร้อม remedy แบบ uniform:

```json
{
  "criterion_id": "iso-9001-internal-audit-program",
  "severity":     "medium",
  "status":       "not_implemented",
  "evidence":     { "kind": "none", "value": "" },
  "remedy":       "Runtime เลื่อนไป BWOC-EPIC-3"
}
```

Finding แบบ `attestation` (รูปเป้าหมายของ EPIC-3 ISO 9001 runtime ตาม BWOC-28):

```json
{
  "criterion_id": "9001-management-review",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":       "attestation",
    "value":      "การทบทวนของผู้บริหารจัดขึ้น 2026-04-15 ครอบคลุมผลการทำงาน QMS Q1, feedback ลูกค้า, ผล internal audit, โอกาสปรับปรุง บันทึกการประชุมเก็บไว้",
    "signer":     "ผู้จัดการคุณภาพ: ต้นกล้า ก.",
    "signed_at":  "2026-04-15",
    "valid_through": "2027-04-15"
  }
}
```

Finding แบบ `sample` (รูปเป้าหมายของ EPIC-3 ISO/IEC 20000-1 runtime):

```json
{
  "criterion_id": "20000-1-incident-management",
  "severity":     "high",
  "status":       "pass",
  "evidence": {
    "kind":          "sample",
    "value":         "49 จาก 50 incidents แก้ไขภายใน SLA",
    "sampled_count": 49,
    "sampled_of":    50,
    "window":        "2026-Q1"
  }
}
```

### Exit code — `bwoc audit run`

Process exit code เป็น normative และคงรูปข้ามเวอร์ชัน operator และ CI สามารถ branch ด้วย `$?` ได้โดยไม่ต้องแกะ stdout — `--json` envelope ก็พกข้อมูลเดียวกันในฟิลด์ `summary.fail_count` และ `summary.framework_error`

| Code | ความหมาย |
|---|---|
| `0` | ไม่มี finding ที่ `status = "fail"` ใน plugin ที่ถูกเลือกทั้งหมด (รวมถึงกรณีไม่มี audit plugin ที่ enabled หรือ `--plugin <name>` เจอ plugin ที่ปล่อยแต่ `pass` / `not_applicable` / `not_implemented`) |
| `1..=254` | จำนวน `fail` finding รวมจากทุก plugin ที่ถูกเลือก clamp ไว้สูงสุดที่ `254` ถ้า run จริงสร้าง `fail` ≥ 255 ค่าจริงยังอ่านได้จาก `summary.fail_count` ใน `--json` |
| `255` | Framework หรือ plugin runtime error — ค้น manifest ไม่เจอ parse manifest ผิด plugin spawn ไม่ได้ คืน stdout ไม่ใช่ JSON หรือ finding ละเมิดสคีมาข้างบน — `--json` envelope จะมี `summary.framework_error = true` |
| `2` | Operator/usage error — หาตัว workspace ไม่เจอ (ไม่มี `--workspace`, ไม่มี `BWOC_WORKSPACE`, ไม่มี `.bwoc/workspace.toml` ใน ancestor) หรือ `--plugin <name>` ที่ส่งมาไม่ตรงกับ audit-kind plugin ที่ติดตั้งอยู่ |

`0` กับ `1..=254` หมายความว่า framework รัน run จบสมบูรณ์และกำลังรายงานผลของ plugin `255` หมายความว่า framework เองสร้างรายงานที่เชื่อถือได้ไม่ได้ `2` หมายความว่า invocation ของ operator ผิดตั้งแต่ก่อน plugin จะถูกเรียก

---

## สคีมา Jira Issue Mapping

ปลั๊กอินประเภท `jira` map scrum story ไปยัง Jira issue ผ่าน **mapping entry** สคีมาด้านล่างเป็น normative — ทั้ง reference plugin และ sync ledger (`.scrum/jira-sync.json`) ต้อง persist mapping entry ตามรูปนี้ และ resolution plan ของ `bwoc jira sync` (ตาม `BWOC-42`) คำนวณบนสคีมานี้โดยตรง เฟรมเวิร์ก validate ฟิลด์ที่บังคับที่ขอบเขตของ `invoke` ทุกครั้งที่อ่านหรือเขียน mapping; ฟิลด์บังคับที่หายไปคือ bug ของปลั๊กอินที่ทำให้ sync run fail ไม่ใช่สถานะที่ operator ต้องมานั่ง reconcile เอง

นี่คือ contract ของ kind `jira` เป็น analogue ฝั่งเขียนของ [สคีมา Findings สำหรับ Audit](#สคีมา-findings-สำหรับ-audit) ที่ kind `audit` พกไว้ Auth model, ขอบเขต JQL/rate-limit, และนโยบาย conflict สองทิศทางที่ใช้ฟิลด์เหล่านี้อยู่ใน [BWOC-40 design note](../../notes/2026-05-27_jira-plugin-architecture.md) และไม่ทำซ้ำที่นี่

### ฟิลด์

| ฟิลด์ | ชนิดข้อมูล | บังคับ | ความหมาย |
|---|---|---|---|
| `issue_key` | string | ใช่ | Jira issue key (เช่น `BWOC-123`) **external key ที่คงที่** — ฟิลด์ที่ mapping ใช้เป็น key คู่กับ scrum story id การเปลี่ยนที่นี่คือ mapping drift (การย้าย project ใน Jira ทำให้ key เปลี่ยน) ไม่ใช่การแก้ field (ดู [Field stability](#field-stability)) |
| `project` | string | ใช่ | Project key ของ Jira ที่ issue อยู่ (เช่น `BWOC`) ทุกการอ่าน project-scoped; mapping ที่ `project` หลุดจาก project ที่ตั้งไว้จะถูก reject |
| `summary` | string | ใช่ | ชื่อ issue เป็น projection ของ Jira state ที่เปลี่ยนได้ refresh ทุก sync |
| `status` | string | ใช่ | Workflow status ของ issue (เช่น `In Progress`) map ไปยัง scrum status เปลี่ยนได้; เทียบ field-by-field กับ watermark `last_synced` เพื่อตรวจ conflict |
| `assignee` | string | ไม่ | Account identifier ของผู้รับงาน (Atlassian `accountId` หรือ email) ตัดออกเมื่อ issue ไม่มีผู้รับงาน |
| `story_points` | number | ไม่ | คะแนนประมาณการ ตัดออกเมื่อ issue ยังไม่ได้ประเมิน |
| `parent_epic` | string | ไม่ | `issue_key` ของ epic แม่ ตัดออกสำหรับ issue ที่ไม่อยู่ใน epic ใด |
| `sprint` | string | ไม่ | ชื่อหรือ identifier ของ sprint ที่ issue ถูก assign ตัดออกเมื่อ issue อยู่ใน backlog (ไม่มี sprint) |
| `last_synced` | string (ISO 8601 datetime) | ใช่ | Watermark ของการ sync สำเร็จครั้งล่าสุดของ issue นี้ ขับเคลื่อนการตรวจ conflict แบบ per-field last-writer-wins; เป็นอิสระจาก credential ดังนั้นการ rotate API token จึงไม่ทำให้มัน invalid |

ฟิลด์ที่ไม่บังคับจะถูก **ตัดออกจาก entry** เมื่อ issue ไม่มีค่า — issue ที่ไม่มีผู้รับงานจะไม่มี key `assignee` — ไม่ serialize เป็น `null` เหมือนกับที่ audit finding ที่ผ่านตัด `remedy` ออกแทนที่จะส่งค่าว่าง

### Field stability

`issue_key` คือ external key ที่คงที่ Mapping ใช้ `issue_key` เป็น key (คู่กับ scrum story id); เป็นฟิลด์ **เดียว** ที่ consumer — sync ledger, เครื่องมือ diff, dashboard — ถือเป็น identifier ถาวรได้ อีกแปดฟิลด์เป็น projection ของ Jira state ที่เปลี่ยนได้ refresh ทุก sync และเทียบ field-by-field กับ watermark `last_synced`; อย่าใช้ `summary`, `status`, `assignee`, `story_points`, `parent_epic`, หรือ `sprint` เป็น key การเปลี่ยน `issue_key` เอง (การย้าย project ใน Jira ที่ re-key issue) คือ **mapping drift** ไม่ใช่การแก้ field — surface ให้ operator ไม่เขียนทับเงียบ ๆ ตามการจัดการ `404 → mapping drift` ใน [BWOC-40 design note](../../notes/2026-05-27_jira-plugin-architecture.md)

### ตัวอย่าง

Mapping entry ของ story ที่ข้อมูลครบและอยู่ใน sprint:

```json
{
  "issue_key":    "BWOC-123",
  "project":      "BWOC",
  "summary":      "ประกาศ jira plugin kind ใน PLUGINS spec",
  "status":       "In Progress",
  "assignee":     "agent-jisoo@bwoc.local",
  "story_points": 5,
  "parent_epic":  "BWOC-100",
  "sprint":       "Sprint 6",
  "last_synced":  "2026-05-27T10:00:00Z"
}
```

Entry ของ issue ใน backlog ที่ไม่มีผู้รับงาน ตัดฟิลด์ไม่บังคับที่ไม่มีค่าออก:

```json
{
  "issue_key":   "BWOC-200",
  "project":     "BWOC",
  "summary":     "ร่าง scrum-via-jira skill",
  "status":      "To Do",
  "last_synced": "2026-05-27T10:00:00Z"
}
```

---

## สคีมา OKR Progress

ปลั๊กอิน `okr` ติดตาม Objectives + Key Results ที่ operator เขียนเอง แล้วส่ง **progress entry** ต่อ key result หนึ่งตัว สคีมาด้านล่างเป็น normative — verb `report` ของ reference plugin (ตาม `BWOC-49`) และ output ของ `bwoc okr report` (ตาม `BWOC-48`) ต้องส่ง entry ที่ตรงรูปนี้ และ `bwoc check` (ตาม `BWOC-50`) validate มัน นี่คือสัญญาของ kind `okr` เป็นคู่ฝั่งติดตามเป้าหมายของ [สคีมา Findings สำหรับ Audit](#สคีมา-findings-สำหรับ-audit) ที่ kind `audit` พก

ตัว objective และ key result เขียนโดย operator ในไฟล์ TOML ในเครื่องสองไฟล์ (`objectives.toml`, `key_results.toml`); รูปร่างการเขียนและสัญญา verb `track` / `check-progress` / `report` อยู่ใน [BWOC-46 design note](../../notes/2026-05-28_okr-plugin-architecture.md) ไม่ทำซ้ำที่นี่

### ฟิลด์

| ฟิลด์ | ชนิด | บังคับ | ความหมาย |
|---|---|---|---|
| `objective_id` | string | ใช่ | objective แม่ **อ้างอิง** — ต้อง resolve ไปยัง `objective_id` ที่ประกาศใน `objectives.toml`; การอ้างถึงที่ค้าง (dangling) เป็น bug ของ plugin ที่ทำให้ `bwoc check` ไม่ผ่าน ไม่ใช่ state ที่ operator ต้องแก้ |
| `key_result_id` | string | ใช่ | key ที่เสถียรของ key result นี้ unique ภายใน plugin ฟิลด์เดียวที่ consumer (dashboard, diff tooling) ถือเป็น identifier ถาวรได้ |
| `target` | number | ใช่ | ค่าเป้าหมายที่ key result มุ่งไป |
| `current` | number | ใช่ | ค่าล่าสุดที่ติดตาม อัปเดตโดย verb `track`; ไม่เกินและไม่ clamp กับ `target` — การทำเกินเป้า (`current > target`) มีความหมายและถูกเก็บไว้ |
| `unit` | enum | ใช่ | หนึ่งใน `count` \| `percent` \| `currency` \| `ratio` \| `boolean` วิธีอ่าน `target` / `current` |
| `confidence` | enum | ใช่ | หนึ่งใน `high` \| `medium` \| `low` — การประเมินเชิงคุณภาพของ operator ว่า trajectory ยังไปได้ไหม เป็น enum ไม่ใช่คะแนนตัวเลข โดยเจตนา (BWOC-46 §5): attainment พกสัญญาณเชิงปริมาณ, `confidence` พกเชิงคุณภาพ |
| `evidence` | object | ใช่ | **reuse [Evidence kinds](#evidence-kinds) ของ audit** — `{ kind, value, ...ฟิลด์เฉพาะ kind }` โดย `kind ∈ { "file", "content", "command", "attestation", "sample", "none" }` Musāvāda guard มีผล: ค่า `current` ที่ติดตามควรพก referent ที่ reproduce ได้ (หรือ `kind = "none"` เมื่อไม่มี) ไม่มีการสร้าง evidence kind เฉพาะ OKR |
| `as_of` | string (ISO 8601 date) | ไม่ | เวลาที่ `current` ถูกติดตามล่าสุด ตัดออกเมื่อไม่เคยติดตาม |

ฟิลด์ไม่บังคับจะ **ตัดออกจาก entry** เมื่อไม่มีค่า — key result ที่ไม่เคยติดตามไม่มี key `as_of` — ไม่ serialize เป็น `null` สอดคล้องกับ convention ของ Audit Findings และ Jira Issue Mapping

### ความเสถียรของฟิลด์

`key_result_id` เป็น key ที่เสถียร — mapping ที่ consumer ถือเป็นถาวรได้ `objective_id` เป็น reference ที่เสถียร (ชี้ไป objective ด้วย id ที่ประกาศ) ฟิลด์ที่เหลือ (`target`, `current`, `unit`, `confidence`, `evidence`, `as_of`) เป็น projection ที่เปลี่ยนได้ของ tracking state รีเฟรชเมื่อ operator เขียน target และ verb `track` บันทึกความคืบหน้า; อย่า key บนมัน `current` และ `confidence` โดยเฉพาะเปลี่ยนทุก check-in

### ตัวอย่าง

```json
{
  "objective_id":  "O1",
  "key_result_id": "O1-KR1",
  "target":        1,
  "current":       1,
  "unit":          "count",
  "confidence":    "high",
  "evidence":      { "kind": "file", "value": "docs/en/PLUGINS.en.md" },
  "as_of":         "2026-05-28"
}
```

---

## สคีมา Council Decision

ปลั๊กอิน `council` บันทึก decision ของฟลีตเป็น **decision entry** สคีมาด้านล่างเป็น normative — verb ของ reference plugin (ตาม `BWOC-59`) และ output ของ `bwoc council` (ตาม `BWOC-58`) ต้องส่ง entry ที่ตรงรูปนี้ และ `bwoc check` (ตาม `BWOC-60`) validate มัน นี่คือสัญญาของ kind `council` เป็นคู่ฝั่ง coordination ของ [สคีมา Findings สำหรับ Audit](#สคีมา-findings-สำหรับ-audit)

decision เดินตาม protocol `proposed → discussing → voting → resolved` (หรือ `abandoned` ถ้า quorum ไม่ถึง); รายละเอียด protocol, voting model, และกฎ quorum/tie-break อยู่ใน [BWOC-56 design note](../../notes/2026-05-28_council-plugin-architecture.md) ไม่ทำซ้ำที่นี่

### ฟิลด์

| ฟิลด์ | ชนิด | บังคับ | ความหมาย |
|---|---|---|---|
| `decision_id` | string | ใช่ | key เสถียรของ decision — ฟิลด์เดียวที่ consumer ถือเป็น identifier ถาวรได้ |
| `status` | enum | ใช่ | `proposed` \| `discussing` \| `voting` \| `resolved` \| `abandoned` สถานะของ protocol |
| `participants` | array of string | ใช่ | agent id ที่ดึงจาก `bwoc team` ที่อ้างถึง participant นอก team ถูกปฏิเสธ |
| `options` | array of string | ใช่ | ตัวเลือกที่กำลังตัดสิน (≥2) |
| `rounds` | array | ใช่ | discussion rounds เรียงลำดับ แต่ละ round มี turns `{ participant, message_ref }` โดย `message_ref` ชี้ไป envelope ของ `bwoc send` ที่ถือ turn — inbox เป็น transport, record อ้างถึง ไม่ copy append-only |
| `votes` | array | ใช่ | `{ participant, option, abstain }` ต่อผู้โหวต append-only; การโหวตซ้ำ append ไม่ทับ (trail คือจุดประสงค์) |
| `outcome` | string | ไม่ | ตัวเลือกที่ resolve ตัดออกจนกว่า `status = resolved` |
| `dissent` | array | ไม่ | จุดยืนเสียงข้างน้อยที่บันทึก `{ participant, option, rationale }` รักษาไว้ ไม่ทิ้ง — การบันทึก dissent เป็นจุดประสงค์หนึ่งของ council |
| `evidence_links` | array | ไม่ | **reuse [Evidence kinds](#evidence-kinds) ของ audit** — `{ kind, value, ... }` referent ที่หนุน decision ไม่มี evidence kind เฉพาะ council |
| `opened_at` | string (ISO 8601 datetime) | ใช่ | เวลาที่ propose decision |
| `closed_at` | string (ISO 8601 datetime) | ไม่ | เวลาที่ resolve หรือ abandon ตัดออกขณะยังเปิด |

ฟิลด์ไม่บังคับจะ **ตัดออกจาก entry** เมื่อไม่มีค่า — decision ที่ยังไม่ resolve ไม่มี key `outcome` / `closed_at` — ไม่ serialize เป็น `null` ตาม convention ของ Audit Findings / Jira / OKR

### ความเสถียรของฟิลด์

`decision_id` เป็น key เสถียร `status`, `rounds`, `votes`, `outcome`, `dissent`, `closed_at` เปลี่ยนได้เมื่อ protocol เดินหน้า (rounds + votes สะสม, status เปลี่ยน, outcome/closed_at เติมตอน resolve); อย่า key บนมัน `participants` และ `options` ตายตัวตอน propose — การเปลี่ยนคือ decision ใหม่ ไม่ใช่ edit

### ตัวอย่าง

```json
{
  "decision_id":  "D1",
  "status":       "resolved",
  "participants": ["agent-jisoo", "agent-jennie", "agent-lisa", "agent-rose"],
  "options":      ["adopt", "defer"],
  "rounds":       [{ "round": 1, "turns": [{ "participant": "agent-jisoo", "message_ref": "msg-20260528T120000Z-a1b2c" }] }],
  "votes":        [{ "participant": "agent-jisoo", "option": "adopt", "abstain": false }],
  "outcome":      "adopt",
  "dissent":      [],
  "evidence_links": [{ "kind": "file", "value": "notes/2026-05-28_council-plugin-architecture.md" }],
  "opened_at":    "2026-05-28T12:00:00Z",
  "closed_at":    "2026-05-28T12:30:00Z"
}
```

---

## สคีมา Figma Asset Mapping

ปลั๊กอิน `figma` map Figma node กับ artifact ที่ export + design token ผ่าน **asset entry** สคีมาด้านล่างเป็น normative — verb ของ reference plugin (ตาม `BWOC-64`) และ output ของ `bwoc figma` (ตาม `BWOC-63`) ต้องส่ง entry ที่ตรงรูปนี้ และ `bwoc check` (ตาม `BWOC-65`) validate มัน นี่คือสัญญาของ kind `figma` เป็นคู่ฝั่ง design→dev แบบอ่านเป็นหลักของ [สคีมา Jira Issue Mapping](#สคีมา-jira-issue-mapping) — พกวินัย schema ของ jira แต่ไม่เขียนกลับระบบภายนอก

auth model, ขอบเขต file-vs-team-library, การจัดการ REST rate-limit, และกลยุทธ์ export caching แบบ content-addressable อยู่ใน [BWOC-61 design note](../../notes/2026-05-28_figma-plugin-architecture.md) ไม่ทำซ้ำที่นี่

### ฟิลด์

| ฟิลด์ | ชนิด | บังคับ | ความหมาย |
|---|---|---|---|
| `file_key` | string | ใช่ | Figma file key (จาก URL ของไฟล์) คู่กับ `node_id` เป็น **key ภายนอกที่เสถียร** ที่ mapping ยึด |
| `node_id` | string | ใช่ | node ภายในไฟล์ (frame / component / instance / …) ครึ่งที่สองของ stable key |
| `name` | string | ใช่ | ชื่อ node projection ที่เปลี่ยนได้ของ Figma state รีเฟรชทุก `fetch` |
| `type` | string | ใช่ | ชนิด node (`FRAME`, `COMPONENT`, `INSTANCE`, …) |
| `last_modified` | string (ISO 8601 datetime) | ใช่ | timestamp last-modified ของไฟล์จาก Figma — สัญญาณ cache-invalidation ของ content-addressable export |
| `exported_path` | string | ไม่ | path (เทียบ workspace) ของรูปที่ export ใต้ `figma/exports/` ตัดออกจนกว่าจะ export |
| `image_url` | string | ไม่ | render URL ที่ Figma host จากการ export **ไม่ durable** — render URL ของ Figma หมดอายุ; artifact ที่ durable คือ `exported_path` ไม่ใช่ตัวนี้ ตัดออกเมื่อไม่ขอ |
| `design_tokens` | object | ไม่ | design token ที่สกัด `{ name: value }` (สี, spacing, type) ผูกกับ node นี้ — สะพาน design→spec ตัดออกเมื่อไม่มี |

ฟิลด์ไม่บังคับจะ **ตัดออกจาก entry** เมื่อไม่มีค่า — node ที่ไม่เคย export ไม่มี key `exported_path` — ไม่ serialize เป็น `null` ตาม convention ของ Audit Findings / Jira / OKR / Council

### ความเสถียรของฟิลด์

`file_key` + `node_id` เป็น stable key — คู่ที่ consumer (dashboard, การอ้าง token ใน spec doc) ถือเป็น identifier ถาวรได้ ฟิลด์อื่นเป็น projection ที่เปลี่ยนได้ของ Figma state (`name`, `type`, `last_modified`, `design_tokens`) หรือผลการ export ในเครื่อง (`exported_path`, `image_url`) รีเฟรชทุก `fetch`/`export`; อย่า key บนมัน `image_url` โดยเฉพาะไม่ durable (หมดอายุ) — เก็บ `exported_path` แทน

### ตัวอย่าง

```json
{
  "file_key":      "AbC123dEf456",
  "node_id":       "12:345",
  "name":          "Primary Button",
  "type":          "COMPONENT",
  "last_modified": "2026-05-27T09:00:00Z",
  "exported_path": "figma/exports/9f86d081884c7d65.png",
  "design_tokens": { "color/primary": "#2D7FF9", "radius/sm": "4px" }
}
```

---

## โครงสร้างไดเรกทอรี

```
modules/plugins/
└── <name>/
    ├── manifest.toml       # บังคับ — contract
    ├── SPEC.md             # บังคับ — รายละเอียดในรูปแบบ Obsidian
    └── ...                 # implementation (binary, Rust crate, script)
```

`<name>` เป็น `kebab-case` หนึ่งปลั๊กอินต่อหนึ่งไดเรกทอรี `kind` ของปลั๊กอินประกาศใน `manifest.toml` (ดู [Manifest](#manifest--manifesttoml)) ไม่ encode ลงใน path — สมมาตรกับ [`SKILLS.th.md`](SKILLS.th.md#โครงสร้างไดเรกทอรี)

---

## Manifest — `manifest.toml`

```toml
[plugin]
name        = "memory-tier2-noop"               # บังคับ — ต้องตรงกับชื่อไดเรกทอรี
kind        = "memory-backend"                  # บังคับ — หนึ่งใน: memory-backend | llm-backend | workflow | audit | jira
version     = "0.1.0"                           # บังคับ — semver
description = "No-op Tier 2 memory backend that forwards to Tier 1."   # บังคับ — สรุปหนึ่งประโยค
compat      = ">=2.5.0"                         # บังคับ — semver range; เวอร์ชันเฟรมเวิร์กที่ปลั๊กอินนี้ใช้ได้
entry       = "bwoc-plugin-memory-tier2-noop"   # บังคับ — binary บน PATH (แนะนำ) หรือชื่อ Rust crate ข้างเคียง

[config.schema]                                 # ไม่บังคับ — ตัดทั้ง table ออกได้ถ้าปลั๊กอินไม่รับ config
# ปลั๊กอินกำหนดเอง; JSON-schema-lite ตาราง [plugins.<name>] ของ workspace ถูก validate กับ schema นี้
# แต่ละ key map ไปยัง inline table ที่มี: type, required (bool), และ (เมื่อ required = false) default
# ตัวอย่าง:
# storage_path = { type = "string", required = false, default = "memories/tier2" }
# max_results  = { type = "integer", required = true }
```

### อ้างอิงฟิลด์

| Section | Field | บังคับ | ชนิดข้อมูล | ความหมาย |
|---|---|---|---|---|
| `[plugin]` | `name` | ใช่ | string (kebab-case) | ชื่อปลั๊กอิน; ต้องตรงกับชื่อไดเรกทอรีใต้ `modules/plugins/` |
| `[plugin]` | `kind` | ใช่ | enum | หนึ่งใน `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`; เปลี่ยนไม่ได้หลัง `init` |
| `[plugin]` | `version` | ใช่ | string (semver) | Semver ของปลั๊กอินเอง แยกจากเวอร์ชันเฟรมเวิร์ก |
| `[plugin]` | `description` | ใช่ | string | สรุปหนึ่งประโยค; เป็นค่า **ที่เดียว** ใน manifest ที่ยอมให้มีชื่อ vendor |
| `[plugin]` | `compat` | ใช่ | string (semver range) | ช่วงเวอร์ชันเฟรมเวิร์กที่ปลั๊กอินนี้ใช้ได้; ถ้าไม่ตรงเฟรมเวิร์กปฏิเสธการ load |
| `[plugin]` | `entry` | ใช่ | string | Binary บน `PATH` (แนะนำ) หรือชื่อ Rust crate ข้างเคียงที่เฟรมเวิร์ก dispatch ไป |
| `[config.schema]` | (free keys) | ไม่ | inline-table ต่อ key | Schema สำหรับ validate `workspace.toml [plugins.<name>]`; แต่ละ key ระบุ `type`, `required`, และ `default` (ไม่บังคับ) |

### ข้อจำกัดเรื่องความเป็นกลาง (HARD)

ปลั๊กอินประเภท `memory-backend` ต้องทำงานได้กับ agent ทุกตัวไม่ว่าใช้ backend ใด ปลั๊กอินประเภท `llm-backend` ต้องไม่แอบอ้างเป็นหนึ่งใน backend ห้าตัวที่ประกาศ ชื่อ vendor ใน **manifest values** ทนได้เฉพาะใน `description` (ที่อธิบายเป้าหมายของ integration); ที่อื่นยังห้ามอยู่ กฎ **Samānattatā** เดียวกันกับที่ `bwoc check` บังคับใช้กับ `AGENTS.md` อยู่แล้ว

---

## Lifecycle

```
init  → configure → invoke (หลายครั้ง) → teardown
```

- **`init`** — เรียกครั้งเดียวเมื่อเฟรมเวิร์กเห็นปลั๊กอินใน `workspace.toml` ครั้งแรก **Idempotent** ไม่มี side-effect ต่อระบบภายนอกเกินกว่าที่จำเป็นเพื่อยืนยันว่าปลั๊กอินรันได้
- **`configure`** — เรียกพร้อมบล็อก config `[plugins.<name>]` ที่ resolve แล้ว **Idempotent**: เรียกซ้ำด้วย block เดิมเป็น no-op; เรียกด้วย block ใหม่จะ reconcile ไปยังสถานะใหม่ Validate config กับ `[config.schema]`; ปฏิเสธหาก schema violate
- **`invoke`** — เรียกต่อ logical operation (เขียน memory, dispatch model call, post ไปยัง issue tracker) **Idempotent ที่ระดับ operation**
- **`teardown`** — เรียกครั้งเดียวเมื่อเฟรมเวิร์ก shutdown หรือปลั๊กอินถูก disable **Idempotent** เป็นการ cleanup เท่านั้น

Idempotency เป็น **ข้อกำหนดบังคับทุกเฟส** เฟรมเวิร์กอาจ retry init หรือ configure หลัง crash; `invoke` อาจรันสองครั้งหากผู้เรียกของเฟรมเวิร์ก retry; teardown อาจถูก replay ข้าม shutdown ปลั๊กอินที่ mutate external state แบบไม่ idempotent คือพังโดยการออกแบบ

### ผู้รับ lifecycle ต่อ kind

| Kind | ผู้รับ | เมื่อ init ทำงาน | เมื่อ invoke ทำงาน |
|---|---|---|---|
| `memory-backend` | Memory subsystem ของ agent | การ read/write memory ครั้งแรกที่ escalate ไป Tier 2 | ต่อการ read/write Tier 2 |
| `llm-backend` | `bwoc spawn` | Agent spawn ที่ entry ใน registry ระบุปลั๊กอินนี้ | ต่อ model call จาก harness ของ agent |
| `workflow` | โค้ด agent ที่ import integration | การเรียกครั้งแรกจาก agent | ต่อ operation ที่ agent เริ่ม |
| `audit` | `bwoc audit` CLI | `bwoc audit run` ครั้งแรกที่เลือกปลั๊กอินนี้ในการเรียกปัจจุบัน | ต่อการเรียก `bwoc audit run [--plugin <name>]` โดย operator; ไม่เรียกโดย implicit |

### สัญญา Hook — success, failure, partial state

ปลั๊กอินเชื่อมผ่านฟิลด์ `entry` — เป็น binary บน `PATH` หรือ Rust crate ข้างเคียง contract จึงแสดงทั้งในรูป exit-code (binary) และ return-value (crate); เฟรมเวิร์กถือว่าทั้งสองเทียบเท่ากัน สำหรับแต่ละ hook คำว่า "success" และ "failure" คือผลลัพธ์ที่เฟรมเวิร์กเห็น; "partial state" เป็นความรับผิดชอบของผู้เขียนปลั๊กอินที่จะ bound ไว้

| Hook | Success คือ | Failure คือ | Partial state |
|---|---|---|---|
| `init` | Exit `0` (binary) / return `Ok` (crate) | Non-zero exit / `Err` เฟรมเวิร์กปฏิเสธการ load ปลั๊กอินและส่ง diagnostic ไปยัง stderr | init ต้องเสร็จเต็มหรือ roll back ก่อนล้มเหลว เฟรมเวิร์กถือว่า init ที่ล้มเหลวเหมือนไม่เคยรัน |
| `configure` | Exit `0` / `Ok` ปลั๊กอินพร้อมรับ `invoke` | Non-zero exit / `Err` ระบุ key ที่ผิด (เช่น `max_results: required, missing`) เฟรมเวิร์กปฏิเสธการ start workspace | Validate-first, apply-second — ไม่ apply ครึ่ง ๆ การ apply ครึ่งคือ bug ของปลั๊กอิน |
| `invoke` | Exit `0` / `Ok` พร้อม typed result Stdout = payload, stderr = diagnostic (รูปแบบ binary) | Non-zero exit / `Err` เฟรมเวิร์กส่ง error ต่อให้ caller (agent หรือ operator); caller ตัดสินใจว่าจะ retry หรือไม่ | Operation ต้อง durable-or-discarded — ไม่ apply ครึ่ง ๆ การ retry ตกบน path ที่ idempotent |
| `teardown` | Exit `0` / `Ok` เฟรมเวิร์ก release slot ของปลั๊กอิน | Non-zero exit / `Err` Log แต่ไม่ fatal — การ shutdown ของเฟรมเวิร์กต้องไม่ block | Idempotent บน replay เฟรมเวิร์กอาจเรียก teardown อีกครั้งใน shutdown ครั้งถัดไปหากครั้งแรกไม่จบ |

### ตัวอย่างต่อเฟส

```text
# init — ยืนยันว่าปลั๊กอินรันได้; ยังไม่ side-effect ทางธุรกิจ
init():
  if not writable(cfg.storage_path):
    exit 1, "storage_path not writable: <path>"
  open_lazy_handle(cfg.storage_path)
  exit 0

# configure — validate กับ [config.schema] แล้ว apply แบบ atomic
configure({ storage_path: "memories/tier2", max_results: 8 }):
  errors = validate_against_schema(input)
  if errors:
    exit 2, "configure: " + errors.join(", ")
  apply_atomic(input)               # all-or-nothing
  exit 0

# invoke — idempotent ที่ระดับ operation
invoke("write_memory", { id: "m-1", body: "..." }):
  existing = lookup("m-1")
  if existing and body_hash(existing) == body_hash(input):
    exit 0, { status: "noop" }      # replay-safe
  store("m-1", input)
  exit 0, { status: "written" }

# teardown — cleanup เท่านั้น, idempotent
teardown():
  flush_pending(timeout = 5s)       # best-effort
  close_handles()
  exit 0                            # ปลอดภัยที่จะเรียกซ้ำ
```

---

## การโหลด — `workspace.toml`

Operator ประกาศปลั๊กอินที่ workspace นี้ใช้โดยเพิ่ม entry ใน `workspace.toml`:

```toml
[plugins]

[plugins.memory-tier2-noop]
enabled      = true
storage_path = "memories/tier2"

[plugins.workflow-github]
enabled = false      # ลงทะเบียนแต่ปิด — เก็บไว้เพื่อระบุเจตนา
```

Schema ของแต่ละ table `[plugins.<name>]`:

- `<name>` (table key, string, บังคับ) — ชื่อไดเรกทอรีของปลั๊กอินที่ติดตั้งใต้ `modules/plugins/` Key คือชื่อปลั๊กอิน; **ไม่** ประกาศ `kind` ที่นี่ — `kind` เป็นของ manifest ของปลั๊กอินเอง (`[plugin].kind` ใน `manifest.toml`) และอ่านจากที่นั่นตอนโหลด
- `enabled` (bool, บังคับ) — กำหนดว่าปลั๊กอินจะถูกโหลดตอนเฟรมเวิร์ก startup หรือไม่ ตั้ง `false` เพื่อเก็บ entry ไว้แสดงเจตนาแต่ไม่ load สอดคล้องกับ pattern `config.manifest.json skills.framework[] enabled` ใน [`SKILLS.th.md`](SKILLS.th.md#discovery); ใช้ `bwoc plugin disable <name>` เพื่อ flip โดยไม่ลบ entry
- Key อื่นทั้งหมด (ปลั๊กอินกำหนดเอง) — validate กับ `[config.schema]` ของปลั๊กอินตอนเฟรมเวิร์ก startup ปฏิเสธเมื่อ schema ผิด; ไม่ apply ครึ่ง ๆ (ดู [Lifecycle](#lifecycle))

Entry ที่ไม่มีฟิลด์ `enabled` ถือเป็น manifest error — `bwoc check` จะปฏิเสธ ไม่มี default โดยปริยาย; เจตนาที่ชัดเจนคือ contract

เมื่อเฟรมเวิร์ก startup runtime จะ:

1. อ่าน table `[plugins]` จาก `workspace.toml`
2. กรองเฉพาะ entry ที่ `enabled` เป็น `true` Entry ที่ `enabled = false` ยังอยู่ใน `workspace.toml` (เป็นเจตนาที่บันทึกไว้) แต่ถูกข้ามตอนโหลด
3. Resolve แต่ละ entry กับไดเรกทอรี `modules/plugins/<name>/` ของ workspace `<kind>` อ่านจาก manifest ของปลั๊กอินที่ติดตั้ง ไม่ encode ลงใน path
4. Validate config block ของ entry กับ `[config.schema]` ของปลั๊กอิน แล้ว dispatch `init` ตามด้วย `configure`
5. ปฏิเสธการ start workspace เมื่อปลั๊กอินที่เปิดอยู่ไม่มีอยู่ใต้ `modules/plugins/`, `[plugin] compat` ไม่ตรงกับเวอร์ชันเฟรมเวิร์กที่รันอยู่, validate `[config.schema]` ไม่ผ่าน, หรือ `init` / `configure` ส่งกลับผลที่ไม่ใช่ zero

ไม่มี central index ปลั๊กอินมีอยู่สำหรับ workspace เพราะถูกติดตั้งใน `modules/plugins/` และมีชื่ออยู่ใน `workspace.toml` เท่านั้น การ resolve ตอน startup เป็นแบบ local ต่อ workspace เสมอ — ไม่มี network call ตอน runtime **Anattā** ยังคงอยู่

---

## CLI Surface

Surface แบบ read-only (ไม่มี side-effect กับ workspace):

```
bwoc plugin list                    # ลิสต์ปลั๊กอินที่ติดตั้ง (เปิดและปิด)
bwoc plugin list --enabled          # กรองเฉพาะที่เปิด
bwoc plugin list --kind memory-backend
bwoc plugin list --json

bwoc plugin show <name>             # manifest + spec + config ปัจจุบันแบบเต็ม
bwoc plugin show <name> --json
```

Surface สำหรับ lifecycle (write — รายละเอียดดูในหัวข้อที่อ้างถึง):

```
bwoc plugin init <name> --kind <k>  # scaffold ปลั๊กอินใหม่จาก modules/plugin-template/
                                    #   (ดู "Scaffolding from template")

bwoc plugin install <source>        # ติดตั้งจาก local path / git URL / tarball URL
                                    #   (ดู "Sources & Installation")

bwoc plugin enable <name>           # ตั้ง enabled=true ใน workspace.toml [plugins.<name>]
bwoc plugin disable <name>          # ตั้ง enabled=false (เก็บ entry ไว้)

bwoc plugin remove <name>           # ลบ modules/plugins/<name>/ และ clean workspace.toml
                                    #   (ดู "Removal")
```

ไม่มี `bwoc plugin verify` ใน v1 — ปลั๊กอินไม่ประกาศ verify gate มาตรฐาน (kind ต่างกันมาก) Verification เป็นเรื่องของปลั๊กอินเอง แสดงผ่าน exit semantics ของ `invoke` v2 ในอนาคตอาจเพิ่ม verify ต่อ kind หากเห็น pattern ที่ชัด

คำสั่ง read-only ทั้งหมดมี `--json` คู่ คำสั่ง lifecycle emit JSON ที่มีโครงสร้างเมื่อใส่ `--json`; `install` exit non-zero เมื่อ trust-gate ไม่ผ่าน; `remove` exit non-zero เมื่อ target ไม่มี เว้นแต่ใส่ `--yes`

### Resolve "current workspace"

Plugin มี scope ที่ workspace (ต่างจาก skill ที่ scope ที่ agent) `enable`, `disable`, `remove` resolve target workspace ตามลำดับ:

1. **flag `--workspace <path>`** — override ชัดเจน
2. **environment variable `BWOC_WORKSPACE`**
3. **Working directory** — walk ขึ้นจาก cwd หา `.bwoc/workspace.toml` ที่ใกล้ที่สุด
4. **อื่น ๆ** — error: `no workspace context; pass --workspace <path> or run from inside a workspace`

Resolution นี้เหมือนกับที่ `bwoc list` และ `bwoc workspace info` ใช้หา workspace ทุกวันนี้

---

## Sources & Installation

Framework plugin เข้าสู่ workspace ได้ 2 ทาง — เขียนเองใต้ `modules/plugins/<name>/` หรือ install จาก 3 ประเภท source:

| ประเภท Source | ตัวอย่าง | การตรวจจับ |
|---|---|---|
| **Local path** | `bwoc plugin install ./vendor/my-plugin/` | argument ขึ้นต้นด้วย `./`, `../`, หรือ `/` และ resolve เป็นไดเรกทอรี |
| **Git URL** | `bwoc plugin install https://github.com/org/plugin.git#v0.1.0` | argument มี scheme `http(s)://` หรือ `git://` และลงท้าย `.git` (`#<ref>` เลือก branch / tag / sha) |
| **Tarball URL** | `bwoc plugin install https://example.com/plugin-0.1.0.tar.gz` | argument มี scheme `http(s)://` และลงท้าย `.tar.gz` หรือ `.tgz` |

กลไกการ install:

1. Resolve ประเภท source จาก argument
2. **Pre-flight** — ถ้า source ไม่มี `manifest.toml` ที่ root → refuse พร้อม error `source missing manifest.toml; cannot resolve name or kind` ไม่ fetch / extract / write อะไร
3. **Trust gate** (ดูข้างล่าง) — fetch และ verify SHA-256 checksum
4. อ่าน manifest ของปลั๊กอินจาก source เพื่อรู้ `name` และ `kind` Kind **มาจาก source manifest เสมอ** — flag ไม่สามารถ override ได้
5. Materialize source ลง `modules/plugins/<name>/` (copy สำหรับ local; clone-แล้ว-ทิ้ง-`.git` สำหรับ git; extract สำหรับ tarball)
6. Validate manifest ที่ติดตั้งด้วย `bwoc check`
7. บันทึก install ลง `.bwoc/installed-sources.toml` (schema ด้านล่าง) เขียน registry record เฉพาะเมื่อสำเร็จเท่านั้น
8. **ไม่ auto-enable** ปลั๊กอินที่ติดตั้งแล้ว dormant จนกว่า `bwoc plugin enable <name>` จะเพิ่ม entry ใน `workspace.toml [plugins.<name>]` พร้อม `enabled = true`

### Re-install และการจัดการความล้มเหลว

- **Target มีอยู่แล้ว** — ถ้า `modules/plugins/<name>/` มีอยู่แล้ว default behavior คือ refuse พร้อม `<name> already installed at version X; pass --upgrade to replace`
  - `--upgrade` — แทนที่ในที่เดิม เก็บ record ใน `installed-sources.toml` (update `last_hash` และ `installed_at`)
  - `--force` — แทนที่ไม่มีเงื่อนไข แม้ install ปัจจุบันมี local edit ที่ยังไม่ commit (stderr warning ระบุสิ่งที่ถูก overwrite)
- **Network failure ระหว่าง install** — install ไม่ atomic by design; เมื่อล้มเหลวชั่วคราว (download ขาด, extract error) ไดเรกทอรีบางส่วนถูกลบก่อน exit และ `installed-sources.toml` **ไม่** ถูก update ปลอดภัยที่จะ retry

### Trust gate (v1)

ทุกการ install verify SHA-256 checksum **ก่อน** materialize:

- **Tarball URL** — CLI fetch `<source>.sha256` (URL เดียวกัน + suffix `.sha256`) อ่าน digest ที่คาดหวัง และเทียบกับ digest ของ archive ที่ดาวน์โหลด
- **Git URL** — CLI fetch checksum ที่ URL โดยแทนที่ `.git` ด้วย `.sha256` ตัวอย่าง:
  - Source: `https://github.com/org/plugin.git#v0.1.0`
  - Checksum: `https://github.com/org/plugin.sha256` (operator เผยแพร่ manifest ของ tree-sha ที่คาดหวังตาม ref)
  - หลัง clone เฟรมเวิร์กรัน `git rev-parse <ref>^{tree}` และเทียบกับ entry สำหรับ `<ref>` ใน manifest ที่ fetch มา
  - Operator มักจะเผยแพร่ manifest นี้ผ่าน GitHub release asset หรือไฟล์ static-hosted แยก
- **Local path** — checksum เป็น optional ถ้ามีไฟล์ `<dir>.sha256` ข้างไดเรกทอรีจะ verify ถ้าไม่มีก็ install ต่อ (local path ถือเป็น operator-trusted by convention)

มี 2 flag ที่ผ่อนปรน gate:

- `--no-verify` — ข้าม checksum verification emit stderr warning ใช้กับ source ที่ develop อยู่และ serve local ผ่าน HTTP
- `--allow-new-source` — บังคับ **ครั้งแรก** ที่ install source URL หนึ่งในเวิร์กสเปซนี้ เป็นการระบุว่า "ฉันได้ตรวจสอบ source นี้แล้ว" Install ครั้งถัดไปจาก source เดิม (บันทึกใน `.bwoc/installed-sources.toml`) จะข้าม prompt นี้

Trust gate ตรงกับ spec ของ SKILLS — flag เดียวกัน, ไฟล์ registry เดียวกัน, semantics เดียวกัน Trust v2 ในอนาคต (signed envelopes; identity proof) ขยายทั้งสอง surface โดยไม่ break v1 contract

**Anattā ยังคงอยู่** ไม่มี central registry ไม่มี service สำหรับ resolve name-to-URL ไม่มี auto-update ทุกการ install ระบุ source อย่างชัดเจน เฟรมเวิร์กไม่ใช่ package manager

### Schema ของ `.bwoc/installed-sources.toml`

ใช้ร่วมกับ SKILLS — registry เดียวของ workspace ครอบทั้งสองประเภทการ install ดู [`SKILLS.th.md` — installed-sources schema](SKILLS.th.md#schema-ของ-bwocinstalled-sourcestoml) สำหรับตารางเต็ม; entry ของ plugin ใช้ `kind = "plugin"` และ `target = "modules/plugins/<name>"`

---

## Scaffolding from template

`bwoc plugin init <name> --kind <kind>` สร้างปลั๊กอินใหม่ใน `modules/plugins/<name>/` โดย copy template ที่ `modules/plugin-template/` และแทนที่ placeholder (รวม `kind`):

```
modules/plugin-template/
├── manifest.toml          # มี {{pluginName}}, {{pluginVersion}}, {{pluginKind}} เป็น placeholder
└── SPEC.md                # รูปแบบ Obsidian; มี placeholder สำหรับชื่อปลั๊กอินและคำอธิบาย
```

Placeholder ใช้รูปแบบ `{{camelCase}}` เดียวกับ `modules/agent-template/` และ `modules/skill-template/` รายการ substitute ที่บังคับอยู่ใน [`SPEC.md`](../../modules/plugin-template/SPEC.md) ของ template เอง

flag `--kind` บังคับ — ไม่มี default ค่าที่ถูกต้อง: `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira` Kind ในอนาคตขยาย enum นี้โดยไม่เปลี่ยนโครงสร้าง template flag นี้ทำให้ operator ต้องระบุเจตนาตั้งแต่ต้น และเลี่ยง manifest ที่มี `kind` field หาย/ผิด

`bwoc plugin init` เป็นวิธีที่แนะนำสำหรับเริ่มปลั๊กอินใหม่ — สร้างเองด้วยมือทำได้แต่ข้าม consistency ของ placeholder

### `init` vs `install` — ทำไม `--kind` ทำงานต่างกัน

`init` และ `install` จัดการ `kind` แบบไม่สมมาตรโดยตั้งใจ:

- **`init <name> --kind <kind>`** — operator ระบุเจตนา; `--kind` ถูก substitute ลงใน template manifest ตัวใหม่ บังคับเพราะยังไม่มี manifest ให้ derive kind จาก
- **`install <source>`** — `kind` อ่านจาก `manifest.toml` ของ source override ไม่ได้ — source manifest ที่ระบุ `kind = "memory-backend"` จะถูก install โดยคง kind นั้นไว้ใน manifest เสมอ ไม่ว่าใส่ flag ใด

ความไม่สมมาตรนี้มีเพราะ install flow เชื่อเจตนาของผู้เขียน source: ถ้า source บอกว่าเป็น `workflow` plugin ก็เป็น `workflow` plugin Operator ที่ไม่เห็นด้วยควรปฏิเสธการ install ไม่ใช่แก้ manifest ภายหลัง

---

## Removal

`bwoc plugin remove <name>`:

1. **Confirm กับ user** เว้นแต่ใส่ `--yes` แสดงสิ่งที่จะลบ (`modules/plugins/<name>/`) และแก้ (`workspace.toml [plugins.<name>]`); รายงาน `kind` ของปลั๊กอิน (อ่านจาก manifest) เป็น context
2. **ลบ** `modules/plugins/<name>/` แบบ recursive
3. **Clean** `workspace.toml` — ลบ table `[plugins.<name>]` ทั้งหมด ไม่ใช่แค่ตั้ง `enabled = false`

Idempotent — `remove` กับ target ที่ไม่มีจะรายงาน "not installed" และ exit 0 flag `--yes` ข้าม confirmation prompt

Source ที่ถูกลบไม่ถูก auto-uninstall จาก `.bwoc/installed-sources.toml` ใส่ `--forget-source` เพื่อลบ source registration ด้วย

---

## Verification

`bwoc check` ขยายไปตรวจสอบ `modules/plugins/<name>/` รวมถึง registry ของ source ที่ติดตั้ง:

| Check | เงื่อนไขผ่าน |
|---|---|
| Manifest parseable | `manifest.toml` เป็น TOML ที่ valid และตรง schema ด้านบน |
| ชื่อตรงกับไดเรกทอรี | `[plugin].name == basename(directory)` |
| Kind valid | `[plugin].kind` เป็นหนึ่งใน `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira` (หรือ kind ในอนาคตที่เพิ่มเข้า enum) |
| Neutrality | ชื่อ vendor ปรากฏเฉพาะใน `description`; ที่อื่นไม่ได้ |
| มี `SPEC.md` | ไฟล์ `SPEC.md` อยู่ข้าง manifest |
| ฟิลด์บังคับครบ | `name`, `kind`, `version`, `description`, `compat`, `entry` ครบ |
| ช่วง compat valid | `[plugin].compat` parse เป็น semver range ได้ |
| Source registry parseable | `.bwoc/installed-sources.toml` เป็น TOML ที่ valid ถ้ามี |
| ไม่มี orphan source record | ทุก entry ที่ `kind = "plugin"` ใน registry มี `modules/plugins/<name>/` ที่ match |
| ไม่มี orphan installation | ทุก `modules/plugins/<name>/` มี registry entry หรือมี marker file `.authored-in-place` |
| Registry drift | `installed_hash` ใน registry match SHA-256 ปัจจุบันของ `modules/plugins/<name>/` (หรือใส่ `bwoc check --update-hashes` เพื่อรับทราบ drift) |

Check ที่ไม่ผ่าน exit non-zero ใน workspace audit — surface เดียวกัน, exit semantics เดียวกันกับ `bwoc check --all` ที่มีอยู่

---

## สิ่งที่ Spec นี้ไม่ครอบคลุม

- **Skills** — ดู [`SKILLS.th.md`](SKILLS.th.md) Skill ถูก agent invoke; plugin ถูก framework load
- **ห้า backend ที่ประกาศ** (`claude`, `antigravity`, `codex`, `kimi`, `ollama`) — เป็น first-class ไม่ใช่ plugin ดู [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md)
- **Reference plugin ตัวแรกเอง** — ดู story `BWOC-7` และ (เมื่อลงแล้ว) `modules/plugins/memory-tier2-noop/SPEC.md`
- **Trust v2 / signing ของ plugin binary** — เลื่อนออก ปัจจุบัน plugin binary trust ได้เพราะติดตั้งใต้ `modules/plugins/`; trust gating ที่ละเอียดกว่าจะลงพร้อมงาน Trust v2 ในภาพรวม

---

## ดูเพิ่ม

- [`SKILLS.th.md`](SKILLS.th.md) — spec พี่น้อง; substrate เดียวกัน, invoker ต่างกัน
- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — modules ประกอบกับส่วนอื่นของเฟรมเวิร์กอย่างไร
- [`WORKSPACE.th.md`](WORKSPACE.th.md) — schema ของ `workspace.toml`; spec นี้ขยายด้วย `[plugins]`
- [`HARNESS.th.md`](HARNESS.th.md) — ollama harness; แบบที่ปลั๊กอิน `llm-backend` ในอนาคตจะตาม
- [`NAMING.th.md`](NAMING.th.md) — แนวทางตั้งชื่อไฟล์และไดเรกทอรี
- [`GLOSSARY.th.md`](GLOSSARY.th.md) — คำบาลี (Anattā, Samānattatā, Mattaññutā)
