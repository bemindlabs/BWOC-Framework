# 2026-09-19 — Bare `bwoc` on vendor coding CLIs (#529)

Bare `bwoc` refused every vendor-CLI backend, so a host with only a Claude Code or Codex subscription (no API key, no Ollama) could not open a session at all. Vendor backends (`claude`, `codex`, `agy`, `kimi`, `grok`, `copilot`) now exec their CLI in the current directory.

## What changed

- `runtime.rs`: `Resolution::Vendor(VendorSession)` for any backend with a `cli_name()`. It execs the CLI (Unix `exec`, so the CLI owns the terminal and its signals; elsewhere `status()` with the exit code passed through), forwarding `--model` when set. All six CLIs take `--model <id>` (checked against the installed `--help` output).
- A one-line stderr notice before handing over: own login, tools and permissions; harness tools and trust gates do not apply.
- `[defaults] backend` stands in for an absent `[runtime] backend` in the same file. Nothing else read that key, so the reporter's declared backend had no effect anywhere.
- No-provider help names vendor CLIs found on `PATH` (a cheap `is_file` scan, no `--version` exec).
- `cli` stays refused, with an error that lists both families.

## Decisions

- **Exec the vendor CLI rather than wrap it as a chat-only TUI** (the issue's proposal 1). The harness `cli` provider flattens the whole transcript into one print-mode call per turn, can't stream, and speaks only Claude's flag set. Each vendor CLI is already a full coding agent, so a chat-only wrapper would be strictly worse than the CLI itself. Handing over the terminal is honest about who enforces what.
- **No automatic vendor pick.** Auto-detect still only chooses harness providers. Silently launching a third-party agent with ambient authority is the wrong default; the help names the exact command instead.

## Status / deferred

#485 (ACP adapter) and #452 (Dispatch seam) stay open. Both are recorded as deferred and gated on demand (an editor user asks; a third trust-tier route exists), and neither gate has been met.
