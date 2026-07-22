---
title: Illustrator
aliases:
  - illustrator
tags:
  - group/framework-skills
  - type/skill
  - domain/media
maturity: L1
---

# Illustrator 🎨

> [!abstract] The craft of turning an intent into a **visual** — a diagram, illustration, thumbnail, character frame, or scene — by composing a precise prompt and iterating it through the workspace's configured image backend. The visual counterpart to [[../writer/SPEC|writer]]: same brief, different medium. Encodes **Sammā-diṭṭhi** (see the thing clearly before you depict it) and **Mattaññutā** (the right image, not the most ornate).

## What This Skill Does

Producing images is a recurring fleet task (video character frames, hero art, thumbnails, diagrams). This skill wraps the prompt→image loop so an agent gets a usable visual deliberately, rather than a lucky first shot.

Two operations are exposed:

- **`compose_prompt(intent)`** — turn an intent (subject · style · composition · aspect · constraints) into a concrete image prompt. Name the subject, the framing, the palette/mood, the aspect ratio, and the hard constraints (safe zone, no text, character consistency) explicitly — a vague prompt yields a vague image.
- **`generate(prompt)`** — render the prompt through the **workspace-configured image backend** (`<imageBackend>` — a local model, a job queue, or an OpenAI-compatible image endpoint), inspect the result, and iterate the prompt until it matches the intent.

## Why It Exists

An image carries intent the way a sentence does — and the same discipline applies: **see clearly first** (Sammā-diṭṭhi), then depict only what serves the brief (Mattaññutā). Centralising the loop keeps an agent from over-generating: compose a precise prompt, render, compare to intent, adjust one variable. Pairing it with `writer` means one brief can produce both the words and the picture in the same voice.

Working rules:

1. **Specify, don't wish.** Subject + framing + palette + aspect + constraints — every axis named.
2. **Iterate one variable.** Change one thing per render so you learn what moved the result.
3. **Respect the frame.** Aspect ratio and safe zones are part of the prompt, not an afterthought.
4. **Consistency is a constraint.** Recurring characters/brands get pinned, not re-rolled.

## Where The Backend Lives

`<imageBackend>` is **operator-configured** for the environment — a local diffusion model, an image job queue, or an OpenAI-compatible image endpoint. The skill teaches the compose→render→iterate *pattern*; the concrete backend + auth resolve from the workspace config, so the skill names no vendor.

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `compose_prompt` | `intent` (subject/style/aspect/constraints) | Return a concrete, fully-specified image prompt | Pure — re-running refines, doesn't mutate |
| `generate` | `prompt` | Render via `<imageBackend>`, return the image + iterate | Not idempotent (generation is stochastic); the loop converges on the intent |

`generate` reaches the configured image backend only — no external system of record — so it carries no operator-confirm gate (the deliverable is a file the operator places).

## Lifecycle Mapping

```
init       → resolve <imageBackend> from the workspace config
invoke     → compose_prompt → generate (loop until the image matches intent)
teardown   → no-op (the image is handed off)
```

Replay-safe; holds no state between invocations.

## Maturity

Declared **L1**. Bumps to L2 once two agents have shipped an image through `compose_prompt → generate`; to L3 once `bwoc skill verify illustrator` is wired + green.

## Neutrality

`<imageBackend>` is operator-configured — no hardcoded model/vendor. The verify command is a framework command. Satisfies **Samānattatā**.

## See Also

- [[../writer/SPEC|writer]] — the word counterpart to the same brief.
- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
