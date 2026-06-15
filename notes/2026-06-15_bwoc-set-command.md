# 2026-06-15 — `bwoc set`: the CRUD "update" verb

Added `bwoc set <name>` to update an incarnated agent's backend and/or model in
place. It completes the agent CRUD surface: `new` creates, `retire` removes,
`set` mutates. Motivated by the bwoc-control-center fleet app, which had
create/read/delete wired but no way to change an agent's backend or model
without retire+recreate.

## What changed

- `crates/bwoc-cli/src/set.rs` (new) — `run(SetArgs)`:
  - `--backend <B>` rewrites the `backend` field of the agent's `.bwoc/agents.toml`
    registry entry. All backend symlinks (`CLAUDE.md`/`AGY.md`/…) already exist
    (`bwoc new` force-creates the full set), so a switch needs no file changes
    beyond the registry field.
  - `--primary-model` / `--fallback-model` rewrite `config.manifest.json` via
    `Manifest::load_from_path` / `save_to_path`. The runtime reads `primaryModel`
    from there; `fallbackModel` is metadata-only (status/dashboards — runtime
    fallback is `primaryModel: "auto"` + `autoModels`).
  - At least one field required; unchanged values are reported as no-ops
    (`registry_updated` / `manifest_updated` = false). `--json` for scripting.
- `crates/bwoc-cli/src/main.rs` — `mod set;` + `Commands::Set { … }` (inline
  struct variant, like `Check`/`Update`) + dispatch.
- `crates/bwoc-cli/tests/smoke.rs` — `end_to_end_set_updates_backend_and_model`:
  init → new → set → assert registry backend + manifest models changed.

## Decisions

- **Touch only the structured sources of truth** (registry + manifest), never
  the prose in `AGENTS.md` (§8.2 config example, Appendix A). Those are
  documentation; the runtime never reads a model from them, and re-templating
  backend-specific prose post-hoc is fragile. `bwoc check` validates JSON +
  neutrality, not model-string consistency, so this stays green.
- **No symlink work on backend switch** — the full symlink set already exists,
  so the change is a single registry field. (Yoniso manasikāra: verified the
  template ships all seven backend symlinks regardless of the chosen backend.)
- **Exit-code contract** (review follow-up): `2` = user/input error
  (nothing-to-change, no workspace, agent-not-found), `1` = operational/IO
  failure (registry/manifest read or write). Aligns `set` with the rest of the
  CLI; scripts can distinguish a bad invocation from a runtime fault.

## Status / deferred

- The AGENTS.md prose model references are intentionally left untouched (see
  Decisions). If a future need arises to keep them in sync, that's a separate,
  larger "re-render §0" feature.
- control-center side: wire `agent_update` (bwocd `PATCH /agents/{id}` + UI) onto
  this command — tracked in that repo.

## Related

- bwoc-control-center agent CRUD (create/retire/status) — the consumer.
