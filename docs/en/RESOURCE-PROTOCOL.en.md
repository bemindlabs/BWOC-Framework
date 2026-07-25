---
title: Resource Protocol — Fleet Compute & Memory Sharing
aliases:
  - Resource Protocol
  - BRP
  - Fleet Resource Sharing
tags:
  - group/protocol
  - type/design
  - meta/framework
status: draft (v2026.7.25 — slices A–C shipped: snapshot + gate-check + advertise/discover clients + the gateway broker (discovery half); claim/lease + offload deferred to slice D)
canonical-source: DN 16 (Mahāparinibbāna Sutta) §1.4 — Aparihāniya-dhamma 7, condition 6 (honor shared resources)
parent: English
nav_order: 11
---

# Resource Protocol — Fleet Compute & Memory Sharing

> [!abstract] A signed, time-bounded **lease** over a typed resource, brokered across the fleet through the `bwoc-gateway` relay. A light host (a laptop) borrows compute (GPU/CPU), working memory (RAM, shared KV/context), or read-only knowledge (federated `.bwoc/memory` / RAG) from a heavy host (a GPU server) — under an opt-in **sharing gate** on the provider, so nobody's resources are consumed without consent. This is [Fleet Governance](FLEET-GOVERNANCE.en.md) condition 6 — *honor shared resources* — made operational.

## Why this exists

An agent's home host is rarely the right host for every task. A laptop agent that needs to run a 14B-parameter inference, encode video, or hold a 40 GB dataset in RAM has the *intent* locally but not the *silicon*. Somewhere on the fleet a GPU server sits idle. Today, moving work there means bespoke SSH, hardcoded hostnames, and no accounting of who is using what.

