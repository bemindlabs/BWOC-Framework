---
title: bwoc-harness — Self-Hosted Agent Runtime
aliases: [harness, ollama-harness, agentic-harness]
tags: [harness, runtime, safety, tools, ollama, self-host]
status: v1 (P1–P5 complete; see caveats)
canonical-source: crates/bwoc-harness/src/
parent: English
nav_order: 8
---

# bwoc-harness — Self-Hosted Agent Runtime

> [!abstract]
> `bwoc-harness` is a new crate that makes BWOC an **OpenAI-compatible model-API client and agentic loop runtime**, enabling self-hosted and provider-neutral LLM backends (Ollama first). The crate's heavy dependencies (tokio, reqwest, keyring) are quarantined inside it — `bwoc-cli`, `bwoc-agent`, and `bwoc-core` remain lean so the default `bwoc` path never pulls a runtime unless the user opts in.

See also: [[ARCHITECTURE.en.md]], [[PHILOSOPHY.en.md]], [[GLOSSARY.en.md]]

---

## What and Why

Before this crate, `bwoc spawn` worked by exec-ing a vendor agentic CLI (`claude`, `agy`, `codex`, `kimi`). That model has a fundamental gap: **Ollama has no agentic CLI**. To run a BWOC agent against a self-hosted model the framework must supply the agentic loop itself.

`bwoc-harness` closes that gap with three design commitments:

1. **Provider neutrality (Samānattatā)** — the harness speaks OpenAI-compatible `/v1/chat/completions` (tools + SSE streaming), not Ollama-native `/api/chat`. Any endpoint that speaks that dialect (Ollama, vLLM, LM Studio, llama.cpp server, or OpenAI itself) works without a code change.

2. **Dep-quarantine** — tokio, reqwest, keyring, and futures-util live only in `crates/bwoc-harness`. Users who never run a self-hosted backend never compile or link that weight. The "zero-dep orchestrator" promise holds for the default path.

3. **Safety-first (Sīla 5 + Taṇhā 3)** — the harness enforces a non-overridable guardrail layer before any tool executes. Denials are fed back to the model as tool results, keeping the loop alive rather than panicking it.

---

## Architecture

```
bwoc spawn --backend ollama
  │
  └─▶  bwoc-harness binary
         │
         ├─ load: AGENTS.md (system prompt) + persona + manifest + memory
         ├─ connect: OpenAI-compat endpoint (default: http://localhost:11434/v1)
         │
         └─ agentic loop (Iddhipāda 4 — engine of work)
              ┌──────────────────────────────────────────────────────────┐
              │  build messages (system + history + tool schemas)         │
              │  → POST /v1/chat/completions stream=true tools=[…]        │
              │  → accumulate SSE token deltas + tool_calls              │
              │  → for each tool_call:                                    │
              │      GUARDRAILS → PERMISSION → SANDBOX → execute          │
              │  → append assistant(tool_calls) + tool results            │
              │  → repeat                                                 │
              │                                                          │
              │  stop when: no tool_calls (final answer)                  │
              │           | max_iterations reached                        │
              │           | external cancel                               │
              │           | context overflow → compact history            │
              └──────────────────────────────────────────────────────────┘
              │
              └─ emit telemetry → session-metrics.jsonl
                   (in task mode) → bwoc task complete
```

### Crate layout

