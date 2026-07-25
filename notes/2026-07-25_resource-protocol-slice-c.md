# 2026-07-25 — Resource Protocol slice C: advertise + discover clients

The broker clients. With the gateway broker (slice B, `bwoc-gateway` PR #12) up,
a light host can now publish an offer and a consumer can find it — the discovery
loop works fleet-wide end-to-end.

## What changed

- `crates/bwoc-cli/src/resource.rs` — two new verbs:
  - `bwoc resource advertise --provider <id> [--ttl 30] [--gateway URL]` — load
    `[resource]` config (must `share = true`, must offer kinds, must have a
    gateway), build a fresh snapshot, `POST /v1/resource/advertise`. One shot —
    run on a timer for a heartbeat.
  - `bwoc resource discover --kind K [--gpu-vram N] [--ram N] [--cores N] [--gateway URL]`
    — `POST /v1/resource/discover`, print matching live offers (best-free-VRAM
    first) or `--json`.
- Transport helpers: `gateway_http_base` (`ws(s)://` → `http(s)://` normalise),
  `resolve_gateway` (flag > `[resource] gateway`), `http_post_json` (shells
  `curl`), pure `advertise_body` / `discover_body` builders.
- Removed the `#[allow(dead_code)]` on `SharingConfig.gateway` — now consumed.
- `docs/{en,th}/RESOURCE-PROTOCOL` §Slices: A–C shipped; `claim`/offload = slice D.
- 6 new unit tests (URL normalisation, gateway resolution precedence, body shapes).

## Decisions

- **Shell `curl`, don't add reqwest.** `bwoc-cli` is deliberately HTTP-client-free
  (no reqwest/tokio) to stay a fast, light CLI. The framework's established HTTP
  pattern is "plugins shell `curl`" (gcloud/gws/accounting bash entries). The
  gateway is HTTPS (tailscale serve), which rules out a hand-rolled TCP client,
  so `advertise`/`discover` shell `curl` too — same convention, zero new deps.
- **`advertise` is one-shot, not a daemonised heartbeat loop.** A CLI verb that
  blocks in a loop is un-Unixy and hard to supervise. One-shot + "run on a timer"
  composes with cron/systemd/launchd and keeps the verb testable. The `--ttl` is
  the broker-side eviction window; the operator's timer interval is the heartbeat.
- **Discovery half only; claim rides the relay.** Per slice B's design, `claim` →
  `RES.LEASE` is *not* a broker route — it's a signed envelope relayed to the
  provider (keeps the broker dumb). So slice C ships the two broker-backed verbs;
  `claim`/`release`/offload (the relay round trip + provider-side gate + job exec)
  are slice D.
- **Broker transport failure = exit 255** (distinct from local/usage errors), so a
  script can tell "the gateway is unreachable" from "you passed bad args."

## Status / deferred

- Shipped 2.40.0 (`v2026.7.25-2`); gateway broker in `bwoc-gateway` (slice B).
- **Slice D (framework):** `claim` (send `RES.CLAIM` envelope over the relay →
  provider runs `evaluate_gate` → mints signed `RES.LEASE`), `release`, `compute`
  offload execution (run the job on the provider via A2A → return the result),
  then `kv` + `knowledge`. This is the piece that actually *runs* borrowed work.

## Related

- `crates/bwoc-cli/src/resource.rs`, `docs/en/RESOURCE-PROTOCOL.en.md`
- `bwoc-gateway` PR #12 (the broker this talks to)
- `notes/2026-07-25_resource-protocol-slice-a.md` (the types + gate this builds on)
