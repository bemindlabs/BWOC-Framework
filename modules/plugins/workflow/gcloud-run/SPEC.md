---
title: gcloud-run — Google Cloud Run Serverless
aliases:
  - gcloud-run
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-run — Google Cloud Run Serverless

> [!abstract] The third write-capable GCP slice (`BWOC-EPIC-11`). It owns **Cloud Run services** — `list` / `describe` (read) and `deploy` (gated write). `deploy` is **T2** (confirm + echo the resolved target): a deploy mutates a live, traffic-serving service, but is reversible via revision rollback. Write gating lives in the `bwoc gcloud run` CLI, never in the plugin. Sources credential helpers from the sibling [[../gcloud-auth/SPEC|`gcloud-auth`]]. Full framing: [[../../../notes/2026-05-29_gcloud-serverless-epic11-design|EPIC-11 design note]].

## Why Cloud Run only (no Cloud Build, no delete)

v1 is service `list`/`describe`/`deploy`. `gcloud run deploy --source` triggers a server-side build, so a standalone `gcloud-build` (`builds submit`) is **deferred** to its own slice. `services delete` (removes a live service) and traffic-only splits are also deferred — `deploy` is the lowest-blast-radius write that earns the serverless pattern. Builds on the EPIC-8 auth foundation; stays the `workflow` kind.

## Verbs

| Operation | Direction | Auth | HTTP / side effect | Risk tier | Gate |
|---|---|---|---|---|---|
| `list` | read | required | `gcloud run services list [--region <r>]` | T0 | none |
| `describe` | read | required | `gcloud run services describe <svc> --region <r>` | T0 | none |
| `deploy` | **write** | required | `gcloud run deploy <svc> --region <r> {--image <img> \| --source <dir>}` | **T2** | **confirm + echo `service/region/source/traffic`** (CLI) |

Tiers are the reusable scale from the [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|EPIC-9 risk matrix]] (tier = reversibility × blast radius). `deploy` = T2: reversible (revision rollback) with a service-wide availability blast radius.

## How it runs

The `bwoc gcloud run` CLI spawns `gcloud.sh` from this directory:

| Channel | What it carries |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `list` \| `describe` \| `deploy` — fallback for `.operation`. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; the sibling SA JSON path resolves under it. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory; finds `../gcloud-auth/gcloud.sh`. |
| stdin | One-line JSON, e.g. `{"operation":"deploy","service":"api","region":"us-central1","image":"gcr.io/p/api:v2"}`. |

> [!warning] Option-injection guard (#92). Operator values reach `gcloud` as `--flag=value` (region/image/source bound) or as a positional **after a `--` separator** (the service name); a `-`-leading value can't be parsed as a flag. The CLI validates service/region names and canonicalizes `--source` to an absolute path before dispatch.

## Authentication

This plugin **never reads any credential value**. It sources the sibling [[../gcloud-auth/SPEC#Authentication|`gcloud-auth`]] helpers and asks `gcloud` for Cloud Run state. [[auth|auth.toml]] declares the **same** shape-only contract. Precedence: ADC → `.bwoc/secrets/gcloud-sa.json` → `BWOC_GCLOUD_*` env.

> [!danger] **Sila — Adinnādāna.** No token enters this plugin's address space. `deploy` transmits only the service/region/image/source to the local `gcloud` CLI.

## Output shapes

### `list`

```json
{ "ok": true, "plugin": "gcloud-run", "operation": "list", "total": 1,
  "services": [ { "name": "api", "region": "us-central1", "url": "https://api-xxxx.run.app", "ready": "True" } ] }
```

### `describe`

```json
{ "ok": true, "plugin": "gcloud-run", "operation": "describe",
  "service": "api", "region": "us-central1", "url": "https://api-xxxx.run.app",
  "latest_ready_revision": "api-00007-abc", "latest_created_revision": "api-00007-abc",
  "traffic": [ { "revision": "api-00007-abc", "percent": 100, "latest": true } ] }
```

### `deploy`

```json
{ "ok": true, "plugin": "gcloud-run", "operation": "deploy",
  "service": "api", "region": "us-central1", "url": "https://api-xxxx.run.app",
  "latest_ready_revision": "api-00008-def" }
```

`gcloud run deploy` runs with `--quiet` (the BWOC CLI owns the T2 confirmation); the envelope reports the resulting URL + new ready revision.

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout. |
| `1` | dependency | `jq` missing, sibling `gcloud-auth/gcloud.sh` absent, or `gcloud` missing. |
| `2` | usage | Unknown operation; `describe`/`deploy` without `.service`+`.region`; `deploy` with neither or both of `.image`/`.source`. |
| `3` | not-authenticated | No active `gcloud` credential. |
| `6` | gcloud-error | The underlying `gcloud` command failed; truncated diagnostic on stderr. |

## Configuration

```toml
# workspace.toml
[plugins.gcloud-run]
enabled = true
```

No `[config.schema]` — service state is queried live. Only the universal `enabled` key.

## Lifecycle mapping

`workflow` kind owner is the operator via the `bwoc gcloud run` CLI; no local state beyond `gcloud`'s cache. `init` sources the helpers + checks `jq`; `invoke` runs the verb; `teardown` is implicit.

## Idempotency

- `list` / `describe` are read-only.
- `deploy` is effectively idempotent: re-deploying the same image+config converges to an equivalent serving revision. A `--source` deploy rebuilds; the resulting revision is functionally equivalent for the same source.

## Maturity

Declared **L1** — first runnable `workflow/gcloud-run` plugin; all three verbs functional. Bumps to **L2** once smoke tests exercise the verbs end-to-end against an authenticated `gcloud` with a deployable service.

> [!warning] Live-test gap. End-to-end against a real Cloud Run service is gated on an operator-provided sandbox (design note §Status). v0.1.0 is verified by `bash -n` + shellcheck, the missing-sibling/unauthenticated paths erroring cleanly, and `bwoc check` accepting the manifest.

## Neutrality

Manifest values name no LLM backend or model. `kind = "workflow"` is the framework's enum. "gcloud" / "Cloud Run" appear only in `description` + this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-29_gcloud-serverless-epic11-design|EPIC-11 design note]] — framing + the T2 deploy rationale.
- [[../gcloud-auth/SPEC|gcloud-auth SPEC]] — sibling plugin (credential state); helpers sourced here.
- [[../gcloud-compute/SPEC|gcloud-compute SPEC]] — EPIC-9 sibling; the risk matrix this slice reuses.
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `workflow` kind row.
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
