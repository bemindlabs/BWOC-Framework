---
title: Skill Template
aliases:
  - skill-template
tags:
  - group/framework-skills
  - type/template
  - domain/scaffolding
maturity: template
---

# Skill Template

> [!abstract] Scaffold for new framework skills. `bwoc skill init <name>` copies this directory to `modules/skills/<name>/` and substitutes the `{{camelCase}}` placeholders below. The template is **not an installed skill** — `bwoc check` discovers `modules/skills/*/manifest.toml` only, so this sibling directory is skipped.

## Expected Directory Shape After Substitution

```
modules/skills/<name>/
├── manifest.toml       # placeholders substituted
└── SPEC.md             # placeholders substituted; Obsidian-formatted
```

Optional implementation files (Rust crate, shell script, etc.) the skill author adds after `init` returns. The two required files above mirror the layout declared in [[../../docs/en/SKILLS.en#directory-layout|SKILLS.en.md §Directory Layout]] — every installed skill MUST keep them. Anything else is the skill author's concern.

## Placeholder Substitution

| Placeholder | Required | Replaced by | Example |
|---|---|---|---|
| `{{skillName}}` | yes | `bwoc skill init <name>` argument; kebab-case; must equal the new directory name under `modules/skills/` | `documenter` |
| `{{skillVersion}}` | yes | Author edit; semver of the skill itself, separate from the framework version | `0.1.0` |
| `{{skillDescription}}` | yes | Author edit; one-sentence summary surfaced by `bwoc skill list` | `Capture how a system actually works.` |
| `{{skillOperation}}` | yes | Author edit; first named operation declared in `[contract] exposes` — additional operations are appended manually | `document` |

`maturity` is seeded as `L1` (first successful use, unverified) per [[../../docs/en/SKILLS.en#maturity-levels|SKILLS.en.md §Maturity Levels]]. The author bumps it honestly as the skill earns the next level — over-claiming is a `bwoc check` violation.

`[contract] requires` defaults to `[]`; declare dependencies on other installed skills here when they appear. No `[gates]` table is scaffolded — `verify` is optional. Add one only when the skill has something concrete to check. The gate runs inside `bwoc skill verify --run-gates`, so it must not call `bwoc skill verify` itself; `bwoc check` flags that as a violation.

## What This Template Is Not

- **Not an installed skill.** Lives at `modules/skill-template/` (sibling to `modules/skills/`), so the skill-discovery walker — `discover_skill_dirs` over `modules/skills/*/manifest.toml` — does not see it. Unresolved `{{placeholder}}` markers therefore never reach `bwoc check`.
- **Not the source of validation rules.** The schema lives in [[../../docs/en/SKILLS.en#manifest|SKILLS.en.md §Manifest]]; this file only points to it. If the spec changes, the template follows — never the reverse.
- **Not authoritative for `--kind` / per-kind specialisation.** Skills declare no `kind`. Compare with the plugin counterpart at `modules/plugin-template/` — also flat, also one template — where `{{pluginKind}}` is the *only* per-instance specialisation.

## Neutrality

Manifest values name no backend, model, or vendor CLI — the **Samānattatā** rule enforced by `bwoc check`. A `[gates] verify` command, if added, follows the same rule.

## See Also

- [[../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this template scaffolds against.
- [[../skills/documenter/SPEC|documenter]] — a shipped skill; use it as a worked example of a fully substituted manifest + SPEC.
- [[../plugin-template/SPEC|plugin-template]] — the parallel template for the plugin surface.
- [[../agent-template/README|agent-template]] — the original `{{camelCase}}` placeholder precedent.
