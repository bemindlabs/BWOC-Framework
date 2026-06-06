# 2026-06-06 — Connector PR3: daemon supervision

`bwoc-agent --serve` now keeps an agent's chat connector alive, so connectors
run as part of the fleet rather than as a hand-launched process (the
"on the bwoc-agent daemon" decision).

## What changed

- **New `bwoc-agent::connectors`** — `ConnectorSupervisor`:
  - `detect(cwd)`: finds an enabled connector (`connectors/<platform>.toml`
    with `enabled = true`; PR3 knows `telegram`) and resolves the
    `bwoc-connect` sibling binary.
  - `tick()` (idle-tick hook): spawns `bwoc-connect <platform> --agent <dir>`
    if not running; on child exit, respawns after a 5s backoff. Cheap
    (`try_wait`), so it's safe on every poll.
  - `shutdown()`: kills the child on daemon stop.
- **`serve_core`** wires it: detect + announce + initial `tick()` at startup,
  `tick()` on each idle poll, `shutdown()` before the graceful exit / endpoint
  cleanup.
- `bwoc-agent` gains only `toml` (to read the `enabled` flag via `toml::Value`,
  no serde-derive). **`bwoc-connect` is spawned, never a dependency** — the
  network deps (reqwest/tokio) stay quarantined (verified `bwoc-agent`'s tree
  has neither).

## Decisions

- **Supervise-as-subprocess, mirroring `bwoc-harness`/lead workers.** Keeps the
  daemon lean and the failure domain isolated (a crashing connector can't take
  the daemon down; it just respawns).
- **5s respawn backoff** so a misconfigured/crash-looping connector can't spin
  the daemon's CPU or flood logs (pairs with bwoc-connect's own 2s poll-error
  backoff).
- **Single connector in PR3** (first enabled platform). Multi-connector and
  Discord are PR4-era.

## Tests

`connectors`: `connector_enabled` (true / false / missing-flag / malformed /
absent file) and `detect` (finds enabled telegram / none when disabled). The
spawn/respawn/shutdown subprocess loop is the eyeball-reviewed edge (mirrors the
existing untested-by-unit `bwoc-harness` spawn). 31 bwoc-agent tests pass; fmt +
clippy `-D warnings` clean.

## Status / next

- PR3 (daemon supervision) done. **Remaining PR3 follow-ups**: keyring token
  resolution (the one cross-platform-dep risk — its own careful PR) and a
  `bwoc status` connector-health line. **PR4**: Discord.

## Related

- `crates/bwoc-agent/src/{connectors,main}.rs`
- `notes/2026-06-06_connect-pr1-telegram-dm.md`, `..._pr2-telegram-group.md`,
  `..._chat-connectors-design.md`
