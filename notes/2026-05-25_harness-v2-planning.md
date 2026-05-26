# 2026-05-25 — bwoc-harness v2 planning (#39)

Turned the #39 design epic into an implementation plan: resolved the four open maintainer decisions with code-grounded recommendations, refined the workstream sequencing, and seeded HV2-1..7 into the workspace-root scrum board. **Design only — no crate code written** (build per workstream after review, like the plugin-cycle epic #6).

## What changed

- This note.
- Workspace-root scrum board (`<workspace>/.scrum/backlog.json`) seeded with the seven HV2 workstreams. The pre-existing **BWOC-3 (Trust v2)** — migrated here earlier today from the retired framework-local board — **is** HV2-4, so it was repurposed in place rather than duplicated. New items BWOC-4..9 cover HV2-1/2/3/5/6/7. All carry `epic: "harness-v2"` and `source: "GH #39 ..."`.

## Decisions

- **HV2-1 sub-agents → spawned `bwoc-harness` subprocesses (not in-process tasks).** The OS sandbox is process-scoped (landlock applies to a process + children; sandbox-exec wraps a process). In-process workers would share the lead's sandbox profile and address space — a compromised worker leaks into the lead. Subprocess workers each get their own worktree + sandbox profile and reuse the v1 safety pipeline unchanged. Lead spawns `bwoc-harness --task … --workdir <worktree>`, coordinates via existing `queue.rs` (TaskSource/Saṅgha) + inbox. *Sīla — the guardrails→permission→sandbox invariant must wrap every new exec path; subprocess is the only option that keeps that true for free.*
- **HV2-2 checkpoint → per-turn atomic JSON at `~/.bwoc/runs/<run-id>/checkpoint.json`.** Loop state today is all in-memory locals in `run_loop()` (`agent_loop.rs:330` history, `:334` turns, `:346` active_model, `:343` provider-limit cache, counters); `ChatMessage` is already serde. Persist `{run_id, task, history, turns, compactions, token_pressure_switches, active_model, telemetry_cursor}` after each turn's `TurnBuilder::finish()` (the consistent boundary — reply + tool results applied), temp-write + rename. Worktree state already persists on disk, so resume = reload + re-attach to the existing worktree; no replay. *Anicca — persist the seam, not the whole world.*
- **HV2-4 signing → ed25519 signed envelopes, spec-first.** HMAC fails the cross-workspace case (#20 give-feedback): a shared secret across a trust boundary lets any holder forge. Per-agent ed25519 keypair, private key in keyring (the `tools/auth.rs` broker exists), public key published with agent identity (manifest / `interconnect/shared.toml`). Signed payload carries `{nonce, ts, recipient}`; recipient keeps a sliding nonce window (the `inbox.refusals.jsonl` log is the seam). Replaces the keyword-only `check_identity_spoof()` (`guardrails.rs:293`). **Gate on its own `TRUST-V2.en.md` + `.th.md` spec pass before build** — this is the parked #6 decision. *Kalyāṇamitta / Musāvāda.*
- **HV2-5 → MCP client only (defer the server role).** The thesis is "extend tools without harness code changes" = client. A server role adds a network surface (contradicts the no-ports infra invariant + observe-don't-drive) for no stated need. Every MCP tool still flows through `execute_tool_calls()` → guardrails→permission→sandbox. *Mattaññutā.*

### Sequencing (refines Oracle's)

1. **HV2-2 durability** — foundational, additive, low-risk; unblocks resumable subprocess workers. **Build first.**
2. **HV2-3 self-improvement** — independent, low-risk; builds on existing `telemetry.rs`/`eval`. Can run parallel to (1).
3. **HV2-1 Saṅgha runtime** — headline; depends on (1) + wiring `run_loop` into the `queue.rs:295` placeholder executor.
4. **HV2-4 signing** — spec-first, then build; unblocks #20.
5. **HV2-7 streaming-usage before HV2-6 budget** — flipped Oracle's order: the SSE path returns `None` for usage (`agent_loop.rs:584`), so a streaming budget gate is blind until the usage gap closes. HV2-6 may ship for non-streaming first.
6. **HV2-5 MCP client** — independent extension.

## Alternatives considered

- In-process sub-agents (rejected: shared sandbox/address space breaks the safety invariant).
- HMAC signing (rejected: unsafe shared-secret distribution across workspace trust boundaries).
- On-tool checkpoint cadence (rejected: tools mutate the worktree mid-turn; the turn boundary is the only consistent seam).
- MCP server role (deferred: network surface + no current need).

## Code seams (grounded, for the implementers)

- Loop state / checkpoint target: `agent_loop.rs:295` `run_loop()`, locals at `:330/:334/:346/:343`.
- Multi-agent: `queue.rs:53` `TaskSource` trait, `:295` placeholder executor to replace, `:333` `poll_sangha()`.
- Self-improvement: `telemetry.rs:71` `TurnMetrics`, `:170` `SessionRecord` (write-only today, appended on exit at `main.rs:186`); `eval/mod.rs:183` offline `run_fixture()` — no live retrospective path exists.
- Safety invariant: `policy/mod.rs:87` `run_pipeline()` (guardrails→permission), sandbox after, in `agent_loop.rs:800` `execute_tool_calls()` — every new tool routes here automatically.
- Streaming-usage gap: `agent_loop.rs:584` (returns `None`); token-switch at `:373`; provider-queried limits `provider/client.rs:264` (TODO(#13) at `agent_loop.rs:219`).
- Signing today: keyword-only `check_identity_spoof()` `guardrails.rs:293`.

## Status / deferred

- Design only; each workstream is a separate build after review.
- **Doc drift surfaced (separate from #39):** `docs/en/HARNESS.en.md` §Not-Yet (~line 302) still lists the OS sandbox as a "Stub — only `NoopOsSandbox`," but landlock/sandbox-exec shipped in 2.3.0 (`sandbox.rs` `make_os_sandbox()`). That row is stale; fix the EN + TH pair separately.

## Related (links)

- GH #39 (this epic), #20 (give-feedback, unblocked by HV2-4), #6 (plugin-cycle precedent + parked Trust v2 decision).
- `<workspace>/.scrum/backlog.json` — BWOC-3..9 (the HV2 workstreams).
- `crates/bwoc-harness/` — the v1 baseline being extended.
