---
title: scrum-via-jira skill + skill-on-plugin dependency model
date: 2026-05-27
sprint: BWOC sprint-7
epic: BWOC-EPIC-6
story: BWOC-44
related_stories: [BWOC-40, BWOC-41, BWOC-42, BWOC-43, BWOC-45]
---

# 2026-05-27 — scrum-via-jira skill (BWOC-44, closes EPIC-6)

Lands the `scrum-via-jira` framework skill and the **skill-on-plugin dependency model** the [BWOC-40 design note](2026-05-27_jira-plugin-architecture.md) §5 motivated. The skill is the agent-facing scrum surface that drives the `jira`-kind plugin (`jira-cloud-rest`, shipped in BWOC-43) through its `bwoc jira` verbs — the last piece of EPIC-6 after the kind/schema (BWOC-41), the CLI (BWOC-42), and the reference plugin (BWOC-43). No Rust changes; spec + scaffold only.

## What changed

- **New skill `modules/skills/scrum-via-jira/`** — `manifest.toml` + `SPEC.md` + `SPEC.th.md`. Exposes six scrum operations (`propose-sprint`, `open-sprint`, `transition-story`, `sync-backlog`, `close-sprint`, `list-active-sprints`), each documented as a thin call onto one or more `bwoc jira` verbs (`query` / `link` / `transition` / `sync` / `status`). The skill knows scrum; the plugin knows REST/auth/JQL/rate-limits/ledger — the boundary is a first-class section in the SPEC.
- **`SKILLS.en.md` + `SKILLS.th.md`** — added the `requires_plugins` contract field to the manifest example and field-reference table, a new `## Skill-on-plugin dependency` section, the spawn-resolution step for plugin kinds (Discovery step 4), and a `bwoc check` row validating `requires_plugins` kinds. EN+TH edited in the same commit (bilingual parity).

## Decisions

- **`requires_plugins` is a dedicated `[contract]` field, not an overload of `requires`.** Ratifies the BWOC-40 note §5 recommendation. `requires` resolves against skill *names*; `requires_plugins` against plugin *kinds*. Conflating the namespaces would make a bare `"jira"` ambiguous (skill named `jira`? kind `jira`?).
- **`requires_plugins` names plugin _kinds_, not plugin _names_.** The skill depends on *any* enabled `jira`-kind adapter, never on `jira-cloud-rest` specifically — keeps the skill neutral and lets the adapter be swapped. `"jira"` is the framework's own kind enum (PLUGINS.en.md), so it is not a vendor-name neutrality violation.
- **Dependency direction is one-way: skill → plugin.** The plugin never declares a skill dependency and is fully usable without the skill (operator runs `bwoc jira` directly).
- **Resolution is three-tiered:** static kind-validity at `bwoc check`, full enabled-in-workspace check at `bwoc skill verify` and again at agent spawn (fail-fast, never half-wired).
- **Operation names are kebab-case** (`propose-sprint`) per the BWOC-40 note and story wording, vs worktree-discipline's snake_case `claim_task` — the spec mandates no case convention for operation names.

## Alternatives considered

- **Overload `[contract].requires` with a `plugin:` namespace** (`requires = ["plugin:jira"]`) — rejected; keeps the skill-name namespace clean and reads unambiguously at verify / spawn (BWOC-40 note §5).
- **Reference the plugin by name** (`requires_plugins = ["jira-cloud-rest"]`) — rejected; pins the skill to one vendor adapter and breaks neutrality + swappability. Depend on the kind.
- **Bundle the scrum operations into the plugin** — rejected upstream in BWOC-40 §5; forces REST/auth concerns onto every agent and blocks multiple consumers sharing one integration.

## Status / deferred

- Spec + scaffold complete; gates (`cargo fmt/clippy/test/build`, `bwoc check --all`, `bwoc skill verify scrum-via-jira`) run on the branch.
- **CLI enforcement of `requires_plugins` is forward-declared.** The spawn-time / verify-time resolution is specified here; wiring it into `bwoc` (the resolver + `bwoc check` kind-validity row) is a downstream impl concern (CLI lane), not part of this spec story.
- Live end-to-end verification against a real Jira Cloud instance remains gated on an operator-provided sandbox token (the standing EPIC-6 risk) — unchanged by this story.
- Unblocks **BWOC-45** (rose — `bwoc check` extension for the skill/plugin surface).

## Related

- Design framing: [2026-05-27_jira-plugin-architecture.md](2026-05-27_jira-plugin-architecture.md) §5 (plugin-vs-skill split), §6 (single-writer ledger).
- Skill: [`modules/skills/scrum-via-jira/SPEC.md`](../modules/skills/scrum-via-jira/SPEC.md) / [`SPEC.th.md`](../modules/skills/scrum-via-jira/SPEC.th.md).
- Plugin it drives: [`modules/plugins/jira-cloud-rest/SPEC.md`](../modules/plugins/jira-cloud-rest/SPEC.md) (BWOC-43).
- Spec: [`docs/en/SKILLS.en.md`](../docs/en/SKILLS.en.md) / [`docs/th/SKILLS.th.md`](../docs/th/SKILLS.th.md) §Skill-on-plugin dependency.
- Epic: `BWOC-EPIC-6` (Jira Plugin Kind + scrum-via-jira Skill).
