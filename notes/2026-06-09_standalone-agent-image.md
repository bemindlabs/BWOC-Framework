# 2026-06-09 — Standalone agent: deployable container image

`deploy/standalone-agent.Dockerfile` packages one incarnated agent plus the
five runtime binaries it needs into a single image, so `docker run` brings a
live agent online against a `bwoc-gateway` relay — it receives relayed
messages, verifies + replay-checks them (pinned `peers.toml`), runs an
UNTRUSTED harness turn, and replies, all across machines. This is the
packaging half (Component 5) of the approved "standalone agent
(production-ready core)" plan; the recv bridge, trust/replay gate, and
untrusted auto-process landed in the preceding PRs.

## What changed

- **`deploy/standalone-agent.Dockerfile`** (new) — three-stage build:
  - `fw` stage builds `bwoc`, `bwoc-agent`, `bwoc-harness` from a
    BWOC-Framework checkout (`fw/` in the build context).
  - `gw` stage builds `bwoc-gateway-send` + `bwoc-gateway-recv` from a
    `bwoc-gateway` checkout (`gw/`).
  - final `debian:bookworm-slim` + `ca-certificates` (TLS for `wss://`) copies
    all five binaries onto `PATH` (so the daemon's `sibling_binary` resolution
    finds them) and the incarnated agent dir (`agent/`). Runs as a non-root
    `agent` user, `ENTRYPOINT ["bwoc-agent","--serve"]`.

## Decisions

- **Secrets are mounted at run, never baked.** The ed25519 identity
  (`.bwoc/agent.key`, 0600) and provider creds are injected via `-v`/`-e` at
  `docker run`, never `COPY`-ed into a layer — an untrusted turn can read its
  own image but not a runtime mount it has no path to. A named volume for
  `.bwoc/` persists the inbox + read cursor across restarts.
- **Five binaries in one image, not a sidecar.** `bwoc-agent --serve`
  supervises `bwoc-gateway-recv` as a child and shells out to `bwoc-harness`
  per turn; co-locating them keeps `sibling_binary` resolution trivial and the
  unit self-contained. Network/TLS deps stay confined to the two gateway
  binaries (dep-quarantine holds — `bwoc-agent`/`bwoc-core` never link WS/TLS).

## Status / deferred

- Build context is assembled by hand (`fw/ gw/ agent/`); a `bwoc bundle` /
  `bwoc deploy` verb that produces it is deferred (see plan §Deferred).
- Image is multi-arch-capable in principle but only built/verified for the
  host arch so far.

## Related (links)

- `deploy/standalone-agent.Dockerfile`
- Plan: standalone agent (production-ready core)
- Preceding PRs: gateway receive bridge; pinned-peer keyring + replay defense;
  untrusted gateway auto-process.
