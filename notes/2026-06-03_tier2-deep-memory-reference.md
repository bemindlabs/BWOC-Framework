# 2026-06-03 — Tier 2 deep-memory reference implementation (`bwoc-deep-memory`)

Shipped the reference implementation of the Tier 2 "deep memory" backend — the
last item deferred off the Phase 3 DoD. The pluggable *interface*
(`bwoc-core::deep_memory`) already existed and was fully wired
(`bwoc memory wake-up|search|mine`, `bwoc new --deep-memory-cmd`, help, check);
what was missing was a concrete tool that speaks the contract so a fresh
`--deep-memory-cmd <tool>` works out of the box. This note records that tool.

## What changed

- **New crate `crates/bwoc-deep-memory`** → binary `bwoc-deep-memory`, a
  self-contained tool implementing the three-verb contract from
  `bwoc-core::deep_memory`:
  - `wake-up [--limit N]` — emit the N most recent memories for session-start injection.
  - `search "<query>" [--limit N]` — embed the query, return top-N by cosine similarity.
  - `mine <path> --mode <m>` — walk session files under `<path>`, chunk, embed, store.
- **Storage** (`store.rs`): a single SQLite file (`rusqlite` `bundled` — SQLite
  compiled from source, no system libsqlite3, keeps the CI matrix green).
  Embeddings stored as little-endian `f32` BLOBs; ranking is **brute-force
  cosine in Rust**.
- **Embedding seam** (`embed.rs`): `Embedder` trait with `HttpEmbedder` (a
  self-contained `reqwest` blocking client for any OpenAI-compatible
  `POST /v1/embeddings` — Ollama, llama.cpp, vLLM, OpenAI) and a deterministic
  `StubEmbedder` (FNV-hash bag-of-words) so `cargo test` never touches the network.
- **Ingestion** (`mine.rs`): recursive walk of text extensions
  (`md/txt/jsonl/json/log`), 5 MiB per-file cap, paragraph-bounded chunking at
  ~1200 chars with char-boundary-safe hard-split for over-long paragraphs.
- **Config resolution** (`main.rs`): flags > env (`BWOC_DEEP_MEMORY_DB`,
  `BWOC_EMBED_URL`, `BWOC_EMBED_MODEL`, `BWOC_EMBED_API_KEY`) > defaults.
- 20 unit tests; `bwoc-core` untouched (heavy deps `rusqlite` + `reqwest`
  quarantined in the new crate per the dep-quarantine HARD RULE).
- ROADMAP (EN + TH) updated: Tier 2 reference impl marked shipped under "Shipped
  beyond Phase 3"; nothing now remains deferred off the Phase 3 DoD.

## Decisions

- **Brute-force cosine, not `sqlite-vec` (Anattā).** The user chose semantic
  recall over keyword FTS. Rather than static-link the `sqlite-vec` C extension
  (a Windows-CI build wrinkle), v1 stores vectors as BLOBs and scores in Rust.
  For a single agent's memories this is trivially fast, and the `Store` public
  surface is unchanged when `sqlite-vec` k-NN swaps in later — no clinging to
  the v1 storage detail.
- **Injectable `Embedder` seam (Yoniso manasikāra / testability).** Mirrors the
  existing `RunnerFn` injection in `bwoc-core::deep_memory` — verb logic is
  tested against canned vectors offline, the HTTP path is exercised only by the
  real binary smoke test.
- **Self-contained client, not reuse of `bwoc-harness/provider` (Mattaññutā /
  dep-quarantine).** The harness has an OpenAI-compatible client but only for
  `/v1/chat/completions`, and importing it would pull the whole harness
  (tokio/sandbox/mcp) for one embed call. Copied the pattern (reqwest + rustls),
  not the dependency.
- **Dimension-mismatch rows are skipped, not mis-scored.** If the embedding
  model changes, old vectors of a different length score `NaN` and drop out of
  results rather than corrupting the ranking.

## Alternatives considered

- **SQLite FTS5 (keyword/BM25)** — leaner (no embed source) but adds little over
  Tier 1; rejected because Tier 2's whole point is cross-session semantic recall.
- **`sqlite-vec` static-linked** — real ANN, but native-ext build risk on the
  Windows CI leg for no payoff at single-agent scale; deferred behind the seam.

## Status / deferred

- v1 scope: single-agent local store, semantic recall only. **Deferred until
  demand:** `sqlite-vec` k-NN, re-ranking, multi-agent shared store, eviction
  policy, incremental/dedup mining.
- Verified locally: `cargo test -p bwoc-deep-memory` (20 pass), fmt + clippy
  (`-D warnings`) clean, and a real `mine`→`search`→`wake-up` round-trip against
  a local Ollama `/v1/embeddings` (search correctly ranked the relevant chunk
  above an unrelated one).

## Related

- Interface: `crates/bwoc-core/src/deep_memory.rs`, `crates/bwoc-cli/src/deep_memory_cmd.rs`
- Roadmap: `docs/en/ROADMAP.en.md` (+ `docs/th/ROADMAP.th.md`) — "Shipped beyond Phase 3"
- Prior sequencing note: `notes/2026-05-23_phase3-remaining-sequencing.md`
