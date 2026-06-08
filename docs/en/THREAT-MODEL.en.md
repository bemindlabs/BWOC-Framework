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
> **t7a does NOT claim** egress containment — network, ssh-agent, and
> abstract-socket reachability remain open and are **t7b / ticket t11**.

This is deliberately *not* phrased as "no shared writable mount": a shared mount
still exists. What t7a removes is the executor's filesystem *reach* into the
harness and the `ptrace`/proc path to the parent's RAM.

## Controls and what they close

| Control | Threat closed | Mechanism |
|---|---|---|
| **C1** — FS jail on the executor | Executor reads/mutates harness files (`~/.ssh`, source, checkpoint) | Linux **Landlock** domain installed in `pre_exec`: read+write+exec on `{worktree, per-turn tempdir}`, read+exec on the binary + a minimal system allowlist, everything else (`$HOME`, checkpoint dir, `/proc/<other>`) denied, `no_new_privs` set. macOS = `sandbox-exec` **write-confinement only** (reads NOT jailed — Linux-only, mirroring t6's Linux-only `RLIMIT_AS`). Probes and **LOUD-skips** when unavailable. |
| **C4** — parent anti-ptrace | **CRIT-1**: a same-uid child reads the parent's API keys from RAM via `ptrace`/`process_vm_readv` | Parent calls `prctl(PR_SET_DUMPABLE, 0)` (a non-dumpable process can only be inspected by root, so a same-uid child gets `EPERM`) **and** verifies `kernel.yama.ptrace_scope ≥ 1`, **failing closed** if it reads `0`. |
| **C5** — no wholesale `/proc` | proc-mem / cross-process info leak | The jail allowlist excludes `/proc` entirely; only path-granular entries the loader/runtime actually need are granted (none under `/proc` in practice). |
| **C6** — env-scrub extension | Live-authority leak into the child | `SSH_AUTH_SOCK`, `GPG_AGENT_INFO`, `GNUPGHOME`, `DBUS_SESSION_BUS_ADDRESS` are dropped from the executor env (allowlist-deny + pattern-deny). |
| **C7** — jailed post-turn git | **CRIT-2**: a planted `build.rs` / git hook / `core.fsmonitor` executes as the unjailed parent | Build/test run **in-child** under the C1 jail (a planted `build.rs` runs *confined*); the parent runs `git` on the untrusted worktree inside the same jail with `core.hooksPath=/dev/null` plus config overrides (`core.fsmonitor`, `core.pager`, `core.sshCommand`, `diff.external`, global/system config → `/dev/null`). |
| **C8** — binary RO; checkpoint outside the jail | **M3** (overwrite `current_exe`), **M2** (forge the trust latch) | The binary is read+exec-only (never writable); the SessionTrust checkpoint lives outside the jail rw set, so the child cannot write it — the latch is parent-written only. |
| **C9** — bounded fd3 read | IPC abuse / descriptor smuggling | The parent reads one frame with a finite timeout, a max-length cap, close-after-one-frame, and **no `cmsg` buffer** (so a child cannot pass a descriptor back via `SCM_RIGHTS`). |

## Residuals — what is NOT contained (t7b / t11)

t7a is the **process/FS** half of the original t7. The egress half is deferred to
**t7b (ticket t11)** and is explicitly still open:

- **Network egress** — the executor can still open sockets (DNS, TCP, UDP).
- **ssh-agent / abstract sockets** — `SSH_AUTH_SOCK` is scrubbed from the env
  (C6), but an abstract-namespace socket reachable without a path is not
  contained by an FS jail; that is a seccomp/netns concern (t7b).
- **Mount-namespace isolation** — not claimed; the worktree mount is shared.
- **macOS read confinement** — Linux-only; on macOS the executor jail is
  write-confinement only (the red-team read/ptrace/proc arms LOUD-skip there).

## Proof

The gate proof is the adversarial red-team suite
(`crates/bwoc-harness/tests/sandbox_escape.rs`, built under
`--features test-redteam`): a hostile child is spawned **inside the real executor
jail** and every escape it attempts against planted canaries must fail —
fail-closed, never silent-pass. See the charter (`phase5-samvara-charter.md`) for
the full control-by-control proof model.
