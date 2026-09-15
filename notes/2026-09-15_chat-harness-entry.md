# 2026-09-15 — Chat entry into bwoc-harness (R0)

Phase R0 of the "bwoc-harness as a daily-driver coding agent" plan. It fixes two ways into the harness that didn't work: non-TUI `bwoc chat` for harness backends, and the missing harness route to the Anthropic provider.

## What changed

- `crates/bwoc-cli/src/chat.rs`: for a harness backend, the default (non-TUI) route now picks between the TUI and protocol mode with `harness_chat_route(stdin_tty, stdout_tty)`:
  - **both TTY** → `bwoc_tui::run` with a one-line stderr note;
  - **otherwise** → `spawn` with extra `--chat --workdir <agent> [--team-chat <log>]`.
  
  `--team` is now honoured on every harness route.
- `crates/bwoc-cli/src/chat.rs` (follow-up): a pure `pane_command` builds the `--tmux` / `--ghostty` command. For harness backends it relaunches `bwoc chat <id> --workspace <ws> --lang <l> [--tui] [--team <t>]`; the pane has a TTY, so it lands on the TUI route. Vendor backends keep `bwoc spawn --path <agent> --backend <b>`. `tmux_launch_args` and `open_in_ghostty` now take that command.
- Shared backend parser: `Backend::from_registry_name` in `spawn.rs` is derived from `value_variants()` + `display_name()`. It replaces the two hand-written `parse_backend` tables in `chat.rs` and `run.rs`; `chat.rs`'s table was missing `grok`. A test round-trips every `bwoc new --backend` value through it. The TUI's non-drivable test list now includes `grok`.
- `crates/bwoc-cli/src/spawn.rs`:
  - new `Backend::Anthropic` (`anthropic`), which spawns `bwoc-harness --backend anthropic [--endpoint baseUrl]`;
  - `uses_harness` includes it;
  - the no-TTY guard lets harness `--chat` through, because it is a pipe protocol.
- `crates/bwoc-cli/src/run.rs`: an `anthropic` arm (harness `--task … --backend anthropic`) and a parser entry.
- `crates/bwoc-tui/src/session.rs`: `is_harness_drivable` accepts `anthropic`, keeping it in step with `uses_harness`. bwoc-tui still doesn't depend on bwoc-cli.
- `help.rs` has a short paragraph; the manifest `backend` doc comment lists `anthropic`.

## Decisions

- **TUI fallback for non-TUI chat on a terminal.** `bwoc-harness --chat` reads `ChatInput` JSON lines from stdin and writes `ChatEvent` JSON lines to stdout (`chat_session.rs`, `run_chat_mode`). It prints no prompt and does not parse plain text, so it isn't usable by a human at a terminal. The TUI is the only human frontend. When stdin or stdout is not a TTY, the caller is a machine, so it gets the raw protocol unchanged. Checking stdout as well as stdin follows the TUI's own stdout-TTY check.
- **Explicit `anthropic` backend instead of re-reading `claude`.** `backend = "claude"` means the vendor Claude Code CLI (subscription auth) for spawn, run, chat and `--tui`'s vendor fallback. Pointing the TUI's `claude` at the harness would silently switch those users to API-key billing. `anthropic` is the opt-in: the harness already accepts `--backend anthropic` (`build_provider` treats it the same as `claude`, and swaps the default Ollama endpoint for the Anthropic endpoint). Nothing changes for `claude`. The manifest and registry store `backend` as a free `String`, so no serde or enum change was needed and older manifests parse as before. `bwoc check` doesn't validate backend values.

## Alternatives considered

- Make the harness `--chat` offer a plain-text REPL on a TTY. That's a bigger change and duplicates the TUI, so it's deferred to a later phase if wanted.
- Make `claude` harness-drivable only when a manifest flag (e.g. `baseUrl`) is present. That's implicit and easy to trigger by accident.

## Smoke (temp workspace, local debug build)

- `bwoc chat tester < /dev/null` (ollama, `gemma4`): reaches harness chat setup and fails with `model 'gemma4' not found`, exit 1. It no longer fails with "--task is required".
- `bwoc chat tester2 < in.jsonl` (ollama, `gemma4:latest`, piped `user` + `quit`): full round-trip — `ready`, `token`, `message` ("pong"), `turn_end`, `bye`; exit 0.
- `bwoc chat tester2 --tmux` (run inside a detached tmux session, so it takes the `new-window` path): the caller prints "Opened tmux window 'agent-tester2' (backend: ollama)". The window runs the relaunched `bwoc chat`, which lands in the chat TUI (status "gemma4:latest · ollama", ready, prior session restored) with a live `bwoc-harness --chat` child.
- `bwoc chat claudey < in.jsonl` (anthropic, no key): `no Anthropic API key — set ANTHROPIC_API_KEY …`, exit 1. The harness Anthropic provider is selected.

## Bugs surfaced and fixed

- `bwoc chat --tmux` / `--ghostty` on harness backends re-invoked `bwoc spawn` without `--chat` and hit the same `--task` error. They now relaunch `bwoc chat`.
- `chat.rs`'s backend parser rejected `grok` agents ("unknown backend"), while `run.rs` accepted them. Both now use one parser derived from the enum, so the lists can't drift.

## Decisions (follow-up)

- The pane gets the resolved `--workspace`, so it doesn't depend on the pane's cwd or `BWOC_WORKSPACE`. It gets the resolved `--lang` (a global flag), so the pane matches the caller.
- The TUI predicate stays a string list in `bwoc-tui`: `session` is a private module, and `bwoc-tui` must not depend on `bwoc-cli`. It is kept in step by tests on both sides.
