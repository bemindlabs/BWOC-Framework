# 2026-05-31 — `bwoc chat --tmux` launched the wrong `bwoc` binary

Investigating the `bwoc chat --tmux` launcher (shipped in 2.17.0 / PR #137) surfaced a binary-resolution bug shared by every "shell out to `bwoc`" site in `chat.rs` and `dashboard.rs`.

## What changed

`crates/bwoc-cli/src/chat.rs` and `crates/bwoc-cli/src/dashboard.rs` no longer hardcode the program name `"bwoc"` when re-invoking the CLI in a child process. They now call a new helper `crate::spawn::bwoc_exe()`, which returns `std::env::current_exe()` (falling back to `"bwoc"` only if that fails). Sites fixed:

- `chat.rs`: `open_in_tmux` (via `tmux_launch_args`, which gained a `bwoc_exe: &str` param) and `open_in_ghostty`.
- `dashboard.rs`: the `t` (chat-in-tmux), `g` (Ghostty), `l` (log), `i` (inbox) hotkeys and the `start`/`stop` captured shell-outs.

## Decisions

- **One concern, fix it whole.** The user named "tmux chat", but the defect is uniform: a bare `"bwoc"` in an argv/`Command` is a `$PATH` lookup, not the running binary. Half-fixing only the tmux-chat path would leave the identical latent bug one line away in the same files and invite a "why partial?" review. So every `bwoc`-re-invocation in both files was routed through the one helper. (Yoniso manasikāra — fix the actual defect, not just the named symptom; Mattaññutā — one helper, not six copies.)
- **Helper lives in `spawn.rs`** next to `harness_binary()`, which already encodes the "sibling of the running binary" resolution rule. `bwoc_exe()` is the `bwoc`-self analogue of that.
- **Fallback to `"bwoc"`** preserves the old behavior in the (practically impossible) case `current_exe()` errors, rather than failing hard.

## Bugs surfaced and fixed

`./target/debug/bwoc chat <agent> --tmux` opened a tmux window running `/opt/homebrew/bin/bwoc` (the installed **2.11.0**) instead of the dev **2.18.0** that launched it — a silent version mismatch. With no `bwoc` on `$PATH`, the window ran a non-existent command and closed immediately, leaving the user staring at a vanished pane with no diagnostic.

Verified end-to-end after the fix: from inside tmux, the dev binary's `chat --tmux` opens a window whose `pane_start_command` is the dev `target/debug/bwoc spawn …`, with `$PATH` pointing only at the 2.11 install. Unit test `chat::tests::launch_args_use_the_given_bwoc_exe_verbatim` pins the argv to the passed exe path (and asserts no bare `bwoc`).

## Status / deferred

Shipped on `fix/tmux-chat-running-binary`. The `--ghostty` path is macOS-only and unchanged in behavior beyond the binary it points at.

## Related (links)

- Origin of the launcher: PR #137 (`bwoc chat --tmux` auto-start), release 2.17.0.
- Precedent: `crates/bwoc-cli/src/spawn.rs::harness_binary` (sibling-of-running-binary resolution).
