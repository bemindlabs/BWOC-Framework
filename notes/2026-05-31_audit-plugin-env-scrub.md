# 2026-05-31 — Audit plugins no longer inherit the operator environment

A multi-agent debate (6-lens survey → adversarial proponent/skeptic/judge → synthesis) over "what to build next" surfaced — and the judge ranked first — a real token-exfiltration hole: the audit-plugin spawn ran third-party code with the full operator environment.

## What changed

- **New shared module `crates/bwoc-core/src/env_scrub.rs`** — `scrub_env()` + the `ENV_ALLOWLIST` / `ENV_SENSITIVE_PATTERNS` consts, hoisted out of `bwoc-harness/src/sandbox.rs`. Pure `std`, no new deps. Now the single source of truth for "what env may a less-trusted child see."
- **`bwoc-harness/src/sandbox.rs`** re-exports `scrub_env` (`pub use bwoc_core::env_scrub::scrub_env`) so existing call sites — and `tools::auth`, which builds on `sandbox::scrub_env` — keep their path. Its duplicate env-scrub unit test was dropped (covered in core).
- **`bwoc-cli/src/audit.rs`** — `invoke_plugin` now spawns with `.env_clear().envs(plugin_child_env(...))` instead of inheriting the ambient env. New `plugin_child_env()` returns `scrub_env()` + the three `BWOC_*` context vars. Regression test asserts every non-`BWOC_*` var in the child env is allowlisted.

## Decisions

- **Hoist into `bwoc-core`, do NOT add a `bwoc-harness` dependency to `bwoc-cli`.** The proposal's first instinct was to reuse `sandbox::scrub_env` directly, which would mean `bwoc-cli → bwoc-harness` — breaching the dep-quarantine documented at `spawn.rs:18` and dragging the whole runtime/HTTP/tokio graph into every `bwoc` build. `bwoc-cli` and `bwoc-harness` both already depend on the lean `bwoc-core`, so core is the correct shared home. (The skeptic in the debate caught this; the proponent had it wrong.)
- **Deterministic test, no `std::env::set_var`.** Edition 2024 makes `set_var` `unsafe` and it's racy under parallel tests. Instead of injecting a fake `GITHUB_TOKEN`, the test asserts the *invariant*: every non-`BWOC_*` key in the child env is in `ENV_ALLOWLIST`. A raw-inherited env (the bug) would surface CI vars like `GITHUB_TOKEN` / `GITHUB_ACTIONS`, none allowlisted, and fail — so the test catches the regression regardless of the runner's ambient env.
- **Scope discipline (Mattaññutā).** Excluded the debate's "and ideally" creep — applying the harness `os_sandbox`/`scan_args` to the plugin spawn — as Unix-first/P1 net-new surface. This PR deletes risk and reuses tested code; it adds essentially no new security logic.

## Bugs surfaced and fixed

`audit.rs invoke_plugin` spawned the plugin with `.env("BWOC_*")` adds but **no `.env_clear()`**, so the child inherited `GITHUB_TOKEN`, `AWS_*`, `NPM_TOKEN`, and every other operator secret. Audit plugins are installed from external URLs — the least-trusted code path in the repo was also the least-confined spawn.

## Status / deferred

Shipped on `fix/audit-plugin-env-scrub`. The debate's runner-ups remain open and tracked for later: propagate `--token-budget`/`--cost-limit`/`--vetted-mode` to Saṅgha workers (scalar half only); a provider-client builder timeout; shell-operator-aware guardrail tokenization (scope a threat model first).

## Related (links)

- Origin: `bwoc-next-debate` workflow (29 agents) — the judge's top pick of 7 candidates.
- Precedent reused: `bwoc-harness/src/sandbox.rs` env scrub (now in core), applied at `sandbox.rs:300`.
