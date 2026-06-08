---
title: Phase 5 Saṃvara Charter — Pure-Read Egress Boundary (t4)
nav_exclude: true
---

# Phase 5 Saṃvara Charter — t4: the pure-read egress boundary

This charter records the **proof contract** for the `PURE_READ_TOOLS` whitelist:
the binding claim each entry makes, the scope of that claim, how it is proven,
and the residuals the proof deliberately does **not** cover. It is the prose
companion to `crates/bwoc-harness/src/policy/mod.rs` (the whitelist + classifier)
and `crates/bwoc-harness/tests/egress_pure_read.rs` (the proof).

## The claim

A tool listed in `PURE_READ_TOOLS` is **read-only and egress-clean**: invoking it
on any input produces no externally-observable side effect. The whitelist is the
**single source of truth** — the classifier mints `Capability::PureRead` only by
membership in it, and the proof harness enumerates the *same* slice. The three
consumers cannot drift apart.

## Scope of "egress-clean" (BC-2)

"Egress-clean" means **no externally-observable effect**, enumerated as:

| In scope (must not happen) | Definition |
|---|---|
| Network | any socket connect / send — TCP, UDP, raw |
| DNS | any name resolution (a special case of network) |
| Filesystem write outside the worktree | create / write / truncate / rename / delete / chmod of any path not under the agent's worktree root |
| Process spawn | `fork`/`exec` of any child process |
| IPC | pipes, sockets, shared memory, signals to other processes |

**Explicitly out of scope — known-accepted residual:**

- **Wall-clock timing.** A pure-read tool consumes CPU and wall-clock time, and
  its runtime is weakly input-dependent (a large file reads slower). This is an
  **intra-turn** signal only: it never crosses a process or network boundary, so
  it is *not* egress. A timing side-channel is accepted, not proven absent.
- **Reads themselves.** Reading any path the process can already read is the
  whole point of a pure-read tool; reads are unrestricted by design.

## Proof model

Two strengths, both wired into the normal `cargo test` suite (BC-1a) so they run
in the required ubuntu build+test CI. Promoting either to a *required* status
check in branch protection is operator-side configuration — flagged here, not
attempted from the repo.

### Behavioral — PRIMARY (Linux)

Each whitelisted tool is run for real under a Landlock domain that denies **all**
filesystem writes and **all** TCP connect/bind, while leaving reads unrestricted.
Reaching completion with the expected read output proves the tool performed no
write and no egress on that input.

- **Fail-closed (BC-1b).** If the sandbox cannot be fully enforced the test
  **fails** — it never skip-passes. Before any tool runs, the domain is
  *empirically self-tested*: a file-create and a TCP connect must both be refused
  with `EACCES`. A no-op or partial sandbox therefore cannot yield a false green,
  because the self-test would not trip.
- **No opt-out (BC-3a).** The harness enumerates `PURE_READ_TOOLS` directly and
  has no skip-list. A tool with no behavioral input fixture makes the suite
  **panic** — a tool that cannot be exercised cannot be PureRead.
- **Platform.** Gated behind `cfg(target_os = "linux")`; the behavioral run is
  the Linux primary. macOS and other platforms run the static floor only.

### Static — UNIVERSAL floor

On every platform, each whitelisted tool's source (its `impl ToolImpl` block plus
the same-file helpers it calls, transitively) is scanned for forbidden effect
symbols: `reqwest`, `std::net`, `tokio::net`, `TcpStream`, `Command::new`,
`tokio::process`, `fs::write`, `create_dir`, `OpenOptions`, `fs::remove`,
`append`. This is a **fast floor**, not a full proof.

## Single construction site (BC-3b)

