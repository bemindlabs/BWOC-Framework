# bwoc-deep-memory

The Tier 2 **deep-memory** reference implementation for the [BWOC framework](../../README.md) — a standalone binary giving an agent semantic recall across sessions.

Tier 2 is optional: the framework runs on Tier 1 (file-based memory) alone. This crate is what you point `deepMemoryCmd` at when you want cross-session semantic recall without writing your own backend. It speaks the `wake-up | search | mine` sub-command contract defined by [`bwoc-core::deep_memory`](../bwoc-core/src/deep_memory.rs), which [`bwoc-cli`](../bwoc-cli/) shells out to for `bwoc memory wake-up | t2-search | mine`. **Dep-quarantine is load-bearing** — `rusqlite` (SQLite, `bundled`), `regex`, and `reqwest` are pulled in here, never in `bwoc-core`. The crate splits into a library (unit-tested offline against a stub embedder and an in-memory store) and a thin `main.rs` that only parses args and resolves config.

## Scope

- **`store`** — the SQLite file. One `memories` table (`source`, `text`, `mode`, `ts`, `embedding` as a little-endian `f32` BLOB) with a `ts` index. `insert` / `recent` / `search` / `prune` / `count`. Ranking is brute-force cosine in Rust; rows whose stored dimension differs from the query (model changed, corrupt BLOB) score `NaN` and are skipped rather than mis-ranked.
- **`embed`** — the `Embedder` trait plus `HttpEmbedder` (`POST {base}/v1/embeddings`, 60s timeout, optional bearer auth) and `StubEmbedder`, a deterministic FNV-1a bag-of-words hash so `cargo test` never touches the network.
- **`mine`** — walks a file or directory for `.md/.txt/.jsonl/.json/.log`, skips files over 5 MiB, and splits bodies into paragraph-bounded chunks capped at 1200 chars (over-long paragraphs hard-split on char boundaries).
- **`redact`** — scrubs secrets out of chunk text *before* it is embedded or stored, so the store never becomes a secret sink. Precision-biased rules: PEM private-key blocks, `key = value` assignments with secret-ish keys, AWS `AKIA…`, GitHub `gh[pousr]_…`, `sk-…`, Slack `xox[baprs]-…`, and JWTs.
- **`lib` verbs** — `mine`, `search`, `wake_up`, and `prune` take a `&Store` (plus a `&dyn Embedder` for the two that embed), and `render` formats the resulting `Memory` list, so every verb is testable with no network and no disk.

## Usage

A binary, not a workspace dependency:

```bash
# Ingest session artifacts (a file or directory) into the store.
bwoc-deep-memory --db agent/.bwoc/deep.db \
  --embed-url http://localhost:11434 --embed-model nomic-embed-text \
  mine ./sessions --mode convos

# Semantic search / session-start context.
bwoc-deep-memory --db agent/.bwoc/deep.db search "which TLS library did we pick" --limit 5
bwoc-deep-memory --db agent/.bwoc/deep.db wake-up --limit 10

# Retention (operator/cron, not part of the recall contract).
# The two rules combine as a union; at least one is required.
bwoc-deep-memory --db agent/.bwoc/deep.db prune --older-than-days 90 --keep 5000 --dry-run
```

Wire it into an agent's `config.manifest.json`:

```text
deepMemoryCmd = "bwoc-deep-memory --db agents/agent-foo/.bwoc/deep.db \
                 --embed-url http://localhost:11434 --embed-model nomic-embed-text"
```

Flags take precedence over environment variables over defaults:

| Flag | Env | Default |
|---|---|---|
| `--db` | `BWOC_DEEP_MEMORY_DB` | `.bwoc/deep-memory.db` |
| `--embed-url` | `BWOC_EMBED_URL` | `http://localhost:11434` |
| `--embed-model` | `BWOC_EMBED_MODEL` | `nomic-embed-text` |
| — | `BWOC_EMBED_API_KEY` | (none; sent as `Authorization: Bearer` when set) |

The embedding endpoint is any OpenAI-compatible `POST /v1/embeddings` — Ollama, llama.cpp, vLLM, OpenAI, or a gateway.

## Status

Working tool, not a stub. All four sub-commands (`wake-up`, `search`, `mine`, `prune`), mining with secret redaction, and the cosine-ranked store ship today. The `Store` surface is deliberately stable so a `sqlite-vec` k-NN backend can replace brute-force scoring without touching callers. Still deferred: ANN indexing, re-ranking, multi-agent shared stores, and incremental/dedup mining.

## License

[MIT](../../LICENSE).
