# 2026-09-08 — Schema-version seam (3.0 groundwork, W1)

First slice of the 2.44 → 3.0.0 major: give every on-disk format BWOC owns a way to
say which revision wrote it. Through 2.x exactly one artifact could — the manifest's
`trust` block — so every other format could only ever break *silently*: an old binary
reading a new file, or a new binary reading an old one, with nothing to tell them
apart. Nothing consumes the new marker yet beyond two fail-closed control-plane
reads; `bwoc migrate`, `bwoc check` reporting, and the deprecation notice land in
later slices.

## What changed

- **New `crates/bwoc-core/src/schema.rs`** — `SchemaVersion` (a transparent `u32`
  newtype), `SchemaStatus` (`Current` | `Legacy` | `Future`), `marker_from_toml`
  for loosely-typed reads, and the two format lists. `CURRENT = 3`,
  `LEGACY = 2`. Zero new dependencies, so the `bwoc-core` quarantine holds.
- **Marker adopted** on `Workspace`, `AgentsRegistry` (`workspace.rs`), `Routes` +
  the private `RawRoutes` (`routing.rs`), and `HarnessPolicy`
  (`bwoc-harness/src/policy/permission.rs`). Every one is `#[serde(default)]`, and
  every one is declared **first** in its struct.
- **Writes stamp `CURRENT`.** `Workspace::save` / `AgentsRegistry::save` /
  `Routes::remove_agent_routes` write v3 regardless of what was loaded — the file
  coming out *was* written by this build, so claiming v2 would be a lie, and it
  means any command that rewrites a file carries it forward.
- **Two control-plane reads now fail closed on a future revision.**
  `HarnessPolicy::load` errors, and `resolve_pinned_peer`
  (`bwoc-agent/src/trust.rs`) returns `None` (sender stays unverified).
