---
title: Incarnation
parent: English
nav_order: 2
---

# Incarnation

How to create a new BWOC agent from the canonical template — start to first commit in under 30 minutes.

This document is the **single source of truth** for incarnation. The README and agent-template README provide quickstarts that link here.

---

## What "Incarnation" Means

A new agent is born by copying [`modules/agent-template/`](../../modules/agent-template/) into its own directory, resolving the `{{placeholders}}` for that agent's identity, and validating backend neutrality. This is the **uppāda** phase of the BWOC arc — identity created, capabilities declared, manifest resolved.

After incarnation, the agent is a self-contained repository. It can be moved, version-controlled, and operated independently. There is no central registry; the framework provides the recipe, the agent owns its instance.

---

## Prerequisites

- The `bwoc` CLI on PATH (see [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md)).
- `git` available on PATH.
- (Optional) The backend CLI of choice — `claude`, `agy`, `codex`, `kimi`, or `ollama` (via `bwoc-harness`) — installed where you'll operate the agent.

---

## Canonical Path

```bash
bwoc new <agent-name>
```

- **`<agent-name>`** — lowercase, hyphen-separated (e.g. `database-schema`); the directory is named `agent-<agent-name>`.
- **`--target <path>`** — optional. Default: `<workspace>/agents/agent-<agent-name>` when a workspace resolves; otherwise next to the template or under the current directory.

`bwoc new` copies the template (auto-detected `modules/agent-template/`, else the copy embedded in the binary), writes the resolved `config.manifest.json`, substitutes manifest fields into `AGENTS.md` and `persona/README.md`, creates the backend symlinks (`CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md`, … → `AGENTS.md`), registers the agent in `.bwoc/agents.toml` when a workspace resolves, and prints next steps. It does not run `git init` or commit.

---

## Setting the Manifest

`bwoc new` accepts manifest fields as inputs, validates them, and writes the resolved manifest. Two input modes:

- **Flags** — every required field has a flag. Example:
  ```bash
  bwoc new <name> \
    --role "database schema reviewer" \
    --primary-model claude-opus-4-8 \
    --fallback-model claude-haiku-4-5 \
    --lint-cmd "cargo clippy" \
    --format-cmd "cargo fmt" \
    --test-cmd "cargo test" \
    --build-cmd "cargo build"
  ```
- **Interactive prompts** — missing required fields trigger a TTY prompt with the field's description from `config.manifest.json` `requiredConfig.<field>.description`. Non-TTY contexts (CI) fail fast with the missing-fields list.

Required fields and their schemas live in `modules/agent-template/config.manifest.json` `requiredConfig`. The CLI reads that schema at incarnation time; adding a new required field is a manifest-schema change, not a CLI change.

## Editing the Manifest After Incarnation

The manifest is owned by the agent after incarnation. **Edit `config.manifest.json` directly** with your editor — that is the canonical path.

Phase 2 may add `bwoc manifest set <key> <value>` and `bwoc manifest get <key>` if direct editing turns out to be a friction point in practice. The framework does not add these commands speculatively (Mattaññutā).

## Step-by-Step

### 1. Run `bwoc new`

```bash
bwoc new foo
```

The new directory (e.g. `agents/agent-foo/`) contains a configured agent: the symlinks are real and the manifest holds the values you supplied or accepted.

### 2. Review `config.manifest.json`

```bash
cd agents/agent-foo
$EDITOR config.manifest.json
```

Confirm the resolved fields. At minimum:

