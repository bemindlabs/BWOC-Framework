---
title: Scrum via Jira
aliases:
  - scrum-via-jira
tags:
  - group/framework-skills
  - type/skill
  - domain/scrum
  - domain/integration
maturity: L1
---

# Scrum via Jira

> [!abstract] The framework's first **skill-on-plugin** skill. It gives an agent six higher-level scrum operations — propose / open / close a sprint, transition a story, sync the backlog, list active sprints — each implemented as a thin call onto a `jira`-kind plugin's `bwoc jira` verbs. The skill owns *scrum semantics*; the plugin owns *the integration* (REST, auth, JQL, rate limits, the sync ledger). It depends on a `jira`-kind plugin via the [`requires_plugins`](#dependency-model--requires_plugins) contract field.

## What This Skill Does

Wraps the operator-facing `bwoc jira` CLI surface (the `jira` plugin kind, [[../../docs/en/PLUGINS.en#Plugin Kinds|PLUGINS.en.md §Plugin Kinds]]) in scrum vocabulary so an agent can run a sprint against an external issue tracker without re-deriving the integration. Six operations are exposed; each names *what scrum thing happens* and delegates *how it reaches Jira* to a `bwoc jira` verb under the hood.

- **`propose-sprint`** — gather candidate backlog stories and emit a proposed sprint composition. **Read-only.** Uses `bwoc jira query` (project-scoped JQL) under the hood; performs no external write.
- **`open-sprint`** — activate the agreed sprint: ensure each selected story is mapped, then push the sprint assignment. Uses `bwoc jira link` (map story ↔ issue) then `bwoc jira sync` (push the assignment) under the hood. **Gated write.**
- **`transition-story`** — move one story across the scrum lifecycle (`backlog → in_progress → done`). Uses `bwoc jira transition` under the hood. **Gated write.**
- **`sync-backlog`** — reconcile the scrum backlog with Jira (pull external changes, push local ones). Uses `bwoc jira sync` under the hood — `--dry-run` previews the resolution plan, the bare apply is gated. **Gated write.**
- **`close-sprint`** — finalize a sprint: transition any remaining stories and record the close. Uses `bwoc jira transition` + `bwoc jira sync` under the hood. **Gated write.**
- **`list-active-sprints`** — report sprints currently open for the configured project. **Read-only.** Uses `bwoc jira query` under the hood.

## Why It Exists

Bundling scrum operations into the `jira` plugin would force every agent that wants to run a sprint to also carry REST, auth, rate-limit, and conflict concerns — the wrong layer for an agent capability ([[../../notes/2026-05-27_jira-plugin-architecture|BWOC-40 design note]] §5). Splitting them lets *many* consumers — this skill, a future skill, or the operator's own direct `bwoc jira` use — sit on **one** plugin, and lets the plugin be installed, configured, audited, and tested independently of any agent. Same substrate, different invoker — the [[../../docs/en/PLUGINS.en#Skill vs Plugin|Skill vs Plugin]] axis applied verbatim.

## The Skill ↔ Plugin Boundary

This is the load-bearing distinction; keep it sharp.

| | `scrum-via-jira` (this skill) | a `jira`-kind plugin (e.g. `jira-cloud-rest`) |
|---|---|---|
| Layer | Agent capability | Framework integration |
| Opt-in via | `<agent>/config.manifest.json` (`skills.framework[]`) | `workspace.toml [plugins.<name>]` |
| Invoker | The agent, during its own operation | The framework runtime / `bwoc jira` CLI |
| Knows | Scrum semantics — sprints, stories, backlog, the `backlog → in_progress → done` lifecycle | REST v3, HTTP Basic auth, JQL syntax, `429`/`Retry-After`, the Issue Mapping schema, the sync ledger |
| Does **not** know | REST, auth, JQL syntax, rate limits, the ledger format | Anything about scrum |

Two rules fall out of the table and are **non-negotiable**:

1. **Dependency is one-way: skill → plugin.** The skill calls the plugin's verbs; the plugin has no knowledge of the skill. A `jira`-kind plugin is fully usable with no skill present (the operator runs `bwoc jira` directly).
2. **The skill never touches `.scrum/jira-sync.json` directly, and never holds a credential.** It reaches the sync ledger *only transitively through the plugin* — the plugin is the ledger's single writer ([[../../notes/2026-05-27_jira-plugin-architecture|BWOC-40 note]] §6), and credentials resolve inside the plugin from `BWOC_JIRA_*` env / `.bwoc/secrets.toml`, never through the skill.

## Operations Contract

Each operation composes one or more `bwoc jira` verbs. The verb's own gate is inherited — write verbs carry the plugin's operator-confirmation gate ([[../../modules/plugins/jira-cloud-rest/SPEC|jira-cloud-rest SPEC.md]] §Verbs); the skill adds no second gate and removes none.

| Operation | Scrum intent | `bwoc jira` verb(s) under the hood | Direction | Gate |
|---|---|---|---|---|
| `propose-sprint` | Draft a sprint from candidate backlog stories | `query` | read | none — reads are free |
| `open-sprint` | Activate a sprint; assign its stories | `link` → `sync` | write | operator confirmation (in the plugin) |
| `transition-story` | Advance one story's status | `transition` | write | operator confirmation (in the plugin) |
| `sync-backlog` | Reconcile backlog ↔ Jira | `sync` (`--dry-run` previews) | read/write | apply is gated (in the plugin) |
| `close-sprint` | Finalize a sprint | `transition` → `sync` | write | operator confirmation (in the plugin) |
| `list-active-sprints` | List open sprints for the project | `query` | read | none — reads are free |

The offline ledger verb `bwoc jira status` underpins read introspection across operations (e.g. resolving the current story ↔ issue mapping before a `transition`). Every operation is observed by `Kāyānupassanā` (the ledger/filesystem state the plugin reports) and `Dhammānupassanā` (which gate is in force). Failures surface the operation, the root cause, and the remedy — never just "failed."

## Dependency Model — `requires_plugins`

This skill is the framework's **first skill-on-plugin dependency**. It is expressed through a dedicated contract field, distinct from the skill-name `requires` array:

```toml
[contract]
requires         = []          # framework SKILL names (the existing field)
requires_plugins = ["jira"]    # plugin KINDS this skill needs enabled (new)
```

- **`requires_plugins` names plugin _kinds_, not plugin _names_.** `"jira"` is the kind enum value from [[../../docs/en/PLUGINS.en#Plugin Kinds|PLUGINS.en.md]] — so the skill depends on *any* enabled `jira`-kind adapter (`jira-cloud-rest` today, another tomorrow), never on a specific vendor implementation. This keeps the skill neutral and lets the adapter be swapped without touching the skill.
- **Resolved at agent spawn.** If `scrum-via-jira` is enabled on an agent but no `jira`-kind plugin is enabled in the workspace, spawn fails fast with a clear diagnostic naming the missing kind — the agent is never half-wired.
- **Caught earlier by `bwoc skill verify scrum-via-jira`**, which checks the same dependency before spawn time.

The full dependency model — why a dedicated field beats overloading `requires`, and how spawn-time resolution works — is specified in [[../../docs/en/SKILLS.en#Skill-on-plugin dependency|SKILLS.en.md §Skill-on-plugin dependency]].

## Lifecycle Mapping

Per [[../../docs/en/SKILLS.en#Lifecycle|SKILLS.en.md §Lifecycle]]:

```
init       → resolve the jira-kind plugin dependency; cache the bwoc jira dispatch handle.
             No REST calls, no credential read — Anattā.
invoke     → each operation composes one or more bwoc jira verbs; write verbs inherit
             the plugin's operator-confirmation gate. Idempotent at the operation level:
             a replayed transition the plugin already applied converges to a no-op.
teardown   → no-op. The skill holds no external state — the sync ledger is the plugin's.
```

The skill holds no global state between invocations. Replay-safe.

## Maturity

Declared **L1** — spec + scaffold; the operations contract is fixed but unverified end-to-end. Live verification against a real Jira Cloud site is gated on an operator-provided sandbox token (the same `BWOC-EPIC-6` risk the [[../../modules/plugins/jira-cloud-rest/SPEC|jira-cloud-rest plugin]] carries). Bumps to L2 once at least one agent has driven a sprint through all six operations end-to-end; to L3 once `bwoc skill verify scrum-via-jira` is wired and passes in CI.

## Neutrality

Manifest values name no LLM backend, model, or vendor CLI. `requires_plugins = ["jira"]` references the framework's own plugin-**kind** enum value, not a vendor — exactly as the [[../../modules/plugins/jira-cloud-rest/SPEC#Neutrality|jira-cloud-rest plugin]] sets `kind = "jira"`. "Jira" / "Atlassian" appear only in `description` and this SPEC's prose as the integration-target name, tolerated under the same rule that lets the plugin name its target. `bwoc jira` and `bwoc skill verify` are framework commands, not backend commands. Satisfies **Samānattatā**.

## See Also

- [[../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to; the skill-on-plugin dependency model.
- [[../../modules/plugins/jira-cloud-rest/SPEC|jira-cloud-rest SPEC.md]] — the `jira`-kind plugin this skill drives; the verb contract under the hood.
- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the `jira` kind row + Skill vs Plugin axis.
- [[../../notes/2026-05-27_jira-plugin-architecture|2026-05-27_jira-plugin-architecture.md]] — EPIC-6 framing (§5 plugin-vs-skill split, §6 single-writer ledger).
- [[../worktree-discipline/SPEC|worktree-discipline]] — the first reference framework skill; the shape this one follows.
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
