---
title: "llm-backend: hermes (inbound stub)"
tags:
  - group/framework-plugins
  - kind/llm-backend
  - status/stub
maturity: stub
---

# llm-backend — `hermes` (inbound stub)

> [!abstract] The inbound half of the BWOC ↔ Hermes bridge. This stub declares the Hermes agent runtime (Nous Research) as an additional spawnable `llm-backend`, so a future `bwoc spawn` can drive an agent that runs on Hermes. Runtime is deferred — the manifest registers intent; no model calls happen yet.

## Why this exists

The framework ships six first-class backends (`claude`, `antigravity`, `codex`, `kimi`, `ollama`, `openai-compatible`). Hermes is **not** one of them, so reaching it must go through the `llm-backend` plugin surface (see [`PLUGINS.en.md`](../../../docs/en/PLUGINS.en.md) §Plugin Kinds).

This is one direction of a **two-way bridge**:

| Direction | Where it lives | What it does |
|---|---|---|
| **Outbound** — BWOC → Hermes | `bemindlabs/bwoc-plugin-hermes` (separate repo) | Packages the BWOC fleet as a Hermes plugin (tools / CLI command / memory provider that wrap the `bwoc` CLI). |
| **Inbound** — Hermes → BWOC | **this stub** | Registers Hermes as a backend `bwoc spawn` can target. |

## Status — stub

> [!warning] No runtime yet. The `entry` (`bwoc-llm-hermes`) is not built; the framework refuses to load this plugin until the binary exists, so it stays *registered-but-disabled* (documented intent), exactly like the `audit-iso-*` stubs. Enabling it in `workspace.toml` before the runtime lands is rejected at startup.

## When the runtime lands

A future slice will provide the `bwoc-llm-hermes` dispatch entry (a harness binary or sibling crate) that translates the agent's harness calls into Hermes API calls, and a `[config.schema]` for any endpoint/credential shape (credentials resolve at runtime from env / `.bwoc/secrets`, never committed). Until then this file is the contract placeholder.

## See also

- [`PLUGINS.en.md`](../../../docs/en/PLUGINS.en.md) — the plugin spec (`llm-backend` kind, lifecycle, loading).
- `modules/plugins/llm-backend/openclaw/` — the sibling inbound stub for OpenClaw.
