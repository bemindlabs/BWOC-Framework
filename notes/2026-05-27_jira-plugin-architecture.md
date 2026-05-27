---
title: Jira plugin kind + scrum-via-jira skill — architecture framing for EPIC-6
date: 2026-05-27
sprint: BWOC sprint-6
epic: BWOC-EPIC-6
story: BWOC-40
related_stories: [BWOC-41, BWOC-42, BWOC-43, BWOC-44, BWOC-45]
---

# 2026-05-27 — Jira plugin kind + scrum-via-jira skill (EPIC-6 framing)

This note sets the spec frame for `BWOC-EPIC-6` before any code or spec lands. It answers the design questions Sprint 6 must resolve so `BWOC-41` (PLUGINS spec + Issue Mapping schema), `BWOC-42` (`bwoc jira` CLI), `BWOC-43` (reference plugin + auth), and `BWOC-44` (the `scrum-via-jira` skill) can be drafted without churn: why `jira` is a distinct plugin kind rather than a `workflow` plugin, what the auth model is and how tokens stay out of git, how JQL and Atlassian rate limiting bound the read path, what conflict-resolution policy governs the bidirectional scrum↔Jira sync, why the skill is split from the plugin, and how the existing `.scrum/jira-sync.json` is reused rather than reinvented.

The throughline: **`jira` is the framework's first write-capable plugin kind.** Every kind shipped so far (`audit`, plus the planned `okr` / `council`) is a *reporting* kind — it reads the workspace and emits findings, never mutating external state. `jira` reads **and writes** an external system of record. That single property — durable, hard-to-reverse external side-effects on `invoke` — is what forces every decision below.

## Decisions

### 1. `jira` is a distinct plugin kind — an integration adapter, not a reporting kind

