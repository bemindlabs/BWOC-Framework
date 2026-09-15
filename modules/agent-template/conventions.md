---
title: Naming Conventions & Type Definitions
aliases:
  - Conventions
  - Naming
  - Types
tags:
  - group/agents
  - type/conventions
  - meta/template
---

# Naming Conventions & Type Definitions

> [!abstract] Strict standards for BWOC agent profiles. Consistency enables reliable tooling, template substitution, and multi-agent interoperability.

## Document Format (Two-Tier Rule)

| Tier | Audience | Format | Files |
|---|---|---|---|
| **Documentation** | Humans (Obsidian vault) | Obsidian Markdown | `README.md`, `conventions.md`, `neutrality.md`, `trust-model.md`, `persona/`, `memories/`, `skills/`, `mindsets/`, `projects/`, `docs/` |
| **Instructions** | LLM backends | Plain Markdown | `AGENTS.md` (and symlinks: `AGY.md`, `CODEX.md`, `KIMI.md`) |

`CLAUDE.md` is an exception — a real file with template-repo-specific guidance for Claude Code, not a symlink.

### Documentation files MUST include:
1. YAML frontmatter: `title`, `aliases`, `tags`
2. Wikilinks for cross-references: `[[path|Display Text]]`
3. Callouts where appropriate (approved palette below)

### Instruction files (`AGENTS.md`) MUST NOT include:
- YAML frontmatter
- Wikilinks (`[[...]]`)
- Obsidian callouts (`> [!type]`)

### Approved Callout Palette

| Callout | Purpose |
|---|---|
| `> [!abstract]` | Section summary — opening block of every major document |
| `> [!tip]` | Non-obvious best practices |
| `> [!warning]` | Rules where violation causes real damage |
| `> [!example]` | Concrete examples (use collapsible `<details>`) |
| `> [!note]` | Supplementary context |
| `> [!danger]` | Security warnings, irreversible actions |

Avoid: `[!info]` → use `[!note]`; `[!caution]` → use `[!warning]`.

---

## Naming Styles

> [!note] For the complete `*.md` naming standard (12 categories — top-level metadata, specification docs, crate READMEs, slot landings, skills, memory, notes, translations), see [`docs/en/NAMING.en.md`](../../docs/en/NAMING.en.md). The rules below cover everything else (directory names, JSON fields, YAML frontmatter, placeholders, branches, task IDs, agent IDs, commits).

### Directories: `kebab-case`

```
persona/
memories/
skills/
mindsets/
projects/
agent-oracle-coding/
```

Agent directories must use `agent-{name}` prefix.

### JSON Fields: `camelCase`

```json
{
  "taskId": "PROJ-42",
  "agentId": "agent-oracle-coding",
  "startedAt": "2026-05-22T10:00:00Z",
  "isBlocked": false
}
```

Boolean: `is`/`has` prefix. Timestamps: `*At` suffix. Arrays: plural.

### YAML Frontmatter: `lowercase`

```yaml
---
name: memory-name
description: one-line hook
type: feedback
created: 2026-05-22
updated: 2026-05-22
---
```

### Placeholders: `{{camelCase}}`

Double curly braces, camelCase:

| Placeholder | Meaning | Example |
|---|---|---|
| `{{agentId}}` | Agent identifier | `agent-oracle-coding` |
| `{{name}}` | Agent name (short) | `oracle-coding` |
| `{{taskId}}` | Task identifier | `PROJ-42` |
| `{{moduleName}}` | Module path | `my-module` |
| `{{branchName}}` | Git branch | `feat/PROJ-42` |
| `{{worktreePath}}` | Worktree directory | `/tmp/proj-42` |
| `{{memoryPath}}` | File-based memory dir | `memories/` |
| `{{sessionsPath}}` | Session data dir | `~/.claude/projects/` |
| `{{deepMemoryCmd}}` | Tier 2 memory CLI | resolved at runtime |
| `{{primaryModel}}` | Primary LLM model ID | resolved at runtime |
| `{{fallbackModel}}` | Fallback LLM model ID | resolved at runtime |
| `{{lintCmd}}` | Lint command | resolved at runtime |
| `{{formatCmd}}` | Format command | resolved at runtime |
| `{{testCmd}}` | Test command | resolved at runtime |
| `{{buildCmd}}` | Build command | resolved at runtime |
| `{{worktreeBase}}` | Worktree base path | `/tmp` |

#### Neutrality-Critical Placeholders

Never hardcode these in `AGENTS.md` or base template files:

