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
> **t7a (โดยลำพัง) ไม่ได้อ้าง** การกักกัน egress — นั่นคือครึ่ง **t7b / t11** ซึ่ง
> ตอนนี้ **ลงแล้ว (LANDED)** (ดูหัวข้อ *การกักกัน network egress* ด้านล่าง) ตั้งแต่
> t11 เป็นต้นมา network egress ของ executor ถูกกักกันบน Linux แล้ว t7a ยังคงเป็น
> ครึ่ง FS/process ของการพิสูจน์

จงใจ **ไม่** ใช้ถ้อยคำว่า "no shared writable mount" เพราะ shared mount ยังคงมีอยู่
สิ่งที่ t7a ตัดออกไปคือ *การเอื้อมถึง* filesystem ของ executor เข้าไปใน harness
และเส้นทาง `ptrace`/proc ไปยัง RAM ของ parent

## มาตรการควบคุมและสิ่งที่แต่ละข้อปิด

| Control | ภัยที่ปิด | กลไก |
|---|---|---|
| **C1** — FS jail บน executor | executor อ่าน/แก้ไขไฟล์ของ harness (`~/.ssh`, source, checkpoint) | บน Linux ติดตั้ง Landlock domain ใน `pre_exec`: read+write+exec บน `{worktree, per-turn tempdir}`, read+exec บน binary + allowlist ระบบขั้นต่ำ, นอกนั้น (`$HOME`, checkpoint dir, `/proc/<other>`) ถูกปฏิเสธทั้งหมด พร้อมตั้ง `no_new_privs` ส่วน macOS เป็น `sandbox-exec` แบบ **write-confinement + deny egress (t29) + secret read-denylist แบบเลือกจุด** (#329 — residual ที่ *แคบลง* ไม่ใช่ read-jail เต็มรูป ดู Residuals) ส่วน read-jail เต็มรูปยังเป็น Linux-only เลียนแบบ `RLIMIT_AS` ที่เป็น Linux-only ใน t6 มีการ probe และ **LOUD-skip** เมื่อใช้ไม่ได้ |
| **C4** — กัน ptrace ที่ parent | **CRIT-1**: child uid เดียวกันอ่าน API keys ของ parent จาก RAM ผ่าน `ptrace`/`process_vm_readv` | parent เรียก `prctl(PR_SET_DUMPABLE, 0)` (process ที่ non-dumpable มีเพียง root ที่ตรวจสอบได้ child uid เดียวกันจะได้ `EPERM`) **และ** ตรวจสอบ `kernel.yama.ptrace_scope ≥ 1` โดย **fail-closed** ถ้าอ่านได้ `0` |
| **C5** — ไม่ allowlist `/proc` ทั้งก้อน | proc-mem / ข้อมูลรั่วข้าม process | allowlist ของ jail ตัด `/proc` ออกทั้งหมด ให้เฉพาะ entry แบบ path-granular ที่ loader/runtime ต้องใช้จริง (ในทางปฏิบัติไม่มีอันใดอยู่ใต้ `/proc`) |
| **C6** — ขยายการ scrub env | authority ที่ยังมีชีวิตรั่วเข้า child | ตัด `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, `GNUPGHOME`, `DBUS_SESSION_BUS_ADDRESS` ออกจาก env ของ executor (allowlist-deny + pattern-deny) |
| **C7** — git หลัง turn ที่ถูก jail | **CRIT-2**: `build.rs` / git hook / `core.fsmonitor` ที่ถูกวางไว้รันในฐานะ parent ที่ไม่ถูก jail | build/test รัน **ใน child** ภายใต้ C1 jail (`build.rs` ที่ถูกวางไว้จึงรันแบบ *ถูกกักกัน*) ส่วน parent รัน `git` บน worktree ที่ไม่เชื่อถือภายใน jail เดียวกัน พร้อม `core.hooksPath=/dev/null` และ override config (`core.fsmonitor`, `core.pager`, `core.sshCommand`, `diff.external`, global/system config → `/dev/null`) |
| **C8** — binary RO; checkpoint อยู่นอก jail | **M3** (เขียนทับ `current_exe`), **M2** (ปลอม trust latch) | binary เป็น read+exec เท่านั้น (เขียนไม่ได้) ส่วน checkpoint ของ SessionTrust อยู่นอก rw set ของ jail child จึงเขียนไม่ได้ — latch เขียนโดย parent เท่านั้น |
| **C9** — อ่าน fd3 แบบมีขอบเขต | การใช้ IPC ในทางที่ผิด / ลักลอบส่ง descriptor | parent อ่านหนึ่งเฟรมด้วย timeout จำกัด, จำกัดความยาวสูงสุด, ปิด socket หลังหนึ่งเฟรม และ **ไม่มี `cmsg` buffer** (child จึงส่ง descriptor กลับผ่าน `SCM_RIGHTS` ไม่ได้) |

## การกักกัน network egress (t11 / t7b — บังคับใช้แล้ว ENFORCED, Linux)

t11 (= t7b) คือครึ่ง egress ของ t7 เดิม และตอนนี้ **ลงมือทำแล้ว** (เป็น control ที่
บังคับใช้ ไม่ใช่ residual อีกต่อไป) ทฤษฎีบทการปิด (closure theorem) ที่การพิสูจน์
ตั้งอยู่บน (yudi รับมาจากผล red-team ของ nezha):

> **egress ถูกกักกัน ⟺ (A) child *หา* network fd มาไม่ได้ ∧ (B) มัน *ไม่ถือ* ไว้เลย
> ∧ (D) arch-guard แน่นหนา**

**no-fd invariant (A ∧ B)** คือ control หลัก ส่วน deny ฝั่งส่ง (send) เป็น
belt-and-suspenders สำหรับ descriptor ที่ไม่ควรมีอยู่ตั้งแต่แรก แต่ละขาพิสูจน์ด้วย
arm ของ red-team (`tests/sandbox_escape.rs`, `--features test-redteam`) ไม่ใช่สมมติเอา

| Control | ภัยที่ปิด | กลไก |
|---|---|---|
| **t11-A** — หา network fd ไม่ได้ | executor เปิด socket / ขโมยมา (DNS, TCP, UDP, abstract-namespace AF_UNIX, `pidfd_getfd` ขโมย fd, io_uring) | seccomp-bpf `SECCOMP_RET_KILL_PROCESS` deny set (socket/connect/accept*/bind/listen/getpeername/sendmmsg/recvmmsg/io_uring_*/pidfd_getfd/pidfd_open) คอมไพล์ด้วย **seccompiler** (pure-Rust) ใน parent ติดตั้งผ่าน `prctl` ใน `pre_exec` ของ child ใช้ KILL (ไม่ใช่ ERRNO) เพื่อไม่ให้ child ที่เป็นศัตรูได้ oracle สำหรับไล่หมายเลข syscall ตระกูล send/recv แบบ **unary** และ `socketpair` แบบ *local* ยังอนุญาต (IPC AF_UNIX ของ executor + tokio ต้องใช้) deny เฉพาะตัวแปร batched `*mmsg` เท่านั้น |
| **t11-B** — ไม่ถือ network fd | fd socket ที่รั่ว/สืบทอดมา (รวมถึงที่ **>= 1024**) คือเป้าหมาย egress ที่พร้อมใช้ | `close_range(EXECUTOR_FD+1, ~0)` ปิด descriptor table ทั้งก้อน (ลูป `4..1024` เดิมพลาดตัวที่ >= 1024) บวกการตรวจ stdio (0/1/2) ที่ชี้ stdio ซึ่งเป็น socket ไปที่ `/dev/null` |
| **t11-D** — arch-guard แน่น | syscall ผ่าน i386 `int 0x80` หรือ x32-renumber (`nr | __X32_SYSCALL_BIT`) เลี่ยง deny set ของ x86_64 | arch prologue ของ seccompiler KILL ทุก `seccomp_data.arch` ที่ไม่ใช่ native และ deny set ยังบรรจุตัวแปร x32-renumber ด้วย พิสูจน์ทั้งคู่ด้วย arm ของ red-team (การ KILL ของ prologue **พิสูจน์ด้วยเทสต์** ไม่ใช่สมมติเอา) |
| **t11 / C4** — กัน ptrace เชิงลึก | child uid เดียวกันอ่าน RAM ของ parent ผ่าน ptrace/process_vm_readv | ptrace set (ptrace/process_vm_readv/process_vm_writev/kcmp/perf_event_open) อยู่ใน KILL deny set ซ้อนบน `PR_SET_DUMPABLE(0)` ของ parent |

**Fail-closed (Linux):** การกักกัน egress เป็นข้อบังคับ ถ้าติดตั้ง filter ไม่ได้
turn-executor ฝั่ง production จะ **ปฏิเสธที่จะรัน** (ไม่ลดระดับไปเป็น executor ที่ไม่มี
filter เด็ดขาด) ตัวติดตั้ง seccomp ตั้ง `PR_SET_NO_NEW_PRIVS` แบบ idempotent (ไม่พึ่ง
Landlock) seccomp คงอยู่ข้าม `execve` และสืบทอดไปยัง grandchild ของ `run_command`
ด้วย มันจึงถูกกักกัน egress เช่นกัน

**ขอบเขต (ตามจริง)** นี่คือ **การกักกัน network egress (Linux)** ไม่ได้อ้างถึง
ช่องทางลับ local แบบ uid เดียวกัน — OS keyring, SysV-IPC, POSIX message queue,
ไฟล์ที่แชร์ใต้ worktree หรือ signal: sibling uid เดียวกันเข้าถึงสิ่งเหล่านั้นได้เสมอ
seccomp เป็นเครื่องมือผิดประเภทสำหรับมัน จึง **อยู่นอกขอบเขต** ของ t11 (ติดตามแบบ
ฉวยโอกาสในชื่อ NEWNET)

## ส่วนที่ยังเหลือ — สิ่งที่ยัง **ไม่** ถูกกักกัน

- **การแยก mount-namespace** — ไม่ได้อ้าง; mount ของ worktree ยังแชร์กันอยู่
- **ช่องทางลับ local แบบ uid เดียวกัน** — อยู่นอกขอบเขต (ดู *ขอบเขต* ด้านบน)
- **เพดาน process ต่อ turn** — t9 ลงแล้ว: มีเพดานสัมบูรณ์ต่อ turn แบบ cgroup v2
  `pids.max` บังคับใช้ **เมื่อมี cgroup v2 subtree ที่ writable ถูก delegate** (systemd
  `Delegate=yes` / privileged container) — ✅ แต่ในกรณี **default** (เครื่อง dev,
  bare-SSH login, container ที่ไม่ได้ delegate) **ไม่มี** subtree ถูก delegate ตัวกัน
  fork จึงลดเหลือ floor แบบ **per-UID, RELATIVE** `RLIMIT_NPROC` best-effort — 🟠
  residual จึงคือกรณี default ที่ไม่ได้ delegate: fork-bomb เติมตาราง process ระดับ
  per-UID จนเต็ม (เป็น DoS ต่อ availability ของ harness **ไม่ใช่** การหนีออกจาก sandbox)
  ตั้ง `BWOC_REQUIRE_CGROUP_PIDS=1` ให้ harness ปฏิเสธการเริ่มถ้าไม่มี subtree ที่
  delegate ไว้ สำหรับ prod ที่ต้องการเพดานแบบ hard (ส่วน prereq ฝั่ง deployment —
  unit drop-in `Delegate=yes` — คือ **t14**)
- **การ confine การอ่านบน macOS (แคบลงแล้ว, #329)** — macOS เป็นแพลตฟอร์ม **dev-only**
  สำหรับ turn-executor การ confine การเขียน (SBPL `(deny file-write*)`) และการ deny
  network egress (SBPL `(deny network*)`, t29) บังคับใช้แล้ว ส่วนด้าน **การอ่าน** เป็น
  residual ที่ *แคบลง* **ไม่ใช่** parity กับ Landlock โดยวาง arm `(deny file-read* …)`
  แบบเลือกจุด ทับ **denylist** ของ path ลับมูลค่าสูงที่รู้จัก บน macOS read surface
  **ทั้งสอง** ผ่าน renderer ตัวเดียวกัน: turn-executor jail
  (`jail.rs::macos_write_confine_profile`) และ tool sandbox
  (`sandbox.rs::build_sbpl_profile`) เซ็ต: `~/.ssh`, `~/.aws`, `~/.config/gcloud`,
  `~/.config/gh`, และ BWOC home ที่เก็บ agent keys + checkpoint ของ SessionTrust
  เปิดโดยดีฟอลต์และ fail-closed
  (`BWOC_SANDBOX_ALLOW_SECRET_READ=1` เป็น seam เดียวสำหรับ opt-out) จงใจเลี่ยง arm
  แบบ deny-default ทั้งหมด — เพราะมันทำให้การอ่าน dyld shared-cache ที่ `sandbox-exec`
  ต้องใช้เพื่อ launch binary แบบ dynamically-linked พัง **Residual:** path ลับที่
  *ไม่อยู่ในลิสต์* ยังอ่านได้ และ arm อ่าน / ptrace ของ red-team ยังเป็น Linux-only
  (จะ LOUD-skip บน macOS) ส่วนการ deny egress (t29) ชดเชยเส้นทาง exfil ตรงไว้แล้ว

## รั้วกั้นมาตรการที่เลื่อนออกไป (deferred-control fence — t8)

Phase 5 t8 สร้าง *รั้ว* เพื่อให้การขาดหายของมาตรการที่เลื่อนออกไปลืมไม่ได้และปลอมไม่ได้
คือ single source of truth ที่ `scripts/deferred-controls.txt` ระบุชื่อมาตรการที่ขาดด้วย
สะกดจริงระดับ kernel/library มี CI guard ที่ `scripts/check-deferred-fence.sh` คอยตรึง
SSOT, fence region ด้านล่าง และ source จริงให้ตรงกัน และมี phantom-control check ที่ทำให้
build ล้มถ้ามีโค้ด *อ้างถึง* มาตรการเหล่านี้ (แม้ใน string literal) โดยไม่มีคำกำกับ
`// DEFERRED(tNN):` ที่ยอมรับอย่างตรงไปตรงมาว่ามันยังไม่มีอยู่จริง

มาตรการทั้งสองที่รั้วนี้กั้นไว้ได้ **ลงมือทำเสร็จแล้ว**: t11 ปิดขอบเขต **egress** และ t9
เพิ่ม **เพดาน process ต่อ turn** (cgroup v2 `pids.max` แบบ best-effort — ดู *ส่วนที่ยัง
เหลือ* ด้านบน) ดังนั้น SSOT จึง **ไม่มี token ที่เลื่อนออกไปเหลืออยู่** และรั้วนี้ **ถูกปลด
ครบแล้ว (fully discharged)** guard ยังรันทุกครั้งใน CI เพื่อรักษาสถานะปลายทางนี้ให้ตรงตาม
จริง: มันจะ fail อีกถ้า residual ของมาตรการที่ลงแล้วถูกตัดออกจาก THREAT-MODEL (condition C,
EN + TH) หรือถ้ามีการเลื่อนมาตรการใหม่ที่ทำให้เกิด phantom ขึ้นมาอีก fence region ด้านล่าง
จึงว่างเปล่าจาก token

<!-- DEFERRED-FENCE:BEGIN — machine-checked against scripts/deferred-controls.txt; this region must hold NO deferred tokens (backticks) and NO ticket ids while the fence is discharged. -->

มาตรการที่เลื่อนออกไปของ Phase 5 ลงครบทั้งหมดแล้ว (เพดาน process ต่อ turn และ
egress/syscall filter) รั้วนี้จึง **ถูกปลดครบแล้ว (fully discharged)** residual ตามจริงที่
ยังเหลือจากแต่ละมาตรการที่ลงแล้ว — โดยเฉพาะ fallback ของตัวกัน fork กรณีไม่ได้ delegate —
ถูกบันทึกไว้ในส่วน *ส่วนที่ยังเหลือ* ด้านบน ไม่ใช่ตรงนี้

<!-- DEFERRED-FENCE:END -->

### ขอบเขตการอนุญาต (คำ sign-off ที่ผูกพัน) — ปรับปรุงที่ t11

t8 ปิด gate ด้าน **ความซื่อตรง (honesty)** จากนั้น t11 ปิดตัว control ด้าน egress ที่
t8 กั้นรั้วไว้ คำ sign-off เดิมจึงถูกยกเลิกสำหรับตัวบล็อกเกอร์เดิม:

> **Phase 5 ลงนามครบถ้วนแล้ว (t11 merge แล้ว)** บน **Linux** network egress ของ
> turn-executor ถูกกักกันแล้ว (seccomp + no-fd invariant + arch-guard แน่น แบบ
> fail-closed) ข้อจำกัดเดิม — *"ship ได้เฉพาะ context ที่ยอมรับ egress ได้ / แยก
> network จนกว่า t11 จะลง"* — **ถูกยกเลิกบน Linux** เหลือคำเตือนตามจริงสองข้อ:
> (1) ตัวกัน fork ต่อ turn เป็นเพดานสัมบูรณ์ cgroup v2 `pids.max` **เฉพาะเมื่อมี subtree
> ที่ writable ถูก delegate** (systemd `Delegate=yes` / privileged container ตรวจที่ startup
> ด้วย `BWOC_REQUIRE_CGROUP_PIDS=1`) ในกรณี **default** ที่ไม่ได้ delegate จะลดเหลือ floor
> แบบ **per-UID** `RLIMIT_NPROC` best-effort — เป็น DoS ต่อ availability ของ harness ไม่ใช่
> การหนี (prereq ฝั่ง deployment คือ **t14**); (2) บน **macOS** ทั้ง Landlock และ seccomp
> ใช้ไม่ได้ (write-confinement เท่านั้น)
> macOS จึงยังเป็นแพลตฟอร์มสำหรับ dev เท่านั้น ส่วนช่องทางลับ local แบบ uid เดียวกัน
> อยู่นอกขอบเขตโดยการออกแบบ (NEWNET)

## การพิสูจน์

หลักฐานของ gate คือชุดทดสอบ red-team เชิงปฏิปักษ์
(`crates/bwoc-harness/tests/sandbox_escape.rs` สร้างภายใต้
`--features test-redteam`): child ที่เป็นศัตรูจะถูก spawn **ภายใน jail ของ executor
จริง** และทุกการพยายามหนีออกไปยัง canary ที่วางไว้ต้องล้มเหลว — fail-closed ไม่ใช่
ผ่านแบบเงียบ ๆ ดูแบบจำลองการพิสูจน์ทีละ control ได้ใน charter
(`phase5-samvara-charter.md`)
