---
title: Phase 5 t7a — turn-executor process / FS jail
date: 2026-06-08
tags:
  - type/note
  - area/harness
  - phase/5
---

# Phase 5 t7a — turn-executor process / FS confinement

## What changed

Hardened the re-exec'd `--__turn-executor` child (t5/t6) from a bare
resource-bounded process into an FS-confined one, and closed the two CRITs the
prior gates left open. Scoped to **t7a** per yudi's SPLIT ruling; the
egress/seccomp/netns half is **t7b (ticket t11)**, a deferred hard-blocker.

New module `crates/bwoc-harness/src/jail.rs` (the reusable process FS jail) plus
wiring in `turn_executor.rs`, `result.rs`, `main.rs`, and `bwoc-core::env_scrub`.

## Controls (all mandatory; all landed)

- **C1** — FS jail on the executor process, installed in `roundtrip`'s
  `pre_exec`. Linux: a **Landlock** domain (rw = {worktree, per-turn tempdir};
  read+exec = binary + minimal system allowlist; `$HOME`/checkpoint/`/proc`
  denied; `no_new_privs`). The ruleset is built in the PARENT (alloc-heavy) and
  only `landlock_restrict_self` runs post-fork (async-signal-safe — the parent
  is multi-threaded tokio, so a post-fork heap alloc could deadlock). macOS:
  `sandbox-exec` **write-confinement only** (read jail too fragile to run a
  dynamic binary — `deny default` kills even `echo`), documented Linux-only like
  t6's `RLIMIT_AS`.
- **C4** — `prctl(PR_SET_DUMPABLE,0)` on the parent + `yama.ptrace_scope ≥ 1`
  fail-closed at startup. Dumpable=0 is the real block (same-uid
  ptrace/process_vm_readv → EPERM); yama is defence in depth. Closes **CRIT-1**.
- **C5** — `/proc` is not allowlisted wholesale (none needed in practice).
- **C6** — drop `SSH_AUTH_SOCK` / `GPG_AGENT_INFO` / `GNUPGHOME` /
  `DBUS_SESSION_BUS_ADDRESS` from the executor env (allowlist + pattern deny).
- **C7** — closes **CRIT-2**. Builds/tests run in-child under C1 (a planted
  `build.rs` runs *confined*); the parent's post-turn `git`
  (`DiffSummary::from_worktree`) runs inside the same jail with
  `core.hooksPath=/dev/null` + config overrides + global/system config →
  `/dev/null`, so a planted hook/`core.fsmonitor`/`diff.external` can't run as
  the unjailed parent.
- **C8** — binary is read+exec-only (M3: no `current_exe` overwrite); the
  SessionTrust checkpoint dir is outside the jail rw set (M2: parent-written
  only). Both structural, not runtime checks.
- **C9** — bounded fd3 read: finite `IPC_TIMEOUT`, max-frame cap,
  close-after-one-frame, no cmsg buffer (drops `SCM_RIGHTS`).

## Decisions / surprises

- **macOS read jail abandoned.** Empirically, a `deny default` sandbox-exec
  profile cannot run a dynamic binary (mach/sysctl exec machinery). Chose
  write-confinement on macOS + honest Linux-only scoping rather than a fragile,
  version-sensitive read jail. The red-team read/ptrace/proc arms LOUD-skip on
  macOS.
- **Nested sandbox-exec is forbidden on macOS** (`sandbox_apply: Operation not
  permitted`). Since the C1 executor jail is inherited by the `run_command`
  grandchild, the grandchild's own OS-sandbox layer is now redundant — and would
  fail to nest on macOS. The parent signals `BWOC_TURN_EXECUTOR_JAILED=1` and the
  child uses a no-op OS sandbox when jailed (arg-scan + env-scrub still apply),
  falling back to `make_os_sandbox` only when the jail was unavailable.
- **`SSH_AUTH_SOCK` was already pattern-stripped** (it contains "AUTH"); removed
  from the allowlist anyway so the intent is explicit and future-proof.

## Proof

`crates/bwoc-harness/tests/sandbox_escape.rs` (`--features test-redteam`) spawns
the hostile `sandbox_redteam` bin inside the real executor jail and asserts every
escape fails (read `~/.ssh`, write outside, overwrite checkpoint/self-binary,
process_vm_readv the parent, read `/proc/<parent>/environ`); a second test plants
`core.fsmonitor` and asserts the hardened `DiffSummary::from_worktree` does not
run it. Fail-closed / LOUD-skip throughout.

**Local validation (macOS):** clippy (incl. `--features test-redteam`), fmt,
`bwoc-harness`/`bwoc-core` lib (495), and the `process_isolation` +
`resource_limits` + `sandbox_escape` integration suites all pass. The Linux
Landlock + C4 arms (read/ptrace/proc) are CI-verified — no Linux target locally
(established t6 pattern; bemind has no cargo).

## Still open

t7b / t11: egress containment (network, ssh-agent, abstract sockets) via
seccomp/netns. Mount-namespace isolation is explicitly NOT claimed by t7a.
