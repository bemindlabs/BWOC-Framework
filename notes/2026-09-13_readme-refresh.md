---
title: README refresh — 3.0 focus
tags: [notes, docs, readme]
created: 2026-09-13
---

# 2026-09-13 — README refresh (3.0 focus)

I audited every README against `main` after 3.0.1 and fixed or trimmed the claims that were wrong or stale. This was docs-only apart from five `Cargo.toml` `description` strings and one file rename in the agent template.

## What changed

- **modules/**: I regenerated the plugin (28) and skill (23) READMEs from their manifests. I also updated `PLUGINS.en/th` from "six" to the ten declared backends (`manifest.rs`).
- **agent-template**: I rewrote the README as a short link hub.
  - It now says the template carries spec 3.0 and leads incarnation with `bwoc new`.
  - It lists the real interconnect files and all eight backend entry files.
  - It states that `CLAUDE.md` is a regular file in the template and a symlink after `bwoc new`.
  - I removed the stale Phase 3/4 status.
  - The persona README now points to `neutrality.md` instead of carrying a 4-row backend table.
- **Rename**: `docs/README.md` / `README.bad.md` became `docs/persona-example.good.md` / `.bad.md`. No code, test or `include_dir!` consumer referenced the path. Only `INCARNATION.en/th.md` and `.claude/loop-roadmap.md` did, and I updated them.
- **crates/**:
  - `bwoc-cli`: the subcommand count is now 61, `migrate` is listed, and the exit-code contract, plan-gated `task add` and `task reopen` are noted.
  - `bwoc-core`: added the `schema` module and `Principal::Platform`.
  - `bwoc-deep-memory`: added the `embed_model` column and the unique insert dedup (#491), and fixed the JSON manifest example.
  - `bwoc-connect`: added per-chat session files (#501).
  - `bwoc-harness`: dropped the "P1–P5" wording, here and in `HARNESS.en/th` frontmatter.
  - `bwoc-loop-tui`: noted that tasks added with `a` are plan-gated.
  - `bwoc-agent`: noted that `bwoc-gateway-recv` is built in a separate repo.
- **Cargo descriptions**: I corrected `bwoc-agent`, `bwoc-harness`, `bwoc-a2a`, `bwoc-connect` and `bwoc-core`.
- **examples/**: I removed the Phase/4-backend claims, and `configure-backends.md` now lists all ten backends and uses `bwoc set`. I trimmed the showcases and usecases placeholders.
- **Root README**:
  - The backend list now shows 10, and the crate tree covers all 11 members.
  - The Infrastructure section no longer claims "no network ports / no SQLite". Opt-in network and storage surfaces are now described as out-of-process.
  - I added a 2.x → 3.x upgrade line (`bwoc migrate`, COMPATIBILITY, MIGRATION).
  - The status table uses shipped wording.
  - The Latest release and Status lines are untouched (`release_pointers.rs`).

## Decisions

- I kept the persona examples in `docs/` rather than moving them to a new `examples/` slot, because that is the smallest change.
- I did not document the connect `[bot]` block or public mode, because PR #506 is not merged.

## Related

- `crates/bwoc-cli/tests/release_pointers.rs`, `crates/bwoc-cli/tests/crate_readmes.rs`
