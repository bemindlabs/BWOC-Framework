# `modules/plugins/` — Framework Plugins

**Status:** planned. No plugins shipped yet.

## What plugins are

Plugins extend the framework with capabilities that **do not belong in every agent** but should be available to agents that need them. They are distinct from:

- **Agent skills** (`modules/agent-template/skills/`) — capabilities an individual agent declares.
- **Framework skills** (`modules/skills/`) — the recommended baseline set of skills any agent can opt into.
- **Backend integrations** — the six declared backends (Claude, Antigravity, Codex, Kimi, Ollama, OpenAI-compatible) are first-class and live in spec, not as plugins.

## What plugins might be

Concrete examples that fit the plugin shape:

- **Tier 2 memory backends** — vector stores, semantic search, deep-memory CLIs that agents can configure via `{{deepMemoryCmd}}` in `config.manifest.json`.
- **Additional LLM-backend integrations** beyond the four declared — opt-in for agents that need them; framework remains backend-neutral by treating the four as canonical.
- **Workflow integrations** — issue trackers, code review tools, CI providers an agent might interact with.

## What plugins are NOT

- A loophole to add per-vendor logic to the framework spec. Vendor-specific phrasing in `AGENTS.md` is still forbidden (Samānattatā).
- A place for one-off scripts. Those belong with the agent that uses them.

## Spec status

The plugin loading mechanism, manifest, and lifecycle hooks are not yet specified. The first plugin lands together with its spec.

## See Also

- [`modules/README.md`](../README.md)
- [`docs/en/ARCHITECTURE.en.md`](../../docs/en/ARCHITECTURE.en.md)
- [`docs/en/ROADMAP.en.md`](../../docs/en/ROADMAP.en.md)
