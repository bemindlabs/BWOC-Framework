# 2026-08-08 — fleet status shows real pid liveness (online), not inbox mtime

`bwoc fleet status` derived its LAST-SEEN column from the inbox.jsonl mtime,
which is "last time a message was written" — not liveness. A dead agent that just
received a broadcast read "just now"; an idle-but-running agent with no inbox read
"never". Real pid liveness (`livecheck::running_pid`) was already used by
`bwoc status`, `dashboard`, `workspace`, and `fleet health` — but not by the view
literally named for fleet presence.

## What changed
- `AgentStatus` gains `online: bool` = `running_pid(&workspace, entry).is_some()`.
- Table: new `ONLINE` column (● online / ○ offline); `LAST-SEEN` relabeled
  `LAST-MSG` so the mtime isn't mistaken for liveness.
- `--json`: adds `online`; keeps `last_seen_secs` (commented as last-msg age).

## Note
Reuses the existing pid path — no new heartbeat file (a first cut of the deferred
presence gap). Surfaced by the multi-agent gap audit (§3, MED); pairs with the
extension Fleet-view liveness fix (#34).
