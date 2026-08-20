# bwoc-mqtt

MQTT transport for [BWOC](../../README.md) inter-workspace routing — publish one envelope to a broker, or subscribe and deliver received envelopes into agent inboxes.

The wire payload is exactly the JSON envelope line `bwoc send` appends to a local `inbox.jsonl`, so MQTT delivery and local-FS delivery produce identical inbox lines. Depends on [`bwoc-core`](../bwoc-core/) (for the `AgentsRegistry` recipient lookup and `redact_broker`) and `rumqttc` for broker I/O. **Dep-quarantine:** this is the only crate in the workspace that links an MQTT client — [`bwoc-cli`](../bwoc-cli/) reaches a `transport = "mqtt"` route by *spawning* the `bwoc-mqtt` binary, never by linking it. `rumqttc` is built with `default-features = false`, so there is no TLS stack and brokers are plaintext `mqtt://`.

## Scope

One flat library module (`src/lib.rs`) plus the CLI (`src/main.rs`):

- **`parse_broker` → `Broker { host, port, username, password }`** — parses `mqtt://[user[:pass]@]host[:port]` or a bare `host[:port]`; port defaults to `DEFAULT_PORT` (1883), userinfo is percent-decoded, and a parse failure echoes a *credential-redacted* URL.
- **`topic_for`** — the recipient's topic: an explicit override, else `bwoc/<id>/inbox`.
- **`recipient_from_envelope`** — extracts the `to` field from the envelope JSON.
- **`inbox_path` / `append_envelope`** — resolve `<workspace>/<agent path>/.bwoc/inbox.jsonl` through the `AgentsRegistry`, then append one newline-terminated line.
- **`publish`** — sends one envelope to a topic at QoS 1, pumping the event loop until the PubAck lands, then disconnecting.
- **`deliver` → `Delivery::{Delivered, UnknownRecipient}`** — resolve + append one received payload; callable (and tested) without a broker.
- **`serve`** — subscribe, then `deliver` per message. An unknown recipient or a malformed payload is dropped with a log line, never fatal.
- **`MqttError`** — `BadBroker`, `Client`, `Conn`, `Io`, `Workspace`, `BadEnvelope`, `MissingBroker`.

## Usage

Install it next to the `bwoc` binary (or anywhere on `PATH`) so `bwoc send` can find it: `cargo install --path crates/bwoc-mqtt`.

```bash
# sender — publish one envelope (reads stdin when --payload is omitted)
bwoc-mqtt publish --broker mqtt://broker.local:1883 --topic bwoc/agent-neo/inbox \
  --payload '{"ts":"…","messageId":"m1","from":"agent-a","to":"agent-neo","message":"hi"}'

# peer — subscribe and deliver into this workspace's agent inboxes
bwoc-mqtt serve --broker mqtt://broker.local:1883 \
  --workspace /path/to/workspace --topic 'bwoc/+/inbox'
```

`--broker` is optional on both subcommands: when omitted the URL is read from `BWOC_MQTT_BROKER_FILE` (a file holding it), then `BWOC_MQTT_BROKER`; with none of the three set the command exits with `MissingBroker`. That fallback is what keeps a credentialed broker URL out of `ps` — `bwoc send` passes the URL to the spawned child through `BWOC_MQTT_BROKER`, never as an argument. `--topic` defaults to `bwoc/+/inbox` on `serve`; `--client-id` defaults to `bwoc-mqtt-pub` / `bwoc-mqtt-serve`. `serve` reads recipients from `<workspace>/.bwoc/agents.toml`; a missing registry is not an error — it just leaves every recipient unknown, so every message is dropped with a log line.

## Status

In production use: `bwoc send` publishes over MQTT whenever a route resolves to `transport = "mqtt"`, and the peer's `bwoc-mqtt serve` daemon completes the hop into `inbox.jsonl`. The pure helpers (broker parse, topic derivation, recipient extraction, inbox resolution, broker-source precedence) are unit-tested with no broker; the `rumqttc` I/O is verified end-to-end against a live broker. No TLS support yet — re-enabling `rumqttc`'s `use-rustls` is the open follow-up.

## License

[MIT](../../LICENSE).
