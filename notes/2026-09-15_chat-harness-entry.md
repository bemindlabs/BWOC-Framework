# 2026-09-15 — Chat entry into bwoc-harness (R0)

Phase R0 of the "bwoc-harness as a daily-driver coding agent" plan. It fixes two ways into the harness that didn't work: non-TUI `bwoc chat` for harness backends, and the missing harness route to the Anthropic provider.

## What changed

- `crates/bwoc-cli/src/chat.rs`: for a harness backend, the default (non-TUI) route now picks between the TUI and protocol mode with `harness_chat_route(stdin_tty, stdout_tty)`:
  - **both TTY** → `bwoc_tui::run` with a one-line stderr note;
  - **otherwise** → `spawn` with extra `--chat --workdir <agent> [--team-chat <log>]`.
  
  `--team` is now honoured on every harness route except `--tmux` / `--ghostty`.
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
- `bwoc chat claudey < in.jsonl` (anthropic, no key): `no Anthropic API key — set ANTHROPIC_API_KEY …`, exit 1. The harness Anthropic provider is selected.

## Status / deferred

- `bwoc chat --tmux` / `--ghostty` for harness backends still re-invoke `bwoc spawn` without `--chat`, so they hit the same `--task` error. The fix is to relaunch `bwoc chat <id>` instead.
- `chat.rs::parse_backend` has no entry for `grok`, which `run.rs` does have. This predates the change and was left alone.
