---
title: Documenter
aliases:
  - documenter
tags:
  - group/framework-skills
  - type/skill
  - domain/documentation
maturity: L1
---

# Documenter 📄

> [!abstract] The craft of capturing how a system *actually* works so the next reader — human or agent — reuses the knowledge instead of re-deriving it, and keeping that record **true to current reality** as the system changes. Where [[../writer/SPEC|writer]] optimises for a reader's experience, `documenter` optimises for **accuracy, conventions, and staying in sync**. Encodes **Sīlasāmaññatā** (shared conventions) and **Sacca** (truthful to the current artifact).

## What This Skill Does

Two operations are exposed:

- **`document(subject)`** — write the record: describe what the thing does, how it's structured, how to use it, and the decisions + gotchas that aren't obvious from the code — grounded in the *current* artifact (read it, don't recall it). Follow the repo's conventions: file placement, naming, and any hard rules (e.g. bilingual `en ↔ th` parity, changelog vs implementation-note split).
- **`sync(doc, change)`** — keep the record honest as reality moves: when the system changes, update the doc in the same change; flag drift (a doc that now contradicts the code); retire what's obsolete. Documentation that lies is worse than none.

## Why It Exists

Knowledge that lives only in someone's head, or in a doc that's quietly wrong, forces the next person to re-derive it — the exact cost the fleet's Remember-first stance exists to avoid. **Sacca** demands the doc match the *current* artifact; **Sīlasāmaññatā** demands it follow the shared conventions so anyone can find and trust it. Separating `document` (capture) from `sync` (keep true) makes the maintenance duty explicit — a doc is not a write-once artifact but a claim about reality that must stay valid.

Working rules:

1. **Document the artifact you can see**, not the one you remember (read current source).
2. **Honor conventions**: placement, naming, and hard rules (bilingual parity, note-vs-changelog).
3. **Capture the *why* and the gotchas**, not just the *what* — that's what code can't tell the reader.
4. **Update in the same change** that changes the system; never leave the doc lying.
5. **Prune the obsolete.** A stale doc is a liability; retire it explicitly.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `document` | `subject` (the current artifact) | An accurate, convention-following record | Converges — re-documenting an unchanged subject reproduces the same record |
| `sync` | `doc` + `change` | The doc updated to match reality; drift flagged; obsolete pruned | Idempotent — sync on an already-current doc is a no-op |

Writes documentation files in the repo; landing follows the repo's own gates (parity checks, naming audits), so the operator-facing gate is at merge, not in the skill.

## Lifecycle Mapping

```
init       → read the current artifact + the repo's doc conventions
invoke     → document (capture) · sync (keep true on every change)
teardown   → no-op (the doc is the durable artifact)
```

## Maturity

**L1**. → L2 once two subjects have been documented + kept in `sync` across a real change without drift; → L3 once `bwoc skill verify documenter` is wired + green.

## Neutrality

Names no backend/model/vendor; conventions are the repo's own. Satisfies **Samānattatā**.

## See Also

- [[../writer/SPEC|writer]] — reader-facing prose craft (the sibling; accuracy ↔ experience).
- [[../engineering/SPEC|engineering]] — the work this skill records.
- [[../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sīlasāmaññatā, Sacca.
