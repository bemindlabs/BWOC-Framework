# 2026-07-29 — Fix flaky `task_hook` test (ETXTBSY retry)

`sangha::tests::task_hook_missing_is_noop_blocking_is_err` flaked on ubuntu-latest CI, blocking two consecutive PRs (#392, #393) and passing on rerun.

## Root cause
The third assertion writes an executable hook (`#!/bin/sh\nexit 0`, chmod 755) and **immediately** executes it via `run_task_hook`. On Linux under parallel test load this intermittently returns **ETXTBSY ("Text file busy", errno 26)** — the classic write-then-exec race where the writer's `close()` hasn't fully settled when `execve()` runs. `Command::output()` then errors and `is_ok()` fails.

## Fix
`run_task_hook` now retries the exec on ETXTBSY — up to 5 times, 5 ms apart — before giving up. This is a real production hardening too (a concurrent `bwoc` that just installed a hook could hit the same race), not only a test patch; the retry lives in the function the test calls, so no test change is needed. Any non-ETXTBSY error, or ETXTBSY persisting past the retries, still fails.

## Verification
macOS: fmt + clippy clean; the test passes 25/25 under repeated runs (ETXTBSY is rarer on Darwin, so this is a regression check, not the reproduction). Linux (bemind): stressed under high parallelism — see PR body.

## Related
- `crates/bwoc-cli/src/sangha.rs::run_task_hook` · recurring CI flake noted across #392/#393.
