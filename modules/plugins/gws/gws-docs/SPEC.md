---
title: gws-docs — Google Docs (Read + In-Place Write)
aliases:
  - gws-docs
tags:
  - group/framework-plugins
  - type/plugin
  - kind/gws
  - domain/integration
  - integration/google-workspace
maturity: L1
---

# gws-docs — Google Docs (Read + In-Place Write)

> [!abstract] A per-service plugin of the `gws` kind (`BWOC-354`) — the **first write-capable `gws` service**. It reads a **Google Doc** (`get`, Docs `documents.get`) and edits it in place via `batch-update` (`documents.batchUpdate`, the general write verb) plus the convenience `replace-all-text`. Reads project into the normative [[../../../docs/en/PLUGINS.en#Workspace Resource Schema|Google Doc shape]]. Its write verbs carry the [[../../../docs/en/PLUGINS.en#Write verbs — the operator-confirm gate (normative)|operator-confirm gate]] at the `bwoc gws docs` CLI boundary. Sources the OAuth credential helpers from the [[../gws-auth/SPEC|`gws-auth`]] foundation, so it carries no auth code of its own. Requires the `documents` scope. Full framing: [[../../../notes/2026-05-28_google-workspace-plugin-architecture|BWOC-72 design note]].

## Verbs

| Operation | Direction | Docs endpoint | Side effect |
|---|---|---|---|
| `get` | read | `GET /v1/documents/{documentId}` (`documents.get`) | None — metadata + bounded plain-text body extraction. |
| `batch-update` | **write** | `POST /v1/documents/{documentId}:batchUpdate` | **Durable** — applies the caller's `requests[]` (the general Docs write path). Gated. |
| `replace-all-text` | **write** | `POST …:batchUpdate` (one `replaceAllText`) | **Durable** — convenience over a single `replaceAllText` request. Gated. |

> [!warning] The write verbs (`batch-update`, `replace-all-text`) mutate a live document irreversibly. They carry the operator-confirm gate at the `bwoc gws docs …` command: an interactive operator answers `y/N` (default **No**); a headless agent must pass `--yes`, and only when the operator authorized that specific edit. `--json` requires `--yes`. The plugin itself executes when invoked — the gate lives one level up, at the CLI.

## How it runs

The framework CLI (`bwoc gws docs …`) discovers this enabled plugin, applies the confirm gate for write verbs, then invokes `gws.sh` with a one-line JSON request on stdin:

| Channel | What it carries |
|---|---|
| `BWOC_GWS_OPERATION` (env) | `get` \| `batch-update` \| `replace-all-text` — fallback for `.operation` when stdin is empty. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root (token file resolution, via the sibling). |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin's directory — used to find `../gws-auth/gws.sh`. |
| `BWOC_GWS_TOKEN` (env) | The OAuth2 access token — **secret**, consumed by the sibling helpers. |
| stdin | One-line JSON request — see the contract examples below. |

```jsonc
{"operation":"get","document_id":"1AbC_dEf"}
{"operation":"batch-update","document_id":"1AbC_dEf","requests":[{"insertText":{"location":{"index":1},"text":"Hello"}}]}
{"operation":"replace-all-text","document_id":"1AbC_dEf","find":"March 31","replace":"In stock","match_case":false}
```

## Authentication & scope

Credentials resolve through the sibling `gws-auth` foundation (`BWOC_GWS_TOKEN` env / `<workspace>/.bwoc/secrets/gws-token.json`), never from workspace config. Requires the `https://www.googleapis.com/auth/documents` scope — unlike the read-mostly services, this is the **read+write** Docs scope. A token consented to `documents.readonly` can `get` but not write; a 403 on a write names the scope gap.

## Output shapes

### `get`

```json
{ "ok": true, "plugin": "gws-docs", "operation": "get",
  "document": { "document_id": "1AbC_dEf", "title": "Q3 Plan",
                "revision_id": "ALm37…", "web_view_link": "https://docs.google.com/document/d/1AbC_dEf/edit" },
  "text": "Q3 Plan\n…", "text_truncated": false }
```

### `batch-update` / `replace-all-text` (write receipt)

```json
{ "ok": true, "plugin": "gws-docs", "operation": "replace-all-text",
  "document_id": "1AbC_dEf", "revision_id": "ALm38…",
  "requests_applied": 1, "occurrences_changed": 3, "replies": [ … ] }
```

The write receipt never echoes the document body — it reports what changed, not the new content.

## Error classes

| Exit | Class | Meaning |
|---|---|---|
| `0` | success | One JSON object on stdout. |
| `1` | dependency | `jq` or `curl` missing from PATH. |
| `2` | usage / no-token | Unknown / missing operation, missing `.document_id`, an invalid id, a missing/empty/non-array `.requests`, a missing `.find`, or no resolvable token. |
| `3` | auth / scope | HTTP 401 (token invalid) or 403 (lacks the `documents` scope; a read-only token cannot write). |
| `4` | rate-limited | HTTP 429 after the backoff budget. |
| `5` | not-found | HTTP 404 (no such document). |
| `6` | transport / unexpected | Network failure or an unmapped HTTP status. |

## Configuration

```toml
# workspace.toml
[plugins.gws-docs]
enabled = true
```

No plugin-local config — the only surface is `enabled`. Credentials come from `gws-auth`.

## Lifecycle mapping

| Phase | What this plugin does |
|---|---|
| `init` | Implicit per invocation; verifies `jq` + `curl` on PATH and the sibling helpers are present. |
| `invoke` | Reads the request; for writes the CLI has already confirmed. Calls Docs via the sibling `gws_curl`; projects the read into a Doc entry or the write into a receipt. |
| `teardown` | Implicit; no state to release. |

## Idempotency

`get` is idempotent. `batch-update` and `replace-all-text` are **not** inherently idempotent — re-running applies the requests again (e.g. an `insertText` inserts twice). The operator-confirm gate exists precisely because these are durable, non-idempotent writes.

## Maturity

L1 — first slice: `get` + `batch-update` + `replace-all-text`. Higher-level convenience verbs (structured table edits, named-range updates) are deliberately deferred; the general `batch-update` already exposes the full Docs write surface.

## Neutrality

Backend-neutral: no LLM, no model, no vendor beyond Google Docs itself. The plugin is a thin, auditable REST adapter.
