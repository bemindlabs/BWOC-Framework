# 2026-05-24 — Doc-kinds extension: custom kinds + retro metrics-prefill (GH #17)

Implemented two deferred follow-ups from GitHub issue #17.

## What changed

**Feature A — workspace-declared custom doc-kinds (TODO #12)**

- `bwoc-core::doc_kind`: `DocKind` refactored from `&'static str` + `fn() -> String` to an owned struct with a `TemplateSource` enum (`Fn`, `Inline`, `File`). Built-in kinds use `Fn`; custom kinds use `Inline` or `File` from the TOML declaration.
- Added `load_custom_kinds(workspace_root)` — reads `.bwoc/doc-kinds.toml`, returns `Vec<DocKind>`, never errors (returns empty on absent/malformed).
- Added `resolve_kind(name, workspace_root)` — built-in first, then custom, else `Err(String)` listing all available kinds.
- `template_with_root(Option<&Path>)` allows `template_file`-based kinds to read their template from disk; graceful fallback when the file is missing.
- Added `bwoc doc <kind> <new|list|view>` subcommand in `bwoc-cli::main` wired through `dispatch_doc_kind_cmd`. Existing `bwoc notes/retro/research` aliases unchanged.

**Feature B — retro metrics-prefill (TODO #10)**

- `bwoc-cli::doc_cmd::cmd_new`: when kind is `retrospectives`, calls `prefill_retro_metrics(body, root)`.
- Searches `<workspace>/metrics/session-metrics.jsonl` and `<workspace>/agents/*/metrics/session-metrics.jsonl`.
- Parses JSONL with `serde_json`; sums `tasksAttempted`, `tasksCompleted`, `gatesPassed`, `gatesFailed` across all session records.
- Injects values into the `## Metrics` table placeholder rows using integer floor division for gate pass rate.
- Absent, unreadable, or malformed files are silently skipped; placeholder rows stay unchanged.

**Docs — bilingual parity**

- `docs/en/NAMING.en.md` and `docs/th/NAMING.th.md`: added row 10c for custom doc kinds; added section documenting `.bwoc/doc-kinds.toml` schema and `bwoc doc <kind>` CLI surface. Thai file verified zero U+FFFD.

## Decisions

- Built-in `DocKind` still backed by a `fn() -> String` (no allocation at call site until needed); `TemplateSource::Fn` holds the function pointer.
- `template()` is kept as a zero-arg convenience; `template_with_root` is the full version. Built-ins ignore the workspace root arg.
- Resolution order "built-in first" prevents a workspace from shadowing `notes`/`retrospectives`/`research`.
- Integer floor division for gate pass rate (14/16 → 87%) chosen over rounding for conservative reporting.
- `tempfile` added as `[dev-dependencies]` to `bwoc-core`; it was already a dev-dep in `bwoc-cli` and `bwoc-harness`. No new production dep.

## Related

- GitHub issue #17 (custom doc-kinds + retro prefill)
- `crates/bwoc-core/src/doc_kind.rs`
- `crates/bwoc-cli/src/doc_cmd.rs`
- `crates/bwoc-cli/src/main.rs` — `DocKindSubcommand` + `dispatch_doc_kind_cmd`
