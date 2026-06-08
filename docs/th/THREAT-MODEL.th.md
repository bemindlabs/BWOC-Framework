---
title: แบบจำลองภัยคุกคาม
parent: ภาษาไทย
nav_order: 15
---

# แบบจำลองภัยคุกคาม — ขอบเขตความเชื่อถือของ turn-executor (Phase 5)

เอกสารนี้บันทึกขอบเขตความเชื่อถือ (trust boundary) ของ harness แบบ self-hosted
ของ BWOC (`bwoc-harness`) โดยเฉพาะการแยกตัว (isolation) ของ **turn-executor**
ที่สร้างขึ้นใน Phase 5 เป็นเอกสารคู่ขนานระดับ framework กับ `THREAT-MODEL.md`
ของ agent template (แบบจำลอง Taṇhā-3 ระดับราย agent) และกับสัญญาการพิสูจน์ใน
`phase5-samvara-charter.md`

## ขอบเขต

หลังจาก safety pipeline อนุมัติการเรียก tool แล้ว harness จะ **ไม่** รัน tool นั้น
ใน process ของ agent โดยตรง แต่ parent จะ re-exec binary ของตัวเองเป็น child ที่
ซ่อนชื่อไว้ว่า `--__turn-executor` ส่ง request หนึ่งเฟรมผ่าน socket ที่สืบทอดมา
แล้ว child จะรัน tool นั้นเพียงตัวเดียวก่อนจะตายไป (Phase 5 t5) โดย parent ยังถือ
LLM loop, provider API keys และ latch `SessionTrust` แบบ monotonic เอาไว้ ส่วน
child ไม่ได้ถืออะไรเลย

- **เชื่อถือได้ (Trusted):** process ของ parent (เก็บ keys, trust latch, เป็นผู้เขียน checkpoint)
- **ไม่เชื่อถือหลัง child turn แรก:** เนื้อหาใน worktree (child turn อาจเขียน
  `build.rs`, git hook หรือ `.git/config` ที่เป็นอันตรายไว้)
- **ถือว่าเป็นศัตรูตั้งแต่ต้น:** ตัว turn-executor child เอง — t7a ปฏิบัติต่อมัน
  เสมือนถูกผู้โจมตีควบคุมเต็มที่ แล้วถามว่ามันยังเอื้อมไปถึงอะไรได้บ้าง

## สิ่งที่ t7a อ้าง (ข้อความ gate ตามจริง)

> **t7a อ้างว่า:** turn-executor ไม่สามารถอ่านหรือแก้ไข harness ผ่าน filesystem,
> ผ่าน `ptrace`/proc-mem, หรือผ่าน build artifact ที่ถูกวางไว้ได้
>
> **t7a ไม่ได้อ้าง** การแยกตัวด้วย mount-namespace
>
> **t7a ไม่ได้อ้าง** การกักกัน egress — network, ssh-agent และ abstract-socket
> ยังเข้าถึงได้อยู่ และเป็นเรื่องของ **t7b / ticket t11**

จงใจ **ไม่** ใช้ถ้อยคำว่า "no shared writable mount" เพราะ shared mount ยังคงมีอยู่
สิ่งที่ t7a ตัดออกไปคือ *การเอื้อมถึง* filesystem ของ executor เข้าไปใน harness
และเส้นทาง `ptrace`/proc ไปยัง RAM ของ parent

## มาตรการควบคุมและสิ่งที่แต่ละข้อปิด

