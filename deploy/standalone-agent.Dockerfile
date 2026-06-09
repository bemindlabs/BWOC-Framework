# Standalone BWOC agent image — bundles the framework + gateway runtime binaries
# with one agent so `docker run` brings a live agent online against a
# `bwoc-gateway` relay: it receives relayed messages, verifies + replay-checks
# them, runs an UNTRUSTED harness turn, and replies — all across machines.
#
# Build context must contain three subdirectories:
#   fw/     — a BWOC-Framework checkout  → bwoc, bwoc-agent, bwoc-harness
#   gw/     — a bwoc-gateway checkout     → bwoc-gateway-send, bwoc-gateway-recv
#   agent/  — the incarnated agent dir    → AGENTS.md, config.manifest.json, slot
#             dirs, interconnect/gateway.toml (enabled, url, auto_process),
#             interconnect/routes.toml (a transport=gateway route per peer it
#             replies to), and .bwoc/peers.toml (pinned sender pubkeys).
#
# Build:  docker build -f deploy/standalone-agent.Dockerfile -t bwoc-agent:<name> .
# Run:    docker run -d --name <name> \
#           -e ANTHROPIC_API_KEY \                         # or mount provider creds
#           -v $PWD/agent/.bwoc/agent.key:/agent/.bwoc/agent.key:ro \  # identity
#           -v <name>-state:/agent/.bwoc \                 # inbox + cursor persist
#           bwoc-agent:<name>
#
# The ed25519 identity key and provider credentials are mounted/injected at run,
# never baked into a layer.

FROM rust:1-bookworm AS fw
WORKDIR /src
COPY fw/ .
RUN cargo build --release -p bwoc-cli -p bwoc-agent -p bwoc-harness

FROM rust:1-bookworm AS gw
WORKDIR /src
COPY gw/ .
RUN cargo build --release -p gateway-client --bin bwoc-gateway-send --bin bwoc-gateway-recv

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 -m -d /home/agent agent
# All five binaries on PATH so the daemon's sibling-binary resolution finds them.
COPY --from=fw /src/target/release/bwoc              /usr/local/bin/bwoc
COPY --from=fw /src/target/release/bwoc-agent        /usr/local/bin/bwoc-agent
COPY --from=fw /src/target/release/bwoc-harness      /usr/local/bin/bwoc-harness
COPY --from=gw /src/target/release/bwoc-gateway-send /usr/local/bin/bwoc-gateway-send
COPY --from=gw /src/target/release/bwoc-gateway-recv /usr/local/bin/bwoc-gateway-recv
COPY --chown=agent:agent agent/ /agent
USER agent
WORKDIR /agent
# `--serve` supervises bwoc-gateway-recv (dials the relay) and auto-processes
# inbound messages via bwoc-harness, per interconnect/gateway.toml.
ENTRYPOINT ["bwoc-agent", "--serve"]
