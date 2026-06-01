# 2026-06-01 — Routing: MQTT transport target (foundation)

Make the inter-workspace routing table transport-aware so a `[[route]]` can
deliver over **MQTT** (cross-machine federation), not only to a local peer
workspace path. This is the `bwoc-core` foundation; the actual MQTT publish +
serve daemon will live in a separate `bwoc-mqtt` crate.

## What changed

- `Route` no longer carries a bare `workspace: PathBuf`. It now has a
  `target: RouteTarget`:
  - `RouteTarget::Local(PathBuf)` — peer workspace root (the v1 default; written
    straight into the peer's `inbox.jsonl`). `transport = "local"` (or absent).
  - `RouteTarget::Mqtt { broker, topic }` — `transport = "mqtt"`, a broker URL +
    optional topic (defaults to `bwoc/<recipient-id>/inbox`).
- `routes.toml` (`RawRoute`) gains optional `transport` / `broker` / `topic`;
  `workspace` is now optional (required only for `local`). Defaults to `local`,
  so every pre-MQTT route keeps its exact meaning.
- New validation errors: `LocalMissingWorkspace`, `MqttMissingBroker`,
  `UnknownTransport`. `BothKeys`/`NeitherKey` now label by the route's
  `agent`/`namespace` key (a route need not carry a workspace anymore).
- `Routes::resolve_target` returns the `RouteTarget`; `Routes::resolve` keeps
  its `Option<&Path>` signature (returns `None` for an MQTT route, so local-only
  callers like `bwoc peer` treat it as no match).
- `bwoc peer list` shows a TARGET column (local path or `mqtt <broker> [topic]`).

## Decisions

- **No MQTT dependency in `bwoc-core`.** Routes carry plain strings; the publish
  is the `bwoc-mqtt` crate's job (dep-quarantine — keep core lean).
- **Allow-list of transports** (`local`/`fs`, `mqtt`) with a clear
  `UnknownTransport` error rather than silently defaulting an unknown value.
- `remove_agent_routes` round-trips the raw TOML (filter by `agent`), so MQTT
  routes survive a retire untouched — no `Route`→`RawRoute` conversion needed.

## Status / deferred

- Foundation only — **nothing publishes over MQTT yet**. Next: a `bwoc-mqtt`
  crate (`publish` an envelope to `broker`/`topic`; `serve` to subscribe and
  drop into `inbox.jsonl`), then wire `bwoc send`/`peer` to publish when a route
  resolves to `RouteTarget::Mqtt`.

## Related (links)

- `crates/bwoc-core/src/routing.rs` — `RouteTarget`, `resolve_target`, validation
- `crates/bwoc-cli/src/peer.rs` — TARGET column
- `modules/agent-template/interconnect/routing.md` — spec (to extend for MQTT)
