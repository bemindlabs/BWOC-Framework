---
title: Naming
parent: English
nav_order: 4
---

# Markdown File Naming Standard

A single, consistent convention for every `*.md` file in the BWOC framework, agent template, and incarnated agents. The rules below decide what each new file is called and where it lives.

---

## Categories

| # | Category | Location | Pattern | Examples |
|---|---|---|---|---|
| 1 | Top-level project metadata | repo root | `UPPERCASE.md` | `README.md` · `LICENSE` · `CHANGELOG.md` · `VERSION.md` · `VISION.md` · `SECURITY.md` · `CODE_OF_CONDUCT.md` · `CONTRIBUTING.md` |
| 2 | Top-level translation | repo root | `UPPERCASE.<lang>.md` | `VISION.th.md` |
| 3 | Specification doc | `docs/<lang>/` | `UPPERCASE.<lang>.md` | `PHILOSOPHY.en.md` · `GLOSSARY.th.md` · `ARCHITECTURE.en.md` · `INCARNATION.th.md` · `WORKSPACE.en.md` |
| 4 | Template / module prose | `modules/<x>/` | `lowercase-hyphen.md` | `conventions.md` · `neutrality.md` · `trust-model.md` |
| 5 | Slot landing (Obsidian-formatted) | `modules/<x>/<slot>/` | `README.md` | `memories/README.md` · `persona/README.md` |
| 6 | Crate documentation | `crates/<crate>/` | `README.md` | `crates/bwoc-cli/README.md` |
| 7 | Skill definition | `.claude/skills/<name>/` | `SKILL.md` | `.claude/skills/incarnate/SKILL.md` |
| 8 | Memory index | `<memory-scope>/` | `MEMORY.md` | `~/.bwoc/memory/MEMORY.md` · `<agent>/memories/MEMORY.md` |
| 9 | Memory entry | `<memory-scope>/` | `<type>_<slug>.md` | `feedback_policy_docs.md` · `user_role.md` |
| 10 | Note | `notes/` (any scope) | `YYYY-MM-DD_<title>.md` | `notes/2026-05-22_workspace-design.md` |
| 10a | Retrospective | `retrospectives/` (any scope) | `YYYY-MM-DD_<title>.md` | `retrospectives/2026-05-24_sprint-1.md` |
| 10b | Research document | `research/` (any scope) | `YYYY-MM-DD_<title>.md` | `research/2026-05-24_llm-caching.md` |
| 10c | Custom doc kind | declared `dir` in `.bwoc/doc-kinds.toml` | `YYYY-MM-DD_<title>.md` | `decisions/2026-05-24_use-postgres.md` |
| 11 | Claude Code instructions | repo root | `CLAUDE.md`, `CLAUDE.local.md` | `CLAUDE.md` · `CLAUDE.local.md` |
| 12 | Agent instructions (backend-neutral) | repo root of an agent | `AGENTS.md` + symlinks | `AGENTS.md` · `CLAUDE.md → AGENTS.md` |

---

## Rule Definitions

### `UPPERCASE.md` — Open-source standard files

Used for: top-level project metadata that GitHub and the wider OSS community treat as a known artifact (`README`, `LICENSE`, `CHANGELOG`, `CONTRIBUTING`, `CODE_OF_CONDUCT`, `SECURITY`) plus BWOC-canonical top-level docs (`VISION`, `VERSION`).

Reason: GitHub renders these in its UI; community expectation; case-insensitive filesystems on macOS treat them consistently.

### `UPPERCASE.<lang>.md` — Specification + bilingual translation

Two uses, same shape:

- Inside `docs/<lang>/` for every specification document (`PHILOSOPHY.en.md`, `GLOSSARY.th.md`).
- At the repo root for translations of top-level docs (`VISION.th.md`).

`<lang>` is a lowercase BCP 47 / ISO 639-1 code (`en`, `th`, `ja`, `zh`, ...). English is canonical.

### `lowercase-hyphen.md` — Module / template prose

Used for: prose inside `modules/agent-template/` and similar areas where the file is implementation detail rather than a community-standard artifact. Hyphens for word breaks; never spaces, never underscores in this category.

### `README.md` — Subdirectory landing pages

Used inside slots (`memories/`, `persona/`, `interconnect/`, `mindsets/`, `skills/`) and inside each Rust crate (`crates/<x>/README.md`).

Slot READMEs are **Obsidian-formatted** (YAML frontmatter + callouts) — they are spec files for the slot, not OSS landing pages.

Crate READMEs are **plain Markdown** — Rust convention, displayed on crates.io.

### `SKILL.md` — Claude Code skills

Fixed name per Claude Code convention. Inside `.claude/skills/<skill-name>/SKILL.md`.

### `MEMORY.md` — Memory index

Fixed name. Lives inside any memory scope (`<agent>/memories/`, `<workspace>/.bwoc/memory/`, `~/.bwoc/memory/`). Capped at 200 lines (Mattaññutā).

### `<type>_<slug>.md` — Memory entries

`<type>` is one of `user`, `feedback`, `project`, `reference`. `<slug>` is kebab-case but uses underscore between type and slug for readability (`feedback_policy_docs.md`, not `feedback-policy-docs.md` or `feedback.policy-docs.md`).

### `YYYY-MM-DD_<title>.md` — Notes (NEW)

For session notes, design notes, decision records, and anything chronological that does not have a stable identity beyond its date.

- `YYYY-MM-DD` is ISO 8601 (zero-padded month/day).
- Single underscore separates date and title.
- `<title>` is lowercase, hyphen-separated, descriptive.
- Sorts chronologically when listed.

