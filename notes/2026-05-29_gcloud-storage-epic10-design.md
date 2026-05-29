---
title: gcloud Storage — architecture framing for EPIC-10
date: 2026-05-29
epic: BWOC-EPIC-10
tracking: "#97"
related: ["#86 (EPIC-8 foundation)", "#96 (EPIC-9 compute)", "#98", "#99"]
status: frozen
---

# 2026-05-29 — gcloud storage slice (EPIC-10 framing)

> **Status: FROZEN (architect sign-off 2026-05-29) + implemented.** Sets the
> spec frame for `BWOC-EPIC-10` (the second write-capable GCP slice). The three
> open questions on decisions 3–4 were resolved with the recommended answers
> (see "Resolved"); the slice is implemented in this change.

The throughline: **storage is where a write first becomes *irreversible*.** A
misfired `compute stop` (EPIC-9) is undone by `start`; a misfired
`objects.delete` is gone (unless the bucket has versioning, which the operator
controls, not us). So `objects.delete` is the **first use of the matrix's T3
tier** (typed-name confirmation), and `objects.put` — which silently overwrites
an existing object — is treated as data-loss-adjacent, not a plain create.

## Decisions

### 1. Scope — object-level only

`workflow/gcloud-storage` v1 verbs:

| Verb | Kind | gcloud |
|---|---|---|
| `list` | read | `gcloud storage ls gs://<bucket>[/<prefix>]` |
| `stat` | read | `gcloud storage objects describe gs://<bucket>/<object>` |
| `put` | **write** | `gcloud storage cp <local> gs://<bucket>/<object>` |
| `delete` | **write** | `gcloud storage rm gs://<bucket>/<object>` |

**Deferred:** bucket lifecycle (`buckets create`/`delete` — a bucket delete is
catastrophic: every object at once), `rsync`/recursive deletes, ACL/IAM on
objects (that's the EPIC-12 IAM surface), signed URLs. v1 is single-object
read + put/delete. Recursive/bulk operations are their own future slice with
their own (higher) gating.

### 2. Plugin + auth — reuse the foundation

New `modules/plugins/workflow/gcloud-storage/` sourcing `../gcloud-auth/gcloud.sh`
for credential resolution (EPIC-8 §Decision 2); `workflow` kind; `auth.toml`
shape-only; EN/TH `SPEC` pair. `bwoc gcloud storage {list,stat,put,delete}`
dispatches it. Same option-injection guard as #92 (`--`+`=`-bound args).

### 3. Risk-matrix instantiation (first T3 use)

Same axes as the EPIC-9 template (Mutation / Reversibility / Blast radius /
Idempotent? / Tier); the tier is a function of **reversibility × blast radius**.

| Verb | Mutation | Reversibility | Blast radius | Idempotent? | Confirmation tier |
|---|---|---|---|---|---|
| `list` / `stat` | none | — | none | yes | **T0 — none** |
| `put` (new object) | creates an object | trivial (`delete`) | data (single object) + cost | yes | **T1 — confirm** |
| `put` (overwrites existing) | replaces object bytes | **lossy** (old bytes gone unless versioned) | data (one object's prior bytes) | no | **T2 — confirm + echo target** |
| `delete` | removes an object | **irreversible** (unless bucket versioning) | data (one object, permanent) | yes (gone→gone) | **T3 — typed-name confirm** |

This is the first slice to exercise **T3**: the operator must re-type the
object path (`gs://bucket/object`), not just `y`, to delete. `put` is T1/T2
depending on whether it would overwrite (decision 4).

### 4. Overwrite handling for `put` — the new surface

A `put` to a path that already holds an object **silently replaces** it. Options
considered:

- **Always T1 (plain confirm).** Treats overwrite like a create — silently
  destroys the previous bytes on a typo'd path. Rejected (Musāvāda-adjacent: the
  prompt says "upload" while it means "replace").
- **Stat-first, escalate to T2 on overwrite (recommended).** The CLI stats the
  target; if an object exists, the prompt says "**overwrite** gs://… (current
  size/updated shown)?" — T2 with the existing object echoed. A new path stays
  T1. Honest about which case the operator is in.
- **Refuse overwrite without `--force`.** Safest, but adds a flag and a
  two-step dance for the common "re-upload" case. Heavier than the risk warrants
  for a reversible-if-versioned op.

### 5. Skill exposure — reads only

`put`/`delete` are operator-CLI-only (EPIC-8 §Decision 5 / EPIC-9 precedent —
agents never autonomously delete objects). A future read-only `storage-inventory`
addition to a skill (`list`/`stat`) is fine; writes stay behind the gated CLI.

## Resolved (architect sign-off 2026-05-29)

1. **`delete` tier** → **T3** (re-type the `gs://bucket/object`). It is the first
   irreversible verb; typed-name friction is proportionate to "gone forever".
2. **`put` overwrite** → **stat-first**: T1 for a new path, escalate to **T2**
   (echo the existing object) when it would overwrite. No `--force` flag.
3. **v1 scope** → **object-level only** (`list`/`stat`/`put`/`delete`). Bucket
   lifecycle and recursive/bulk ops deferred to their own future slices.

## Alternatives considered

- **Recursive `rm -r` / `rsync` in v1** — rejected. Bulk/recursive deletes are a
  far larger blast radius than single-object; they earn their own slice with
  stricter gating once single-object is proven.
- **Treat `delete` as T2 like compute `stop`** — rejected. The matrix sets the
  tier from **reversibility × blast radius**: `stop` and object `delete` share a
  single-resource blast radius, but `stop` is reversible (→ T2) while `delete`
  is irreversible (→ T3). The reversibility axis is what separates them here.

## Status / deferred

- Decisions 1, 2, 5 follow EPIC-8/9 precedent and are stable.
- **Decisions 3–4 frozen** with the resolved answers above; implemented in this change.
- Live verification gated on an operator-provided sandbox (a test bucket + a
  throwaway object) at the smoke-test gate.
- T4 (security/refuse+opt-in) remains unused until EPIC-12 (IAM).

## Related

- Tracks #97 (EPIC-10). Builds on EPIC-8 (#86) + the EPIC-9 risk matrix (#96).
- [EPIC-9 compute design note](2026-05-28_gcloud-compute-epic9-design.md) — §3 the T0–T4 matrix this slice instantiates at T3.
- [EPIC-8 foundation note](2026-05-28_gcloud-workflow-plugin-architecture.md) — §7 slice roadmap.
- Future siblings: #98 (serverless), #99 (IAM).
