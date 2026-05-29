---
title: gcloud-iam — Google Cloud IAM project bindings
aliases:
  - gcloud-iam
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-iam — Google Cloud IAM project bindings

> [!abstract] The fourth and **LAST** write-capable GCP slice (`BWOC-EPIC-12`) — the highest blast radius, deliberately built last. It owns **project IAM policy** — `get` (read) and `add` / `remove` of a `(member, role)` binding (gated write). Both writes are **T4 — refuse-by-default + standing opt-in, on top of a T3 typed-name confirm**: an IAM mutation changes *who can do what*, and the exposure window during a bad grant is not undoable. Write gating lives entirely in the `bwoc gcloud iam` CLI, never in the plugin. Sources credential helpers from the sibling [[../gcloud-auth/SPEC|`gcloud-auth`]]. Full framing: [[../../../notes/2026-05-29_gcloud-iam-epic12-design|EPIC-12 design note]].

> [!danger] **T4 — the top of the risk matrix.** `add`/`remove` run only when (1) the workspace explicitly enables IAM writes via `[plugins.gcloud-iam] writes_enabled = true`, **and** (2) the operator clears a typed-name confirm (re-type the resolved `member role`). Public principals (`allUsers` / `allAuthenticatedUsers`) are **hard-refused**. High-privilege roles (`owner` / `editor` / `*.admin` / `iam.*`) are allowed but flagged with an elevated-risk warning.

## Why project bindings only (no set-policy, no SA keys)

v1 is project-level `get` + `add`/`remove` of a single binding. **Deferred — each strictly more dangerous than one binding:** `set-iam-policy` (wholesale policy replace; one stale etag clobbers every binding — `add`/`remove` are the surgical, server-atomic primitives), **service-account key creation** (mints a long-lived credential — violates the standing Adinnādāna rule, likely deferred forever), custom-role CRUD, SA create/delete, and non-project resource IAM (bucket / Cloud Run / instance level). Builds on the EPIC-8 auth foundation; stays the `workflow` kind.

## Verbs

| Operation | Direction | Auth | HTTP / side effect | Risk tier | Gate |
|---|---|---|---|---|---|
| `get` | read | required | `gcloud projects get-iam-policy <project>` | T0 | none (but never skill-exposed — discloses security posture) |
| `add` | **write** | required | `gcloud projects add-iam-policy-binding <project> --member=<m> --role=<r>` | **T4** | **standing `writes_enabled` + typed `member role` confirm** (CLI) |
| `remove` | **write** | required | `gcloud projects remove-iam-policy-binding <project> --member=<m> --role=<r>` | **T4** | **standing `writes_enabled` + typed `member role` confirm** (CLI) |

Tiers are the reusable scale from the [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|EPIC-9 risk matrix]] (tier = reversibility × blast radius). IAM writes are **T4**: reversible (a matching `remove`/`add` undoes the binding) but with a **security** blast radius — the exposure window is not undoable — so reversibility does not demote the tier.

## How it runs

The `bwoc gcloud iam` CLI spawns `gcloud.sh` from this directory:

| Channel | What it carries |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `get` \| `add` \| `remove` — fallback for `.operation`. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; the sibling SA JSON path resolves under it. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory; finds `../gcloud-auth/gcloud.sh`. |
| stdin | One-line JSON, e.g. `{"operation":"add","project":"p","member":"user:x@y.com","role":"roles/viewer"}`. |

> [!warning] Option-injection guard (#92). Operator values reach `gcloud` as `--flag=value` (member/role bound) or as a positional **after a `--` separator** (the project id); a `-`-leading value can't be parsed as a flag. The CLI validates the project id, the member's IAM-principal syntax, and the role shape, refuses the public principals, and flags high-privilege roles **before** dispatch.

## Authentication

This plugin **never reads any credential value**. It sources the sibling [[../gcloud-auth/SPEC#Authentication|`gcloud-auth`]] helpers and asks `gcloud` for IAM state. [[auth|auth.toml]] declares the **same** shape-only contract. Precedence: ADC → `.bwoc/secrets/gcloud-sa.json` → `BWOC_GCLOUD_*` env.

> [!danger] **Sila — Adinnādāna.** No token enters this plugin's address space. `add`/`remove` transmit only the project/member/role to the local `gcloud` CLI. The plugin never mints a credential (no SA-key creation — out of scope by design).

## Output shapes

### `get`

```json
{ "ok": true, "plugin": "gcloud-iam", "operation": "get", "project": "my-proj",
  "bindings": [ { "role": "roles/viewer", "members": ["user:x@y.com"] } ] }
```

### `add` / `remove`

```json
{ "ok": true, "plugin": "gcloud-iam", "operation": "add",
  "project": "my-proj", "member": "user:x@y.com", "role": "roles/viewer", "present": true }
```

`present` reflects whether the `(member, role)` pair is in the policy returned by the mutation — `true` after a successful `add`, `false` after a `remove`. The `etag` and `auditConfigs` are dropped (we use the atomic `add`/`remove` primitives, so no read-modify-write etag is needed).

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout. |
| `1` | dependency | `jq` missing, sibling `gcloud-auth/gcloud.sh` absent, or `gcloud` missing. |
| `2` | usage | Unknown operation; `get` with no resolvable project; `add`/`remove` missing `.project`/`.member`/`.role`. |
| `3` | not-authenticated | No active `gcloud` credential. |
| `6` | gcloud-error | The underlying `gcloud` command failed; truncated diagnostic on stderr. |

## Configuration

```toml
# workspace.toml
[plugins.gcloud-iam]
enabled = true
# REQUIRED to allow any IAM write. Without it, `bwoc gcloud iam add/remove`
# refuses by default (T4 standing opt-in). Reads (`get`) never need it.
writes_enabled = true
```

The `writes_enabled` gate is read by the **CLI**, not this plugin — the plugin only ever sees an already-vetted request. Reads work with `enabled = true` alone.

## Lifecycle mapping

`workflow` kind owner is the operator via the `bwoc gcloud iam` CLI; no local state beyond `gcloud`'s cache. `init` sources the helpers + checks `jq`; `invoke` runs the verb; `teardown` is implicit.

## Idempotency

- `get` is read-only.
- `add` is idempotent: re-adding an existing `(member, role)` is a server-side no-op (the binding is already present).
- `remove` is idempotent: removing an absent `(member, role)` leaves the policy unchanged.

## Maturity

Declared **L1** — first runnable `workflow/gcloud-iam` plugin; all three verbs functional. Bumps to **L2** once smoke tests exercise the verbs end-to-end against an authenticated `gcloud` with a throwaway project + test principal.

> [!warning] Live-test gap. End-to-end against a real project IAM policy is gated on an operator-provided sandbox (a throwaway project + a disposable principal) — IAM writes are never run against a real project in CI (design note §Status). v0.1.0 is verified by `bash -n` + shellcheck, the missing-sibling/unauthenticated paths erroring cleanly, and `bwoc check` accepting the manifest.

## Neutrality

Manifest values name no LLM backend or model. `kind = "workflow"` is the framework's enum. "gcloud" / "IAM" appear only in `description` + this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-29_gcloud-iam-epic12-design|EPIC-12 design note]] — framing + the T4 rationale (first use of the matrix's top tier).
- [[../gcloud-auth/SPEC|gcloud-auth SPEC]] — sibling plugin (credential state); helpers sourced here.
- [[../gcloud-storage/SPEC|gcloud-storage SPEC]] — EPIC-10 sibling; first T3 (typed-name) slice that T4 layers an opt-in on top of.
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `workflow` kind row.
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
