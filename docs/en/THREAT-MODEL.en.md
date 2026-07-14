---
title: Threat Model
parent: English
nav_order: 15
---

# Threat Model — the turn-executor trust boundary (Phase 5)

This document records the trust boundary of the BWOC self-hosted harness
(`bwoc-harness`) and, in particular, the **turn-executor** isolation built in
Phase 5. It is the framework-level companion to the agent template's own
`THREAT-MODEL.md` (the per-agent Taṇhā-3 craving model) and to the
`phase5-samvara-charter.md` proof contract.

## The boundary

After the safety pipeline approves a tool call, the harness does **not** run it
in the agent process. The parent re-execs its own binary as a hidden
`--__turn-executor` child, hands it one framed request over an inherited socket,
and the child runs exactly that one tool then dies (Phase 5 t5). The parent keeps
the LLM loop, the provider API keys, and the monotonic `SessionTrust` latch; the
child holds none of them.

- **Trusted:** the parent process (keys, trust latch, checkpoint writer).
- **Untrusted after the first child turn:** the worktree contents (a child turn
  may have written a malicious `build.rs`, git hook, or `.git/config`).
- **Hostile, by assumption:** the turn-executor child itself — t7a treats it as
  fully attacker-controlled and asks what it can still reach.

## t7a claim (honest gate text)

> **t7a claims:** the turn-executor cannot read or mutate the harness via the
> filesystem, via `ptrace`/proc-mem, or via a planted build artifact.
>
> **t7a does NOT claim** mount-namespace isolation.
>
> **t7a (by itself) does not claim** egress containment — that is the **t7b /
> t11** half, now **LANDED** (see *Network egress containment* below). As of t11
> the executor's network egress IS contained on Linux; t7a remains the FS/process
> half of the proof.

This is deliberately *not* phrased as "no shared writable mount": a shared mount
still exists. What t7a removes is the executor's filesystem *reach* into the
harness and the `ptrace`/proc path to the parent's RAM.

## Controls and what they close

