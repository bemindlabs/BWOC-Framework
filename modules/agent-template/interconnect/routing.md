---
title: Inter-Workspace Routing
aliases:
  - Routing
  - Interconnect Routing
  - Anattā / No Central Authority
tags:
  - group/agents
  - type/design
  - meta/template
status: draft (v2026.5.23 — spec only; no code yet)
canonical-source: SN 22.59 (Anattalakkhaṇa Sutta) — Saṃyutta Nikāya 22.59
---

# Inter-Workspace Routing

> [!abstract] `bwoc send` today can only reach an agent in the **same** workspace registry. Routing adds a per-workspace, peer-declared table (`.bwoc/interconnect/routes.toml`) that lets a message reach an agent in a **peer** workspace — with no central broker. Each workspace declares its own peers; none is the privileged center. This is the structural reading of **Anattā**: a routing mesh with no permanent controlling self.

## Motivation

Phase 3's Definition of Done has two halves: *an agent's life ends cleanly* (vaya — see [`retire`](../../../crates/bwoc-cli/src/retire.rs)) and *agents coordinate without a central authority*. Trust ([`trust.md`](trust.md)) and messaging ([`messaging.md`](messaging.md)) gave agents a verified channel **within** one workspace. They cannot cross a workspace boundary: `send` resolves the recipient from a single `AgentsRegistry::load(&workspace)` and appends to that workspace's tree ([`send.rs:88-124`](../../../crates/bwoc-cli/src/send.rs)).

"Without a central authority" is the design constraint that rules out the obvious fix (a global agent directory every workspace consults). Instead each workspace **declares the peers it knows**, and routing is the union of those local declarations — never a single owned map. This is the same non-ownership principle the framework already applies to branches and worktrees (Anattā in the vaya sense), turned toward topology: **no node owns the mesh.**

