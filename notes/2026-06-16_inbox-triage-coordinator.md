# 2026-06-16 — `bwoc triage`: rule-based gateway-inbox coordinator (issue #296)

The gateway receiver appends remote messages to `.bwoc/inbox.jsonl`, but a
relay-only orchestrator had **nothing that consumed them** — busaba accumulated
97 unprocessed messages over 6 days. The framework already has an auto-processor
(`bwoc-agent --serve` + `auto_process=true`) that runs the agent on each message,
but it **refuses ambient (`cli`) backends** by design (t30: a vendor subprocess
the harness can't confine must not be driven by untrusted remote input) — which
is exactly what the orchestrators use. So those agents had no safe consumer.

## What changed

New `bwoc triage <agent>` (`crates/bwoc-cli/src/triage.rs`): a **deterministic,
no-LLM** coordinator loop — safe for any backend because it never feeds the
untrusted message to a model.

- Reads new envelopes from a **separate byte cursor** (`.bwoc/inbox.triage.cursor`,
  starting at 0 so the accumulated backlog drains — unlike the daemon's
  `inbox.cursor` which starts at EOF). Reuses the inbox resolver from #302.
- **Classifies** each by rules from an optional `interconnect/triage.toml`
  (`pattern` = message substring or `from:<id>`; `action` = `ack` | `escalate` |
  `forward` + `target`). Default action `escalate` — nothing is silently dropped.
- **Acts**: `ack` (record + drop), `escalate` (surface in the digest), `forward`
  (re-deliver to another agent's inbox via the shared resolver, tagged
  `forwarded_by`).
- Writes a **receipt** per message to `.bwoc/inbox.triage.jsonl` (the ack trail,
  also a step toward #299's read-receipt ask), **advances the cursor**
  (exactly-once — messages never reprocess or accumulate), and prints a digest
  (`--json` twin). `--loop` polls; `--dry-run` previews without side effects.

## Decisions

- **Triage, not auto-respond.** The user chose a rule-based loop over enabling
  the model auto-processor precisely because the orchestrators are ambient/`cli`
  and the auto-processor (correctly) refuses them. Triage is the safe consumer:
  it routes/escalates without ever running a model on adversarial input.
- **Separate cursor from the daemon.** `bwoc-agent --serve` and `bwoc triage`
  can both watch the same inbox without fighting over one offset.
- **Default escalate.** Fail toward operator visibility, not silent drops — an
  unconfigured triage still drains the backlog into a receipt log + digest.

## Status / deferred

- The deployment side (replace/augment the launchd `*-worker.service` to run
  `bwoc triage <orch> --loop`) is an ops change on the Mac/bemind, not framework.
- Richer rules (regex, priority, time-window) and a `dispatch`-to-task-queue
  action are later slices if the substring/`from:` matcher proves too coarse.

## Related

- issue #296; `crates/bwoc-cli/src/triage.rs`, `crates/bwoc-cli/src/main.rs`;
  pairs with the #302 resolver and #297 `fleet status` (which surfaced the pileup).
