---
title: Phase 6 t31b — backend-parametrized eval + ambient-backend guard
date: 2026-06-12
tags:
  - type/note
  - area/harness
  - area/eval
  - phase/6
---

# Phase 6 t31b — eval skips tool-fixtures on an ambient backend

## What changed

`crates/bwoc-harness/src/eval/mod.rs::run_fixture` gained a `backend: &str`
parameter and an **ambient guard**, composing with t30's backend-trust split:

- New `fixture_requires_tools(&Fixture) -> bool` — true when the rubric has any
  `file_contains` / `file_matches` / `gates_must_pass` (the agent must *act
  through tools* to satisfy it, not merely chat).
- `run_fixture` short-circuits **before running the loop** when
  `backend_trust_tier(backend).is_ambient()` (i.e. `cli`) **and** the fixture
  requires tools. It returns an `EvalResult { skipped: true, skip_reason: Some(…) }`
  instead of running a chat-only model that would leave the work dir untouched
  and score a structural 0.
- `EvalResult` gained `skipped: bool` + `skip_reason: Option<String>` (both
  `#[serde(default)]`, so older JSON still deserializes). A suite aggregator MUST
  drop skipped fixtures from the score denominator rather than count them as 0.

## Why (the t30 linkage)

t30 established that the `cli` backend is **ambient / chat-only**: the vendor CLI
runs its own tools out of harness reach, so the harness loop never sees
`tool_calls`. An eval fixture that needs a file written would therefore *always*
score 0 on `cli` — a **structural** failure, not a model-quality one. Recording
that as a 0 would poison the suite score and read as "this model is bad" when the
truth is "this backend can't run this kind of fixture." Skipping is the honest
signal.

## Decisions / alternatives

- **`backend: &str`, not `BackendTrust`.** The runner derives the tier itself
  (`backend_trust_tier`) so the skip reason can name the backend, and so a future
  `bwoc eval --backend X` plumbs one string end-to-end. (There is no eval CLI
  entry today — `run_fixture` has only test callers — so this is a library-level
  change; the CLI surface is a separate follow-up.)
- **Skip, not fail.** A skipped fixture is neither pass nor fail. Marking it
  `skipped` (vs. reusing `passed=false`) keeps the suite math honest.
- **Guard on the rubric, not a fixture flag.** `fixture_requires_tools` is
  derived from the existing rubric shape — no new `task.toml` field to set or
  drift. A pure-chat fixture (no rubric) still runs on `cli`.

## Status / deferred

- No eval CLI subcommand exists yet; wiring `--backend` through a `bwoc eval`
  command is a separate task if/when the runner is surfaced.
- Suite-level aggregation that excludes `skipped` from the denominator is the
  caller's responsibility (documented on `EvalResult::skipped`).

## Proof

`fixture_requires_tools_detects_rubric_kinds`, `ambient_backend_skips_tool_fixture`
(empty mock proves the loop never ran), `ambient_backend_runs_chat_only_fixture`.
`cargo test -p bwoc-harness --lib eval` 15 passed; workspace fmt/clippy/test green.

## Related

- `notes/2026-06-12_t30-cli-backend-trust-tier.md` — the backend-trust split this builds on.
- `crates/bwoc-core/src/trust.rs` — `backend_trust_tier` / `BackendTrust`.
