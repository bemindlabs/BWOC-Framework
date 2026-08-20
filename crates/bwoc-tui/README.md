# bwoc-tui

The full-screen ratatui chat client behind `bwoc chat --tui` in the [BWOC framework](../../README.md).

A library crate consumed only by [`bwoc-cli`](../bwoc-cli/). It spawns a [`bwoc-harness`](../bwoc-harness/) `--chat` subprocess, parses its stdout into `bwoc_core::chat_proto::ChatEvent`, and writes `ChatInput` lines back to its stdin. Dep-quarantine: it compile-depends only on [`bwoc-core`](../bwoc-core/) (plus ratatui/crossterm/serde) — never on `bwoc-cli` or `bwoc-harness`, which stay runtime subprocesses. No async; the child's stdout is read on a `std::thread` and forwarded over an `mpsc` channel to the draw loop. Colours come from `bwoc_core::design` tokens, mapped to ratatui's *named* colours so the terminal theme keeps authority ([DESIGN.en.md](../../docs/en/DESIGN.en.md)).

## Scope

- **crate root (`lib.rs`)** — single-agent mode: `TuiArgs` + `run()` spawn the harness, run the crossterm event loop, and draw the three-row layout (status line, full-width transcript, input box). `App::apply` folds each `ChatEvent` (`Ready`, `Restored`, `Token`, `Message`, `ToolCall`, `ToolResult`, `PermissionRequest`, `ModeChanged`, `Compacted`, `TurnEnd`, `TeamMessage`, `Error`, `Bye`) into one interleaved transcript.
- **crate root — fleet mode** — `FleetArgs` + `run_fleet()` discover the workspace via `bwoc list --json`, lazily open an `App` pane per agent (plus a live `Session` when that agent's backend is harness-drivable), and drain **every** open session each tick so background agents keep streaming. Includes a `Ctrl-P` command palette (switch pane / forget conversation / quit) and `@agent` message routing between panes.
- **`session`** — `Session` (spawn + reader thread + `send`/`is_alive`, with `Drop` sending `Quit` and reaping the child), `SessionConfig::for_agent` (per-agent model/endpoint from each agent's `config.manifest.json`, path-traversal guarded), `AgentInfo`, `fetch_fleet`, and `is_harness_drivable` (`ollama` / `openai-compatible` / `openrouter` / `litellm`).

Keys: `Enter` sends · `a`/`d` allow/deny a pending permission request · `F2` cycles permission mode (default → accept_edits → bypass) · `PgUp`/`PgDn`/`End` scroll · `Tab`/`Shift-Tab` switch fleet panes · `Ctrl-C`, `Esc`, or `q` on an empty input quits.

## Usage

Not invoked directly — reached through the CLI:

```bash
bwoc chat <agent> --tui                    # single agent
bwoc chat <agent> --tui --fleet            # fleet sidebar, one session per agent
bwoc chat <agent> --tui --team <team>      # join a team's shared chat channel
```

As a workspace dependency:

```toml
[dependencies]
bwoc-tui = { workspace = true }
```

```rust
let code = bwoc_tui::run(bwoc_tui::TuiArgs {
    agent_id: "agent-pi".to_string(),
    agent_path: std::path::PathBuf::from("agents/agent-pi"),
    backend_name: "ollama".to_string(),
    team_chat: None,
});
```

## Status

Working and in use. Both modes ship today; `run()` and `run_fleet()` return a process exit code (`2` when stdout is not a TTY or the harness binary is missing). Pure helpers — argv construction, status line, token formatting, `@mention` parsing, event folding, config overrides — are unit-tested without a terminal.

## License

[MIT](../../LICENSE).
