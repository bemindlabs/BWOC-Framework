# 2026-10-14 — Copilot review backlog: bwoc-harness

Triage of 73 unresolved Copilot review threads on merged bwoc-harness PRs, judged against current `main`. Real defects fixed in one batch; the rest were already addressed or declined.

## What changed
- **Budget (HV2-6):** `RunState` persists `total_tokens`, so `--resume` continues the run's budget; compaction's summarizer usage now counts toward it; with no provider `usage`, the gate enforces a chars/4 estimate instead of never tripping.
- **Provider HTTP:** the OpenAI-compat client bounds non-streaming requests end to end but the SSE stream only per read (idle), so a long live generation is no longer cut at 120s; `ollama_root` tolerates a trailing `/`; client build failure panics loudly instead of silently dropping the timeout.
- **Subprocess hygiene:** MCP stdio servers and deep-memory commands are `kill_on_drop` (no orphans on exit/timeout).
- **Sandbox / guardrails:** landlock ruleset built in the parent (`pre_exec` only calls `restrict_self`); Windows guardrail matches path-qualified binaries, looks through `powershell`/`pwsh` hosts, and collapses dot-segments in root checks; curl-pipe-shell scan knows Windows shells; `harden_child_fds` fails closed if stdio cannot be re-pointed; `.git` read in the parent is bounded.
- **Loop:** token-pressure switch resets the malformed-call streak; static context limits skip the provider query; the context estimate counts tool-call arguments.
- **Misc:** `--mcp` is split quote-aware; a resumed run keeps its original task in the Tier 2 transcript and worker envelope; `proc_listpids` gated to macOS; selftest fd probe widened; redteam checks `waitpid`; doc/comment corrections.

## Decisions
- Declined: cgroup v2 `"0"` → `cgroup.procs` (the kernel treats 0 as the writer on v2 too), leak-on-`Box::leak` MCP names (one-time, bounded by `tools/list`), always-serialized eval `skipped`, emitting both reasoning fields (duplicates when both present), egress tripwire on a public IP (landlock denies before routing).
