---
title: gcloud Serverless (Cloud Run) — architecture framing for EPIC-11
date: 2026-05-29
epic: BWOC-EPIC-11
tracking: "#98"
related: ["#86 (EPIC-8 foundation)", "#96 (EPIC-9 compute)", "#97 (EPIC-10 storage)", "#99"]
status: frozen
---

# 2026-05-29 — gcloud serverless slice (EPIC-11 framing)

> **Status: FROZEN (architect sign-off 2026-05-29) + implemented.** Sets the
> spec frame for `BWOC-EPIC-11` (the third write-capable GCP slice). The three
> open questions — the `deploy` tier (Decision 3), the Cloud Build scope
> (Decision 1), and traffic handling (Decision 4) — were resolved with the
> recommended answers (see "Resolved"); the slice is implemented in this change.

The throughline: **a deploy mutates a *live, traffic-serving* service.** Unlike
compute `stop` (one instance) or object `delete` (one object), a bad Cloud Run
deploy can serve errors to every caller of that service at once — but it is
**reversible**: Cloud Run keeps prior revisions, and traffic can be rolled back.
So `deploy` lands at **T2** (confirm + echo the resolved target), not T3 — the
blast radius is availability, the reversibility is real (roll back the revision).

## Decisions

### 1. Scope — Cloud Run service deploy + reads; defer Cloud Build

`workflow/gcloud-run` v1 verbs:

| Verb | Kind | gcloud |
|---|---|---|
| `list` | read | `gcloud run services list --region <r>` |
| `describe` | read | `gcloud run services describe <svc> --region <r>` (URL, latest revision, traffic split) |
| `deploy` | **write** | `gcloud run deploy <svc> --region <r> {--image <img> \| --source <dir>}` |

**Deferred:** a separate `gcloud-build` plugin (`builds submit`) — `gcloud run
deploy --source` already triggers a server-side build, covering the common
"build + deploy from source" path without a second plugin. A standalone
`gcloud-build` (raw `builds submit`, build triggers) is its own slice if a
concrete need appears. Also deferred: `services delete` (closer to T3 — removes
a live service), traffic-only splits (`services update-traffic`), and domain
mappings. v1 is **list / describe / deploy**.

### 2. Plugin + auth — reuse the foundation

New `modules/plugins/workflow/gcloud-run/` sourcing `../gcloud-auth/gcloud.sh`
(EPIC-8 §Decision 2); `workflow` kind; `auth.toml` shape-only; EN/TH `SPEC`
pair. `bwoc gcloud run {list,describe,deploy}` dispatches it. Same
option-injection guard as #92 (`--`+`=`-bound args).

### 3. Risk-matrix instantiation

Same axes as the EPIC-9 template (tier = reversibility × blast radius).

| Verb | Mutation | Reversibility | Blast radius | Idempotent? | Confirmation tier |
|---|---|---|---|---|---|
| `list` / `describe` | none | — | none | yes | **T0 — none** |
| `deploy` | new revision; shifts serving traffic | reversible (roll back to prior revision) | **availability** — every caller of the service | yes-ish (same image+config → same revision state) | **T2 — confirm + echo resolved service / region / source + traffic intent** |

`deploy` is T2: reversible (like compute `stop`) but with a service-wide
availability blast radius, so the prompt **echoes the full target and whether
traffic shifts** — the operator confirms *which* service in *which* region is
about to take new traffic.

### 4. What `deploy` consumes + traffic

- **Require `--service` + `--region`** (no implicit default — deploying to the
  wrong service/region is the footgun). Validate both (RFC 1035-ish names).
- **Require exactly one of `--image <ref>` or `--source <dir>`.** `--image`
  deploys a prebuilt container; `--source` triggers a server-side build then
  deploys (subsumes the deferred Cloud Build path for the common case).
- The confirm echoes `service / region / {image|source} / traffic`.

### 5. Skill exposure — reads only

`deploy` is operator-CLI-only (EPIC-8 §Decision 5 / EPIC-9/10 precedent — agents
never autonomously ship a revision). A future read-only `run-inventory` skill
addition (`list`/`describe`) is fine; the write verb stays behind the gated CLI.

## Resolved (architect sign-off 2026-05-29)

1. **`deploy` tier** → **T2** (confirm + echo resolved target). A deploy is
   reversible via revision rollback; T3 typed-name would over-prompt a routine
   deploy. The echoed `service / region / source / traffic` covers the
   wrong-target risk.
2. **Cloud Build scope** → **`gcloud-run` only.** `run deploy --source` covers
   build+deploy; a standalone `gcloud-build` (`builds submit`) is its own future
   slice.
3. **Traffic on deploy** → **keep the default 100%-routing**, echoed in the
   prompt ("routes 100% traffic to the new revision"). `--no-traffic` is a cheap
   follow-up if a concrete need appears.

## Alternatives considered

- **Treat `deploy` as T3 (irreversible-style)** — rejected. Cloud Run revisions
  make a deploy reversible (roll back traffic); T3's typed-name friction fits
  `delete`-class verbs, not a routine reversible deploy. The availability blast
  radius is handled by T2's echoed target.
- **Ship `services delete` in v1** — rejected. Deleting a live service is a
  larger, less-reversible commitment than a revision deploy; defer with its own
  (higher) gating, like compute `delete`/`reset` were deferred.
- **A separate `gcloud-build` plugin in v1** — deferred. `run deploy --source`
  covers build+deploy; a raw `builds submit` plugin earns its own slice when a
  build-without-deploy need is concrete.

## Status / deferred

- Decisions 2 and 5 follow EPIC-8/9/10 precedent and are stable.
- **Decisions 1, 3, 4 frozen** with the resolved answers above — Decision 1's
  scope incl. the Cloud Build deferral (Open question 2), the `deploy` tier
  (3), and traffic handling (4); implemented in this change.
- Live verification gated on an operator-provided sandbox (a deployable Cloud
  Run service + a test image/source) at the smoke-test gate.
- T3/T4 remain for `delete`-class (future) and IAM (EPIC-12).

## Related

- Tracks #98 (EPIC-11). Builds on EPIC-8 (#86) + the EPIC-9 risk matrix (#96).
- [EPIC-9 compute design note](2026-05-28_gcloud-compute-epic9-design.md) — §3 the T0–T4 matrix.
- [EPIC-10 storage design note](2026-05-29_gcloud-storage-epic10-design.md) — the prior write slice (T3).
- Future sibling: #99 (IAM, EPIC-12 — the last + highest-blast-radius slice).
