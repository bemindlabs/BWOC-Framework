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
