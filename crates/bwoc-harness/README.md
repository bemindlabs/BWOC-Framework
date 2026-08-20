# bwoc-harness

The self-hosted agentic run loop of the [BWOC framework](../../README.md) — provider client, tool set, and a four-layer safety pipeline in one binary.

Nothing in the workspace links this as a library: `bwoc spawn`, `bwoc chat`, and `bwoc eval` locate it as a sibling binary and drive it as a subprocess. That is the dep-quarantine seam — tokio, reqwest, keyring, landlock, and seccompiler live here only, so [`bwoc-cli`](../bwoc-cli/) and [`bwoc-core`](../bwoc-core/) stay lean. Its sole workspace dependency is `bwoc-core`. Linux carries the full kernel-enforced isolation (Landlock, seccomp-bpf, cgroup v2 `pids.max`); macOS degrades to a weaker documented profile. See [`HARNESS.en.md`](../../docs/en/HARNESS.en.md) and [`THREAT-MODEL.en.md`](../../docs/en/THREAT-MODEL.en.md).

## Scope

- **`agent_loop`** — the turn loop: build messages → provider call with bounded retry and a fallback-model chain → run each `tool_call` through the policy pipeline → append results → repeat until the model stops calling tools, `max_iterations`, cancel, or budget.
- **`provider`** — the `ProviderClient` trait over `client` (OpenAI-compatible `/v1/chat/completions`, blocking and SSE; backs `ollama` / `openai-compatible` / `openrouter` / `litellm`), `anthropic` (Messages API translated into the same OpenAI-shaped `types`), and `cli` (subscription-authenticated vendor CLI, one subprocess per turn, no API key).
- **`tools`** — `read_file`, `write_file`, `edit_file`, `list_dir`, `grep`, `run_command`, `git`, `run_gates`, `bwoc_task`, `bwoc_send`, `bwoc_run`, `memory_read`, `memory_write`, plus `computer` behind the `browser` feature. `registry` builds the schema list and dispatches; `auth` is the OS-keyring `CredentialBroker`.
- **`policy`** — the four layers, in order: capability gate (`Capability` grades blast radius; refuses effectful tools on an untrusted turn) → `guardrails` (Sīla 5 + Taṇhā 3, non-overridable) → `permission` (`allow | ask | deny` from `config.manifest.json` and `.bwoc/harness-policy.toml`) → sandbox. `approval` routes `ask` to a file-backed operator console when there is no TTY. Denials return to the model as tool results, never as hard errors.
- **`session_trust`** — the session-monotonic trust latch. `scan_turn_trust` grades a turn by ingress principal (tool and MCP output count as untrusted); `SessionTrust` is set-once, never-clear, and persists into the checkpoint so it survives compaction and reload.
- **`sandbox`, `turn_executor`, `jail`, `seccomp`, `cgroup`** — worktree path confinement, env scrub, and arg scan; then per-turn re-exec of the binary as a one-shot executor child over an inherited `socketpair`, hardened with rlimits, a Landlock filesystem jail, a seccomp-bpf network-egress deny-set, and a per-turn `pids.max` leaf. The parent keeps the keys and the trust latch; the child holds neither.
- **`chat_session`, `lead`, `worker`, `queue`, `review`, `result`** — the multi-turn `chat_proto` session driver; the Saṅgha lead that drains a task list into per-task git worktrees and subprocess workers, with an optional peer-review gate and a `WorkerResult` envelope read back after each worker exits.
- **`checkpoint`, `compact`, `budget`, `telemetry`, `retrospective`, `eval`, `model_select`, `deep_memory`, `mcp`** — durable per-turn run state under `$BWOC_HOME/runs` (else `~/.bwoc/runs`); summarize-first context compaction with a truncate fallback; token/cost budgets; `session-metrics.jsonl` plus a run-end retrospective; the fixture eval runner; `primaryModel: "auto"` resolution against the live provider; tier-2 deep memory; and an MCP **client** (stdio and Streamable HTTP) that registers remote tools as `mcp__<server>__<tool>` so they flow through the same pipeline.

## Usage

A binary, not a library dependency. `bwoc` spawns it, or run it directly:

```bash
# one task, confined to a worktree
bwoc-harness --task "fix the failing test" --workdir . --model qwen3:8b --backend ollama

# resume a checkpointed run (reload + re-attach; no replay)
bwoc-harness --resume <run-id>

# interactive session over the chat_proto JSON-line stream on stdio
bwoc-harness --chat --team-chat .bwoc/teams/core/chat.jsonl

# machine-driven session: same loop, `ask` auto-approves (no human present)
bwoc-harness --headless

# Sangha lead: drain tasks into worktree workers, re-firing until DoD
bwoc-harness --lead --tasks tasks.jsonl --concurrency 4 --loop --loop-max-iters 20

# score one eval fixture (a dir holding `fixture.toml` + optional `seed/` / `expected/`)
bwoc-harness --eval <fixture-dir> --json
```

Optional features: `otel` (OTLP export), `browser` (live `computer` executor), `test-redteam` (the hostile-child escape suite — `cargo test -p bwoc-harness --features test-redteam --test sandbox_escape`). The default build pulls none of them.

## Status

Built and in use — P1 through P5 are complete: loop, tools, safety pipeline, task queue, telemetry, tool auth, eval, and process isolation. The isolation claims are proven by the gate suites in `tests/` (`sandbox_escape`, `egress_pure_read`, `process_isolation`, `resource_limits`, `cgroup_pids`), not asserted; residual gaps are tracked in [`THREAT-MODEL.en.md`](../../docs/en/THREAT-MODEL.en.md).

## License

[MIT](../../LICENSE).