```
crates/bwoc-harness/src/
├── main.rs             — entry point: context load, loop launch
├── provider/
│   ├── mod.rs          — ProviderClient trait + types
│   ├── client.rs       — OpenAI-compat HTTP client (reqwest + SSE)
│   └── types.rs        — ChatMessage, ToolCall, ChatCompletion, …
├── agent_loop.rs       — turn loop, retry, fallback, compaction, telemetry
├── tools/
│   ├── mod.rs          — ToolContext, tool trait
│   ├── registry.rs     — ToolRegistry + dispatch
│   ├── impls.rs        — read_file, write_file, edit_file, list_dir, grep, …
│   ├── extra_tools.rs  — run_gates, bwoc_task, bwoc_send, memory_read/write
│   └── auth.rs         — CredentialBroker (P3)
├── policy/
│   ├── mod.rs          — run_pipeline: guardrails → permission → sandbox
│   ├── guardrails.rs   — hard safety rules (non-overridable)
│   └── permission.rs   — per-tool/per-pattern allow | ask | deny
├── sandbox.rs          — fs path confinement, env scrub, arg scan, OsSandbox trait
├── telemetry.rs        — per-turn metrics → session-metrics.jsonl (P3)
├── queue.rs            — async bounded cancellable task queue (P3)
└── eval/
    └── mod.rs          — offline fixture runner + rubric scorer (P4)
```

---

## The 8 Production Components

| Component | What it does | BWOC framework | Phase |
|---|---|---|---|
| **Safety guardrails** | Hard rules that run before permission and cannot be overridden. Blocks `rm -rf` repo root, secret writes, identity spoof, gate-bypass (`--no-verify`, `--force`), privilege escalation (`sudo`/`su`/`doas`). | Sīla 5 + Taṇhā 3 | P2 |
| **Permission system** | Per-tool / per-pattern `allow \| ask \| deny` loaded from `.bwoc/harness-policy.toml`. `ask` in non-TTY / autonomous mode falls back to `default_mode` (fail-safe: `deny`). Denials are fed back as tool results. | Taṇhā 3 (gate the cravings) | P2 |
| **Sandbox** | Confines all tool effects to the agent's worktree. Filesystem write allowlist (path-escape rejected via symlink resolution). `run_command` cwd locked to worktree root. Env scrub strips credential-like vars. Arg scan blocks `curl|sh`, privilege escalation, force-push. OS-level confinement is a v1 **stub trait** (see caveats). | Sīla 5 + Anattā (worktree isolation) | P2 |
| **Tool authentication** | OS keyring credential broker. Tools declare required creds (`CredentialRequest`); broker injects scoped vars into child-process env at exec time only — never in the prompt, never in telemetry, never logged. | Sīla (Adinnādāna) + Kalyāṇamitta | P3 |
| **Task queue** | Async, bounded, cancellable queue. Integrates with `bwoc-core::team` (Saṅgha shared task list). One task in flight per worktree; rollback to `pending` if the queue rejects after a claim. | Saṅgha + Padhāna 4 | P3 |
| **Streaming** | SSE token stream from the model. Delta-accumulates `content` and `tool_calls` fragments into a single `ChatMessage`. Wired in `agent_loop.rs` via `stream=true`. | Sammā-vācā (transparent speech) | P1 |
| **Telemetry** | Per-turn `TurnMetrics` (tokens in/out, latency, tool-call count, denial count, gate pass/fail, context tokens). Appended to `session-metrics.jsonl` per session. Additive to the `AGENTS.md §8b` schema — existing readers ignore the `"harness"` key. Optional OpenTelemetry export behind `--features otel`. | Satipaṭṭhāna 4 | P3 |
| **Eval framework** | Offline fixture runner. `task.toml` (prompt + rubric) + `seed/` (initial repo state) + `expected/` (expected outputs). Rubric scores: `file_contains`, `file_matches` (exact bytes), `gates_must_pass`. All tests use a mock provider — no live model or network required in CI. Feeds the Paññā 3 retrospective triggers in `session-metrics`. | Paññā 3 + Bhāvanā 4 | P4 |

---

## The Safety Pipeline

Every tool call passes through three sequential layers. **The order is fixed and non-negotiable.**

```
GUARDRAILS  (Sīla 5 + Taṇhā 3 — hard, non-overridable)
  ↓ pass
PERMISSION  (per-tool / per-pattern policy from harness-policy.toml)
  ↓ pass
SANDBOX     (worktree confinement + env scrub + arg scan)
  ↓ pass
  execute
```

