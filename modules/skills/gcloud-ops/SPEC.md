---
title: gcloud Ops
aliases:
  - gcloud-ops
tags:
  - group/framework-skills
  - type/skill
  - domain/gcp
  - domain/integration
maturity: L1
---

# gcloud Ops

> [!abstract] The framework's first **skill-on-multiple-plugins** skill. It gives an agent three read-mostly GCP self-orientation operations — `whoami`, `current-project`, `switch-project` — each implemented as a thin call onto the `bwoc gcloud` verbs that dispatch the [[../../modules/plugins/workflow/gcloud-auth/SPEC|`gcloud-auth`]] and [[../../modules/plugins/workflow/gcloud-project/SPEC|`gcloud-project`]] `workflow`-kind plugins. The skill owns *agent self-orientation semantics*; the plugins own *the integration* (credential resolution, `gcloud` shell-out, project introspection). It depends on the `workflow` plugin kind via the [`requires_plugins`](#dependency-model--skill-on-multiple-plugins) contract field.

## What This Skill Does

Wraps the operator-facing `bwoc gcloud` CLI surface (the two `workflow`-kind gcloud plugins, [[../../docs/en/PLUGINS.en#Plugin Kinds|PLUGINS.en.md §Plugin Kinds]]) in agent self-orientation vocabulary, so an agent can answer "who am I, where am I, and switch where I'm pointed" without re-deriving credential resolution or project introspection. Three operations are exposed; each names *what the agent needs to know or change* and delegates *how it reaches GCP* to a `bwoc gcloud` verb under the hood.

- **`whoami`** — report the active credential (source + account email) and the current default project in one view. **Read-only.** Composes `bwoc gcloud auth` (→ `gcloud-auth status`) + `bwoc gcloud project show` (→ `gcloud-project show` on the default). Never surfaces a credential value.
- **`current-project`** — report just the current default project descriptor. **Read-only.** The faster path than `whoami` when only the project matters. Uses `bwoc gcloud project show` under the hood.
- **`switch-project`** — change the local default project. Composes `bwoc gcloud project set-default` (→ `gcloud-project set-default`). **Gated write** — relays the operator-confirmation prompt the CLI enforces; the skill adds no second gate and removes none.

What this skill **deliberately does not** expose (design note [[../../notes/2026-05-28_gcloud-workflow-plugin-architecture|BWOC-51]] §Decision 5):

- **`login`** — `gcloud-auth login` opens a browser and cannot be agent-driven safely. An agent that needs authentication surfaces "please run `gcloud auth login`" to the operator, full stop.
- **Anything outside the two foundation plugins** — no compute, storage, or IAM. Those are separate future skills/epics built on this foundation.

## Why It Exists

Bundling self-orientation into the gcloud plugins would force every agent that wants to know its GCP context to also carry credential-resolution and `gcloud` shell-out concerns — the wrong layer for an agent capability ([[../../notes/2026-05-28_gcloud-workflow-plugin-architecture|BWOC-51 design note]] §Decision 5). Splitting them lets *many* consumers — this skill, a future GCP skill, or the operator's own direct `bwoc gcloud` use — sit on the **same** two plugins, and lets the plugins be installed, configured, audited, and tested independently of any agent. Same substrate, different invoker — the [[../../docs/en/PLUGINS.en#Skill vs Plugin|Skill vs Plugin]] axis applied verbatim.

## The Skill ↔ Plugin Boundary

This is the load-bearing distinction; keep it sharp.

| | `gcloud-ops` (this skill) | the gcloud `workflow` plugins |
|---|---|---|
| Layer | Agent capability | Framework integration |
| Opt-in via | `<agent>/config.manifest.json` (`skills.framework[]`) | `workspace.toml [plugins.<name>]` |
| Invoker | The agent, during its own operation | The framework runtime / `bwoc gcloud` CLI |
| Knows | Self-orientation semantics — who am I, where am I, switch context | ADC vs service-account precedence, `gcloud` CLI invocation, project JSON shape, the operator-confirm gate |
| Does **not** know | How credentials resolve, how `gcloud` is called, the project descriptor shape | Anything about agent self-orientation |

Two rules fall out of the table and are **non-negotiable**:

1. **Dependency is one-way: skill → plugins.** The skill calls the plugins' verbs through `bwoc gcloud`; the plugins have no knowledge of the skill. The gcloud plugins are fully usable with no skill present (the operator runs `bwoc gcloud` directly).
2. **The skill never reads a credential and never calls `gcloud` directly.** It reaches GCP *only transitively through the plugins* — the plugins are the sole `gcloud` callers, and credentials resolve inside `gcloud-auth` from ADC / `.bwoc/secrets/gcloud-sa.json` / `BWOC_GCLOUD_*` env, never through the skill ([[../../notes/2026-05-28_gcloud-workflow-plugin-architecture|BWOC-51 note]] §Decision 3).

## Operations Contract

Each operation composes one or more `bwoc gcloud` verbs. The verb's own gate is inherited — the one write verb (`switch-project`) carries the CLI's operator-confirmation gate ([[../../modules/plugins/workflow/gcloud-project/SPEC|gcloud-project SPEC.md]] §Verbs); the skill adds no second gate.

| Operation | Agent intent | `bwoc gcloud` verb(s) under the hood | Direction | Gate |
|---|---|---|---|---|
| `whoami` | Who am I authenticated as, and where am I pointed? | `auth` + `project show` | read | none — reads are free |
| `current-project` | What is my current default project? | `project show` | read | none — reads are free |
| `switch-project` | Point me at a different default project | `project set-default` | write (local) | operator confirmation (in the CLI) |

The read verbs underpin self-orientation before any GCP action: an agent calls `whoami` to confirm it is authenticated and pointed at the intended project before doing anything else. Every operation is observed by `Kāyānupassanā` (the credential/project state the plugins report) and `Dhammānupassanā` (which gate is in force). Failures surface the operation, the root cause, and the remedy — never just "failed" — including the actionable "no credential resolved; run `gcloud auth login`" when `gcloud-auth status` reports `active_source = none`.

## Dependency Model — skill-on-multiple-plugins

This skill is the framework's **first skill that composes more than one plugin**. Both plugins it drives — `gcloud-auth` and `gcloud-project` — are `workflow`-kind, so the dependency is expressed once through the existing kind-based field:

```toml
[contract]
requires         = []                  # framework SKILL names (the existing field)
requires_plugins = ["workflow"]        # plugin KINDS this skill needs enabled
```

- **`requires_plugins` names plugin _kinds_, not plugin _names_** ([[../../../docs/en/SKILLS.en#Skill-on-plugin dependency|SKILLS.en.md §Skill-on-plugin dependency]]). `"workflow"` is the kind enum value from [[../../docs/en/PLUGINS.en#Plugin Kinds|PLUGINS.en.md]]. The skill depends on the `workflow` *kind* being present; the **specific** plugins it composes (`gcloud-auth` + `gcloud-project`) are enumerated in this SPEC's [Operations Contract](#operations-contract), not in the manifest.
- **Why kind-level, not name-level.** The framework's dependency resolver is kind-based by design — it keeps skills neutral and adapters swappable ([[../../../docs/en/SKILLS.en#Skill-on-multiple-plugins|SKILLS.en.md §Skill-on-multiple-plugins]]). A skill that composes several plugins of one kind lists that kind once; the SPEC names the instances. **Name-level enforcement** (asserting *both* `gcloud-auth` and `gcloud-project` specifically are enabled, not just *some* `workflow` plugin) is a documented future extension — at L1 the skill fails gracefully at invoke time if a composed plugin is absent, surfacing which `bwoc gcloud` verb could not dispatch.
- **Resolved at agent spawn** — if `gcloud-ops` is enabled but no `workflow`-kind plugin is enabled in the workspace, spawn fails fast with a diagnostic naming the missing kind. Caught earlier by `bwoc skill verify gcloud-ops`.

The full dependency model is specified in [[../../../docs/en/SKILLS.en#Skill-on-plugin dependency|SKILLS.en.md §Skill-on-plugin dependency]]; the multiple-plugins refinement in [[../../../docs/en/SKILLS.en#Skill-on-multiple-plugins|§Skill-on-multiple-plugins]].

## Lifecycle Mapping

Per [[../../../docs/en/SKILLS.en#Lifecycle|SKILLS.en.md §Lifecycle]]:

```
init       → resolve the workflow-kind plugin dependency; cache the bwoc gcloud dispatch handle.
             No gcloud call, no credential read — Anattā.
invoke     → each operation composes one or more bwoc gcloud verbs; the one write verb
             (switch-project) inherits the CLI's operator-confirmation gate. Read operations
             are idempotent; switch-project converges (re-setting the current default is a no-op).
teardown   → no-op. The skill holds no external state — credential + project state live in
             gcloud's own config, read transitively through the plugins.
```

The skill holds no global state between invocations. Replay-safe.

## Maturity

Declared **L1** — spec + scaffold; the operations contract is fixed but unverified end-to-end. Live verification against a real GCP project is gated on an operator-provided sandbox (a service-account JSON at `.bwoc/secrets/gcloud-sa.json`, or a logged-in local ADC) — the same `BWOC-EPIC-8` risk the [[../../modules/plugins/workflow/gcloud-auth/SPEC|gcloud-auth]] + [[../../modules/plugins/workflow/gcloud-project/SPEC|gcloud-project]] plugins carry. Bumps to L2 once at least one agent has driven `whoami` → `switch-project` end-to-end; to L3 once `bwoc skill verify gcloud-ops` is wired and passes in CI.

## Neutrality

Manifest values name no LLM backend, model, or vendor CLI. `requires_plugins = ["workflow"]` references the framework's own plugin-**kind** enum value, not a vendor. "GCP" / "Google Cloud" / "gcloud" appear only in `description` and this SPEC's prose as the integration-target name, tolerated under the same rule that lets the plugins name their target. `bwoc gcloud` and `bwoc skill verify` are framework commands, not backend commands. Satisfies **Samānattatā**.

## See Also

- [[../../../docs/en/SKILLS.en|SKILLS.en.md]] — the spec this skill conforms to; the skill-on-plugin + skill-on-multiple-plugins dependency models.
- [[../../modules/plugins/workflow/gcloud-auth/SPEC|gcloud-auth SPEC.md]] — the credential-state plugin this skill drives (`status` verb).
- [[../../modules/plugins/workflow/gcloud-project/SPEC|gcloud-project SPEC.md]] — the project-context plugin this skill drives (`show`, `set-default` verbs).
- [[../../docs/en/PLUGINS.en|PLUGINS.en.md]] — the `workflow` kind row + Skill vs Plugin axis.
- [[../../notes/2026-05-28_gcloud-workflow-plugin-architecture|2026-05-28_gcloud-workflow-plugin-architecture.md]] — EPIC-8 framing (§Decision 5 skill scope, §Decision 2 two-plugin split).
- [[../scrum-via-jira/SPEC|scrum-via-jira]] — the first skill-on-plugin skill; the shape this one extends to multiple plugins.
- [[SPEC.th|SPEC.th.md]] — Thai counterpart (bilingual parity).
