---
title: Agent Skills
aliases:
  - Skills
  - Capabilities
tags:
  - group/agents
  - type/skill
  - meta/template
---

# Agent Skills

> [!abstract] Concrete capabilities an agent declares it can do. Each skill is a contract — bounded, verifiable, and named in `interconnect/capabilities.md` for inter-agent discovery.

## Purpose

A skill is a concrete capability an agent owns — "schema review", "migration planning", "test authoring", "release notes generation". Each skill is small enough to verify and bounded enough to refuse out-of-scope work cleanly.

Skill files are reference material: no backend loads them into the prompt; the agent sees one only if it reads it.

Skills compose with three other slots:

- [[../persona/README|persona]] — declares WHO the agent is; skills are what it *does*.
- [[../mindsets/SPEC|mindsets]] — declares HOW the agent thinks; skills are what it *executes*.
- [[../interconnect/capabilities|interconnect/capabilities.md]] — the summary other agents can read for delegation (not parsed by framework code).

## Distinction

- **Agent skills** (this slot) — declared by ONE agent in its own template.
- **Framework skills** (`modules/skills/`) — baseline recommended by the framework, opt-in.
- **Claude Code project skills** (`.claude/skills/`) — slash commands available in a Claude Code session for this repo.

These three layers do not overlap. An agent skill is a *capability*; a framework skill is a *recommended baseline*; a Claude Code skill is a *tool invocation*.

## File Format

One `.md` per skill. Obsidian frontmatter required.

```markdown
---
title: <Skill Name>
aliases: [<short alias>]
tags:
  - type/skill
  - domain/<area>
maturity: L1 | L2 | L3 | L4 | L5 | L6 | L7
---

# <Skill Name>

> [!abstract] One-sentence description of what the agent does with this skill.

## Domain
What files / areas does this skill operate on?

## Inputs
What does this skill need to start?

## Outputs
What does this skill produce?

## Verification Gates
How does the agent know it succeeded?

## Out of Scope
What this skill does NOT do — adjacent skills that handle the difference.
```

## Maturity Levels (Ariya-dhana 7)

| Level | Meaning |
|---|---|
| L1 | First successful use; unverified |
| L2 | Used multiple times; informal verification |
| L3 | Verification gates pass consistently |
| L4 | Resilient to common failure modes |
| L5 | Mentorship — agent can guide another agent in this skill |
| L6 | Cross-domain transfer — applied beyond original context |
| L7 | Canonical — adopted by other agents as a reference |

## Rules

- **One skill per file.** Don't bundle.
- **Every skill has explicit "Out of Scope"** — Mattaññutā / Attanutata.
- **Maturity level must match observed reality** — declared L5 without mentorship is overclaiming; threat T-1.4 in [`THREAT-MODEL.en.md`](../docs/en/THREAT-MODEL.en.md).
- **Cross-link from `interconnect/capabilities.md`** so peer agents can discover it.

## Status

This slot is part of the canonical agent template. Empty by default — each incarnated agent populates it with the skills matching its declared role.