| Control | Threat closed | Mechanism |
|---|---|---|
| **C1** — FS jail on the executor | Executor reads/mutates harness files (`~/.ssh`, source, checkpoint) | Linux **Landlock** domain installed in `pre_exec`: read+write+exec on `{worktree, per-turn tempdir}`, read+exec on the binary + a minimal system allowlist, everything else (`$HOME`, checkpoint dir, `/proc/<other>`) denied, `no_new_privs` set. macOS = `sandbox-exec` **write-confinement + egress-deny (t29) + a selective secret read-denylist** (#329 — a *narrowed* residual, not a full read-jail; see Residuals); the full read-jail stays Linux-only, mirroring t6's Linux-only `RLIMIT_AS`. Probes and **LOUD-skips** when unavailable. |
| **C4** — parent anti-ptrace | **CRIT-1**: a same-uid child reads the parent's API keys from RAM via `ptrace`/`process_vm_readv` | Parent calls `prctl(PR_SET_DUMPABLE, 0)` (a non-dumpable process can only be inspected by root, so a same-uid child gets `EPERM`) **and** verifies `kernel.yama.ptrace_scope ≥ 1`, **failing closed** if it reads `0`. |
| **C5** — no wholesale `/proc` | proc-mem / cross-process info leak | The jail allowlist excludes `/proc` entirely; only path-granular entries the loader/runtime actually need are granted (none under `/proc` in practice). |
| **C6** — env-scrub extension | Live-authority leak into the child | `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, `GNUPGHOME`, `DBUS_SESSION_BUS_ADDRESS` are dropped from the executor env (allowlist-deny + pattern-deny). |
| **C7** — jailed post-turn git | **CRIT-2**: a planted `build.rs` / git hook / `core.fsmonitor` executes as the unjailed parent | Build/test run **in-child** under the C1 jail (a planted `build.rs` runs *confined*); the parent runs `git` on the untrusted worktree inside the same jail with `core.hooksPath=/dev/null` plus config overrides (`core.fsmonitor`, `core.pager`, `core.sshCommand`, `diff.external`, global/system config → `/dev/null`). |
| **C8** — binary RO; checkpoint outside the jail | **M3** (overwrite `current_exe`), **M2** (forge the trust latch) | The binary is read+exec-only (never writable); the SessionTrust checkpoint lives outside the jail rw set, so the child cannot write it — the latch is parent-written only. |
| **C9** — bounded fd3 read | IPC abuse / descriptor smuggling | The parent reads one frame with a finite timeout, a max-length cap, close-after-one-frame, and **no `cmsg` buffer** (so a child cannot pass a descriptor back via `SCM_RIGHTS`). |

## Network egress containment (t11 / t7b — ENFORCED, Linux)

t11 (= t7b) is the egress half of the original t7, and it is now **implemented**
(an enforced control, no longer a residual). The closure theorem the proof is
built on (yudi, adopting nezha's red-team result):

> **egress contained ⟺ (A) the child can't *acquire* a network fd ∧ (B) it
> *holds* none ∧ (D) the arch-guard is tight.**

The **no-fd invariant (A ∧ B)** is the PRIMARY control; the send-side syscall
deny is belt-and-suspenders for a descriptor that should not exist. Each leg is
proven by a red-team arm (`tests/sandbox_escape.rs`, `--features test-redteam`),
never assumed.

| Control | Threat closed | Mechanism |
|---|---|---|
| **t11-A** — can't acquire a network fd | Executor opens a socket / steals one (DNS, TCP, UDP, abstract-namespace AF_UNIX, `pidfd_getfd` fd-theft, io_uring) | seccomp-bpf `SECCOMP_RET_KILL_PROCESS` deny set (socket/connect/accept*/bind/listen/getpeername/sendmmsg/recvmmsg/io_uring_*/pidfd_getfd/pidfd_open) compiled by **seccompiler** (pure-Rust) in the parent, installed via `prctl` in the child's `pre_exec`. KILL (not ERRNO) so a hostile child gets no syscall-enumeration oracle. The **unary** send/recv family and a *local* `socketpair` stay allowed (the executor's AF_UNIX IPC + tokio need them); only the batched `*mmsg` egress variants are denied. |
| **t11-B** — holds no network fd | A leaked/inherited socket fd (incl. one **>= 1024**) is a ready egress target | `close_range(EXECUTOR_FD+1, ~0)` closes the WHOLE descriptor table (the old `4..1024` loop missed >= 1024), plus a stdio (0/1/2) audit that re-points any socket-backed stdio at `/dev/null`. |
| **t11-D** — arch-guard is tight | An i386 `int 0x80` or x32-renumbered (`nr | __X32_SYSCALL_BIT`) syscall side-steps the x86_64 deny set | seccompiler's arch prologue KILLs any non-native `seccomp_data.arch`; the deny set additionally carries the x32-renumbered variants. Both proven by red-team arms (the prologue's KILL is **verified by test**, not assumed). |
| **t11 / C4** — ptrace defence-in-depth | A same-uid child reads parent RAM via ptrace/process_vm_readv | The ptrace set (ptrace/process_vm_readv/process_vm_writev/kcmp/perf_event_open) is in the KILL deny set, layered on top of the parent's `PR_SET_DUMPABLE(0)`. |

**Fail-closed (Linux):** egress containment is mandatory. If the filter cannot be
installed the production turn-executor **refuses to run** (it never degrades to an
unfiltered executor). `PR_SET_NO_NEW_PRIVS` is set idempotently by the seccomp
installer (not relied upon from Landlock). seccomp persists across `execve` and is
inherited by any `run_command` grandchild, so it too is egress-contained.

**Scope (honest).** This is **network egress containment (Linux)**. It is
explicitly **NOT** a claim about local, same-uid covert channels — the OS keyring,
SysV-IPC, POSIX message queues, a shared file under the worktree, or signals.
A same-uid sibling can always reach those; seccomp is the wrong tool for them and
they are **out of scope** for t11 (tracked opportunistically as NEWNET).

## Residuals — what is still NOT contained

- **Mount-namespace isolation** — not claimed; the worktree mount is shared.
- **Local same-uid covert channels** — out of scope (see *Scope* above).
- **Per-turn process cap** — t9 landed: an absolute per-turn cgroup v2 `pids.max`
  cap is enforced **when a writable cgroup v2 subtree is delegated** (systemd
  `Delegate=yes` / a privileged container) — ✅. In the **default** case (dev box,
  bare-SSH login, non-delegated container) **no** subtree is delegated, so the
  fork guard degrades to the best-effort **per-UID, RELATIVE** `RLIMIT_NPROC`
  floor — 🟠. The residual is therefore the non-delegated default: a fork bomb
  fills the per-UID process table (a harness-availability DoS, **not** an escape).
  `BWOC_REQUIRE_CGROUP_PIDS=1` makes the harness refuse to start unless the
  delegated subtree is present, for prod that demands the hard cap (the file-tracked
  deployment prereq — a `Delegate=yes` unit drop-in — is **t14**).
- **macOS read-confinement (narrowed, #329)** — macOS is a **dev-only** platform
  for the turn-executor. Write-confinement (SBPL `(deny file-write*)`) and network
  egress-deny (SBPL `(deny network*)`, t29) are enforced; the **read** side is a
  *narrowed* residual, **not** Landlock parity. `sandbox.rs` layers a selective
  `(deny file-read* …)` arm over a curated **denylist** of known high-value
  secrets (`~/.ssh`, `~/.aws`, `~/.config/gcloud`, `~/.config/gh`, and the BWOC
  home holding agent keys + SessionTrust checkpoints), on-by-default and
  fail-closed (`BWOC_SANDBOX_ALLOW_SECRET_READ=1` is the only opt-out seam). A
  full deny-default read arm is deliberately avoided — it breaks the dyld
  shared-cache reads `sandbox-exec` needs to launch a dynamically-linked binary.
  **Residual:** an *unlisted* secret path is still readable, and the red-team
  read / ptrace arms stay Linux-only (they LOUD-skip on macOS). The egress-deny
  (t29) already compensates the direct-exfil path.

## The deferred-control fence (t8)

Phase 5 t8 built a *fence* so a deferred control's absence could never be
forgotten or faked: a single source of truth (`scripts/deferred-controls.txt`)
named each missing control by its real kernel/library spelling, a CI guard
(`scripts/check-deferred-fence.sh`) kept that SSOT, the fence region below, and the
live source in lock-step, and a phantom-control check failed the build if code ever
*referenced* one of these controls (even in a string literal) without an explicit
`// DEFERRED(tNN):` admission that it was not really there.

Both controls it fenced have since **landed**: t11 closed the **egress** boundary,
and t9 added the **per-turn process cap** (cgroup v2 `pids.max`, best-effort — see
*Residuals* above). The SSOT therefore enumerates **no deferred tokens** and the
fence is **fully discharged**. The guard still runs every CI pass to keep that
terminal state honest: it re-fails if a landed control's residual is dropped from
the THREAT-MODEL (condition C, EN + TH), or if a fresh deferral re-introduces a
phantom. The machine-checked region below is consequently empty of tokens.

<!-- DEFERRED-FENCE:BEGIN — machine-checked against scripts/deferred-controls.txt; this region must hold NO deferred tokens (backticks) and NO ticket ids while the fence is discharged. -->

All Phase 5 deferred controls have landed (the per-turn process cap and the
egress/syscall filter); this fence is **fully discharged**. The honest residual
that survives each landed control — chiefly the non-delegated fork-guard fallback —
is documented in the Residuals section above, not here.

<!-- DEFERRED-FENCE:END -->

### Permission scope (the binding sign-off) — updated at t11

t8 closed the **honesty** gate; t11 then closed the **egress** control it was
fencing. The sign-off is therefore lifted for its original blocker:

> **Phase 5 is FULLY signed off (t11 merged).** On **Linux**, the turn-executor's
> network egress is now contained (seccomp + the no-fd invariant + a tight
> arch-guard, fail-closed). The earlier restriction — *"ship only into
> egress-acceptable / network-isolated contexts until t11 lands"* — is **lifted on
> Linux**. Two honest caveats remain: (1) the per-turn fork guard is an absolute
> cgroup v2 `pids.max` cap **only where a writable subtree is delegated** (systemd
> `Delegate=yes` / privileged container, asserted at startup by
> `BWOC_REQUIRE_CGROUP_PIDS=1`); in the **default** non-delegated case it degrades
> to the best-effort **per-UID** `RLIMIT_NPROC` floor — a harness-availability DoS,
> not an escape (the delegated-deployment prereq is **t14**); (2) on **macOS**
> neither Landlock nor seccomp applies (write-confinement only), so macOS remains a
> dev-only platform. Local same-uid covert channels are out of scope by design
> (NEWNET).

## Proof

The gate proof is the adversarial red-team suite
(`crates/bwoc-harness/tests/sandbox_escape.rs`, built under
`--features test-redteam`): a hostile child is spawned **inside the real executor
jail** and every escape it attempts against planted canaries must fail —
fail-closed, never silent-pass. See the charter (`phase5-samvara-charter.md`) for
the full control-by-control proof model.
