# 2026-09-21 — Nine more `/` commands in the chat TUI

A gap pass over the slash surface: everything the session already knew but never showed, plus the four environment questions the CLI could answer all along.

## What changed
- **From state the TUI holds:** `/status`, `/tools`, `/cost`, `/retry`, `/save [path]`. The `Ready` event has carried a `tools` list since the protocol was written — the TUI was discarding it with `..`.
- **From the caller, via a new `bwoc_tui::EnvironmentInfo` trait:** `/models`, `/backends`, `/settings` (alias `/config`), `/doctor`. `bwoc-cli::runtime::Environment` implements it.

## Decisions
- **A trait again, not a dependency** — same reasoning as `SessionControl`: probing Ollama, reading `secrets.toml` and running `doctor` are `bwoc-cli`'s business, and `bwoc-tui` stays on `bwoc-core`.
- **`/models` refuses to guess.** Only a backend with a real model index answers (Ollama); a hosted API returns "does not list models — pass one to `/model <name>`" instead of a hardcoded list that rots.
- **`/backends` names the *source* of each key** (`env ANTHROPIC_API_KEY`, `~/.bwoc/secrets.toml`) and never a key.
- **`/doctor` shells out to `bwoc doctor --json`** rather than reimplementing the checks; a second implementation would drift from the one operators trust.
- **`/settings` labels are captured at session start**, where the layers that produced them are in scope, and include the config files behind the values and the precedence order.
- **`/retry` refuses while a turn is running**, like a session switch.

## Status / deferred
- `/compact`, `/permissions`, `/mcp` and `/context` need protocol additions and are not in this change.
- `/workspaces` and `/fleet` are deliberately absent: a project session is not in a workspace, and `bwoc fleet` covers the case where it is.
