# 2026-09-20 — Runtime R4b: undo/redo of a turn's file changes

The last R4 item. A session can now take back what a turn wrote.

## What changed
- New `bwoc-harness::undo` — a per-conversation **edit journal** at `<session-file>.undo/<turn>.json` plus a `cursor` file, with `record` / `undo` / `redo`.
- `chat_session`: each turn collects an `undo::Turn` as file-mutating tools run (reusing the read the `Diff` event already takes), and records it at turn end; `Undo` / `Redo` inputs are handled between turns and answered with `Reverted { undo, restored, conflicts, skipped }`.
- `bwoc-tui`: `/undo`, `/redo`, and a transcript report of what was restored, what conflicted and what was never journalled.

## Decisions
- **Edit journal, not a shadow git dir** (operator's call). It costs what the agent touched instead of the whole worktree, and it never puts a repository the operator owns under a second VCS. The trade is that a change made by `run_command` is outside it — which is exactly what the conflict check surfaces.
- **Conflicts are refusals.** Undo restores a file only when what is on disk is the state the step left; otherwise the file is reported and untouched. An undo that overwrites someone else's edit would be a data-loss bug wearing a convenience feature's clothes.
- **Text only, 1 MB per side.** A binary or oversized write is recorded by name as `skipped`, so an undo says what it cannot restore instead of appearing complete.
- **A file touched twice in a turn keeps its original `before`**, so one undo takes the whole turn back.
- **Recording after an undo drops the redo tail** — the timeline branched, exactly as an editor's undo stack does.

## Status / deferred
- Undo covers file tools only; `run_command` side effects are not journalled (and make a later undo report a conflict rather than clobber).
- The journal is never pruned; a long conversation keeps every turn it changed files in.