`Capability::PureRead` is constructed in exactly one place — the membership check
`if PURE_READ_TOOLS.contains(&tool_name)` in `classify_capability`. No second
match arm may mint the tier. A source-level test in `egress_pure_read.rs` asserts
this (one construction site across the module's non-test code), and a behavioral
golden test asserts non-whitelisted tools never classify PureRead.

## Behavior-preserving extraction (BC-5)

Extracting `PURE_READ_TOOLS` from the old inline `match` arm into a named const +
membership check is a pure refactor. A golden-table unit test pins the
classification of every known tool (the 4 whitelist strings → PureRead; writes →
WorktreeWrite; everything else → Gated) and fails if any verdict drifts.

## Residuals — what is NOT fully proven (BC-4)

This proof is strong but bounded. We **do not** claim "fully proven":

- **Transitive-dependency taint is the behavioral test's job, not the static
  floor's.** The static scan only reaches a tool's own `impl` block and the
  same-file helpers it calls. A forbidden effect reached through a *crate
  dependency* or a *cross-file helper* (e.g. `memory_read`'s use of
  `normalize_path_pub`, which is pure path arithmetic) is invisible to the static
  floor. The behavioral test catches it: a real runtime socket or write under the
  deny-all sandbox trips `EACCES` regardless of how deep the call originates.
- **Residual egress is input-conditional.** A tool could be clean on one input
  and effectful on another, so the behavioral fixtures exercise *representative*
  inputs per tool — grep match **and** no-match, file/dir **present and absent**,
  and paths **inside and outside** confinement. This samples the input space; it
  does not exhaust it.
- **Timing** is an accepted residual (see Scope above).

The honest summary: the behavioral test proves the listed tools make no
observable write/egress on the exercised inputs, on Linux, under a sandbox proven
armed; the static floor is a fast cross-platform sanity check. Extending coverage
(more inputs, more platforms, fuzzed args) strengthens — but does not gate — the
claim.

---

## Phase 5 t7a — turn-executor process / FS confinement

t7a hardens the **process** that runs an approved tool (the re-exec'd
`--__turn-executor` child), not just individual tool calls. It is the
process/FS-confinement half of the original t7; the egress/seccomp/netns half is
**t7b (ticket t11)**, a deferred hard-blocker (yudi's SPLIT ruling).

### The t7a claim (honest gate text)

> **t7a claims:** the turn-executor cannot read or mutate the harness via the
> filesystem, via `ptrace`/proc-mem, or via a planted build artifact.
>
> **t7a does NOT claim** mount-namespace isolation.
>
> **t7a does NOT claim** egress containment — network, ssh-agent, and
> abstract-socket reachability remain open and are **t7b / t11**.

(Deliberately *not* phrased as "no shared writable mount": there IS a shared
mount; what t7a removes is the executor's filesystem *reach* into it and the
ptrace/proc path to the parent's RAM.)

### How each control delivers the claim

| Control | Closes | Mechanism |
|---|---|---|
| **C1** FS jail on the executor | FS read/mutate of the harness | Linux **Landlock** in `pre_exec` (rw = {worktree, per-turn tempdir}; read+exec = binary + minimal system allowlist; `$HOME`/checkpoint/`/proc/<other>` denied; `no_new_privs`). macOS = sandbox-exec **write-confinement only** (reads NOT jailed — Linux-only, mirrors t6's RLIMIT_AS). Probe + **LOUD-skip** when unavailable. |
| **C4** parent anti-ptrace | **CRIT-1** (ptrace RAM-read of keys) | Parent `prctl(PR_SET_DUMPABLE,0)` (blocks same-uid `ptrace`/`process_vm_readv`) **+** verify `kernel.yama.ptrace_scope ≥ 1`, **fail-closed** on `0`. |
| **C5** no wholesale `/proc` | proc-mem / info leak | Jail allowlist excludes `/proc` entirely; only the loader/runtime paths it actually needs are granted (none under `/proc` in practice). |
| **C6** env-scrub extension | authority leak | Drop `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, `GNUPGHOME`, `DBUS_SESSION_BUS_ADDRESS` from the executor env (allowlist-deny + pattern-deny). |
| **C7** jailed post-turn git | **CRIT-2** (build.rs/worktree RCE) | The parent runs build/test **in-child** under the C1 jail (so a planted `build.rs` executes *confined*), and runs `git` on the untrusted worktree inside the same jail **with `core.hooksPath=/dev/null`** + config overrides (fsmonitor/pager/sshCommand/diff.external) so a planted hook/config cannot run as the unjailed parent. |
| **C8** binary RO, checkpoint outside jail | **M3**, **M2** | The binary is read+exec-only (no `current_exe` overwrite); the SessionTrust checkpoint lives outside the jail rw set (child cannot forge the latch — parent-written only). |
| **C9** bounded fd3 read | IPC abuse | Parent reads one frame with a finite timeout, max-length cap, close-after-one-frame, and **no cmsg buffer** (drops `SCM_RIGHTS`). |

### Proof model (fail-closed, never silent-pass)

The gate proof is the adversarial **red-team** suite
(`crates/bwoc-harness/tests/sandbox_escape.rs`, `--features test-redteam`): a
hostile child (`src/bin/sandbox_redteam.rs`) is spawned **inside the real
executor jail** and attempts each escape against planted canaries — read a
`~/.ssh` secret, write outside the worktree, overwrite the checkpoint canary
(M2), overwrite its own binary (M3), `process_vm_readv` the parent (CRIT-1 / C4),
read the parent's `/proc/<pid>/environ` (C4). Every attempt must **fail**. A
planted `build.rs` is exactly worktree-resident code running under the same jail,
so the read/write arms ARE its confinement proof (CRIT-2); a separate C7 test
plants a malicious `core.fsmonitor` and asserts the production
`DiffSummary::from_worktree` (hardened + jailed git) does not execute it.

**Platform honesty (mirrors t6's Linux-only memory cap):** the read / ptrace /
`/proc` guarantees are the **Linux** Landlock + C4 controls. On macOS the jail is
write-confinement only, so those arms **LOUD-skip** (never silent-pass); the
write-outside and checkpoint arms still bite. If Landlock is unavailable on a
Linux kernel, the whole proof LOUD-skips rather than green.

---

## Phase 5 t8 — the deferred-control fence (capstone closure)

t8 is the Phase 5 capstone. It adds **no new containment** — by design
(Mattaññutā: t8 is honesty, not coverage). t1–t7a hardened the turn-executor to
the process/FS boundary; two whole classes of containment remain **known-open**.
t8's job is to make that openness *impossible to forget or to fake*, and to
state in writing where t1–t7a may therefore be shipped.

### The t8 claim (honest gate text)

> **t8 claims:** every control Phase 5 deferred is enumerated by its real
> kernel/library spelling in a single source of truth, is reflected truthfully in
> the THREAT-MODEL fence table, and is fenced in CI so that (a) the docs and the
> SSOT cannot drift apart in either direction and (b) no code can quietly
> *reference* a deferred control as if it existed.
>
> **t8 does NOT claim** any new runtime containment. The egress gap (t11) and the
> per-turn process-cap gap (t9) are exactly as open after t8 as before it — t8
> only guarantees they are named, scoped, and un-foolable.

### What the fence is made of

| Artifact | Role |
|---|---|
| `scripts/deferred-controls.txt` | **SSOT.** Enumerates each deferred control by its REAL spelling (`seccomp`, `PR_SET_SECCOMP`, `SECCOMP_SET_MODE_FILTER`, `libseccomp` → t11; `cgroup`, `/sys/fs/cgroup`, `cgroup.procs`, `pids.max` → t9), each bound to its ticket. Not nicknames. |
| `scripts/check-deferred-fence.sh` | The CI guard. Parses the SSOT, the EN + TH fence tables, and the live source; fails on drift or a phantom control. |
| THREAT-MODEL fence table (EN + TH) | The human-facing truth: ticket, deferred control, real spellings, honest residual, severity. |
| `.github/workflows/ci.yml` → `fence-guard` job | Runs the guard on every push / PR. |

### How the five binding conditions land

- **A — real spellings, not nicknames.** The SSOT lists the exact kernel/library
  tokens above; the phantom-control guard greps **those** in live (comment- and
  test-stripped) `.rs`.
- **B — annotate, don't instant-fail.** A deferred token that appears in live
  code — *including inside a string literal* such as an honest
  `"seccomp unavailable"` error — is **must-annotate**: tag the line with
  `// DEFERRED(tNN):` (matching ticket) and it passes. This is what stops the
  guard from false-positiving against the project's own honest error strings.
- **C — the honest t9 truth.** The t9 fence row states it plainly: the only fork
  guard today is `RLIMIT_NPROC`, which is **per-UID and RELATIVE**. We
  **confirmed** the turn-executor child does **not** run under a dedicated
  separate UID — it re-execs the harness binary under the harness's own UID
  (`crates/bwoc-harness/src/turn_executor.rs`; no `setuid`/`seteuid` on the spawn
  path, only `getuid` for the per-UID proc count). So a fork-bomb in that child
  fills the **per-UID** process table and can DoS the harness itself — an
  **availability** failure, not an escape. Severity stays **🟠**. The guard also
  enforces the *best-effort rule*: `RLIMIT_NPROC` keeps its RELATIVE/best-effort
  marker and is never treated as a hard guarantee (`.expect`/`assert!`) in live
  code (`setrlimit` failure is handled conditionally, not panicked on).
- **D — the binding sign-off scopes shipping permission.** See the sign-off
  below; it is also written into the THREAT-MODEL fence section.
- **E — bidirectional doc-sync.** Every ticket in the SSOT must appear in the
  fence table and vice-versa; the SSOT token set must equal the table's token
  set, both languages. CI fails on drift in **either** direction.

### The tiāntíng sign-off (binding — as written)

> **t1–t7a may be shipped ONLY into egress-acceptable / network-isolated
> execution contexts until t11 lands.** Because the turn-executor retains full
> network egress (deferred to t11) and only a best-effort per-UID fork guard
> (deferred to t9), **t8 is NOT a license to ship the harness into a production
> context that takes hostile input over the network.** In any network-reachable,
> untrusted-input deployment the egress residual is live and must be closed (t11)
> or compensated by an out-of-band network boundary (netns / firewall / no route)
> before shipping.

### Proof model

The proof is the guard itself, exercised both directions: a clean tree passes;
an unannotated deferred token in live code (or in a string literal), a hard
guarantee on `RLIMIT_NPROC`, or any ticket/token drift between SSOT and either
fence table **fails** the `fence-guard` CI job. t8 implements **neither t9 nor
t11** — that is the point.

---

## Phase 5 t11 (= t7b) — turn-executor network-egress containment

t11 is the **egress half** of the original t7 (yudi's SPLIT ruling; the FS/process
half was t7a). It closes the last hard-blocker the t8 fence was holding open. It
is the prose companion to `crates/bwoc-harness/src/seccomp.rs`,
`crates/bwoc-harness/src/jail.rs` (`harden_child_fds`), the seccomp install wired
into `turn_executor::roundtrip`, and the proof in
`crates/bwoc-harness/src/bin/sandbox_redteam.rs` + `tests/sandbox_escape.rs`.

### The closure theorem (the binding claim)

> **egress contained ⟺ (A) the child can't *acquire* a network fd ∧ (B) it
> *holds* none ∧ (D) the arch-guard is tight.**

The **no-fd invariant (A ∧ B) is the PRIMARY control**; the send-side syscall deny
(`sendto`/`sendmsg`/…) is belt-and-suspenders for a descriptor that should not
exist in the first place. **A ∧ B ∧ D are proven by TEST, not assumed** — each is
a red-team arm that LOUD-skips (never false-passes) when seccomp is unavailable.

| Leg | Claim | Mechanism | Proof arm(s) |
|---|---|---|---|
| **A** | can't acquire a network fd | seccomp `KILL_PROCESS` on socket/connect/accept*/bind/listen/getpeername/`sendmmsg`/`recvmmsg`/io_uring_*/`pidfd_getfd`/`pidfd_open` (the **unary** send/recv family + local `socketpair` stay allowed — IPC/tokio need them; only the batched `*mmsg` egress variants are denied) | `net_socket`, `net_abstract_connect`, `pidfd_getfd_steal` (+ `net_socketpair` as the precision/allowed control) |
| **B** | holds no network fd | `close_range(EXECUTOR_FD+1, ~0)` (whole table, not the old `4..1024`) + stdio (0/1/2) socket→`/dev/null` audit | `holds_no_network_fd`, `splice_to_existing_fd`, + the C10 selftest fd assertion + a leaked-`>=1024`-socket regression |
| **D** | arch-guard is tight | seccompiler arch prologue KILLs non-native `seccomp_data.arch`; deny set carries x32-renumbered (`nr \| __X32_SYSCALL_BIT`) variants | `arch_i386_int80` (i386 `int 0x80`), `arch_x32_renumber` |

### Design decisions (the binding conditions)

- **KILL_PROCESS, not ERRNO.** An ERRNO filter is a syscall-enumeration *oracle*;
  KILL gives a hostile child one bit (death) and no map. The ptrace set
  (ptrace/process_vm_readv/process_vm_writev/kcmp/perf_event_open) is in the same
  KILL set, layered on C4's `PR_SET_DUMPABLE(0)`.
- **seccompiler (pure-Rust), not libseccomp.** No C dependency, no FFI to a system
  library. The BPF is compiled in the **parent** (allocation) and installed in the
  child's `pre_exec` via two raw `prctl`s — async-signal-safe, no-alloc (C1). The
  **same installer** (`seccomp::install_in_child`) is called by both
  `turn_executor::roundtrip` (production) and `jail::jail_command` (the red-team /
  standalone path), so they cannot drift.
- **Fail-closed on Linux.** Egress containment is mandatory: if the filter cannot
  be installed, the production executor **refuses to run** (it never degrades to an
  unfiltered child). `PR_SET_NO_NEW_PRIVS` is set idempotently by the installer
  (not relied upon from Landlock). seccomp persists across `execve` and is
  inherited by the `run_command` grandchild.
- **Verify the arch-guard, do not assume it.** P0-1: the seccompiler arch prologue
  must KILL a non-native syscall. This is **proven** by the `arch_i386_int80` arm
  firing an i386 `int 0x80` and asserting a `SIGSYS` kill — the gate does not flip
  green on the assumption that seccompiler emits a killing prologue.

### Condition #5 — Landlock-writable paths must be local filesystems

A Landlock "write" onto a networked filesystem (NFS / CIFS / SMB / 9p / FUSE) is a
network egress in disguise — the FS jail would allow it and seccomp would not see
it (the egress happens in the kernel mount layer, not via a socket the child
holds). The charter therefore binds: **every path in the executor `rw` set MUST be
a local filesystem.** The `landlock_writable_paths_are_local_fs` test `statfs`'s
each existing writable path and fails on any networked `f_type`; it is a standing
CI regression so adding a networked path re-reds the fence. (Honest bound: the
test checks the paths present on the CI runner; a production deployment that points
the worktree at NFS must still satisfy this — it is an operator obligation the test
documents and checks where it can.)

### Condition #7 — explicit scope: network egress only

t11's claim is **network egress containment (Linux)**. It is explicitly **NOT** a
claim about local, same-uid covert channels — the OS keyring, SysV-IPC, POSIX
message queues, a shared file under the worktree, or signals. A same-uid sibling
can always reach those; seccomp is the wrong instrument for them. They are
**out of scope** for t11 and tracked opportunistically as **NEWNET**.

### Proof model (fail-closed, never silent-pass)

`tests/sandbox_escape.rs` (`--features test-redteam`) spawns the hostile child
inside the real seccomp filter and asserts every arm above is contained; the suite
LOUD-skips when `seccomp::available()` is false (so a seccomp-less kernel cannot
yield a false green). The red-team binary uses a **fork-per-arm** harness: each
KILL-prone escape runs in a forked child so the `SIGSYS` takes only that child and
the reporter survives to print the verdict (`killed by SIGSYS` ⇒ blocked;
`exited 0` ⇒ the escape ran — containment FAILED). The same suite runs in CI
(`build-and-test` → `cargo test -p bwoc-harness --features test-redteam --test
sandbox_escape`), so A ∧ B ∧ D are a **standing regression**, not a one-time check.

**Phase 5 is FULLY signed off on t11 merge.** The remaining Phase 5 residual is t9
(the cgroup `pids.max` per-turn process cap — a harness-availability DoS, not an
escape); the macOS arm stays write-confinement-only (Linux is the egress-proof
platform, mirroring t6/t7a).
