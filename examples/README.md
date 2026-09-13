# BWOC Examples

Concrete material for adopting BWOC. Three categories, smallest steps first:

| Directory | What lives here | Status |
|---|---|---|
| [`howto/`](howto/) | Short focused recipes — one task per file, end-to-end runnable | Ready |
| [`usecases/`](usecases/) | Patterns for specific real-world scenarios (one agent vs many, monorepo, polyglot, etc.) | Placeholder |
| [`showcases/`](showcases/) | Full reference agents that have been incarnated, with their persona/memories/skills filled in | Placeholder |

If you're new to BWOC, start with [`howto/first-agent.md`](howto/first-agent.md) — it gets you from `bwoc init` to a working agent in 5 minutes.

## Relation to the spec docs

These examples complement `docs/en/` — they're **how**, not **what**:

- **Spec docs** (`docs/en/PHILOSOPHY.en.md`, `INCARNATION.en.md`, `WORKSPACE.en.md`, etc.) define the framework's shape, vocabulary, and contracts.
- **Examples** (this directory) show real-world adoption patterns.

If a how-to ever contradicts the spec, the spec wins; please open an issue.

## Scope and contributions

The framework reserves `examples/` for material that's:

- **Concrete** — every command shown must actually run against the published CLI
- **Lean** — one focused topic per file; no kitchen-sink walkthroughs
- **Backend-neutral** — examples don't assume any specific backend unless the topic *is* a specific backend
- **Apolitical** — no Buddhist proselytizing; Pali terms are engineering vocabulary, not religious instruction

Contributions welcome via PR. See [`CONTRIBUTING.md`](../CONTRIBUTING.md) for the workflow.

## See also

- [`docs/en/INCARNATION.en.md`](../docs/en/INCARNATION.en.md) — formal spec for the incarnation flow
- [`docs/en/WORKSPACE.en.md`](../docs/en/WORKSPACE.en.md) — workspace layout
- [`docs/en/ROADMAP.en.md`](../docs/en/ROADMAP.en.md) — what shipped, phase by phase
- [`crates/bwoc-cli/README.md`](../crates/bwoc-cli/README.md) — full CLI surface
