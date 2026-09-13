# How-To: Configure Backends

## Goal

Understand the per-agent backend choice and how to switch it.

## Prerequisites

- A workspace with at least one incarnated agent (see [`first-agent.md`](first-agent.md))
- The backend CLI you want to switch to is installed and on PATH

## Background

BWOC supports ten declared backends (Samānattatā — equal treatment, no vendor lock-in):

| Backend | CLI binary | Common models |
|---|---|---|
| Claude | `claude` | `claude-opus-4-8`, `claude-sonnet-4-6`, `claude-haiku-4-5` |
| Antigravity | `agy` | `gemini-3.5-flash-medium`, `gemini-3.1-pro-high`, `claude-sonnet-4.6-thinking`, `gpt-oss-120b-medium` |
| Codex | `codex` | `gpt-5.5`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.3-codex` |
| Kimi | `kimi` | `kimi-k2`, `kimi-k1.5` |
| Ollama | `bwoc-harness` | `qwen2.5-coder:7b`, `llama3.1:8b`, `mistral-nemo`, `gemma4:8b` |
| OpenAI-compatible | `bwoc-harness` | `gpt-5.5`, `gpt-5.5-pro`, `gpt-5.4`, `gpt-5.4-mini` |
| Copilot | `copilot` | see the `bwoc new` picker |
| Grok | `grok` | `grok-4.5`, `grok-code-fast-1`, `grok-build` |
| OpenRouter | `bwoc-harness` | any OpenRouter model id (key: `OPENROUTER_API_KEY`) |
| LiteLLM | `bwoc-harness` | `gpt-5.5`, `claude-opus-4-8`, `gemini-2.5-pro` (base: `LITELLM_API_BASE`) |

Each agent picks **one** backend at incarnation time, recorded in its `config.manifest.json` (`primaryModel` + optional `fallbackModel`) and in the workspace's `.bwoc/agents.toml`.

## Steps

### Option A — set the backend when you create the agent

```bash
bwoc new my-agent --backend agy --primary-model gemini-3.5-flash-medium
```

Or pass `--backend agy` and let the interactive picker show you Antigravity's models.

### Option B — change an existing agent's backend

```bash
bwoc set my-agent --backend agy --primary-model gemini-3.5-flash-medium
```

`bwoc set` rewrites the `.bwoc/agents.toml` entry and `primaryModel` / `fallbackModel` in `config.manifest.json`; the backend symlinks already exist. Verify:

```bash
bwoc check agents/my-agent     # should still pass
bwoc list                      # should show the new backend
```

### Option C — spawn against a different backend without changing the manifest

`bwoc spawn` takes `--backend` directly, overriding the agent's recorded choice for one session:

```bash
bwoc spawn --path agents/my-agent --backend kimi
```

Useful for cross-backend testing — verifying an agent's `AGENTS.md` is genuinely backend-neutral.

## Verify

```bash
bwoc check agents/my-agent
```

Should print `Neutrality check passed.` regardless of which backend you switch to — if it doesn't, your manifest has backend-specific content that should be moved into persona or memory.

## Caveats

- All ten backends read **the same `AGENTS.md`** — via entry-file symlinks (`CLAUDE.md`, `AGY.md`, `CODEX.md`, `KIMI.md`, `COPILOT.md`, `GROK.md`, `OLLAMA.md`, `OPENAI.md`), and Copilot and Grok also read `AGENTS.md` natively. If your agent's instructions assume a specific backend, `bwoc check` will flag it as a neutrality violation.
- Model identifiers in the picker are a convenience catalog, not a whitelist — type any model name and it's accepted as-is.

## What's next

- [`first-agent.md`](first-agent.md) — full incarnation walkthrough
- [`docs/en/PHILOSOPHY.en.md` §Samānattatā](../../modules/agent-template/docs/en/PHILOSOPHY.en.md) — the principle behind backend neutrality
- `crates/bwoc-cli/src/spawn.rs::Backend::models()` — the source of truth for the picker catalog
