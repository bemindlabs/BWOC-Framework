# 2026-10-08 — herdr as an optional agent-state source and pane backend

`bwoc sessions` could only guess `working`/`idle` from tmux window activity, and tmux was hard-coded wherever BWOC opens or wakes a pane. This change adds [herdr](https://herdr.dev) (v0.9.3) as an opt-in source of agent state (`blocked`, `done`) and as a second pane backend for `bwoc fleet term` and `bwoc send` wakeups. Both are off by default; with no new keys, behaviour is unchanged. Design and decisions: `research/2026-10-08_herdr-integration.md`.

## What changed
- **Phase 1 — state.** `crates/bwoc-cli/src/herdr.rs` is a unix-socket client (newline JSON, one shared ~300 ms budget per command, fail-fast after the first failure, no-op off unix). `sessions.rs` gains `SessionState::{Blocked, Done}` and a `stateSource` JSON field. Agents are matched by `foreground_cwd` inside `agents/<id>/`, else by marker pid in `pane.process_info`. Enabled by `[integrations.herdr] enabled = true` (`bwoc-core` `Integrations::load`). The dashboard renders the new states.
- **Phase 2a — trait.** `pane_backend.rs`: `PaneBackend` + `TmuxBackend`, a byte-identical move of the tmux builders and their tests. `fleet_term.rs` and `send.rs` route through it.
- **Phase 2b — herdr backend.** `herdr_backend.rs`: `workspace.create` + `layout.apply` (each pane runs the same `bwoc spawn --path … --backend …` as tmux), `pane.list` to locate, `pane.send_text` + Enter to wake. `bwoc fleet term --backend tmux|herdr` overrides `[fleet] pane_backend`. `TmuxLayout` is renamed `PaneLayout`. `bwoc supervise` warns when the workspace selects herdr.
- **Spec.** `docs/{en,th}/FLEET-GOVERNANCE` §2 states the one-owner rule for agent processes.

## Decisions
- No new crate dependencies; `std::os::unix::net` instead of `interprocess`.
- herdr can never fail a `bwoc` command: any error, timeout or `protocol_mismatch` means "unavailable" and the old path runs.
- `agent.start` is not used: it only accepts herdr's built-in agent kinds, which would exclude ollama agents.
- `fleet status` is unchanged: its `online` column is pid liveness, not a session state.

## Alternatives considered
- Replacing the in-process TUI panes (`/agents`, `/layout`) with herdr — rejected; herdr's protocol is pre-1.0 (v5→v14 in six months).
- A herdr-side plugin — deferred until the backend has been used against a live herdr.

## Status / deferred
- **Not run against a live herdr.** All herdr tests use a fake socket server. Attach, `layout.apply` tab replacement, `send_text` + Enter in each agent TUI, and the 5 s `fleet term` budget are unverified.
- Windows builds are expected to compile (herdr code is `#[cfg(unix)]`) but were checked only by CI.
- Deferred: herdr `remain-on-exit` equivalent; `pane report-agent` for ollama agents; routing `chat --tmux`, `dashboard.rs` and `bwoc-agent` `task_watch.rs` through the trait.
