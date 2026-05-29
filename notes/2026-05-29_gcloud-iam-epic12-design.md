---
title: gcloud IAM (policy bindings) — architecture framing for EPIC-12
date: 2026-05-29
epic: BWOC-EPIC-12
tracking: "#99"
related: ["#86 (EPIC-8 foundation)", "#96 (EPIC-9 compute)", "#97 (EPIC-10 storage)", "#98 (EPIC-11 serverless)"]
status: frozen
---

# 2026-05-29 — gcloud IAM slice (EPIC-12 framing — LAST, highest blast radius)

> **Status: FROZEN (architect sign-off 2026-05-29).** Sets the spec frame for
> `BWOC-EPIC-12`, the **fourth and final** write-capable GCP slice and the
> deliberately-last one (only after compute/storage/serverless exercised the
> patterns). It introduces **T4** — the matrix's top tier — for the first time.
> The three open questions — the T4 opt-in mechanism, dangerous-grant guards,
> and the typed-confirm string — were resolved (see "Resolved"); the slice is
> implemented in this change.

The throughline: **an IAM mutation changes *who can do what*, and the danger is
not undoing it — it is the window of exposure while it stands.** A bad
`add-iam-policy-binding` can grant `roles/owner` to a stranger, or expose a
resource to `allUsers`, in one call. The grant is *reversible* (a matching
`remove` undoes it) — but unlike a stopped VM or a rolled-back revision, the
blast radius is **security**: during the window, an attacker may have already
used the access. So reversibility does **not** demote the tier. IAM writes are
**T4 — refuse-by-default + explicit opt-in, on top of T3 typed-name confirm.**

## Decisions

### 1. Scope — project-level policy reads + `add`/`remove` binding; defer everything else

`workflow/gcloud-iam` v1 verbs:

| Verb | Kind | gcloud |
|---|---|---|
| `get` | read | `gcloud projects get-iam-policy <project> --format=json` (current bindings) |
| `add` | **write (T4)** | `gcloud projects add-iam-policy-binding <project> --member=<m> --role=<r>` |
| `remove` | **write (T4)** | `gcloud projects remove-iam-policy-binding <project> --member=<m> --role=<r>` |

**Deferred — deliberately, and each more dangerous than a single binding:**

- **`set-iam-policy`** (wholesale policy replace) — a read-modify-write of the
  *entire* policy; one stale etag clobbers every binding. `add`/`remove` are the
  surgical, server-atomic primitives; `set` is never the right tool for an agent
  framework.
- **Service-account key creation** (`iam service-accounts keys create`) — *mints
  a long-lived credential*. Arguably higher risk than a binding (Adinnādāna: the
  framework's standing rule is that secrets never enter its surface). Out of
  scope, possibly forever.
- **Custom role CRUD** (`iam roles create/update/delete`), **SA create/delete**,
  and **non-project resource IAM** (bucket / Cloud Run / instance-level
  `add-iam-policy-binding`). v1 is **project-level bindings only**.

### 2. Plugin + auth — reuse the foundation

New `modules/plugins/workflow/gcloud-iam/` sourcing `../gcloud-auth/gcloud.sh`
(EPIC-8 §Decision 2); `workflow` kind; `auth.toml` shape-only; EN/TH `SPEC`
pair. `bwoc gcloud iam {get,add,remove}` dispatches it. Same option-injection
guard as #92 (`--`+`=`-bound args; member/role bound as `--flag=value`, project
positional after `--`). `bwoc check` auto-audits the manifest.

### 3. Risk-matrix instantiation — first use of T4

Same axes as the EPIC-9 template (tier = reversibility × blast radius).

| Verb | Mutation | Reversibility | Blast radius | Idempotent? | Confirmation tier |
|---|---|---|---|---|---|
| `get` | none | — | none (but discloses security posture) | yes | **T0 — none** (skill-gated; see §5) |
| `add` | grants `(member, role)` on the project | reversible (`remove`) — but exposure window is **not** undoable | **security** — escalates privilege immediately | yes (re-add = no-op) | **T4 — opt-in flag + typed-name confirm** |
| `remove` | revokes `(member, role)` | reversible (`add`) | **security/availability** — can lock out a live principal | yes (re-remove = no-op) | **T4 — opt-in flag + typed-name confirm** |

T4 = **everything T3 requires, plus a standing opt-in.** Concretely: the verb
**refuses by default** and runs only when the workspace has explicitly enabled
IAM writes via `[plugins.gcloud-iam] writes_enabled = true` in `workspace.toml`
**and** the operator clears a **typed-name confirm** (re-type the resolved
`member role`, echoed first). With no `writes_enabled`, `add`/`remove` error out
pointing at the config key — no ambient IAM-write path. `--json` requires
`--yes` **and** the standing enable (the typed confirm is the interactive gate;
`--yes` is the non-interactive equivalent, still fenced by `writes_enabled`).

### 4. Targeting + validation discipline

The footgun is granting the *wrong role* to the *wrong member*. So:

- **Require `--project`, `--member`, `--role`** for `add`/`remove` — no implicit
  default. The confirm echoes the **resolved** `project / member / role`.
