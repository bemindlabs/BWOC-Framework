# bwoc-deep-memory

Tier 2 **deep-memory** reference implementation for BWOC — a self-contained
tool that speaks the backend-neutral contract defined by
[`bwoc-core::deep_memory`](../bwoc-core/src/deep_memory.rs) over a local SQLite
store with **semantic (embedding) recall**.

Tier 2 is optional. The framework runs on Tier 1 (file-based memory) alone; this
crate is what you point `deepMemoryCmd` at when you want cross-session semantic
recall without writing your own backend.

## The contract

Any tool wired into an agent via `deepMemoryCmd` in `config.manifest.json` must
speak three sub-commands:

```text
<cmd> wake-up                  session start: emit prior context to stdout
<cmd> search "<query>"         find relevant past memories
<cmd> mine <path> --mode <m>   session end: persist learnings under <path>
```

`bwoc-deep-memory` is the reference that implements them.

## Usage

```bash
# Ingest session artifacts (a file or directory) into the store.
bwoc-deep-memory --db agent/.bwoc/deep.db \
  --embed-url http://localhost:11434 --embed-model nomic-embed-text \
  mine ./sessions --mode convos

# Semantic search.
bwoc-deep-memory --db agent/.bwoc/deep.db search "which TLS library did we pick" --limit 5

# Session-start context (most recent memories).
bwoc-deep-memory --db agent/.bwoc/deep.db wake-up --limit 10
```

Wire it into an agent:

```text
deepMemoryCmd = "bwoc-deep-memory --db agents/agent-foo/.bwoc/deep.db \
                 --embed-url http://localhost:11434 --embed-model nomic-embed-text"
```

## Configuration

Flags take precedence over environment variables over defaults:

| Flag | Env | Default |
|---|---|---|
| `--db` | `BWOC_DEEP_MEMORY_DB` | `.bwoc/deep-memory.db` |
| `--embed-url` | `BWOC_EMBED_URL` | `http://localhost:11434` |
| `--embed-model` | `BWOC_EMBED_MODEL` | `nomic-embed-text` |
| — | `BWOC_EMBED_API_KEY` | (none; sent as `Authorization: Bearer` when set) |

The embedding endpoint is any OpenAI-compatible `POST /v1/embeddings`
(Ollama, llama.cpp, vLLM, OpenAI, a gateway, …).

## Design

- **Store** — single SQLite file (`rusqlite` `bundled`; no system libsqlite3).
  Embeddings are `f32` BLOBs; v1 ranks by **brute-force cosine** in Rust. The
  `Store` surface is stable, so a `sqlite-vec` k-NN backend can swap in later
  without touching callers.
- **Embedding seam** — the `Embedder` trait has an `HttpEmbedder` for production
  and a deterministic `StubEmbedder` so the verb logic is unit-tested offline.
- **Dep-quarantine** — `rusqlite` and `reqwest` live here, never in `bwoc-core`.

## Non-goals (v1)

`sqlite-vec` ANN, re-ranking, multi-agent shared stores, eviction policies, and
incremental/dedup mining are deferred until demanded.
