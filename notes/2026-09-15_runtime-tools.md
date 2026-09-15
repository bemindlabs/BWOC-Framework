# 2026-09-15 — Runtime R3: coding-agent tools

Phase R3 of "BWOC as its own coding agent". `bwoc-harness` gains the tools a coding session expects: `glob`, a regex `grep`, `edit_file` `replace_all` plus `multi_edit`, a `run_command` timeout, and three chat-only tools, `webfetch`, `todo` and `subagent`. Built on `main` after R0/R1, in parallel with R2 (chat parity).

## What changed

- **`tools/extra_tools.rs`**: a shared confined walker (`walk_files`) now backs `grep` and the new `glob`. Globs compile to anchored regexes (`glob_to_regex`). `grep` matches a regex, with `fixed_strings`, a `glob` file filter and a NUL-byte binary sniff, and accepts a file as `path` (it used to return no matches). `edit_file` gains `replace_all`. The new `multi_edit` shares `apply_one` with it.
- **`tools/impls.rs`**: `run_command` spawns into its own process group with a null stdin and `kill_on_drop`, waits under `timeout_secs` (default 120, max 600), and `killpg`s the group on expiry.
- **`tools/webfetch.rs`** (new) and **`tools/session.rs`** (new, `todo` and `subagent`), registered in `run_chat_mode` only.
- **Policy lists**: `glob` joins `PURE_READ_TOOLS` (with egress fixtures), plan mode and the chat allow list. `todo` and `subagent` join the allow list only. `todo` left plan mode at the R2 merge: every plan-mode tool must pass the capability gate on an untrusted turn, which only `PURE_READ_TOOLS` do, and that list is executor-proven. `multi_edit` is `WorktreeWrite`, is auto-approved in `accept_edits`, and the Adinnādāna secret scan covers every `edits[].new_string`. `webfetch` stays `ask` / `Gated`.
- **`regex`** is now a harness dependency. It was already a workspace dependency, so the lockfile gains no crate.
- `prompts/coding_agent.md`, HARNESS EN/TH tool tables (TH also gains the missing `memory_search` row), crate README, CHANGELOG.

## Decisions

- **Chat-only tools run in-process.** `todo` and `subagent` hold session state, and `webfetch` opens a socket, which the turn-executor child's seccomp filter answers with KILL. None of the three is in `default_registry`, so `is_marshallable_tool` is false for them.
- **Plan mode excludes `webfetch` and `subagent`.** bwoc-connect public sessions run in `plan` (`READ_ONLY_MODE`). Egress and model-call fan-out are not for strangers, even though both are read-only locally.
- **`grep` compatibility.** Regex by default, but an invalid pattern searches literally and adds a note, so a substring caller (`foo(`) keeps working. Valid patterns with metacharacters (`a.b`) now match a superset of before, which is harmless for search.
- **No `.gitignore`.** No ignore-matcher dependency is in the tree, so hidden directories are skipped (as `grep` already did) and callers narrow with `path` / `glob`.
- **`replace_all` is exact-only.** The whitespace-tolerant fallback stays for single edits; a bulk rewrite must not guess.
- **`multi_edit` is all or nothing in memory**, with one plain write. It does not use temp + rename, the same as `edit_file`.
- **`webfetch` refuses local targets** (localhost, loopback, private, link-local, CGNAT, and IPv4-mapped IPv6 forms), including on redirect, because headless sessions auto-approve `ask`. DNS names that resolve to private addresses are not caught; the approval prompt covers interactive use.
- **`subagent` is minimal.** It uses `complete` (no streaming), `read_file` / `list_dir` / `grep` / `glob`, the parent's guardrail check and confined `ToolContext`, and at most 15 provider calls. Depth 1 holds structurally, because the child registry has no `subagent`.
- **`run_command` stdin is explicitly null.** `output()` used to null it implicitly. `spawn` would inherit it, and in chat mode stdin is the protocol stream.

## Alternatives considered

- **`apply_patch`.** Skipped (Mattaññutā). A patch grammar the model must produce exactly, plus a parser and fuzz matching, would duplicate what `multi_edit` already does against exact text.
- **Adding `ignore` / `globset`.** Rejected. It adds new crates to the heaviest crate for a convenience the `path` argument already gives.
- **Routing `todo` / `subagent` inside `chat_session::dispatch_call`.** Rejected, to keep R2's `chat_session.rs` changes mergeable. Plain `ToolImpl`s registered in `main.rs` need only a one-line allow-list edit there.

## Bugs surfaced and fixed

- `grep` with a file `path` walked it as a directory and always reported no matches.

## Status / deferred

- **R2 merge.** If R2 routes chat tool calls through the turn executor, `webfetch`, `todo` and `subagent` must stay on the in-process path, or they will be denied as un-marshallable. For `webfetch` a failure is guaranteed anyway, since seccomp kills on `socket`.
- **`subagent` gaps.** Its tool calls are not streamed to the frontend, its tokens are not counted in `TurnEnd`, and a long run cannot be cancelled from the UI.
- **`run_command` on timeout** discards partial output. On Windows only the direct child is killed.
- **`todo`** is not cleared by `forget` and is not persisted.

## Related

- `docs/en/HARNESS.en.md` §The Tool Set (TH pair updated)
- `notes/2026-09-15_runtime-zero-setup.md` (R1)
