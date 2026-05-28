---
title: gcloud-compute — Google Cloud Compute Instance Lifecycle
aliases:
  - gcloud-compute
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-compute — Google Cloud Compute Instance Lifecycle

> [!abstract] The first **write-capable** GCP slice (`BWOC-EPIC-9`), built on the EPIC-8 foundation. It owns **instance lifecycle** — `list` / `describe` (read) and `start` / `stop` (gated writes). Writes are confirmation-gated in the `bwoc gcloud compute` CLI, never in the plugin. Sources credential helpers from the sibling [[../gcloud-auth/SPEC|`gcloud-auth`]] plugin. Full framing + the reusable write-verb risk matrix: [[../../../notes/2026-05-28_gcloud-compute-epic9-design|EPIC-9 design note]].

## Why build on the foundation

`gcloud-compute` reuses the auth surface `gcloud-auth` established — credential resolution is defined once and sourced here at startup, exactly as `gcloud-project` does (EPIC-8 design note §Decision 2). It stays the `workflow` kind (not a new `gcp` kind): the framework does not own the lifecycle, it calls `gcloud` out and surfaces the result. EPIC-9 is deliberately the **reversible** lifecycle slice only — `start`/`stop` — because a misfired `stop` is recoverable with `start`. `instances.{delete,reset,create}` are out of scope (irreversible / higher blast radius — their own future slices).

## Verbs

| Operation | Direction | Auth | HTTP / side effect | Risk tier | Gate |
|---|---|---|---|---|---|
| `list` | read | required | `gcloud compute instances list` (optionally `--zones`) | T0 | none |
| `describe` | read | required | `gcloud compute instances describe <i> --zone <z>` | T0 | none |
| `start` | **write (remote)** | required | `gcloud compute instances start <i> --zone <z>` — resumes a stopped instance | T1 | **confirm** (CLI; `--json` ⇒ `--yes`) |
| `stop` | **write (remote)** | required | `gcloud compute instances stop <i> --zone <z>` — stops a running instance | T2 | **confirm + echo resolved `project/zone/instance`** (CLI) |

