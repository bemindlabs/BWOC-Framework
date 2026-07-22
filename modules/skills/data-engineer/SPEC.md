---
title: Data Engineer
aliases:
  - data-engineer
tags:
  - group/framework-skills
  - type/skill
  - domain/data
maturity: L1
---

# Data Engineer 🛢️

> [!abstract] The craft of **moving data reliably** — idempotent, observable pipelines from ingest → transform → store — and guaranteeing what comes out is trustworthy (schema, validation, lineage, freshness). Data plumbing is a system, so this is the data-plane sibling of [[../systems-engineer/SPEC|systems-engineer]]. Encodes **Sacca** (the data must be *true* to its source) and **Sīla** (pipelines are gated + tested like any other engineering).

## What This Skill Does

Two operations are exposed:

- **`build_pipeline(spec)`** — build the flow: ingest from the source, transform to the target shape, and land it in the store — **idempotent** (a re-run doesn't double-count), **observable** (you can see rows in/out, lag, failures), and recoverable (a failed run is safe to retry). Handle late/duplicate/malformed records explicitly, not by hoping.
- **`ensure_data_quality(dataset)`** — guarantee the output: validate schema + types, assert the invariants (uniqueness, referential integrity, ranges), check freshness (is it current?), and track lineage (where did this come from, what transformed it). A silent quality failure poisons every downstream consumer, so it must fail loud.

## Why It Exists

Downstream analytics, ML, and decisions are only as true as the pipeline that fed them — **Sacca** starts at ingestion. A pipeline that silently drops or duplicates rows is worse than a broken one, because the wrongness is invisible. Separating `build_pipeline` (move it reliably) from `ensure_data_quality` (prove it's right) makes the guarantee explicit: data isn't "moved," it's moved *correctly and observably*, and its quality is asserted, not assumed.

Working rules:

1. **Idempotent by design.** A re-run reproduces the same result — no double-counting.
2. **Observable.** Rows in/out, lag, and failures are visible; a silent pipeline is a liability.
3. **Handle the ugly records explicitly** — late, duplicate, malformed, schema-drifted.
4. **Assert quality, fail loud.** Schema + invariants + freshness checks that block bad data, never warn-and-continue.
5. **Track lineage.** Every dataset can name its source + transforms (Sacca — traceable to truth).

## Operations Contract

| Operation | Input | Effect | Idempotency |
|---|---|---|---|
| `build_pipeline` | `spec` (source · transform · sink) | An idempotent, observable, recoverable pipeline | The *pipeline* is idempotent by construction; building it converges |
| `ensure_data_quality` | a `dataset` | Schema/invariant/freshness assertions + lineage; blocks bad data | Pure checks — repeatable |

Building runs through the repo's own engineering gates; a pipeline that writes to a production store follows the operator-confirm the framework applies to durable external writes.

## Lifecycle Mapping

```
init       → know the source, the target shape, and the quality bar
invoke     → build_pipeline (reliable move) → ensure_data_quality (prove it)
teardown   → hand a trustworthy dataset to data-science / consumers
```

## Maturity

**L1**. → L2 once two pipelines have run idempotently with quality checks catching a real defect; → L3 once `bwoc skill verify data-engineer` is wired + green.

## Neutrality

Names no backend/model/vendor; a store-agnostic data craft. Satisfies **Samānattatā**.

## See Also

- [[../data-scientist/SPEC|data-scientist]] — the consumer of the trustworthy data this skill produces.
- [[../systems-engineer/SPEC|systems-engineer]] — pipelines are systems; the reliability stance is shared.
- [[../../../docs/en/PHILOSOPHY.en|PHILOSOPHY.en.md]] — Sacca, Sīla.
- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to.
