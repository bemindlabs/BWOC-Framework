# 2026-06-11 — t17: serialize fork-heavy harness integration tests in CI

Phase 5 post-release CI hygiene (tianting t17). The roundtrip-based `bwoc-harness` integration tests fork via `turn_executor::roundtrip()` from a multi-threaded test runner; under parallel threads (12 tests / 6 cores on ubuntu runners) the `pre_exec` fd surgery races against fd-table churn from sibling test threads → intermittent `EBADFD` (err 77). Not a code defect — the executor's fd discipline is correct for its production (non-racing) parent — so the fix is CI determinism, not src changes.

## What changed

- `.github/workflows/ci.yml` build-and-test matrix:
  - `cargo test --workspace` → `--exclude bwoc-harness`
  - new step: `cargo test -p bwoc-harness --lib --bins` (unit tests — no fork helpers, stay parallel)
  - new step: `cargo test -p bwoc-harness --tests -- --test-threads=1` (integration bins serial)
  - `sandbox_escape` (test-redteam) step also gets `-- --test-threads=1` — same roundtrip-based suite

## Decisions

- **CI-side fix, not a serial lock in `roundtrip()`**: a process-wide mutex in src would serialize production turn spawns to cure a test-runner-only race — wrong layer (mattaññutā). Task wording allowed either; chose the one with zero prod impact.
- Verified fork helpers (`run_isolated_selftest`/`run_isolated_forged`) are referenced only from `tests/*.rs`, so harness unit tests keep default parallelism.

## Status / deferred

- cgroup_pids' dedicated t9 jobs already ran `--test-threads=1`; unchanged.
- Remaining tianting Phase-5 residuals t10/t13/t14 need bemind (Linux) access — untouched.

## Related

- tianting task `t17` (claimed by agent-luban)
- Phase 5 release notes: `notes/` Phase-5 series; sandbox escape suite `crates/bwoc-harness/tests/`
