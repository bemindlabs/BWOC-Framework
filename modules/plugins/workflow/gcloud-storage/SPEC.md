---
title: gcloud-storage — Google Cloud Storage Objects
aliases:
  - gcloud-storage
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-storage — Google Cloud Storage Objects

> [!abstract] The second write-capable GCP slice (`BWOC-EPIC-10`), and the first with an **irreversible** write. It owns **object operations** — `list` / `stat` (read) and `put` / `delete` (gated writes). `delete` is **T3** (typed-name confirmation) because object deletion is permanent; `put` is **stat-first** (T1 for a new path, T2 when it would overwrite). Writes are gated in the `bwoc gcloud storage` CLI, never in the plugin. Sources credential helpers from the sibling [[../gcloud-auth/SPEC|`gcloud-auth`]]. Full framing: [[../../../notes/2026-05-29_gcloud-storage-epic10-design|EPIC-10 design note]].

## Why object-level only

v1 is single-object read + write. **Bucket lifecycle** (`buckets create`/`delete` — a bucket delete drops every object at once) and **recursive/bulk** ops (`rm -r`, `rsync`) are deferred to their own future slices with stricter gating. EPIC-10 validates the irreversible-write pattern (T3) on the smallest blast radius first. Builds on the EPIC-8 auth foundation (sources `gcloud-auth`); stays the `workflow` kind.

## Verbs

| Operation | Direction | Auth | HTTP / side effect | Risk tier | Gate |
|---|---|---|---|---|---|
| `list` | read | required | `gcloud storage ls gs://<bucket>[/<prefix>]` | T0 | none |
| `stat` | read | required | `gcloud storage objects describe gs://<bucket>/<object>` (returns `exists:false` on a clean not-found) | T0 | none |
| `put` | **write** | required | `gcloud storage cp <local> gs://<bucket>/<object>` | T1 / **T2** | **confirm** (T2 + echo existing object when overwriting) |
| `delete` | **write (irreversible)** | required | `gcloud storage rm gs://<bucket>/<object>` | **T3** | **typed-name confirm** (re-type `gs://bucket/object`) |

Tiers are the reusable scale from the [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|EPIC-9 risk matrix]]. EPIC-10 is the first slice to use **T3**.

## How it runs

The `bwoc gcloud storage` CLI spawns `gcloud.sh` from this directory:

| Channel | What it carries |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `list` \| `stat` \| `put` \| `delete` — fallback for `.operation`. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; the sibling SA JSON path resolves under it. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory; finds `../gcloud-auth/gcloud.sh`. |
| stdin | One-line JSON, e.g. `{"operation":"put","bucket":"b","object":"a.txt","local":"./a.txt"}`. |

> [!warning] Option-injection guard (#92). Every operator value reaches `gcloud` as a `--flag=value` or as a positional **after a `--` separator** (the `gs://…` URL and the local path), so a `-`-leading value can't be parsed as a flag. The CLI validates bucket/object names before dispatch.

## Authentication

This plugin **never reads any credential value**. It sources the sibling [[../gcloud-auth/SPEC#Authentication|`gcloud-auth`]] helpers and asks `gcloud` for object state. [[auth|auth.toml]] declares the **same** shape-only contract. Precedence: ADC → `.bwoc/secrets/gcloud-sa.json` → `BWOC_GCLOUD_*` env.

> [!danger] **Sila — Adinnādāna.** No token enters this plugin's address space. `put`/`delete` transmit only the bucket/object/local-path to the local `gcloud` CLI.

## Output shapes

### `list`

```json
{ "ok": true, "plugin": "gcloud-storage", "operation": "list", "bucket": "b",
  "total": 2,
  "objects": [
    { "url": "gs://b/a.txt", "size": 12, "updated": "2026-05-29T08:00:00Z" },
    { "url": "gs://b/logs/", "size": null, "updated": null }
  ] }
```

### `stat`

```json
{ "ok": true, "plugin": "gcloud-storage", "operation": "stat",
  "exists": true, "bucket": "b", "object": "a.txt",
  "size": 12, "updated": "2026-05-29T08:00:00Z", "content_type": "text/plain", "storage_class": "STANDARD" }
```

A clean not-found returns `{ "ok": true, "exists": false, "bucket": …, "object": … }` (exit `0`) — this is what the CLI's `put` reads to choose T1 (new) vs T2 (overwrite).

### `put` / `delete`

```json
{ "ok": true, "plugin": "gcloud-storage", "operation": "put",  "bucket": "b", "object": "a.txt", "source": "./a.txt" }
{ "ok": true, "plugin": "gcloud-storage", "operation": "delete","bucket": "b", "object": "a.txt", "deleted": true }
```

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout (incl. `stat` with `exists:false`). |
| `1` | dependency | `jq` missing, sibling `gcloud-auth/gcloud.sh` absent, or `gcloud` missing. |
| `2` | usage | Unknown operation; `stat`/`put`/`delete` without `.bucket`+`.object`; `put` without/with-missing `.local`. |
| `3` | not-authenticated | No active `gcloud` credential. |
| `6` | gcloud-error | The underlying `gcloud` command failed (a real error, distinct from `stat`'s clean not-found). |

## Configuration

```toml
# workspace.toml
[plugins.gcloud-storage]
enabled = true
```

No `[config.schema]` — object state is queried live. Only the universal `enabled` key.

## Lifecycle mapping

`workflow` kind owner is the operator via the `bwoc gcloud storage` CLI; no local state beyond `gcloud`'s cache. `init` sources the helpers + checks `jq`; `invoke` runs the verb; `teardown` is implicit.

## Idempotency

- `list` / `stat` are read-only.
- `put` is idempotent (re-uploading the same bytes converges).
- `delete` is idempotent in effect (gone→gone); a `delete` of an absent object surfaces a `gcloud` error (exit 6) rather than a false success.

## Maturity

Declared **L1** — first runnable `workflow/gcloud-storage` plugin; all four verbs functional. Bumps to **L2** once smoke tests exercise the verbs end-to-end against an authenticated `gcloud` with a test bucket + throwaway object.

> [!warning] Live-test gap. End-to-end against a real bucket is gated on an operator-provided sandbox (design note §Status). v0.1.0 is verified by `bash -n` + shellcheck, the missing-sibling/unauthenticated paths erroring cleanly, and `bwoc check` accepting the manifest.

## Neutrality

Manifest values name no LLM backend or model. `kind = "workflow"` is the framework's enum. "gcloud" / "Google Cloud Storage" appear only in `description` + this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-29_gcloud-storage-epic10-design|EPIC-10 design note]] — framing + the T3 instantiation.
- [[../gcloud-auth/SPEC|gcloud-auth SPEC]] — sibling plugin (credential state); helpers sourced here.
- [[../gcloud-compute/SPEC|gcloud-compute SPEC]] — EPIC-9 sibling; the risk matrix this slice extends to T3.
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `workflow` kind row.
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