- **`--member` validated against IAM principal syntax** — must carry a known
  prefix (`user:`, `serviceAccount:`, `group:`, `domain:`); a bare or
  `-`-leading value is rejected before the `--` guard.
- **Public principals `allUsers` / `allAuthenticatedUsers` are hard-refused.** A
  CLI agent tool never makes a resource public; the verb errors regardless of
  the standing enable or typed confirm. Liftable later only if a concrete need
  appears (Mattaññutā).
- **`--role` validated** against `roles/<name>` (predefined) or
  `projects/<p>/roles/<name>` (custom) shape. **High-privilege roles**
  (`roles/owner`, `roles/editor`, anything matching `*.admin` or `iam.*`) are
  **allowed but flagged** — the confirm prints an elevated-risk line above the
  typed prompt so the operator re-affirms knowingly. No exhaustive allowlist.
- **`--project` validated** via the existing `is_valid_project_id`.
- `--` option-injection guard unconditionally (#92); member/role/project never
  reach `gcloud` as parseable flags.

### 5. Skill exposure — NONE, not even reads

Stricter than every prior slice. Compute/storage/serverless exposed *reads* as a
candidate future skill; **IAM exposes nothing — not even `get`.** An IAM policy
read is reconnaissance (it maps who-can-do-what); the write verbs are
catastrophic. The entire `gcloud-iam` surface stays behind the operator-run
`bwoc gcloud iam` CLI. No `gcloud-ops` skill entry, ever.

### 6. Output — least disclosure

`get` projects the policy into a compact `{ bindings: [ { role, members:[…] } ] }`
envelope; drop `etag` and `auditConfigs` noise (we use the atomic `add`/`remove`
primitives, so no read-modify-write etag is needed). `add`/`remove` echo the
resulting `{ project, member, role, done }` — no full-policy dump.

## Resolved (architect sign-off 2026-05-29)

1. **T4 opt-in mechanism** → **workspace standing enable.** Writes refuse unless
   `[plugins.gcloud-iam] writes_enabled = true` is set in `workspace.toml`; then
   each call still clears the typed confirm. (The per-invocation-flag
   alternative was considered; the architect chose the standing config gate —
   one deliberate workspace decision to enable IAM writes at all, with the typed
   confirm as the per-call guard.) Without the key, `add`/`remove` error out
   naming the config key.
2. **Dangerous-grant guardrails** → **refuse public + warn high-privilege.**
   Hard-refuse `allUsers` / `allAuthenticatedUsers` members outright; **allow**
   high-privilege roles (`owner` / `editor` / `*.admin` / `iam.*`) but have the
   confirm print an elevated-risk line so the operator re-affirms knowingly. No
   exhaustive role allowlist.
3. **Typed-confirm string** → the exact resolved **`member role` pair** (echoed
   immediately above the prompt), so muscle-memory `y` can't fire and the
   operator re-affirms *which grant* — the IAM analogue of re-typing
   `gs://bucket/object` for storage `delete`.

## Alternatives considered

- **Include read-only IAM in the EPIC-8 foundation** — rejected at EPIC-8 (its
  alternatives §): even reads disclose security posture, so IAM waited for its
  own gated slice. Honored here (§5: no skill exposure even for `get`).
- **Demote `add`/`remove` to T3 because they are reversible** — rejected. The
  exposure window during a bad grant is not undoable; security blast radius
  pins the tier at T4 regardless of reversibility.
- **Support `set-iam-policy` for completeness** — rejected. Wholesale replace is
  strictly more dangerous (etag clobber) and never the right primitive here;
  `add`/`remove` are surgical and server-atomic.
- **Ship SA-key minting alongside** — rejected. Minting a long-lived credential
  violates the standing Adinnādāna rule (no secrets on the framework surface);
  out of scope, likely permanently.

## Status / deferred

- Decisions 2, 6 follow EPIC-8/9/10/11 precedent and are stable.
- **Decisions 1, 3, 4, 5** are frozen with the resolved answers above (the T4
  opt-in is a workspace standing enable; public principals refused; high-priv
  roles warned; typed confirm is the `member role` pair); implemented in this change.
- Live verification gated on an operator-provided GCP sandbox (a throwaway
  project + a test principal) at the smoke-test gate — IAM writes are never run
  against a real project in CI.
- This is the **last** tracked gcloud slice. `set-iam-policy`, SA-key minting,
  custom roles, and resource-level (non-project) IAM remain deferred with no
  successor epic unless a concrete need appears.

## Related

- Tracks #99 (EPIC-12, LAST). Builds on EPIC-8 (#86) + the EPIC-9 risk matrix (#96).
- [EPIC-9 compute design note](2026-05-28_gcloud-compute-epic9-design.md) — §3 the T0–T4 matrix (T4 defined here, used for the first time).
- [EPIC-10 storage design note](2026-05-29_gcloud-storage-epic10-design.md) — first T3 (typed-name) slice; T4 layers an opt-in on top.
- [EPIC-11 serverless design note](2026-05-29_gcloud-serverless-epic11-design.md) — prior slice (T2).
