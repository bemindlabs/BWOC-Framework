# 2026-06-19 — Design: warm / served agent mode (issue #301)

**Status: approved 2026-06-19. PR A in progress.**

## Resolved decisions (architect, 2026-06-19)

- **Flag name:** `--headless` (not `--warm`/`--served`).
- **Spawn/idle (PR B):** lazy spawn on first routable message + idle-exit after N
  idle minutes (frees model GPU/RAM), respawn on next message.
- **Result delivery (PR B):** inbox receipt, reusing the #299 receipt path.
- **Approved to build PR A** (the standalone `--headless` loop).

## Problem

Agents cold-start a backend process **per task/message**: `bwoc run` builds a
one-shot command and spawns a fresh child every time
(`crates/bwoc-cli/src/run.rs:155-160`, `build_command()` at `:249-363`). For
harness backends that means a new `bwoc-harness` subprocess per message — paying
provider re-connect, tool-registry init, system-prompt + memory reload, and
model warm-up on every turn. The issue (and the HTTP-gateway-over-`bwoc run`
work) both hit this: "per message = slow / expensive cold start."

## Key realization — the warm primitive already exists

`bwoc-harness --chat` is already a resident, multi-turn loop
(`crates/bwoc-harness/src/chat_session.rs`): one process serves successive
messages over a JSON-line protocol (`bwoc_core::chat_proto`), keeping the
provider client, tool registry, and conversation **resident** across turns, and
persisting the conversation atomically to `.bwoc/chat-session.json`
(`save_session`/`load_session`, `:386-408`). The protocol is already
frontend-agnostic: `ChatInput` lines in on stdin, `ChatEvent` lines out on
stdout. A daemon can be that frontend.

**So warm mode is mostly wiring, not new machinery.** We reuse the chat loop;
the `bwoc-agent --serve` daemon becomes its driver instead of a human terminal.

## Hard scope constraint — harness backends only

`Backend::uses_harness()` (`crates/bwoc-cli/src/spawn.rs:50-78`) splits backends:

- **Confined / harness** — `ollama`, `openai-compatible`, `openrouter`. Run
  inside `bwoc-harness`'s own loop → **can be kept warm**.
- **Ambient / vendor CLI** — `claude`, `codex`, `kimi`, `antigravity`,
  `copilot`. Invoked one-shot (`claude -p <task>`); the vendor process has no
  resident-session concept we control → **cannot be kept warm** by us. They also
  carry full ambient authority (`BackendTrust::Ambient`,
  `bwoc-core/src/trust.rs:28-37`), so a daemon-driven resident loop would also be
  the wrong place to grant them standing tool authority.

Warm mode therefore applies **only** to `uses_harness()` backends. Ambient
backends keep today's cold-start path; the daemon logs that warm mode is N/A for
them. This is honest and avoids pretending we can warm a stateless vendor CLI.

## Design

### New: headless harness loop — `bwoc-harness --headless`

A non-interactive sibling of `--chat`. Same `drive()` core, same `chat_proto`
wire format, but:

- **No human.** The daemon writes `{"type":"user",...}` lines to the child's
  stdin and reads `ChatEvent` lines from stdout. (`--headless` ≈ `--chat` with a
  machine frontend — named distinctly to avoid colliding with
  `bwoc-agent --serve` and to switch the permission policy, below.)
- **Non-interactive permission policy.** `--chat` routes permission prompts to
  the operator; a daemon has no one to ask. `--warm` runs a fixed policy:
  Allow confined tools within the worktree sandbox, **Deny** anything escaping
  it (no mid-turn `permission_request` events). This reuses the existing
  `permission`/`policy.rs` machinery with a preset, not a new gate — and leans
  on Phase 5 confinement (harness tools are already sandboxed).
- **Same persistence.** Reuses `.bwoc/chat-session.json`, so a restarted daemon
  respawns the harness and the conversation continues for free.

### Daemon supervision — `bwoc-agent --serve` (opt-in)

Today the daemon only watches inbox/tasks and announces / tmux-pings
(`crates/bwoc-agent/src/main.rs:280-291`); it spawns no backend. Warm mode adds,
**behind an opt-in flag** (default off — Mattaññutā, no behavior change):

1. **Spawn** one resident `bwoc-harness --warm` child — lazily on the first
   routable message, or eagerly at startup. Track its pid in `.bwoc/harness.pid`
   (separate from `.bwoc/agent.pid`).
