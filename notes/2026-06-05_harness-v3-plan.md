# 2026-06-05 — Harness v3 plan ("run together, remember, improve")

Planning note for the third major `bwoc-harness` iteration. Grounded in a full
survey of the current 17 modules and the v1/v2 history; no code in this note.

**Arc:** v1 (2026-05-24) = *run safely* — agentic loop + the
guardrails→permission→sandbox pipeline. v2 (#39, 2026-05-27 → refined through
2.23.0) = *run durably & at scale* — checkpoints/resume, Saṅgha lead/workers,
budget gate, MCP client, retrospective, concurrent tools, streaming usage;
plus Trust v2, native Anthropic, chat (TUI/desktop) with compaction +
permission modes. **v3 = teams that actually talk, agents that remember
across sessions, and runs that leave the agent measurably better.**

## Workstreams

- **HV3-1 — Memory in the loop (Sati).** Wire `bwoc-deep-memory` into the
  runtime: `wake-up` output injected as session-start context, `mine` fired on
  session end, and a `memory_search` tool (Tier-2 semantic) beside
  `memory_read`/`memory_write`. Driven by the manifest's `deepMemoryCmd`;
  absent stays non-fatal (Tier 1 alone keeps working). Touches `agent_loop`,
  `chat_session`, `tools`. (M)
- **HV3-2 — Unified context engine.** The batch loop still compacts by
  truncate-with-marker; generalize chat's `compact.rs` (LLM summarization)
  into one `ContextEngine` consumed by both paths, and feed HV3-1 retrieval
  hits into context under the existing budget gates. (M)
- **HV3-3 — Saṅgha collaboration (Kalyāṇamitta).** (a) Team chat: agents see
  each other's replies (opt-in broadcast in the chat protocol — today they
  answer independently). (b) Worker → lead **structured result envelope**
  (diff summary, gate outcomes, metrics) instead of a bare exit code.
  (c) **Peer-review gate**: the lead may route a worker's diff to a reviewer
  agent before `run_gates`; a refusal sends the task back to the queue with
  the feedback attached. (L)
- **HV3-4 — Self-improvement v2 (Paññā).** Retrospective suggestions become
  *draft patches* (policy / prompt / mindset stubs) opened as PRs — never
  auto-applied; the eval suite runs pre/post as the regression gate and every
  suggestion carries its eval evidence. Closes the §8b loop with a human hand
  on the merge button. (M)
- **HV3-5 — Live remote surface.** Evolve `bwoc remote` from bookkeeping to
  control: expose a running session as an **MCP server** (stdio first;
  loopback HTTP later) *or* extend the **A2A** listener so an external
  runtime / Remote Control client can drive a session — through the SAME
  guardrails→permission pipeline, never around it. Open decision:
  MCP-server-first vs A2A-first. (M-L)
- **HV3-6 — Vendor headless adapters.** Non-interactive adapters for
  codex / kimi / agy so `bwoc run` covers all six declared backends and
  cross-backend CI reaches ×5 (Samānattatā). Vendor-CLI-bound; effort per
  vendor. (M each)
- **HV3-7 — Hardening.** CredentialBroker wired by default for
  network-touching tools; finer per-tool sandbox profiles (landlock /
  sandbox-exec); telemetry secret-redaction audit. (S-M)

## Sequencing

HV3-1 → HV3-2 (foundation) → HV3-3 → HV3-4 (the heart). HV3-5 can run in
parallel after HV3-1. HV3-6 / HV3-7 are demand-driven and independent.

## Definition of done (v3)

A Saṅgha team completes a task where: workers share context, a peer agent
reviews the diff before gates, every agent wakes with Tier-2 memory and mines
on exit, and the run ends with an eval-backed improvement PR — all inside the
v1 safety pipeline, demonstrated on ≥ 2 backends.

## Non-goals

- MCP network server beyond loopback (no-ports invariant holds).
- Autonomous self-modification — improvement lands only via human-gated PRs.
- TUI/interactive parity for vendor-CLI backends.

## Open decisions (need the architect)

1. HV3-5 transport: MCP-server-first or A2A-first?
2. HV3-3 reviewer selection: fixed reviewer agent per team vs round-robin vs
   manifest-declared (`reviewerOf` style)?
3. HV3-4 patch surface: policy + prompt only, or also mindset/skill stubs?

## Related

- Survey basis: `crates/bwoc-harness/src/*` (17 modules), `HARNESS.en.md`,
  `notes/2026-05-25_harness-v2-planning.md`, CHANGELOG v2.2.0 → 2.23.0.
- Known stale doc flagged en route: HARNESS.en.md "Not Yet" row on the OS
  sandbox (landlock/sandbox-exec shipped 2.3.0) — fix with HV3 docs pass.
