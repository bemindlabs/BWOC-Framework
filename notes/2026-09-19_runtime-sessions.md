# 2026-09-19 — Several conversations per directory (runtime R4a)

First of three R4 PRs (R4a sessions → R4b undo/redo → R4c protocol Cancel/SetModel/diff/cost). Bare `bwoc` kept one conversation per directory at `~/.bwoc/sessions/<dir-hash>.json`; it now keeps any number in `~/.bwoc/sessions/<dir-hash>/<id>.json`.

## What changed

- New `crates/bwoc-cli/src/coding_session.rs`: `SessionStore` (prepare / list / resolve / find / fork / remove), `list_all`, and the `bwoc session list|fork|rm` subcommand.
- Bare `bwoc` flags `--new` and `--session <id>` (`SessionPick`). Both count as session flags: rejected on subcommands, and need a terminal like `--backend`.
- `runtime::session_file_for` removed; `run_session` resolves the file through the store.
- The harness is untouched: it still gets one `--session-file` and still writes it after each turn.

## Decisions

- **Default stays "resume the latest"** (user's call, 2026-09-19) — 3.2 behaviour, so upgrades feel identical. OpenCode's "new by default, `-c` to continue" was the alternative.
- **Latest = newest mtime**, not newest id, read from metadata only — opening a session never parses every conversation (Copilot review on #532). The harness rewrites the file every turn, so mtime is "last used" with no index to keep in sync.
- **Ids are `YYYYMMDDTHHMMSSZ-xxxx`**: sortable, shell-safe, prefix-matchable (`--session 202609`). The suffix mixes sub-second nanos with the pid.
- **No index file.** Title and message count come from reading each conversation; `dir.json` records the directory (for `--all`), `<id>.meta.json` exists only for forks. Fewer files to drift (Mattaññutā).
- **3.2 migration is lazy and keeps the id**: the old file moves in as `<dir-hash>.json` the first time its directory is opened (`prepare`), so an id copied from `session list` before the move still resolves; a rename that finds the file gone lost a race to another `bwoc` and is not an error (Copilot review on #532). `list` shows it before then, and `list --all` shows unmoved files as "(not opened since 3.2)" since a hash can't be reversed to a path.
- **`bwoc session`, singular**, because `bwoc sessions` already lists running agent processes. Help text on both says which is which.
- `--session` is scoped to the current directory: resuming another directory's conversation here would give the model the wrong repo.
- Vendor backends (`claude`, `codex`, …) refuse `--new` / `--session`: those CLIs keep their own sessions.

## Status / deferred

- In-TUI session switching/forking is R5 (slash commands).
- R4b (undo/redo via a shadow git dir) and R4c (protocol Cancel/SetModel/diff/cost) follow as separate PRs.

## Related

- `notes/2026-09-15_runtime-zero-setup.md` (R1, where the per-directory file came from)
