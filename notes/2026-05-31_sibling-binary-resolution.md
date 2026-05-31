# 2026-05-31 — Sibling BWOC binaries resolve relative to the running executable

Follow-up sweep after the `bwoc chat --tmux` fix (#150): the same "bare program name = `$PATH` lookup, not the running binary" bug existed at every site where one BWOC binary spawns another. Consolidated the resolution rule into `bwoc-core` and fixed all of them.

## What changed

- **New module `crates/bwoc-core/src/exec.rs`** — `sibling_binary(name) -> Option<PathBuf>` and `binary_or_name(name) -> OsString`. The three-tier rule (already used ad-hoc by `spawn::harness_binary`): file next to `current_exe()` → `CARGO_BIN_EXE_<name>` (Cargo test) → `$PATH`. Pure `std`, no new deps — safe for the lean core crate. Lives in core so `bwoc-cli`, `bwoc-harness`, and `bwoc-agent` (all already depend on it) share one implementation.
- **Fixed sites** (each previously spawned a bare name):
  - `bwoc-cli/src/start.rs`, `supervise.rs` — spawn the `bwoc-agent` daemon. Error message now names the resolved binary instead of an unconditional "is bwoc-agent on PATH?".
  - `bwoc-harness/src/tools/extra_tools.rs` (×2) — harness `task` and `send` tools shell out to `bwoc`.
  - `bwoc-agent/src/task_watch.rs` — auto-claim shells out to `bwoc task claim`.
- **De-duplicated** `spawn::harness_binary()` down to a one-line delegate to `bwoc_core::exec::sibling_binary("bwoc-harness")`.

## Decisions

- **`binary_or_name` returns `OsString`, not `String`** — it feeds `Command::new`, which takes `AsRef<OsStr>`; avoids a lossy round-trip and matches `Command`'s native type.
- **Fallback to the bare name** preserves prior behavior when `current_exe()` and `$PATH` both miss, rather than failing hard — same conservative stance as `bwoc_exe()`.
- **Kept `bwoc_exe()` (from #150) separate** from `binary_or_name("bwoc")`. `bwoc_exe()` means "run *myself*" → `current_exe()` directly (correct even if the binary was renamed). `binary_or_name("bwoc")` means "find the sibling *named* `bwoc`" → used by harness/agent, whose own `current_exe()` is `bwoc-harness`/`bwoc-agent`. Different intent, different function.
- **One concern per PR.** This is the "resolve sibling binaries" concern. The separately-found `EVIDENCE_KINDS` duplication (`check.rs` + `audit.rs`) and the `banner.rs` backend-list drift are a distinct "dedup drift-prone constants" concern → deferred to their own PR.

## Bugs surfaced and fixed

In any layout where the running `bwoc`/`bwoc-harness`/`bwoc-agent` is *not* the first match on `$PATH` (dev builds, side-by-side version installs, non-PATH installs), these shell-outs silently invoked a different — often older — binary. Same root cause and proof as #150 (a 2.18 dev build resolving the 2.11 Homebrew install).

## Status / deferred

Shipped on `fix/sibling-binary-resolution`. Deferred: `EVIDENCE_KINDS` dedup + `banner.rs` backend list (next PR).

## Related (links)

- #150 — `bwoc chat --tmux` launches the running bwoc (the originating fix).
- Precedent consolidated here: `spawn::harness_binary` (now a delegate).
