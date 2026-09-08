---
title: Compatibility
aliases:
  - Compatibility Policy
  - Support Window
tags:
  - group/framework
  - type/process
  - meta/operations
parent: English
nav_order: 8
---

# Compatibility

> [!abstract] What BWOC promises about breaking things, and what it does not.

This is the contract behind the MAJOR row in [`VERSION.md`](../../VERSION.md#cargo-semver-bump-rules).
It exists because 2.x had no such contract: a breaking change was documented in
`CHANGELOG.md` and otherwise left to be discovered.

---

## What is a public surface

Three things. A breaking change to any of them is a MAJOR bump; nothing else is.

| Surface | Where it lives | What "breaking" means |
|---|---|---|
| **The specification** | [`AGENTS.md`](../../modules/agent-template/AGENTS.md), whose version is mirrored in `VERSION.md` §Specification | A section an existing agent's `AGENTS.md` must now have, or one whose meaning changed |
| **On-disk schemas** | `config.manifest.json` and the `.bwoc/` control plane — see [Versioned artifacts](#versioned-artifacts) | A field that becomes required, changes meaning, moves, or is removed |
| **The CLI** | Every documented `bwoc` subcommand, flag, `--json` shape and exit code | A command or flag removed or renamed; a `--json` key removed or retyped; an exit code that changes meaning |

**Explicitly not public:** the Rust API. The crates are not published to
crates.io, `bwoc-core`'s modules are `pub` for the binaries in this workspace and
nothing else, and a MINOR release may reshape them freely. If that ever changes,
this table gains a fourth row first.

Also not public: internal file layout under `target/`, log formats, the wording
of human-readable (non-`--json`) output, and anything a doc calls experimental.

---

## Versioned artifacts

Every format BWOC owns declares which revision wrote it — `schema_version` in
TOML, `schemaVersion` in JSON (each file uses its own casing). The rules are
implemented once, in [`bwoc-core::schema`](../../crates/bwoc-core/src/schema.rs).

| Artifact | Marker |
|---|---|
| `.bwoc/workspace.toml` | `schema_version` |
| `.bwoc/agents.toml` | `schema_version` |
| `.bwoc/interconnect/routes.toml` | `schema_version` |
| `.bwoc/harness-policy.toml` | `schema_version` |
| `.bwoc/peers.toml` | `schema_version` |
| `config.manifest.json` | `version` (the specification version — `3.0`), plus `trust.schemaVersion` for the Kalyāṇamitta-7 sub-spec, which is versioned independently |

**Absent marker means schema 2** — everything BWOC 2.x wrote. That is what makes
an upgrade non-destructive, and it is why the marker is `#[serde(default)]`
rather than required.

Deliberately unversioned, because no reader could act on a marker: `.bwoc/doc-kinds.toml`
(additive; an unparseable one already degrades to "no custom kinds"),
`.bwoc/secrets.toml` (a secret store), `.bwoc/installed-sources.toml` and
`.bwoc/teams/*.toml` (rewritten wholesale by the commands that own them), and
every `*.jsonl` append-only stream.

---

## The support window

**One major version of overlap.**

- A release in the **3.x** line reads schema 2 *and* schema 3. Reading a schema-2
  artifact is a warning, never an error, and the warning names `bwoc migrate`.
- **4.0** removes schema 2. A workspace that has not migrated by then fails to
  load rather than being silently misread.

So the window is: from the moment 3.0 ships until 4.0 ships. Not a fixed number
of months — BWOC does not promise a release cadence, and promising one in time
rather than in versions would be a promise the project cannot keep.

### Reading forward

An artifact declaring a revision **newer** than the running build is refused, not
guessed at. For `.bwoc/harness-policy.toml` this is an error; for `.bwoc/peers.toml`
the peer resolves as unpinned. Both fail closed on purpose: these files decide
what a turn may do and whose signature counts as verified, so a key this build
cannot interpret would mean granting a permission the operator never wrote.

### Reading backward (downgrade)

Going back to an older `bwoc` works as long as the *content* is compatible: no
struct in the workspace uses `deny_unknown_fields`, so an older build skips a
`schema_version` it does not know and carries on. This is a courtesy of the
implementation, not a promise — a schema revision that changes what a field
*means* will break a downgrade regardless of what the parser tolerates.

---

## Deprecation

A public surface is removed only after a release that both keeps it working and
says it is going away.

1. **Announce.** The `CHANGELOG.md` entry for the release that deprecates it says
   what is deprecated, what replaces it, and which major removes it.
2. **Warn in the tool.** `bwoc check` reports it as a warning — not a violation,
   because `check` exits non-zero on violations and turning an existing
   installation red the day someone upgrades is not a deprecation, it is a
   removal with extra steps.
3. **Remove at the next major**, where it becomes a violation or an error.

Plugins carry their own version of this contract: `[plugin].compat` is a bounded
semver range of framework versions, enforced since 3.0. See
[`PLUGINS.en.md` §Stability](PLUGINS.en.md#stability).

---

## Supported versions

Only the latest release is supported. There are no maintenance branches: fixes
land on `main` and ship in the next release, and a security fix is a reason to
cut a release, not to backport (see [`SECURITY.md`](../../SECURITY.md)).

This is a deliberate consequence of the release model — tags are cut directly on
`main` and `CONTRIBUTING.md` forbids `release/*` branches. Supporting an older
line would require the branches the project chose not to have.

---

## See Also

- [`VERSION.md`](../../VERSION.md) — the version namespaces and the bump rules.
- [`MIGRATION.en.md`](MIGRATION.en.md) — moving a 2.x installation to 3.0.
- [`RELEASING.en.md`](RELEASING.en.md) — how a release is cut.
- [`PLUGINS.en.md`](PLUGINS.en.md) — the plugin-side compatibility surface.
