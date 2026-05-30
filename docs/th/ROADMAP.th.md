---
title: แผนพัฒนา
parent: ภาษาไทย
nav_order: 6
---

# Roadmap

แผนทีละ phase ของ BWOC **Phase** อธิบาย milestone ของการ implement; แต่ละ phase อาจครอบคลุม SemVer release หลายครั้ง ดู [`VERSION.md`](../../VERSION.md) สำหรับการแยก version กับ phase ดู [`VISION.th.md`](../../VISION.th.md) สำหรับ success criteria ที่ 1 ปีและ 3 ปี

---

## สถานะปัจจุบัน

**Phase ที่ active:** Phase 3 — *วยะ + Interconnect* — **DoD บรรลุแล้ว** (interconnect routing + worktree lifecycle + `bwoc retire` full vaya ship ครบ 2026-05-23) รายการ Phase 3 ที่เหลือ (Trust v2, Tier 2 memory) เลื่อนออก อยู่นอก DoD DoD ของ Phase 1 v2.0 และ Phase 2 ก็บรรลุแล้ว **BWOC 2.0** release เป็น `v2026.5.23-2`
**Software-Version:** ดู [`VERSION.md`](../../VERSION.md)
**Document-Version:** ดู [`VERSION.md`](../../VERSION.md)

---

## Phase 1 v2.0 — รากฐาน อุปฺปาท

**นิยามของเสร็จ:** end-to-end **อุปฺปาท** สำหรับ backend หนึ่งตัว — incarnate · check · spawn agent ที่รันได้

### เสร็จแล้ว

- Cargo workspace (`bwoc-core`, `bwoc-cli`, `bwoc-agent`) scaffold; edition 2024; MSRV 1.85
- `VERSION.md` มี `Software-Version`, `Document-Version`, และ `Last-Updated`; auto-managed โดย `.claude/hooks/auto-version.sh`
- Open-source hygiene: `VISION.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`; root `README.md` พร้อม badge, TOC, footer
- เอกสารสเปก (bilingual EN/TH ทุกตัว): `PHILOSOPHY` §0.1 *วงรอบ*, `GLOSSARY`, `ARCHITECTURE`, `INCARNATION`, `WORKSPACE`, `NAMING`
- Crate README (`bwoc-core`, `bwoc-cli`, `bwoc-agent`)
- เครื่องมือ Claude Code: 4 project skills (`/incarnate`, `/check-neutrality`, `/check-bilingual`, `/task-log`); 2 PostToolUse hooks (`bilingual-reminder`, `auto-version`)
- shell script `incarnate.sh` และ `check-agent-neutrality.sh` ใน template (ใช้ได้วันนี้; จะถูก port เป็น Rust)

### ส่งมอบใน Phase 1 v2.0 (เสร็จแล้ว)

รายการทั้งหมดด้านล่าง implement แล้ว Definition of Done ของ phase นี้ (uppāda end-to-end สำหรับ backend หนึ่ง) **บรรลุ** เหลือเฉพาะ HELD policy items (`CODEOWNERS` · `ISSUE_TEMPLATE/config.yml`) ที่รอ user direction; release pipeline พร้อมใช้แล้ว (ดู Phase 2)

