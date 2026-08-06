# 2026-08-06 — serve: warm per-sender message sessions (#410)

`bwoc-agent --serve` auto-processing used to **cold-start a fresh
`bwoc-harness --chat` per inbound message** and discard it. Now each remote
sender gets a **warm** session kept alive across messages and idle-reaped, so a
back-and-forth reuses one process + its conversation context — the perf/latency
half of #410 (message ingress), realizing #301's "keep warm" intent for the
message path.

## The load-bearing constraint (why not the task resident)

`SessionTrust` is a **monotonic latch** (`session_trust.rs` — set-once, survives
compaction *and* reload, never re-opens). So one Untrusted turn taints a session
for good. The warm task resident (`warm::WarmHarness`) runs **Trusted**
`LocalOperator` task prompts; feeding an Untrusted remote message into it would
permanently downgrade it and break effectful task execution. Therefore message
sessions are **deliberately separate** from the task resident. The message pool
is Untrusted from its first turn and stays that way, so the latch is a non-issue
there.

## What changed (`crates/bwoc-agent/src/autoprocess.rs`, `main.rs`)

- New `WarmSession` (spawn `--chat`, `run_turn`, `is_alive`, `Drop` = Quit+kill+wait)
  extracted from the old `run_untrusted_turn`. `run_turn` drains to **`TurnEnd`**
  (capturing `Message` en route) so the stream is left on a clean boundary for
  the *next* turn — breaking early on `Message` would leave a stray `TurnEnd`
  that the next turn would eat as an instant empty reply (the warm-reuse bug).
- `AutoProcessor` gains `sessions: HashMap<sender, WarmSession>` + `idle_timeout`
  (600 s, mirroring `warm::DEFAULT_IDLE`). `handle` is now `&mut self`:
  get-or-respawn this sender's session, run the turn, on error drop the session
  so the next message respawns. `tick_idle` (reap idle/dead) + `shutdown` wired
  into the serve loop next to `warm.tick_idle()` / `warm.shutdown()`.
- Untrusted posture unchanged: `Principal::Unknown`, permission prompts
  auto-denied, ambient-backend refusal (#271) intact.

## Scope / deferred

- **Gateway + A2A** (the inbox path) only. Chat connectors already keep warm
  per-`chat_id` sessions in their own process tree (`bwoc-connect`) — untouched.
- Still one message at a time (blocks the serve loop for a turn); a concurrent
  pool is a later slice.
- `auto_process` still opt-in/off-by-default; the "silent drop when off" log
  (#410 rec 3) is a separate follow-up.

## Related

- Issue #410 — the investigation that found the cold-start gap.
- [[2026-06-19_design-warm-agent-mode]] — the task-resident warm design this mirrors.
