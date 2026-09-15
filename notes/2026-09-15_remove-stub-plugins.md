# 2026-09-15 — Remove the three stub plugins with no runtime

Removed `modules/plugins/memory-tier2-noop/` and `modules/plugins/llm-backend/{hermes,openclaw}/`. Each named an `entry` binary (`bwoc-plugin-memory-tier2-noop`, `bwoc-llm-hermes`, `bwoc-llm-openclaw`) that no crate builds and no code path loads. They were registered-but-unloadable intent, not capability. The framework now ships 25 plugins across 7 kinds.

## What changed

- `git rm -r` of the three plugin directories. `modules/plugins/llm-backend/` is gone with them.
- `modules/plugins/README.md`: the totals and the kind table were regenerated from `git ls-files 'modules/plugins/**/manifest.toml'`. The now-empty `llm-backend` / `memory-backend` rows are dropped. `modules/README.md`: the plugin count and kinds list.
- `docs/{en,th}/PLUGINS.*.md`: the intro, the status callout and "What This Spec Does NOT Cover" no longer claim a reference plugin. They say plainly that none ships for `memory-backend` / `llm-backend`, and that Tier 2 memory runs through `deepMemoryCmd`. The manifest and `workspace.toml` examples use an illustrative `memory-example`.
- `modules/plugin-template/SPEC.md`: the worked example now points at the shipped `workflow/gcloud-auth`. `audit-iso-29110/SPEC{,.th}.md`: the dead sibling wikilink is dropped.
- `crates/bwoc-cli/src/check.rs`: one comment. The tests that use the `memory-tier2-noop` name write inline manifests into a tempdir and never read the deleted directory, so they are kept as fixtures.

## Decisions

- **The kinds stay in the spec.** `memory-backend` and `llm-backend` remain valid `kind` values. Only the shipped instances go.
- **Tier 2 is not a plugin.** It is wired through `bwoc-core::deep_memory` plus the agent's `deepMemoryCmd`, and `bwoc-deep-memory` is the reference implementation. The noop plugin described a path the runtime never took. (Yoniso manasikāra: the docs now match the code.)
- **Trim, don't stub.** A stub that "stays registered-but-disabled until the binary exists" is intent and belongs in a roadmap or story, not in `modules/`. (Mattaññutā.)

## Status / deferred

- The historical notes (`2026-05-26_skills-plugins-standard.md`, `2026-06-08_inbound-llm-backend-stubs.md`, `2026-09-13_readme-refresh.md`) and the past CHANGELOG releases are left as written.
- The outbound adapters (`bemindlabs/bwoc-plugin-{hermes,openclaw}`) live in separate repos and are unaffected.