A blocked call at any layer returns the blocking reason as the tool result message so the model can adapt. **The loop does not panic or stop on a denial.**

> [!warning]
> The pipeline is **fail-safe by default**. With no policy file present, `default_mode = "deny"`. An agent with no `.bwoc/harness-policy.toml` can read files but cannot write them or run commands unless the policy explicitly permits it.

### Guardrail rules

Each rule maps to a Sīla precept or Taṇhā root:

| Rule ID | Triggers on | Precept |
|---|---|---|
| `sila_panatatipata` | `rm -rf` targeting `/` or worktree root; `git clean -f*` | Pāṇātipāta (no destruction) |
| `sila_adinnadana` | Writing PEM keys, GitHub PATs, AWS keys, `password=`, `token=`, etc. to tracked files | Adinnādāna (no theft) |
| `sila_musavada` | `from`/`sender` field containing `spoof`/`impersonate`/`fake` in `bwoc_send` or `bwoc_task` | Musāvāda (no false speech) |
| `sila_surameraya` | `--no-verify` on any command; `git push --force`/`-f`/`--force-with-lease` | Surāmeraya (no heedlessness) |
| `bhava_tanha_escalation` | `sudo`, `su`, `doas` as the command binary | Bhava-taṇhā (privilege escalation) |

---

## The Tool Set

All tools are registered in `tools/registry.rs` and dispatched through the safety pipeline before execution. Every tool respects `ToolContext::workdir` for path resolution.

| Tool | Description |
|---|---|
| `read_file` | Read a file from the worktree |
| `write_file` | Write / overwrite a file |
| `edit_file` | Targeted string replacement (`old_string` → `new_string`) |
| `list_dir` | List directory contents |
| `grep` | Search file contents with a regex pattern |
| `run_command` | Run a shell command (sandboxed: cwd locked, env scrubbed, arg scanned) |
| `git` | Structured git operations (`subcommand` + `args` array) |
| `run_gates` | Run lint / fmt / test / build gates from the manifest |
| `bwoc_task` | Claim / complete tasks in the Saṅgha team list |
| `bwoc_send` | Send a message to another agent via `interconnect/` |
| `memory_read` | Read from the agent's `memories/` |
| `memory_write` | Write to the agent's `memories/` |

---

## `.bwoc/harness-policy.toml` Schema

Place this file in the agent's workspace root. The harness loads it at startup. If the file is absent, `default_mode = "deny"` applies (fail-safe).

```toml
# Global fallback mode for any tool or pattern not explicitly listed.
# Valid values: "allow" | "ask" | "deny"
# Default when absent: "deny" (fail-safe)
default_mode = "allow"

# Per-tool overrides. Key = exact tool name.
[tools]
read_file   = "allow"
list_dir    = "allow"
write_file  = "ask"     # prompts the operator on TTY; deny in non-TTY/autonomous
run_command = "deny"

# Pattern rules — matched against the full JSON arguments string.
# Rules are evaluated in declaration order; the first match wins.
[[patterns]]
pattern = "git push"
mode    = "deny"
reason  = "git push requires human review"

[[patterns]]
pattern = "cargo test"
mode    = "allow"
```

> [!note]
> `ask` mode in non-TTY or autonomous contexts (CI, background agent spawned by `bwoc spawn`) falls back to `default_mode`, which itself defaults to `deny`. This is intentional fail-safe behaviour.

---

## Backend Usage

### Spawn an Ollama agent

```bash
# Ensure an Ollama-compatible model is running locally.
# Then spawn the agent with the ollama backend:
bwoc spawn --backend ollama --path agents/my-agent
```

`bwoc spawn` detects the `ollama` backend and launches the `bwoc-harness` binary instead of a vendor CLI. The harness:

