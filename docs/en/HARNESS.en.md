---
title: bwoc-harness — Self-Hosted Agent Runtime
aliases: [harness, ollama-harness, agentic-harness]
tags: [harness, runtime, safety, tools, ollama, self-host]
status: shipped (see caveats)
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

## Quick start: `bwoc` in any repository

Run `bwoc` with no arguments, in a terminal, in any directory. It opens the chat TUI on a coding session there, with no workspace, registered agent or manifest. Outside a terminal (a pipe, a script, CI) bare `bwoc` still prints the banner exactly as before, and `bwoc about` prints it on a terminal.

**Providers and keys.** The chat TUI runs on API keys or local models:

| Backend | Key |
|---|---|
| `anthropic` | `bwoc auth set anthropic`, or `ANTHROPIC_API_KEY` |
| `openrouter` | `bwoc auth set openrouter`, or `OPENROUTER_API_KEY` |
| `litellm` | optional: `bwoc auth set litellm`, or `LITELLM_API_KEY` |
| `openai-compatible` | optional: `bwoc auth set openai-compatible` (no env var, so a generic key is never sent to an arbitrary endpoint) |
| `ollama` | none |

**Subscription CLIs.** With a vendor-CLI backend (`claude`, `codex`, `agy`, `kimi`, `grok`, `copilot`), bare `bwoc` execs that CLI in the current directory, forwarding `--model` when one is set. The CLI runs on its own login, so a Claude Code or Codex subscription needs no API key. It also runs its own tools and permission prompts, so bwoc-harness tools, the capability gate and trust gates do not apply; `bwoc` prints a one-line notice saying so before handing over the terminal. `endpoint`, `max_tokens` and `max_context` are ignored for these backends. `cli` is not a session backend; name the vendor CLI instead.

`bwoc auth set <provider>` reads the key from stdin (hidden on a terminal) or from `--from-env VAR`, and writes `[<provider>] api_key` into `~/.bwoc/secrets.toml`, creating the file `0600`. It refuses to write into a group- or world-readable file (run `chmod 600` first) and keeps every other section. `bwoc auth status` lists which providers have a key and where it comes from. It never prints a key or its length.

**Choosing the runtime**, highest precedence first:

1. flags: `bwoc --backend … --model … --endpoint …`
2. env: `BWOC_BACKEND`, `BWOC_MODEL`, `BWOC_ENDPOINT`
3. `.bwoc/config.toml` in the directory, or an ancestor up to the git root
4. `~/.bwoc/config.toml`
5. auto-detect: an Anthropic key selects `anthropic`; otherwise an Ollama answering on `localhost:11434` selects `ollama` and its first model; otherwise `bwoc` prints setup help, naming any vendor CLI found on `PATH`, and exits `2`. A vendor CLI is never picked automatically.

A layer that names a different backend from the winning one contributes nothing else, so a model written for one provider is never sent to another.

```toml
schema_version = 3

[runtime]
backend    = "ollama"
model      = "<model>"
endpoint   = "http://localhost:11434/v1"   # optional
max_tokens = 8192                          # optional
max_context = 32768                        # optional: model context window
```

When a file has no `[runtime] backend`, its `[defaults] backend` (the fleet default for new agents) is used instead. An absent `schema_version` is read as legacy; a newer one is refused with an error naming `schema_version`. Unknown keys are ignored.

**What the session puts in its system prompt**, in order:

1. A short built-in coding-agent preamble: the harness's tools, investigate → act → verify, and ask before destructive actions.
2. An environment block: working directory, OS/arch, UTC date, and git state (branch, clean or uncommitted changes). A missing or hung `git` only removes detail.
3. Project instructions: `AGENTS.md` (else `CLAUDE.md`) from each directory between the git root and the working directory, root first and nearest last, capped at 32 KB with the farthest directories cut first.

Conversations are saved per directory under `~/.bwoc/sessions/`, never inside the repository. A directory can hold several: bare `bwoc` resumes the one written most recently, `bwoc --new` starts another, and `bwoc --session <id>` (an id or unique prefix) resumes a specific one. `bwoc session list` shows this directory's conversations (`--all` for every directory, `--json` for scripts), `bwoc session fork [<id>]` copies one into a new session (which bare `bwoc` then resumes; the original stays as it was), and `bwoc session rm <id>` deletes one. A 3.2 conversation moves into the new layout the first time its directory is opened. File tools stay confined to the working directory, and writes, edits and commands ask first (the chat default policy) unless `.bwoc/harness-policy.toml` says otherwise.

