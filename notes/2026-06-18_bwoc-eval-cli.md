# 2026-06-18 — `bwoc eval` CLI (surface the eval framework)

The harness already had a fixture eval framework (`bwoc_harness::eval`:
`Fixture`, `run_fixture`, scored `EvalResult`, ambient-backend skip) with 15
green tests, but **no CLI** — `run_fixture` was only reachable from Rust. The
t31b note flagged "wiring `--backend` through a `bwoc eval` command" as a
separate task. This is it.

## What changed

- **`bwoc-harness --eval <fixture-dir>` mode** (`bwoc-harness/main.rs`) — loads
  `fixture.toml`, builds the provider via the existing `build_provider`
  (so it reuses every backend: ollama / openai-compatible / claude / openrouter /
  cli), runs `run_fixture` in `--workdir`, and prints the `EvalResult`
  (human or `--json`). Runs before the startup banner so `--json` stdout is a
  single clean object. Exit `0` = pass or **skip** (a structurally-unscorable
  fixture is not a failure), `1` = fail.
- **`bwoc eval <fixture>`** (`bwoc-cli/eval.rs`) — a thin front that resolves the
  sibling `bwoc-harness`, forwards `--backend`/`--model`/`--endpoint`/`--json`,
  runs the fixture in a **fresh temp work dir** by default (so a tool-requiring
  fixture's writes don't litter the cwd), and relays the harness exit code.
  Dep-quarantine: the provider/runtime weight stays in `bwoc-harness`.

## Decisions

- **Command lives in the harness, shelled from `bwoc`.** `run_fixture` needs the
  provider + agent-loop machinery, which is the harness's, and `bwoc-cli` does
  not depend on `bwoc-harness` (kept lean). Same pattern as `bwoc spawn` /
  `bwoc chat`. `--backend` is the key the eval framework lacked — now forwarded.
- **Permissive policy for the eval loop** (`default_mode = Allow`). Eval runs in
  an isolated, seeded work dir — a controlled benchmark, not untrusted input —
  so a tool-requiring fixture must be able to write files / run gates to score.
- **Skip ≠ fail in the exit code.** Mirrors `EvalResult::skipped` (the suite
  aggregator drops skips from the denominator), so an ambient backend running a
  tool fixture exits 0, not 1.

## Status / deferred

- Single-fixture only; a suite runner (`bwoc eval <dir-of-fixtures>` → aggregate
  score, excluding skips) is the natural follow-up.

## Related

- `crates/bwoc-harness/src/{main.rs,eval/mod.rs}`, `crates/bwoc-cli/src/eval.rs`;
  notes/2026-06-12_t31b-eval-ambient-backend-guard.md
