---
title: Jira Cloud REST v3 Adapter
aliases:
  - jira-cloud-rest
tags:
  - group/framework-plugins
  - type/plugin
  - kind/jira
  - domain/integration
  - integration/jira-cloud
maturity: L1
---

# Jira Cloud REST v3 Adapter

> [!abstract] The first **write-capable** reference plugin — the `jira` kind's adapter. It reads issues via project-scoped JQL and performs **gated** status transitions against Atlassian Cloud REST v3. Dispatched by the `bwoc jira` CLI (`BWOC-42`), it owns the HTTP; the CLI owns argument parsing, the sync ledger, the auth gate, and the write-confirmation gate. Mapping entries conform to the [[../../docs/en/PLUGINS.en#Jira Issue Mapping Schema|Jira Issue Mapping Schema]] (`BWOC-41`).

## Why a `jira` kind, not a `workflow` plugin

Every kind shipped so far (`audit`, and the planned reporting kinds) **reads** the workspace and emits findings — never mutating external state. `jira` reads **and writes** an external system of record: issue transitions and (later) field/sprint updates. That single property — durable, hard-to-reverse external side-effects on `invoke` — is what earns it a distinct kind and forces the write-confirmation gate and the conflict policy. The full rationale is in [[../../notes/2026-05-27_jira-plugin-architecture|the BWOC-40 design note]] §1.

## Verbs

The `bwoc jira` CLI delegates exactly three live verbs to this adapter; the offline verbs (`status`, `link`, `unlink`) never reach it — they touch only the ledger.

