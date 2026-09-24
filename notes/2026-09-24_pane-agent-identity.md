# 2026-09-24 — Agent panes answer as the agent

The owner opened agent-inao (a `claude`-backend agent) with `/agents`, asked "อิเหนาใช่ไหม?", and it replied that it was agent-fern. Two causes stacked.

## What changed

- `bwoc-tui` `Panes::open`: the pane's harness now gets `--workdir <workspace>/<agent path>`. Before, it got the workspace root. The harness builds its prompt, memory recall and session file from the workdir, so every pane got the generic project preamble with no `AGENTS.md`, persona or memory. Every pane also shared the root's session file: the main session showed a pane's old `PANE-OK` exchange. An unsafe registry path now refuses to open the pane instead of falling back to the root.
- `bwoc-harness` `CliClient::with_cwd`: `build_provider` passes the session workdir. Each `claude -p` turn now runs there, not in the harness process's cwd, which was wherever `bwoc` was launched. There, the CLI loaded that directory's `CLAUDE.md` and the user's SessionStart hooks. Those are real system instructions, so they outranked the agent profile, which the harness sends only as `System:` text on stdin.

## Verification

- Before (release `bwoc-harness` 3.7.0, pane argv, real `claude` haiku): "who are you?" → `agent-busaba — บุษบา (Busaba)`. This server's hook identity is the equivalent of Fern's.
- After: the same prompt through the tmux TUI, `/agents agent-claudey` → `agent-claudey, claude test`; `/agents agent-helper` (LiteLLM) → `agent-helper`. The claude pane restored only its own history.
- New test `turn_runs_in_the_session_workdir`.

## Status / deferred

- The user-level `~/.claude/CLAUDE.md` and hooks still load for a `claude` agent, because the CLI always reads them. With the agent's own `CLAUDE.md` (→ `AGENTS.md`) now in the cwd, identity held in testing. Sending the profile via `--append-system-prompt` would make it stronger, but it changes the documented CLI flag contract (`-p --model --output-format json`) that wrappers rely on. Not done.
