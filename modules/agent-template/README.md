# Agent Base Profile — Template

The canonical template for creating BWOC-compliant AI coding agents. It carries **specification 3.0** (`| **Version** | 3.0 |` in `AGENTS.md`), which `bwoc check` validates and `bwoc migrate` moves older agents to.

[![Template](https://img.shields.io/badge/role-agent%20template-blue.svg)](../../README.md)
[![Spec](https://img.shields.io/badge/spec-3.0-green.svg)](../../docs/en/COMPATIBILITY.en.md)
[![Backend-neutral](https://img.shields.io/badge/backends-neutral-purple.svg)](neutrality.md)
[![Format](https://img.shields.io/badge/format-two--tier%20Markdown-lightgrey.svg)](../../CLAUDE.md)
[![Docs](https://img.shields.io/badge/docs-EN%20%7C%20TH-blue.svg)](docs/)

One copy per agent. Every backend reads the same `AGENTS.md`.

> Framework root: [`../../README.md`](../../README.md) · Philosophy: [`docs/en/PHILOSOPHY.en.md`](docs/en/PHILOSOPHY.en.md) · The Arc: [`PHILOSOPHY.en.md §0.1`](docs/en/PHILOSOPHY.en.md#01-the-arc--uppāda--ṭhiti--vaya)

---

## Incarnating a New Agent

```bash
bwoc new <name>          # interactive pickers; substitutes {{placeholders}}, creates backend symlinks
bwoc check agents/agent-<name>
```

Full walkthrough: [`INCARNATION.en.md`](../../docs/en/INCARNATION.en.md) · [`INCARNATION.th.md`](../../docs/th/INCARNATION.th.md).

## What's in the Template

| Path | Purpose |
|---|---|
| `AGENTS.md` | Agent instructions: the single source of truth, plain Markdown |
| `AGY.md` · `CODEX.md` · `KIMI.md` · `COPILOT.md` · `GROK.md` · `OLLAMA.md` · `OPENAI.md` | Symlinks → `AGENTS.md` |
| `CLAUDE.md` | A **regular file** in the template repo (guidance for editing the template itself). `bwoc new` replaces it with a symlink → `AGENTS.md` in the incarnated agent. |
| `config.manifest.json` | Placeholders + runtime config |
| [`neutrality.md`](neutrality.md) · [`conventions.md`](conventions.md) | Backend-neutrality rules · communal conventions |
| [`persona/`](persona/) · [`mindsets/`](mindsets/) · [`skills/`](skills/) | Identity, principles, capabilities (Obsidian tier-2 slots) — reference material; no backend loads them into the prompt |
| [`memories/`](memories/) | `MEMORY.md` index (≤ 200 lines) |
| [`interconnect/`](interconnect/) | [`capabilities`](interconnect/capabilities.md) · [`messaging`](interconnect/messaging.md) · [`routing`](interconnect/routing.md) · [`sangha`](interconnect/sangha.md) · [`trust`](interconnect/trust.md) |
| [`docs/`](docs/) | `en/` + `th/` spec pairs (OVERVIEW, PHILOSOPHY, PRD, SRS, THREAT-MODEL, SELF-IMPROVEMENT); memory and task-log examples; [persona example](docs/persona-example.good.md) and [anti-pattern](docs/persona-example.bad.md) |

## Rules and Reading Paths

- **Backend neutrality & adding a backend:** [`neutrality.md`](neutrality.md)
- **Non-negotiable rules** (worktrees, gates, memory cap, cleanup): [`AGENTS.md`](AGENTS.md)
- **Where to start reading:** [`docs/en/OVERVIEW.en.md`](docs/en/OVERVIEW.en.md)
- **Upgrading an older agent:** [`MIGRATION.en.md`](../../docs/en/MIGRATION.en.md)
