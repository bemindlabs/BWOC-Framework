# 2026-05-28 — A2A auth phase AP1: inbound Bearer auth

First slice of the A2A auth-phase epic (#80, follow-up to the loopback-only v1
#48). Adds Bearer-token authentication to the listener — the foundation that
later phases (non-loopback bind, webhook delivery, rate caps) build on.

## What changed

- **`serve.rs`** — when a token is configured, the JSON-RPC + SSE endpoint
  requires `Authorization: Bearer <token>`; a missing/invalid credential gets
  `401` + `WWW-Authenticate: Bearer`. Token compared **constant-time**
  (`ct_eq`). The **Agent Card GET stays public** (peers must be able to discover
  the auth requirement). No token configured ⇒ unchanged loopback-only posture.
- **`types.rs`** — `AgentCard` gains optional `securitySchemes` + `security`;
  `AgentCard::with_bearer_security()` sets an `httpAuthSecurityScheme` (Bearer).
- **`bwoc-a2a` binary** — resolves the token from `BWOC_A2A_TOKEN` (env, wins)
  or the agent's `.bwoc/a2a.token` file; advertises the scheme on the card when
  present; the startup line reports `auth ON/OFF`. The non-loopback warning now
  fires only when auth is **off** (binding wide open with a token is no longer
  the unguarded footgun it warned about).

## Decisions

- **Bearer, not OAuth2/mTLS.** The minimal foundation for a local-first
  framework; richer schemes can slot behind the same `securitySchemes` surface.
- **Card GET is unauthenticated.** A2A discovery is public by design — the card
  *advertises* the requirement; the protected surface is the RPC/SSE endpoint.
- **Constant-time token compare.** Hand-rolled `ct_eq` (length check then XOR
  fold) rather than a new `subtle` dependency — a few lines, no dep, folds every
  byte of equal-length inputs so compare time doesn't leak the match position.
  (Length is allowed to leak — a token's length is not the secret.)
- **Token via env or file, auto-detected.** No new flag: presence of
  `BWOC_A2A_TOKEN` / `.bwoc/a2a.token` turns auth on. Keeps the common
  loopback-dev case zero-config while making "expose it" a deliberate act.

## Status / deferred (later AP phases, #80)

- AP2 — drop the non-loopback warning entirely once auth is on (this PR only
  stops it lying about "no auth").
- AP3 — push **webhook delivery** + SSRF guard (#48-P5 deferral).
- AP4 — per-token request rate + `SubscribeToTask` concurrency caps.
- AP5 — outbound client auth (`bwoc a2a send`/`fetch-card` present credentials).
- `.bwoc/a2a.token` perms: the operator creates it; a future `bwoc a2a keygen`
  could mint it `0600` like the signing key.

## Verification

- 49 `bwoc-a2a` tests incl. auth: missing/wrong token → `401` (unary + SSE
  method), correct token → `200`, card public + advertises the scheme. Full
  workspace + clippy green; `bwoc-cli` still HTTP-free. Live curl: 401/401/200
  against a real `bwoc a2a serve` with `BWOC_A2A_TOKEN`.

## Related

- Epic #80 (auth phase). Builds on the v1 listener (#72).
- `crates/bwoc-a2a/src/serve.rs`, `types.rs`, `main.rs`.
