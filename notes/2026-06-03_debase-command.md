# 2026-06-03 — `bwoc debase` command + `bwoc new --project`

Made the agent → base-project "debase" relationship a first-class CLI surface,
and added a project-aware incarnation path. The relationship already existed
*functionally* (an agent's manifest `worktreeBase = <project>/worktrees`), but
was implicit — set by hand or inferred. This exposes it.

## What changed

- **New module `crates/bwoc-cli/src/debase.rs`** + `Commands::Debase` wiring in `main.rs`:
  - `bwoc debase list` — every registered agent → base project (relativized to the workspace) + buildable stack; `--json`.
  - `bwoc debase show <agent>` — one agent's binding: raw `worktreeBase`, derived project root, buildable stack, and the `<worktreeBase>/<agentId>/<taskId>` worktree pattern; `--json`.
  - `bwoc debase set <agent> <project>` — gated write (TTY confirm unless `--yes`) that canonicalizes `<project>` and sets `worktreeBase = <project>/worktrees`. Idempotent; refuses a non-existent project (exit 2); uses the typed `Manifest` load→mutate→save round-trip (lossless + minimal diff for framework-shaped manifests).
- **`bwoc new --project <path>`** (`new.rs::apply_project_defaults`): on derive, default `worktreeBase` to `<project>/worktrees` and seed the four build gates from `detect_project_kind(project)` — reusing the existing `ProjectKind` / `detect_project_kind` / `suggested_cmd` helpers (now `pub(crate)`). Explicit flags always win.
- Binding convention centralized in `debase.rs`: `WORKTREES_DIR`, `worktree_base_for_project()`, `project_root_of()` — `new.rs` calls the first so the convention has one home.
- Docs: `WORKSPACE.en.md` (+ TH) CLI-surface row + a "debase binding" subsection; CHANGELOG `[Unreleased]`.

## Decisions

- **The binding IS `worktreeBase` — not a new manifest field (Mattaññutā / Yoniso manasikāra).** The memory of the workspace records that `worktreeBase` is the *only* mechanism that functionally binds agent → project; adding a parallel field would create two sources of truth. `debase` reads/writes the existing one.
- **`set` uses the typed `Manifest` round-trip, not a surgical `serde_json::Value` edit.** Verified luban's manifest keys all map to the struct in declaration order, so the round-trip is lossless *and* produces a minimal diff (only `worktreeBase` changes). A `Value` edit would either drop the typed guarantees or (without `preserve_order`) alphabetize keys into a noisy diff.
- **Both manager + derive flag (per the design choice).** `set` rebinds an existing agent; `--project` binds at birth. They share the convention helpers so they can't drift.
- **Unbound is a first-class state.** The 8 security agents legitimately have no `worktreeBase` (they operate on targets, don't build a project); `list` shows them as `—` rather than erroring.

## Bugs surfaced

- During manual testing, `bwoc new --project … --json` with `BWOC_WORKSPACE` set still created the agent in the *real* workspace — `bwoc new` resolves its target from cwd/template default, not `BWOC_WORKSPACE`. Not a regression from this change (pre-existing target-resolution behavior); the stray `agent-builder` was retired immediately. Tests for derive use `--target` into a temp dir instead.

## Status / deferred

- v1 `set` rebinds `worktreeBase` only; it does **not** re-seed build gates on an already-incarnated agent (that's `--project`'s job at birth). Re-seeding gates on `set` is deferred until asked.
- Verified: `cargo test -p bwoc-cli` (debase 5 + new 10 relevant pass), fmt + clippy (`-D warnings`) clean, and live smoke tests — `list`/`show` against the real fleet (luban→bwoc-framwork/Rust; security agents unbound), `set` idempotency + bad-path exit 2, and a real `--project` derive (worktreeBase + 4 Rust gates seeded).

## Related

- `crates/bwoc-cli/src/debase.rs`, `crates/bwoc-cli/src/new.rs`
- `docs/en/WORKSPACE.en.md` (+ `docs/th/WORKSPACE.th.md`) — CLI Surface
- Manifest field: `crates/bwoc-core/src/manifest.rs` (`worktreeBase`)
