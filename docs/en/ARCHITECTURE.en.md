---
title: Architecture
parent: English
nav_order: 1
---

# Architecture

How the BWOC framework, the agent template, incarnated agents, the CLI, and the runtime fit together — at the level of files, processes, and information flow.

For the **conceptual** stack (the 22 Buddhist-framework groupings), see [`PHILOSOPHY.en.md`](../../modules/agent-template/docs/en/PHILOSOPHY.en.md). This document is about **implementation**.

---

## Implementation Stack

```
┌──────────────────────────────────────────────────────┐
│  Framework repository (this repo)                    │  ← spec + tooling
│  - Markdown specification                            │
│  - Rust workspace (crates/)                          │
│  - Claude Code hooks, skills, memory                 │
└──────────────────────────────────────────────────────┘
                       │ provides
                       ▼
┌──────────────────────────────────────────────────────┐
│  Agent template (modules/agent-template/)            │  ← blueprint
│  - AGENTS.md (single source of truth)                │
│  - Backend symlinks (CLAUDE/AGY/CODEX/KIMI.md)       │
│  - Slots: persona, memories, interconnect, ...       │
│  - bwoc-agent binary (ships with each agent)         │
└──────────────────────────────────────────────────────┘
                       │ cloned by `bwoc new`
                       ▼
┌──────────────────────────────────────────────────────┐
│  Incarnated agents (anywhere on disk)                │  ← one repo each
│  - One directory per agent — no central registry     │
│  - {{placeholders}} resolved at incarnation time     │
└──────────────────────────────────────────────────────┘
                       │ managed by ↓
┌──────────────────────────────────────────────────────┐
│  bwoc CLI (crates/bwoc-cli/)                         │  ← arc orchestrator
│  - uppāda: new, check                                │
│  - ṭhiti:  spawn, list, status, log, send            │
│  - vaya:   stop, retire                              │
│  - Localized output (TH · EN; folder-drop locales)   │
└──────────────────────────────────────────────────────┘
                       │ `bwoc spawn` exec's ↓
┌──────────────────────────────────────────────────────┐
│  Backend execution                                   │  ← LLM runtime
│  - Subprocess: claude · agy · codex · kimi CLI       │
│    or bwoc-harness (ollama / OpenAI-compatible)      │
│  - Backend reads AGENTS.md via its own symlink       │
│  - bwoc-agent runtime (Phase 2+) for control socket  │
└──────────────────────────────────────────────────────┘
```

---

## Layers

### 1. Framework Repository

This repo. Holds:

- The Markdown specification — `AGENTS.md`, the 22 framework mappings in `PHILOSOPHY.en.md`, `PRD`, `SRS`, `THREAT-MODEL`, etc.
- The Rust workspace under `crates/` — `bwoc-core`, `bwoc-cli`, `bwoc-agent`.
- Claude Code tooling under `.claude/` — skills (`/incarnate`, `/check-neutrality`, `/check-bilingual`, `/task-log`), the bilingual-reminder hook, project memory.

No agents *live* here. The framework provides the recipe.

### 2. Agent Template — `modules/agent-template/`

The canonical blueprint copied into every new agent. Contains:

- **`AGENTS.md`** — backend-neutral single source of truth.
- **Backend symlinks** — `CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `OLLAMA.md`, `OPENAI.md` all point at `AGENTS.md`.
- **`config.manifest.json`** — the placeholder schema (`{{agentId}}`, `{{primaryModel}}`, etc.).
- **Slots** — `persona/`, `memories/`, `interconnect/`, `mindsets/`, `skills/`.
- **`scripts/`** — `incarnate.sh`, `check-agent-neutrality.sh`.
- **`bwoc-agent`** binary — shipped with each incarnated agent (Phase 1: liveness stub).

### 3. Incarnated Agents

Created by `bwoc new <name>`, which copies the template to a new directory and resolves placeholders. After incarnation:

- The agent is a self-contained repo. It can be moved, forked, and version-controlled independently.
- **No central registry.** `bwoc list` discovers agents by scanning a configured search path (Phase 1: filesystem convention; Phase 2 may add an opt-in cache).
- The agent's `AGENTS.md` is its own — divergence from the template is expected and acceptable.

### 4. CLI — `crates/bwoc-cli/`

The `bwoc` binary. Native single binary for macOS · Linux · Windows.

- **Thin orchestrator.** Does not embed an LLM client. Talks to backend CLIs by subprocess.
- **Localized.** TH and EN ship at launch; any future language is a folder drop under `crates/bwoc-cli/locales/`.
- **Arc-aligned.** Commands are organized by the three phases (uppāda · ṭhiti · vaya).

See [`crates/bwoc-cli/README.md`](../../crates/bwoc-cli/README.md) for install and per-command status.

### 5. Backend Execution

Phase 1 model: **`bwoc spawn` exec's the configured backend CLI** (Claude Code, Antigravity CLI, Codex CLI, Kimi CLI) or **`bwoc-harness`** (`bwoc spawn --backend ollama` — self-hosted Ollama / any OpenAI-compatible endpoint) in the agent's directory. The backend reads `AGENTS.md` via its own backend file (which symlinks there), and operates per the spec. See [`docs/en/HARNESS.en.md`](HARNESS.en.md) for the harness spec.

Phase 2+ adds the `bwoc-agent` runtime alongside, exposing a control socket so `bwoc status` and `bwoc send` can talk to a live agent.

---

## Information Flow — `bwoc spawn agent-foo`

```
1. User                  bwoc spawn agent-foo
2. CLI                   resolve `agent-foo` to a directory on disk
                         (search path: cwd → ~/bwoc-agents/ → $BWOC_PATH)
