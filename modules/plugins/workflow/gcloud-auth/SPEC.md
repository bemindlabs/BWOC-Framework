---
title: gcloud-auth — Google Cloud Credential State
aliases:
  - gcloud-auth
tags:
  - group/framework-plugins
  - type/plugin
  - kind/workflow
  - domain/integration
  - integration/gcloud
maturity: L1
---

# gcloud-auth — Google Cloud Credential State

> [!abstract] One of two reference `workflow` plugins for the GCP foundation (`BWOC-EPIC-8`). It owns **credential state** — which credential is active (ADC vs service-account vs env), the active account email, and presence/absence. Verbs: `status` (read-only; **never prints the credential value**) and `login` (operator-driven shell-out to `gcloud auth login`). Pairs with [[../gcloud-project/SPEC|`gcloud-project`]] for project introspection; together they form the stable auth+context foundation that future GCP plugins build on. Full framing: [[../../../notes/2026-05-28_gcloud-workflow-plugin-architecture|BWOC-51 design note]].

## Why two plugins, why the `workflow` kind

`gcloud-auth` and `gcloud-project` ship as **two** plugins so future slices (`gcloud-compute`, `gcloud-storage`, …) can reuse the auth surface declared here without inheriting project verbs they don't need. The `workflow` kind (rather than a new `gcp` kind) is correct because the framework does not own the lifecycle: agents call out to the local `gcloud` CLI and read its output; nothing in BWOC owns a sync ledger or normative GCP schema. Full rationale: design note decisions 1 + 2.

## Verbs

| Operation | Direction | Auth | Side effect |
|---|---|---|---|
| `status` | read | none required | None — reads local config + file presence only; **never** prints a token or credential value. |
| `login` | write (local) | none required | Streams `gcloud auth login` to the operator's TTY. **Operator-driven only**; never auto-invoked by an agent (BWOC-54 / design note §Decision 5). |

`status` is the canonical agent-facing read. `login` is a thin pass-through to the `gcloud` CLI.

## How it runs

The `bwoc gcloud` CLI (`BWOC-52`) spawns `gcloud.sh` from this directory (mirroring how `bwoc audit` dispatches an `audit` plugin and `bwoc jira` dispatches the `jira-cloud-rest` adapter):

| Channel | What it carries |
|---|---|
| `BWOC_GCLOUD_OPERATION` (env) | `status` \| `login` — fallback for `.operation` when stdin is empty. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; resolves the service-account JSON path. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory (informational). |
| `BWOC_GCLOUD_ACCOUNT` / `BWOC_GCLOUD_PROJECT` / `BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT` (env, optional) | Operator overrides (lowest auth precedence). |
| stdin | One-line JSON request, e.g. `{"operation":"status"}` or `{"operation":"login","account":"me@example.com"}`. |

On success: exit `0`, one JSON object on stdout. On error: human diagnostic on stderr + non-zero exit (the CLI surfaces it as `plugin '<name>' exited <code>`).

## Authentication

The plugin **never reads any credential value**. It only inspects file presence and asks `gcloud` for state. Precedence (first source whose state resolves wins; reflected in `status.active_source`):

1. **Application Default Credentials (ADC)** — `~/.config/gcloud/application_default_credentials.json` (or `$CLOUDSDK_CONFIG/application_default_credentials.json` if set). The default for human developer sessions. Written by `gcloud auth application-default login`.
2. **Service-account JSON** — `${BWOC_WORKSPACE}/.bwoc/secrets/gcloud-sa.json`. Gitignored at the workspace level (added by this story), `chmod 600`, never committed. CI / headless agent setup.
3. **Environment variables** — `BWOC_GCLOUD_ACCOUNT`, `BWOC_GCLOUD_PROJECT`, optionally `BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT`. Lowest precedence — most transient.

[[auth|auth.toml]] declares the **shape** — which sources are consulted, which env vars matter, which file paths are expected — with **no values**. `bwoc check` (`BWOC-55`) will refuse to pass if any value-looking field appears.

> [!danger] **Sila — Adinnādāna.** A malformed `auth.toml` cannot leak a token because no token ever enters the plugin's address space. The plugin's only secret-adjacent operations are (a) checking that `gcloud-sa.json` exists, and (b) letting `gcloud` itself read it. The token is never echoed, logged, or placed in any JSON output. The `status` verb deliberately surfaces metadata (paths, email, source) — **never** the credential value.

## Output shapes

### `status`

```json
{
  "ok": true,
  "plugin": "gcloud-auth",
  "operation": "status",
  "gcloud_cli_present": true,
  "active_source": "adc",
  "account_email": "me@example.com",
  "has_credential": true,
  "sources": {
    "adc":             { "present": true,  "path": "/Users/me/.config/gcloud/application_default_credentials.json" },
    "service_account": { "present": false, "path": null },
    "env":             { "present": false, "vars": ["BWOC_GCLOUD_ACCOUNT","BWOC_GCLOUD_PROJECT","BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT"] }
  }
}
```

