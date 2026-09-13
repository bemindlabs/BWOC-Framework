# 2026-09-08 — `bwoc migrate`

Second slice of 2.44 → 3.0.0. The seam landed first (`notes/2026-09-08_schema-version-seam.md`);
this is the command that moves a fleet across it. `bwoc migrate [path] [--all]`
stamps the schema marker onto every artifact BWOC owns and moves an agent's
declared specification version from 2.0 to 3.0.

Deliberately ordered before anything else in the 3.0 line writes a v3 artifact:
nobody should end up on a build that produces files the shipped tooling cannot
explain.

## What changed

- **New `crates/bwoc-cli/src/migrate.rs`** + `Commands::Migrate` (right after
  `Check` — operators use them as a pair) and its dispatch arm.
- Covers six artifacts: `.bwoc/workspace.toml`, `.bwoc/agents.toml`,
  `.bwoc/interconnect/routes.toml`, `.bwoc/harness-policy.toml`,
  `.bwoc/peers.toml` (marker) plus an agent's `config.manifest.json` and
  `AGENTS.md` (spec version `2.0` → `3.0`). Absent files are skipped, not
  reported as a problem.
- **New `crates/bwoc-cli/tests/migrate_roundtrip.rs`** — the end-to-end
  contract, and the only test that proves the property the feature exists for.

## Decisions

- **Parse to decide, splice to write — never reserialize.** This is the whole
  design. `Manifest::save_to_path` and `Workspace::save` both round-trip through
  typed structs with no catch-all, so either would silently delete keys it does
  not model: `skills.framework[]` (written raw by `bwoc skill enable`) and
  `[plugins.*]` respectively. A migration that quietly ate an operator's plugin
  config while "upgrading" it is worse than no migration. So every writer here
  edits the file's text — the marker is spliced in as a line, the version
  replaced in place — and re-parses before the write lands. Comments, key order,
  trailing comments and unmodeled tables all survive byte-for-byte, which the
  integration test asserts explicitly.
- **The marker goes above the first non-comment, non-blank line.** That line is
  either the file's first key or its first `[table]` header, so the marker
  always lands where TOML requires a top-level scalar while staying below the
  file's banner comment.
- **Backups live at `<root>/.bwoc/migrate-backup/<stamp>/`, not beside the
  original.** The harness control-plane gate protects any path with a `.bwoc`
  component plus the exact name `config.manifest.json`. A backup written as
  `<agent>/config.manifest.json.bak` would fall *outside* that gate, so an
  untrusted turn could plant a poisoned "backup" for an operator to restore.
  Under `.bwoc/` it inherits the existing protection with no change to the gate.
  The relative path is preserved inside the backup dir, so restoring is a plain
  recursive copy.
- **A future artifact is reported, never rewritten — exit 3.** Distinct from the
  write-failure code, because the operator's next move is "upgrade bwoc", not
  "fix this file". The file is left exactly as found.
- **No new dependency.** `toml_edit` would be the obvious reach for
  comment-preserving edits, and it is already in the lock file — but splicing
  one line needs no parser round-trip at all, and the verify-then-write step
  gives the same safety. (Mattaññutā.)
- **`--json` requires `--yes`**, matching `bwoc plugin remove`: a machine-readable
  invocation of a command that writes must be explicit about it.
- **Detects what it was pointed at.** A workspace root has
  `.bwoc/workspace.toml`; an agent has `config.manifest.json`. No mode flag for
  something the filesystem already answers.

## Alternatives considered

- **Reserialize through the typed structs.** Rejected — lossy, see above.
- **Fix `Manifest::save_to_path` first, then reserialize.** The right fix
  eventually, but it is a separate bug with its own blast radius (six call sites
  write manifests today), and migrate should not be blocked behind it.
- **Flattening the backup filenames.** Produced hidden files
  (`.bwoc_interconnect_routes.toml`) an operator would not see in `ls`, and made
  restoring a manual per-file exercise. Preserving the tree costs nothing.

## Status / deferred

Still ahead in the 3.0 line: `bwoc check` / `doctor` reporting a legacy
artifact and pointing at this command; the one-shot deprecation notice; the
template's own `AGENTS.md` / `config.manifest.json` bump to 3.0;
`[plugin].compat` enforcement; the compatibility + migration docs (EN/TH); the
version bump itself.

Not added: a `bwoc help` topic for migrate. The clap long-help covers it, and a
13th topic has to earn its place.

## Related

- `notes/2026-09-08_schema-version-seam.md` — the seam this command writes to.
- `crates/bwoc-cli/tests/migrate_roundtrip.rs` — the contract, stated as a test.