The current PLUGINS spec enumerates four kinds in [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds): `memory-backend`, `llm-backend`, `workflow`, `audit`. Per the precedent set in the [ISO-compliance note](2026-05-26_iso-compliance-plugins.md#1-audit-becomes-the-4th-plugin-kind-not-compliance-or-policy), a kind is defined by **the lifecycle hook the framework calls and who owns the call site**. `jira` earns its own kind on both axes. `BWOC-41` adds it as the seventh kind (after `okr` and `council` if those land first per EPIC-4 and the council work; directly after `audit` otherwise).

**How it differs from `audit` / `okr` (the reporting family):**

| | `audit` / `okr` / `council` (reporting) | `jira` (integration adapter) |
|---|---|---|
| Data direction | Read workspace → emit report | Read **and write** an external system |
| `invoke` side-effects | None outside stdout (the findings/report payload) | Durable mutations on a third-party system of record (issue transitions, field updates, sprint assignment) |
| Reversibility | A wrong report is recomputed for free | A wrong write corrupts an external tracker — hard to reverse |
| Sync state | Stateless; each run is a fresh projection | **Stateful** — persists a sync ledger (`.scrum/jira-sync.json`) to detect drift between runs |
| Lifecycle owner | `bwoc audit` CLI (operator-invoked, never implicit) | `bwoc jira` CLI (operator-invoked, never implicit) |
| Confirmation gate | None — reads are free | **Write verbs gated** behind operator confirmation (see §4) |

Reporting kinds answer "what is true about this workspace?" `jira` answers "make these two systems agree" — and *agreeing* means writing. That is a categorically larger blast radius (**Surameraya** — no heedlessness; and the workspace "executing actions with care" rule), and it is why the conflict model (§4) and the write-confirmation gate are first-class concerns no reporting kind has ever needed.

**Why not reuse the existing `workflow` kind?** `workflow` already names "issue trackers, code review, CI" in its description, so this is the obvious objection. It is rejected for three reasons:

1. **Different call-site owner.** `workflow`'s lifecycle owner is "the agent calling out" — ad-hoc, per-agent, fire-and-forget outbound calls (PLUGINS.en.md §Lifecycle owner per kind). `jira`'s owner is the `bwoc jira` CLI — operator-driven, workspace-scoped, exactly mirroring how `audit` is owned by `bwoc audit`. By the kind-definition rule, a different call-site owner is a different kind.
2. **`workflow` carries no contract.** It is a generic escape hatch with no normative schema, no sync state, and no conflict semantics. `jira` requires a **normative Issue Mapping schema** (the 9 fields in `BWOC-41`: `issue_key`, `project`, `summary`, `status`, `assignee`, `story_points`, `parent_epic`, `sprint`, `last_synced`), a persistent ledger, and a conflict policy. Hanging all of that off `workflow` would either bloat the generic kind or smuggle a contract into a kind that promises none.
3. **Discoverability + targeted verification.** `bwoc plugin list --kind jira` and the `bwoc check` extension that audits `jira/*` manifests (`BWOC-43` AC) need a first-class kind to key on. Folding into `workflow` would make `bwoc jira` dispatch ambiguous (which `workflow` plugin?) and would prevent kind-specific manifest checks.

`workflow` remains the right home for one-shot, stateless outbound calls (post a comment, open a PR, trigger CI). `jira` is the home for a **stateful bidirectional sync adapter**. They are not the same shape.

### 2. Auth model — operator-provided credentials, never committed

Atlassian Cloud authenticates REST v3 with **HTTP Basic = `email:api_token`** against a site base URL. The three inputs map cleanly to env vars:

| Input | Env var | Role |
|---|---|---|
| Account email | `BWOC_JIRA_EMAIL` | Basic-auth username half |
| API token | `BWOC_JIRA_TOKEN` | Basic-auth password half — **the secret** |
| Site URL | `BWOC_JIRA_BASE_URL` | e.g. `https://<site>.atlassian.net` |

**Resolution order (first hit wins):**

1. **Environment variables** — `BWOC_JIRA_TOKEN` / `BWOC_JIRA_EMAIL` / `BWOC_JIRA_BASE_URL`. Preferred for CI and ephemeral sessions; nothing touches disk.
2. **`.bwoc/secrets.toml`** — file-permission gated, gitignored. The plugin **refuses to read it** unless it is owner-only (`chmod 600`; reject group/world-readable). This is the **Adinnādāna** guard at the file boundary — a secrets file the rest of the machine can read is treated as compromised, not convenient.

```toml
# .bwoc/secrets.toml  — NEVER committed; chmod 600; gitignored
[jira]
email    = "operator@example.com"
token    = "<api-token>"
base_url = "https://example.atlassian.net"
```

**`auth.toml` ships the SHAPE only.** The reference plugin's `modules/plugins/jira/jira-cloud-rest/auth.toml` declares *which keys exist, their types, and which env var each resolves from* — it is a declarative contract, the auth analogue of how the `audit` kind ships a declarative `criteria.toml`. It contains **no values, ever**:

```toml
# auth.toml — declares the auth contract; carries NO secret values.
[jira.auth]
email    = { env = "BWOC_JIRA_EMAIL",    required = true }
token    = { env = "BWOC_JIRA_TOKEN",    required = true, secret = true }
base_url = { env = "BWOC_JIRA_BASE_URL", required = true }
```

**Non-negotiables (Sila 5 — Adinnādāna):**

- No token value is ever written to a tracked file. `bwoc check` extends its secret-scan to refuse any committed string matching a token shape under `modules/plugins/jira/`.
- `.scrum/jira-sync.json` (§6) is **state, not secrets** — it MUST NOT contain the token. Credentials and sync-state are separate files with separate lifecycles.
- **Token rotation is config-only.** Rotating a revoked/expired token means updating the env var or `.bwoc/secrets.toml` — no code change, and the sync ledger stays valid because `last_synced` watermarks are independent of the credential. A `401`/`403` from the API is surfaced as "re-authenticate / rotate token", never as a sync conflict.

### 3. JQL constraints + Atlassian REST v3 rate limiting

**JQL (the read path).** `bwoc jira query` accepts JQL but the plugin bounds it:

- **Project-scoped.** The plugin validates/augments every query so it is constrained to the configured `project` key(s). A query that escapes the configured project is rejected — no accidental cross-project reads (least surprise + least leakage).
- **Bounded result sets.** All reads paginate (`startAt` / `maxResults`); the plugin never issues an unbounded fetch. This is **Mattaññutā** (right amount) at the API boundary and also the first defense against rate limits.
- **No injection.** Operator/skill-supplied filter values are passed as discrete JQL clauses the plugin composes, not string-concatenated into a query template.
- JQL is read-only by nature; writes go through the typed transition/update verbs, never through JQL.

**Rate limiting (HTTP 429).** Atlassian Cloud applies cost-based rate limiting and returns `429 Too Many Requests` with a `Retry-After` header. The plugin's `jira.sh` MUST:

- **Honor `Retry-After`.** Back off for the stated interval; if absent, exponential backoff with jitter, capped retry count, then surface a retryable error.
- **Distinguish error classes** so the hook contract (PLUGINS.en.md §Hook contract) stays meaningful: `429` → retryable (back off); `401`/`403` → fatal auth error (rotate token, §2); `404` → mapping drift (the linked issue was deleted/moved — surface to operator, do not silently recreate).
- **Stay idempotent across retries.** A transition or field update replayed after a `429` backoff must converge to the same state — this is the PLUGINS-spec idempotency requirement applied to a flaky network, and it is what makes automatic retry safe.

### 4. Bidirectional sync conflict resolution — field-level LWW with operator-confirmation on true conflict

**Recommended policy: per-field last-writer-wins, keyed on `last_synced` watermarks, escalating to operator confirmation only on a genuine concurrent edit.** This is a deliberate hybrid, not either pure extreme.

For each mapped field (`status`, `summary`, `assignee`, `story_points`, `sprint`, `parent_epic`), at sync time the plugin compares each side's change against the `last_synced` watermark recorded in `.scrum/jira-sync.json`:

| Scrum side | Jira side | Resolution |
|---|---|---|
| Changed since `last_synced` | Unchanged | Push scrum → Jira |
| Unchanged | Changed since `last_synced` | Pull Jira → scrum |
| Unchanged | Unchanged | No-op (idempotent) |
| **Changed** | **Changed** | **True conflict → do NOT auto-resolve.** Surface both values + timestamps; operator picks. |

Resolution is **per field, not per issue**: a `status` change on Jira and a `story_points` change in scrum are *not* a conflict — they touch different fields, so both apply in one sync. Only the same field mutated on both sides since the last sync is a true conflict.

**Why this hybrid, and not the two pure options:**

- **Pure field-level LWW (fully automatic).** Simplest, zero prompts — but on a genuine concurrent edit it *silently discards* one side's work. The scrum backlog and Jira are both systems of record; silently losing an operator's edit is the data-integrity equivalent of **Musāvāda** (a claim with no honest referent — the ledger says "synced" while work was dropped). Unacceptable as the default.
- **Pure operator-confirmation on any divergence.** Safest, never loses data — but *every* routine one-sided change counts as "divergence" and prompts. Sync stops being automation and becomes a rubber-stamp queue; operators learn to confirm blindly, which is worse than LWW. Violates **Mattaññutā** (over-prompting is its own excess).
- **The hybrid** is automatic on the ~99% non-conflicting path (one side changed, or neither did) and pulls a human in *only* for the rare genuine conflict. This matches the workspace's "executing actions with care" rule precisely: confirm the hard-to-reverse external write **when, and only when, it is genuinely ambiguous**.

**Mechanics + tie-breakers:**

- `last_synced` is a **per-issue** watermark in v1 (the floor). Per-field source timestamps/hashes are the precise form and are recommended where the data is available (Jira exposes field-level update times unevenly; scrum side we control).
- **Tie-breaker when timestamps are equal or unavailable: do NOT write.** Default to the safe side and escalate to the operator. The system never silently overwrites on ambiguity.
- `bwoc jira sync --dry-run` prints the full resolution plan (per issue, per field: push / pull / no-op / conflict) before any write. The non-dry-run apply is itself a **write verb gated behind operator confirmation** (`BWOC-43` AC) — `--dry-run` is the read-only preview, the bare `sync` is the gated apply.

### 5. Why `scrum-via-jira` is a SKILL, separate from the `jira` PLUGIN

The split follows the existing [PLUGINS.en.md §Skill vs Plugin](../docs/en/PLUGINS.en.md#skill-vs-plugin) axis verbatim — **who turns it on / who invokes it**:

- **The `jira` plugin = the framework integration surface.** Workspace-loaded via `workspace.toml [plugins.jira-cloud-rest]`, operator-facing. It owns the REST v3 adapter (`jira.sh`), auth (§2), JQL + rate-limit handling (§3), the Issue Mapping schema, the sync ledger, and the `bwoc jira {sync,query,transition,link,unlink,status}` CLI dispatch. Lifecycle `init → configure → invoke → teardown`, owned by `bwoc jira`. It knows nothing about scrum.
- **The `scrum-via-jira` skill = the agent-facing capability.** Agent-loaded via `config.manifest.json`, invoked by an agent during its own operation. It exposes higher-level scrum operations — `propose-sprint`, `open-sprint`, `transition-story`, `sync-backlog`, `close-sprint`, `list-active-sprints` — each of which **calls the plugin's `invoke` verbs** under the hood. It knows scrum semantics; it does not know REST, auth, or rate limits.

**Dependency direction: skill → plugin (one-way).** The skill depends on the plugin; the plugin has no knowledge of the skill. This is the framework's **first skill-on-plugin dependency**, which is why `BWOC-44` must add the model to SKILLS.en/th — `BWOC-40` only motivates it. Today `[contract].requires` holds *skill* names (SKILLS.en.md §contract). Two ways to express a plugin dependency:

- Overload `requires` with a namespace: `requires = ["plugin:jira"]`, or
- **Add a dedicated `requires_plugins = ["jira"]` field (recommended).** Explicit, keeps the skill-name namespace clean, and reads unambiguously at `bwoc skill verify` / agent-spawn time.

Resolution is at agent spawn: if `scrum-via-jira` is enabled but no `jira`-kind plugin is enabled in the workspace, spawn fails fast with a clear diagnostic; `bwoc skill verify scrum-via-jira` catches the same gap earlier (`BWOC-44` AC).

**Why split at all.** Bundling would force every agent that wants scrum operations to also carry REST/auth/rate-limit/conflict concerns — the wrong layer for an agent capability. Splitting lets *multiple* consumers (the skill, a future skill, or direct `bwoc jira` CLI use by the operator) sit on **one** plugin, and lets the plugin be installed, configured, audited, and tested independently of any agent. Same substrate, different invoker — textbook application of the existing table.

### 6. Reuse `.scrum/jira-sync.json` — one sync ledger, owned by the plugin

`.scrum/jira-sync.json` already exists in the workspace's contract: it is gitignored (`.gitignore` line 28 — "churns every sync; may carry credentials/tokens") and the scrum skill already references it via its `init jira` flow (EPIC-6 notes). EPIC-6 reuses it rather than inventing a parallel state file.

- **It is the sync ledger.** Per-issue mapping (`scrum story id ↔ Jira issue_key`) plus the `last_synced` watermarks and last-seen field values/hashes that §4's conflict detection reads. Sketch (the normative 9-field Issue Mapping schema is `BWOC-41`'s deliverable; this is only the *state* shape):

```json
{
  "version": 1,
  "site": "https://example.atlassian.net",
  "project": "BWOC",
  "issues": {
    "BWOC-40": {
      "issue_key": "BWOC-123",
      "last_synced": "2026-05-27T10:00:00Z",
      "fields": { "status": { "hash": "…", "source_mtime": "…" } }
    }
  }
}
```

- **Single writer.** The `jira` plugin (and the `bwoc jira` CLI that dispatches it) is the **sole** reader/writer of this file (`BWOC-42` AC: "reads/writes `.scrum/jira-sync.json`"). The `scrum-via-jira` skill touches it **transitively through the plugin**, never directly — one owner avoids two-writer races on the ledger.
- **State, not secrets.** Despite the gitignore comment's "may carry credentials", the EPIC-6 contract is explicit: the token lives in env / `.bwoc/secrets.toml` (§2) and **must not** be written here. The gitignore stays (the file still churns every sync and carries issue metadata), but the credential separation is a hard rule.
- **Why reuse, not reinvent.** The scrum skill already established the file and its location; a second state file would split the source of truth for "what is synced." One ledger, one owner. **Anattā** — no duplicate state to drift.

## Alternatives considered

- **Make `jira` a `workflow` plugin.** Rejected — different call-site owner (`bwoc jira` CLI vs. ad-hoc agent calls), and `workflow` carries no normative schema, sync state, or conflict contract. See §1.
- **Pure field-level LWW with no operator gate.** Rejected — silently drops one side's work on a true concurrent edit; unacceptable for two systems of record. See §4.
- **Operator confirmation on every divergence.** Rejected — every routine one-sided change would prompt, turning sync into a rubber-stamp queue and training operators to confirm blindly. See §4.
- **Bundle the scrum operations into the plugin.** Rejected — forces REST/auth/rate-limit concerns onto every agent that wants scrum ops, and prevents multiple consumers from sharing one integration. See §5.
- **Invent a new `.scrum/jira-state.json` separate from `jira-sync.json`.** Rejected — splits the source of truth; the scrum skill already owns `jira-sync.json`. See §6.
- **Store the token in `.scrum/jira-sync.json` (it's already gitignored).** Rejected — conflates credentials with churning state; rotation and blast-radius reasoning both demand separate files. Token lives in env / `.bwoc/secrets.toml` only. See §2.
- **Overload `[contract].requires` for the plugin dependency.** Tentatively rejected in favor of a dedicated `requires_plugins` field — keeps the skill-name namespace clean. Final call lands in `BWOC-44`. See §5.

## Status / deferred

- This note: complete. No code or spec edits land in `BWOC-40` itself — it is design framing only.
- `BWOC-41` (PLUGINS spec: declare `jira` kind + 9-field Issue Mapping schema, EN+TH): next in queue; unblocked by this note.
- `BWOC-42` (`bwoc jira` CLI surface, agent-jennie) and `BWOC-43` (reference plugin + auth, agent-lisa): blocked on `BWOC-41`.
- `BWOC-44` (`scrum-via-jira` skill + SKILLS skill-on-plugin dependency model, agent-jisoo): blocked on `BWOC-43`; this note recommends the `requires_plugins` field for it to ratify.
- Live end-to-end verification against a real Jira Cloud instance is gated on an operator-provided sandbox token (a sprint risk flagged in the epic) — not blocking the spec work.

## Related

- Epic: `BWOC-EPIC-6` (Jira Plugin Kind + scrum-via-jira Skill) — fills the `integrations.issue_tracker` gap currently `null` in `.scrum/config.json`.
- Stories framed by this note: `BWOC-41`, `BWOC-42`, `BWOC-43`, `BWOC-44`, `BWOC-45`.
- Spec docs touched downstream: [`PLUGINS.en.md`](../docs/en/PLUGINS.en.md) / [`PLUGINS.th.md`](../docs/th/PLUGINS.th.md), [`SKILLS.en.md`](../docs/en/SKILLS.en.md) / [`SKILLS.th.md`](../docs/th/SKILLS.th.md).
- Kind-definition precedent: [`2026-05-26_iso-compliance-plugins.md`](2026-05-26_iso-compliance-plugins.md) — "a kind is defined by the lifecycle hook the framework calls and who owns the call site."
- State file: `.scrum/jira-sync.json` (gitignored); config gap: `.scrum/config.json` `integrations.issue_tracker`.
- Reference designs to research downstream: Atlassian REST API v3 (auth, JQL, 429/`Retry-After`), Jira Cloud vs. Data Center differences.
