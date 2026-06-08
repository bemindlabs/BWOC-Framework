# 2026-06-08 — Inbound llm-backend stubs: hermes + openclaw

Added two `llm-backend` plugin **stubs** under `modules/plugins/llm-backend/` — `hermes` and `openclaw` — completing the inbound direction of the BWOC ↔ host bridge for the two hosts that are **not** first-class backends.

## What changed
- `modules/plugins/llm-backend/hermes/{manifest.toml,SPEC.md}`
- `modules/plugins/llm-backend/openclaw/{manifest.toml,SPEC.md}`

Each declares `kind = "llm-backend"`, a placeholder `entry` (`bwoc-llm-<host>`, not yet built), and no `[config.schema]`. They are registered-but-not-loadable until the dispatch binary lands — same posture as the `audit-iso-*` stubs.

## Decisions
- **Why `llm-backend`, not `workflow`.** Hermes and OpenClaw are agent/LLM *runtimes* BWOC would spawn into, not outward integrations BWOC calls. `llm-backend` is the declared kind for "backends beyond the six first-class" (PLUGINS.en.md §Plugin Kinds).
- **Why a stub now.** The outbound adapters already ship (`bemindlabs/bwoc-plugin-{hermes,openclaw}`); these stubs record the inbound contract + intent so the bidirectional bridge is documented in-tree before the runtime is built. claude/codex/agy need no inbound plugin — they are already first-class backends `bwoc spawn` can target.
- **Layout.** Grouped under `modules/plugins/llm-backend/<name>/`, mirroring the existing `workflow/gcloud-*`, `gws/gws-*`, `council/*` kind-grouping convention.

## Status / deferred
- No runtime. `entry` binaries `bwoc-llm-hermes` / `bwoc-llm-openclaw` are future slices (harness/crate that maps the agent harness onto each host's API; `[config.schema]` + credential shape added then).

## Related
- `docs/en/PLUGINS.en.md` — plugin spec.
- Outbound adapters: `bemindlabs/bwoc-plugin-hermes`, `bemindlabs/bwoc-plugin-openclaw`.
