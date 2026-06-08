# 2026-06-08 — Phase 5 t8: the deferred-control fence (capstone)

Closed Phase 5 with t8 — the capstone. **No new runtime containment** by design
(Mattaññutā: t8 is honesty, not coverage). It builds a *fence* around the two
containment gaps Phase 5 deliberately left open (t9, t11) so they cannot be
forgotten, mis-stated, or quietly faked. Docs + a CI guard script only; no heavy
Rust.

## What changed

- `scripts/deferred-controls.txt` — **SSOT**. Each deferred control by its REAL
  spelling, bound to a ticket: `seccomp` · `PR_SET_SECCOMP` ·
  `SECCOMP_SET_MODE_FILTER` · `libseccomp` → **t11**; `cgroup` · `/sys/fs/cgroup`
  · `cgroup.procs` · `pids.max` → **t9**.
- `scripts/check-deferred-fence.sh` — the **fence-guard**. Portable to bash 3.2
  (no associative arrays / `mapfile`, so the local macOS gate runs it too).
- `docs/en/THREAT-MODEL.en.md` + `docs/th/THREAT-MODEL.th.md` — a
  machine-checked **deferred-control fence table** (between
  `DEFERRED-FENCE:BEGIN/END` markers) + the binding ship-scope sign-off.
- `.github/workflows/ci.yml` — a `fence-guard` job (pure shell, runs first).
- `docs/phase5-samvara-charter.md` — t8-closure section (claim, the five binding
  conditions, the tiāntíng sign-off as written).
- `docs/en/ROADMAP.en.md` + `.th.md` — Phase 5 section (t1–t8 shipped; t9/t11
  deferred + fenced); Current Status flipped to Phase 5.

## yudi's five binding conditions — how each landed

- **A — real spellings, not nicknames.** SSOT lists exact kernel/library tokens;
  the phantom guard greps *those* in live (comment- + test-stripped) `.rs`.
- **B — annotate, don't instant-fail.** A deferred token in live code — *incl.
  inside a string literal* (`"seccomp unavailable"`) — is **must-annotate**:
  `// DEFERRED(tNN):` on the line (or the line above) clears it. Verified the
  guard does NOT false-positive against honest error strings.
- **C — the honest t9 truth.** Confirmed in `turn_executor.rs`: the executor
  child re-execs under the **harness's own UID** (no `setuid`/`seteuid` on the
  spawn path — only `getuid` for the per-UID proc count). So `RLIMIT_NPROC`
  (per-UID, RELATIVE) means a fork-bomb in the child fills the **per-UID** process
  table and can DoS the harness itself — **availability, not escape → 🟠**. The
  t9 row says exactly this. Best-effort rule also enforced: NPROC keeps its
  RELATIVE/best-effort marker, and the guard fails if any live line treats it as
  a hard guarantee (`.expect`/`assert!`).
- **D — ship-scope sign-off.** Written verbatim in the THREAT-MODEL fence section,
  the charter, and the ROADMAP: t1–t7a ship ONLY into egress-acceptable /
  network-isolated contexts until t11; t8 is NOT a license to ship into a
  network-reachable prod context taking hostile input.
- **E — bidirectional doc-sync.** Guard fails on ticket OR token drift in either
  direction, both languages (SSOT↔EN table, SSOT↔TH table).

## Guard internals (honest about limits)

The Rust stripper (perl) handles `//` + `/* */` comments (string-aware),
preserves string/char literal *contents* (so condition B can see tokens in
strings), distinguishes `'a` lifetimes from `'c'` char literals, and blanks
`#[cfg(test)]` mod/fn blocks by brace-match. It does **not** fully model raw
strings with embedded quotes (`r#"..."#`) — no such usage exists today; the
limitation is documented in the script rather than silently assumed away. The
red-team bin (`sandbox_redteam.rs`, gate-only) is excluded from the live-src set.

## Verification

`bash scripts/check-deferred-fence.sh` green on a clean tree. Exercised both
directions: bare phantom token → FAIL; annotated → PASS; token in a string,
unannotated → FAIL; annotated → PASS; token only in a comment / in a
`#[cfg(test)]` mod → PASS (stripped); ticket/token drift (EN + TH) → FAIL; NPROC
`.expect`-as-guarantee in live code → FAIL. Full gate: fence-guard + `cargo fmt
--check` + `cargo clippy -D warnings` + `cargo test --workspace`.

## Not done (deliberately)

t9 and t11 are **not** implemented — that is the whole point of an honesty gate.
The fence guarantees they stay named, scoped, and un-foolable until their own
tickets land.
