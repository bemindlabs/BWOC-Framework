---
title: Phase 6 t29 (PHASE6-1) — macOS egress-containment parity
date: 2026-06-12
tags:
  - type/note
  - area/harness
  - phase/6
---

# Phase 6 t29 — macOS network-egress parity in the sandbox SBPL

## What changed

`crates/bwoc-harness/src/sandbox.rs::build_sbpl_profile` (the macOS
`run_command` grandchild OS-sandbox layer) now emits a default `(deny network*)`
arm — the macOS parity for the Linux seccomp egress filter (`seccomp.rs`, t11).
Previously the macOS SBPL jailed FS writes to the worktree but left "network, ipc
out of scope", so the sandboxed turn-executor subprocess on macOS could do
arbitrary network egress. That gap is closed.

`network*` covers `network-inbound` / `network-outbound` / `network-bind` — the
SBPL analogue of seccomp killing `socket`/`connect`/`bind`/`listen`/`accept*`
(control A: the child can neither acquire nor use a network socket).

## Decisions

- **Posture symmetry over flag symmetry (Samānattatā).** The brief asked for "the
  same env/config flag names Linux uses." Linux uses *none* — `seccomp` egress is
  unconditional and strictly fail-closed (no opt-out at all; the t11 closure
  theorem makes it mandatory). The honest parity is therefore **on-by-default,
  fail-closed, no production opt-out** on macOS too. I added a single
  `BWOC_SANDBOX_ALLOW_NET` env *seam* honored only when set to exactly `"1"`
  (anything else ⇒ denied), documented as a test/operator escape-hatch — NOT a
  production toggle. It fills the same role Linux fills by running a legitimately
  network-bound tool in the *parent* instead of the child.
- **Rule ordering.** SBPL is last-match-wins. The deny-network line sits ABOVE the
  file-write rules and below `(allow default)`, so it cannot be re-opened by a
  later rule and does not perturb the FS jail. Tested.
- **Scope.** Only `build_sbpl_profile` + its tests. The Linux-only residual
  (process-cap / cgroup pids.max — t9) is explicitly OUT OF SCOPE.

## Out of scope (named, not hidden)

- This is the `sandbox.rs` grandchild layer. Per Phase 5 t7a, when the executor
  FS-jail is active the grandchild already runs a **no-op** OS sandbox (nested
  `sandbox-exec` is forbidden on macOS), so this deny mainly hardens the
  **fallback** path (jail unavailable) — exactly mirroring seccomp's
  belt-and-suspenders role. The executor-jail SBPL in `jail.rs`
  (`macos_write_confine_profile`) is a separate profile and was left untouched;
  extending egress-deny there is a follow-up if macOS is ever treated as more than
  a dev box.
- Linux process-cap residual (t9) — separate ticket.

## Ruling (agent-luban) — blanket `(deny network*)` over a narrow allowlist

**Affirmed: blanket `(deny network*)`. Rejected: a localhost / host-scoped
allowlist on macOS.** Grounded in the actual control inventory, not symmetry for
its own sake:

- **macOS has no control-A backstop.** The egress story is two controls.
  *Control A* stops the child **opening a new** socket (`socket`/`connect`);
  *control B* (`harden_child_fds`, `jail.rs:269`) stops it **reusing an
  inherited** one. On Linux, A = seccomp (`seccomp.rs`, t11, unconditional) and B
  = `close_range` (total). On macOS, B is a **best-effort bounded `0..1024`
  loop** (no `close_range`; an fd ≥ 1024 leaks) and, crucially, **there is no
  control A at all** until this SBPL arm — nothing else can stop the sandboxed
  child from calling `socket()`+`connect()` itself. The `(deny network*)` line
  is therefore the **sole** control A on macOS. It is load-bearing in a way the
  Linux line is not.
- **An allowlist on macOS is an uncontained hole.** A localhost exception would
  let the child open a *new* egress socket to loopback, and control B does
  nothing about a socket the child opens itself — there is no second layer to
  catch what the allowlist admits. On Linux you could narrow seccomp and still
  lean on landlock + the total no-fd invariant; macOS has only this one line.
- **localhost is not benign here.** Loopback reaches the IPC gateway, sibling
  sandboxed peers, and local daemons — precisely the lateral-movement /
  exfil-relay surface the egress filter exists to close. The Linux t11 closure
  theorem denies loopback too; a macOS localhost allowance would break parity in
  the *weakening* direction.
- **Mattaññutā.** One deny line that cannot drift beats an allowlist that must
  enumerate and maintain host scopes. The legitimate "I need a local socket"
  case is already served by the fail-closed `BWOC_SANDBOX_ALLOW_NET=1` seam plus
  the "run the network tool in the parent" pattern — no standing hole required.

Samānattatā is satisfied as **posture** parity (on-by-default, fail-closed, no
production opt-out), not flag-for-flag mimicry. The implementation already
encodes this ruling; no code change follows from it — it is finalized as-is.

## Proof

Three rendered-string tests beside the existing macOS sandbox tests
(`sbpl_profile_denies_network_by_default`,
`sbpl_profile_network_escape_hatch_toggles`,
`sbpl_network_escape_hatch_is_fail_closed`) — assert the SBPL string, no
`sandbox-exec` spawn required. They share a `NET_ENV_LOCK` mutex because they
mutate the process-global env var (a parallel writer clobbered a reader — caught
on first run, fixed by serializing).

`cargo test -p bwoc-harness --lib`: 389 passed, 1 ignored. `cargo clippy
-p bwoc-harness -- -D warnings`: clean. `cargo fmt`: clean.