| Principle | In the System | Maps to |
|---|---|---|
| **Anattā** (no central self) | No global directory, no broker; each workspace is its own locus | [`PHILOSOPHY.en.md` #4 Tilakkhaṇa](../docs/en/PHILOSOPHY.en.md) |
| **Samānattatā** (equal standing) | Every peer workspace is equal — routing privileges none | [`PHILOSOPHY.en.md` #12 Saṅgahavatthu 4](../docs/en/PHILOSOPHY.en.md) |

> [!note] Mapping confirmed by the operator (2026-05-23): the canonical anchor is **SN 22.59 / Anattā** (no central self → no central broker), with Samānattatā as the supporting design principle. Considered and not chosen: nesting under Saṅgaha ([`sangha.md`](sangha.md)) or Aparihāniya-dhamma ([`FLEET-GOVERNANCE.en.md`](../../../docs/en/FLEET-GOVERNANCE.en.md)).

Three design constraints for v1:

1. **Peer-declared, not discovered.** A workspace reaches only the peers it lists. No broadcast, no gossip, no service registry. Adding a peer is an explicit local edit.
2. **Additive — current behaviour is the fallback.** The local registry lookup stays the fast path and is tried first; routing is consulted only on a miss. No existing single-workspace flow changes.
3. **Local filesystem only.** v1 delivers to peer workspaces reachable on the local filesystem. Network transport (ssh, http) is deferred and travels with [Trust v2](trust.md) cross-workspace work.

## The Routing Table

`.bwoc/interconnect/routes.toml`, one per workspace. Absent file ≡ no peers ≡ today's behaviour.

```toml
# Each route tells `bwoc send` where to deliver messages for a recipient
# that is NOT in the local registry. There is no central directory —
# this file is the workspace's own declaration of who it can reach.

[[route]]
agent = "agent-neo"                 # exact recipient id
workspace = "/abs/path/to/peer-ws"  # the peer workspace root (local FS)

[[route]]
namespace = "team-b"                # OR a prefix: routes any `team-b-*` recipient
workspace = "/abs/path/to/team-b-ws"
```

A route is either `agent` (exact id) **or** `namespace` (prefix) — not both. `workspace` is the peer's root directory (the one holding its `.bwoc/agents.toml`).

## Resolution Order

`send` resolves a recipient in three steps. The first match wins; behaviour is purely additive.

1. **Local registry** — `AgentsRegistry::load(&workspace)`, `find(id == lookup_id)`. Unchanged fast path. Hit → deliver locally as today.
2. **Routing table** — on a local miss, load `routes.toml`:
   - exact `agent` match → resolve the peer `workspace`, load **that** registry, find the recipient, append to the peer's `<agent>/.bwoc/inbox.jsonl`.
   - else longest `namespace` prefix match → same.
3. **No match** — the existing `NotFound { name, workspace }` error, unchanged.

> [!example] `bwoc send agent-neo "ping" --from agent-oracle` where `agent-neo` is absent locally but `routes.toml` has `agent="agent-neo", workspace="/srv/ws-b"` → the envelope lands in `/srv/ws-b/<agent-neo path>/.bwoc/inbox.jsonl`, `from = "agent-oracle"`.

## Composition with Trust — Why Routing Ships Before Trust v2

Cross-workspace delivery and the trust gate compose into a **safe default** without any new code in the gate:

- The recipient daemon's trust check resolves the envelope's `from` against **its own** registry ([`trust.md` §Refusal Semantics](trust.md)).
- A cross-workspace sender is not in the recipient's registry → resolves as `unknown_sender` → refused to `inbox.refusals.jsonl` (the envelope is preserved, never deleted).
- So with `BWOC_TRUST_GATING=1`, cross-workspace messages from unknown senders are **refused by default** — exactly the conservative posture you want before identity is provable. Without `=1` (the framework default is warn-only gating), the quality gate never refuses them.

Routing therefore does **not** block on [Trust v2](trust.md): the two features are orthogonal and their interaction is correct as-is.

> [!warning] Seam left for Trust v2. The envelope `from` is a bare id (`agent-oracle`). Across workspaces a bare id is ambiguous and unprovable. v2 should introduce a workspace-qualified, signed identity (e.g. `agent-oracle@ws-b`) so cross-workspace senders can be *trusted* rather than only *refused*. v1 keeps `from` bare and marks this seam — do not widen the envelope schema for routing alone.

## What This Spec Does NOT Cover

- **Network transport.** Local-filesystem peers only. ssh/http/queue transports are deferred (Trust v2 cross-workspace).
- **Discovery.** No automatic peer discovery, broadcast, or gossip. Peers are declared by hand.
- **Cross-workspace trust.** Routing delivers; *trusting* a cross-workspace sender is Trust v2 (see seam above). Until then, cross-ws senders are strangers (refused under `BWOC_TRUST_GATING=1`).
- **Loop / cycle protection.** v1 does a single hop (local → one peer). Multi-hop forwarding (A→B→C) is out of scope; a route resolves to a terminal workspace, not another routing table.
- **Routing the read side.** `bwoc inbox` still reads the local agent's own inbox. Routing governs `send` (delivery), not cross-workspace reads.

## Spec Revision History

- **v1 / 2026-05-23 (initial draft, Oracle):** `routes.toml` schema (`agent` | `namespace` → peer `workspace`), additive 3-step resolution, local-FS v1 scope, trust-composition safe-default, Trust v2 seam. Mapping (Anattā / SN 22.59) confirmed by the operator 2026-05-23.

## Implementation Order (when code work begins)

1. `bwoc-core`: a `Routes` type that deserializes `routes.toml` (`Vec<Route>`; each `Route` is `agent` xor `namespace`, plus `workspace`). Absent file → empty routes. Validation: reject a route with both/neither key.
2. `send.rs`: after the local-registry miss (between [`send.rs:99`](../../../crates/bwoc-cli/src/send.rs) and the `NotFound` return), consult routes; on a peer hit, load the peer registry and retarget `inbox_path`. Local hit path untouched.
3. Tests: local hit unchanged; exact-agent peer route; namespace prefix route; both-keys validation error; no-match `NotFound`; trust-gated peer send → `unknown_sender` refusal at recipient.
4. CHANGELOG row + ROADMAP cross-reference + bilingual TH parity (`routing.th.md` mirrors this file).

Each step is mergeable independently. Step 2 is the only one touching live `send` behaviour and must keep the local path byte-for-byte identical.

## Cross-References

- [`PHILOSOPHY.en.md` #4 Tilakkhaṇa](../docs/en/PHILOSOPHY.en.md) — Anattā, the principle this spec embodies (no central self → no central broker).
- [`trust.md`](trust.md) — the recipient-side gate routing composes with; the `unknown_sender` refusal is what makes cross-ws safe by default.
- [`messaging.md`](messaging.md) — the sender-identity envelope (`from`) that routing carries across the boundary.
- [`sangha.md`](sangha.md) — shared task list; a peer workspace's tasks are reachable only once routing lands.
- SN 22.59 Anattalakkhaṇa Sutta — canonical source ([SuttaCentral](https://suttacentral.net/sn22.59)).
