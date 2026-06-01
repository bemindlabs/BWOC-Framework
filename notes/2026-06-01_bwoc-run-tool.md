# 2026-06-01 — `bwoc_run` tool: "ollama launches bwoc"

The model driving a `bwoc-harness` loop (ollama / openai-compatible backends) can now launch *another* BWOC agent on a subtask and read back its result — delegation at the tool layer.

## What changed

- **New tool `BwocRun` (`bwoc_run`)** in `crates/bwoc-harness/src/tools/extra_tools.rs`, registered in `default_registry()`. Mirrors the existing `bwoc_task` / `bwoc_send` tools: it shells out to `bwoc run <agent> --task <task> --json --timeout <n>` (sibling-resolved `bwoc` via `bwoc_core::exec`), `current_dir = ctx.workdir`, minimal scrubbed env, and returns stdout (+ stderr / exit code on failure).
- Schema: `agent` (required), `task` (required), `timeout_secs` (optional, default 300). Covered by `all_new_tools_have_schemas`; new `bwoc_run_requires_agent_and_task` test pins the arg validation.

## Decisions

- **Direction = "ollama is the launcher."** Disambiguated with the user: the ollama model *drives* bwoc (model → CLI), not a convenience launcher for ollama agents. The missing verb was "launch another agent" — `bwoc_task` (claim work) and `bwoc_send` (message) already existed; `bwoc run` (delegate a task) did not.
- **Safety = lean on the existing permission layer, don't invent a new guard.** Every harness tool is permission-gated and the policy's `default_mode` is fail-safe **deny** (`permission.rs:112/169`), so `bwoc_run` is denied unless an operator allowlists it in `.bwoc/harness-policy.toml`. That gate *also* bounds recursion (a launched agent can re-launch only if its own policy allows `bwoc_run`), so no custom depth/env counter was added (Mattaññutā — the framework already solves this). Each launch is additionally time-bounded (`timeout_secs`, default 300) so a delegate can't hang the caller.
- **Shell out, don't re-implement.** Same rationale as `bwoc_task`/`bwoc_send`: agent resolution, backend selection, and the run lifecycle live in `bwoc-cli`; duplicating them in the harness would drift.

## How to enable (operator)

In the launching agent's `.bwoc/harness-policy.toml`:

```toml
[tools]
bwoc_run = "allow"
```

Then the model can call `bwoc_run { "agent": "specialist", "task": "…" }`.

## Status / deferred

Shipped on `feat/bwoc-run-tool`. Not added (would be scope creep): a launch-depth env counter (the permission gate already bounds recursion), and streaming/partial results from the delegate (the run is captured headless).

## Related (links)

- Precedent mirrored: `bwoc_task` / `bwoc_send` in the same file.
- Sibling-binary resolution used for the spawn: `bwoc_core::exec::binary_or_name` (BWOC-15).