1. Reads `AGENTS.md` (via `OLLAMA.md → AGENTS.md` symlink) as the system prompt.
2. Reads `config.manifest.json` for the model name and `context_limit`.
3. Connects to `http://localhost:11434/v1` (or `$OLLAMA_BASE_URL` if set).
4. Validates that the model exists on the Ollama instance before the first turn.
5. Runs the agentic loop.

### Spawn an OpenAI-compatible agent

```bash
# Any OpenAI-compatible endpoint (vLLM, LM Studio, llama.cpp server, remote):
bwoc spawn --backend openai-compatible --path agents/my-agent
```

Set `"baseUrl"` in the agent's `config.manifest.json` to the endpoint — **required** for `openai-compatible` (`ollama` defaults to `http://localhost:11434/v1` and treats `baseUrl` as optional). `bwoc spawn` passes it to the harness `--endpoint`. Register the backend with the `OPENAI.md → AGENTS.md` symlink; the provider client is unchanged (same OpenAI-compatible `/v1/chat/completions` path).

### Vetted-model enforcement

`--vetted-mode off | warn | enforce` (default `warn`) controls how the loop treats a model **not** in the `vetted_models` allowlist:

- `off` — no check.
- `warn` — log a warning and proceed (historical behaviour; backward-compatible default).
- `enforce` — refuse to run an unvetted **primary** model (error before the first turn).

An empty `vetted_models` allowlist means no restriction regardless of mode.

### Add the OLLAMA.md symlink to an existing agent

```bash
cd agents/my-agent
ln -s AGENTS.md OLLAMA.md
```

No other change required. The harness reads the same `AGENTS.md` every other backend reads.

### Model configuration in `config.manifest.json`

```json
{
  "model": "gemma4",
  "fallbackModel": "qwen2.5-coder:7b",
  "contextLimit": 8192
}
```

`fallbackModel` is tried if the primary model produces malformed tool calls more than twice in a row. `contextLimit` triggers history compaction (truncate-with-marker strategy) when the estimated context token count approaches the limit.

---

## Dep-Quarantine Design

> [!tip]
> This is the structural guarantee that makes `bwoc-harness` optional, not mandatory.

```
crates/bwoc-core    — lean: serde, serde_json, toml, thiserror only
crates/bwoc-cli     — lean: clap + bwoc-core + ratatui; no tokio, no HTTP
crates/bwoc-agent   — lean: bwoc-core + fluent-bundle; no tokio
crates/bwoc-harness — heavy: tokio, reqwest, futures-util, keyring, async-trait
```

`bwoc-harness` depends on `bwoc-core` (lean data types) but `bwoc-core`, `bwoc-cli`, and `bwoc-agent` do **not** depend on `bwoc-harness`. A user who only runs `bwoc spawn --backend claude` never compiles or links the harness.

This preserves the VISION "zero-dep orchestrator" identity for the default path while enabling production-grade self-hosted operation via an opt-in crate.

---

## Live Validation Result (2026-05-23)

The harness was validated end-to-end against a real Ollama instance before the docs were written.

**Model: `gemma4:latest` (8B)**

- Turn 1: model called `read_file` (read-before-edit emerged on its own, not instructed).
- Turn 2: model called `write_file` with correct Python code and valid Thai Unicode (`สวัสดี, Pi`).
- Turn 3: model gave final answer.
- Running `greet("Pi")` on the output returned `สวัสดี, Pi`. The file was valid and executed correctly.

**With no policy file (fail-safe deny):**

- Write was correctly denied.
- Denial reason was fed back to the model as the tool result.
- The model adapted and gave a final answer explaining why it could not complete the task.

**With a permissive `.bwoc/harness-policy.toml` (`default_mode = "allow"`):**

- The write succeeded end-to-end.

**Model: `llama3.2:3b`**

- The mechanism ran correctly (tool calls were dispatched) but the model garbled the Unicode output, skipped the read, and broke the file structure.
- This confirmed the vetted-model gate design: small models should not be used for code edits without validation.

---

