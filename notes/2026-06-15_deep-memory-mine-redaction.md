# 2026-06-15 — deep-memory `mine` secret redaction (t32 security slice)

Scrub credentials out of mined chunk text **before** it is embedded and stored,
so `bwoc-deep-memory`'s SQLite store never becomes a secret sink. This is the one
slice of the deferred t32 work that the deferral retro flagged as worth doing now
(security-aligned, no CI risk) — see `reports/retro/t32-deep-memory-design.md`.

## What changed

- New `crates/bwoc-deep-memory/src/redact.rs`: a small `redact(text) -> (String,
  usize)` over a `OnceLock`-compiled ruleset. Rules: PEM private-key block,
  `key=value` assignments with secret-ish keys (value-only replacement, keeps the
  `key = ` prefix + opening quote), AWS access-key id, GitHub tokens, `sk-` keys,
  Slack tokens, JWT.
- `lib.rs::mine`: redact every chunk before embedding; the **redacted** text is
  what gets embedded AND inserted, so a secret never reaches the embedding
  endpoint or the DB. `MineReport` gains a `redacted: usize` count.
- `main.rs`: prints `redacted N secret(s) before storing` when N > 0.
- New dep `regex` (workspace + crate). Pure-Rust — deliberately **not** the
  sqlite-vec C-extension that t32 parked over the Windows-CI question. Quarantined
  to `bwoc-deep-memory`, never `bwoc-core`.

## Decisions

- **Precision over recall.** Every pattern is anchored to a high-signal shape
  (known prefixes, or explicit `key: value`). A false positive corrupts a real
  memory, which is worse than missing an exotic secret — the agent's own memory
  must stay trustworthy (Mattaññutā: redact the right amount, not everything).
- **Redact before embed, not just before store.** Embedding a secret would leak
  it to the embedding endpoint and bake it into the vector. Redacting first
  closes both.
- **`regex` over a hand-rolled scanner.** Already in the lockfile, pure-Rust, no
  cross-platform cost. The `key=value` rule in particular is impractical to
  hand-roll safely.

## Alternatives considered

- Hand-rolled byte scanners (no new dep) — rejected: the assignment rule is
  error-prone without regex, and `regex` adds no C-extension / Windows-CI risk.
- Redact at `store.insert` instead of in `mine` — rejected: would still embed the
  raw secret before the store sees it.

## Status / deferred

Done: redaction + tests (7 unit + 1 mine integration; 29 crate tests green, fmt +
clippy clean). Still parked from t32 (no current pressure): retention/TTL prune,
sqlite-vec ANN. Per-agent isolation remains structural (`.bwoc/deep.db` per agent).

## Related

- `reports/retro/t32-deep-memory-design.md` (deferral record + recommended first slice)
- `crates/bwoc-deep-memory/src/redact.rs`
