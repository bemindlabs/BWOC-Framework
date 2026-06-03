---
title: Workspace
parent: English
nav_order: 3
---

# Workspace

A **workspace** is a directory that holds one or more BWOC agents plus the metadata the CLI needs to operate on them coherently. The CLI accepts a user-specified workspace path. **Operational commands refuse to run until the workspace is structurally complete** (fail-fast, with an actionable error).

This document defines the workspace concept, the on-disk structure, the validation rules, the central per-user memory at `~/.bwoc/`, and how the CLI resolves which workspace to act on.

---

## Concept

| Term | Meaning |
|---|---|
| **Workspace** | A directory the user designates as the home for their BWOC work. May contain many agents. |
| **Workspace marker** | The `.bwoc/` directory at the workspace root. Its presence + valid contents make a directory a workspace. |
| **Agent** | An incarnated BWOC agent — its own self-contained sub-directory inside (or outside) a workspace. |
| **Central memory** | Per-user memory at `~/.bwoc/memory/`, shared by every agent the user runs on this machine. |

Workspaces let a user organize many agents under one roof — pinned models, shared memory, machine-level config — without coupling them to a single git repository.

---

## Workspace Structure (Required)

```
<workspace>/
├── .bwoc/                    # workspace marker + metadata  (REQUIRED)
│   ├── workspace.toml        # workspace config             (REQUIRED)
│   ├── agents.toml           # auto-maintained agent index  (REQUIRED — created by CLI)
│   └── memory/               # workspace-scoped memory      (OPTIONAL)
│       ├── MEMORY.md
│       └── *.md
├── agents/                   # incarnated agents             (RECOMMENDED location)
│   ├── agent-foo/
│   └── agent-bar/
└── ...                       # user's other files (the workspace can coexist with anything)
```

### `.bwoc/workspace.toml` — Required Fields

```toml
[workspace]
name = "my-workspace"            # required, slug
version = "0.1.0"                # required, SemVer of BWOC framework this conforms to
created = "2026-05-22T05:50:00Z" # required, ISO 8601 UTC

[defaults]
backend = "claude"               # optional: claude | agy | codex | kimi | ollama
lang = "en"                      # optional: BCP 47 / ISO 639-1
agents_dir = "agents"            # optional, default "agents" (relative to workspace root)
```

### `.bwoc/agents.toml` — Auto-Maintained

```toml
# Updated by `bwoc new` and `bwoc retire`. Manual edits are honored but not recommended.

[[agent]]
id = "agent-foo"
path = "agents/agent-foo"
backend = "claude"
incarnated = "2026-05-22T05:51:00Z"
status = "active"

[[agent]]
id = "agent-bar"
path = "agents/agent-bar"
backend = "agy"
incarnated = "2026-05-22T05:52:00Z"
status = "active"
```

---

## Validation Rules — "Complete Before Work"

A workspace is **complete** iff all of:

1. `.bwoc/` directory exists.
2. `.bwoc/workspace.toml` exists, parses as TOML, and contains required `[workspace]` fields (`name`, `version`, `created`).
3. `.bwoc/agents.toml` exists and parses as TOML (empty `[[agent]]` array is acceptable — new workspace).
4. The `agents_dir` named in `workspace.toml` (or its default) exists, even if empty.
5. The `version` field in `workspace.toml` is a parseable SemVer.

**Operational commands** (`bwoc spawn`, `bwoc new`, `bwoc check`, `bwoc list`, `bwoc retire`) call validation before doing work. On failure they exit with code `2` and an actionable message naming the missing or malformed part. **No agent work runs against an incomplete workspace.**

**Inspection commands** (`bwoc workspace info`, `bwoc workspace validate`) report status without operating. `bwoc init` creates the structure when it is absent.

---

## CLI Surface

| Command | Purpose | Phase |
|---|---|---|
| `bwoc init [path]` | Create workspace structure at `path` (default: current directory). Idempotent — refuses to overwrite an existing `workspace.toml`. | Phase 1 v2.0 |
| `bwoc workspace info [path]` | Print resolved workspace path, config, and agent count. | Phase 1 v2.0 |
| `bwoc workspace validate [path]` | Run all validation rules; print findings; exit 0 if complete, 2 if incomplete. | Phase 1 v2.0 |
| `bwoc new <name>` | Incarnate a new agent into the workspace (uses `agents_dir`). `--project <path>` derives the agent bound to a base project (sets `worktreeBase` + seeds build gates). | Phase 1 v2.0 |
| `bwoc list` | List agents registered in the workspace (from `agents.toml`). | Phase 1 v2.0 |
| `bwoc spawn <name>` | Validate workspace, then exec the agent's backend. | Phase 1 v2.0 |
| `bwoc debase list \| show <agent> \| set <agent> <project>` | Manage the agent → base-project binding carried by `worktreeBase`. `list`/`show` are read-only (`--json`); `set` is a gated write that points an agent's `worktreeBase` at `<project>/worktrees`. | Phase 3 |
| `bwoc remote link \| list \| status \| unlink` | Link an agent to a **remote-control session** (e.g. Claude Code Remote Control) and manage those links. Backend-neutral bookkeeping recorded at `.bwoc/remote/<agentId>.json`; `link` writes, `list`/`status` read (`--json`), `unlink` is a gated remove. | Phase 3 |

