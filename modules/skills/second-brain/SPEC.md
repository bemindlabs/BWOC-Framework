---
title: Second Brain
aliases:
  - second-brain
tags:
  - group/framework-skills
  - type/skill
  - domain/knowledge
maturity: L1
---

# Second Brain

> [!abstract] Remember-first, fleet-wide. Before an agent re-derives something, it consults the **Second Brain** — a harvested knowledge graph of the whole fleet (every workspace, repo, commit, branch, PR, issue, memory, note, agent, domain, container) — and reuses what the fleet already knows. Encodes **Yoniso Manasikāra** ("verify against what exists") across *all* projects, not just the local repo.

## What This Skill Does

An agent's own `MEMORY.md` covers *its* history; a repo's git log covers *one* project. The Second Brain covers the **whole fleet at once** — so an agent can find that another agent already solved a problem, that a decision was recorded in a sibling project, or that a memory in a different workspace answers the question — before spending a turn rediscovering it.

Two operations are exposed:

- **`query_brain(topic)`** — search the graph for nodes whose label/description match `topic`, ranked by connectivity (`degree`), and read their text. Returns the most relevant prior knowledge across every node kind (`mem` / `note` / `commit` / `issue` / `pr` / `repo` / `domain`).
- **`refresh_brain()`** — regenerate the graph from the live fleet (git + memories + docker + registries) so a query reflects current reality.

## Why It Exists

The fleet's knowledge is otherwise siloed: 500+ memories, hundreds of notes and commits, dozens of workspaces — each discoverable only from inside its own project. The Second Brain harvests them into one queryable graph so **Sīlasāmaññatā** (shared conventions) and **Yoniso Manasikāra** (check current reality first) apply *across* the fleet. Centralising the "how do I consult it" recipe as a skill means an agent doesn't re-derive the query path each time, and a stale answer is one `refresh_brain` away.

## Where The Brain Lives

| Surface | Location |
|---|---|
| Data (the graph) | `<secondBrainRoot>/projects/brain-web/src/data/graph.json` — `{ generatedAt, stats, nodes[], links[] }` |
| Browsable UI | the Second Brain web app (a Vite SPA rendering the graph as a neural network) |
| Harvester | `<secondBrainRoot>/projects/brain-web` → `npm run harvest` (local) · `npm run harvest:all` (+ remote) · `npm run harvest:full` (+ audit) |

A **node** is `{ id, label, group, kind, description, degree }` (memory/note nodes also carry their full `text`); a **link** connects two node ids. `kind` ∈ `mem · note · commit · repo · agent · branch · tag · domain · pr · issue · ws · dock`. Configure `<secondBrainRoot>` per the operator's environment; the graph is a plain JSON file, so any host that can read it can query it.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `query_brain` | `topic` (free text) | Read-only: filter `nodes[]` by `label`/`description`/`text` match, rank by `degree`, follow `links[]` for context | Pure read — repeatable, no side effects |
| `refresh_brain` | — | Re-run the harvester to rebuild `graph.json` from the live fleet | Idempotent: a second run with no fleet change reproduces the same graph (modulo `generatedAt`) |

`query_brain` is observed by **Dhammānupassanā** (it is a *read* — free, no gate); `refresh_brain` touches only the local `graph.json` artifact (no external system), so like the `okr track` local write it carries **no** operator-confirm gate.

### Recipe (reference)

Query by term over label+description (highest-degree first), read the matches' text, then widen via links. A `jq` filter over `graph.json` is the backend-neutral baseline; the web UI is the human path. On zero matches, `refresh_brain` and retry before concluding the fleet has no prior art.

## Lifecycle Mapping

```
init       → resolve <secondBrainRoot> (the harvested graph.json) for this environment
invoke     → query_brain before deriving; refresh_brain when the answer looks stale
teardown   → no-op (the graph is a shared artifact, not skill-scoped state)
```

The skill holds no state between invocations. Replay-safe.

## Maturity

Declared **L1** — first use, unverified across backends. Bumps to L2 once two agents have used `query_brain` to reuse prior fleet knowledge end-to-end; to L3 once `bwoc skill verify second-brain` is wired and green in CI.

## Neutrality

Manifest values name no backend, model, or vendor CLI; `<secondBrainRoot>` is an operator-configured path, not a hardcoded host. The verify command is a framework command (`bwoc skill verify`). Satisfies the **Samānattatā** rule enforced by `bwoc check`.

## See Also

- [[../server-rag/SPEC|server-rag]] — the fleet's *semantic* memory (ask a question, get prose + sources); this skill's sibling. Graph vs RAG: query structure here, ask "explain X" there.
- [[../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
- [[../../modules/agent-template/AGENTS|agent-template AGENTS.md]] — Remember-first / Yoniso Manasikāra in the base profile.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Yoniso Manasikāra, Sīlasāmaññatā framing.
