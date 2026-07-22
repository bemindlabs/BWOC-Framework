---
title: gws-slides — Google Slides (Read + In-Place Write)
aliases:
  - gws-slides
tags:
  - group/framework-plugins
  - type/plugin
  - kind/gws
  - domain/integration
  - integration/google-workspace
maturity: L1
---

# gws-slides — Google Slides (Read + In-Place Write)

> [!abstract] A per-service plugin of the `gws` kind — a write-capable Google Slides adapter. Reads a presentation (`get`, `presentations.get`) and edits it via `batch-update` (`presentations.batchUpdate`, the general write verb) plus the convenience `replace-all-text`. Reads project into the normative [[../../../docs/en/PLUGINS.en#Workspace Resource Schema|Google Presentation shape]]. Its write verbs carry the [[../../../docs/en/PLUGINS.en#Write verbs — the operator-confirm gate (normative)|operator-confirm gate]] at the `bwoc gws slides` CLI boundary. Sources the [[../gws-auth/SPEC|`gws-auth`]] foundation. Requires the `presentations` scope.

## Verbs

| Operation | Direction | Slides endpoint | Side effect |
|---|---|---|---|
| `get` | read | `GET /v1/presentations/{id}` | None — title + slide count/ids. |
| `batch-update` | **write** | `POST /v1/presentations/{id}:batchUpdate` | **Durable** — applies the caller's `requests[]` (gated). |
| `replace-all-text` | **write** | `POST …:batchUpdate` (one `replaceAllText`) | **Durable** — convenience (gated). |

> [!warning] The write verbs mutate a live presentation. They carry the operator-confirm gate at `bwoc gws slides …`: interactive `y/N` (default **No**); headless agents pass `--yes`; `--json` requires `--yes`. The plugin executes when invoked — the gate lives at the CLI.

## How it runs

The CLI invokes `gws.sh` with a one-line JSON request on stdin (`BWOC_GWS_OPERATION` / `BWOC_WORKSPACE` / `BWOC_PLUGIN_DIR` / `BWOC_GWS_TOKEN` in env), same channel contract as the sibling `gws-*` plugins.

```jsonc
{"operation":"get","presentation_id":"1AbC"}
{"operation":"batch-update","presentation_id":"1AbC","requests":[{"createSlide":{}}]}
{"operation":"replace-all-text","presentation_id":"1AbC","find":"{{title}}","replace":"Q3 Review","match_case":false}
```

## Authentication & scope

Credentials resolve through `gws-auth`. Requires `https://www.googleapis.com/auth/presentations` (read+write); a `presentations.readonly` token can `get` but not write (a write 403s naming the scope gap).

## Output shapes

`get` → `{ presentation: { presentation_id, title, slide_count, web_view_link }, slide_ids: [ … ] }`.
`batch-update` / `replace-all-text` (write receipt) → `{ presentation_id, requests_applied, occurrences_changed, replies: [ … ] }`. The receipt reports what changed, never the new slide content.

## Error classes

Same exit taxonomy as the sibling gws plugins: `0` success · `1` missing `jq`/`curl` · `2` usage / no-token (unknown op, missing/invalid `presentation_id`, missing/empty/non-array `requests`, missing `find`) · `3` auth/scope (401/403; a read-only token cannot write) · `4` 429 · `5` 404 · `6` transport/unexpected.

## Configuration

```toml
[plugins.gws-slides]
enabled = true
```

No plugin-local config — the only surface is `enabled`. Credentials come from `gws-auth`.

## Idempotency

`get` is idempotent. `batch-update` / `replace-all-text` are **not** inherently idempotent — re-running applies the requests again. The operator-confirm gate exists because these are durable writes.

## Maturity

L1 — `get` + `batch-update` + `replace-all-text`. Higher-level convenience verbs (per-shape edits, layout templating) are deferred; the general `batch-update` already exposes the full Slides write surface.

## Neutrality

Backend-neutral: no LLM, no model, no vendor beyond Google Slides. A thin, auditable REST adapter.
