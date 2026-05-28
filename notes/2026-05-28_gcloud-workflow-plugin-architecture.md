---
title: gcloud Workflow Plugin — architecture framing for EPIC-8
date: 2026-05-28
sprint: BWOC sprint-8
epic: BWOC-EPIC-8
story: BWOC-51
related_stories: [BWOC-52, BWOC-53, BWOC-54, BWOC-55]
---

# 2026-05-28 — gcloud Workflow Plugin (EPIC-8 framing)

This note sets the spec frame for `BWOC-EPIC-8` before any code or spec lands. It answers the design questions Sprint 8 must resolve so `BWOC-52` (`bwoc gcloud` CLI), `BWOC-53` (the two reference plugins), `BWOC-54` (the `gcloud-ops` skill), and `BWOC-55` (the `bwoc check` extension) can be drafted without churn: why `gcloud` reuses the existing `workflow` kind rather than minting a seventh, why the foundation splits into **two** plugins (`gcloud-auth` and `gcloud-project`) instead of one, what the auth model is and how credentials stay out of git, what verb shapes each plugin carries, why the `gcloud-ops` skill is bounded to **read-mostly** operations in this slice, and what the future-slice roadmap looks like.

The throughline: **gcloud is the framework's second workflow-kind integration, but the first one that ships read-mostly first by design.** EPIC-6's `jira` was a write-capable kind from day one — its whole motivation was bidirectional scrum↔Jira sync. EPIC-8 deliberately inverts that: the foundation slice writes nothing meaningful (one operator-confirmed `set-default` aside), and every real write surface (compute lifecycle, storage object operations, Cloud Run / Cloud Build deploys) is deferred to **separate later epics** that build on this foundation. The reason is blast radius: a misfiring `gcloud compute instances delete` is unrecoverable; a misfiring `gcloud auth status` is not. Earn the trust before extending the reach.

## Decisions

### 1. `gcloud` reuses the existing `workflow` plugin kind — no new kind

The current PLUGINS spec enumerates five kinds in [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds): `memory-backend`, `llm-backend`, `workflow`, `audit`, `jira`. The `workflow` row already covers **"external system integrations (issue trackers, code review, CI)"** — gcloud (Google Cloud Platform CLI wrapping IAM, projects, compute, storage, run, build, …) lands squarely under that umbrella.

Why not mint a `gcp` or `cloud` kind? Two reasons:

