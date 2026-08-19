# 2026-08-18 — Loop-Engineering control-center TUI (PR1: observe)

A new operator console for **Loop-Engineering L1** (the Saṅgha goal-loop),
opencode-style and focused on driving a team's task list to Definition-of-Done.
This is the first of three slices; PR1 is the **observe-only** foundation.

## What changed

- **New crate `crates/bwoc-loop-tui`** (pure lib, deps `bwoc-core` + `ratatui` +
  `crossterm`) — mirrors `bwoc-tui`'s dep-quarantine: it depends only on
  `bwoc-core` (the `team` model), never on `bwoc-cli`/`bwoc-harness`. The harness
  that drives a goal-loop is a *runtime* subprocess (added in PR2), not a build
  dep, so the `bwoc` side stays free of the harness runtime graph.
- **`bwoc loop` subcommand** (`crates/bwoc-cli/src/loop_cmd.rs`) — resolves the
  workspace (the standard `--workspace` > `BWOC_WORKSPACE` > ancestor-walk chain,
  same as `dashboard`) and hands a concrete dir to `bwoc_loop_tui::run`. Flags:
  `--team`, `--interval-secs` (default 5), `--max-iters` (default 20).
- Registered the crate in the workspace `members` + dep table; wired the CLI dep,
  `mod loop_cmd`, the `Loop` command, and its dispatch.

## The screen (PR1)

Header status bar (title · team · goal status), a two-column body — task list
(state icon + id + title + deps + plan-gate lock, colour-coded) on the left, a
Goal detail pane (state counts, ticker/budget, selected-task detail) on the
right — and a keybind footer. `↑/↓`/`j`/`k` move the task selection, `Tab`/
`Shift-Tab` cycle teams, `r` refreshes, `q`/`Esc`/`Ctrl-C` quit. Auto-refreshes
on a 2s tick.

Goal status is derived purely from task states + deps: `Empty` / `Done` (all
completed) / `In Progress` (something claimable or running) / `Blocked` (only
dependency-blocked pending remain) — the same shape the harness goal-loop
reports at runtime, surfaced statically.

## Decisions

- **Separate crate, not a `bwoc-cli` module** (chosen with the architect) —
  keeps the ratatui surface isolated and the dep-quarantine intact, exactly as
  `bwoc-tui` does for `chat --tui`.
- **Observe-only first.** No spawn, no writes — the reviewable foundation
  everything else builds on. PR2 = start/stop the loop (`bwoc-harness --lead
  --loop` subprocess) + a live iteration-log pane; PR3 = edit (add task, adjust
  ticker/budget, approve plan) via the locked `bwoc task …` CLI path (no
  duplicated lock).
- **8 unit tests** including a headless `TestBackend` render smoke test (proves
  `draw()` lays out without a real TTY, so it runs in CI).

## Status / deferred

- Spec cross-reference in `LOOP-ENGINEERING.{en,th}.md` and a handbook entry are
  deferred to the feature-complete slice (PR3) — documenting a half-built surface
  invites drift. clap `--help` covers discovery meanwhile.

## Related

- Loop-Engineering spec: `docs/en/LOOP-ENGINEERING.en.md` (#436).
- L1 goal-loop runtime: `crates/bwoc-harness/src/lead.rs` (#437/#438).
- Precedent crate: `crates/bwoc-tui` (chat `--tui`).
