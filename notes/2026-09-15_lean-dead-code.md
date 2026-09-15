# 2026-09-15 — Lean pass: remove dead code and the unused `browser` feature

Removed code with no production caller. Before deleting anything, the whole repo was grepped: crates, tests, docs, CI, scripts and Formula. The Rust API is not a public surface (`COMPATIBILITY.en.md`), so this fits a minor release. CLI behaviour is unchanged.

## What changed

- **bwoc-core**: deleted the empty `identity` / `error` stub modules and `lifecycle` (`LifecyclePhase` had no users). Removed `TrustBlock::missing_in`, which only its own tests called. `team::render_chat` is now a test-module fixture.
- **bwoc-cli**: removed `git_worktree::{worktree_add, worktree_path, worktree_branch}`. retire keeps using list/remove/branch helpers. Removed `resource::Caps::max_leases`, a `workspace.toml` key that was parsed but never read. The key is still accepted, and it stays in `RESOURCE-PROTOCOL` for broker slice B.
- **bwoc-agent**: removed `i18n::t` and its self-test.
- **bwoc-harness**: removed `ToolQueue::in_flight_count`. Removed the `browser` feature, `tools/browser.rs`, the `default_registry` hook, the `#[ignore]` live test and the optional `chromiumoxide` dep. No workflow, script or Formula enabled the feature. `DEFAULT_VIEWPORT` moved to `tools::computer`, because the Anthropic provider uses it in every build.
- **bwoc-tui**: removed `FleetSnapshot.workspace`, which was never read.

## Decisions

- **Kept `audit.rs` `PluginSection.description`**: it looks unused to rustc, but it is a required serde field. Removing it would let `bwoc audit` accept a manifest without `description`, which `plugin.rs` rejects. That counts as a CLI behaviour change.
- The pure action→CDP mapping (`cdp_plan`) went with `browser.rs`: its only consumer was the gated live executor. `computer` (the action model, `ComputerExecutor` seam, Anthropic tool spec) stays; a host can still supply an executor.
- `otel` untouched (separate decision).

## Status / deferred

- `TrustDeclared::has` is now used only by its own tests. Out of scope for this pass; a candidate for the next one.