**While a turn runs**, `Esc` cancels it: the harness finishes the tool call in flight so the conversation stays well-formed, then ends the turn (`Cancel` on the wire). Between turns `Esc` does nothing. `/model` shows the model and `/model <name>` switches the one later turns use. When a tool writes, edits or multi-edits a file inside the working directory, the session shows a unified diff of that file (capped at 8 KB, marked when cut) — display-only, since the model already has the tool result. The status line shows a session cost only when the provider reports one (OpenRouter does); nothing is estimated from a local price table.

**In the input line**, `/` opens a command menu and `@` completes project files; `↑`/`↓` pick, `Tab` completes, `Esc` hides the menu. `/sessions` lists this directory's conversations (the open one marked), `/session <id>` opens another, `/new` starts one and `/fork [<id>]` copies one and opens the copy — all without leaving the TUI; the harness is reopened on the chosen conversation in the same terminal. `/undo` takes back the file changes of the last turn that made any and `/redo` reapplies them — an edit journal beside the conversation, text files up to 1 MB; a file changed since (by `run_command`, or by you) is reported and left alone rather than overwritten, and a binary or oversized write is reported as not journalled. `/help` lists the commands, `/clear` forgets this conversation (deleting its saved history, like the fleet palette's Forget), `/mode [default|accept_edits|bypass]` shows or sets the permission mode (as `F2` does), and `/quit` or `/exit` ends the session. The TUI handles these itself and sends nothing to the model; an unknown `/name` is reported rather than sent, and a line whose first word is a path such as `/etc/hosts` goes out as a message. On send, each `@path` naming a text file inside the working directory attaches that file's content to the message (up to 32 KB per file, marked when truncated); a path outside the directory or a binary file is reported and not attached, and an `@name` that isn't a file stays plain text.

**A session runs the batch paths.** Provider calls retry transient errors, and repeated malformed tool calls move to the next `autoModels` fallback. `--mcp` / `--mcp-http` servers register as in `bwoc run`. Every tool call passes the capability gate, guardrails and permission policy, then runs through the turn executor and OS sandbox — except the three chat-only tools below (`webfetch`, `todo`, `subagent`), which run in-process after the same checks. Once untrusted content (a tool output, a connector message) is in the conversation, a gated call such as `run_command` needs an explicit Allow, even in bypass mode; a `--headless` session denies it, as batch does. Compaction is sized from the model's context window: `max_context` (`--max-context`), else what the provider reports, else 200k tokens for `anthropic`, else 8,000 tokens. Reasoning text streamed by the provider shows as a dimmed line that collapses when the answer starts. An assistant turn is rendered as Markdown in the TUI — headings, list bullets, block quotes, fenced and inline code, bold and italic; anything else, including every tool result, is shown verbatim.

**Agent sessions are unchanged.** When the workdir has `config.manifest.json` (`bwoc chat <agent>`), the prompt is still the agent's `AGENTS.md` plus its `MEMORY.md` index, now followed by a condensed persona and mindsets block: the `persona/README.md` body and each mindset's title and first paragraph, capped at 8 KB. A bwoc-connect public workdir (`.bwoc/public/…`) never reads instructions from above itself.

---

## Architecture

