---
title: Neutrality in Agent Cloning
aliases:
  - Neutral Cloning
  - Backend-Neutral Templates
tags:
  - group/agents
  - type/design
  - meta/template
---

# Neutrality in Agent Cloning

> [!abstract] Design Principle (Samanatatta — Equal Treatment)
> When an agent profile is cloned from this template, the result must function identically across all supported LLM backends — Claude, Antigravity, Codex, Kimi — without modification. This is the technical expression of *samanatatta*: no backend is preferred over another.

---

## Why Neutrality Matters

1. **Portability** — an agent built today for Claude Code must work tomorrow with Antigravity CLI or any future harness without editing the profile
2. **Multi-agent interoperability** — agents in a fleet may run on different backends; a neutral profile lets them collaborate
3. **Trust** — when cloning from external sources, neutrality prevents hidden assumptions from creating compatibility gaps
4. **Future-proofing** — new LLM backends emerge; neutral profiles adapt without rewrites

---

## Four Dimensions of Neutrality

### 1. LLM Backend Neutrality

`AGENTS.md` must not assume a specific LLM backend.

| Neutral | Not Neutral |
|---|---|
| `{{primaryModel}}` | `claude-opus-4-6` |
| `{{deepMemoryCmd}}` | `mempalace` |
| `AGENTS.md` + symlinks | Separate per-backend instruction files |

**Symlink pattern:**

```bash
ln -s AGENTS.md AGY.md
ln -s AGENTS.md CODEX.md
ln -s AGENTS.md KIMI.md
```

`CLAUDE.md` is the exception — a real file with Claude Code-specific project guidance.

### 2. Tool & Platform Neutrality

Use placeholders for all tools:

| Neutral | Not Neutral |
|---|---|
| `{{lintCmd}}` | `oxlint`, `eslint` |
| `{{testCmd}}` | `vitest`, `cargo test` |
| `{{deepMemoryCmd}}` | `mempalace`, `chromadb` |

**Exception:** agent-specific profiles (e.g., `agent-oracle-coding/AGENTS.md`) MAY reference specific tools in their defined tech stack. Neutrality applies to the base template, not to specializations.

### 3. Orchestration Neutrality

The profile must work whether the agent is:
- Run standalone (single session, one human)
- Part of a multi-agent wave (parallel workers, orchestrator)
- Spawned by a CI/CD pipeline

### 4. Memory Backend Neutrality

| Tier | Pattern |
|---|---|
| Tier 1 (file-based) | Markdown files with YAML frontmatter — works everywhere |
| Tier 2 (deep backend) | `{{deepMemoryCmd}}` placeholder — agent config resolves to specific tool |

---

## Enforcement

```bash
bwoc check --all        # validate every agent in the workspace
bwoc check <path>       # validate one agent or the template
```

`bwoc check` includes:
- No hardcoded model IDs in base template files
- No hardcoded tool names in base template files
- No backend-specific language in `AGENTS.md`
- All backend-varying config uses `{{placeholder}}` syntax
- Symlinks point to `AGENTS.md`
- `AGENTS.md` is plain Markdown (no frontmatter, no wikilinks, no callouts)

---

## Cloning Workflow

```bash
# 1. Incarnate — copies the template, writes the manifest, creates backend symlinks
bwoc new {name}

# 2. Customize identity
# Edit AGENTS.md section 1 (identity)
# Edit persona/README.md

# 3. Validate
bwoc check agents/agent-{name}
```

---

## Cloning from External Sources

When importing an agent profile from an external repo:

1. **Inspect before trusting** — read all files before enabling. See [[trust-model|Trust Model]]
2. **Validate neutrality** — run the check script
3. **Sandbox first** — run in an isolated worktree with limited permissions
4. **Adapt config** — map external placeholders to your local config values

---

## See Also

- [[README|Agent Template]] — template overview
- [[conventions|Conventions]] — placeholder syntax and naming
- [[trust-model|Trust Model]] — security for external cloning
- [[docs/en/PHILOSOPHY.en.md|Philosophy]] — Samanatatta and Sila-samannata
