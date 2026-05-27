---
title: Skills
parent: English
nav_order: 11
---

# Framework Skills

A **framework skill** is a capability the framework recommends as a baseline that an agent can opt into. It is the "standard library" of agent behaviors — well-defined, verifiable, neutral across backends, discoverable through one manifest format.

This spec defines the manifest format, invocation contract, discovery mechanism, and verification gates. The first reference skill (`worktree-discipline`) ships alongside this spec — both lands and proves the format together.

> [!abstract] Status: initial scaffold. Manifest tables and lifecycle hooks below are normative; prose may be refined as story BWOC-1..3 work refines the contract. The first reference skill lands in BWOC-6.

---

## Three Layers, One Word

The word "skill" appears in three distinct places in BWOC. They do not overlap.

| Layer | Path | Audience | Invoker |
|---|---|---|---|
| **Framework skill** (this spec) | `modules/skills/<name>/` | Agent author | The agent, during its own operation |
| **Agent skill** ([SPEC](../../modules/agent-template/skills/SPEC.md)) | `<agent>/skills/<name>.md` | One agent | That agent's own logic |
| **Claude Code skill** | `.claude/skills/<name>/SKILL.md` | This repo's Claude Code session | A `/<name>` slash command |

A framework skill is a *recommended baseline*. An agent skill is a *declared capability*. A Claude Code skill is a *tool invocation*. Picking the right layer is the first decision when adding anything that calls itself a "skill".

---

## Directory Layout

```
modules/skills/
└── <name>/
    ├── manifest.toml       # required — the contract
    ├── SPEC.md             # required — Obsidian-formatted skill description
    └── ...                 # optional implementation (Rust crate, shell script, etc.)
```

`<name>` is `kebab-case`. One skill per directory.

---

## Manifest — `manifest.toml`

```toml
[skill]
name        = "worktree-discipline"             # required — must match the directory name
version     = "0.1.0"                           # required — semver
description = "Create, isolate, cleanup worktrees per Anatta."   # required — one-sentence summary
maturity    = "L1"                              # required — see "Maturity" below

[contract]
requires         = []                              # optional (default []) — other framework skills this depends on
requires_plugins = []                              # optional (default []) — plugin KINDS this skill needs enabled
exposes          = ["claim_task", "release_task"]  # required — named operations the skill makes available
                                                   #   (must be a non-empty array; empty means the skill exposes nothing
                                                   #    and should not exist — see Field reference)

[gates]
verify      = "bwoc skill verify worktree-discipline"   # optional — shell command; exits 0 iff the skill works here
```

### Field reference