```
bwoc spawn --backend ollama
  │
  └─▶  bwoc-harness binary
         │
         ├─ load: AGENTS.md (or CLAUDE.md) + MEMORY.md index + optional Tier-2 wake-up
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
| **Worker result envelope** | A Saṅgha worker writes a structured outcome to its worktree (`.bwoc/worker-result.json`: turns, active model, a diff summary of working-tree changes, and bounded result text) in place of a bare exit code; the lead reads it before teardown and logs a one-line summary. Best-effort — a worker that writes none degrades to the exit code. | Saṅgha + Kalyāṇamitta | P3 |
| **Peer-review gate** | With `--lead --reviewer <agent>` (or a team's `reviewer` field), each successful worker's diff is routed to the reviewer agent (a `bwoc-harness` run in the worktree) before completion: APPROVE → complete; REJECT → re-queue, keeping the worktree + feedback. Fail-safe — a spawn/timeout/unparseable-verdict rejects; self-review is skipped. | Saṅgha + Kalyāṇamitta | P3 |
| **Team chat broadcast** | A `--chat --team-chat <path>` session shares a team's append-only `chat.jsonl`: teammate messages posted since the last turn are injected as a "Team conversation" system note before each turn, and the agent's reply is appended after. Reachable as `bwoc chat <agent> --tui --team <id>` (membership-checked); peer messages also surface to the TUI as `📢` lines via a `TeamMessage` event. Opt-in — no flag = solo session; an agent never sees its own messages echoed. | Saṅgha + Kalyāṇamitta | P3 |
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
| `edit_file` | Targeted string replacement (`old_string` → `new_string`); `replace_all` replaces every exact occurrence |
| `multi_edit` | An ordered list of `edit_file` replacements on one file, written only if every edit succeeds |
| `list_dir` | List directory contents |
| `grep` | Search file contents with a regex; `fixed_strings`, `case_insensitive` and a `glob` file filter. Binary files and hidden directories are skipped; an invalid regex falls back to a literal search |
| `glob` | Find files by glob (`*`, `**`, `?`, `[..]`, `{a,b}`). Read-only; hidden directories skipped, `.gitignore` not read |
| `run_command` | Run a shell command (sandboxed: cwd locked, env scrubbed, arg scanned). `timeout_secs` (default 120, max 600) kills the command's process group |
| `git` | Structured git operations (`subcommand` + `args` array) |
| `run_gates` | Run lint / fmt / test / build gates from the manifest |
| `bwoc_task` | Claim / complete tasks in the Saṅgha team list |
| `bwoc_send` | Send a message to another agent via `interconnect/` |
| `memory_read` | Read from the agent's `memories/` |
| `memory_write` | Write to the agent's `memories/` |
| `memory_search` | Semantic search over the Tier 2 deep-memory store (registered only when the manifest configures `deepMemoryCmd`; read-only) |

`--chat` and `--headless` sessions also register three tools that are not in `default_registry`. They run in-process and are never sent to the turn-executor child:

| Tool | Description | Chat default policy | Plan mode |
|---|---|---|---|
| `webfetch` | GET an http(s) URL and return text (HTML converted). 30 s timeout, 1 MB body, at most 5 redirects; localhost and loopback, private, link-local and CGNAT addresses are refused, including on redirect | `ask` (network egress) | blocked |
| `todo` | The session's task list, held in memory (`read` / `write`) | `allow` | blocked |
| `subagent` | A read-only child session: same provider and model, fresh context, `read_file` / `list_dir` / `grep` / `glob` only, at most 15 model calls, cannot start another subagent. Returns its final answer | `allow` | blocked |

`webfetch`, `todo` and `subagent` stay out of plan mode: public connector sessions run in plan mode, and every plan-mode tool must pass the capability gate on an untrusted turn, which only `PURE_READ_TOOLS` do. There is no `apply_patch`: `multi_edit` covers multi-site edits without a patch grammar.

---

## Tier 2 Deep Memory (HV3-1)

When the agent's manifest configures `deepMemoryCmd` (any tool speaking the `bwoc-core::deep_memory` contract — the reference is `bwoc-deep-memory`), the harness closes the memory loop around every session:

1. **wake-up** — at session start (batch and `--chat`), `<cmd> wake-up` output is appended to the system prompt as a *"Prior context (Tier 2 memory)"* block.
2. **memory_search** — a read-only tool (`<cmd> search "<q>"`) is registered so the model can recall past decisions mid-run; like every tool it flows through the guardrails → permission pipeline. Chat's default policy allows it alongside `memory_read`.
3. **mine** — at session end the session becomes memory: `--chat` mines the persisted `.bwoc/chat-session.json`; a successful batch run distils *task → outcome* into `.bwoc/last-run.md` and mines that (the checkpoint is already cleaned up on success); a failed run mines its surviving checkpoint (failed runs are exactly what's worth remembering).

Everything is **opt-in, best-effort, and bounded**: an absent/placeholder `deepMemoryCmd` disables all three (Tier 1 keeps working); failures degrade to warnings; every subprocess call carries a timeout (wake-up 10 s, search 15 s, mine 60 s) so a hung memory backend can never stall a run. *(Sati — the agent remembers across sessions.)*

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
2. Reads `config.manifest.json` for the model name. (`context_limit` is not a manifest field: the harness hardcodes it to `0`, i.e. no compaction, and only `auto` model selection supplies per-model limits.)
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
  "primaryModel": "gemma4",
  "fallbackModel": "qwen2.5-coder:7b"
}
```

