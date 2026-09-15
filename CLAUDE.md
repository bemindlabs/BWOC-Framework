# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Repo Is

The **BWOC framework** (Buddhist Way of Coding) — a backend-neutral specification for AI coding agents, plus the native Rust implementation that incarnates and runs them.

Distinguish two scopes when reading or editing:

- **Framework root** (this directory) — framework spec docs (`docs/en/`, `docs/th/`, root-level `README.md` / `VISION.md` / `VERSION.md` / `CONTRIBUTING.md`), the Rust workspace (`crates/bwoc-cli`, `crates/bwoc-agent`, `crates/bwoc-core`, `Cargo.toml`), and helper scripts (`scripts/install.sh`, `scripts/bump-version.sh`).
- **Agent template** (`modules/agent-template/`) — the cloneable artifact. Has its own `AGENTS.md`, `CLAUDE.md`, `docs/`, `interconnect/`, `memories/`, `persona/`, `mindsets/`, `skills/`.

The template's `CLAUDE.md` is for incarnated agents reading themselves; this file is for Claude editing the framework.

## Two-Tier Document Format (HARD RULE)

| Tier | Format | Files |
|---|---|---|
| Instructions | Plain Markdown — **no YAML, no wikilinks, no callouts** | `modules/agent-template/AGENTS.md` and its backend symlinks (`CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`) |
| Documentation | Obsidian Markdown — YAML frontmatter, wikilinks, callouts allowed | All other `.md` files |

`AGENTS.md` must stay parseable by any LLM backend without Obsidian. Approved callout types in tier 2: `abstract`, `tip`, `warning`, `example`, `note`, `danger` — no others.

## Backend Neutrality (HARD RULE)

- All configurable values in `AGENTS.md` use `{{camelCase}}` placeholder syntax (e.g., `{{agentId}}`, `{{primaryModel}}`). Never hardcode model IDs, vendor names, or tool names in `AGENTS.md`.
- Backend-specific phrasing (Claude, Antigravity, Codex, Kimi) belongs only in Section 0 "Backend Registration" of `AGENTS.md` or in vendor entry files that symlink to it.
- Adding a new backend is one command: `ln -s AGENTS.md <BACKEND>.md`. Do not create separate per-backend content.

## Bilingual Parity (HARD RULE)

Every `docs/en/*.en.md` has a counterpart `docs/th/*.th.md`. When you edit one, edit the other in the same change. This applies to both the framework root `docs/` and `modules/agent-template/docs/`.

Existing template doc pairs: `OVERVIEW`, `PHILOSOPHY`, `PRD`, `SELF-IMPROVEMENT`, `SRS`, `THREAT-MODEL`.

### What requires a TH pair (root level)