3. CLI                   read agent-foo/config.manifest.json
                         resolve {{primaryBackend}}, {{primaryModel}}
4. CLI                   `cd agent-foo && exec <backend-cli>`
                         (e.g., `exec claude code`)
5. Backend CLI           read AGENTS.md (via its symlinked entry file)
                         apply persona, manifest, capabilities
6. Agent                 execute the task per Ariyasacca-4 cycle
                         append entries to task-log.jsonl
7. Agent                 on completion, exit; OR
                         (Phase 2) bwoc-agent stays resident on a socket
```

No central daemon, no shared state across agents. Coordination (Phase 3) flows through `interconnect/` files and explicit `bwoc send` messages — never through global state.

---

## Backend Neutrality

`AGENTS.md` is the *only* place where instructions live. Adding a new backend is one command:

```bash
ln -s AGENTS.md <BACKEND>.md
```

No other change required. The CLI's `--backend` flag picks which backend CLI to invoke at spawn time; the spec the backend reads is unchanged.

See [`modules/agent-template/neutrality.md`](../../modules/agent-template/neutrality.md) for the validation rules, and `/check-neutrality` for the runnable audit.

---

## Multilingual Structure

Three parallel patterns, all keyed by BCP 47 / ISO 639-1 codes:

| Surface | Path pattern | Example |
|---|---|---|
| Framework-root docs | `docs/<lang>/<NAME>.<lang>.md` | `docs/en/GLOSSARY.en.md`, `docs/th/GLOSSARY.th.md` |
| Root-level metadata | `FILENAME.md` (EN canonical) + `FILENAME.<lang>.md` | `VISION.md` + `VISION.th.md` |
| CLI strings | `crates/bwoc-cli/locales/<lang>/cli.ftl` | `locales/en/cli.ftl`, `locales/th/cli.ftl` |

English is canonical in all three. Adding a new language is a folder/file drop — never a code change.

---

## Trust Boundaries

Where untrusted input enters and what the threat model addresses:

| Boundary | Threat category | Reference |
|---|---|---|
| User → CLI args | Command injection in `bwoc spawn` payloads | [`THREAT-MODEL.en.md`](../../modules/agent-template/docs/en/THREAT-MODEL.en.md) |
| CLI → Backend subprocess | Untrusted args propagating to LLM context | THREAT-MODEL §1 |
| Backend → `AGENTS.md` | Direct prompt injection | THREAT-MODEL §1.1 |
| Backend → file content read | Indirect prompt injection | THREAT-MODEL §1.2 |
| Agent → memory files | Social engineering via planted memory | THREAT-MODEL §1.3 |
| Agent ↔ Agent (Phase 3) | Capability spoofing | THREAT-MODEL §1.4 |

The baseline forbidden actions (Sīla 5) and craving-based threat categories (Taṇhā 3) are the doctrinal basis. See `SECURITY.md` for the reporting process.

---

## See Also

- [`PHILOSOPHY.en.md`](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) — conceptual framework stack and 22 framework mappings.
- [`PHILOSOPHY.en.md §0.1`](../../modules/agent-template/docs/en/PHILOSOPHY.en.md#01-the-arc--uppāda--ṭhiti--vaya) — the arc (uppāda · ṭhiti · vaya) that organizes CLI commands.
- [`GLOSSARY.en.md`](GLOSSARY.en.md) — fast-lookup for Pali terms used in this doc.
- [`THREAT-MODEL.en.md`](../../modules/agent-template/docs/en/THREAT-MODEL.en.md) — full threat model.
- `INCARNATION.en.md` (planned) — step-by-step agent creation.
- `ROADMAP.en.md` (planned) — phase timeline.
