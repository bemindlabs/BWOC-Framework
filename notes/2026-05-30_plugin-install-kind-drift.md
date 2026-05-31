# 2026-05-30 — `bwoc plugin install` rejected 5 of 9 declared plugin kinds

Found by test-installing every bundled reference plugin into a scratch workspace: 8 of the 19 installable plugins failed with `'<kind>' is not a valid plugin kind. Expected one of: memory-backend, llm-backend, workflow, audit`.

## Root cause

Two divergent plugin-kind allowlists:

- `crates/bwoc-cli/src/check.rs::PLUGIN_KINDS` — the canonical set mirroring PLUGINS.en.md §"Plugin Kinds": `memory-backend, llm-backend, workflow, audit, jira, okr, council, figma, gws` (9). `bwoc check` validates against this, so all bundled plugins pass `check`.
- `crates/bwoc-cli/src/plugin.rs::VALID_KINDS` — a separate, **stale** list with only the original four (`memory-backend, llm-backend, workflow, audit`). `bwoc plugin install` validated against this.

The kinds added in BWOC-43 (`jira`), -47 (`okr`), -57 (`council`), -62 (`figma`), -73 (`gws`) were appended to `check.rs` but never to `plugin.rs`, so the installer fell behind. Net effect: the shipped reference plugins for those kinds (`council-sangha-7`, `figma-rest`, all four `gws-*`, `jira-cloud-rest`, `workspace-okrs`) could not be installed — even though they pass `bwoc check`.

## Fix

Deleted `plugin.rs::VALID_KINDS`; `validate_plugin_kind` now references the single source of truth `check::PLUGIN_KINDS` (made `pub(crate)`). The "Expected one of: …" message and the `--kind` filter docs now derive from the same list, so the installer and `bwoc check` cannot drift again. Updated the `--kind` doc comments in `plugin.rs` + `main.rs`.

## Verification

Test-installed all 19 plugin roots under `modules/plugins/` into a temp workspace:

- **Before:** 11 OK / 8 FAIL.
- **After:** 19 OK / 0 FAIL — `plugin list` shows every kind; `plugin enable` + `--kind` filter confirmed on `figma-rest` and `audit-iso-9001`.

Unit test `validate_kind_accepts_all_declared` now iterates `check::PLUGIN_KINDS` (+ spot-checks the five previously-rejected kinds), so a future kind added to `check.rs` is automatically required to install too. `bwoc-cli` build + tests green; clippy + fmt clean.

## Decisions

- **Share, don't duplicate** — the bug was duplication drift, so the fix is a single shared list rather than re-syncing two. *(Yoniso — fix the cause, not the symptom.)*
- `pub(crate)` (not `pub`) — the list stays crate-internal; only sibling modules need it.

## Related (links)

- `crates/bwoc-cli/src/plugin.rs` (`validate_plugin_kind`)
- `crates/bwoc-cli/src/check.rs` (`PLUGIN_KINDS` — now the shared source)
- `docs/en/PLUGINS.en.md` §"Plugin Kinds"