| Placeholder | Anti-Pattern |
|---|---|
| `{{primaryModel}}` | `claude-opus-4-6`, `gemini-3.5-flash-medium` |
| `{{deepMemoryCmd}}` | `mempalace`, `chromadb` |
| `{{lintCmd}}` | `oxlint`, `eslint` |
| `{{testCmd}}` | `vitest`, `cargo test` |
| `{{buildCmd}}` | `pnpm build`, `cargo build` |

### Branch Names: `type/task-id`

Trunk-based: `main` is the only long-lived branch. Branch `type` uses the Conventional Commit vocabulary (`feat fix docs refactor test chore perf style ci`).

```
feat/PROJ-42
fix/PROJ-43
docs/branching-standard
agent/agent-oracle/feat/PROJ-45
```

No `release/*` or `hotfix/*` branches — CalVer tags (`v<YYYY>.<M>.<D>-<patch>`) are cut directly on `main`. Delete the branch after merge (Anattā).

### Task IDs: `UPPER-N`

```
PROJ-42    MOD-99
```

### Agent IDs: `agent-{kebab-case}`

```
agent-oracle-coding    agent-qa-testing
```

### Commit Messages: `Scope: action`

```
Auth: add token refresh handler
CLI: fix verbose flag parsing
```

Scope = PascalCase. Action = lowercase imperative. Under 72 characters.

---

## Type Definitions

### TaskLogEntry

```yaml
TaskLogEntry:
  taskId: string           # "PROJ-42"
  moduleName: string       # "my-module"
  branchName: string       # "feat/proj-42"
  worktreePath: string     # "/tmp/proj-42"
  status: TaskStatus
  startedAt: string        # ISO 8601
  lastAction: string
  completedAt?: string     # ISO 8601 (optional)
  blockedReason?: string   # required if status=blocked

TaskStatus:
  enum: [pending, in_progress, blocked, completed, failed]
```

### MemoryFile (frontmatter)

```yaml
MemoryFrontmatter:
  name: string
  description: string      # one-line relevance hook
  type: MemoryType
  created: string          # ISO date (YYYY-MM-DD)
  updated: string          # ISO date (YYYY-MM-DD)

MemoryType:
  enum: [user, feedback, project, reference]
```

### AgentConfig

```yaml
AgentConfig:
  agentId: string
  model: string
  fallbackModel?: string
  maxConcurrentTasks: number    # default: 3
  worktreeIsolation: boolean    # default: true
  worktreeBase: string          # default: "/tmp"
  memory: MemoryConfig

MemoryConfig:
  fileBasedPath: string
  deepMemoryCmd?: string
  wakeUpOnStart: boolean        # default: true
  maxMemoryIndexLines: number   # default: 200
```

### SessionMetrics

```yaml
SessionMetrics:
  sessionId: string
  agentId: string
  startedAt: string
  endedAt: string
  metrics:
    tasksAttempted: number
    tasksCompleted: number
    tasksFailed: number
    gatesPassed: number
    gatesFailed: number
    revisionCycles: number
    memoriesCreated: number
    memoriesUpdated: number
    memoriesRemoved: number
  discoveries: string[]
```

---

## Validation Checklist

Before committing agent profile files:

- [ ] Markdown file names follow [`docs/en/NAMING.en.md`](../../docs/en/NAMING.en.md) (12 categories)
- [ ] Directory names are `kebab-case`
- [ ] JSON fields are `camelCase`
- [ ] YAML frontmatter fields are `lowercase`
- [ ] Placeholders use `{{camelCase}}` (double braces)
- [ ] Git branches use `type/task-id`
- [ ] Task IDs use `UPPER-N`
- [ ] Agent IDs use `agent-kebab-case`
- [ ] Commit messages use `Scope: action`
- [ ] Status values match `TaskStatus` enum
- [ ] Timestamps are ISO 8601

### Format Validation

- [ ] Documentation files have YAML frontmatter (`title`, `aliases`, `tags`)
- [ ] `AGENTS.md` has NO frontmatter, wikilinks, or callouts
- [ ] Callouts use only the approved palette

### Neutrality Validation

- [ ] No hardcoded LLM model IDs
- [ ] No hardcoded tool/CLI names
- [ ] No backend-specific language ("Claude will...", "Antigravity supports...")
- [ ] All backend-varying config uses `{{camelCase}}` placeholders
- [ ] Symlinks point to `AGENTS.md`
- [ ] Run `bwoc check` — 0 violations

---

## See Also

- [[README|Agent Template]] — template overview
- [[neutrality|Neutrality]] — backend-neutral cloning design
- [[trust-model|Trust Model]] — external agent cloning security
- [[docs/en/PHILOSOPHY.en.md|Philosophy]] — 22 Buddhist frameworks
- [`docs/en/NAMING.en.md`](../../docs/en/NAMING.en.md) — full `*.md` naming standard (12 categories)
