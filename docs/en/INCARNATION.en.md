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

- A shell (bash, zsh, or PowerShell with Git Bash on Windows).
- `git` available on PATH.
- `rsync`, `ln`, `python3` on PATH (used by `incarnate.sh` and `check-agent-neutrality.sh`).
- (Optional) The backend CLI of choice — `claude`, `agy`, `codex`, `kimi`, or `ollama` (via `bwoc-harness`) — installed where you'll operate the agent.

The `bwoc` Rust CLI is Phase 1 v2.0 in progress; today's canonical path uses the shell scripts shipped with the template. Once `bwoc new` ports the script's logic, the command becomes a single invocation.

---

## Canonical Path (today)

From the framework root:

```bash
cd modules/agent-template
./scripts/incarnate.sh <agent-name> [target-path]
```

Defaults:

- **`<agent-name>`** — lowercase, hyphen-separated (e.g. `agent-database-schema`).
- **`[target-path]`** — optional. Default: `../agent-<agent-name>/` relative to the template.

The script copies the template, creates backend symlinks (`CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md` → `AGENTS.md`), initializes git, makes the first commit, and runs the neutrality check. Output names every step and ends with a "Next steps" block.

---

## Setting the Manifest — current vs planned

**Today (via `incarnate.sh`):** the script copies the template and stops. You edit `config.manifest.json` manually to resolve placeholders.

**With `bwoc new` (Phase 1 v2.0 in progress):** the CLI accepts manifest fields as inputs, validates them, and writes the resolved manifest atomically. Two input modes:

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

### 1. Run `incarnate.sh`

```bash
./scripts/incarnate.sh agent-foo
```

Produces:

```
+ CLAUDE.md -> AGENTS.md
+ AGY.md    -> AGENTS.md
+ CODEX.md  -> AGENTS.md
+ KIMI.md   -> AGENTS.md
+ OLLAMA.md -> AGENTS.md
+ git initialized
...
Done in 3s
```

The new directory `../agent-foo/` now contains a working but unconfigured agent. Symlinks are real; the manifest still holds `{{placeholders}}`.

### 2. Edit `config.manifest.json`

```bash
cd ../agent-foo
$EDITOR config.manifest.json
```

Resolve every required placeholder. At minimum:

- `agentId` — matches the directory name without the `agent-` prefix.
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

A good persona example: see `modules/agent-template/docs/README.md` (currently misnamed — to be renamed `examples/persona-good.md`).

### 5. Verify Backend Neutrality

```bash
./scripts/check-agent-neutrality.sh
```

Must exit 0. The script checks for:

- `AGENTS.md` is plain Markdown (no YAML frontmatter, no wikilinks).
- Backend symlinks exist and point at `AGENTS.md`.
- `config.manifest.json` parses as valid JSON.
- No hardcoded model IDs or vendor-specific phrasing in `AGENTS.md`.

Any FAIL line names the violation. Fix and re-run.

### 6. First Commit

```bash
git add -A
git commit -m "feat(agent): incarnate agent-foo from BWOC template v2"
```

`incarnate.sh` already created the initial scaffold commit; this is your first **configured** commit.

**Target: steps 1–6 in under 30 minutes.**

---

## Adding a Backend

The five default backends (Claude, Antigravity, Codex, Kimi, Ollama) ship as symlinks. Adding a sixth is one command:

```bash
ln -s AGENTS.md <BACKEND>.md
```

No other change required. Re-run `check-agent-neutrality.sh` to confirm.

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

- [ ] `./scripts/check-agent-neutrality.sh` exits 0
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