| Control | ภัยที่ปิด | กลไก |
|---|---|---|
| **C1** — FS jail บน executor | executor อ่าน/แก้ไขไฟล์ของ harness (`~/.ssh`, source, checkpoint) | บน Linux ติดตั้ง Landlock domain ใน `pre_exec`: read+write+exec บน `{worktree, per-turn tempdir}`, read+exec บน binary + allowlist ระบบขั้นต่ำ, นอกนั้น (`$HOME`, checkpoint dir, `/proc/<other>`) ถูกปฏิเสธทั้งหมด พร้อมตั้ง `no_new_privs` ส่วน macOS เป็น `sandbox-exec` แบบ **write-confinement เท่านั้น** (ไม่ jail การอ่าน — เป็น Linux-only เลียนแบบ `RLIMIT_AS` ที่เป็น Linux-only ใน t6) มีการ probe และ **LOUD-skip** เมื่อใช้ไม่ได้ |
| **C4** — กัน ptrace ที่ parent | **CRIT-1**: child uid เดียวกันอ่าน API keys ของ parent จาก RAM ผ่าน `ptrace`/`process_vm_readv` | parent เรียก `prctl(PR_SET_DUMPABLE, 0)` (process ที่ non-dumpable มีเพียง root ที่ตรวจสอบได้ child uid เดียวกันจะได้ `EPERM`) **และ** ตรวจสอบ `kernel.yama.ptrace_scope ≥ 1` โดย **fail-closed** ถ้าอ่านได้ `0` |
| **C5** — ไม่ allowlist `/proc` ทั้งก้อน | proc-mem / ข้อมูลรั่วข้าม process | allowlist ของ jail ตัด `/proc` ออกทั้งหมด ให้เฉพาะ entry แบบ path-granular ที่ loader/runtime ต้องใช้จริง (ในทางปฏิบัติไม่มีอันใดอยู่ใต้ `/proc`) |
| **C6** — ขยายการ scrub env | authority ที่ยังมีชีวิตรั่วเข้า child | ตัด `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, `GNUPGHOME`, `DBUS_SESSION_BUS_ADDRESS` ออกจาก env ของ executor (allowlist-deny + pattern-deny) |
| **C7** — git หลัง turn ที่ถูก jail | **CRIT-2**: `build.rs` / git hook / `core.fsmonitor` ที่ถูกวางไว้รันในฐานะ parent ที่ไม่ถูก jail | build/test รัน **ใน child** ภายใต้ C1 jail (`build.rs` ที่ถูกวางไว้จึงรันแบบ *ถูกกักกัน*) ส่วน parent รัน `git` บน worktree ที่ไม่เชื่อถือภายใน jail เดียวกัน พร้อม `core.hooksPath=/dev/null` และ override config (`core.fsmonitor`, `core.pager`, `core.sshCommand`, `diff.external`, global/system config → `/dev/null`) |
| **C8** — binary RO; checkpoint อยู่นอก jail | **M3** (เขียนทับ `current_exe`), **M2** (ปลอม trust latch) | binary เป็น read+exec เท่านั้น (เขียนไม่ได้) ส่วน checkpoint ของ SessionTrust อยู่นอก rw set ของ jail child จึงเขียนไม่ได้ — latch เขียนโดย parent เท่านั้น |
| **C9** — อ่าน fd3 แบบมีขอบเขต | การใช้ IPC ในทางที่ผิด / ลักลอบส่ง descriptor | parent อ่านหนึ่งเฟรมด้วย timeout จำกัด, จำกัดความยาวสูงสุด, ปิด socket หลังหนึ่งเฟรม และ **ไม่มี `cmsg` buffer** (child จึงส่ง descriptor กลับผ่าน `SCM_RIGHTS` ไม่ได้) |

## ส่วนที่ยังเหลือ — สิ่งที่ยัง **ไม่** ถูกกักกัน (t7b / t11)

t7a เป็นครึ่ง **process/FS** ของ t7 เดิม ส่วนครึ่ง egress ถูกเลื่อนไปเป็น
**t7b (ticket t11)** และยังเปิดอยู่อย่างชัดเจน:

- **Network egress** — executor ยังเปิด socket ได้ (DNS, TCP, UDP)
- **ssh-agent / abstract sockets** — `SSH_AUTH_SOCK` ถูก scrub ออกจาก env (C6)
  แล้ว แต่ abstract-namespace socket ที่เข้าถึงได้โดยไม่ต้องมี path นั้น FS jail
  กักไม่ได้ ต้องใช้ seccomp/netns (t7b)
- **การแยก mount-namespace** — ไม่ได้อ้าง; mount ของ worktree ยังแชร์กันอยู่
- **การ confine การอ่านบน macOS** — เป็น Linux-only; บน macOS jail ของ executor
  เป็น write-confinement เท่านั้น (arm อ่าน/ptrace/proc ของ red-team จะ LOUD-skip)

## การพิสูจน์

หลักฐานของ gate คือชุดทดสอบ red-team เชิงปฏิปักษ์
(`crates/bwoc-harness/tests/sandbox_escape.rs` สร้างภายใต้
`--features test-redteam`): child ที่เป็นศัตรูจะถูก spawn **ภายใน jail ของ executor
จริง** และทุกการพยายามหนีออกไปยัง canary ที่วางไว้ต้องล้มเหลว — fail-closed ไม่ใช่
ผ่านแบบเงียบ ๆ ดูแบบจำลองการพิสูจน์ทีละ control ได้ใน charter
(`phase5-samvara-charter.md`)
