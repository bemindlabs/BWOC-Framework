# 2026-06-08 — Hotfix: clarify fallbackModel is metadata only

`config.manifest.json`'s `fallbackModel` reads like a runtime fallback, but
no code path consumes it as one. It is only displayed (`bwoc status`,
dashboards, `bwoc-agent` liveness) and substituted into `AGENTS.md` as
`{{fallbackModel}}`. The actual runtime model-fallback chain
(`agent_loop.rs`) is fed solely by `primaryModel: "auto"` + the `autoModels`
pool (resolved in `bwoc-harness/main.rs`). So the field misleads — including
the common belief that it works for the Ollama / OpenAI-compatible backends.

## What changed

- `new.rs` — `bwoc new` now prints an advisory when `fallbackModel` is set:
  it is metadata only; for real fallback use `primaryModel: "auto"` + `autoModels`.
  Logic in the pure helper `fallback_metadata_notice()` for testability.
- `help.rs` — the field reference now says "metadata only" and points to
  `autoModels` as the runtime mechanism.
- Test: `fallback_metadata_notice_only_when_set`.

## Decisions

- **Warn, don't wire.** The user (framework architect) chose to keep
  `fallbackModel` as intentional metadata rather than wiring it into the harness
  chain (that would be a behavior-changing feature, not a hotfix). The fix is to
  stop the field from misleading. **Yoniso manasikāra**: verified against the
  actual consumers before acting — the original "warn on vendor backend" framing
  was wrong because the field is unused for *every* backend, not just vendors.

## Status

Hotfix of the `bwoc new` user-error trap set (companions: symlink completeness
#239, memory-scaffold check #246, spawn TTY guard #241). Closes the #3 item;
the earlier "warn only on vendor backends" plan was dropped as inaccurate.
