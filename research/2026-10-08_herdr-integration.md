# 2026-10-08 — herdr as an optional pane + agent-state provider for the fleet

## Sources (Sutamayā)

- https://herdr.dev/ · https://github.com/herdrdev/herdr (Apache-2.0, Rust) — read at **v0.9.3** (2026-09-29): `docs/versions/0.9.3/website/src/content/docs/{concepts,socket-api,cli-reference,agents,add-herdr-support,plugins,session-state}.mdx`, `src/api/server.rs`
- BWOC source at `upstream/main` `25ea6b9` (3.13.0): `crates/bwoc-cli/src/{sessions,fleet_term,send,fleet}.rs`
- Not read: herdr.dev/plugins (JS-rendered); herdr was **not** installed or run — every herdr behaviour below is from docs/source, not observed.

## TL;DR

herdr is a background server that owns terminal panes and classifies the agent in each pane as `working | blocked | done | idle | unknown`. BWOC has **no `blocked` state today** — `sessions.rs:34-35` infers `working`/`idle` from tmux `#{window_activity}` age, and the `BackendDetector` extension point is a TODO (`sessions.rs:46-48, 80-81`). That gap is the cheapest, highest-value thing herdr fills.

Adopt in two phases, both **opt-in, off by default, no new crate deps**:

1. **Phase 1 — read-only state provider.** When enabled and a herdr socket answers, `bwoc sessions` (and the `bwoc dashboard` agents pane, which reads the same sessions) take each matched agent's state from herdr. `bwoc fleet status` is unchanged — its `online` column is a pid-liveness check, not a session state. No process-ownership change.
2. **Phase 2 — `PaneBackend` adapter.** Extract the tmux calls in `fleet_term.rs` / `send.rs` / `chat.rs` behind a trait; add a herdr impl so `bwoc fleet term --backend herdr` lays the fleet out with `layout.apply` and `bwoc send` wakes via `agent.prompt` instead of `tmux send-keys`.

Explicitly **not** adopted: replacing the in-process TUI panes (`/agents`, `/layout`, 3.11) with herdr, and a herdr-side plugin (deferred; revisit after Phase 2).

## What herdr gives (verified from docs/source)

| Need | herdr surface |
|---|---|
| Transport | Newline-delimited JSON over a unix socket, mode `0o600` (`server.rs` `SOCKET_PERMISSION_MODE`). Path: `--session` > `HERDR_SOCKET_PATH` > `HERDR_SESSION` > `~/.config/herdr/herdr.sock`. Request `{"id","method","params"}` → `{"id","result"}` or `{"id","error":{code,message}}`. Not JSON-RPC 2.0. |
| Agent state | `agent.list` / `pane.list` objects carry `agent_status` and `foreground_cwd`; `pane.process_info` returns foreground pids + cwd. Events: `pane.agent_status_changed`. |
| Layout | `layout.apply` takes a declarative split tree with per-pane `cwd`, `env`, argv `command`. |
| Prompting | `agent.prompt <target> <text> --wait --until done\|blocked`; `pane.send_text`. |
| Pane env | Every pane gets `HERDR_SOCKET_PATH`, `HERDR_ENV=1`, `HERDR_WORKSPACE_ID/TAB_ID/PANE_ID`. |
| Unknown agents | `pane report-agent $HERDR_PANE_ID --source … --agent … --state … --seq N` — needs no herdr change. |

## Constraints that shape the design

1. **Process ownership.** The herdr server is the PTY parent of every pane process; `herdr server stop` kills them, and restart restores only layout (session-state.mdx). ⇒ Phase 1 never launches anything through herdr. Phase 2 agents launched in herdr panes are **not** also run under `bwoc supervise` — one owner per process.
2. **Pre-1.0 protocol.** Client/server protocol bumped v5→v14 in six months; 0.9.2 removed `pane.graphics.*` with no replacement; no deprecation window is promised. Docs do promise "clients should ignore unknown fields and handle unsupported methods as normal errors", and mismatches return `protocol_mismatch`. ⇒ BWOC deserialises leniently, treats **any** error/timeout as "herdr unavailable", and falls back to the existing heuristic. herdr must never be able to fail a `bwoc` command.
3. **Coverage.** herdr detects 23 agent CLIs; `ollama` is not one. ⇒ Ollama-backed agents keep the tmux heuristic (Phase 1) or self-report via `pane report-agent` (Phase 2, optional).
4. **Global config side-effects.** `herdr integration install claude` edits `~/.claude/settings.json`. ⇒ BWOC never runs it; it is an operator choice, documented only.
5. **Install.** Release binary (`herdr-macos-aarch64` etc.) is the supported path for BWOC docs. Whether the homebrew-core bottle covers every macOS we run on is **unverified**; a bottle miss compiles from source.

## Phase 1 — read-only state provider

**Opt-in.** `[integrations.herdr] enabled = true` in `.bwoc/workspace.toml`. Optional `socket = "<path>"`; otherwise the herdr resolution order above.

**Probe.** One `agent.list` round-trip per `bwoc sessions` call, one shared ~300 ms budget for all herdr calls in a command (fail-fast after the first failure), over `std::os::unix::net::UnixStream` — no new dep. Any failure → provider returns nothing, silently (`bwoc sessions` has no `--verbose` flag).

