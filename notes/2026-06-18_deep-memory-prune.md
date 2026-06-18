# 2026-06-18 — deep-memory `prune` (retention/TTL) + the sqlite-vec ANN call

Task #11 asked for two things on `bwoc-deep-memory`: **retention/TTL prune** and
**sqlite-vec ANN**. This ships the first and makes a deliberate call on the
second.

## What changed

- **`bwoc-deep-memory prune`** — an operator/maintenance verb to bound store
  growth. Two retention rules, applied as a **union** (a row is pruned if it
  matches either):
  - `--older-than-days N` → drop rows with `ts < now - N·86400` (TTL by age).
  - `--keep N` → keep only the newest `N` rows (by `ts` desc, `id` desc), drop
    the rest (cap by count).
  - `--dry-run` reports the exact count a real run would remove, deletes
    nothing. At least one rule is required (no-rule is a hard error, never an
    accidental table wipe). Deletes run in one transaction.
- `Store::prune(older_than, keep_newest, dry_run) -> i64` (store.rs) + a thin
  `prune(...)` verb wrapper (lib.rs) + the `Prune` CLI subcommand (main.rs).
- 5 store tests (older-than, keep-newest, dry-run, union-dedup, no-rule no-op).

## Decisions

- **`prune` is an operator verb, not part of the recall contract.** The
  agent-facing `bwoc-core::deep_memory` contract is `wake-up | search | mine` —
  the three things an agent calls *during* a session. Retention is run by
  cron/maintenance, never mid-session, so it stays out of the `DeepMemory`
  trait. The contract stays lean (Mattaññutā); no neutrality surface touched.
- **Union, not intersection, of the two rules.** An operator typically wants
  "drop anything older than 90 days **and also** never let the store exceed
  5000 rows" — both ceilings active at once. Intersection would only prune rows
  satisfying both, defeating either cap alone.

## sqlite-vec ANN — deferred, deliberately (not dropped)

Held off on swapping brute-force cosine for a `sqlite-vec` k-NN index this PR:

- **No current benefit.** Search loads every row and scores in Rust. For a
  single agent's store (hundreds–low-thousands of vectors) that's sub-
  millisecond. ANN indexes only pay off at ~10⁵⁺ vectors — a scale a per-agent
  memory store does not reach.
- **Real cost.** `sqlite-vec` is a native C extension. Adopting it means
  enabling rusqlite loadable-extension loading (a security-sensitive surface in
  a security framework) and bundling/building the extension across the
  macOS/Linux/Windows CI matrix. That's a standing maintenance burden for a
  *reference* crate.
- **The seam already exists.** store.rs's header documents the swap as a future
  drop-in "without changing the public surface" (Anattā). Nothing here closes
  that door; `prune` keeps the store small enough that brute force stays fast.

Recommendation: adopt `sqlite-vec` only behind an off-by-default cargo feature,
and only once a real deployment shows a store large enough to need it. Tracked
as a separate concern (one concern per PR).

## Status / deferred

- Shipped: `prune`. Deferred with rationale: sqlite-vec ANN.

## Related

- task #11; `crates/bwoc-deep-memory/src/{store,lib,main}.rs`;
  `crates/bwoc-core/src/deep_memory.rs` (the unchanged recall contract).
