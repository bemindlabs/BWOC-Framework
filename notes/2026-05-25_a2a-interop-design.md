# 2026-05-25 — A2A (Agent2Agent) interop — design (Full v1)

Design for making BWOC agents speak the open **A2A (Agent2Agent)** protocol so they interoperate with non-BWOC agents. Scope chosen by maintainer: **Full v1** (server + client + streaming + push + full task lifecycle). Drafted by agent-oracle (architect lane) for review — **not implemented, not routed**. Items marked **[DECIDE]** need a maintainer call before pi starts.

## Why this is a clean fit

BWOC already has every A2A concept under a different name; interop is mostly *mapping*, not new invention:

| BWOC | ↔ | A2A | Notes |
|---|---|---|---|
| `config.manifest.json` (agentId / role / primaryCapability / scope) + `capabilities.md` | → | **Agent Card** (`/.well-known/agent.json`) | `bwoc a2a card` generates it; skills ← capabilities; auth scheme declared |
| envelope (`from\|to\|ts\|messageId\|message`) + `inbox.jsonl` | ↔ | **`message/send`** (Message + Parts) | text Part ↔ `message`; file/data Parts = v1 documented limit |
| Saṅgha task states pending/in_progress/blocked/completed/failed (`team.rs`) | ↔ | **Task lifecycle** submitted/working/input-required/completed/failed/canceled | near 1:1 — incoming A2A work creates a Saṅgha task; `tasks/get` queries it |
| trust Kalyāṇamitta-7 + ed25519 (PR #40) | → | Agent Card **auth schemes** / identity | reuse #40 keys; signed identity prevents card spoofing |
| routing network transport (**deferred**, local-FS) | → | A2A **HTTP / JSON-RPC** transport | A2A *forces* the deferred network transport — it is the concrete driver |

## Architecture decisions

1. **New crate `bwoc-a2a`** — HTTP server (axum/hyper) + JSON-RPC + reqwest (client). Keep all network/HTTP deps **out of `bwoc-core`** (stays lean). `bwoc-agent` gains an optional A2A listener via `bwoc a2a serve`, distinct from the existing Unix control socket.
2. **Agent Card** — `bwoc a2a card` renders `/.well-known/agent.json` from the manifest (skills from `primaryCapability` + `scope`, declared auth, endpoint URL). Served by `bwoc a2a serve`.
3. **Task model = the integration crux** — A2A tasks map onto **Saṅgha tasks** (`team.rs`). An inbound `message/send` that requests work creates a Saṅgha task; its status is queryable via `tasks/get`/`tasks/cancel`. This unifies external A2A task lifecycle with internal coordination — the biggest win *and* the biggest contract change (`team.rs` is core — additive, careful).
4. **Identity / auth** — reuse ed25519 (#40); the Agent Card declares the scheme. External A2A sender → BWOC envelope `from = agent@endpoint`; the existing trust gate evaluates it. **Network transport now real → this also unblocks Trust v2 cross-workspace and #20 give-feedback.** A2A is the unifying driver for the whole deferred-network cluster.
5. **Transport security** — bind **localhost by default**; external bind is explicit opt-in (Sīla / Surameraya — never expose by default). HTTPS + the declared auth scheme for non-local.
6. **Message Parts** — v1 maps text Parts ↔ `message`; file/data Parts get a documented limit (store-ref or reject) rather than silent drop (Musāvāda).

## Full-v1 delivery phasing (honor "full" as the target; ship in gated stages)

Even at Full-v1 scope, build in order so each lands behind gates:
- **P1** — `bwoc-a2a` crate + Agent Card (`bwoc a2a card`) + `bwoc a2a serve` + sync `message/send` → inbox.
- **P2** — `tasks/get` / `tasks/cancel` ↔ Saṅgha task states.
- **P3** — streaming (`message/stream`, SSE).
- **P4** — client/outbound (`bwoc send` to an external A2A endpoint).
- **P5** — push notifications + remaining task states.

## Philosophy binding

- **Samānattatā** — equal treatment now *crosses the framework boundary* to non-BWOC agents. The strongest mapping; A2A is Samānattatā made interoperable.
- **Anattā (SN 22.59)** — A2A is peer-to-peer (agent cards + direct endpoints, no central broker) — consistent with `routing.md`.
- **Sīla / Surameraya** — localhost-default bind, explicit external opt-in.
- **Musāvāda** — ed25519-signed identity prevents Agent-Card spoofing.

## Risks / honest caveats

- **A2A spec evolves.** This mapping is from the protocol's core concepts as of the Jan-2026 knowledge cutoff. **pi MUST verify against the current published A2A spec version at implementation time** and pin a target version — flag any drift from this note.
- New HTTP/JSON-RPC deps are significant — isolated in `bwoc-a2a` to protect `bwoc-core`.
- `team.rs` integration is a contract change — additive, reviewed.
- Coordinates with #40 (Trust v2 identity) and the routing network-transport work — A2A drives all three.

## Open decisions for maintainer [DECIDE]

1. **Bind default** — localhost-only + explicit external opt-in (recommended) vs configurable.
2. **Transport variant** — JSON-RPC only for v1 (recommended) vs also REST/gRPC.
3. **External-peer registry** — a lightweight `a2a-peers.toml` separate from `routes.toml` (recommended) vs folding into `routes.toml`.
4. **Target A2A spec version** — pin one.

## Status / sequencing

- Design only; **not routed**. After maintainer review of the architecture + the four open decisions, route to pi as a phased epic (P1→P5). Should sequence after the current queue (#46) and coordinate with #40.

## Related

- Epic issue (TBD), #40 (Trust v2 ed25519), #20 (give-feedback — A2A network transport unblocks cross-WS)
- `modules/agent-template/interconnect/`: `messaging.md`, `routing.md`, `sangha.md`, `trust.md`, `capabilities.md`
- `crates/bwoc-core/src/team.rs` (Saṅgha task model — the A2A task integration point)
