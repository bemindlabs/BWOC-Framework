# 2026-05-26 — HV2-5 MCP client (BWOC-7)

The harness now consumes external MCP tool servers: launch a server subprocess, speak JSON-RPC 2.0 over its stdio, discover its tools, and register each as an ordinary `ToolImpl`. The tool set extends without harness code changes. Last buildable workstream of the harness-v2 epic (only BWOC-3 remains, gated at its spec).

## What changed

- New `crates/bwoc-harness/src/mcp.rs`:
  - `RpcTransport` trait (`request` / `notify`) — injectable so client logic is tested without a real server.
  - `StdioTransport` — line-delimited JSON-RPC over a spawned subprocess's stdio; id-correlated responses (skips notifications / non-JSON server chatter); keeps the child alive for its lifetime.
  - `McpClient` — `connect_stdio` (spawn + `initialize` + `notifications/initialized`), `list_tools` (`tools/list`), `register_tools` (registers one `McpTool` per discovered tool).
  - `McpTool` — a `ToolImpl` whose `execute` forwards to `tools/call` and extracts `result.content[].text`.
- `main.rs`: `--mcp "<server cmd>"` (repeatable) launches servers and registers their tools before the run; failures warn, not fatal.
- `lib.rs`: `pub mod mcp;`.
- Tests: `tools/list` parsing, registry registration with prefixed name, `execute` forwards to `tools/call` (via a mock transport). 235 lib tests green; clippy + fmt clean.

## Decisions

- **Client only — handled by `ToolImpl`, so the safety pipeline wraps it for free.** MCP tools register like any built-in; calls flow `dispatch` → `execute_tool_calls` → guardrails → permission → sandbox. No new exec path, no bypass. *Satisfies BWOC-7 AC2 by construction.*
- **Hand-rolled stdio JSON-RPC, no new dependency** (maintainer-ratified this turn). `serde_json` + `tokio::process`, matching the hand-rolled OpenAI provider; dep-quarantine stays clean. The `RpcTransport` trait keeps it testable and leaves room for an HTTP/SSE transport later.
- **Names prefixed `mcp__<server>__<tool>`** to avoid collisions with built-ins and across servers (mirrors common MCP-host naming).
- **Runtime names leaked to `&'static str`.** `ToolImpl::name()` returns `&'static str`, but MCP names are discovered at runtime; `Box::leak` the prefixed name. The set is bounded (one per discovered tool) and lives for the process — acceptable rather than reshaping the trait.

## Alternatives considered

- The `rmcp` SDK — rejected this turn (new heavy dep under dep-quarantine; the hand-rolled client covers stdio tools/list + tools/call).
- Reshaping `ToolImpl` for owned names instead of leaking — deferred; not worth a trait change for a bounded, process-lifetime set.

## Status / deferred

- Status set to `review` on the workspace board (BWOC-7).
- **stdio transport only** — HTTP/SSE deferred (the `RpcTransport` trait is the seam to add it).
- **MCP server role deferred** (per the planning-note decision: network surface, no current need).
- Concurrent MCP calls serialize at the transport's stdio mutex (the external server may not be concurrency-safe anyway).
- No real-server integration test (no MCP server in the test env); covered via a mock transport.

## Related (links)

- `notes/2026-05-25_harness-v2-planning.md` — HV2-5 decision (client-only) + the registry/safety seams.
- GH #39 (harness-v2 epic, HV2-5). `<workspace>/.scrum/backlog.json` — BWOC-7.