Confirmation tiers are the reusable scale defined in the [[../../../notes/2026-05-28_gcloud-compute-epic9-design#3. Write-verb risk matrix (the reusable template — NEW)|design note §3]]: T0 read · T1 reversible/cost · T2 reversible/availability (echo target) · T3 irreversible (typed-name) · T4 security (refuse + opt-in). EPIC-9 uses T0/T1/T2; `start`=T1, `stop`=T2.

## How it runs

The `bwoc gcloud compute` CLI spawns `gcloud.sh` from this directory:

| Channel | What it carries |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `list` \| `describe` \| `start` \| `stop` — fallback for `.operation`. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; the sibling SA JSON path resolves under it. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory; finds the sibling helpers at `../gcloud-auth/gcloud.sh`. |
| stdin | One-line JSON, e.g. `{"operation":"describe","instance":"web-1","zone":"us-central1-a"}`. |

On success: exit `0`, one JSON object on stdout. On error: human diagnostic on stderr + non-zero exit.

> [!warning] Option-injection guard (#92). Every operator-supplied value reaches `gcloud` as a `--flag=value` (bound, never re-parsed) or as a positional **after a `--` end-of-options separator**, so a `-`-leading instance id can never be read as a gcloud flag. The CLI also validates instance/zone charset before dispatch.

## Authentication

This plugin **never reads any credential value**. It sources the sibling [[../gcloud-auth/SPEC#Authentication|`gcloud-auth`]] helpers (`gcloud_assert_cli`, `gcloud_assert_authenticated`) at startup and asks `gcloud` for compute state. [[auth|auth.toml]] declares the **same** auth contract (shape only) as the siblings so `bwoc check` can validate it per-plugin and an operator sees the full model here.

Credential precedence is identical to [[../gcloud-auth/SPEC#Authentication|`gcloud-auth §Authentication`]]: ADC → `.bwoc/secrets/gcloud-sa.json` → `BWOC_GCLOUD_*` env. The plugin fails fast with a clear diagnostic if there is no active credential.

> [!danger] **Sila — Adinnādāna.** No token ever enters this plugin's address space. We only call `gcloud` and surface its output. Lifecycle writes transmit nothing but the instance/zone/project to the local `gcloud` CLI.

## Output shapes

### `list`

```json
{
  "ok": true,
  "plugin": "gcloud-compute",
  "operation": "list",
  "total": 2,
  "instances": [
    { "name": "web-1", "zone": "us-central1-a", "status": "RUNNING",    "machine_type": "e2-medium", "internal_ip": "10.0.0.2" },
    { "name": "batch", "zone": "us-central1-b", "status": "TERMINATED", "machine_type": "e2-small",  "internal_ip": "10.0.0.3" }
  ]
}
```

### `describe`

```json
{
  "ok": true,
  "plugin": "gcloud-compute",
  "operation": "describe",
  "name": "web-1",
  "zone": "us-central1-a",
  "status": "RUNNING",
  "machine_type": "e2-medium",
  "internal_ip": "10.0.0.2",
  "external_ip": "34.x.x.x",
  "create_time": "2026-05-01T08:00:00Z"
}
```

### `start` / `stop`

```json
{
  "ok": true,
  "plugin": "gcloud-compute",
  "operation": "stop",
  "instance": "web-1",
  "zone": "us-central1-a",
  "status": "TERMINATED"
}
```

`gcloud` waits for the lifecycle operation; the plugin then re-reads the instance and reports the **actual** resulting `status` (Sacca — report what is true, not the intended state). `status` is `null` if the post-op read fails.

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout. |
| `1` | dependency | `jq` missing, sibling `gcloud-auth/gcloud.sh` absent, or `gcloud` missing. |
| `2` | usage | Unknown operation, or `describe`/`start`/`stop` invoked without `.instance` + `.zone`. |
| `3` | not-authenticated | No active `gcloud` credential. |
| `6` | gcloud-error | The underlying `gcloud` command failed; truncated diagnostic on stderr. |

Missing `gcloud` CLI fails **gracefully** — clear stderr + non-zero exit, never a panic.

## Configuration

```toml
# workspace.toml
[plugins.gcloud-compute]
enabled = true
```

No `[config.schema]` — compute state is queried live. The only workspace-level surface is the universal `enabled` key.

## Lifecycle mapping

Per [[../../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]], the `workflow` kind's owner is the **agent/operator** calling out via the `bwoc gcloud compute` CLI. The plugin holds **no local state** beyond what `gcloud` caches.

| Phase | What this plugin does |
|---|---|
| `init` | Implicit; source the sibling helpers; verify `jq` on PATH. |
| `invoke` | Read the request, call `gcloud compute instances ...`, emit JSON. |
| `teardown` | Implicit; no state to release. |

## Idempotency

- `list` and `describe` are read-only.
- `start` / `stop` are idempotent: starting a running instance (or stopping a stopped one) converges to the same state, and the re-read `status` reflects the terminal state. Replays after a transient `gcloud` error converge.

## Maturity

Declared **L1** — first runnable `workflow/gcloud-compute` reference plugin; all four verbs functional. Bumps to **L2** once smoke tests exercise the verbs end-to-end against an authenticated local `gcloud` with a stoppable test instance.

> [!warning] Live-test gap. Live end-to-end against a real GCP instance is gated on an operator-provided sandbox (SA JSON at `.bwoc/secrets/gcloud-sa.json` + a stoppable test instance — design note §Status). v0.1.0 is verified by: `bash -n gcloud.sh`, the missing-sibling path erroring cleanly, the unauthenticated path returning the `not-authenticated` diagnostic, the read verbs against a live `gcloud`, and `bwoc check` accepting the manifest.

## Neutrality

Manifest values name no LLM backend or model. `kind = "workflow"` is the framework's own enum value. "gcloud" / "Google Cloud" appear only in `description` (where integration-target names are tolerated per [[../../../docs/en/PLUGINS.en#Neutrality constraint (HARD)|PLUGINS.en.md §Neutrality]]) and in this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-28_gcloud-compute-epic9-design|EPIC-9 design note]] — framing + the reusable write-verb risk matrix.
- [[../gcloud-auth/SPEC|gcloud-auth SPEC]] — sibling plugin (credential state); helpers sourced here.
- [[../gcloud-project/SPEC|gcloud-project SPEC]] — the other foundation plugin.
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `workflow` kind row.
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
