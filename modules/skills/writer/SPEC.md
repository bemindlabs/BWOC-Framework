---
title: Writer
aliases:
  - writer
tags:
  - group/framework-skills
  - type/skill
  - domain/content
maturity: L1
---

# Writer ✍️

> [!abstract] The craft of writing well for a reader — articles, docs, release notes, social posts, video scripts, UI copy — in **their** language and the fleet's terse, truthful voice. Encodes **Sammā-vācā** (right speech): say what is true, useful, and kind, at the right time, in the right amount.

## What This Skill Does

Writing is a recurring fleet task (handbooks, READMEs, changelogs, party posts, video scripts, invite copy). This skill wraps the discipline so an agent produces a clear draft and improves it deliberately, instead of emitting a wall of text.

Two operations are exposed:

- **`draft(brief)`** — turn a brief (audience + purpose + key points) into a first draft. Lead with the result; match the reader's language (Thai ↔ English as the reader writes); pick the form that fits (bullets for scanning, prose for narrative, a table for comparison).
- **`revise(draft, note)`** — sharpen an existing draft against a note or a self-critique: cut preamble and hedging, tighten every line to earn its place, fix the one claim that isn't verifiably true, and keep the reader's voice.

## Why It Exists

Good writing is **Sammā-vācā** made concrete: *true* (no fabricated confidence), *useful* (leads with what the reader needs), *timely* (fits the moment), *kind* (respects the reader's attention). Centralising the craft as a skill keeps every agent honest about those four tests and about **Mattaññutā** — the smaller, sharper piece beats the longer, complete one.

The skill's working rules:

1. **Lead with the result.** No throat-clearing; the first line carries the point.
2. **Match the reader's language + register.** Write to *this* audience, not a generic one.
3. **Every line earns its place.** Cut a sentence that doesn't add signal (Mattaññutā).
4. **Truth over polish.** A claim you can't verify gets softened or dropped, never dressed up.
5. **Form follows purpose.** Bullets, prose, table, or code block — chosen, not defaulted.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `draft` | `brief` (audience · purpose · points) | Produce a first draft in the reader's language + fitting form | Pure generation — re-running yields another draft, not a mutation |
| `revise` | `draft` + `note` (or self-critique) | Return a tightened draft: cut, verify, keep voice | Converges — repeated revision approaches a stable, lean text |

Both are observed by **Dhammānupassanā** (which register/audience is in force). Neither writes to an external system — the artifact is text the operator places.

## Lifecycle Mapping

```
init       → read the brief: who reads this, why, what must land
invoke     → draft → revise (loop until lean + true)
teardown   → no-op (the text is handed off, not skill-scoped state)
```

Holds no state between invocations. Replay-safe.

## Maturity

Declared **L1**. Bumps to L2 once two agents have shipped reader-facing text through `draft → revise`; to L3 once `bwoc skill verify writer` is wired + green.

## Neutrality

Names no backend, model, or vendor. The craft is language-agnostic and reader-first; the verify command is a framework command. Satisfies **Samānattatā**.

## See Also

- [[../illustrator/SPEC|illustrator]] — the visual counterpart (words ↔ images for the same brief).
- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
- [[../../agent-template/docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sammā-vācā, Mattaññutā framing.
