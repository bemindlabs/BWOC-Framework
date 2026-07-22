---
title: Server RAG
aliases:
  - server-rag
tags:
  - group/framework-skills
  - type/skill
  - domain/knowledge
maturity: L1
---

# Server RAG

> [!abstract] The fleet's *second* memory system, alongside [[../second-brain/SPEC|second-brain]]. Where the Second Brain is a **structured graph** you query by term, the Server RAG is a **semantic Q&A** endpoint (embeddings + vector store) over the host's operational docs — ask it a natural-language question about the server/ops and get an answer with sources. Remember-first / Yoniso Manasikāra for "how does this machine actually work."

## What This Skill Does

Operational facts — which port a service is on, how a container is wired, where a credential lives, what a past incident concluded — are scattered across notes, READMEs, and configs. The Server RAG ingests them into a local, fully self-hosted retrieval endpoint (embeddings + a vector store, no cloud) so an agent can **ask instead of grep**.

Two operations are exposed:

- **`ask_rag(question)`** — POST a natural-language question to the RAG query endpoint; get `{ answer, sources }`. The answer is grounded in the ingested docs; `sources` name the chunks it drew from (verify before trusting — the RAG can say "I don't have that").
- **`refresh_rag()`** — re-collect + re-ingest the host's docs after the system changes, so a query reflects current reality (secret-scanned on the way in).

## Why It Exists

The Second Brain graph answers "what nodes relate to X" across the fleet; the Server RAG answers "explain X about this host" in prose. They are complementary: the graph is precise + structural, the RAG is fuzzy + explanatory. Centralising the "how do I ask it" recipe as a skill keeps an agent from re-deriving the endpoint contract, and a stale answer is one `refresh_rag` away. Fully local (no cloud egress) keeps it inside the fleet's trust boundary.

## Where The RAG Lives

| Surface | Location |
|---|---|
| Query endpoint | `POST <ragQueryUrl>` — body `{ "question": "<free text>", "top_k": <n> }` → `{ answer, sources }` |
| Ingest / refresh | `<ragRoot>/scripts/refresh-*.sh` — collect + secret-scan + ingest into the vector store |

`<ragQueryUrl>` and `<ragRoot>` are **operator-configured** for the environment (the endpoint is a loopback/tailnet HTTP service; the store is local). The contract is a plain JSON POST, so any host that can reach the endpoint can consult it.

> [!warning] Endpoints drift — an internal service can move ports (e.g. a fleet-wide "internal ≥ 10000, bind 127.0.0.1" migration). Resolve `<ragQueryUrl>` from current infra notes, not a memorised port; a dead port means "look it up / `refresh_rag`," not "the RAG is gone."

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `ask_rag` | `question` (free text), optional `top_k` | Read-only: POST to the query endpoint, return `{ answer, sources }` | Pure read — repeatable, no side effects |
| `refresh_rag` | — | Re-collect + re-ingest host docs into the vector store | Idempotent: a re-run with no doc change reproduces the same index |

`ask_rag` is a free **read** (Dhammānupassanā — no gate). `refresh_rag` writes only the local vector store (no external system) → no operator-confirm gate, the same reasoning the `okr track` local write follows. Both stay inside the host — no cloud egress.

## Lifecycle Mapping

```
init       → resolve <ragQueryUrl> / <ragRoot> from current infra notes
invoke     → ask_rag before deriving an ops fact; refresh_rag when the answer is stale/empty
teardown   → no-op (the vector store is a shared artifact, not skill-scoped state)
```

Holds no state between invocations. Replay-safe.

## When To Use Which

| Question shape | Reach for |
|---|---|
| "What relates to / who touched / where is X across the fleet" (structural) | [[../second-brain/SPEC\|second-brain]] (`query_brain`) |
| "Explain how X on this host works / what did we decide about Y" (prose) | this skill (`ask_rag`) |

On a miss in one, try the other — they index overlapping but differently-shaped knowledge.

## Maturity

Declared **L1** — first use, unverified across backends. Bumps to L2 once two agents have used `ask_rag` to reuse an operational fact end-to-end; to L3 once `bwoc skill verify server-rag` is wired and green in CI.

## Neutrality

Manifest names no backend, model, or vendor CLI; `<ragQueryUrl>` / `<ragRoot>` are operator-configured, not hardcoded hosts. The verify command is a framework command (`bwoc skill verify`). Satisfies the **Samānattatā** rule enforced by `bwoc check`.

## See Also

- [[../second-brain/SPEC|second-brain]] — the fleet's structured-graph memory (this skill's sibling).
- [[../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Yoniso Manasikāra framing.