1. **The kind boundary is the lifecycle hook, not the vendor.** Per the precedent established in the [ISO-compliance note](2026-05-26_iso-compliance-plugins.md#1-audit-becomes-the-4th-plugin-kind-not-compliance-or-policy), a new kind is justified only when the framework calls a **different lifecycle hook** or hands the call site to a **different owner**. `gcloud` doesn't: it's invoked by the agent (via the `bwoc gcloud` CLI or the `gcloud-ops` skill), it returns shell-style output that the agent interprets, and it never enters a framework-managed lifecycle like `bwoc audit run` does for the `audit` kind. That's the `workflow` shape exactly.

2. **`jira` earned its own kind for a different reason that does NOT apply here.** EPIC-6's [jira-plugin-architecture note](2026-05-27_jira-plugin-architecture.md#1-jira-is-a-distinct-plugin-kind--an-integration-adapter-not-a-reporting-kind) extracted `jira` from `workflow` because it carried a **normative Jira Issue Mapping schema** (durable cross-tool data shape) and a **bidirectional sync ledger** (`.scrum/jira-sync.json`). gcloud foundation has neither. The state lives in Google's APIs (read-only from our side in foundation) and in the local `gcloud` CLI's own config (`~/.config/gcloud/`). We never persist a sync ledger; we never reshape Google's data into a BWOC-normative schema; we only call `gcloud` and surface its output.

> [!note]
> If a future GCP slice (e.g., a compute-resource inventory that BWOC normalizes into a portable JSON schema) ever needs a normative schema like jira's Issue Mapping, **that slice** can earn its own kind at that time. The foundation does not.

PLUGINS spec touch in `BWOC-52` is therefore **light**: a one-line annotation under the `workflow` row in the Kinds table noting "gcloud (GCP, see workflow/gcloud-* plugins)" plus a short paragraph referencing this note. No new normative section. No schema additions. EN + TH parity in the same commit (cf. the [Bilingual Parity HARD rule](../CLAUDE.md#bilingual-parity-hard-rule)) — but the TH delta is small (~5 lines), not a full SPEC translation.

### 2. Two plugins — `gcloud-auth` + `gcloud-project` — not one mega-plugin

The foundation ships **two** workflow plugins, not one. The split is by concern:

| Plugin | Owns | Verbs (foundation) |
|---|---|---|
| `gcloud-auth` | Credential state — which credential is active (ADC vs service-account), the account email, presence/absence | `status`, `login` |
| `gcloud-project` | Project context — list, describe, default-project | `list`, `show`, `set-default` |

Why split:

1. **Separation of concerns at the manifest layer.** Each plugin's `manifest.toml` declares its own `entry` script, its own `auth.toml` shape (project plugin requires auth-resolved-state; auth plugin owns the resolution), and its own SPEC. Future GCP slices — `gcloud-compute`, `gcloud-storage`, `gcloud-run` — each **reuse** the auth state surface that `gcloud-auth` defines, without each one re-implementing credential resolution. The two plugins together act as a **stable foundation layer** for the rest of the GCP integration.

2. **Test isolation.** `bwoc check` extensions and unit tests can target each plugin independently. A regression in project-list handling cannot mask a regression in credential precedence.

3. **Optional install.** A team that only needs project introspection (read-only auditing, "what projects do we have access to") can install `gcloud-project` without `gcloud-auth` if they already manage ADC via their environment. The reverse — `gcloud-auth` without `gcloud-project` — is a valid CI-only setup. One mega-plugin makes both halves mandatory.

The alternative — **one** `workflow/gcloud` plugin with all verbs under it — was rejected primarily on (1): the future-slice cost. Without the split, every later GCP plugin either duplicates auth resolution or grows the mega-plugin. Neither is acceptable under [Mattaññutā / Right amount](../CLAUDE.md#philosophy-grounding).

The two plugins **share shell helpers**. `gcloud-auth/gcloud.sh` exports credential-resolution functions (`gcloud_active_credential`, `gcloud_account_email`, `gcloud_assert_authenticated`); `gcloud-project/gcloud.sh` sources them rather than re-implementing. The source path is hardcoded relative to the workspace plugin tree — keeping the dependency explicit, no PATH games.

### 3. Auth model — operator-provided credentials, never committed

Three credential sources, in **precedence order** (first one that resolves wins):

1. **Application Default Credentials (ADC)** — `gcloud auth application-default login` writes `~/.config/gcloud/application_default_credentials.json`. This is the default for local developer workstations and the recommended approach for human-driven sessions. `gcloud-auth` simply asks the local `gcloud` CLI for the active credential — no BWOC-side handling of the underlying token.
2. **Service-account JSON** at `.bwoc/secrets/gcloud-sa.json` (workspace-local, **gitignored**). This path is the BWOC convention; the file is `chmod 600`, never committed, never logged. `gcloud-auth` sets `GOOGLE_APPLICATION_CREDENTIALS` to this path when calling `gcloud` if the file exists. Use case: CI runs, headless agents, scenarios where ADC isn't appropriate.
3. **Environment variables** — `BWOC_GCLOUD_ACCOUNT`, `BWOC_GCLOUD_PROJECT`, optionally `BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT`. These let the operator override resolution without touching files. Lowest precedence because they're the most transient.

`auth.toml` in each plugin declares the **shape** of credential resolution — which sources are consulted, which environment variables matter, which file paths are expected — but carries **no secret values**. The shape is:

```toml
# auth.toml — declares the auth contract; carries NO secret values.
[sources]
adc = { path = "~/.config/gcloud/application_default_credentials.json", priority = 1 }
service_account = { path = ".bwoc/secrets/gcloud-sa.json", priority = 2 }
env = { vars = ["BWOC_GCLOUD_ACCOUNT", "BWOC_GCLOUD_PROJECT", "BWOC_GCLOUD_IMPERSONATE_SERVICE_ACCOUNT"], priority = 3 }
```

`.bwoc/secrets/gcloud-sa.json` is added to `.gitignore` in `BWOC-53`. `bwoc check` (extended in `BWOC-55`) verifies the `auth.toml` shape — keys present, no values — and refuses to pass if any value-looking field appears. This is the same secret-leak guard introduced for `jira` (`BWOC-45`); the rule is now reusable across `workflow` integrations and worth a paragraph in PLUGINS spec under a "Workflow plugins — secret handling" subsection (added in `BWOC-52`'s spec slice).

> [!warning]
> The plugin **never reads the credential value itself**. It only reads paths/env-var-names from `auth.toml`, hands them to the `gcloud` CLI, and reads `gcloud`'s output. This means a malformed `auth.toml` cannot leak a token — there's no token in the BWOC process address space to leak.

### 4. Verb shapes — what each plugin invokes

`bwoc plugin invoke <plugin> <verb> [args]` is the framework's contract. The two plugins ship the following verbs in the foundation:

**`gcloud-auth`**

| Verb | Inputs | Output | Side effect |
|---|---|---|---|
| `status` | — | JSON: `{ active_source: adc\|service-account\|env\|none, account_email, has_credential: bool }` | None — read-only. Never prints the credential value or token. |
| `login` | `--account <email>` (optional) | Streams `gcloud auth login` output to the operator | **Operator-driven only** — opens a browser; never auto-invoked by an agent. |

`status` is the canonical agent-facing verb. `login` is essentially a thin pass-through to `gcloud auth login` and is **not** part of the `gcloud-ops` skill (see decision 5).

**`gcloud-project`**

| Verb | Inputs | Output | Side effect |
|---|---|---|---|
| `list` | — | JSON: `[{ project_id, project_number, name, lifecycle_state }]` (paginated under the hood, single response surfaced) | None — read-only. |
| `show` | `--project <id>` (optional, default = `gcloud config get project`) | JSON: full project descriptor + IAM bindings count + enabled APIs (subset) | None — read-only. |
| `set-default` | `--project <id>` | JSON: `{ previous, current }` | **Mutates** `~/.config/gcloud/config` (the local `gcloud` config DB). Operator-confirm gated in the CLI (`BWOC-52`). |

`set-default` is the only write verb in the foundation. It does not touch Google's APIs — it only changes which project the local `gcloud` CLI defaults to. Risk is operator-local; reversibility is trivial (`gcloud config set project <previous>`). The operator confirmation is still required: a wrong default can route subsequent agent verbs at the wrong project, which is the kind of silent footgun that this slice is explicitly trying to avoid.

### 5. `gcloud-ops` skill — read-mostly, no `login`, no `set-default`

`modules/skills/gcloud-ops/` is the agent-facing wrapper. It exposes three operations to the agent's prompt-side surface:

| Skill op | Wraps | Why exposed to agents |
|---|---|---|
| `whoami` | `gcloud-auth status` + `gcloud-project show` (on default) | "Who am I authenticated as, and which project am I about to act on?" — the standard self-check before any GCP read. |
| `current-project` | `gcloud-project show` (default) | Faster path than `whoami` when only the project matters. |
| `switch-project` | `gcloud-project set-default --project <id>` | The **only** write operation in the skill — and it **relays the operator-confirmation prompt** rather than auto-approving. |

What's deliberately **not** in the skill in foundation:

- `login` — opens a browser, can't be agent-driven safely.
- Anything outside the two foundation plugins — no compute, no storage, no IAM mutation. Those are separate future skills/epics.
- `gcloud-auth login` even via the operator-confirmation pattern — kept explicitly out of the agent surface. An agent that needs to log in surfaces "please authenticate via `gcloud auth login`" to the operator, period.

The skill declares dependency on **both** plugins. The SKILLS spec already documents the skill-on-plugin pattern (added by EPIC-6's `BWOC-44`); EPIC-8 extends it lightly to cover the skill-on-multiple-plugins case (`gcloud-ops` is the first such example). The doc touch is one paragraph in SKILLS.en/SKILLS.th in `BWOC-54`.

### 6. Relation to the `jira` write-adapter pattern

EPIC-6 [established the write-capable workflow pattern](2026-05-27_jira-plugin-architecture.md#1-jira-is-a-distinct-plugin-kind--an-integration-adapter-not-a-reporting-kind):

1. Read-mostly verbs invoke freely (no operator gate).
2. Write verbs gate behind operator confirmation in the CLI.
3. Authentication shape (not value) declared in `auth.toml`.
4. `bwoc check` extension validates the auth shape and refuses on value-leaks.
5. The skill exposes a curated agent-safe subset, not the full plugin surface.

EPIC-8 **reuses all five** patterns. The only divergence is **scope**: foundation has effectively no remote-API writes (only the local `gcloud` config change). That's intentional — the patterns are validated against a lower-blast-radius slice before being applied to the higher-blast-radius future slices.

> [!tip]
> When the first GCP write-capable slice lands (compute lifecycle, most likely), this note's decision 4 expands with a write-verb risk matrix — the same matrix the jira note's decision 4 carries for `transition` and `sync`. The matrix template is reusable across `workflow` plugins.

### 7. Future-slice roadmap — what is NOT in EPIC-8

The decisions above intentionally bound the foundation. Concrete deferrals:

| Future slice | Plugin(s) | First write verb | Tracking |
|---|---|---|---|
| **Compute lifecycle** | `workflow/gcloud-compute` | `instances.{start,stop}` | EPIC-9 — [#96](https://github.com/bemindlabs/BWOC-Framework/issues/96) |
| **Storage** | `workflow/gcloud-storage` | `objects.{put,delete}` | EPIC-10 — [#97](https://github.com/bemindlabs/BWOC-Framework/issues/97) |
| **Serverless deploy** | `workflow/gcloud-run` (+ `gcloud-build` if separable) | `services.deploy`, `builds.submit` | EPIC-11 — [#98](https://github.com/bemindlabs/BWOC-Framework/issues/98) |
| **IAM mutations** | `workflow/gcloud-iam` | `bindings.{add,remove}` — **highest blast radius, latest** | EPIC-12 — [#99](https://github.com/bemindlabs/BWOC-Framework/issues/99) |

Each of these is its own future epic (EPIC-9 onward, tracked in [#96](https://github.com/bemindlabs/BWOC-Framework/issues/96)–[#99](https://github.com/bemindlabs/BWOC-Framework/issues/99)). They build on the auth surface this foundation establishes. **None** are pulled into Sprint 8.

The deferral order is roughly **lowest blast radius first** when those epics are eventually planned. Compute and storage have user-visible costs; IAM has security-visible consequences. The most dangerous slice — IAM bindings — must be last, after the patterns are well-exercised on the safer slices.

## Alternatives considered

- **Single mega-plugin `workflow/gcloud`** — rejected (decision 2). Forces future slices to either duplicate auth or expand the mega-plugin; both bad under Mattaññutā.
- **New `gcp` plugin kind** — rejected (decision 1). The kind boundary is the lifecycle hook, not the vendor; gcloud is `workflow` shape exactly.
- **Use the Google Cloud SDK gRPC libraries directly from a Rust plugin** — rejected for foundation. Shelling out to the local `gcloud` CLI is the lower-risk path: gcloud already handles auth refresh, ADC, impersonation, and quota-project resolution. A Rust-native path can be revisited if a future slice (probably compute-watch-style streaming) genuinely needs it. Foundation does not.
- **Ship `gcloud-iam` early because IAM read is "free"** — rejected. Even read-only IAM tooling normalizes a mental model that puts IAM ops in the agent's hands. Better to keep IAM out of the foundation entirely and revisit when there's a concrete operator need with a specific scope.
- **Auto-detect `gcloud` CLI on PATH and refuse to install plugin if absent** — partially adopted. The plugin manifests declare `requires = ["gcloud"]` in metadata; `bwoc check` (BWOC-55) surfaces a clear error if `gcloud` is not on PATH, but does not block install — a workspace might install plugins before the operator installs `gcloud` itself.

## Status / deferred

- Decisions 1–7 frozen for EPIC-8 unless `BWOC-52`/`BWOC-53` surface a concrete contradiction during impl.
- Live end-to-end verification against a real GCP project is **gated on an operator-provided sandbox** (a GCP project + a service-account JSON dropped at `.bwoc/secrets/gcloud-sa.json`). Surfacing point: when `BWOC-53` reaches the smoke-test gate.
- The write-verb risk matrix (per the jira precedent) is **deferred to the first write-capable GCP slice** (likely EPIC-9 compute), not authored here.
- An eventual `gcloud` Rust-native bindings exploration is deferred — revisit when a future slice actually requires it.

## Related

- Sprint 8 planning: [`.scrum/planning/sprint-8-planning.md`](../../.scrum/planning/sprint-8-planning.md) (workspace, not framework)
- EPIC-6 [jira-plugin-architecture note](2026-05-27_jira-plugin-architecture.md) — the write-capable workflow pattern this foundation explicitly does not exercise yet.
- EPIC-2 [ISO-compliance plugins note](2026-05-26_iso-compliance-plugins.md) — the precedent for kind-boundary reasoning.
- [PLUGINS.en.md §Plugin Kinds](../docs/en/PLUGINS.en.md#plugin-kinds) — the kind enumeration this slice does **not** extend.
