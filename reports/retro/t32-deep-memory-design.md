---
title: t32 deep-memory (sqlite-vec / governance) — DEFERRED
date: 2026-06-12
tags:
  - type/design
  - area/deep-memory
  - phase/6
status: deferred
---

# t32 — deep-memory sqlite-vec / governance — **DEFERRED**

**Decision (owner, 2026-06-12): do not implement now — premature.** This note
records the investigation so the work is resumable, not lost.

## What already exists

`crates/bwoc-deep-memory/` is a working Tier-2 reference backend (the thing you
point `deepMemoryCmd` at):

- Speaks the backend-neutral contract from `bwoc-core::deep_memory`
  (`wake-up` / `search "<q>"` / `mine <path> --mode <m>`).
- Embeddings via an OpenAI-compatible `/v1/embeddings` endpoint (`embed.rs`).
- Local store on `rusqlite` with **bundled** SQLite (`store.rs`); vectors are
  stored as little-endian `f32` BLOBs.
- Recall is **brute-force cosine in Rust** — `store.rs` loads every row and
  ranks. Its own doc says this is "trivially [fine] for a single agent's
  memories."

## Why defer

### sqlite-vec (the ANN upgrade)
- The store's brute-force scan is adequate at the current scale (one agent's
  memories). Replacing it with the `sqlite-vec` extension is an optimisation
  with **no demonstrated need** yet — classic YAGNI / Mattaññutā.
- `sqlite-vec` is a **C extension**. The crate deliberately uses *bundled*
  SQLite for cross-platform portability; adding a loadable C extension reopens
  the cross-platform CI question (Windows especially). Not worth that cost until
  there is a real multi-thousand-memory scale need.

### governance (retention / redaction / isolation)
- **Per-agent isolation is already structural** — each agent points at its own
  `agents/agent-<id>/.bwoc/deep.db` file; there is no shared store to leak
  across. No code needed.
- **Retention/TTL + prune** and **secret redaction on `mine`** are real future
  value (the redaction piece is the security-aligned one — don't let the memory
  store become a secret sink), but there is no current pressure forcing them.

## If/when resumed — recommended first slice

1. **Redaction on `mine`** (security-aligned): scan mined text for
   secrets/credentials and drop/redact before `store.insert`. Highest value,
   no CI risk.
2. **Retention / TTL**: `prune --max-age <days>` subcommand (+ optional
   auto-prune on `mine`).
3. **sqlite-vec**: only once a real scale need appears, and only after settling
   the Windows-CI story for the C extension.

Per-agent isolation needs only a verifying test + a doc line, not new code.

## Status

Investigated, **not implemented**. Phase 6 closes with t29–t31; t32 stays parked
here until the owner revisits.