| File type | TH pair required? | Why |
|---|---|---|
| **Doctrine docs at root** — `VISION.md`, any future `PHILOSOPHY-AT-ROOT.md` | **Yes** (`VISION.md` ↔ `VISION.th.md`) | Project identity; the Thai-speaking audience reads these to decide whether BWOC is for them. |
| **Spec docs under `docs/<lang>/`** | **Yes** | Already enforced by the per-lang directory convention. |
| **Mechanical OSS docs at root** — `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `LICENSE`, `VERSION.md` | **No, by design** | Translation cost outweighs value for short-lived process docs that change with every PR. EN-only keeps the maintenance burden honest. |
| **Crate READMEs** — `crates/*/README.md` | **No, until needed** | Code-side convention. Adopt TH pair only when the framework explicitly targets a Thai developer audience. |
| **Implementation notes** — `notes/YYYY-MM-DD_<title>.md` | **No** | Per-session logs; ephemeral. |
| **`.claude/` content** — `loop-roadmap.md`, etc. | **No** | Operator-internal; not a public-contributor surface. |

When in doubt: if the doc states project direction or principles, pair it. If it documents how-to-contribute or current state-of-the-repo, EN-only is fine.

## Repo State Quirks

- **The framework repo is itself a BWOC workspace** (`.bwoc/workspace.toml` at root) used for end-to-end CLI testing. Its `.gitignore` therefore ignores `.bwoc/`, `agents/`, `projects/` entirely — the **opposite** of what `bwoc init` writes for user workspaces. See `.gitignore` lines 176-196 and `crates/bwoc-cli/src/init.rs::GITIGNORE_TEMPLATE` for the contrast.
- **`applications/` is an empty placeholder** for Phase 4. `examples/` and `docs/{en,th}/` now have real content — check before assuming empty.
- **Referenced but not present**: `.github/CODEOWNERS` — `CONTRIBUTING.md` links it. Do not pretend it exists.
- **Auto-version hook** (`.claude/hooks/auto-version.sh`) bumps `Cargo.toml` patch on `.rs`/`.toml` writes and `VERSION.md` `Document-Version` on `.md` writes. Expect those files to appear in your working tree after edits — that's the hook, not drift.

## Template Tooling

The template ships no shell scripts. Incarnate and audit agents with the CLI:

```bash
bwoc new <agent-name>      # copies the template into a new agent (crates/bwoc-cli/src/new.rs)
bwoc check <path>          # backend-neutrality audit of the template or an agent (crates/bwoc-cli/src/check.rs)
```

## Commits

Lightweight Conventional Commits: `type(scope): subject`. Example: `docs(philosophy): clarify Yoniso Manasikāra`. See `CONTRIBUTING.md` for the full PR checklist.

## Merging & Pull Requests (HARD RULE)

`main` is a **protected branch** in every BWOC repo (the fork included). Changes land **only** through a pull request that auto-merges once its gates are green — **never a direct push to `main`**, not even for automation or a one-line sync. Full rules: [`CONTRIBUTING.md` §Merging & External PRs](CONTRIBUTING.md#merging--external-prs). The load-bearing points:

- **A PR is mergeable only when all hold:** the four required CI checks pass (**fmt + clippy** and **build + test** on macOS, Linux, Windows); the branch is **up to date with `main`** (`gh pr update-branch <pr>` if stale — strict protection rejects a stale branch even when conflict-free); and **every review conversation is resolved**.
- **Use native auto-merge** — `gh pr merge <pr> --squash --auto` — and let GitHub merge the moment gates clear. Do **not** hand-merge (`--merge`/`--squash` immediately) to skip the CI gate.
- **Cross-repo (fork → upstream) PRs get a full review before merge — never a blind merge.** They carry external code into a security framework, so each is scrutinized for injection / path-traversal / RCE, dep-quarantine (`bwoc-core` stays lean), backend neutrality, and EN/TH parity. Security or correctness blockers → **request changes** (the conversation-resolution gate keeps it blocked) rather than "merge to fix later."
- **One concern per PR.** Keep diffs focused. A sprint's worth of unrelated stories must not ride in a single mega-PR — split per feature.

When working the **fork** (`kla-ondemand/BWOC-Framework`) as a staging ground, hold the same discipline: branch → PR → let CI gate → auto-merge. Do not direct-push to fork `main`.

## Philosophy Grounding

Every structural decision in this repo maps to one of the 22 Buddhist frameworks in `@modules/agent-template/docs/en/PHILOSOPHY.en.md`. On conflict, the philosophy document wins on the principle. The five most-applied:

1. **Yoniso manasikāra** — verify against current files, not memory or assumption.
2. **Mattaññutā** — right amount; don't bloat docs (`MEMORY.md` capped at 200 lines).
3. **Anattā** — no clinging to stale branches or worktree state.
4. **Samānattatā** — treat all backends equally.
5. **Sīlasāmaññatā** — follow the communal conventions in `@modules/agent-template/conventions.md`.

## Implementation Logs (HARD RULE)

Every **significant change** to this repo gets a note in `notes/` following the [`NAMING.en.md`](docs/en/NAMING.en.md#yyyy-mm-dd_title-md--notes-new) pattern: `notes/YYYY-MM-DD_<title>.md`. Notes are **development-oriented** (what changed, *why*, decisions, alternatives, bugs surfaced) — distinct from `CHANGELOG.md`, which is release-oriented ("what shipped"). Both serve.

A change is "significant" if any of these are true:

- New documentation file added or removed (not a fix/typo)
- New code module, crate, hook, skill, workflow, or script
- Decision affecting future contributors (architecture, naming, versioning, policy)
- Bug fix that changed behavior observers can notice
- Anything that warranted a CHANGELOG entry beyond a single line

Note skeleton (lean — fill only the sections that apply):

```markdown
# YYYY-MM-DD — <Title>

<one-paragraph summary>

## What changed
## Decisions
## Alternatives considered
## Bugs surfaced and fixed
## Status / deferred
## Related (links)
```

Write one note per session, not per file. A session that produces a multi-doc spec or scaffold gets ONE note covering all of it. Daily routine maintenance does not need a note.

---

## Over-Engineering Protection (HARD RULE)

**Default to NOT adding.** Mattaññutā is enforced, not just named.

- Do not proactively add new files, hooks, skills, workflows, or specs unless the user explicitly requests them in this session.
- Do not chain ahead — finish what's asked, surface what's next, wait for direction.
- Cancel automated work (crons, loops) when it has done its job; resuming requires a fresh user signal.
- For any multi-file or multi-doc addition, propose first with explicit ROI per line. If a line doesn't earn its place, cut it.
- Prefer **trimming** over expanding when both are reasonable. README trim, dedupe, deletion of dead references are valid pull-back moves.
- Audit signals to watch: file-count growth per turn, line-count growth per turn, number of new docs without explicit ask, hook depth, skill count growth.

When in doubt, do less. The framework's own [`VISION.md` §Principles](VISION.md#principles-that-govern-hard-tradeoffs) names "the smaller specification beats the more complete one" as a tradeoff principle.