### The debase binding

An agent's **base project** — the project it derives from and builds — is recorded functionally by its manifest `worktreeBase`: an agent bound to project `P` carries `worktreeBase = <P>/worktrees`, and the framework places task worktrees at `<worktreeBase>/<agentId>/<taskId>`. `bwoc debase` makes that relationship a first-class, inspectable surface, and `bwoc new --project <path>` establishes it at incarnation time (also seeding the build/test/lint/format gates from the project's detected stack). An agent with no `worktreeBase` is **unbound** — it operates *on* targets rather than building a project (e.g. monitoring / audit agents).

### Remote-control session links

A **remote-control session** lets an agent's interactive session be driven from elsewhere — e.g. Claude Code's Remote Control, reachable from claude.ai or mobile. `bwoc remote` is **backend-neutral bookkeeping** over that relationship: it records, per agent, which external control session the agent is linked to (it does not itself open or proxy the session). Each link is stored at `.bwoc/remote/<agentId>.json` — `{ agentId, backend, kind, sessionRef, url?, linkedAt, note? }` — mirroring the `.bwoc/sessions/` marker convention. The `kind` field names the mechanism: `claude-remote-control` is the first, and any backend may declare its own kind (Samānattatā — Claude is the first implementation, not a special case). `link` defaults `backend` from the workspace registry (`agents.toml`, which always records it) and `kind` to `claude-remote-control`.

### Workspace Resolution

Precedence — the first matching wins:

1. `--workspace <path>` global flag.
2. `BWOC_WORKSPACE` environment variable.
3. The nearest ancestor directory of `cwd` that contains a `.bwoc/` directory (walk upward).
4. Current working directory, if it itself contains `.bwoc/`.
5. **No workspace** → operational commands fail with code `2` and a suggestion to run `bwoc init`.

---

## Central Memory — `~/.bwoc/`

Independent of any workspace, the CLI maintains a **per-user** directory at `~/.bwoc/`. This is the user-level memory shared by every BWOC agent the user runs on this machine.

```
~/.bwoc/
├── config.toml               # user-level config (default lang, default backend, etc.)
├── memory/                   # central memory (Tier 1 format)
│   ├── MEMORY.md             # index (≤ 200 lines — Mattaññutā)
│   └── *.md                  # typed memories (user, feedback, project, reference)
├── workspaces.toml           # known workspaces registry (auto-maintained)
└── logs/                     # CLI invocation logs (rotated)
```

### `~/.bwoc/config.toml` — User Defaults

```toml
[defaults]
backend = "claude"
lang = "th"
workspace = "/Users/lps/bwoc"    # optional: default workspace path

[memory]
cap_lines = 200                  # MEMORY.md index cap
```

### `~/.bwoc/memory/` — Memory Format

Same two-tier format as per-agent memory (see [`modules/agent-template/memories/README.md`](../../modules/agent-template/memories/README.md)). The `MEMORY.md` index is capped at 200 lines (Mattaññutā). Individual memory files use the four types: `user`, `feedback`, `project`, `reference`.

Agents access central memory through their `deepMemoryCmd` or, in Phase 2+, via the `bwoc-agent` runtime which exposes a unified memory API spanning the three scopes.

### Memory Scopes (clarification)

| Scope | Path | Visible to |
|---|---|---|
| **Per-agent** | `<agent>/memories/` | One agent only |
| **Per-workspace** | `<workspace>/.bwoc/memory/` | All agents in this workspace (optional) |
| **Per-user (central)** | `~/.bwoc/memory/` | All agents this user runs on this machine |
| **Tier 2 (deep)** | pluggable backend | All scopes (vector DB, semantic search, etc.) |

Higher scopes are **read-shared** by default; **writes** require explicit intent so an agent does not silently mutate context outside its own scope.

---

## Lifecycle — Workspace and the Arc

The workspace participates in every phase of the BWOC arc:

| Phase | Action |
|---|---|
| **uppāda** | `bwoc init` creates the workspace; `bwoc new` adds an agent to it (registers in `agents.toml`). |
| **ṭhiti** | `bwoc spawn` validates the workspace first, then exec's the agent's backend in the agent directory. The workspace itself is long-lived — it persists across many agent operations. |
| **vaya** | `bwoc retire <agent>` removes the agent from `agents.toml`, optionally archives its directory; the workspace remains. `bwoc workspace prune` (Phase 3) reclaims orphaned entries. |

---

## See Also

- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — how the CLI, workspace, agents, and runtime fit at runtime.
- [`INCARNATION.en.md`](INCARNATION.en.md) — step-by-step agent creation (Phase 1 still uses the template's `incarnate.sh`; Phase 2+ wraps it in `bwoc new` which writes to `agents.toml`).
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — Pali term lookup.
- [`modules/agent-template/memories/README.md`](../../modules/agent-template/memories/README.md) — memory format spec (applies to per-agent, per-workspace, and per-user memory).
- [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) — CLI install and current command status.
