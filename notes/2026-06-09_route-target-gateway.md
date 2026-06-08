# 2026-06-09 — `RouteTarget::Gateway` transport

Wire a third `routes.toml` delivery transport — `gateway` — so `bwoc send` can reach a recipient through a `bwoc-gateway` rendezvous/relay server when no direct path exists (NAT, firewalls, the open internet). This is the framework half of the deferred "framework wiring" item; the gateway server + client + `bwoc-gateway-send` binary live in the **separate** [`bemindlabs/bwoc-gateway`](https://github.com/bemindlabs/bwoc-gateway) repo, not in this one.

## What changed

- `bwoc-core/src/routing.rs`: new `RouteTarget::Gateway { url }` variant; `RawRoute.gateway` field; `transport = "gateway"` validated (→ `GatewayMissingUrl` when the `gateway` url is absent); `UnknownTransport` message now lists all three. `resolve` (local-only) returns `None` for gateway routes — they have no local path — while `resolve_target` surfaces the url. Two new tests (parse, missing-url).
- `bwoc-cli/src/send.rs`: `Target::Gateway { url }`; on a routes.toml hit, resolve to it (no peer-registry lookup — the gateway routes by recipient id on its side, same as MQTT). Delivery shells out to `bwoc-gateway-send` with `--url/--agent-id/--to/--key-file`, piping the signed message envelope over stdin. New errors `GatewayUnsigned` / `GatewaySpawn` / `GatewayRelay`.
- `bwoc-cli/src/peer.rs`: `bwoc peer` lists gateway routes (`gateway <url>`).
- `CHANGELOG.md`: `[Unreleased]` row.

## Decisions

- **Mirror MQTT exactly (dep-quarantine).** `bwoc-core`/`bwoc-cli` must not link a WebSocket/TLS/crypto client. As with `bwoc-mqtt publish`, the CLI shells out to a sibling binary (`bwoc-gateway-send`) and pipes the envelope over **stdin** (keeps it out of `ps`, dodges `ARG_MAX`). The two transports are now structurally identical; only the binary and the route key (`--topic` vs `--to`) differ. *Samānattatā* — equal standing for each transport.
- **The signature seam is the inner envelope, not the gateway wrapper.** The harness already signs `{from,to,ts,messageId,message,nonce}` with `bwoc_signing`; that is what the recipient verifies. The gateway transport `Envelope` is a dumb routing wrapper the relay never inspects, so `bwoc-gateway-send` leaves its transport-level `signature` empty and carries the signed message as the opaque `body`. No second signing scheme, no canonicalization duplicated across repos.
- **Gateway requires a signed agent sender.** The sender's keypair *is* the gateway login (the WS challenge handshake), so a `user`/unsigned origin cannot authenticate. `send` fails early with `GatewayUnsigned` and actionable guidance (`bwoc trust --keygen`) rather than spawning a doomed relay. The key file is the existing `<sender>/.bwoc/agent.key`.
- **`routing.md` not rewritten.** The spec doc is frozen at "v1 — local FS only" and already does *not* document the `mqtt` transport that shipped after it (Yoniso manasikāra — verified against the file, not assumed). Adding `gateway` to code while leaving the v1 spec doc as-is matches that established precedent; rewriting the spec for one transport is scope the change doesn't earn (Mattaññutā). CHANGELOG + this note carry the record.

## Alternatives considered

- **In-process `gateway-client` crate dep on `bwoc-cli`** — rejected: pulls WebSocket/TLS/crypto into the lean CLI and couples the framework build to a separate repo. Violates the dep-quarantine the MQTT design exists to honour.
- **`bwoc-gateway-send` pre-built the gateway `Envelope`** (framework owns the wrapper schema) — rejected: leaks the gateway's transport schema into the framework. Instead the binary owns the wrapping and takes `--to`, so the framework stays agnostic (it pipes the same `line` it would for MQTT).

## Status / deferred

- Code + tests green: `bwoc-core` + `bwoc-cli` clippy `-D warnings` clean, fmt clean, full suite passes (897). No end-to-end gateway send test (mirrors MQTT, whose shell-out arm is likewise covered only at the routing layer).
- `bwoc-gateway-send` must be on `PATH` for gateway delivery (`cargo install --path crates/gateway-client --bin bwoc-gateway-send` from the gateway repo) — same operator step as `bwoc-mqtt`.

## Related

- [`bemindlabs/bwoc-gateway`](https://github.com/bemindlabs/bwoc-gateway) — the external repo with the server, client, and `bwoc-gateway-send` binary (PR #2). Not vendored here.
- `modules/agent-template/interconnect/routing.md` — the (v1) routing spec.
- `crates/bwoc-mqtt` — the transport whose shell-out pattern this mirrors.