| Section | Field | Required | Type | Meaning |
|---|---|---|---|---|
| `[skill]` | `name` | yes | string (kebab-case) | Skill identifier; must equal the directory name under `modules/skills/` |
| `[skill]` | `version` | yes | string (semver) | Semver of the skill itself, separate from the framework version |
| `[skill]` | `description` | yes | string | One-sentence summary; shown by `bwoc skill list` |
| `[skill]` | `maturity` | yes | enum `L1`..`L7` | Current maturity level (see [Maturity](#maturity-levels)); honest declaration enforced by `bwoc check` |
| `[contract]` | `requires` | no (default `[]`) | array of strings | Names of other installed framework **skills** this skill depends on; resolved at agent spawn |
| `[contract]` | `requires_plugins` | no (default `[]`) | array of strings | Plugin **kinds** this skill needs enabled in the workspace; resolved at agent spawn (see [Skill-on-plugin dependency](#skill-on-plugin-dependency)) |
| `[contract]` | `exposes` | yes (non-empty) | array of strings | Named operations the skill makes available to its caller; an empty array fails `bwoc check` |
| `[gates]` | `verify` | no | string (shell command) | Command run by `bwoc skill verify <name>`; exits 0 iff the skill works in this environment |

### Neutrality constraint (HARD)

Manifest values **must not** name a specific vendor, model, or backend CLI. A skill that only works on one backend belongs as that backend's integration plugin, not as a framework skill. This is the same **Samānattatā** rule that `bwoc check` already enforces on `AGENTS.md`.

---

## Invocation Contract

A skill exposes named **operations** (declared in `[contract] exposes`). When an agent opts in to a skill, those operations become available to its logic — the *how* of routing the call is the agent's concern, the *what* is the skill's contract.

### Lifecycle

```
init  → invoke (one or more times) → teardown
```

- **`init`** — called once when the skill is first loaded into an agent's runtime. **Idempotent.** Reads any agent-side config the skill needs.
- **`invoke`** — called per operation. **Idempotent at the operation level**: calling `claim_task("t-1")` twice for the same task must not double-claim.
- **`teardown`** — called once when the agent retires or releases the skill. **Idempotent.** Cleanup-only; must not block on external state.

Idempotency is a **hard requirement at every phase** — agents may retry, restart, or replay. A skill that breaks on replay breaks the agent's recovery story.

### Hook contract — success, failure, partial state

A skill is *invoked*, not *imported*. The agent's runtime resolves the skill name to an installed manifest, runs `init` once, then dispatches operations. No global registry; the resolution lookup is per-workspace (see [Discovery](#discovery)).

Skills are in-process abstractions, so the contract is expressed as return / throw semantics (not exit codes — those belong to plugins, see [`PLUGINS.en.md`](PLUGINS.en.md#hook-contract--success-failure-partial-state)).

| Hook | Success means | Failure means | Partial state |
|---|---|---|---|
| `init` | Returns; agent spawn continues. | Throws a typed error naming the skill; agent spawn is refused. | Init must fully complete or roll back before throwing — the caller treats a failed init as if it never ran. |
| `invoke` | Returns the operation's result. | Throws a typed error naming the operation; caller decides whether to retry. | Each operation must be durable-or-discarded — never half-applied. Retries land on the idempotent path. |
| `teardown` | Returns; the slot is released. | Logs and continues — agent retirement must not block on teardown. | Idempotent on replay. Agents may crash mid-retire; the next session calls teardown again. |

### Per-phase examples

```text
# init — load agent-side config once at spawn
init():
  cfg = read("<agent>/config.manifest.json")
  cache cfg.worktreeBase           # used by every invoke()
  # no remote calls — Anattā

# invoke — operation-level idempotency
claim_task("t-1"):
  if worktree_exists("t-1"):
    return existing_path           # replay-safe
  path = create_worktree("t-1")
  register(path)
  return path

# teardown — cleanup-only, idempotent
teardown():
  drop_in_memory_caches()
  # does NOT prune worktrees — release_task owns that
  # safe to call twice
```

---

## Discovery

Skills are **opt-in per agent**, not globally available. An agent's `config.manifest.json` declares which framework skills it consumes:

```json
{
  "skills": {
    "framework": [
      { "name": "worktree-discipline", "version": ">=0.1.0", "enabled": true }
    ]
  }
}
```

Schema for each `skills.framework[]` entry:

- `name` (string, required) — the installed skill's directory name under `modules/skills/`.
- `version` (string, required) — semver constraint the agent will accept; resolved against `[skill].version` in the skill's manifest.
- `enabled` (bool, required) — gates whether the skill is loaded at agent spawn. Set `false` to keep the entry as documented intent without loading. Mirrors the `workspace.toml [plugins.<name>] enabled` pattern in [`PLUGINS.en.md`](PLUGINS.en.md); flip with `bwoc skill disable <name>` to preserve the entry.

A missing `enabled` field is a manifest error — `bwoc check` rejects entries that omit it. There is no implicit default; explicit intent is the contract.

At agent spawn the framework:

1. Reads the `skills.framework` list from the agent's manifest.
2. Filters to entries where `enabled` is `true`. Entries with `enabled = false` are kept in the manifest (as documented intent) but skipped at load.
3. Resolves each entry against the workspace's `modules/skills/<name>/` directory.
4. Resolves each skill's `[contract] requires` (other skills) and `[contract] requires_plugins` (plugin **kinds**) against the workspace — every named skill must be installed-and-enabled and every named kind must have an enabled `workspace.toml [plugins.<name>]` of that kind (see [Skill-on-plugin dependency](#skill-on-plugin-dependency)).
5. Loads each skill's manifest and runs `init`.
6. Refuses to spawn if any required skill is missing, any required plugin kind is not enabled, or any verify gate fails.

No central index. A workspace knows about a skill only because it lives under `modules/skills/`. Sources can be remote (git / tarball / local path — see [Sources & Installation](#sources--installation)) but **the resolution lookup is always local to the workspace** — no runtime network calls during agent spawn. **Anattā** preserved.

---

## Skill-on-plugin dependency

A skill may depend not only on other **skills** (`[contract] requires`) but on a **plugin** (`[contract] requires_plugins`). A skill is an agent capability; a plugin is a framework integration ([`PLUGINS.en.md`](PLUGINS.en.md#skill-vs-plugin)). When a skill is a thin scrum/agent-vocabulary wrapper over an integration the workspace already loads, it declares that integration as a dependency rather than reimplementing it. The first such skill is [`scrum-via-jira`](../../modules/skills/scrum-via-jira/SPEC.md), which wraps a `jira`-kind plugin's `bwoc jira` verbs.

```toml
[contract]
requires         = ["worktree-discipline"]   # framework SKILL names
requires_plugins = ["jira"]                   # plugin KINDS — not plugin names
exposes          = ["transition-story", "..."]
```

### Kinds, not names

`requires_plugins` names plugin **kinds** (the [`PLUGINS.en.md` §Plugin Kinds](PLUGINS.en.md#plugin-kinds) enum: `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`, …), never a specific plugin's `name`. A skill that needs Jira depends on *any* enabled `jira`-kind adapter, not on `jira-cloud-rest` specifically — so the adapter can be swapped without touching the skill, and the skill stays neutral. This is also why a dedicated field is used rather than overloading `requires`: `requires` resolves against skill names, `requires_plugins` against the kind enum, and conflating the two namespaces would make a bare `"jira"` ambiguous (a skill named `jira`? a kind `jira`?).

### Direction is one-way: skill → plugin

The skill calls the plugin's verbs; the plugin has no knowledge of the skill and is fully usable without it (the operator runs the plugin's CLI directly). A plugin therefore never declares a skill dependency — the dependency arrow only ever points skill → plugin.

### Resolution

- **At agent spawn** — for every kind in `requires_plugins`, the framework checks the workspace has an **enabled** plugin of that kind (`workspace.toml [plugins.<name>]` with matching `kind`). If none is enabled, spawn fails fast with a diagnostic naming the missing kind; the agent is never half-wired (Discovery step 4).
- **Earlier, via `bwoc skill verify <name>`** — runs the same check before spawn time so the gap surfaces in CI / pre-flight, not at runtime.
- **Statically, via `bwoc check`** — validates that every `requires_plugins` value is a valid kind enum (see [Verification](#verification)). It does not require the plugin to be enabled at check time — enablement is a per-agent spawn concern, kind-validity is a manifest concern.

---

## CLI Surface

Read-only surfaces (no side-effects on the workspace):

```
bwoc skill list                     # list installed framework skills
bwoc skill list --enabled           # filter to enabled-on-current-agent
bwoc skill list --json              # machine-readable

bwoc skill show <name>              # full manifest + spec for one skill
bwoc skill show <name> --json

bwoc skill verify <name>            # run the gates command from [gates]
bwoc skill verify --all             # verify every installed skill
```

Lifecycle surfaces (write — see referenced sections for details):

```
bwoc skill init <name>              # scaffold a new skill from modules/skill-template/
                                    #   (see "Scaffolding from template")

bwoc skill install <source>         # install from local path / git URL / tarball URL
                                    #   (see "Sources & Installation")

bwoc skill enable <name>            # set enabled=true in current agent's config.manifest.json
bwoc skill disable <name>           # set enabled=false (keeps the entry)

bwoc skill remove <name>            # delete modules/skills/<name>/ and clean every
                                    #   consuming agent's manifest (see "Removal")
```

All read-only commands have `--json` twins. Lifecycle commands emit structured JSON when `--json` is passed (event-shape per command). `verify` exits non-zero on any failed gate; `install` exits non-zero on trust-gate failure; `remove` exits non-zero on missing target unless `--yes` was passed.

### "Current agent" resolution

`enable`, `disable`, and `remove` need to know which agent's `config.manifest.json` to modify. The framework resolves this in the following order, stopping at the first match:

1. **`--agent <name>` flag** — explicit override; always wins.
2. **`BWOC_AGENT` environment variable** — useful for shell sessions scoped to one agent.
3. **Working directory** — if cwd resolves to a descendant of `<workspace>/agents/<id>/`, use `<id>`.
4. **Otherwise** — error: `no agent context; pass --agent <name> or run from within an agent directory`.

`bwoc skill remove --all-agents <name>` skips the resolution and applies to every consuming agent in the workspace (still confirms with the user unless `--yes`).

---

## Sources & Installation

A framework skill enters a workspace either by being authored in place under `modules/skills/<name>/` or by being installed from one of three source kinds:

| Source kind | Example | Detection |
|---|---|---|
| **Local path** | `bwoc skill install ./vendor/my-skill/` | Argument starts with `./`, `../`, or `/` and resolves to a directory |
| **Git URL** | `bwoc skill install https://github.com/org/skill.git#v0.1.0` | Argument scheme is `http(s)://` or `git://` AND ends with `.git` (optional `#<ref>` selects branch / tag / sha) |
| **Tarball URL** | `bwoc skill install https://example.com/skill-0.1.0.tar.gz` | Argument scheme is `http(s)://` AND ends with `.tar.gz` or `.tgz` |

The install mechanism:

1. Resolves the source kind from the argument.
2. **Pre-flight** — if source has no `manifest.toml` at its root, refuse with `source missing manifest.toml; cannot resolve name or kind`. Nothing is fetched / extracted / written.
3. **Trust gate** (see below) — fetches and verifies a SHA-256 checksum.
4. Materializes the source into `modules/skills/<name>/` (copy for local; clone-then-discard-`.git` for git; extract for tarball).
5. Validates the installed manifest with `bwoc check`.
6. Records the install in `.bwoc/installed-sources.toml` (schema below). Only writes the registry record on successful completion.
7. **Does not** auto-enable. The installed skill is dormant until `bwoc skill enable <name>` is called on a consuming agent.

### Re-install and failure handling

- **Target already exists** — if `modules/skills/<name>/` already exists, the default behavior is to refuse with `<name> already installed at version X; pass --upgrade to replace`.
  - `--upgrade` — replaces in place, retains the `installed-sources.toml` record (updates `last_hash` and `installed_at`).
  - `--force` — replaces unconditionally, even if the current install has uncommitted local edits (a stderr warning lists what was overwritten).
- **Network failure during install** — install is non-atomic by design; on transient failure (download interrupted, extract error), the partial directory is removed before exit and `installed-sources.toml` is **not** updated. Safe to retry.

### Trust gate (v1)

Every install verifies a SHA-256 checksum **before** materializing:

- **Tarball URL** — the CLI fetches `<source>.sha256` (same URL with the `.sha256` suffix), reads the expected digest, and compares against the computed digest of the downloaded archive.
- **Git URL** — the CLI fetches the checksum at the URL with `.git` replaced by `.sha256`. Example:
  - Source: `https://github.com/org/skill.git#v0.1.0`
  - Checksum: `https://github.com/org/skill.sha256` (operator publishes a manifest of expected tree-shas keyed by ref)
  - After clone, the framework runs `git rev-parse <ref>^{tree}` and compares against the entry for `<ref>` in the fetched manifest.
  - Operators typically publish this manifest via a GitHub release asset or a separate static-hosted file.
- **Local path** — checksum is optional; if a sibling `<dir>.sha256` exists, it is verified; otherwise the install proceeds (local paths are operator-trusted by convention).

Two flags relax the gate:

- `--no-verify` — skips checksum verification. Emits a stderr warning. Intended for in-development sources served locally over HTTP.
- `--allow-new-source` — required the **first time** a given source URL is installed in this workspace. Establishes "I have inspected this source." Subsequent installs from the same registered source (recorded in `.bwoc/installed-sources.toml`) skip this prompt.

The trust gate is intentionally minimal in v1. A future Trust v2 (signed envelopes; identity proof) extends this surface without breaking the v1 contract — the same `--no-verify` / `--allow-new-source` flags carry forward.

**Anattā preserved.** There is no central registry, no name-to-URL resolution service, no auto-update mechanism. Every install names its source explicitly. The framework is not a package manager.

### `.bwoc/installed-sources.toml` schema

Per-workspace registry of installs. Created on first install; never deleted by the framework (manual cleanup via `bwoc skill remove --forget-source` or hand-edit).

```toml
# Keyed by source-key = SHA-256 hex of the normalized source argument.
# The hash is a stable identifier — the URL itself can move (e.g. ref change)
# without losing history of past installs.

["abc123def456..."]
url             = "https://github.com/org/skill.git#v0.1.0"   # original argument
kind            = "skill"                                      # "skill" | "plugin"
name            = "worktree-discipline"                        # from installed manifest
target          = "modules/skills/worktree-discipline"          # workspace-relative
installed_at    = "2026-05-26T10:23:00Z"                       # ISO 8601 UTC
installed_hash  = "<SHA-256 of installed tree>"                # for drift detection
last_verified   = "2026-05-26T10:23:00Z"                       # set by bwoc check
acknowledged_by = "pituk.kae"                                  # whoever passed --allow-new-source
```

Fields are framework-managed — operators should not hand-edit unless removing stale entries. `bwoc check` validates the registry against the filesystem on each run (see [Verification](#verification)).

---

## Scaffolding from template

`bwoc skill init <name>` creates a new skill in `modules/skills/<name>/` by copying the template at `modules/skill-template/` and substituting placeholders:

```
modules/skill-template/
├── manifest.toml          # contains {{skillName}}, {{skillVersion}} placeholders
└── SPEC.md                # Obsidian-formatted; placeholders for skill name + description
```

Placeholders use the same `{{camelCase}}` convention as `modules/agent-template/`. Required substitutions are listed in the template's own [`SPEC.md`](../../modules/skill-template/SPEC.md).

`bwoc skill init` is the recommended way to start a new skill — manual creation is supported but bypasses placeholder consistency.

---

## Removal

`bwoc skill remove <name>`:

1. **Lists consumers** — every agent whose `config.manifest.json` references this skill (`skills.framework[].name == <name>`).
2. **Confirms with the user** unless `--yes` was passed. Lists what will be deleted (the directory) and modified (each consuming agent's manifest).
3. **Deletes** `modules/skills/<name>/` recursively.
4. **Cleans** `skills.framework[]` in every consuming agent's `config.manifest.json` — removes the entry entirely (not just `enabled = false`).

Idempotent — `remove` on a non-existent target reports "not installed" and exits 0. The `--yes` flag short-circuits the confirmation prompt; the operator owns the consequences.

A removed source is not auto-uninstalled from `.bwoc/installed-sources.toml` — that registry persists so a re-install from the same source does not retrigger `--allow-new-source`. Pass `--forget-source` to also drop the source registration.

---

## Maturity Levels

Reuses the [Ariya-dhana 7](../../modules/agent-template/skills/SPEC.md#maturity-levels-ariya-dhana-7) scale from the agent-template skill slot. A framework skill declares its current level honestly; over-claiming is a `bwoc check` violation.

| Level | Meaning |
|---|---|
| L1 | First successful use; unverified |
| L2 | Used multiple times; informal verification |
| L3 | Verification gates pass consistently |
| L4 | Resilient to common failure modes |
| L5 | Mentorship — one skill can guide another's design |
| L6 | Cross-domain transfer — applied beyond original context |
| L7 | Canonical — adopted as a reference by other skills |

A skill bumps maturity in its own `version` change; the `version` and `maturity` fields move together, not independently.

---

## Verification

`bwoc check` extends to audit `modules/skills/<name>/` plus the installed-source registry:

| Check | Pass condition |
|---|---|
| Manifest parseable | `manifest.toml` is valid TOML and matches the schema above |
| Name matches directory | `[skill].name == basename(directory)` |
| Neutrality | No vendor names / model IDs / backend CLIs in manifest values |
| `SPEC.md` present | A `SPEC.md` file exists alongside the manifest |
| Required fields | `name`, `version`, `description`, `maturity`, `[contract] exposes` all present |
| `requires_plugins` kinds valid | every value in `[contract] requires_plugins` (if present) is a valid plugin-kind enum (does not require the plugin to be enabled — that is a spawn-time check) |
| Source registry parseable | `.bwoc/installed-sources.toml` is valid TOML if present |
| No orphan source records | every entry where `kind = "skill"` in the registry has a matching `modules/skills/<name>/` directory |
| No orphan installations | every `modules/skills/<name>/` either has a registry entry OR contains an `.authored-in-place` marker file (authored-in-place skills opt out of registry tracking) |
| Registry drift | `installed_hash` in registry matches the current SHA-256 of `modules/skills/<name>/` (or `bwoc check --update-hashes` was passed to acknowledge drift) |

A failed check exits non-zero on the workspace audit — same surface, same exit semantics as the existing `bwoc check --all`.

---

## What This Spec Does NOT Cover

- **Per-agent skill slots** — see [`modules/agent-template/skills/SPEC.md`](../../modules/agent-template/skills/SPEC.md). Different layer; different contract.
- **Claude Code session skills** — see `.claude/skills/`. Tool-side concept; not a framework concern.
- **Plugin loading** — see [`PLUGINS.en.md`](PLUGINS.en.md). Skills are agent-invoked; plugins are framework-loaded.
- **The first reference skill itself** — see story `BWOC-6` and (once landed) `modules/skills/worktree-discipline/SPEC.md`.

---

## See Also

- [`PLUGINS.en.md`](PLUGINS.en.md) — the sibling spec; same substrate, different invoker.
- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — how modules compose with the rest of the framework.
- [`modules/agent-template/skills/SPEC.md`](../../modules/agent-template/skills/SPEC.md) — per-agent skill slot; this spec's nearest neighbor.
- [`NAMING.en.md`](NAMING.en.md) — file naming and directory conventions.
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — Pali term lookup (Anattā, Samānattatā, Mattaññutā).
