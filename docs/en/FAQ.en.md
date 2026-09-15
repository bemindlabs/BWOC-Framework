---
title: FAQ
parent: English
nav_order: 9
---

# FAQ

The questions newcomers ask in their first few hours with BWOC. Concise answers; cross-references to the spec for depth.

---

## Conceptual

### Do I need to know Buddhism?

No. Pali terms are **labels** for engineering concepts. The content is purely technical. The [`GLOSSARY.en.md`](GLOSSARY.en.md) gives a one-line engineering meaning for every Pali term in the framework — most readers find it sufficient.

### Is BWOC a religious framework?

No. BWOC uses Buddhist frameworks as **engineering thinking aids**, not religious instruction. The non-religious stance is enforced — see [`VISION.md` §Non-Goals](../../VISION.md#non-goals) and the [`CODE_OF_CONDUCT.md`](../../CODE_OF_CONDUCT.md) framing note. Contributors of any background, faith, or non-faith are welcome.

### Why Buddhist frameworks specifically?

Western engineering frameworks (DDD, Clean Architecture, SOLID, Hexagonal) are precise about structure and dependency. They are thin on **state impermanence, failure tracing, lifecycle, inter-agent trust, and threat modeling** — the exact dimensions where agent systems fail. Buddhist frameworks happen to have unusually precise vocabulary for those dimensions. See [`VISION.md` §The Gap](../../VISION.md#the-gap).

### Does this conflict with DDD / Clean Architecture / SOLID?

No. BWOC **extends** those frameworks into dimensions they were never designed to address. They handle structure; BWOC handles arc, intent, trust, and discipline. Use both.

### Can I use BWOC without the Buddhist framing?

Yes. Keep the technical skeleton — manifest, neutrality, lifecycle, threat model, CLI surface. You lose the unified "why" behind design decisions and the shared vocabulary, but nothing in the implementation requires the framing.

---

## Project Mechanics

### What is the difference between *phase* and *version*?

**Phase** describes implementation milestones (Phase 1 v2.0 = uppāda foundation, Phase 2 = ṭhiti operations, Phase 3 = vaya + interconnect, Phase 4 = reference agents + fleet). **Version** describes release identity (SemVer). One phase may span several SemVer releases. See [`VERSION.md` §Phase vs Version](../../VERSION.md#phase-vs-version).

### Why is the spec written before any code?

Documents-first is doctrine. Code follows the spec, not the reverse. The 22 framework mappings, the arc (uppāda · ṭhiti · vaya), the workspace structure, and the CLI surface were all specified in Markdown before the Rust workspace was scaffolded. The discipline is **Yoniso Manasikāra** — verify intent before action.

### What's the difference between `Software-Version` and `Document-Version`?

They evolve independently. `Software-Version` lives in `Cargo.toml` and tracks code changes (`.rs` / `.toml` edits bump it). `Document-Version` lives in `VERSION.md` and tracks documentation changes (`.md` edits bump it). Both are auto-bumped on every Claude Code edit by `.claude/hooks/auto-version.sh`. See [`VERSION.md`](../../VERSION.md).

---

## Setup

### How do I create a new agent?

```bash
bwoc new <agent-name>
```

Then review the manifest, define persona, run `bwoc check`. Target: first commit in under 30 minutes. Full walkthrough in [`INCARNATION.en.md`](INCARNATION.en.md).

### Where do incarnated agents actually live on disk?

Anywhere you want. Each agent is a self-contained repository copied from the template. The recommended layout is inside a **workspace** at `<workspace>/agents/agent-<name>/`, but you can place agents wherever your filesystem and version-control workflow prefer. There is no central registry. See [`WORKSPACE.en.md`](WORKSPACE.en.md).

### What is a *workspace* and do I need one?

A workspace is a directory the CLI uses as the home for your BWOC work. It holds a `.bwoc/` marker (`workspace.toml`, `agents.toml`), optionally a workspace-scoped memory, and an `agents/` directory for incarnated agents. **You need one** to use the operational CLI commands (`bwoc spawn`, `bwoc list`, etc.) — they refuse to run without a complete workspace. Run `bwoc init` to create one. See [`WORKSPACE.en.md`](WORKSPACE.en.md).

### What lives in `~/.bwoc/`?

Per-user, machine-level state independent of any workspace: `config.toml` (default backend, default language, default workspace), `memory/` (central memory shared by every agent you run on this machine), `workspaces.toml` (registry of workspaces the CLI has seen), and `logs/` (CLI invocation logs). See [`WORKSPACE.en.md` §Central Memory](WORKSPACE.en.md#central-memory--bwoc).

---

## Multi-Language and Multi-Backend

### How do I add a new LLM backend?

One command, no code change:

```bash
ln -s AGENTS.md <BACKEND>.md
```

The backend reads `AGENTS.md` via its own symlink; no per-backend instructions exist by design (Samānattatā — equal treatment). Then re-run `bwoc check` to confirm. See [`INCARNATION.en.md` §Adding a Backend](INCARNATION.en.md#adding-a-backend).

### How do I add a new human language for docs?

```bash
mkdir docs/<lang>          # <lang> = BCP 47 / ISO 639-1 (e.g. "ja", "zh", "de")
# Translate each docs/en/<NAME>.en.md to docs/<lang>/<NAME>.<lang>.md
```

English is canonical; other languages are translations. The framework root, the agent template, and the CLI all use the same `<lang>` pattern (`docs/<lang>/<NAME>.<lang>.md` + `FILENAME.md` ↔ `FILENAME.<lang>.md` at root + `crates/bwoc-cli/locales/<lang>/cli.ftl`). See [`ARCHITECTURE.en.md` §Multilingual Structure](ARCHITECTURE.en.md#multilingual-structure).

### How do I switch the CLI's output language?

Precedence: `--lang <code>` flag → `BWOC_LANG` env → `$LANG` env → `en` fallback. The CLI ships with TH and EN at launch; adding a third language is a folder drop under `crates/bwoc-cli/locales/`.

---

## Conventions

### What pattern should I use to name a new Markdown file?

The single source is [`NAMING.en.md`](NAMING.en.md) — 12 categories with a decision tree. Quick summary:

- Top-level project metadata (OSS standard) → `UPPERCASE.md`
- Spec docs → `UPPERCASE.<lang>.md` in `docs/<lang>/`
- Template prose → `lowercase-hyphen.md`
- Slot READMEs → `README.md` (Obsidian format)
- Crate READMEs → `README.md` (plain Markdown)
- Skills → `SKILL.md`
- Memory index → `MEMORY.md`; entries → `<type>_<slug>.md`
- **Notes → `YYYY-MM-DD_<title>.md`**
- Translations of root files → `FILENAME.<lang>.md` (e.g. `VISION.th.md`)

### Where should I put a session note or decision record?

A note follows `YYYY-MM-DD_<title>.md`. Three valid locations: `<repo>/notes/` (project-level), `<workspace>/.bwoc/notes/` (workspace-scoped), `~/.bwoc/notes/` (per-user). Pick the scope that matches the note's audience. See [`NAMING.en.md` §Notes](NAMING.en.md#yyyy-mm-dd_title-md--notes-new).

---

## Operations

### Can I run multiple agents in parallel?

Yes. Each agent is a self-contained repo with its own backend subprocess. Phase 1 spawns them independently; Phase 2 adds the `bwoc-agent` control socket so the CLI can supervise; Phase 3 adds inter-agent messaging for coordination (Sammā-vācā + Sāraṇīyadhamma 6). See [`ROADMAP.en.md`](ROADMAP.en.md).

### What happens when an agent finishes?

It enters **vaya** — the cessation phase. Worktree is cleaned, branch released, memory pruned, task closed. Phase 1 leaves this manual; Phase 3 introduces `bwoc retire <name>` which performs the full cleanup atomically. The discipline is **Anattā** — no clinging.

### What if I find a security issue?

Do not open a public issue. Email **info@bemind.tech** with details. See [`SECURITY.md`](../../SECURITY.md) for the disclosure process and [`THREAT-MODEL.en.md`](../../modules/agent-template/docs/en/THREAT-MODEL.en.md) for the full threat surface.

---

## Contributing

### How do I contribute?

See [`CONTRIBUTING.md`](../../CONTRIBUTING.md) for the workflow, commit style, and PR checklist. New contributors typically read in this order: [`VISION.md`](../../VISION.md) → [`GLOSSARY.en.md`](GLOSSARY.en.md) → [`PHILOSOPHY.en.md`](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) (groups A–F) → [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) → the area you want to work in.

### What if I want to contribute in a language other than English?

Translation contributions are welcome. The spec docs follow the bilingual rule: every EN doc has a matching translation in `docs/<lang>/<NAME>.<lang>.md`. Open a PR with both the EN edit and the matching translation edit; the bilingual-reminder hook will flag any mismatch.

### What if the framework doctrine and my use case disagree?

Open an issue describing the friction. The framework is normative but not infallible. Doctrine evolves through the same Ariyasacca 4 cycle (Dukkha → Samudaya → Nirodha → Magga) it asks of agents.

---

## See Also

- [`VISION.md`](../../VISION.md) — why BWOC exists.
- [`PHILOSOPHY.en.md`](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) — full conceptual core.
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — Pali term lookup.
- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — how the pieces fit.
- [`INCARNATION.en.md`](INCARNATION.en.md) — how to create a new agent.
- [`WORKSPACE.en.md`](WORKSPACE.en.md) — workspace structure and central memory.
- [`NAMING.en.md`](NAMING.en.md) — Markdown file naming standard.
- [`ROADMAP.en.md`](ROADMAP.en.md) — phase plan.
