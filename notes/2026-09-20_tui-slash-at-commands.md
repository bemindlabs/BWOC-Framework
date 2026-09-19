# 2026-09-20 — `/` commands and `@` file mentions in the chat TUI

The chat TUI (bare `bwoc`, `bwoc chat --tui`) gains a Claude-Code-style input line: `/` opens a command menu, `@` completes project files and attaches their content on send.

## What changed

- New `crates/bwoc-tui/src/complete.rs`: pure helpers (`parse_slash`, `slash_matches`, `mention_at`, `list_files`, `filter_files`, `complete`, `expand_mentions`) with unit tests.
- `lib.rs`: `App` gains `workdir` / `files` / popup state; `handle_key` routes arrows/Tab/Enter/Esc to an open popup; `run_slash` executes commands locally; `draw_popup` overlays the menu above the input; footer names `/` and `@`.
- Docs: HARNESS EN/TH paragraph; CHANGELOG `[Unreleased]`.

## Decisions

- **No protocol change.** Commands map onto existing `ChatInput` (`Forget`, `SetMode`, `Quit`); `/new`, `/session`, `/compact` wait for R4c/R5 protocol work.
- **`@` inlines content** (operator's call), capped at 32 KB per file, text only, canonicalized and confined to the workdir (symlink / `..` escapes are refused). Sent as `LocalOperator`, so trust gating is unchanged.
- **Prose starting with a path** (`/etc/hosts …`) is a message, not an unknown command.
- **Fleet panes opt out** (`workdir = None`) so fleet `@agent` routing is untouched.
- File walk is std-only (no `ignore` crate), skips `.git`/`target`/`node_modules`/dot-dirs, capped at 5,000 files, listed once on the first `@`.

## Status / deferred

- `/clear` deletes the conversation (harness `Forget` semantics); a keep-and-start-fresh `/new` needs the TUI to respawn the harness — R5.
- `.gitignore` is not honored by the file walk.