The framework already has the pieces for *dispatching* work across hosts — A2A (`bwoc a2a`, agent-to-agent task calls), `bwoc remote` (run over a `bwocd` host), and the [gateway](https://github.com/bemindlabs/bwoc-gateway) (signed-envelope relay that survives NAT). What is missing is the layer *above* dispatch: **which host has a free resource right now, on what terms, and how do I borrow it without stepping on anyone.** That is the Resource Protocol.

Three design constraints for v1:

1. **Consent-first, refuse-by-default.** A host shares nothing until its operator opts in (`[resource] share = true`) and declares caps. Borrowing is never implicit. (Sīla — the sharing gate, same shape as the financial-write and IAM gates.)
2. **Leases, not sessions.** Every grant is time-bounded and explicitly released or expired. No lease outlives its `ttl`. (Anattā — no clinging to a hold; Aniccatā — everything reclaimed.)
3. **The broker is dumb and untrusted.** The gateway matches offers to claims and relays signed envelopes; it never executes work and never sees plaintext credentials. Trust lives in the ed25519 signatures, not the relay. (Same principle as the gateway's existing relay design.)

## Actors

| Actor | Who | Role |
|---|---|---|
| **Provider** | a `bwocd` host willing to share (the GPU server) | Advertises a resource **snapshot**, evaluates **claims** against its sharing gate, hosts the leased resource. |
| **Consumer** | an agent/host needing a resource (the laptop) | Discovers offers, claims one, uses the lease, releases it. |
| **Broker** | the `bwoc-gateway` relay | Holds the live offer registry, matches `discover` queries, forwards `claim` to the provider, relays lease traffic. Executes nothing. |

## Resource kinds

One protocol, one lease lifecycle, three typed resource kinds:

| Kind | What is borrowed | Backing | Slice |
|---|---|---|---|
| `compute` | A **job** that needs GPU/CPU the consumer lacks (LLM inference, video/3D gen, a heavy build). The RAM the job needs is a *constraint* on the compute claim, not a separate kind. | The provider runs the job (via A2A / `bwocd` task exec) under the lease and returns the result. | C |
| `kv` | A shared **key/value store** — working state / context that two agents on different hosts both read and write (a distributed scratchpad, a KV-cache handle, a coordination map). | The provider hosts a namespaced, lease-scoped store; the consumer reads/writes over the lease. | C |
| `knowledge` | **Read-only** federation of the fleet's knowledge-memory — query every reachable host's `.bwoc/memory` / notes / RAG index and merge the answers. | The provider answers queries against its local knowledge; no state is borrowed, only read. | C |

`compute` is the anchor kind and ships first (after the broker). `kv` and `knowledge` reuse the identical advertise → discover → claim → lease → release lifecycle, differing only in the `spec`/`use` payloads.

## Lease lifecycle

```
 provider                         broker (gateway)                    consumer
    |                                   |                                 |
    |-- RES.ADVERTISE {snapshot,ttl} -->|   (heartbeat, every N s)        |
    |                                   |<-- RES.DISCOVER {kind,min} ------|
    |                                   |--- offers[] ------------------->|
    |                                   |<-- RES.CLAIM {offer_id,spec} ----|
    |<-- RES.CLAIM (forwarded) ---------|                                 |
    |  [sharing gate: accept/deny]      |                                 |
    |-- RES.LEASE {lease_id,ep,exp} --->|--- RES.LEASE ------------------>|
    |                                   |                                 |
    |<===== USE (job / kv / query, authed by lease token) ===============>|
    |                                   |                                 |
    |                                   |<-- RES.RELEASE {lease_id} -------|
    |  [reclaim] (or auto-expire @ exp) |                                 |
```

- **ADVERTISE** — the provider posts its current [snapshot](#resource-snapshot) plus a `ttl`; the broker holds it live and drops it when the `ttl` lapses without a refresh (a crashed provider self-evicts). Heartbeat, not one-shot.
- **DISCOVER** — the consumer asks the broker for offers of a `kind` meeting a `min_spec` (e.g. `gpu.vram_free ≥ 24 GB`). The broker returns matching live offers, best-fit first. No side effects.
- **CLAIM** — the consumer claims a specific offer with a concrete `spec` (the exact job / kv namespace / query scope). The broker forwards it to the provider. The provider evaluates its [sharing gate](#the-sharing-gate) and either issues a lease or denies with a reason (Dhammānupassanā — a denial reports *why*, never a silent drop).
- **LEASE** — on accept, the provider mints a signed `Lease { lease_id, kind, endpoint, granted_to, spec, expires_at }`. The `lease_id` (+ the provider signature over it) is the bearer credential for USE.
- **USE** — the consumer works against the leased resource directly (provider endpoint) or relayed through the gateway when no direct path exists. Every USE request carries the lease token; the provider rejects an expired or unknown lease.
- **RELEASE / EXPIRE** — the consumer releases explicitly, or the provider reclaims at `expires_at`. A released/expired lease is dead; further USE fails closed.

## Resource snapshot

The unit a provider advertises. Built locally, no network:

```json
{
  "host": "bemind",
  "agent_id": "agent-busaba",
  "gpus": [
    { "index": 0, "model": "NVIDIA RTX A6000", "vram_total_mb": 49140, "vram_free_mb": 40320, "util_pct": 12 }
  ],
  "cpu_cores": 128,
  "cpu_load1": 8.4,
  "ram_total_mb": 128000,
  "ram_free_mb": 96000,
  "services": ["ollama", "wan-i2v"],
  "sampled_at": "2026-07-25T07:00:00Z"
}
```

- **GPU** fields come from `nvidia-smi --query-gpu=index,name,memory.total,memory.free,utilization.gpu --format=csv,noheader,nounits`; absent `nvidia-smi` ⇒ `gpus: []` (a CPU-only host still advertises `compute`).
- **CPU / RAM** — `cpu_cores` from `std::thread::available_parallelism`; `ram_total_mb` / `ram_free_mb` + `cpu_load1` (1-minute load average) from Linux `/proc` (`/proc/meminfo` `MemAvailable`, `/proc/loadavg`) in slice A. `ram_free_mb` is *available* (reclaimable) memory, not merely unused. A non-Linux host reports `0` / "unavailable" until a platform backend (`sysctl` / `sysinfo`) lands.
- **`agent_id` and `services` are advertise-time fields**, not part of the local probe. Slice A's `bwoc resource snapshot` emits the host-probe subset — `host`, `gpus`, `cpu_cores`, `cpu_load1`, `ram_total_mb`, `ram_free_mb`, `sampled_at`. `agent_id` (workspace-derived) and `services` (an operator-declared allow-list of named capabilities, e.g. an `ollama` endpoint — advisory, for discovery filters) are attached when the snapshot is *advertised* (slice B).
- The snapshot is **descriptive, not a promise.** The binding promise is the lease the provider mints at claim time, evaluated against live state — a stale snapshot can never over-grant.

## The sharing gate

A provider shares **nothing** until its operator opts in. In `.bwoc/workspace.toml`:

```toml
[resource]
share = true                      # refuse-by-default master switch
gateway = "wss://gw.bemind.tech"  # broker to advertise to

[resource.caps]
max_vram_mb   = 40000             # never lease a GPU claim needing more free VRAM than this
max_ram_mb    = 64000             # cap RAM a single compute/kv lease may reserve
max_cpu_cores = 96                # cap cores per lease
max_leases    = 4                 # concurrent leases this host will hold
allow         = ["agent-anna", "agent-qianliyan"]  # empty ⇒ allow any enrolled fleet peer
kinds         = ["compute", "knowledge"]           # which kinds this host offers
```

Gate evaluation on each CLAIM (all must hold, else deny):

1. `share = true` — the master opt-in. Absent/false ⇒ every claim denied.
2. The claim's `kind` is in `caps.kinds`.
3. The consumer is in `caps.allow` (or `allow` is empty ⇒ any enrolled peer, per the gateway's existing enrollment).
4. The claim's `spec` fits the caps (`vram ≤ max_vram_mb`, `ram ≤ max_ram_mb`, `cores ≤ max_cpu_cores`) **and** live snapshot has it free.
5. Granting would not exceed `max_leases`.

A denial returns a typed reason (`not_sharing`, `kind_not_offered`, `not_allowed`, `over_cap`, `insufficient_free`, `lease_limit`) — never a silent drop. This is [Fleet Governance §6](FLEET-GOVERNANCE.en.md) (*cetiya* — honor shared resources): the operator's caps are the shrine's rules, and the gate enforces them.

## Wire format

Resource messages are `bwoc-gateway` **signed envelopes** — the same ed25519-authenticated shape the relay already carries, with a resource body:

```json
{
  "v": 1,
  "type": "RES.CLAIM",
  "sender": "agent-anna",
  "recipient": "agent-busaba",
  "sent_at": "2026-07-25T07:00:01Z",
  "nonce": "…",
  "body": { "offer_id": "…", "kind": "compute", "spec": { "gpu_vram_mb": 24000, "job": { … } } },
  "signature": "<hex ed25519 over the canonical bytes>"
}
```

- `type ∈ { RES.ADVERTISE, RES.DISCOVER, RES.OFFERS, RES.CLAIM, RES.LEASE, RES.DENY, RES.RELEASE }`.
- Signing/canonicalization is identical to the gateway's message relay (reuse `cc-signing`); the broker verifies the sender signature before registry mutation. The consumer verifies the **provider's** signature on the returned `RES.LEASE` before trusting the endpoint.
- The broker exposes exactly **two** resource routes (slice B): `POST /v1/resource/advertise` and `POST /v1/resource/discover`. Its registry is in-memory and TTL-evicted — a cache of live offers, never a system of record. **`RES.CLAIM` / `RES.LEASE` / `RES.RELEASE` are *not* broker routes** — they are signed envelopes relayed to the provider over the gateway's existing message relay (the provider evaluates the sharing gate and mints the lease). Keeping claim off the broker is what keeps the broker dumb.

## CLI surface

```
bwoc resource snapshot                      # print this host's ResourceSnapshot (READ; local; no network)   ── slice A
bwoc resource advertise [--ttl 30]          # start the ADVERTISE heartbeat to the configured gateway         ── slice B
bwoc resource discover --kind compute \      # query the broker for matching offers
                        --gpu-vram 24000
bwoc resource claim <offer-id> --spec <json> # CLAIM → LEASE (provider sharing-gate applies)                  ── slice C
bwoc resource release <lease-id>            # RELEASE a held lease
bwoc resource status                        # local: my active leases (held + granted)
bwoc resource kv get|set <ns> <key> [val]   # USE a `kv` lease                                                ── slice C
```

Reads (`snapshot`, `discover`, `status`) are free. `advertise` mutates the broker's view of this host and needs `[resource] share = true`. `claim` consumes another host's resources — the *provider* gates it; the consumer just needs an enrolled key.

## Security model

- **Consent both ways.** The provider's sharing gate bounds what leaves; the consumer's signature-verification of the lease bounds what it trusts.
- **No plaintext secrets on the wire.** A `compute` job's own credentials (e.g. an API key the job needs) are never placed in the claim; they resolve on the provider from its own `.bwoc/secrets` or are supplied out-of-band. The protocol carries resource *shape*, not secrets.
- **Fail closed.** Unknown lease, expired lease, unverifiable signature, or a gate miss all deny. There is no "allow on error" path.
- **Bounded blast radius.** A lease grants exactly one resource of one kind under declared caps; it is not a shell. `compute` jobs run in the provider's existing task-exec sandbox — a lease does not widen it.
- **Auditable.** Every ADVERTISE/CLAIM/LEASE/RELEASE is a signed envelope; the provider logs grants + reclaims. Who borrowed what, when, and under whose signature is reconstructible.

## Philosophy grounding

| Decision | Principle |
|---|---|
| Refuse-by-default sharing gate, operator caps | **Sīla** (the gate) + **Fleet Governance §6** (*cetiya*, honor shared resources) |
| Time-bounded leases, explicit release, auto-expire | **Aniccatā** (impermanence) + **Anattā** (no clinging to a hold) |
| Dumb untrusted broker; trust in signatures | **Yoniso Manasikāra** (trust what's verified, not what's asserted) |
| Typed denials with reasons, never silent drops | **Dhammānupassanā** (report the actual state) |
| One protocol, caps sized to real hardware | **Mattaññutā** (right amount — don't lease what isn't free) |

## Slices

- **A (this revision, framework):** this spec + `bwoc resource snapshot` (GPU/CPU/RAM detection) + `bwoc resource gate-check` (dry-run the sharing gate) + the shared types shipped so far (`ResourceSnapshot`, `Gpu`, `SharingConfig`/`Caps`, `ClaimSpec`, `DenyReason`, `ResourceKind`) + the `[resource]` sharing-gate config parse + `evaluate_gate`. All local + unit-tested; nothing yet talks to the broker. The `Lease` struct and `RES.*` envelope types are specified above but land with the transport in slices B/C.
- **B (bwoc-gateway, shipped):** the broker's discovery half — `POST /v1/resource/advertise` + `POST /v1/resource/discover` over a TTL-evicted in-memory offer registry (one live offer per provider, last-writer-wins). Claim/lease deliberately rides the existing signed-envelope relay, not a new broker route.
- **C (framework, shipped):** the broker clients — `bwoc resource advertise` (one-shot offer publish; run on a timer for a heartbeat, gated on `[resource] share = true`) and `bwoc resource discover` (query by kind + min spec). Both reach the gateway over HTTP(S) via `curl` (the CLI stays HTTP-client-free). The discovery loop now works fleet-wide end-to-end.
- **D (framework, next):** the borrow itself — `claim` (send a `RES.CLAIM` envelope to the provider over the relay; provider evaluates `evaluate_gate` and mints a signed `RES.LEASE`), `release`, `compute` offload execution (run the job on the provider via A2A → return the result), then `kv` and `knowledge`.

## Cross-references

- [Fleet Governance](FLEET-GOVERNANCE.en.md) — §6 *honor shared resources* is this protocol's charter.
- [Signing](SIGNING.en.md) — the ed25519 signed-envelope contract the resource messages reuse.
- A2A (`bwoc a2a`) — the task-execution transport a `compute` lease runs its job over.
- [gateway](https://github.com/bemindlabs/bwoc-gateway) — the relay that becomes the broker.
- [PLUGINS](PLUGINS.en.md) §Write verbs — the sharing gate is the same refuse-by-default shape as the financial-write / IAM gates.
