---
title: Migration
aliases:
  - Upgrade Guide
  - Migrating to 3.0
tags:
  - group/framework
  - type/process
  - meta/operations
parent: English
nav_order: 9
---

# Migration

> [!abstract] Moving an installation across a major version.

One section per major. Start at the one matching the release you are upgrading *to*.

---

# 2.x → 3.0

> [!abstract] Upgrading a 2.x installation. One command for most of it.

3.0 reads everything 2.x wrote, so **nothing breaks the moment you upgrade the
binary**. Migration is something you do at your convenience, before 4.0 removes
the older schema. See [`COMPATIBILITY.en.md`](COMPATIBILITY.en.md#the-support-window)
for the window.

---

## The short version

```bash
bwoc migrate --all --dry-run   # see what would change
bwoc migrate --all             # apply; originals kept under .bwoc/migrate-backup/
bwoc check --all               # confirm: no schema warnings left
```

Then, if you maintain plugins, give each one a bounded `compat` range — see
[Plugins](#plugins) below.

---

## What changed

### 1. Every artifact BWOC owns declares its schema

`workspace.toml`, `agents.toml`, `routes.toml`, `harness-policy.toml` and
`peers.toml` gain a top-level `schema_version = 3`. An artifact without the key
is read as schema 2 — still valid in 3.x, warned about, removed in 4.0.

The practical consequence: BWOC can now tell an old file from a new one. In 2.x
it could not, which meant any future format change could only fail silently.

### 2. The specification is 3.0

An agent's `config.manifest.json` declares `"version": "3.0"`, mirrored by the
`| **Version** | 3.0 |` row in its `AGENTS.md`. This key existed in 2.x and was
never read; `bwoc check` now validates it.

**No section of `AGENTS.md` changed.** The specification version moved because
the *contract around it* did — it is now enforced — not because an agent has to
be rewritten. Migration rewrites two lines per agent.

### 3. `[plugin].compat` is enforced

`PLUGINS.en.md` always said the framework refuses to load a plugin whose
`compat` range does not cover the running framework. It never did. Now it does,
and ranges must be bounded above.

### 4. Two control-plane files fail closed on a newer schema

`harness-policy.toml` and `peers.toml` are refused if they declare a schema this
build does not know, rather than being read with the unknown parts ignored. This
only affects you if you downgrade a binary under a workspace a newer one wrote.

---

## Migrating

### A whole workspace

```bash
bwoc migrate --all --dry-run
```

Reports each artifact and what it would become, writing nothing. Then:

```bash
bwoc migrate --all
```

- **Idempotent.** Running it twice changes nothing the second time.
- **Non-destructive.** The originals are copied to
  `<root>/.bwoc/migrate-backup/<timestamp>/`, keeping their relative paths — a
  restore is `cp -r` back over the root. Pass `--no-backup` to skip.
- **Comment-preserving.** Files are edited as text, not reserialized, so
  comments, key order, and any key BWOC does not model (`[plugins.*]` in
  `workspace.toml`, `skills.framework[]` in a manifest) survive exactly.

### One agent, or one workspace directory

```bash
bwoc migrate ./agents/agent-foo     # an agent directory
bwoc migrate /path/to/workspace     # a workspace root
```

`bwoc migrate` detects which it is: a workspace root has `.bwoc/workspace.toml`,
an agent has `config.manifest.json`.

### In a script

```bash
bwoc migrate --all --json --yes
```

`--json` requires `--yes`, since it writes without prompting. The report:

```json
{
  "targets": [ { "path": "...", "from": "2", "to": "3", "action": "migrated",
                 "backup": "..." } ],
  "summary": { "migrated": 6, "already_current": 0, "failed": 0, "ahead": 0,
               "dry_run": false },
  "schema": { "current": 3, "oldest_supported": 2, "spec": "3.0" }
}
```

Exit codes: `0` success or nothing to do · `1` a file could not be read or
written · `2` no workspace found, or a bad invocation · `3` an artifact declares
a schema newer than this build (upgrade `bwoc`).

---

## Plugins

Every plugin needs a `compat` range that is **bounded above**:

```toml
compat = ">=3.0.0, <4.0.0"     # not ">=3.0.0"
```

An open-ended range claims compatibility with every future major, including the
ones that break the plugin. `bwoc check` warns about that; a range that does not
cover the running framework makes the plugin refuse to load, with the manifest
path and the offending range in the error.

`bwoc migrate` does **not** rewrite `compat` for you. Declaring which framework a
plugin works with is the plugin author's assertion to make, and having a tool
forge it would defeat the point of asking.

---

## Verifying

```bash
bwoc check --all       # no "specification version 2.0" warnings left
bwoc doctor            # workspace health
bwoc list              # agents still resolve
```

A migrated workspace also stays readable by a 2.x binary — nothing in BWOC uses
`deny_unknown_fields`, so an older build skips the `schema_version` key. Treat
that as a safety net for a bad upgrade, not a supported configuration.

---

## If something goes wrong

**Restore from the backup.** Each run writes one timestamped directory per root:

```bash
cp -r <root>/.bwoc/migrate-backup/<timestamp>/. <root>/
```

**A file failed to migrate** (`action: "failed"`). The report names the reason.
The most common is a file that was already malformed — `migrate` refuses to
write anything it cannot re-parse, so a file it could not fix is a file it did
not touch.

**An artifact is "ahead"** (exit 3). It was written by a newer `bwoc` than the
one you are running. Upgrade the binary; do not hand-edit the file down.

---

## See Also

- [`COMPATIBILITY.en.md`](COMPATIBILITY.en.md) — the support window and what counts as breaking.
- [`PLUGINS.en.md`](PLUGINS.en.md#stability) — the plugin compatibility surface.
- [`WORKSPACE.en.md`](WORKSPACE.en.md) — what each workspace file is for.
- [`CHANGELOG.md`](../../CHANGELOG.md) — the release entry for 3.0.
