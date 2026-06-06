# 2026-06-06 — Harness autonomy & backend matrix (state + gaps)

Reference note consolidating a capability review of `bwoc-harness` as of the
**2.24.0 / `v2026.6.6-0`** release (Harness v3 batch HV3-1/2/3a/3b shipped).
Captures what "automatic" and "multi-backend" mean today, the deliberate
boundary, and what's left. Not a code change — a status/orientation note.

## Autonomy: auto-thinking / auto-working / auto-A2A

Humans instruct (a task, a shared task list, or a team); agents run
automatically **within the v1 safety envelope**.

| Capability | State | Mechanism |
|---|---|---|
| Auto-thinking | ✅ | Agentic loop reasons across turns (plan → tool → observe → repeat). Tier 2 memory wake-up (HV3-1) recalls prior sessions; unified context engine (HV3-2) compacts + spills to memory; `reasoningEffort` passthrough. Thinks *across* sessions. |
| Auto-working | ✅ | `bwoc run --task …` runs the full loop autonomously — tools through guardrails→permission→sandbox, checkpoints/resume, budget+cost gates, retrospective. `ask`-mode fails safe to **deny** in non-TTY, so autonomous runs use allow-listed `harness-policy.toml`. |
| Auto A2A / teams | ✅ (mostly) | `--lead` drains a shared `tasks.jsonl`, self-claims, spawns worker subprocesses (parallel, isolated worktrees). Team chat broadcast (HV3-3a), structured worker result envelopes (HV3-3b), peer-review gate (HV3-3c, in progress). A2A protocol: inbox + routing + MQTT federation, cross-machine. |

**The boundary is deliberate (Sīla), not a gap:** destructive ops hard-blocked;
`ask` fails safe; **self-modification lands only via human-gated PRs**
(autonomous self-modification is a stated v3 non-goal). "Instruct once → runs
automatically" ✅. "Instruct once → improves & drives itself hands-off forever"
= the remaining trifecta below.

## Multi-LLM (in the harness loop): ✅

- Provider trait, two impls: **OpenAI-compatible** (Ollama, LiteLLM, any
  OpenAI-compat endpoint) and **Anthropic native** (Messages API).
  `--backend ollama|openai-compatible|claude|anthropic`, `--model`, `--endpoint`.
- Fallback chain (primary + ordered fallbacks), token-pressure model switching,
  vetted-model allowlist.

## Multi AI Agent CLI: ✅ interactive · ⚠️ headless partial

A framework concern (not just the harness), at two levels:

| Level | State | How |
|---|---|---|
| Incarnate across backends | ✅ | `AGENTS.md` backend-neutral, symlinked to `CLAUDE.md`/`CODEX.md`/`KIMI.md`/`AGY.md`/`OLLAMA.md`. One spec, 5+ backends. |
| Interactive spawn (vendor CLI) | ✅ | `bwoc spawn` exec's **claude · agy · codex · kimi**; harness for ollama. |
| Headless / autonomous loop | ⚠️ | `bwoc run` + lead/worker run only on harness-native providers today = **Ollama (OpenAI-compat) + Anthropic**. Vendor CLIs (codex/kimi/agy) not yet wrapped non-interactively. |

The single gap = **HV3-6 vendor headless adapters** (wrap codex/kimi/agy as
non-interactive adapters so `bwoc run` covers all 6 backends; cross-backend CI
×5 — Samānattatā). Demand-driven, deferred.

## Remaining to reach the v3 Definition of Done

DoD: *a Saṅgha team completes a task where workers share context, a peer reviews
the diff before gates, everyone wakes with Tier-2 memory + mines on exit, and the
run ends with an eval-backed improvement PR — on ≥2 backends.*

- **HV3-3c** peer-review gate — *fixed reviewer per team* (decided); **in
  progress** (branch `feat/hv3-3c-peer-review-gate`: core `Team.reviewer` +
  `bwoc-harness::review` done, lead wiring + `--reviewer` flag + tests remaining).
- **HV3-4** self-improvement v2 — *policy+prompt draft-patch PRs, eval-gated*
  (decided); not built. Closes auto-thinking → auto-*better*.
- **HV3-5** live remote surface — *MCP-server-first, stdio* (decided); not built.
  Drive/instruct a running session from outside (incl. phone via RC).
- **HV3-6 / HV3-7** — vendor headless adapters / hardening; demand-driven.

## Related

- `notes/2026-06-05_harness-v3-plan.md` (workstreams + resolved decisions)
- `docs/en/HARNESS.en.md` (architecture + component table)
- HV3 PRs: #205 (HV3-1), #207 (HV3-2), #208 (HV3-3b), #209/#210/#211 (HV3-3a),
  #212 (release 2.24.0)