2. **Route** `check_inbox_for_new()` → existing trust gate → on accept, write a
   `user` line to the child's stdin; collect `ChatEvent`s until `turn_end`;
   append a result/receipt (reusing the #299 receipt path) so the sender sees
   completion.
3. **Serialize** turns: one resident harness per agent, messages queued, a turn
   runs to completion before the next is fed (matches the single-conversation
   model; no concurrent-turn race).
4. **Supervise**: reap + respawn with backoff on crash; on daemon `STOP`, send
   `{"type":"quit"}` then SIGTERM; clean up `.bwoc/harness.pid`.
5. **Fallback**: if the agent's backend is ambient (`!uses_harness()`), warm
   mode is unavailable → keep current announce/wakeup behavior, log once.

### Config surface (opt-in, default off)

- `BWOC_WARM=1` for `bwoc-agent --serve` (matches the existing
  `BWOC_TASK_WAKEUP` / `BWOC_AUTO_CLAIM` env-flag idiom), and/or a
  `warmMode = true` field in `config.manifest.json`. Default off.

## Phasing (one concern per PR)

- **PR A — `bwoc-harness --warm`** (headless loop + preset non-interactive
  policy). Independently testable against the stub provider; no daemon changes.
  Lands and ships value on its own (a warm loop a gateway/script can drive).
- **PR B — daemon supervision** (`bwoc-agent --serve` spawns/feeds/reaps the
  resident harness under `BWOC_WARM=1`, routes inbox→harness, writes receipts,
  ambient fallback).
- **PR C — config + docs** (`warmMode` manifest field, README/ARCHITECTURE +
  EN/TH ROADMAP note, env-var table row).

## Security notes (call out for review)

- **No standing authority for ambient backends.** Warm mode is harness-only by
  construction; we never hold a resident vendor CLI with ambient authority.
- **Untrusted inbox still gated.** A2A/connector envelopes are `Principal`
  untrusted; they pass the existing daemon trust gate **before** being fed to
  the harness, and the preset policy denies sandbox-escaping tools — so a
  malicious message can't escalate via the warm loop.
- **Trust labels survive.** `ChatMessage.principal` is persisted in
  `chat-session.json`, so reloaded turns keep provenance for the Phase 5 t1
  ingress-labeling invariant.

## Open questions for the architect

1. **Eager vs lazy spawn** — start the resident harness at daemon startup, or on
   first routable message? Lazy saves idle cost; eager removes first-message
   latency. (Lean: lazy, with an eager opt-in later.)
2. **Idle shutdown** — should the resident harness self-exit after N idle
   minutes to free the model, respawning on the next message? (Lean: yes, with a
   generous default; bounds GPU/RAM for an idle agent.)
3. **`--warm` naming** — `--warm` vs `--served` vs `--headless`. (Lean: `--warm`
   — pairs with the issue's "warm mode" language; `--serve` is taken by the
   daemon.)
4. **Result delivery** — append turn output to the inbox as a receipt
   (reuse #299), to a separate `.bwoc/results.jsonl`, or both?

## PR A — shipped (the `--headless` loop)

Implemented the standalone served loop; no daemon changes yet.

- **`bwoc-harness --headless`** (`crates/bwoc-harness/src/main.rs`) — conflicts
  with `--chat`/`--task`/`--resume`/`--lead`/`--eval` and, critically, with
  `--unrestricted` (headless leans on the sandbox for confinement, so lifting it
  is forbidden). Reuses `run_chat_mode(.., headless: true)` — same provider,
  system prompt, deep-memory wake-up/mine, tool registry, and
  `.bwoc/chat-session.json` persistence as `--chat`. Deep-memory mine tag is
  `"served"` vs chat's `"chat"`.
- **`ChatConfig.headless`** (`chat_session.rs`) — when set, `drive()` starts
  `session_mode = Bypass`, so an `ask`-mode tool auto-approves and the turn never
  emits a `PermissionRequest` / blocks on a `Permission` answer. Layer-1
  guardrails, policy `deny` rules, and the worktree sandbox are untouched.
- Tests: `headless_auto_approves_ask_tool_without_prompt` (a write_file runs with
  no prompt and no permission line on stdin — would stall in interactive mode)
  and `headless_still_denies_policy_deny_rule` (a `deny` still blocks). fmt +
  clippy clean; 20 `chat_session` tests pass. Live-smoked `--help` + both
  conflict guards.

Deferred to PR B: daemon supervision (lazy spawn, idle-exit, inbox→stdin
routing, receipts, reap/respawn, ambient fallback).

## Related

- issue #301; `crates/bwoc-harness/src/chat_session.rs`,
  `crates/bwoc-core/src/chat_proto.rs`, `crates/bwoc-agent/src/main.rs`,
  `crates/bwoc-cli/src/{run,spawn}.rs`, `crates/bwoc-core/src/trust.rs`.
