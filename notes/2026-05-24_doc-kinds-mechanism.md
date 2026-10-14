# 2026-05-24 — Generic Workspace Document-Kind Mechanism (#12)

Implements the generic document-kind mechanism for BWOC workspaces, subsuming epics #10 (retro CLI) and #11 (research CLI). One registry + one engine covers all three built-in kinds (`notes`, `retrospectives`, `research`) and is designed to accept workspace-declared custom kinds in a future iteration.

## What changed

- `crates/bwoc-core/src/doc_kind.rs` — new module: `DocKind` descriptor, three built-in constants, `fn kind(name) -> Option<DocKind>` lookup.
- `crates/bwoc-core/src/lib.rs` — added `pub mod doc_kind`.
- `crates/bwoc-cli/src/doc_cmd.rs` — new module: generic `run(kind, action, root)` engine; `DocAction` enum (New/List/View); helpers `slugify`, `date_prefix`, `collect_md_files`, `resolve_name`; 15 unit tests covering all required behaviors.
- `crates/bwoc-cli/src/main.rs` — added `DocSubcommand` enum; three `Commands` variants (`Notes`, `Retro`, `Research`); `dispatch_doc_cmd` + `resolve_doc_workspace` helpers.
- `docs/en/NAMING.en.md` + `docs/th/NAMING.th.md` — added rows 10a/10b and rule-definition sections for `retrospectives/` and `research/`.

## Decisions

- `DocKind.template_fn` is a function pointer (`fn() -> String`) to keep the descriptor `const`-constructible. `PartialEq`/`Eq` are implemented manually on `name` alone to avoid the meaningless function-pointer comparison that `#[derive(PartialEq)]` would generate.
- Workspace root fallback is "cwd" (not an error) for the doc-kind commands. Notes and retros are useful even outside a formal BWOC workspace; the operator can always pass `--workspace`.
- No `serde_json` or new crate dependency added. Everything is `std::fs`.

## Alternatives considered

- `bwoc doc <kind> <action>` single entry point — rejected; per-kind aliases (`bwoc notes`, `bwoc retro`, `bwoc research`) read more naturally and follow the existing subcommand style in the codebase.
- Storing kind metadata in `.bwoc/doc-kinds.toml` — deferred as `// TODO(#12)` in the registry. Built-in data is sufficient for v1.

## Status / deferred

- Custom workspace-declared kinds deferred; TODO comment placed at the extension point in `doc_kind.rs`.
- No CHANGELOG entry; orchestrator handles release notes per instructions.

## Related (links)

- GitHub epics #10, #11, #12
- `crates/bwoc-core/src/doc_kind.rs`
- `crates/bwoc-cli/src/doc_cmd.rs`
