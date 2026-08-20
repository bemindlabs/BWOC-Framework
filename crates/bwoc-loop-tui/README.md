# bwoc-loop-tui

The full-screen ratatui control center behind `bwoc loop` — the operator console for Loop-Engineering L1 in the [BWOC framework](../../README.md).

Pick a Saṅgha team, watch its shared task list drive toward Definition-of-Done, start/stop the goal-loop, and edit it in place. Compile-depends only on [`bwoc-core`](../bwoc-core/) (task model + sibling-binary resolution), `ratatui`, and `crossterm` — never on [`bwoc-cli`](../bwoc-cli/) or [`bwoc-harness`](../bwoc-harness/). Both of those are reached as **runtime subprocesses**, which is the dep-quarantine the crate rests on. On Unix it also pulls `libc` for process-group teardown. Workspace resolution lives in the caller (`bwoc-cli`'s `loop_cmd`), so this crate is handed a concrete, existing directory.

## Scope

- **crate root (`lib.rs`)** — `LoopTuiArgs` + `run()`; the `App` state (teams scanned from `.bwoc/teams/*.toml`, tasks read from `<team>/tasks.jsonl`), derived goal status (`Empty` / `Done` / `InProgress` / `Blocked`), the crossterm event loop (200 ms poll, 2 s auto-refresh), the four panes, and `is_safe_team_id` — the path-traversal gate that keeps a team id a single safe path segment before it is joined or crosses an exec boundary.
- **`run`** — the goal-loop subprocess: `LaunchSpec` builds the `bwoc-harness --lead --loop --tasks … --workdir … --loop-interval-secs … --loop-max-iters …` argv (plus optional `--backend` / `--model` / `--endpoint`); `LoopRun` spawns it (on Unix, as its own process-group leader) and captures **both** stdout and stderr into a bounded `LogBuf` via detached reader threads, and tears the whole group down on stop/quit/`Drop` so a mid-flight worker can't orphan and hold the capture pipe open. `parse_outcome` reads the terminal state (`Done` / `Blocked` / `BudgetExhausted`) off the harness's final summary line — the exit code is `0` for all three.

**Panes:** a header (team + goal status), a **Tasks** list with state glyphs, dependency chips and a plan-gate marker, a **Goal** detail pane (state tally, ticker, budget, selected-task detail), a **Loop** pane (run status + live log tail), and a footer that carries key hints, errors, and the modal add-task input.

**Keys:** `↑`/`↓` or `j`/`k` move the selection · `Tab` / `Shift-Tab` cycle teams (refused while a loop runs) · `s` start, `x` stop · `a` add a task (`↵` submit, `Esc` cancel) · `+`/`-` ticker (floored at 1 s) · `]`/`[` budget (floored at 1; `0` = unbounded is reachable only via `--max-iters 0`) · `y`/`n` approve/reject a submitted plan · `r` refresh · `q` / `Esc` / `Ctrl-C` quit, killing any running loop and restoring the terminal.

**Every task-list write goes through the locked CLI path.** Add, approve, and reject shell out to a sibling `bwoc task <verb> --workspace <ws> -- <positionals…>` — never a direct `tasks.jsonl` mutation — so each edit takes the team's `tasks.lock` and serializes with the daemon's auto-claim (`bwoc task claim`) and any other CLI writer. The `--` end-of-options marker is load-bearing: every user-controlled positional follows it, so a leading-dash title or team id can never be misparsed as a flag.

## Usage

```bash
bwoc loop --team squad --interval-secs 5 --max-iters 20 --backend ollama
```

As a library within the workspace:

```toml
[dependencies]
bwoc-loop-tui = { workspace = true }
```

```rust
let code = bwoc_loop_tui::run(bwoc_loop_tui::LoopTuiArgs {
    workspace: std::path::PathBuf::from("/path/to/workspace"),
    team: Some("squad".into()),
    ticker_secs: 5,
    budget_iters: 20,
    backend: None,
    model: None,
    endpoint: None,
});
```

## Status

Implemented and shipping: browse, start/stop, and edit all work against a live harness. Covered by unit tests for goal-status derivation, ticker/budget flooring, the team-id gate, and outcome parsing, plus headless render tests on ratatui's `TestBackend`. See [`LOOP-ENGINEERING.en.md`](../../docs/en/LOOP-ENGINEERING.en.md) for the L1–L3 model this console implements.

## License

[MIT](../../LICENSE).
