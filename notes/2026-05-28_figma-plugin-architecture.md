---
title: Figma Plugin Kind — architecture framing for EPIC-7
date: 2026-05-28
sprint: BWOC sprint-11
epic: BWOC-EPIC-7
story: BWOC-61
related_stories: [BWOC-62, BWOC-63, BWOC-64, BWOC-65]
---

# 2026-05-28 — Figma Plugin Kind (EPIC-7 framing)

This note sets the spec frame for `BWOC-EPIC-7` — the **final epic** of the original roadmap — before any code or spec lands. It answers the design questions Sprint 11 must resolve so `BWOC-62` (PLUGINS spec + Figma Asset Mapping Schema), `BWOC-63` (`bwoc figma` CLI), `BWOC-64` (the `figma-rest` reference plugin), and `BWOC-65` (the `bwoc check` extension) can be drafted without churn: whether `figma` earns its own kind or reuses `workflow`, the auth model, Figma REST rate-limit + scope bounds, the export-caching strategy, and the Figma Asset Mapping Schema shape.

The throughline: **`figma` is a read-mostly external-API integration that bridges design→dev.** It sits in the integration family alongside `workflow`/`jira`, but unlike `jira` it never writes back — it fetches frame metadata, exports images, queries component libraries, and surfaces design tokens. The one decision that shapes everything else is whether it deserves its own kind; the rest (auth, caching, rate limits) follow the patterns `jira` and `gcloud` already set.

## Decisions

### 1. `figma` is its own plugin kind (the 8th) — not a `workflow` reuse

The current PLUGINS spec enumerates seven kinds after EPIC-5: `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`, `okr`, `council`. `BWOC-62` adds `figma` as the **eighth**. This is the close call of the epic, because the last integration we added (`gcloud`, EPIC-8) *reused* `workflow` rather than minting a kind. The decision turns on the same test that split `jira` from `workflow`:

- **`gcloud` reused `workflow`** ([BWOC-51 §Decision 1](2026-05-28_gcloud-workflow-plugin-architecture.md)) because it carries **no normative schema** — it shells to the local `gcloud` CLI and surfaces whatever JSON comes back; BWOC owns no durable data shape for it.
- **`jira` earned its own kind** ([BWOC-40 §1](2026-05-27_jira-plugin-architecture.md)) because it carries a **normative Issue Mapping Schema** — a durable, cross-tool data shape BWOC defines and validates.

`figma` is like `jira` on this axis: it carries a **normative Figma Asset Mapping Schema** (Decision 5) — a durable shape that ties a Figma node to an exported artifact + design tokens, consumed by `bwoc check`, dashboards, and spec-doc token references. A design token mapped to a spec doc is a BWOC-owned relationship, not a passthrough of Figma's API. That normative schema is what earns `figma` its own kind.

> [!note]
> The rule that falls out of `gcloud` (reuse) vs `jira`/`figma` (own kind): **an integration earns its own kind when BWOC defines a normative schema over it; it reuses `workflow` when it's a passthrough to an external CLI/API with no BWOC-owned data shape.** This is now the third data point and the clearest statement of the boundary.

`figma` differs from `jira` in one key way, though: **it is read-mostly.** `jira` is the write-capable kind (bidirectional sync, operator-confirm gates, a sync ledger). `figma` only **reads** Figma's REST API + **writes locally** (exported images to `figma/exports/`). It never mutates Figma. So it carries jira's *schema* discipline but not jira's *write* machinery — no sync ledger, no operator-confirm gates on remote writes (there are none).

### 2. Auth model — operator-provided personal access token, never committed

Figma's REST API authenticates with a **personal access token** (PAT) via the `X-Figma-Token` header. Token sources, in precedence order (mirroring jira/gcloud):

1. **`BWOC_FIGMA_TOKEN`** env — the transient/CI path.
2. **`.bwoc/secrets.toml`** `[figma] token = "..."` — workspace-local, `chmod 600`, **gitignored**.

`auth.toml` in the plugin declares the **shape** — which env var, which secrets path, which token-permission scopes are required — but carries **no token value**. The plugin reads the token only to set the `X-Figma-Token` header on outbound requests; it never logs, echoes, or serializes it (Sīla — Adinnādāna), and `bwoc check` (BWOC-65) fails closed on any value-looking field in `auth.toml`, exactly as the jira (BWOC-45) and gcloud (BWOC-55) guards do.

> [!warning]
> Figma PATs carry **file-access vs team-library** scope. A token scoped to a user's own files cannot read a team library, and vice-versa. The SPEC (BWOC-64) documents which verbs need which scope; a 403 surfaces as "token lacks <scope> for <resource>", never a bare failure.

### 3. Read-mostly — no bidirectional sync (contrast `jira`)

Every `figma` verb either reads Figma (`fetch`, `tokens`, `status`) or writes **locally** (`export` drops an image under `figma/exports/`). Nothing writes back to Figma. This is the deliberate inversion of `jira`:

