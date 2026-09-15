---
title: Memory Tier 2 No-Op
aliases:
  - memory-tier2-noop
tags:
  - group/framework-plugins
  - type/plugin
  - kind/memory-backend
  - domain/memory
maturity: L1
---

# Memory Tier 2 No-Op

> [!abstract] First reference framework plugin. A `memory-backend` stub that satisfies the Tier 2 contract by **forwarding every call to Tier 1** instead of standing up a vector store. Lands together with the loading mechanism so the format can be proved end-to-end before any real backend is chosen.

## What This Plugin Does

Implements the four-phase `memory-backend` lifecycle (`init → configure → invoke → teardown`) without holding any state of its own:

- **`init`** — confirms it can run. No directories created, no handles opened. Returns success.
- **`configure`** — accepts an empty config block (no `[config.schema]` keys); reports success. Re-runs are no-ops because nothing was applied.
- **`invoke`** — every Tier 2 read/write is **forwarded to Tier 1**. A `wake-up` call returns whatever Tier 1's `MEMORY.md` index would surface; a write call appends to the same Tier 1 store; a `search` call (`bwoc memory search --tier 2`) performs a substring search across Tier 1 memory files. No semantic / vector layer.
- **`teardown`** — releases nothing. Returns success.

Idempotency is trivially satisfied at every phase because the plugin owns no external state.

## Why It Exists

Two reasons, both stated in `BWOC-7` and `BWOC-EPIC-1`:

1. **Prove the loading mechanism without committing to a vector store.** Choosing a real Tier 2 backend (Qdrant, LanceDB, Chroma, Pinecone, …) is a separate, much larger decision. The plugin contract — `manifest.toml`, `workspace.toml [plugins.<name>]` entry, lifecycle dispatch — must be exercised first so the eventual vector-store plugin only has to fill in `invoke`.
2. **Give the agent's memory subsystem a default that always works.** Until a real Tier 2 lands, agents that name `memory-tier2-noop` in their workspace get Tier 1 behaviour transparently — no "Tier 2 unavailable" branch in agent code, no second code path to maintain. **Anattā**: the absence of a real backend is not an error state, it is the resting state.

The no-op is **deliberately useless on its own** — its value is the contract it exercises, not the function it performs.

## Configuration

```toml
# workspace.toml
[plugins.memory-tier2-noop]
enabled = true
```

That is the entire config surface. The plugin declares no `[config.schema]` block in its manifest, so the framework will refuse any plugin-specific key beyond the universal `enabled`. This matches the spec note in [[../../docs/en/PLUGINS.en|PLUGINS.en.md §Manifest]]: *"omit the table entirely if the plugin takes no config."*

## Lifecycle Mapping

Per [[../../docs/en/PLUGINS.en|PLUGINS.en.md §Lifecycle]], the four phases dispatched by the agent's memory subsystem (the `memory-backend` lifecycle owner):

| Phase | What this plugin does | Exit / Result |
|---|---|---|
| `init` | Confirm the plugin can run. No filesystem touches. | `Ok` |
| `configure` | Validate the (empty) config block; report ready. | `Ok` |
| `invoke` | Route the call to Tier 1: a read returns Tier 1's view; a write appends to `memories/`; a search greps Tier 1 files. | `Ok` with Tier 1's payload |
| `teardown` | Nothing to release. Safe to call repeatedly. | `Ok` |

Pseudocode for the forwarding `invoke`:

```text
invoke(op, args):
  match op:
    "wake-up"    -> return tier1.read_index()
    "search"     -> return tier1.grep(args.query)
    "write"      -> return tier1.append(args.id, args.body)
    "mine"       -> return { status: "noop", reason: "no-op tier 2" }
    _            -> err "unsupported op: <op>"
```

Note the deliberate divergence: `mine` (the "persist learnings at session end" call) succeeds with a `noop` status rather than forwarding to Tier 1. Tier 1 already holds whatever the agent wrote — there is nothing left to mine into. A real Tier 2 plugin would embed and index here.

## Maturity

Declared **L1** — first successful use, unverified across backends. Bumps to L2 once the plugin has been loaded and exercised end-to-end by at least one agent through the framework's lifecycle dispatcher; to L3 once `bwoc plugin show memory-tier2-noop` and the `[plugins.memory-tier2-noop]` round-trip are covered by an integration test.

## Neutrality

Manifest values name no backend, model, or vendor CLI. The plugin's `kind = "memory-backend"` is the framework's own enum value, not a vendor surface. The `description` mentions "Tier 1" and "Tier 2" only — both framework-internal concepts. Satisfies the **Samānattatā** rule on plugin manifests stated in [[../../docs/en/PLUGINS.en|PLUGINS.en.md §Neutrality constraint]].

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the spec this plugin conforms to.
- [[../../modules/agent-template/AGENTS|agent-template AGENTS.md §7]] — the Tier 1 / Tier 2 memory contract this plugin forwards across.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Anattā framing for "do nothing, succeed anyway."
- [[../../docs/en/ROADMAP.en|ROADMAP.en.md]] — real Tier 2 backend selection sits after this plugin proves the contract.
