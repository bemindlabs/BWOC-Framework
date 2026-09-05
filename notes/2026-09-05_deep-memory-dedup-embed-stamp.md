# 2026-09-05 — deep-memory: insert dedup + embedding-model stamp

Closes #482. `bwoc-deep-memory` had two silent-corruption modes:

1. **No dedup.** `store.rs` had no `UNIQUE` / `INSERT OR IGNORE`, and the full
   `chat-session.json` is re-mined at every session end → duplicates grew
   **linearly per resume**. Prune/keep-newest is meaningless while duplicates
   accumulate.
2. **No embedding-model stamp.** The only staleness guard was a dimension
   mismatch, so switching between two **same-dimension** models (768↔768) silently
   mis-ranked every stored memory.

## What changed

- **Dedup**: a `UNIQUE(source, text)` index + `INSERT OR IGNORE`. `insert` now
  returns `bool` (written vs skipped); `mine` counts skips into a new
  `MineReport.skipped`, printed by the CLI. `init` migrates a pre-#482 store —
  collapses existing duplicates (keep `MIN(id)` per `(source, text)`) before
  creating the unique index.
- **Model stamp**: new `embed_model` column, stamped at insert from
  `Embedder::model_id()` (added to the trait, default `""`; `HttpEmbedder` reports
  its configured model). `search` takes the current model and excludes rows
  stamped with a *different* non-empty model in SQL; unstamped legacy rows and
  same-model rows are kept. The existing dimension check still drops mismatches.

## Decisions

- **Redaction stays BEFORE insert** — unchanged. BWOC is ahead of Grok here (no
  equivalent scrubbing exists in `xai-grok-memory`); this must not regress.
- **Legacy `''` rows are kept, not purged.** We can't know their model, so
  excluding them would nuke recall after an upgrade; keeping them matches prior
  behaviour (no regression) while new rows are stamped and age in.
- Confined to `bwoc-deep-memory` (already owns `rusqlite`) — dep-quarantine
  intact, the 3-verb contract unchanged, backend-neutral (the stamp is an opaque
  operator string).

## Tests

- `store`: `insert_dedups_same_source_text`, `search_filters_out_a_different_embed_model`.
- `lib`: `re_mining_the_same_path_dedups` (second `mine` stores 0, skips all, store size unchanged).

## Related

- Issue #482; source `research/2026-08-23_grok-build-comparison.md`.