| | `jira` | `figma` |
|---|---|---|
| Direction | bidirectional (reads + writes the tracker) | read-mostly (reads Figma, writes local files only) |
| Ledger | `.scrum/jira-sync.json` (sync state) | none — exports are content-addressable, idempotent |
| Gates | operator-confirm on write verbs | none — no remote writes to gate |
| Schema | Issue Mapping (mutable projection) | Asset Mapping (read snapshot + local export path) |

Because there's no remote write, there's no conflict policy, no sync watermark, no confirm gate — the whole bidirectional-sync apparatus jira needed is absent. `figma` is simpler.

### 4. Rate limits + export caching

- **Rate limits.** Figma REST applies per-team rate limits and returns `429` with a `Retry-After`. `figma.sh` (BWOC-64) honors `Retry-After` on `429`, the same handling the jira plugin uses. Bulk operations (e.g. exporting many nodes) batch under the limit.
- **Export caching.** Exports are **content-addressable**: the cached filename is `SHA-256(file_key + node_id + version + format)` under `figma/exports/`. A re-export of an unchanged node is a cache hit (no API call); a changed node (new `version`) produces a new file. This keeps exports idempotent, makes `bwoc figma export` cheap to re-run, and makes the cache safe to `.gitignore` (it's reproducible). `figma/exports/` is added to the workspace gitignore template (BWOC-64).

### 5. Figma Asset Mapping Schema (for BWOC-62)

`BWOC-62` adds a normative **Figma Asset Mapping Schema** to PLUGINS (parallel to Audit Findings / Jira Issue Mapping / OKR Progress / Council Decision). Fields:

| Field | Type | Required | Notes |
|---|---|---|---|
| `file_key` | string | yes | The Figma file key (from the file URL). **The stable external key** — the asset is keyed on `file_key` + `node_id`. |
| `node_id` | string | yes | The node within the file (frame, component, etc.). Stable key component. |
| `name` | string | yes | The node's name. A mutable projection of Figma state, refreshed each `fetch`. |
| `type` | string | yes | Node type (`FRAME`, `COMPONENT`, `INSTANCE`, …). |
| `last_modified` | string (ISO 8601) | yes | Figma's last-modified timestamp for the file — the cache-invalidation signal. |
| `exported_path` | string | no | Workspace-relative path of the exported image under `figma/exports/`. Omitted until exported. |
| `image_url` | string | no | The (short-lived) Figma-hosted render URL from the export call. Omitted when not requested; not durable (expires). |
| `design_tokens` | object | no | Extracted design tokens `{ name: value }` (colors, spacing, type) tied to this node — the design→spec bridge. Omitted when none extracted. |

Optional fields are **omitted** (not `null`) when absent, per the framework convention. `file_key` + `node_id` is the stable key; the rest are mutable projections of Figma state or local export results — never key on them.

## Alternatives considered

- **Reuse `workflow` (like gcloud)** — rejected (Decision 1). figma carries a normative Asset Mapping Schema (a BWOC-owned design→dev relationship); that earns its own kind, the same way jira's Issue Mapping did. gcloud had no schema, so it reused workflow.
- **Make figma write-capable (push tokens/comments back)** — rejected for the foundation. Read-mostly is the safe, useful first slice; write-back (e.g. posting a comment, updating a variable) is a separate future epic if a concrete need appears.
- **Store exports in the repo** — rejected. Exports are content-addressable + reproducible; `figma/exports/` is gitignored. Committing binary renders would bloat the repo for no durable benefit.
- **Cache by node_id alone** — rejected (Decision 4). A node changes; caching must key on `version` (via `last_modified`) too, else a stale export is served after a design update.
- **Persist `image_url`** — kept optional + marked non-durable. Figma's render URLs expire; the durable artifact is `exported_path` (the local file), not the URL.

## Status / deferred

- Decisions 1-5 frozen for EPIC-7 unless `BWOC-62`/`BWOC-64` surface a concrete contradiction.
- **Live verification** (fetch a real file, export an image, list components) gates on an **operator-provided Figma PAT** — the same external-credential gate jira (EPIC-6) and gcloud (EPIC-8) carry. The plugin builds + unit-tests + `bwoc check` passes without it.
- **Write-back** (comments, variable updates) is explicitly **out of scope** — a future epic if needed.
- **Design-token → spec-doc linking** ships as the `design_tokens` field + `bwoc figma tokens` extraction; the deeper "tie a token to a specific spec-doc line and check drift" automation is deferred (the schema carries the tokens; the linking tooling can follow).

## Related

- Sprint 11 planning: [`.scrum/planning/sprint-11-planning.md`](../../.scrum/planning/sprint-11-planning.md) (workspace)
- EPIC-6 [jira-plugin note](2026-05-27_jira-plugin-architecture.md) — the own-kind + normative-schema precedent; the write-capable kind figma is read-mostly against.
- EPIC-8 [gcloud-workflow note](2026-05-28_gcloud-workflow-plugin-architecture.md) — the `workflow`-reuse precedent; figma diverges (it has a schema, so its own kind).
- [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds) — the enumeration `BWOC-62` extends to eight (the last of the original roadmap).
