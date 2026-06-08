# 2026-06-09 — Serialize the process-isolation test suite

`crates/bwoc-harness/tests/process_isolation.rs` flaked on `ubuntu-latest` CI: a *different* subset of its 12 tests failed on each run (observed across three reruns: `c12` → `c5` → `c2_token_scrubbed` + `c8`). Each test passes in isolation and locally. Root cause is parallel execution: every test spawns the real `bwoc-harness` turn-executor child and exercises process-wide sandbox machinery (the one-time capability token, per-turn `setrlimit`/cgroup, the IPC fd, env scrubbing, PID reaping); run concurrently under cargo's default thread pool they contend on a constrained Linux runner and flake.

## What changed

- Added a file-level `static SERIAL: Mutex<()>` and a `serial()` helper; every `#[test]` in the file takes `let _serial = serial();` as its first line, so the suite runs one test at a time. The lock is **poison-tolerant** (`unwrap_or_else(|p| p.into_inner())`) so a panicking test cannot cascade-fail the rest.

## Decisions

- **Dependency-free `Mutex`, not the `serial_test` crate.** Keeps the harness dev-dependencies lean (Mattaññutā) and avoids a proc-macro dep for a one-file need. `Mutex::new(())` is `const`, so the static needs no lazy init.
- **Serialize the whole file, not a hand-picked subset.** The contention is on shared OS/process resources that any of these tests can touch; a partial lock would just move the flake. Wall-clock cost is ~1s for the suite — negligible.

## Bugs surfaced and fixed

- The flake masqueraded as a regression on PR #262 (`RouteTarget::Gateway`), which is unrelated — it touches no harness code. `c2_token_scrubbed_before_grandchild` was previously "stabilized" in v2.27.0; this shows that fix was incomplete and the cause was suite-wide parallelism, not that one test.

## Status / deferred

- Suite now passes 12/12 across repeated local runs. Unblocks the `ubuntu-latest` required check for downstream PRs once merged.

## Related

- PR #262 — `RouteTarget::Gateway` (the PR the flake was blocking).
- `crates/bwoc-harness/src/turn_executor.rs` — the production isolation path these tests drive.
