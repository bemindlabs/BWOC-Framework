# 2026-06-19 — Design: warm / served agent mode (issue #301)

**Status: approved 2026-06-19. All three PRs shipped (#315 A, #316 B, PR C docs) — #301 complete.**

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
  the operator; a daemon has no one to ask. `--headless` starts the session in
  **auto-approve mode** (`ChatConfig.headless` → `drive()` starts
  `session_mode = Bypass`): an `ask`-mode tool runs without emitting a
  `permission_request` or blocking on an answer. It does **not** replace the
  permission policy — guardrails, policy `deny` rules, and the Phase-5 worktree
  sandbox all remain in force; only the interactive prompt is skipped.
- **Same persistence.** Reuses `.bwoc/chat-session.json`, so a restarted daemon
  respawns the harness and the conversation continues for free.

### Daemon supervision — `bwoc-agent --serve` (opt-in)

Today the daemon only watches inbox/tasks and announces / tmux-pings
(`crates/bwoc-agent/src/main.rs:280-291`); it spawns no backend. Warm mode adds,
**behind an opt-in flag** (default off — Mattaññutā, no behavior change):

1. **Spawn** one resident `bwoc-harness --headless` child — lazily on the first
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

- **PR A — `bwoc-harness --headless`** (headless loop + preset non-interactive
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

## Open questions — resolved (architect, 2026-06-19)

1. **Spawn/idle (PR B):** lazy spawn on first routable message + idle-exit after
   N idle minutes (frees model GPU/RAM), respawn on the next message.
2. **Flag naming:** `--headless`.
3. **Result delivery (PR B):** inbox receipt, reusing the #299 receipt path.

(See "Resolved decisions" at the top — repeated here so the section that posed
them now records the answers.)

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

## PR B — shipped (daemon warm supervision)

Implemented `crates/bwoc-agent/src/warm.rs` (`WarmHarness`) + wiring into
`task_watch.rs` and `main.rs`. Opt-in via `BWOC_WARM=1`, default off.

- **Trusted task path only** (the architect's scoping call). When auto-claim wins
  a Saṅgha task and warm is active, the task is fed into a **resident
  `bwoc-harness --headless`** as a trusted `Principal::LocalOperator` (effectful
  tools allowed — the agent *does* its own queue's work), then marked complete
  via `bwoc task complete`. Untrusted gateway input stays on the existing
  `autoprocess` `--chat` + auto-deny path — never routed here.
- **Lifecycle:** lazy spawn on first task (`.bwoc/harness.pid` written),
  idle-reap after 600s (frees the model; respawns on next task), dead-child
  respawn on the next task, quit+reap on daemon shutdown and on `Drop`.
- **Confined backends only:** ambient (`cli`) is refused (announce logs it), same
  fail-closed rule as autoprocess — headless auto-approve can't be confined when
  tools escape the harness.
- **Governance:** a `requires_plan` task is **skipped** (falls back to tmux-wake)
  — the daemon can't stand in for the lead's Pavāraṇā plan approval.

### Result delivery — deviation from the literal "inbox receipt" decision

The approved decision was "inbox receipt (reuse #299)". On building it surfaced
that a **task** has no sender to receipt back to (`Team` has no lead field, `Task`
no requester), so the #299 inbox-*message* receipt doesn't map onto the task
path. The task's authoritative receipt is its **`Completed` state** (set by
`bwoc task complete`, visible fleet-wide via `bwoc tasks`), plus a logged
summary. The #299 inbox-receipt reuse properly attaches to a future warm
inbox-*message* path (messages have senders) — which is also nearer the untrusted
boundary we deliberately kept separate. Flagged for the architect rather than
inventing a fake recipient.

### Tests / verification

5 unit tests (warm-off inert, ambient refused, confined not-flagged, inactive
declines without spawning, plan-gate detection). fmt + clippy clean; 48
bwoc-agent tests pass. The live resident lifecycle (real harness child speaking
`chat_proto`) needs a real backend → manual/e2e follow-up, matching the
autoprocess test depth (which also unit-tests gating only).

## PR C — shipped (docs)

Deliberately minimal (Mattaññutā):

- Added a `BWOC_WARM` row to the README environment-variable table — the genuine
  doc gap (it sits beside its siblings `BWOC_TASK_WAKEUP` / `BWOC_AUTO_CLAIM`).
- **Dropped the `warmMode` manifest field.** The sibling daemon opt-ins are
  env-only; a manifest field for warm alone would be an inconsistent second
  config surface. `BWOC_WARM=1` is the whole opt-in.
- **No ARCHITECTURE/ROADMAP additions.** ARCHITECTURE has no daemon-execution
  section (it frames coordination as file-based) and the ROADMAP is phase-level;
  warm mode is sub-phase detail already captured in `CHANGELOG.md` (PR A + B
  entries). Adding it to either would be gold-plating.

Closes #301.

## Related

- issue #301; `crates/bwoc-harness/src/chat_session.rs`,
  `crates/bwoc-core/src/chat_proto.rs`, `crates/bwoc-agent/src/main.rs`,
  `crates/bwoc-cli/src/{run,spawn}.rs`, `crates/bwoc-core/src/trust.rs`.