Examples:

```
notes/2026-05-22_workspace-design.md
notes/2026-05-22_naming-standard-rollout.md
~/.bwoc/notes/2026-05-23_user-config-cleanup.md
```

#### Where Notes Live

Notes can live at any of:

| Scope | Path | When |
|---|---|---|
| Project-level | `<repo>/notes/` | Decisions about the framework or this repo |
| Workspace | `<workspace>/.bwoc/notes/` | Decisions scoped to a workspace (Phase 2+) |
| Per-user | `~/.bwoc/notes/` | Personal session notes that cross workspaces |

### `YYYY-MM-DD_<title>.md` — Retrospectives

For structured Paññā-3 reviews of a sprint, session, or milestone. Uses the same date-slug pattern as notes.

Sections: **Sutamayā** (what the data / docs say), **Cintāmayā** (synthesis / pattern), **Bhāvanāmayā** (action taken), **Metrics**.

`bwoc retro new "<title>"` creates one in `retrospectives/` using the built-in template.

### `YYYY-MM-DD_<title>.md` — Research

For exploratory investigations: a question, scope, sources, findings, and recommendation.  Committed to the repo like notes and retrospectives.

`bwoc research new "<title>"` creates one in `research/` using the built-in template.

### `YYYY-MM-DD_<title>.md` — Custom doc kinds (NEW)

A workspace may declare extra document kinds in `.bwoc/doc-kinds.toml`:

```toml
[[kind]]
name = "decision"
dir = "decisions"
committed = true
template = "# YYYY-MM-DD — <Decision>\n\n## Context\n\n## Decision\n\n## Consequences\n"
# OR: template_file = ".bwoc/templates/decision.md"
```

Fields: `name` (CLI kind name), `dir` (workspace-relative directory), `committed` (default `true`), and exactly one of `template` (inline string) or `template_file` (path relative to workspace root).

Custom kinds use the same `YYYY-MM-DD_<title>.md` pattern in the declared directory.

Resolution order: built-in kinds win; `.bwoc/doc-kinds.toml` entries come after. Unknown kind → error listing all available kinds.

**CLI:** `bwoc doc <kind> new "<title>"` — resolves any kind (built-in or custom) through the same generic engine.

```
bwoc doc decision new "use postgres"   # custom kind
bwoc doc notes new "my note"           # built-in via generic command
bwoc notes new "my note"               # built-in alias (unchanged)
```

The named aliases (`bwoc notes`, `bwoc retro`, `bwoc research`) remain thin wrappers and are not deprecated.

---

## Forbidden / Reserved

- Mixed case other than the documented patterns above (e.g., `MyFile.md`, `getting_started.md`).
- Spaces in filenames.
- Translations of `README.md` at the repo root (use `docs/<lang>/<NAME>.<lang>.md` instead).
- `README.<lang>.md` anywhere — translations belong in `docs/<lang>/` with their own name.
- Date stamps inside the file body for naming purposes — the filename carries the date for notes.

---

## Quick Decision Tree

```
Is it a GitHub/OSS-standard top-level file (README, LICENSE, CHANGELOG, ...)?
├── yes → UPPERCASE.md at repo root  (translation: UPPERCASE.<lang>.md)
└── no
    ├── Is it a specification document?
    │   └── yes → UPPERCASE.<lang>.md in docs/<lang>/
    ├── Is it a Rust crate README?
    │   └── yes → README.md in crates/<crate>/
    ├── Is it a Claude Code skill?
    │   └── yes → SKILL.md in .claude/skills/<name>/
    ├── Is it a memory entry?
    │   ├── index → MEMORY.md
    │   └── entry → <type>_<slug>.md
    ├── Is it a chronological note or decision record?
    │   └── yes → YYYY-MM-DD_<title>.md in notes/
    └── Otherwise (module prose, slot landing)
        ├── slot landing → README.md (Obsidian format)
        └── prose       → lowercase-hyphen.md
```

---

## Audit

The `/check-naming` skill and the `.github/workflows/docs.yml` CI job both run the same three checks. Run them locally with:

```bash
# A) Root-level: UPPERCASE.md, UPPERCASE.<lang>.md, or CLAUDE.local.md
find . -maxdepth 1 -name '*.md' \
  | grep -vE '^\./(README|LICENSE|CHANGELOG|CONTRIBUTING|CODE_OF_CONDUCT|SECURITY|VISION|VERSION|CLAUDE|AGENTS)(\.local|\.[a-z]{2,3})?\.md$'

# B) docs/<lang>/ files: UPPERCASE.<lang>.md (mindepth 2 skips the docs/ root;
#    slot READMEs like memories/README.md are exempt per category 5)
find docs modules/agent-template/docs -mindepth 2 -type f -name '*.md' \
  | grep -vE '/[A-Z]+(-[A-Z]+)*\.(en|th|[a-z]{2,3})\.md$' \
  | grep -v '/README'

# C) Notes: YYYY-MM-DD_<title>.md
find . -path '*/notes/*.md' \
  | grep -vE '/[0-9]{4}-[0-9]{2}-[0-9]{2}_[a-z0-9-]+\.md$'
```

Any output from any check is a violation. CI exits non-zero with `::error::` annotations.

---

## See Also

- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — multilingual structure across docs, root metadata, and CLI locales.
- [`WORKSPACE.en.md`](WORKSPACE.en.md) — where workspace-scoped notes and memory live.
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — Pali term lookup.
- [`modules/agent-template/conventions.md`](../../modules/agent-template/conventions.md) — the older convention reference, to be updated to point here.
