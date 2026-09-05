# 2026-09-05 — TUI terminal restore on panic (RAII guard)

Closes #481. A panic inside any TUI event loop **stranded the operator's
terminal** in raw mode + alternate screen: `restore_terminal()` was called only
at explicit happy-path sites, the existing `Drop` impls only reaped subprocesses,
and there were no panic hooks / `catch_unwind` in these crates.

## What changed

A `TerminalGuard` (an RAII `Drop` that calls `restore_terminal()`), constructed
right after `setup_terminal()` in all three surfaces:

- `bwoc-tui/src/lib.rs` (chat TUI + fleet TUI — two call sites)
- `bwoc-loop-tui/src/lib.rs`
- `bwoc-cli/src/dashboard.rs`

Because BWOC uses **default unwind panics** (no `panic = "abort"` anywhere), the
stack unwinds through `run()` on a panic and the guard's `Drop` runs — so the
terminal is restored with **no signal handler**. The existing explicit
happy-path `restore_terminal()` calls stay (they log a warning on failure); the
guard is the panic-path safety net, and a double restore is harmless.

## Decisions

- Took the invariant from Grok's `xai-crash-handler` — *every teardown path runs
  the same restore* — **without** its 1,771-line signal machinery, which exists
  for `panic=abort` + heavy `unsafe`/FFI that BWOC doesn't have.
- Per-surface guard (~8 lines each) rather than a shared crate, since each crate
  already owns its own `restore_terminal`.

## Status / testing

No unit test: meaningfully exercising it needs a real TTY **and** a panic in the
event loop, which a unit test can't do without corrupting the test terminal. The
guard is a trivial `Drop`; correctness is by construction (unwind runs
destructors).

## Related

- Issue #481; source `research/2026-08-23_grok-build-comparison.md`.