| รายการ | สเปก | สถานะ |
|---|---|---|
| `bwoc init [path]` | [`WORKSPACE.th.md`](WORKSPACE.th.md#cli-surface) | ✓ |
| `bwoc workspace info` · `validate` | [`WORKSPACE.th.md`](WORKSPACE.th.md#cli-surface) | ✓ |
| `bwoc new <name>` (port ของ `incarnate.sh`) | [`INCARNATION.th.md`](INCARNATION.th.md) | ✓ |
| `bwoc check [path]` (port ของ `check-agent-neutrality.sh`) | [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) | ✓ |
| `bwoc spawn <name>` (minimal `exec`) | [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md#การไหลของข้อมูล--bwoc-spawn-agent-foo) | ✓ |
| `bwoc list` (อ่าน `.bwoc/agents.toml`) | [`WORKSPACE.th.md`](WORKSPACE.th.md) | ✓ |
| flag `--lang` wired เข้ากับ Project Fluent (locale TH + EN) | [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) | ✓ ครบ 8 surface (init/list/spawn/workspace info/workspace validate/check/new/bwoc-agent) |
| Skill `/check-naming` (audit `*.md` กับ `NAMING.th.md`) | [`NAMING.th.md`](NAMING.th.md#audit) | ✓ + wired เข้า `.github/workflows/docs.yml` |
| Runtime ทำงานจาก directory ใดก็ได้ | template ของ agent embedded ผ่าน `include_dir!` + env `BWOC_TEMPLATE` + cache `~/.bwoc/template/` | ✓ |
| Bump version major/minor ด้วยมือ | `scripts/bump-version.sh <level> [--software\|--document\|--both]` | ✓ (patch ยัง auto-bump ผ่าน hook) |

---

## Phase 2 — การปฏิบัติ ฐิติ

**นิยามของเสร็จ:** agent ดำเนินงานพร้อม control surface จริง; backend หลายตัวถูกใช้งาน; release ทำซ้ำได้

### ส่งมอบใน Phase 2 (เสร็จแล้ว)

| รายการ | หมายเหตุ |
|---|---|
| Daemon `bwoc-agent --serve` | Unix-only (`.bwoc/agent.pid` + `.bwoc/agent.sock`; stub cfg-gated บน Windows) |
| IPC control socket — protocol แบบ line-text | `PING`/`STATUS`/`STOP` ผ่าน Unix domain socket; debug ได้ด้วย `nc -U` |
| `bwoc status [name]` | health + runtime indicator (●/○) + uptime ผ่าน socket query; `--all` พิมพ์ detail block ของทุก agent (loop ของ single-agent view; `[name]` กับ `--all` เป็น clap-mutex) |
| `bwoc list` | registry view + runtime indicator + UPTIME column (5m12s เมื่อ alive) + INBOX count; filter `--running` / `--status` / `--backend` / `--inbox-pending` (รวมกันได้); `--sort id\|inbox\|incarnated\|backend` (stable; default = registry order); `--count` (เฉพาะจำนวนแถว) / `--names-only` (bare ids สำหรับ shell loop); JSON มี `uptime_seconds` ต่อ agent (nullable); ใช้ทั้ง human + `--json` |
| `bwoc send <to> <msg>` + `bwoc inbox <agent>` | JSONL inbox ที่ `<agent>/.bwoc/inbox.jsonl` `send` body: inline `<msg>` หรือ `--file <path>` (clap mutex) `inbox`: `--watch` / `--clear` / `--limit` / `--json` / `--count` (envelope count สำหรับ shell script); `--watch --json` stream JSON envelope หนึ่ง envelope ต่อบรรทัด สำหรับ log shipper; `--all` พิมพ์ inbox ของทุก agent ต่อกัน พร้อม header (ปฏิเสธ `--clear` / `--watch`) |
| `bwoc doctor` | env + workspace diagnostic; `--auto` กวาด `agent.pid` / `agent.sock` / `inbox.cursor` ที่ stale; WARN กรณี `agent.log` ใหญ่ (10 MiB, `--auto` truncate) + `inbox.jsonl` ใหญ่ (5 MiB, WARN-only — user data); `--json` สำหรับ shape stable ใช้ CI gating |
| `bwoc start <name>` (idempotent) | flip registry + spawn `bwoc-agent --serve` ถ้ายังไม่ทำงาน; `--no-daemon` ข้าม spawn; `--all` mass-start agent ที่ stopped ทั้งหมด; `--json` (ต้องคู่กับ `--yes`) emit `{ workspace, agent, daemon_spawned, daemon_pid, already_running, registry_updated }` สำหรับ scripted lifecycle |
| `bwoc ping <name>` | CLI client สำหรับคำสั่ง PING ของ daemon; `--all` mass-ping ทุก agent (not-running label แต่ไม่นับเป็น fail; protocol drift / connection error → exit 1) |
| `bwoc chat <name>` (+ `--tmux`) | resolve backend จาก registry; exec `bwoc spawn` |
| `bwoc dashboard` (TUI) | ratatui-based; agents pane + detail pane + auto-refresh 2s + hotkey tmux `t/l/i` (chat / log -f / inbox --watch); `?` เปิด hotkey help overlay กลางจอ; transient `last_action` feedback ใน footer; banner แสดง attention pending count เมื่อมี agent ที่มีข้อความค้าง |
| Daemon-side inbox watch + cursor | ประกาศ envelope ใหม่ไปยัง stderr; `.bwoc/inbox.cursor` รอด restart |
| `--json` ครอบคลุม read-only commands | `list`, `status`, `workspace info`, `workspace validate`, `check` |
| CI matrix | `ubuntu-latest` · `macos-latest` · `windows-latest` เขียวทุก push |
| Release pipeline (CalVer) | `release.yml` trigger เมื่อ push tag `v<YYYY>.<M>.<D>-<patch>`; 4 binary cross-platform + `.sha256` → GitHub Release ที่สร้างอัตโนมัติ |
| Help system (ใน binary) | 12 topic: `getting-started`, `backends`, `workspace`, `manifest`, `arc`, `lifecycle`, `daemon`, `messaging`, `persona`, `memory`, `doctor`, `script` |
| Shell completion | `bwoc completion <bash\|zsh\|fish\|powershell\|elvish>` ผ่าน clap_complete |
| `bwoc init` เขียน `.gitignore` | exclude daemon ephemerals (PID/socket/cursor) สำหรับ user workspace |
| `bwoc new --scope / --out-of-scope / --mindsets / --skills` | persona substitution + mindset/skill stub seeding ตอน incarnate |
| `bwoc new --json` | Emit `{ agent_id, target, registered_in, symlinks, mindset_stubs, skill_stubs, persona_filled }` แทน human report สำหรับ scripted multi-agent setup |
| `bwoc init --json` | Emit `{ workspace, name, version, defaults, files_created }` แทน human creation report ใช้คู่กับ `bwoc new --json` สำหรับ script chain end-to-end: `PATH=$(bwoc init /p --json \| jq -r .workspace) && bwoc new alpha --workspace "$PATH" --json …` entry-point สุดท้ายที่ยังไม่มี `--json` — JSON-everywhere matrix ครอบทุก read+write surface แล้ว (interactive — spawn / chat / dashboard — งดเว้นโดยตั้งใจ) |
| Module `livecheck` ที่ใช้ร่วม | รวม 5 copy ของ `signal_zero_alive` / `running_pid` / `query_uptime` / `format_uptime` / `inbox_count` |
| Stub `bwoc-agent --serve` สำหรับ Windows | build + run default mode ได้; `--serve` exit 2 พร้อมข้อความ "Unix-only" |
| `bwoc workspace info --path-only` | print workspace root ที่ resolved ออกมาบรรทัดเดียว ไม่มีตกแต่ง — สำหรับ shell idiom `cd "$(bwoc workspace info --path-only)"` |
| `bwoc log <agent>` | Tail daemon stderr จาก `<agent>/.bwoc/agent.log`; `-f`/`--follow` สำหรับ live stream; `-n N` สำหรับ N บรรทัดล่าสุด; `--clear` truncate ในที่ |
| Per-workspace memory scaffold | `bwoc init` สร้าง `.bwoc/memory/` พร้อม README อธิบาย 4-tier scope hierarchy (per-agent / per-workspace / per-user / Tier 2) |
| `bwoc memory list \| show \| put \| search \| rm` | CRUD+search ครบสำหรับ `.bwoc/memory/`: `list` (table + `--json` มี `count` / `total_bytes` aggregate inline + `--count` + `--names-only` สำหรับ script iteration + `--sort name\|size\|modified`), `show <name>` หรือ `show --all` (header `# === <name> ===`; `--json` array), `put <name>` (3 source: inline positional > `--file` > stdin; mode: create / `--force` overwrite / `--append`; ทุก write atomic), `search <query>` (substring case-insensitive + `--json`), `rm <name>` (TTY confirm หรือ `--yes`); ทุก subcommand บังคับ flat-name + ห้าม traversal, refuse README.md |
| `bwoc supervise <agent>` | Restart-on-crash supervisor สำหรับ `bwoc-agent --serve`: spawn → wait → respawn เมื่อ exit ไม่ใช่ศูนย์; rate-limit 10/นาที (`--max-restarts-per-min N`); clean exit (status 0) หยุด supervisor stderr → `agent.log` เดียวกับ `bwoc start` `bwoc log -f` ใช้ได้ SIGINT/SIGTERM ผ่าน ctrlc exit clean `--json` emit event แบบ structured ทีละบรรทัด (watch_start / spawn / crash_respawn / clean_exit / rate_limit_hit / signal_stop / spawn_failed) ไป stdout |
| `bwoc check --all` | Fleet-wide neutrality audit: วน workspace registry, run `audit()` ต่อ agent, รวมผลแบบ per-agent section + fleet summary; `--json` emit shape `{ agents[], summary }` ที่ structured Exit 1 ถ้ามี violations |

### ที่เหลือก่อน ship

- **Cross-backend validation** — uppāda + ṭhiti เต็มกับ 5 backend CLI ใน CI (พิสูจน์ Samānattatā); `bwoc-harness` (ollama) คือตัวที่ห้า
- **Code signing** — Apple notarization + Windows Authenticode สำหรับ release artifact (ต้องการ user-cert authorization)
- **Build Linux musl** — `x86_64-unknown-linux-gnu` + `aarch64-unknown-linux-gnu` ship แล้ว; musl (Alpine / distroless) เพิ่มได้เมื่อมีความต้องการ
- **เครื่องมือ memory mining และ interface Tier 2 backend ที่ pluggable**
- **Daemon path สำหรับ Windows ผ่าน named-pipe** — แทน stub cfg-gated ด้วย implementation Windows จริง

---

## Phase 3 — วยะ + Interconnect

**นิยามของเสร็จ:** ชีวิตของ agent จบลงอย่างสะอาด; agent ประสานงานโดยไม่มีศูนย์กลาง

### ส่งมอบใน Phase 3 (เสร็จแล้ว)

| รายการ | หมายเหตุ |
|---|---|
| `bwoc stop <name>` | escalation ladder 3 ขั้น: socket `STOP` → SIGTERM → SIGKILL (รอ ~3s ระหว่างขั้น); idempotent; รายงานว่าขั้นไหนทำให้ daemon จบ `--all` mass-stop agent ที่ไม่ stopped ทั้งหมด (clap บังคับ mutex กับ `name`) `--json` (ต้องคู่กับ `--yes`) emit `{ workspace, agent, daemon_outcome, registry_updated }` สำหรับ scripted lifecycle |
| `bwoc retire <name>` | ลบจาก registry; file mode 3 แบบ: default (ลบ dir), `--keep-files` (เก็บทั้งหมด), `--keep-memory` (เก็บแค่ `memories/`, ลบที่เหลือ — archive ความรู้ที่ agent สั่งสมในขณะที่ปล่อย agent ไป) `--keep-files` กับ `--keep-memory` เป็น clap-mutex |
| `bwoc workspace prune` | ปรับ phantom registry entries vs orphan agent dirs; `--apply` ลบ drift ที่ปลอดภัย; `--json` emit `{ phantoms, orphans, applied, removed }` สำหรับ CI gating |
| User → agent inbox (สัมมาวาจา Phase 0) | `bwoc send` + `bwoc inbox` ship เป็น JSONL envelope; รากฐานสำหรับ agent → agent messaging |
| Kalyāṇamitta 7 trust (5 จาก 5 ขั้น) | implementation ทุกขั้น ship แล้ว 2026-05-23: (1) deserialization ใน `bwoc-core::Manifest`, (2) `bwoc check` ตรวจหลักฐาน, (3) `bwoc trust <agent> read`, (4) refusal ระดับ daemon ที่ inbox poll หลัง `BWOC_TRUST_GATING=1` พร้อม sidecar `inbox.refusals.jsonl` และ `bwoc inbox` merge, (5) CHANGELOG roll-up + TH parity Spec: [`modules/agent-template/interconnect/trust.md`](../../modules/agent-template/interconnect/trust.md) |
| Agent → agent messaging (สัมมาวาจา Phase 1) | `bwoc send --from <agent>` เขียน sender identity ลง envelope; daemon ฝั่งผู้รับประเมินด้วย manifest ของ sender; refusals โผล่ผ่าน `bwoc inbox` JSON merge. กฎ **สาราณียธรรม 6** อยู่ใน [`interconnect/messaging.md`](../../modules/agent-template/interconnect/messaging.md) (+ `.th.md`) |
| `bwoc check` แบบ dual-mode | ตรวจเทมเพลต (placeholder `manifest.name`) vs incarnation (ชื่อจริง) Template mode ยืนยันว่า placeholder ต้องมี + กฎ neutrality; incarnation mode ยืนยันว่า placeholder ต้องหายไป (ยกเว้น `{{taskId}}` ซึ่งเป็น runtime) และข้าม neutrality checks ปิด bug ที่ agent ยังไม่ personalize ผ่าน check แบบเงียบๆ |
| Interconnect routing — Track A | `.bwoc/interconnect/routes.toml` ระดับ workspace, peer-declared (ไม่มี broker กลาง) `bwoc-core::routing` type `Routes` + resolve (exact `agent` → longest `namespace` prefix → `NotFound`); `send` consult เฉพาะตอน local-registry miss, path local-hit ไม่เปลี่ยน byte-for-byte compose กับ trust gate (sender ข้าม workspace → `unknown_sender` → refused) เลย ship ได้โดยไม่ต้องรอ Trust v2 Spec: [`interconnect/routing.md`](../../modules/agent-template/interconnect/routing.md) **อนัตตา / SN 22.59** |
| Worktree lifecycle — Track B | util `git_worktree` แบบ shell-out (ไม่มี `git2`/`gitoxide`) hook `task-claimed` ของ Saṅgha ยิง `git worktree add <worktreeBase>/<agentId>/<taskId> -b agent/<agentId>/feat/<taskId>` ตอน claim; ไม่ขยาย `Task` struct — ตำแหน่ง worktree ตาม path convention `<worktreeBase>/<agentId>/<taskId>` ให้ cleanup deterministic ไม่ต้อง parse log |
| `bwoc retire` full vaya | retire จบ agent อย่างสะอาด: worktree cleanup (worktree ใต้ `<worktreeBase>/<agentId>/` ลบผ่าน git util), branch release (`agent/<agentId>/*` — `-d`, escalate `-D` พร้อมแจ้งชื่อที่ force), interconnect deregister (`Routes::remove_agent_routes` ตัด route ที่ `agent` ชี้ไปยัง retiree ออกจาก `routes.toml`) idempotent; เคารพ file-mode flags; `--json` ขยายแบบ additive ปิดครึ่ง **วยะ** ของ DoD |

### ที่เหลือสำหรับ Phase 3 — เลื่อน (อยู่นอก DoD)

DoD ครบทั้งสองครึ่งแล้ว: *ประสานงานโดยไม่มีศูนย์กลาง* (interconnect routing) และ *ชีวิต agent จบอย่างสะอาด* (`bwoc retire` full vaya) — ship ทั้งคู่ข้างบน เหตุผลการเรียงลำดับ + การตัดสินใจ design ของ worktree-lifecycle / routing อยู่ใน [`notes/2026-05-23_phase3-remaining-sequencing.md`](../../notes/2026-05-23_phase3-remaining-sequencing.md) ที่เหลือเลื่อนออกจาก DoD โดยตั้งใจ:

- **Trust v2** — signed envelopes / identity proof, warn-by-default refusal mode, cross-workspace messaging gate บน telemetry ของ v1; ส่วน cross-workspace สร้างบน routing layer
- **Tier 2 memory** — สองชิ้นแยกกัน: *interface* ของ backend ที่ pluggable (อยู่ใน Phase 2 ที่เหลือด้วย) กับ *reference implementation* Tier 1 file-based memory เสร็จแล้ว

---

## ส่งมอบนอก Phase 3 — v2026.5.24-0 (2.2.0)

รายการต่อไปนี้ ship หลังจาก Phase 3 DoD ประกาศว่าบรรลุแล้ว

| รายการ | หมายเหตุ |
|---|---|
| `bwoc-harness` — self-hosted agentic runtime | OpenAI-compatible model-API client + agentic loop; safety pipeline (guardrails → permission → sandbox); Unix-first v1 (build ได้บน Windows แต่ยังไม่ผ่านการทดสอบ) เพิ่ม **ollama** เป็น backend ตัวที่ห้าที่ประกาศ: `bwoc spawn --backend ollama` เปิด `bwoc-harness` กับ endpoint Ollama / OpenAI-compatible ใด ๆ 8 components ระดับ production Spec: [`docs/th/HARNESS.th.md`](HARNESS.th.md) |

---

## Phase 4 — Reference Agent + Fleet

**นิยามของเสร็จ:** ความเป็นไปได้ของ ecosystem พิสูจน์แล้ว; governance ของ fleet ระดับ production ข้าม vendor ทำได้

### ส่งมอบใน Phase 4 (เสร็จแล้ว)

| รายการ | หมายเหตุ |
|---|---|
| Spec ธรรมาภิบาล fleet | [`docs/th/FLEET-GOVERNANCE.th.md`](FLEET-GOVERNANCE.th.md) (+ `.en.md`) — อปริหานิยธรรม 7 (ทีฆนิกาย 16) map ไปยังการปฏิบัติของ operator ระดับ workspace: ประชุมเนืองนิตย์, เริ่ม-เลิกพร้อมกัน, การเปลี่ยน convention ที่มีกระบวนการ, เคารพ template version, คุ้มครอง agent ที่เปราะบาง, เคารพทรัพยากรร่วม, คุ้มครอง agent อาวุโส ตั้งชื่อ observable signal; เลื่อน automation ไป v2 เมื่อ telemetry สนับสนุนการยกระดับ signal เป็น gate |

### เป้าหมาย (บรรลุโดย adoption ภายนอก)

เป้าหมายเหล่านี้บรรลุโดย maintainer นอกผู้เขียนต้นฉบับใช้ framework — framework เองไปถึงไม่ได้คนเดียว

- Agent อ้างอิงสามตัวหรือมากกว่าในธรรมชาติ สร้างโดยผู้ดูแลนอกทีมผู้เขียนต้นฉบับ (ตาม [`VISION.th.md`](../../VISION.th.md) success ที่ 1 ปี)
- Fleet dashboard — Aparihāniya-dhamma 7 governance ใช้กับการติดตั้ง multi-agent จริง **Spec ลง 2026-05-23** ([`FLEET-GOVERNANCE.th.md`](FLEET-GOVERNANCE.th.md)); การยืนยันกับ fleet จริงรอ
- ศัพท์ BWOC (Yoniso manasikāra checks, Mattaññutā caps, Sīla baselines, Kalyāṇamitta trust scores) ปรากฏใน codebase ที่ไม่มีความสัมพันธ์กับ project นี้ (success ที่ 3 ปี)
- รูปแบบ fleet ระดับ production ข้าม vendor ใช้ในองค์กรมากกว่าหนึ่งแห่ง

---

## ข้ามทุก Phase

- **Bilingual parity** — เอกสารสเปกทุกฉบับมี EN canonical + TH (และภาษาอื่น ๆ ในอนาคต); hook bilingual-reminder gate สิ่งนี้
- **Backend neutrality** — feature CLI ทุกตัวทำงานกับ backend 6 ตัวที่ประกาศ; `/check-neutrality` gate สิ่งนี้สำหรับ `AGENTS.md`
- **Doc-version + software-version คงสอดคล้อง** — ทั้งคู่ stamped อัตโนมัติทุก edit ของ Claude Code
- **Open-source readiness** — artifact ทุกตัวที่ contributor สาธารณะต้องการ (CONTRIBUTING, SECURITY, CoC, LICENSE, VERSION, CHANGELOG, VISION, ROADMAP) up to date และถูกต้อง

---

## สิ่งที่ไม่ใช่เป้าหมาย

ดู [`VISION.th.md` §สิ่งที่ไม่ใช่เป้าหมาย](../../VISION.th.md#สิ่งที่ไม่ใช่เป้าหมาย) สรุป: BWOC ไม่ใช่ศาสนา, ไม่ใช่ runtime/SDK/LLM, ไม่ใช่ตัวแทนของ DDD / Clean Architecture / SOLID, ไม่เอนเอียง vendor, และไม่ใช่กรอบเพิ่มผลผลิต

---

## ดูเพิ่ม

- [`VERSION.md`](../../VERSION.md) — version ปัจจุบันและ SemVer policy
- [`VISION.th.md`](../../VISION.th.md) — success criteria ที่ 1 ปีและ 3 ปี
- [`CHANGELOG.md`](../../CHANGELOG.md) — อะไร ship แล้ว เมื่อไหร่
- [`ARCHITECTURE.th.md`](ARCHITECTURE.th.md) — ส่วนประกอบทำงานร่วมกันอย่างไร