**Matching a herdr agent to a BWOC agent**, first hit wins:
1. `foreground_cwd` is `agents/<id>/` (or inside it) of this workspace — `bwoc spawn`/`chat` run there.
2. Else, a marker pid (`.bwoc/sessions/<id>.json`) appears in `pane.process_info` foreground pids — costs one call per unmatched pane, so only for marker-holding agents.
3. Else unmatched: herdr agents in other dirs are ignored (not BWOC's).

**State precedence.** Dead pid → `stale` (unchanged, BWOC's own liveness check wins). Otherwise a matched herdr status overrides the tmux heuristic. `SessionState` gains `Blocked` and `Done`; `unknown` maps to the existing `Running`.

**Output.** `--json` gains `"state": "blocked"|"done"` values and a `"stateSource": "herdr"|"tmux"|"marker"|null` field (`null` when no activity signal was used — stale and scan-only sessions); table output unchanged except the new values. Additive — no existing field changes meaning.

**Tests.** A fake socket server in-test (no herdr binary in CI): happy path, timeout, `protocol_mismatch`, unknown fields, unmatched cwd.

**Out of scope for Phase 1:** event subscription (one-shot reads suffice for a CLI), `fleet health` signal changes (the seven Aparihāniya signals stay as specified in FLEET-GOVERNANCE).

## Phase 2 — `PaneBackend` adapter

Today tmux is hard-coded across `fleet_term.rs`, `chat.rs`, `spawn.rs`, `send.rs`, `sessions.rs`. As built, `fleet_term.rs` and `send.rs` go through `PaneBackend` (`pane_backend.rs`, herdr impl in `herdr_backend.rs`); `chat --tmux`, `spawn.rs`'s marker pane, `dashboard.rs` and `bwoc-agent`'s `task_watch.rs` stay tmux-only.

| Operation | tmux (existing) | herdr |
|---|---|---|
| open fleet layout | `tmux_fleet_commands` + `select-layout` | `workspace.create` (label = fleet session name) + `layout.apply` replacing its first tab; split tree from `PaneLayout` (renamed from `TmuxLayout`), each pane `label` = agent id, `cwd` = agent dir, argv = the same `bwoc spawn --path <dir> --backend <b>` tmux runs. **Not** `agent.start` — it only accepts herdr's built-in kinds, so ollama agents would fail. |
| locate an agent's pane | session-name candidates / pane title | `pane.list` → pane whose `label` is the agent id, else whose `foreground_cwd`/`cwd` is inside the agent dir (Phase 1 matcher) |
| wake / deliver | `tmux send-keys -l` | `pane.send_text`, ~200 ms, `pane.send_keys ["enter"]` — best-effort |
| last activity | `#{window_activity}` | `agent_status` (Phase 1 provider) |

Selection: `bwoc fleet term --backend tmux|herdr` overrides `[fleet] pane_backend` (default `tmux`; an unknown value warns once and falls back to tmux). `bwoc send` wakes the recipient through **the recipient workspace's** `pane_backend`; `BWOC_DISABLE_TMUX_WAKEUP` disables both backends' wakeup. `[integrations.herdr] enabled` gates only the Phase 1 state reader; its `socket` is reused by the pane backend. `fleet term` socket calls share a 5 s budget because `layout.apply` starts every agent before replying — an unmeasured estimate.

**Deferred:** a herdr equivalent of tmux `remain-on-exit`; `pane report-agent` self-reporting for agents herdr cannot detect (ollama). Whether such panes appear in `agent.list` at all is unverified; if they do with `unknown`, Phase 1 maps that to `running`, which overrides the activity heuristic; routing `chat --tmux` / dashboard / task-watch through the trait.

**Unverified against a live herdr:** attach via the plain `herdr` client, `layout.apply` replacing the tab, and whether `send_text` + Enter submits in every agent TUI. All herdr tests use a fake socket server.

**Supervision.** Under `--backend herdr`, `bwoc supervise` is not used for those agents; herdr's restore + `resume_argv` is the restart story. `bwoc supervise` prints a one-line warning when the workspace's `pane_backend` is `herdr`; FLEET-GOVERNANCE §2 states the rule.

## Decisions (2026-10-08, operator approved "implement all phases")

1. **Workspace keys.** Phase 1: `[integrations.herdr]` with `enabled` (bool, default `false`) and optional `socket` (path). Phase 2: `[fleet] pane_backend = "tmux" | "herdr"` (default `"tmux"`), overridden per call by `bwoc fleet term --backend`. Absent keys = today's behaviour.
2. **`done` stays a distinct state.** It is herdr's "finished, unread" — kept so the operator can see who is waiting on them. Fleet-health signals do not read it.
3. **No version pin.** Lenient parsing (`#[serde(default)]`, unknown fields ignored); any error, timeout or `protocol_mismatch` = "herdr unavailable" → fallback.
4. **Spec placement.** After both phases land: one section in `docs/en/FLEET-GOVERNANCE.en.md` + `docs/th/FLEET-GOVERNANCE.th.md`, not a new doc.
5. **Windows.** herdr uses a named pipe there; Phase 1/2 herdr code is `#[cfg(unix)]` and the provider is a no-op elsewhere.

## Routing

| Piece | Owner |
|---|---|
| This design + EN/TH spec section | agent-jisoo |
| Workspace key, `--json` shape, `--backend` flag, help text | agent-jennie |
| `SessionState` variants, herdr client, matcher, `PaneBackend` trait | agent-lisa |
| Fake-socket test harness, cross-platform CI (Windows has no unix socket — provider compiles to a no-op) | agent-rose |
