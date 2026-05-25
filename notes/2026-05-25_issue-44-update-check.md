# 2026-05-25 — Auto update-check on CLI use (startup drift guard, #44)

Implemented the unbuilt "startup drift guard" from #3: on a normal interactive
invocation, `bwoc` now surfaces a one-line "newer release available" notice —
throttled, non-blocking, silent offline. Reuses `update::check`'s
`fetch_latest_tag` + `CalVer` as-is; adds only a throttle cache, a detached
background refresh, and a guarded hook beside `whats_new::notify_if_updated()`.
Design: agent-oracle. Implementation + tradeoff calls: agent-pi.

## What changed

- `crates/bwoc-cli/src/update.rs` — new "Startup drift guard" section:
  - `notify_if_drifted(is_update_command)` — the one impure orchestrator. Reads
    the cache, prints the cached drift notice this run, and spawns a detached
    refresh only when the throttle window has elapsed.
  - Pure, unit-tested decisions: `should_check(&GuardContext)`,
    `drift_notice(current, latest_seen)`, `throttle_elapsed(last, now, window)`.
  - Throttle cache `~/.bwoc/update-check.json` `{last_checked, latest_seen}`,
    hand-parsed via `serde_json::Value` (matches the file's existing JSON style;
    no serde-derive dep added).
  - `Clock` seam (`SystemClock` + test `MockClock`) so throttle logic is
    testable without sleeping.
  - `run_background_refresh()` / `refresh_cache_at(path, runner, clock)` — the
    detached child's work, split so the core is testable on an explicit path
    (no `$HOME` mutation in tests → no env race with `user_home` tests).
  - 16 new unit tests (CalVer-newer, throttle window incl. clock-skew, all five
    guard skips, cache round-trip/tolerance, refresh online/offline/malformed).
- `crates/bwoc-cli/src/main.rs`:
  - Top of `main()`: short-circuit on `BWOC__UPDATE_REFRESH` → run the refresh
    and exit, before arg parsing or any other hook.
  - Beside `whats_new`: call `update::notify_if_drifted(matches!(... Update ...))`.

## Decisions

- **Detached child, not a thread.** A CLI exits in ms; a refresh thread would be
  killed mid-fetch. Re-exec ourselves with `BWOC__UPDATE_REFRESH=1`, null stdio,
  no wait — the child reparents to init and writes the cache for the *next* run.
  This is the Homebrew/npm pattern: print cached, refresh in the background.
- **Internal env flag, not a hidden subcommand.** Keeps the `Commands` enum
  clean (Mattaññutā — no new user-visible surface) and short-circuits before
  clap even parses.
- **`--json` detected via raw args** (`std::env::args().any(|a| a == "--json")`)
  rather than threading a flag through every subcommand. The notice already goes
  to stderr, but #44 asks to skip `--json` explicitly; this honors it without
  touching N arg structs.
- **Offline advances `last_checked` but never `latest_seen`.** A failed/garbled
  fetch keeps the last known version and bumps the clock, so the 24h throttle
  still holds (no per-invocation spawn storm) and we never fabricate
  "up to date" (Musāvāda). Empty cache + offline → empty `latest_seen` → silent.
- **Released binaries only.** Source builds (`BWOC_RELEASE_CALVER` unset) skip
  entirely — there's no embedded version to compare and it would noise dev work.

## Alternatives considered

- Hidden `bwoc __update-refresh` subcommand — rejected (pollutes `Commands`).
- serde-derive on a cache struct — rejected (new dep; `Value` matches the file).
- Blocking foreground check with a short timeout — rejected (#44: never block).

## Status / deferred

- All gates green: `cargo fmt --all --check`, `cargo clippy --all-targets -D
  warnings`, `cargo test --workspace` (bwoc-cli update module: 46 tests).
- The orchestrator `notify_if_drifted` can't be exercised in dev test builds
  (source-build guard returns early); its pieces are tested individually and the
  refresh core via `refresh_cache_at`. Acceptable — the impure wiring is thin.
- No auto-install, no telemetry beyond the existing GitHub-releases call (#44
  non-goals upheld).

## Related (links)

- Issue #44 (this); #3 (closed — proposed the guard); #8 (`bwoc update` reused).
- Mirrors `whats_new.rs` (sibling startup hook + `~/.bwoc/` state pattern).