`fallbackModel` is metadata only — the harness never reads it. The fallback chain tried after repeated malformed tool calls comes from `autoModels` when `primaryModel` is `"auto"`. (History compaction and per-model context limits live on the harness `LoopConfig`, not as a `config.manifest.json` field.)

For OpenAI-compatible endpoints serving GPT-5.5, prefer an explicit model or
runtime selection pool:

```json
{
  "backend": "openai-compatible",
  "baseUrl": "https://api.openai.com/v1",
  "primaryModel": "auto",
  "autoModels": ["gpt-5.5", "gpt-5.5-pro", "gpt-5.4", "gpt-5.4-mini"],
  "reasoningEffort": "medium",
  "maxTokens": 32000,
  "promptCache": true,
  "thinking": false
}
```

`primaryModel: "auto"` keeps BWOC backend-neutral while letting the harness
choose from models the live provider actually serves. Put the highest-capability
model first and cheaper/lower-latency fallbacks later; the resolver uses that
order as its cost axis after availability and context-fit checks.

OpenAI recommends GPT-5.5 for reasoning-heavy coding and agent workflows, with
`medium` reasoning effort as the balanced starting point and lower effort
evaluated before disabling reasoning. `reasoningEffort` is optional; when set,
the harness sends it as `reasoning_effort` on OpenAI-compatible completion
requests **and** as `output_config.effort` on the native Claude path, so effort
now reaches every HTTP backend that supports it. `maxTokens` is likewise
optional: Claude's Messages API requires `max_tokens`, so this overrides the
harness default there; OpenAI-compatible backends send it only when set and
otherwise fall back to the provider default. `promptCache` is **on by default**
(`false` opts out): the native Claude path marks the stable system-prompt prefix
with `cache_control`, so an agentic loop that resends it pays cache-read (~0.1×)
instead of full input — the tools block, rendered before system, is covered by
the same breakpoint. Below the provider's minimum cacheable size the marker is a
silent no-op. `thinking` is **off by default** (opt in with `true`): the native
Claude path then requests adaptive extended thinking on both non-streaming and
streaming completions and preserves + replays the returned thinking blocks across
turns — required for the tool path, since the Messages API rejects a `tool_result`
whose preceding thinking block was dropped. On the streaming path the thinking
blocks (with their signature) are reassembled from the SSE deltas and carried on
the accumulated assistant message, so replay works identically. **Multimodal
image input** is provider-neutral: a `ChatMessage` may carry base64 images
(`with_images`), rendered as Anthropic `image` blocks on the native path and as
OpenAI `image_url` data-URI parts on the OpenAI-compat path — text-only messages
are byte-for-byte unchanged. (Wiring a captured `computer` screenshot
*through the re-exec turn-executor boundary* into that field is a separate,
bemind-verified follow-up.) The current BWOC harness still speaks the
OpenAI-compatible `/v1/chat/completions` surface so it can also run Ollama and
other compatible providers. A native Responses API adapter is the right next step for full
GPT-5.5 reasoning controls; until then, keep `AGENTS.md` outcome-first, avoid
process-heavy prompt scaffolding, and make completion criteria explicit.

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
| **OS-level sandbox** (macOS `sandbox-exec`, Linux landlock/seccomp) | **Shipped (2.3.0).** Real landlock (Linux ≥ 5.13) + `sandbox-exec` (macOS) via `make_os_sandbox()`, degrading gracefully to worktree-only confinement on unsupported kernels. *(This row previously claimed "stub" — stale since 2.3.0.)* |
| **Streaming** | Wired and functional (SSE delta accumulation tested). Usage token counts are not available on the streaming path (the provider does not return `usage` in SSE deltas). |
| **Vetted-model list** | Small. Currently only `gemma4` and `qwen2.5-coder:7b` are known-good for tool calling. Unvetted models emit a warning but are not hard-blocked. |
| **Context compaction** | **Unified engine (HV3-2).** Summarize-first via one provider call, truncate-with-marker fallback on summarizer failure — one policy for the batch loop and `--chat`. Folded content is mined into Tier 2 when `deepMemoryCmd` is configured. |
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