- **Stale module doc** on `bwoc-core/src/lib.rs` ("Phase 1 v2.0 scaffold. Module
  stubs…") replaced — it described a crate that stopped existing 40 minors ago.

## Decisions

- **Absent ⇒ `LEGACY`, never an error.** This is the whole dual-read promise: 3.x
  reads a 2.x workspace, warns, and offers migration; 4.0 drops v2. Making the
  marker required (the way `TrustBlock` does — correctly, since trust semantics
  ride on that block being well-formed) would have made 3.0 refuse every existing
  workspace on upgrade day.
- **Casing follows each file's own convention** — `schema_version` in TOML
  (snake_case throughout: `agents_dir`, `default_mode`), `schemaVersion` in JSON
  (camelCase, matching `TrustBlock`). One marker, spelled the way a reader of that
  particular file expects.
- **Integer major, not a semver string.** The contract is exactly "which framework
  major wrote this", the support window is defined in majors, and a string invites
  the `"3.0"` vs `"3.0.0"` drift `Manifest.version` already suffers from.
- **A missing *file* is `CURRENT`, not `LEGACY`.** `AgentsRegistry::fresh()` and
  `Routes::fresh()` exist for this: an absent registry is a workspace that has
  never registered an agent, not a 2.x artifact, and reporting "needs migration"
  for it would be noise.
- **Future revisions fail closed, not open.** For files that decide what a turn may
  do (`harness-policy.toml`) or whose signature counts as verified (`peers.toml`),
  ignoring a key this build cannot interpret means granting a permission the
  operator never wrote. Consistent with the deny-by-default those loaders already
  apply. (Yoniso manasikāra — read what is actually there, refuse to infer the
  rest.)

## Alternatives considered

- **A `Versioned` trait, or changing loader return types.** Rejected: the loaders
  have 71 / 43 / 24 / 9 / 4 call sites respectively, and threading a new type
  through all of them buys nothing a public field does not. One field, one line
  per loader, read sites untouched.
- **Versioning `.bwoc/doc-kinds.toml` too.** Rejected (Mattaññutā): nothing writes
  it, nothing validates it, and an unparseable one already degrades to "no custom
  kinds". A marker no reader could act on earns no line. Same for `secrets.toml`,
  `installed-sources.toml`, `teams/*.toml` and the `*.jsonl` streams — the
  `schema.rs` module doc records why each is out.
- **Reusing `TrustBlock::schema_version`.** Rejected: it versions the
  Kalyāṇamitta-7 sub-spec, not a file format. It stays at `1`.

## Bugs surfaced and fixed

- **`Routes::remove_agent_routes` would have silently stripped the marker.** It
  reserializes the whole file through the private `RawRoutes`, so a field present
  only on the public `Routes` would be dropped by any `bwoc retire` — quietly
  un-migrating a workspace. Fixed by carrying the marker on both, with
  `remove_agent_routes_preserves_the_marker` as the regression.
- **TOML field-order trap.** A scalar key after a table (or array of tables) makes
  `to_string_pretty` emit a document that cannot be read back. Declaring
  `schema_version` last in `AgentsRegistry` would have broken `save` at runtime,
  not at compile time — `agents_toml_marker_precedes_the_array_of_tables` pins it.

## Follow-up in this series — specification 3.0

Landed after `bwoc migrate` (`notes/2026-09-08_bwoc-migrate.md`), in that order
deliberately: nothing should declare v3 before the tool that explains v3 exists.

- `modules/agent-template/AGENTS.md` and `config.manifest.json` now declare
  **3.0** — bumped by running `bwoc migrate` on the template rather than by hand,
  which dogfoods the command on the artifact every future agent is cloned from.
- **`bwoc check` reads the version it used to only write.** `audit_spec_version`
  makes `3.0` a pass, `2.0` a **warning** naming `bwoc migrate`, and anything
  else a violation. Warning, not violation, because `check` exits non-zero on
  violations and turning every existing fleet red the day an operator upgrades
  would be hostile when the fix is one command. It becomes a violation in 4.0.
- `VERSION.md` — `Specification: AGENTS.md v3.0`, and the stale line targeting a
  crates.io publish at the Cargo `1.0.0` milestone is replaced by what is
  actually true: the crates are unpublished, the Rust API is not a stable
  surface, and MAJOR tracks the on-disk and CLI contracts.

The per-document `| **Version** |` rows in `PRD` / `SRS` / `PHILOSOPHY` are
deliberately left at 2.0 — `VERSION.md` says those track each document's own
specification and are bumped intentionally, and those documents did not change.

## Status / deferred

Not yet done, in later slices: `bwoc doctor` reporting legacy artifacts; the
one-shot deprecation notice; `[plugin].compat` enforcement; the compatibility +
migration docs (EN/TH); the version bump and release cut.

Two findings from the design pass that change later slices, recorded here so they
are not rediscovered:

- **`is_control_plane` is rule-based, not a filename list.** `policy/mod.rs` matches
  *any* `.bwoc` path component plus the exact name `config.manifest.json` (with a
  canonical-target identity check behind it). The list in its doc comment is
  illustrative. So a new file under `.bwoc/` is protected for free — but a
  control-plane file placed *outside* `.bwoc/` is not. Anything `bwoc migrate`
  writes (backups especially) must live under `.bwoc/`.
- **`Manifest::save_to_path` and `Workspace::save` are lossy.** Both round-trip
  through typed structs with no catch-all, so `save` drops any key the struct does
  not model — `skills.framework[]` on the manifest side (written raw by
  `skill.rs`), `[plugins.*]` on the workspace side. `bwoc migrate` must therefore
  splice text rather than reserialize. The manifest case is a live data-loss bug
  worth fixing on its own.

## Related

- `crates/bwoc-core/src/schema.rs` — the seam, and the rationale in its module doc.
- `VERSION.md` §Cargo SemVer bump rules — the MAJOR trigger this release is built on.
