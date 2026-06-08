# 2026-06-08 — Phase 5 (saṃvara): trust-boundary & sandbox hardening — capstone

Capstone log for Phase 5 — *saṃvara* (restraint at the boundary where untrusted input becomes effect). Phase 3's chat-connectors (Telegram/Discord/LINE) opened an unauthenticated, adversarial ingress surface straight into the self-hosted `bwoc-harness`; Phase 5 closes it. Chartered 2026-06-07 by the tianting council (chair: yudi; contract: luban; red-team: nezha) and executed gate-by-gate, each via **plan → yudi Pavāraṇā approval → implement → stacked PR**, with nezha red-teaming the escape gates. All work accumulates on the integration branch `feat/phase5-t1-ingress-labeling` (PR #238 → `main`).

## The 8 DoD gates (all met)

| Gate | What it establishes | PR |
|---|---|---|
| t1 | Total ingress trust-labeling: every `ChatMessage` carries an immutable `Principal`; `TrustLevel` is *derived*, never stored (so "promote to trusted" is unrepresentable); fail-closed (`Unknown → Untrusted`). Fixed a real laundering bug (teammate text injected as `role:System`). | #238 |
| t2 | Layer-0 default-deny capability gate: an untrusted turn gets read-only tools only; zero allow-by-omission. Sticky `untrusted_seen` latch persisted across compaction+reload (closes the compaction-launder hole). | #242 |
| t3 | Taint propagation + capability-graded gate: Tool/McpTool join trust-scan; compaction max-taint via `Principal::Summary` (whose `trust()` is always fail-closed); worktree-confined writes allowed under untrusted, escape/persist/destruct gated. | #243 |
| t4 | `PURE_READ_TOOLS` proven egress-clean: enumeration-driven proof-test (behavioral tripwire that must be armed = fail-closed) + static scan; single construction site; allow-by-omission impossible. | #250 |
| t5 | Per-turn process isolation via **re-exec** (not fork — tokio-threaded fork is UB): one-shot pipe IPC, unforgeable 256-bit token, env-scrub, fd hygiene, un-marshallable tools fail-closed. | #248 |
| t6 | Per-turn `setrlimit` (CPU 300/600, AS 4GiB, NOFILE 1024, **FSIZE 8GiB**); NPROC relative (`usage+128`), honestly labeled best-effort per-UID. Bombs opt-in; getrlimit-snapshot is the always-on proof. | #249 |
| t7a | Executor **FS jail** (Landlock/sandbox-exec on the executor itself) + `PR_SET_DUMPABLE(0)`+yama (closes ptrace RAM-read) + jailed build/`core.hooksPath=/dev/null` (closes `build.rs` RCE). | #251/#252 |
| t8 | Deferred-risk **fence**: authoritative residual table (one real open vector flagged — egress) + CI `fence-guard` (phantom-control + best-effort + bidirectional doc-sync rules) + honest sign-off statement. | #253 |

## The red-team arc (why nezha mattered)

Three critical escapes the original design matrices missed, all surfaced by nezha and closed:

- **ptrace / `process_vm_readv` (CRIT-1):** "separate address space" ≠ inaccessible — same-UID ptrace reads the parent's live API keys + flips the latch. Closed by t7a's `PR_SET_DUMPABLE(0)`+yama (kernel-level, no seccomp needed).
- **worktree-planted `build.rs` RCE (CRIT-2):** child writes to the rw worktree, parent's *unjailed* `cargo build`/`git` then executes it. Closed by t7a's jailed-build + `core.hooksPath=/dev/null`.
- **seccomp denylist insufficiency (t11):** deny-send-family is whack-a-mole; byte-movers (`write`/`splice`/`sendfile`) defeat it if a socket fd exists. nezha's theorem — *egress contained ⟺ no network-fd ∧ can't acquire one ∧ arch-guard tight (A∧B∧D)* — became the closure criterion. (See [[bwoc-sandbox-escape-gotchas]].)

## Decisions

- **t7 SPLIT (yudi):** t7a = process/FS confinement (shipped); t7b = egress/syscall hardening = **t11**, the single hard-blocker on *full* Phase 5 sign-off. seccomp stays charter-deferred from t7a; only Phase-5-sign-off is gated on t11 — no charter amendment.
- **Capability-graded gate (t3, yudi):** rejected both strict-brick and turn-scoped-taint (the latter re-opens laundering); chose blast-radius tiers so read-then-edit survives without weakening the session-monotonic latch.
- **t11 = denylist + test-proven no-fd invariant + arch-guard** (not full allowlist, not bare denylist), `KILL_PROCESS`+fork-per-arm, `seccompiler` (no `libseccomp` C dep). NEWNET is the truly sound posture but deferred (prod Ubuntu restricts unprivileged userns) → tracked follow-up.
- **Fence honesty (t8):** "fence documented + enforced" ≠ "zero residual." Green rows carry labels + standing CI regression so invariant drift auto-re-reds them (no silent rot).

## Status / deferred

- **t1–t8 DoD: met.** t11 (seccomp) **in flight** at time of writing — on its merge with all 8 binding conditions test-proven, the t8 fence egress row flips 🔴→🟢 and **full Phase 5 sign-off is reached**.
- **Honest claim today:** t1–t7a are shippable into **egress-isolated / network-isolated** contexts; until t11, a hostile child can still exfil over the network.
- **Out of scope (named, not hidden):** local same-uid covert channels (keyring/SysV-IPC/mq/shared-file) — not "data-exfil containment," only "network egress containment (Linux)."
- **Tracked tickets:** t9 (cgroup `pids.max` — deterministic fork-bomb containment), t10 (run fork/mem bombs once on a Linux host — macOS can't enforce), t12 (re-audit the escape matrix under threat-model (B)), + a NEWNET opportunistic-open follow-up.
- **Platform:** the strong controls (Landlock, seccomp, rlimit AS/DATA) are Linux-first; macOS dev boxes degrade with LOUD-skips, never silent-pass. Verification runs on Ubuntu CI / Docker (the dev box + `bmt` have no cargo).

## Related

- `docs/en/ROADMAP.en.md` §Phase 5 (+ `.th.md`) · `docs/phase5-samvara-charter.md`
- `notes/2026-06-07_phase5-charter.md` · `notes/2026-06-08_phase5-t7a-executor-fs-jail.md` · `notes/2026-06-08_phase5-t8-fence.md`
- `docs/en/THREAT-MODEL.en.md` fence table · `scripts/check-deferred-fence.sh`