`active_source` is one of `adc | service-account | env | none`. `account_email` is `null` when no account is set. `gcloud_cli_present` is `false` when `gcloud` is not on PATH — in that mode `account_email` is also `null`, and the envelope still emits cleanly (no panic).

### `login`

`login` streams `gcloud auth login` to the operator's TTY. After `gcloud` exits successfully, the plugin emits a single telemetry line on stdout:

```json
{ "ok": true, "plugin": "gcloud-auth", "operation": "login", "account_email": "me@example.com" }
```

`login` does not auto-retry, does not capture the OAuth flow, and does not persist tokens itself — `gcloud` owns the credential store at `~/.config/gcloud/`.

## Shared helpers

`gcloud.sh` exports four helper functions for sibling `workflow/gcloud-*` plugins to source (design note §Decision 2):

| Function | Returns / Side-effect |
|---|---|
| `gcloud_assert_cli` | Returns `127` (with clear stderr) when `gcloud` is not on PATH. |
| `gcloud_active_source` | Echoes `adc \| service-account \| env \| none` (the same precedence as `status`). |
| `gcloud_account_email` | Echoes the active account email; empty when unauthenticated. **Never prints the token.** |
| `gcloud_assert_authenticated` | Returns `3` (with clear stderr) when there is no active credential. |

The dispatcher (`_gcloud_auth_main`) is `BASH_SOURCE`-guarded, so sourcing this file is a pure import — no stdin is consumed, no verb runs. [[../gcloud-project/gcloud|`gcloud-project/gcloud.sh`]] sources this file via `$BWOC_PLUGIN_DIR/../gcloud-auth/gcloud.sh` (with a script-relative fallback so the plugin remains testable outside the framework dispatcher).

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout. |
| `1` | dependency / login-blocker | `jq` missing from PATH, or `login` invoked without `gcloud` on PATH. |
| `2` | usage | Unknown / missing operation. |
| `3` | not-authenticated | A caller used `gcloud_assert_authenticated` and no active credential was present. `status` itself never returns this code. |
| `127` | helper-only | Returned by `gcloud_assert_cli` to its caller; the dispatcher converts it to exit `1`. |

Missing `gcloud` CLI fails **gracefully**: a clear stderr message and a structured `status` envelope (with `gcloud_cli_present: false`); the plugin never panics or leaves the process in a half-state.

## Configuration

```toml
# workspace.toml
[plugins.gcloud-auth]
enabled = true
```

No `[config.schema]` — credential resolution is environment-driven, not config-driven (design note §Decision 3). The only workspace-level surface is the universal `enabled` key.

## Lifecycle mapping

Per [[../../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]], the `workflow` kind's owner is the **agent** calling out (via the `bwoc gcloud` CLI). `init`/`teardown` are per-invocation around `invoke`. The plugin holds **no local state** beyond what the local `gcloud` CLI itself caches in `~/.config/gcloud/`.

| Phase | What this plugin does |
|---|---|
| `init` | Implicit per invocation; verifies `jq` on PATH (and `gcloud` for verbs that need it). |
| `invoke` | Reads the request, queries `gcloud` (for `login`) or local file state (for `status`), emits JSON. |
| `teardown` | Implicit; no state to release. |

## Idempotency

- `status` is read-only and order-stable across replays.
- `login` is operator-driven; replays converge to the same authenticated state (or no change if the operator cancels). The plugin itself is a thin pass-through.

## Maturity

Declared **L1** — first runnable `workflow/gcloud-auth` reference plugin; both verbs functional. Bumps to **L2** once `bwoc check` extension (`BWOC-55`) and smoke tests exercise it end-to-end against a real local `gcloud` install with operator credentials.

> [!warning] Live-test gap. Live end-to-end verification against a real GCP project is gated on an operator-provided sandbox SA JSON dropped at `.bwoc/secrets/gcloud-sa.json` (design note §Status). v0.1.0 is verified by: `bash -n gcloud.sh`, the missing-`gcloud` path producing a clean `gcloud_cli_present:false` envelope, the missing-`jq` path erroring cleanly, `bwoc check` accepting the manifest, and the `status` verb returning the correct envelope on a vanilla unconfigured workstation.

## Neutrality

Manifest values name no LLM backend or model. `kind = "workflow"` is the framework's own enum value. "gcloud" / "Google Cloud" appear only in `description` (where integration-target names are tolerated per [[../../../docs/en/PLUGINS.en#Neutrality constraint (HARD)|PLUGINS.en.md §Neutrality]]) and in this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../../notes/2026-05-28_gcloud-workflow-plugin-architecture|BWOC-51 design note]] — full framing for the EPIC-8 foundation (decisions 1–7).
- [[../gcloud-project/SPEC|gcloud-project SPEC]] — sibling plugin (project context); sources helpers from this plugin.
- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `workflow` kind row.
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