## Not Yet / v1 Caveats

> [!warning]
> Be accurate about what is and is not production-ready.

| Capability | Status |
|---|---|
| **OS-level sandbox** (macOS `sandbox-exec`, Linux landlock/seccomp) | **Stub.** The `OsSandbox` trait exists and is pluggable, but the only implementation is `NoopOsSandbox`. Worktree+allowlist confinement is active; OS-level syscall isolation is not. |
| **Streaming** | Wired and functional (SSE delta accumulation tested). Usage token counts are not available on the streaming path (the provider does not return `usage` in SSE deltas). |
| **Vetted-model list** | Small. Currently only `gemma4` and `qwen2.5-coder:7b` are known-good for tool calling. Unvetted models emit a warning but are not hard-blocked. |
| **Context compaction** | Active (truncate-with-marker strategy). LLM-summarise is the natural upgrade path but is not implemented in v1. |
| **Tool authentication broker** | Implemented (P3) but not wired into every tool by default. Tools that need OS keyring credentials must declare `CredentialRequest` explicitly. |
| **Concurrent tool execution** | Sequential in P1/P2. Parallel tool dispatch is a P3 item. |
| **Identity spoofing detection** | Conservative: only fires when the `from`/`sender` field literally contains the words `spoof`, `impersonate`, or `fake`. A proper agent-identity proof system is a v2 item (trust step 5 in the roadmap). |
| **Platform support** | **Unix-first in v1** (macOS + Linux). The crate *builds* on Windows, but its tool layer shells out to a POSIX shell and the sandbox / `run_command` tests assume Unix commands (`pwd`, `rm -rf`, …), so the harness is **not tested on Windows** — CI excludes `bwoc-harness` on the Windows job. Windows support is a tracked follow-up; the rest of the toolkit remains fully cross-platform. |

---

## BWOC Framework Mappings

The design maps each component to one or more of the 22 Buddhist frameworks in [[PHILOSOPHY.en.md]]:

| Component | Framework | Why |
|---|---|---|
| Safety guardrails | Sīla 5 | The five precepts become non-negotiable code constraints |
| Permission system | Taṇhā 3 | Permission gates intercept the three roots of craving (kāma, bhava, vibhava) before they become tool calls |
| Sandbox confinement | Anattā + Sīla 1 | No action persists beyond the worktree; the worktree is the agent's conditioned boundary |
| Denial-as-tool-result | Brahmavihāra 4 (Karuṇā) | Surfacing the reason gives the model the information to adapt, rather than silently failing |
| Agentic loop | Iddhipāda 4 | The four bases of power (Chanda, Viriya, Citta, Vīmaṃsā) map to: goal-setting, retry effort, model call, and rubric scoring |
| Telemetry | Satipaṭṭhāna 4 | The four foundations of mindfulness apply to the harness's own operation (body=process, sensation=I/O, mind=tool calls, dhamma=denials/gates) |
| Eval framework | Paññā 3 + Bhāvanā 4 | Offline fixtures feed the three wisdom practices (sutamayā, cintāmayā, bhāvanāmayā) and the four right efforts |
| Task queue | Saṅgha + Padhāna 4 | The queue integrates with the shared task list of the agent team (Saṅgha) and enforces right effort in scheduling |
| Backend neutrality | Samānattatā | Any OpenAI-compatible endpoint is treated identically; no provider is favoured |

---

## See Also

- [[ARCHITECTURE.en.md]] — where `bwoc-harness` fits in the implementation stack
- [[PHILOSOPHY.en.md]] — the 22 BWOC frameworks referenced above
- [[GLOSSARY.en.md]] — Pali term lookup
- `crates/bwoc-harness/src/agent_loop.rs` — annotated loop implementation
- `crates/bwoc-harness/src/policy/guardrails.rs` — guardrail rule implementations and tests
- `notes/2026-05-23_ollama-agentic-harness-design.md` — architecture decisions before implementation
