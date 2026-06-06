# 2026-06-06 — Connector health in `bwoc status`

Surfaces whether an agent's chat connector is running, so the fleet view shows
connector liveness alongside agent liveness.

## What changed

- **`bwoc-agent::connectors`** — `ConnectorSupervisor::write_status(state, pid)`
  writes `<agent>/.bwoc/connector.status` (JSON: `platform`, `state` ∈
  running/exited/stopped, `pid`). Written on spawn (`running`+pid), on detected
  exit (`exited`), and on daemon shutdown (`stopped`); only when a connector is
  configured. Best-effort, serde_json (already a dep).
- **`bwoc-cli::status`** — `read_connector_status` reads that marker; `bwoc
  status` prints a **Connectors** section after the agent table (kept separate
  so the fixed-width table is undisturbed): `<agent>  <platform>  <state> (pid N)`.

## Decisions

- **Marker file, not the control socket.** `bwoc status` is read-only and runs
  without talking to the daemon; a tiny status file matches that posture (same
  spirit as the inbox cursor / pid file) and needs no IPC.
- **Separate section, not a table column** — avoids reflowing the
  carefully-aligned per-agent table.

## Tests

`connectors::write_status_emits_marker_when_configured` (marker written with
platform/state/pid; not written when no connector). bwoc-agent + bwoc-cli
clippy/tests green.

## Status / next

Connector follow-ups remaining: keyring token resolution (cross-platform dep —
careful CI validation), Discord gateway RESUME.

## Related

- `crates/bwoc-agent/src/connectors.rs`, `crates/bwoc-cli/src/status.rs`
