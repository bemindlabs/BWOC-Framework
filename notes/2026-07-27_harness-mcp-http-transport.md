# 2026-07-27 — Harness MCP: Streamable HTTP transport (remote servers)

The deferred half of "MCP modernize" (the first half — protocol bump + negotiation — landed in #384). Adds a second `RpcTransport` so the harness can consume **remote** MCP servers over the Streamable HTTP transport, not only local stdio subprocesses.

## Correction to the earlier "needs bemind" framing

When this was split off (#384/#385 notes) I flagged it as bemind-only. That was over-cautious: HTTP transport is **OS-agnostic**, so the risk logic (JSON-vs-SSE framing, session id, https/redirect guards) is fully Mac-verifiable by factoring it into pure functions + unit tests. Only a *live* remote-server smoke test is bemind-worthy, and it's optional. No Landlock/seccomp-class OS dependency here.

## What changed

- **`HttpTransport`** (`mcp.rs`) implementing the existing `RpcTransport` trait — one POST per JSON-RPC request to a single endpoint, `Accept: application/json, text/event-stream`. Redirects **disabled** (`redirect::Policy::none()`) as SSRF hardening; the server-assigned `Mcp-Session-Id` (from the initialize response) is captured and echoed on later requests; optional `Authorization: Bearer`.
- **Two pure helpers, unit-tested without a socket:**
  - `validate_http_url` — https required except `http://localhost|127.0.0.1|[::1]` (cleartext-downgrade / SSRF guard); non-http(s) schemes refused.
  - `parse_http_rpc_response` — extracts the JSON-RPC `result` for our id from **either** a JSON body (object or batch array) **or** an SSE body (`data:` lines), skipping unrelated ids and surfacing RPC errors.
- **`McpClient::connect_http(url, auth)`** — mirrors `connect_stdio`, reusing the `initialize` version-negotiation from #384.
- **`token_from_secrets(label)`** — bearer token from `~/.bwoc/secrets.toml` `[mcp] <label>_token`, same per-user location + `0600` guard as the provider API keys.
- **CLI `--mcp-http <url>`** (repeatable) in `main.rs` — label = URL host **sanitized to `[a-z0-9_]`** (`example.com` → `example_com`) so it works as both a tool-name prefix segment and a bare TOML secrets key; token auto-resolved; same fail-soft posture as `--mcp` (a failed server warns, the run proceeds). Tools exposed as `mcp__<label>__<tool>`.
- Module doc updated (stdio-only → two transports); the bounded `REQUEST_TIMEOUT` is shared with stdio.

## Decisions

- **`.text()` + timeout, not a streaming reader.** For a single request/response the server sends its reply then closes (or holds SSE open, bounded by `REQUEST_TIMEOUT`). Reading the full body and framing it purely is far simpler than a live SSE reader and covers the request/response shape MCP tools use. A streaming reader that stops at the first matching id is a later refinement (only matters for long-lived server-push).
- **https-except-loopback, redirects-off.** Even though the URL is operator-configured (not model-chosen), silently downgrading to cleartext or following a redirect to an internal address is the wrong default for a security framework. Loopback stays allowed for local dev servers.
- **No new doc section.** `--mcp`/`--mcp-http` and the anthropic key are all documented via CLI `--help` + code comments, not a public HARNESS section; adding one only for HTTP would be asymmetric. The flag help text documents the URL rule + the secrets token key.
- **No dep added.** `reqwest` is already a harness dep (the providers use it).

## Verification

macOS: fmt + clippy (`--workspace` and `-p bwoc-harness --features test-redteam`) clean with `-D warnings`; workspace tests pass, 0 failed. New unit tests: url-guard (https/loopback/reject), JSON-response parse, SSE parse + id-skip, RPC-error + missing-id, cleartext-rejected-at-construction. **Not run:** a live remote MCP server round-trip (optional bemind smoke) — no such server configured here.

## Status / deferred

With this, MCP modernize is complete (protocol negotiation #384 + stdio + Streamable HTTP). Remaining harness follow-up: the multimodal screenshot IPC wiring (crosses the Phase-5 re-exec boundary — genuinely bemind-verified). Structured output (audit item 5) stays parked.

## Related

- `crates/bwoc-harness/src/mcp.rs` · `main.rs` · #384 (protocol bump/negotiation) · MCP spec Streamable HTTP transport.
