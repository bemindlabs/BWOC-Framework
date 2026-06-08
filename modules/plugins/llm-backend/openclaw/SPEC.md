---
title: "llm-backend: openclaw (inbound stub)"
tags:
  - group/framework-plugins
  - kind/llm-backend
  - status/stub
maturity: stub
---

# llm-backend — `openclaw` (inbound stub)

> [!abstract] The inbound half of the BWOC ↔ OpenClaw bridge. This stub declares the OpenClaw Gateway agent runtime as an additional spawnable `llm-backend`, so a future `bwoc spawn` can drive an agent that runs on OpenClaw. Runtime is deferred — the manifest registers intent; no model calls happen yet.

## Why this exists

The framework ships six first-class backends (`claude`, `antigravity`, `codex`, `kimi`, `ollama`, `openai-compatible`). OpenClaw is **not** one of them, so reaching it goes through the `llm-backend` plugin surface (see [`PLUGINS.en.md`](../../../docs/en/PLUGINS.en.md) §Plugin Kinds).

This is one direction of a **two-way bridge**:

| Direction | Where it lives | What it does |
|---|---|---|
| **Outbound** — BWOC → OpenClaw | `bemindlabs/bwoc-plugin-openclaw` (separate repo) | Registers the BWOC fleet into the OpenClaw Gateway as native tools + a memory slot that wrap the `bwoc` CLI. |
| **Inbound** — OpenClaw → BWOC | **this stub** | Registers OpenClaw as a backend `bwoc spawn` can target. |

## Status — stub

> [!warning] No runtime yet. The `entry` (`bwoc-llm-openclaw`) is not built; the framework refuses to load this plugin until the binary exists, so it stays *registered-but-disabled* (documented intent), exactly like the `audit-iso-*` stubs. Enabling it in `workspace.toml` before the runtime lands is rejected at startup.

## When the runtime lands

A future slice will provide the `bwoc-llm-openclaw` dispatch entry that translates the agent's harness calls into OpenClaw Gateway calls, and a `[config.schema]` for any gateway endpoint shape (credentials resolve at runtime from env / `.bwoc/secrets`, never committed). Until then this file is the contract placeholder.

## See also

- [`PLUGINS.en.md`](../../../docs/en/PLUGINS.en.md) — the plugin spec (`llm-backend` kind, lifecycle, loading).
- `modules/plugins/llm-backend/hermes/` — the sibling inbound stub for Hermes.
