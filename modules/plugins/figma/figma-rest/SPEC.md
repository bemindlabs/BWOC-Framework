---
title: Figma REST Adapter
aliases:
  - figma-rest
tags:
  - group/framework-plugins
  - type/plugin
  - kind/figma
  - domain/integration
  - integration/figma
maturity: L1
---

# Figma REST Adapter

> [!abstract] The reference plugin for the `figma` kind — the eighth kind, and the last of the original roadmap. It is **read-mostly**: it reads Figma node metadata, exports node images into a content-addressable local cache, and extracts design tokens from node styles. It **never writes back to Figma**. Dispatched by the `bwoc figma` CLI (`BWOC-63`), it owns the HTTP; the CLI owns argument parsing, the auth gate, and config resolution. Mapping entries conform to the [[../../../docs/en/PLUGINS.en#Figma Asset Mapping Schema|Figma Asset Mapping Schema]] (`BWOC-62`).

## Why a `figma` kind, not a `workflow` plugin

The `gcloud` integration (`EPIC-8`) *reused* `workflow` because it carries no normative schema — it shells to the local `gcloud` CLI and surfaces whatever JSON comes back. `figma` is different: it carries a normative **Figma Asset Mapping Schema** — a durable, BWOC-owned design→dev relationship that ties a Figma node to an exported artifact + design tokens, consumed by `bwoc check` (`BWOC-65`), dashboards, and spec-doc token references. That normative schema earns `figma` its own kind, the same way the Issue Mapping Schema earned `jira` its own kind. The full rationale is in [[../../../notes/2026-05-28_figma-plugin-architecture|the BWOC-61 design note]] §1.

`figma` differs from `jira` in one key way: it is **read-mostly**. `jira` reads and writes an external system of record (gated transitions, a sync ledger). `figma` only reads Figma and writes **locally** (exported images). It carries jira's *schema* discipline but none of its *write* machinery — no sync ledger, no operator-confirm gates, no conflict policy. There are no remote writes to gate.

## Verbs

| Operation | Direction | Auth | HTTP | Local write |
|---|---|---|---|---|
| `fetch` | read | required | `GET /v1/files/<key>/nodes?ids=<csv>` | none |
| `export` | read + **local write** | required | `GET /v1/files/<key>/nodes` (version) then `GET /v1/images/<key>?ids=<node>` | the rendered image, into the cache |
| `tokens` | read | required | `GET /v1/files/<key>/nodes?ids=<csv>` | none |

All three reads are bounded by the requested node-id set — the adapter never issues an unbounded file walk. `export` is the only verb that writes, and it writes only to the workspace-local cache (never to Figma).

## How it runs

The CLI spawns `figma.sh` from this directory (mirroring how `bwoc audit`/`bwoc jira` dispatch their plugins):

| Channel | What it carries |
|---|---|
| `BWOC_FIGMA_OPERATION` (env) | `fetch` \| `export` \| `tokens` — fallback for stdin `.operation`. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; the export cache lives under it. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin directory (informational). |
| `BWOC_FIGMA_TOKEN` (env) | The personal access token — **the secret** (see [§Authentication](#authentication)). |
| stdin | One-line JSON request, e.g. `{"operation":"fetch","file_key":"AbC123","node_ids":["1:2"]}`. |

On success the script exits `0` and emits **one JSON object** on stdout, which the CLI parses. On error it writes a human diagnostic to **stderr** and exits non-zero; the CLI surfaces it as `plugin '<name>' exited <code>`. The output shapes:

- `fetch` → `{ "ok": true, "file_key", "assets": [ <Asset Mapping entry>, … ] }`
- `tokens` → same as `fetch`, with each entry carrying a `design_tokens` object (omitted when none extracted)
- `export` → `{ "ok": true, "cached": <bool>, "asset": <Asset Mapping entry with exported_path> }`

## Authentication

Figma authenticates REST calls with a **personal access token (PAT)** via the `X-Figma-Token: <PAT>` header. The token resolves from (first hit wins):

| Source | Where | Role |
|---|---|---|
| `BWOC_FIGMA_TOKEN` | environment (preferred — nothing touches disk) | the PAT — **the secret** |
| `.bwoc/secrets.toml` `[figma] token` | gitignored, owner-only (`chmod 600`) | the PAT — on-disk fallback for hand-invocation |

`figma.sh` reads the token straight into curl's `X-Figma-Token` header — it is never echoed, never written to a file, and never placed in any JSON output or Asset Mapping entry (**Adinnādāna** at every boundary). The on-disk fallback is **refused** if the secrets file is group/world-readable. If the token is absent, the adapter fails fast with a clear `auth_missing` diagnostic.

> [!danger] `auth.toml` ships the SHAPE only — never a value. [[auth|auth.toml]] declares the env var, the secrets path, and the required scopes, with an **empty** `token` placeholder. No credential is ever committed. Rotating a revoked token is config-only (update the env var / secrets file). `bwoc check` (`BWOC-65`) fails closed on any value-looking field in `auth.toml`, exactly as the jira (`BWOC-45`) and gcloud (`BWOC-55`) guards do.

### Scopes

Figma PATs carry **file-access vs team-library** scope. A token scoped to a user's own files cannot read a team library, and vice-versa. A `403` is surfaced as "token lacks the required scope for `<resource>`" — naming the gap, never a bare failure. The read verbs need the `file_content` scope; team-library reads (a future verb) need `library_content`. See [[auth|auth.toml]] `[figma.auth.scopes]`.

## Rate limiting & error classes

Figma REST applies per-team rate limits and returns `429 Too Many Requests` with a `Retry-After` header. `figma.sh` distinguishes the error classes the [[../../../notes/2026-05-28_figma-plugin-architecture|BWOC-61 note]] §4 calls out:

| HTTP | Class | Adapter behavior |
|---|---|---|
| `2xx` | success | Parse + project the body. |
| `429` | retryable | Honor `Retry-After` (squared fallback if absent); up to 4 attempts, then surface a retryable error. |
| `401` | fatal auth | Token missing/expired/revoked — "rotate `BWOC_FIGMA_TOKEN`". |
| `403` | scope gap | "token lacks the required scope for `<resource>`" — file-vs-team-library mismatch. |
| `404` | not found | Bad `file_key` / `node_id`; surfaced to the operator. |
| other / transport | error | Reported with the truncated body; non-zero exit. |

Because the only writes are local + content-addressable (see [§Export caching](#export-caching)), retries are idempotent.

## Asset Mapping Schema usage

`fetch` projects each node into the normative [[../../../docs/en/PLUGINS.en#Figma Asset Mapping Schema|Asset Mapping Schema]] shape — `file_key`, `node_id`, `name`, `type`, `last_modified`. `tokens` adds the `design_tokens` object. `export` adds `exported_path` (and, on a fresh render, the non-durable `image_url`). `file_key` + `node_id` is the only stable key; every other field is a mutable projection of Figma state or a local export result, refreshed each call. Optional fields are **omitted** when absent, never serialized as `null`.

### Token extraction

`tokens` walks each node's style properties into a `{ name: value }` object — solid `fills` → `color/fill/<n>` hex, `cornerRadius` → `radius/corner`, text `style` → `type/font-*` and `type/line-height`, `itemSpacing` → `spacing/item`, stroke → `border/width`. This is the design→spec bridge; the deeper "tie a token to a spec-doc line and check drift" automation is deferred (the schema carries the tokens; the linking tooling can follow).

## Export caching

Exports are **content-addressable**. The cache filename is `SHA-256(file_key + node_id + version + format)` under the configured `export_dir` (default `figma/exports/`). The `version` is the file's current version (the cache-invalidation signal):

- A re-export of an **unchanged** node (same version) is a **cache hit** — the rendered file already exists, so the heavy, rate-limited image-render + download are skipped. Resolving the current version is a single cheap metadata read; when the caller already holds the version + node metadata (from a prior `fetch`), it passes them in and the hit is a **zero-API** operation.
- A **changed** node (new version) hashes to a new filename, so a stale render is never served after a design update.

This keeps `bwoc figma export` cheap to re-run and makes the cache safe to delete (it is reproducible) — `figma/exports/` is added to the workspace gitignore template (`BWOC-64`). Committing binary renders would bloat the repo for no durable benefit.

## Configuration

```toml
# workspace.toml
[plugins.figma-rest]
enabled    = true
export_dir = "figma/exports"   # optional — where the content-addressable cache lives
```

Credentials are **not** config — the token resolves from `BWOC_FIGMA_TOKEN` env / `.bwoc/secrets.toml`. The only declared `[config.schema]` key is `export_dir`.

## Lifecycle mapping

Per [[../../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]], the `figma` kind's owner is the `bwoc figma` CLI; `init`/`teardown` are per-invocation around `invoke`. The adapter holds **no local state** beyond the content-addressable cache (reproducible, gitignored).

| Phase | What this adapter does |
|---|---|
| `init` | Implicit per invocation; verify token presence. |
| `invoke` | Read the request, call Figma REST, emit JSON (and, for `export`, write the cached image). |
| `teardown` | Implicit; only the temp header/body files are removed. |

## Idempotency

- `fetch` and `tokens` are read-only and order-stable.
- `export` is content-addressable: the same `(file_key, node_id, version, format)` always maps to the same cached file, so a replay after a `429` backoff converges to the same artifact — no duplicate downloads.

## Maturity

Declared **L1** — first runnable `figma` adapter; read + local-export paths functional. Bumps to L2 once exercised end-to-end against a real Figma file with an operator-provided PAT.

> [!warning] Live-test gap. End-to-end verification against a real Figma file (fetch a node, export an image, extract tokens) is gated on an **operator-provided PAT** — the same external-credential gate jira (`EPIC-6`) and gcloud (`EPIC-8`) carry. v0.1.0 is verified by: `bash -n figma.sh`, the auth-missing / unknown-operation paths erroring cleanly, and `bwoc check` accepting the `figma` manifest. The live REST round-trip is unverified until a token is available.

## Neutrality

Manifest values name no LLM backend or model. `kind = "figma"` is the framework's own enum value (`BWOC-62`). "Figma" appears only in `description` (where integration-target names are tolerated per [[../../../docs/en/PLUGINS.en#Neutrality constraint (HARD)|PLUGINS.en.md §Neutrality]]) and in this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `figma` kind + Figma Asset Mapping Schema (BWOC-62).
- [[../../../notes/2026-05-28_figma-plugin-architecture|2026-05-28_figma-plugin-architecture.md]] — EPIC-7 framing note (own-kind, auth, rate-limit, export caching, schema).
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