- `agentId` — `agent-<name>`, matching the directory name.
- `agentRole` — one-line role description (e.g. `database schema reviewer`).
- `primaryModel` / `fallbackModel` — backend-agnostic model selector keys (the backend's own CLI resolves these to its native names).
- `memoryPath`, `deepMemoryCmd` — if Tier 2 memory is in use (see [`memories/README.md`](../../modules/agent-template/memories/README.md)).

The schema documentation lives in [`modules/agent-template/conventions.md`](../../modules/agent-template/conventions.md).

### 3. Fill the Identity Section of `AGENTS.md`

Open `AGENTS.md` and edit Section 1 (`Identity`):

- `{{agentId}}` → your agent's ID.
- `{{agentRole}}`, `{{primaryCapability}}`, `{{scopeDescription}}`, `{{outOfScope}}` — concrete descriptions of what this agent does and does not do (Attanutata — knowing self).

These bind the agent's persona. Be specific. A vague scope produces capability spoofing (Threat T-1.4).

### 4. Define the Persona

Edit [`persona/README.md`](../../modules/agent-template/persona/README.md) with the agent's:

- Identity (name, ID, repo, maintainer)
- Domains (declared file paths it touches)
- Principles (which BWOC frameworks it leans on most)
- Boundaries with other agents

Persona examples: [`persona-example.good.md`](../../modules/agent-template/docs/persona-example.good.md) (good) · [`persona-example.bad.md`](../../modules/agent-template/docs/persona-example.bad.md) (anti-pattern).

### 5. Verify Backend Neutrality

```bash
bwoc check .
```

Must exit 0. It checks, among other things:

- `AGENTS.md` is plain Markdown (no YAML frontmatter, no wikilinks).
- Backend symlinks exist and point at `AGENTS.md`.
- `config.manifest.json` parses as valid JSON.
- No `{{placeholders}}` remain in `AGENTS.md` (the runtime `{{taskId}}` excepted).

Any FAIL line names the violation. Fix and re-run.

### 6. First Commit

```bash
git add -A
git commit -m "feat(agent): incarnate agent-foo from BWOC template v2"
```

`bwoc new` does not commit; run `git init` first if the agent directory is not already inside a repository.

**Target: steps 1–6 in under 30 minutes.**

---

## Adding a Backend

The five default backends (Claude, Antigravity, Codex, Kimi, Ollama) ship as symlinks. Adding a sixth is one command:

```bash
ln -s AGENTS.md <BACKEND>.md
```

No other change required. Re-run `bwoc check` to confirm.

This is **Samānattatā** — equal treatment — enforced at the file-system level.

---

## Bilingual / Multilingual Setup

The template ships with `docs/en/` and `docs/th/` pairs. For each `docs/en/*.en.md`, there is a matching `docs/th/*.th.md`. When you edit one, edit the other.

To add a third language (e.g. Japanese, ISO 639-1 `ja`):

```bash
mkdir docs/ja
# Translate each docs/en/<NAME>.en.md to docs/ja/<NAME>.ja.md
```

`<lang>` is BCP 47 / ISO 639-1. The convention is documented in [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md#multilingual-structure). No code change required.

---

## Verification Checklist

Before declaring the agent ready:

- [ ] `bwoc check` exits 0
- [ ] `config.manifest.json` has no unresolved `{{placeholders}}`
- [ ] `AGENTS.md` Section 1 reflects this agent (not the template defaults)
- [ ] `persona/README.md` names domains and boundaries
- [ ] `task-log.jsonl` exists (empty is fine — entries arrive at first task)
- [ ] All `docs/en/*.en.md` files have matching `docs/th/*.th.md` (if your agent ships bilingual docs)
- [ ] Backend CLI of choice is on PATH and recognizes the agent's directory

---

## After Incarnation — Reading Path

For the agent's first operator session:

1. [`AGENTS.md`](../../modules/agent-template/AGENTS.md) — the agent's full instruction set.
2. [`docs/en/OVERVIEW.en.md`](../../modules/agent-template/docs/en/OVERVIEW.en.md) — 5-min orientation.
3. [`docs/en/PHILOSOPHY.en.md`](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) — 22 frameworks (Groups A–F).
4. [`docs/en/PRD.en.md`](../../modules/agent-template/docs/en/PRD.en.md) and [`SRS.en.md`](../../modules/agent-template/docs/en/SRS.en.md) — product and requirements.
5. [`docs/en/THREAT-MODEL.en.md`](../../modules/agent-template/docs/en/THREAT-MODEL.en.md) — Taṇhā 3 + Sīla 5.

---

## See Also

- [`ARCHITECTURE.en.md`](ARCHITECTURE.en.md) — how the pieces fit at runtime.
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — Pali term → engineering meaning lookup.
- [`VISION.md`](../../VISION.md) — why incarnation is modelled as uppāda.
- [`modules/agent-template/conventions.md`](../../modules/agent-template/conventions.md) — placeholder schema and YAML rules.
- [`modules/agent-template/neutrality.md`](../../modules/agent-template/neutrality.md) — why neutrality is enforced.
