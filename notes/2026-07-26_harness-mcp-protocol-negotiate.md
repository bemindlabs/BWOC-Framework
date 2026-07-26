# 2026-07-26 — Harness MCP: protocol-version bump + negotiation

The MCP client advertised a hardcoded `protocolVersion: "2024-11-05"` (two spec revisions stale) and **ignored** the version the server echoed back. This closes the first, safe slice of the "#7 MCP modernize" gap: version currency + real negotiation. The larger slice — Streamable HTTP transport for remote servers — is deliberately a separate follow-up (scoped out with the user).

## What changed

- **`LATEST_PROTOCOL_VERSION = "2025-06-18"`** and **`SUPPORTED_PROTOCOL_VERSIONS = ["2025-06-18", "2025-03-26", "2024-11-05"]`** (`mcp.rs`) — the harness now advertises the current revision and knows which older ones it can still speak.
- **`initialize()` now negotiates and returns the agreed `String`** instead of `()`. It reads the server's echoed `protocolVersion`: accepts any version in the supported set (a server MAY downgrade to an older shared revision), defaults to the advertised version if the field is absent (pre-negotiation servers), and **refuses** (returns `HarnessError::Provider`) on any version outside the set rather than proceeding with undefined semantics. `connect_stdio` discards the returned version (stdio needs nothing further); it exists for the HTTP transport follow-up and for tests.

## Decisions

- **Accept-set, not exact-match.** MCP explicitly allows the server to respond with an older shared revision, so requiring an exact echo of `LATEST` would break perfectly good `2024-11-05`-only servers we already work with. Refusing only truly-unknown versions keeps interop wide while staying safe.
- **Refuse unknown, don't warn-and-continue.** For a security framework, proceeding against a revision whose message semantics we don't implement is the wrong default. Fail the handshake loudly. Same for a present-but-non-string `protocolVersion` (a malformed/hostile handshake): refuse rather than mask it as "missing" and silently default (Copilot #384).
- **No new doc.** MCP has no existing `docs/` page; per Mattaññutā this internal bump doesn't justify creating one (and thus no EN/TH pair to maintain). The behavior is covered by the module doc-comment + unit tests.
- **HTTP/Streamable transport deferred** (unchanged `RpcTransport` trait already abstracts it). It carries real remote attack surface (SSRF, auth-token handling, host allowlist) and needs bemind Linux verification, so it stays a separate PR.

## Verification

macOS: fmt + clippy (`--workspace` and `-p bwoc-harness --features test-redteam`) clean with `-D warnings`; workspace tests pass, 0 failed. New unit tests: advertises-latest + returns-negotiated, accepts-server-downgrade, defaults-when-version-omitted, rejects-unsupported. Env-scrub security tests unchanged and still green.

## Status / deferred

Remaining "go next all" roadmap: Streamable HTTP MCP transport (this note's deferred half), #8 multimodal input (LARGE — next, scope first). #5 structured output stays parked.

## Related

- `crates/bwoc-harness/src/mcp.rs` · MCP spec <https://modelcontextprotocol.io/specification> · prior harness LLM sprints (#380–#383).
