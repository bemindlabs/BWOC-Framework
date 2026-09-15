# 2026-09-15 — Runtime R2: chat parity with batch

Phase R2 of "BWOC as its own coding agent". The `--chat` / `--headless` session driver (`chat_session.rs`) had fallen behind the batch `run_loop` in six places. It now reuses the batch code for provider retry and fallback, MCP registration, the capability gate and turn executor, and context sizing. It also streams reasoning text to the TUI. Two small fixes ride along: echo restore in `bwoc auth set`, and line breaks in the TUI. Stacked on R1 (#522), with `origin/main` merged in after R1's squash.

## What changed

- **Retry and fallback.** Chat had a private stream accumulator: no retry, and Anthropic thinking blocks dropped, which breaks the next tool turn when thinking is on. `agent_loop::provider` now has one `call_with_retry` behind both `call_with_retry_v2` (batch) and `call_with_retry_live` (chat). `execute::stream_and_accumulate_live` hands each delta to a sink. Chat's `stream_call` joins the provider future with an emitter over an mpsc channel, so tokens stay in order while the call runs. `ModelChain` applies `run_loop`'s malformed-tool-call threshold, and `run_chat_mode` keeps the auto resolver's `remaining` as the chain.
- **MCP.** `register_mcp_servers` (harness `main.rs`) is shared by `run()` and `run_chat_mode`. Batch prints to stdout, chat to stderr.
- **Sandbox parity.** `policy::capability_gate` is Layer 0 of `run_pipeline`, extracted without changes. Chat keeps a `SessionTrust` latch, runs the gate, then guardrails, plan mode, permission, and finally `turn_executor::execute_proceeded`. Tool images now reach history.
- **Context window.** `chat_session::context_budget` / `known_context_window`, and `--max-context`, which bare `bwoc` fills from `[runtime] max_context` through `ProjectSession`.
- **Thinking.** `Delta.reasoning_content` / `Delta.reasoning`; the Anthropic `thinking_delta` also fills `reasoning_content`. `LiveDelta::Thinking` becomes `ChatEvent::Thinking`. The TUI adds `App.thinking` and `flush_thinking`, and a `∴` prefix renders dim.
- **`bwoc auth set`.** `EchoGuard` records the original termios in a static. A once-per-process `ctrlc` handler restores it and exits 130.
- **TUI line breaks.** `styled_lines` splits each transcript entry on `\n`.

## Decisions

- **Capability gate in interactive chat → operator prompt, not a hard deny.** `scan_turn_trust` counts tool output as untrusted, so the latch goes Untrusted after the first `read_file`. Batch then refuses `run_command` / git for the rest of the run. Doing the same in the TUI would make bare `bwoc` unable to run a test after reading a file. The rule instead:
  - `--headless` (no operator) denies, exactly as batch does.
  - An interactive session raises a `PermissionRequest` whose detail names the gate.
  - Bypass / accept-edits mode and policy `allow` never approve a gated call silently.
  - Connectors (`bwoc-connect`, warm / autoprocess) already auto-deny permission requests.

  **Needs architect sign-off**: it is weaker than batch only in the sense that a present human can approve. (Sīla — the gate still runs, and consent is explicit.)
- **No retry after a delta was emitted.** The frontend has already rendered the text; a replay would duplicate it. Transient errors almost always come before the first byte.
- **Fallback switches only when a next model exists.** With an empty chain, chat keeps the old behaviour: the tool rejects bad JSON and the model can correct itself. Ending the turn, as batch's `AllModelsExhausted` does, would regress weak local models in an interactive session.
- **Two reasoning fields, not a serde alias.** An alias errors on a chunk that carries both names, which would fail the whole stream.
- **Known windows keyed by backend, not model id.** Only `anthropic` / `claude` have one window across current models (200k). No model-id table to rot (Mattaññutā).
- **Budget = window minus max(10% headroom, response cap).** The cap is used only when it fits the window. An unknown window keeps the old 8,000, so local-model behaviour is unchanged.
- **The trust latch is not reset on `forget`.** It is monotonic, as in batch; a new process starts clean.

## Alternatives considered

- **Routing chat through `run_pipeline` wholesale.** Its permission layer prompts the TTY, and chat's `ask` must go to the frontend. The gate was extracted instead, and chat keeps its frontend `ask`.
- **An async callback for live deltas.** The borrowed `out` writer makes the lifetimes awkward. A sync sink plus a channel keeps the batch path free of async closures.

## Bugs surfaced and fixed

- Chat dropped Anthropic thinking blocks. With `thinking: true`, the tool turn after a thinking block would be rejected, since the API requires the block to be replayed.
- `--headless` sessions (connectors, `bwoc-agent --serve`) had no Layer-0 capability gate and executed tools in-process. The #271 "Untrusted turn is read-only" guarantee held only in batch.
- TUI transcript lines lost their `\n`.

## Status / deferred

- **MCP and `memory_search` tools are still denied at execution on unix**, in both paths: the turn executor refuses un-marshallable tools (Phase 5 t5, C5). Chat's default policy lists `memory_search` as allowed, so agents with `deepMemoryCmd` now see it denied in chat as they already did in batch. Closing this needs executor IPC for dynamic tools.
- The context budget is sized for the primary model at startup. It is not re-sized after a fallback switch, and there is no token-pressure model switch in chat.
- Agent sessions (`bwoc chat <agent>`) have no manifest `maxContext`. The harness flag works; the TUI passes it only for project sessions.
- The coordinator's TUI e2e report (collapsed newlines) is fixed here. Other TUI rendering is left for R5.

## Related

- R1: [`2026-09-15_runtime-zero-setup.md`](2026-09-15_runtime-zero-setup.md), PR #522
- [`HARNESS.en.md` §Quick start](../docs/en/HARNESS.en.md#quick-start-bwoc-in-any-repository)