| Operation | Direction | Auth | HTTP | Gate |
|---|---|---|---|---|
| `query` | read | required | `GET /rest/api/3/search` (project-scoped JQL, bounded paging) | none — reads are free |
| `transition` | **write** | required | `GET …/transitions` then `POST …/transitions` | operator confirmation (in the CLI) |
| `sync` | read/**write** | required | reads the ledger; `--dry-run` previews | apply is gated (in the CLI) |

`query` and the read half of `sync` are the read-mostly path that ships functional in v0.1.0. `transition` is a structured, idempotent write. `sync` is a structured skeleton (see [§Sync](#sync--structured-skeleton)).

## How it runs

The CLI spawns `jira.sh` from this directory (mirroring how `bwoc audit` dispatches an `audit` plugin):

| Channel | What it carries |
|---|---|
| `BWOC_JIRA_OPERATION` (env) | `query` \| `transition` \| `sync`. |
| `BWOC_WORKSPACE` (env) | Absolute workspace root; `sync` reads `.scrum/jira-sync.json` under it. |
| `BWOC_PLUGIN_DIR` (env) | Absolute path to this plugin directory. |
| `BWOC_JIRA_EMAIL` / `BWOC_JIRA_TOKEN` / `BWOC_JIRA_BASE_URL` (env) | Credentials, inherited (see [§Authentication](#authentication)). |
| `BWOC_JIRA_PROJECT` (env, optional) | Project key used to scope JQL (see [§JQL](#jql--project-scoping)). |
| stdin | One-line JSON request, e.g. `{"operation":"query","jql":"…","start_at":0,"max_results":50}`. |

On success the script exits `0` and emits **one JSON object** on stdout, which the CLI parses. On error it writes a human diagnostic to **stderr** and exits non-zero; the CLI surfaces it as `plugin '<name>' exited <code>`. The output shapes the CLI consumes:

- `query` → `{ "total": <n>, "issues": [ … ], "start_at", "max_results" }`
- `transition` → `{ "ok": true, "issue", "to_status", "transitioned": <bool> }`
- `sync` → `{ "summary": { "push", "pull", "noop", "conflict" }, "dry_run" }` — a non-zero `conflict` makes the CLI exit `3`.

## Authentication

Atlassian Cloud authenticates REST v3 with **HTTP Basic = `email:api_token`** against the site base URL. The three inputs resolve from the environment (first hit wins):

| Input | Env var | Role |
|---|---|---|
| Account email | `BWOC_JIRA_EMAIL` | Basic-auth username half |
| API token | `BWOC_JIRA_TOKEN` | Basic-auth password half — **the secret** |
| Site URL | `BWOC_JIRA_BASE_URL` | e.g. `https://<site>.atlassian.net` |

Resolution order: **environment variables** (preferred — nothing touches disk), then a gitignored, owner-only **`.bwoc/secrets.toml`** (`chmod 600`; the `[jira]` table). The token is read straight into `curl -u` — it is never echoed, never written to a file, and never placed in any JSON output (**Adinnādāna** at every boundary).

> [!danger] `auth.toml` ships the SHAPE only — never a value. [[auth|auth.toml]] declares which keys exist and which env var each binds to, with **empty** `email`/`token`/`base_url` placeholders. No credential is ever committed. Rotating a revoked token is config-only (update the env var / secrets file); the sync ledger's `last_synced` watermarks are independent of the credential, so a `401`/`403` is surfaced as "re-authenticate / rotate token", never as a sync conflict.

If any of the three is missing, the adapter fails fast with a clear `auth_missing` diagnostic naming the absent vars (the CLI also gates this before spawning the adapter — defense in depth).

## JQL — project scoping

`query` accepts JQL but the adapter bounds it (**Mattaññutā** at the API boundary):

- **Project-scoped.** When `BWOC_JIRA_PROJECT` is set and the JQL does not already constrain a project, the adapter wraps it as `project = "<P>" AND (<jql>)` — no accidental cross-project reads. In v0.1.0 this is best-effort env-driven scoping; richer config-driven scoping lands once the CLI forwards the resolved `[plugins.jira-cloud-rest].project` config.
- **Bounded result sets.** `maxResults` is clamped to ≤ 100 and `startAt` is honored — the adapter never issues an unbounded fetch. This is the first defense against rate limits.
- **Read-only by nature.** JQL is never a write path; writes go through the typed `transition` verb.

## Rate limiting & error classes

Atlassian Cloud applies cost-based rate limiting and returns `429 Too Many Requests` with a `Retry-After` header. `jira.sh` distinguishes the error classes the [[../../notes/2026-05-27_jira-plugin-architecture|BWOC-40 note]] §3 calls out:

| HTTP | Class | Adapter behavior |
|---|---|---|
| `2xx` | success | Parse + project the body. |
| `429` | retryable | Honor `Retry-After` (exponential fallback if absent); up to 4 attempts, then surface a retryable error. |
| `401` / `403` | fatal auth | "Rotate `BWOC_JIRA_TOKEN` / check `BWOC_JIRA_EMAIL`" — **never** a sync conflict. |
| `404` | mapping drift | The linked issue moved/was deleted; surfaced to the operator, never silently recreated. |
| other / transport | error | Reported with the truncated body; non-zero exit. |

Retries are safe because the writes are idempotent (see [§Idempotency](#idempotency)).

## Issue Mapping Schema usage

`query` projects each Jira issue into the normative [[../../docs/en/PLUGINS.en#Jira Issue Mapping Schema|Issue Mapping Schema]] shape — `issue_key`, `project`, `summary`, `status`, `assignee` (omitted when unassigned, never `null`). `sync` reads/writes those same entries in `.scrum/jira-sync.json`, the single sync ledger (the adapter is one of its writers, via the CLI). `issue_key` is the only durable key; the other fields are mutable projections refreshed each sync and compared field-by-field against the `last_synced` watermark for conflict detection.

## Sync — structured skeleton

`sync` in v0.1.0 establishes the **contract, auth, and read path**: it reads the ledger, counts mapped issues, and emits the final `summary.{push,pull,noop,conflict}` envelope shape with every mapped issue reported as a **no-op** — no writes performed. The per-field last-writer-wins resolution engine with operator-confirmed true-conflict handling (BWOC-40 note §4) is **deferred to the EPIC-6 sync engine**; the envelope shape here is final so the CLI contract is stable now.

## Configuration

```toml
# workspace.toml
[plugins.jira-cloud-rest]
enabled = true
project = "BWOC"   # the Jira project key reads are scoped to
```

Credentials are **not** config — they resolve from `BWOC_JIRA_*` env / `.bwoc/secrets.toml`. The only declared `[config.schema]` key is `project`.

## Lifecycle mapping

Per [[../../docs/en/PLUGINS.en#Lifecycle|PLUGINS.en.md §Lifecycle]], the `jira` kind's owner is the `bwoc jira` CLI; `init`/`teardown` are per-invocation around `invoke`. The adapter holds **no local state** beyond the shared ledger.

| Phase | What this adapter does |
|---|---|
| `init` | Implicit per invocation; verify auth presence. |
| `invoke` | Read the request, call REST v3, emit JSON. |
| `teardown` | Implicit; only the temp header/body files are removed. |

## Idempotency

- `query` is read-only and order-stable.
- `transition` checks the issue's current status first: if it already equals the target, it is a no-op success. Replaying a transition after a `429` backoff therefore converges to the same state.
- `sync` writes nothing in v0.1.0 (skeleton), so it is trivially idempotent.

## Maturity

Declared **L1** — first runnable `jira` adapter; read path functional, write path structured. Bumps to L2 once exercised end-to-end against a real Jira Cloud instance with an operator-provided token.

> [!warning] Live-test gap. End-to-end verification against a real Jira Cloud site is gated on an operator-provided sandbox token (an `BWOC-EPIC-6` risk). v0.1.0 is verified by: `bash -n jira.sh`, the auth-missing path erroring cleanly, and `bwoc check` accepting the `jira` manifest. The live REST round-trip is unverified until a token is available.

## Neutrality

Manifest values name no LLM backend or model. `kind = "jira"` is the framework's own enum value (`BWOC-41`). "Jira" / "Atlassian" appear only in `description` (where integration-target names are tolerated per [[../../docs/en/PLUGINS.en#Neutrality constraint (HARD)|PLUGINS.en.md §Neutrality]]) and in this SPEC's prose — never in `kind`, `entry`, or config keys. Satisfies **Samānattatā**.

## See Also

- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the plugin spec; `jira` kind row + Jira Issue Mapping Schema (BWOC-41).
- [[../../notes/2026-05-27_jira-plugin-architecture|2026-05-27_jira-plugin-architecture.md]] — EPIC-6 framing note (auth, JQL, rate-limit, conflict policy).
- [[../../crates/bwoc-cli/src/jira|crates/bwoc-cli/src/jira.rs]] — the `bwoc jira` CLI that dispatches this adapter.
- [[auth|auth.toml]] — the auth contract (shape only; no values).
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
