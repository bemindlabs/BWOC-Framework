---
title: Agent Mindsets
aliases:
  - Mindsets
tags:
  - group/agents
  - type/mindset
  - meta/template
---

# Agent Mindsets

> [!abstract] Decision-making frameworks the agent applies in operation. Mindsets are HOW an agent thinks; persona is WHO it is; memories are WHAT it knows.

## Purpose

A mindset is a small, named, reusable decision frame the agent reaches for in a specific class of situation. Where [[../persona/README|persona]] declares identity and [[../memories/README|memories]] hold accumulated knowledge, mindsets shape the *act of choosing* in the moment.

Mindset files are reference material: no backend loads them into the prompt, so a mindset shapes a decision only when the agent reads it (or when `AGENTS.md` restates it).

Mindsets correspond directly to BWOC principles being applied as decision filters — not as religious instruction, but as engineering thinking aids. See [`docs/en/GLOSSARY.en.md`](../../../docs/en/GLOSSARY.en.md) for the full term lookup.

## File Format

One `.md` per mindset. Obsidian frontmatter required.

```markdown
---
title: <Mindset Name>
aliases: [<short alias>]
tags:
  - type/mindset
  - principle/<pali-term>
---

# <Mindset Name>

> [!abstract] One-sentence description of when this mindset applies.

## When to Apply
...
## How to Apply
...
## When NOT to Apply
...
## Related Principles
...
```

## Examples That Fit

| File | Mindset | Principle |
|---|---|---|
| `verify-before-act.md` | Stop and grep before trusting a remembered claim | Yoniso Manasikāra |
| `right-amount.md` | Smallest spec wins; trim before adding | Mattaññutā |
| `non-clinging.md` | Release worktree, branch, stale state at task end | Anattā |
| `equal-treatment.md` | Resist per-vendor convenience that breaks neutrality | Samānattatā |
| `trace-conditions-backward.md` | The visible problem is rarely the root | Paṭiccasamuppāda |

## Rules

- **One mindset per file.** Don't bundle.
- **Each mindset names ONE principle** — that's its anchor in [`PHILOSOPHY.en.md`](../docs/en/PHILOSOPHY.en.md).
- **"When NOT to apply"** is required — every mindset has a domain of applicability; over-applying any mindset becomes its own anti-pattern.

## Status

This slot is part of the canonical agent template. Empty by default — each incarnated agent adds the mindsets that match its role and domain.
