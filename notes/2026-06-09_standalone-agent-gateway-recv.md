# 2026-06-09 — Standalone agent: gateway receive bridge

`bwoc-agent --serve` could only receive messages from a *local* `.bwoc/inbox.jsonl`; nothing fed it a message relayed across machines. This adds the receive half so an agent deployed on any machine can join the mesh through the deployed `bwoc-gateway` relay (the send half — `RouteTarget::Gateway` + `bwoc-gateway-send` — shipped in 2.28.0). Part of the approved "standalone agent (production-ready core)" plan.

## What changed

- **`crates/bwoc-agent/src/gateway.rs`** (new) — `GatewayRecvSupervisor`, a near-clone of `connectors.rs`. When `interconnect/gateway.toml` declares `enabled = true` + `url`, it supervises a `bwoc-gateway-recv` child (resolved via `bwoc_core::exec::sibling_binary`): initial spawn, respawn-on-exit with the same `RESPAWN_BACKOFF` crash-loop throttle, kill-on-shutdown, and a `.bwoc/gateway.status` health marker. agent-id comes from `gateway.toml` `agent_id` or falls back to `config.manifest.json` `agentId`.
- **`crates/bwoc-agent/src/main.rs`** — `serve_core` builds + announces + ticks the gateway supervisor next to the connector supervisor (~4 call-site lines); the accept loop, inbox poller, and trust gate are untouched.
- The `bwoc-gateway-recv` binary itself lives in the **`bwoc-gateway` repo** (separate PR) — it dials the relay, authenticates with the agent's ed25519 keypair (the keypair IS the login), and appends each relayed transport envelope's `body` to `.bwoc/inbox.jsonl` as one line. That `body` is the **full framework message envelope** (`{from,to,ts,messageId,message,nonce,sig}`) that `bwoc-gateway-send` placed there — i.e. exactly the line shape the inbox poller already expects, not a bare message string.

## Decisions

- **Supervised subprocess, not in-process.** The recv bridge carries WebSocket/TLS deps; those must NOT enter `bwoc-agent`/`bwoc-core` (dep-quarantine HARD RULE). The daemon only supervises the child — the exact `bwoc-connect` pattern. The inbox file is the clean seam: the bridge is just one more producer of it, like `bwoc-mqtt serve`.
- **Cloned `connectors.rs` rather than generalizing it.** Different config file + arg shape; a ~150-line clone is clearer than widening one supervisor for two callers (Mattaññutā).
- **Receive == local delivery.** The bridge writes the standard message envelope into the inbox, so everything downstream (poll, cursor, trust gate, refusals) is reused unchanged.

## Status / deferred

- Done here: transport (`bwoc-gateway-recv`, separate repo) + supervision. A standalone agent can now *receive*.
- Still required for production internet-facing (separate PRs): trust-gate **pinned-peer (`peers.toml`) verification** to close `unknown_sender` for remote senders + **replay defense** (nonce/ts); **untrusted auto-process** so a gateway message actually drives a (sandboxed, read-only-by-default) harness turn and replies; the standalone **Dockerfile** + deploy.

## Related

- `bwoc-gateway` PR (the `bwoc-gateway-recv` binary).
- `crates/bwoc-agent/src/connectors.rs` — the supervision pattern cloned.
- Plan: `~/.claude/plans/inherited-strolling-porcupine.md`.
