# bwoc-a2a

A2A (Agent2Agent) protocol interop for the [BWOC framework](../../README.md) — pinned to A2A spec **v1.0.0**.

Lets a BWOC agent talk to non-BWOC agents over the open protocol, in both directions: an inbound axum listener that serves the agent's Agent Card and a JSON-RPC endpoint, and an outbound reqwest client that calls remote A2A agents. Ships both a library and a `bwoc-a2a` binary — the CLI execs that binary as a sibling subprocess so axum/tokio/reqwest never enter [`bwoc-cli`](../bwoc-cli/)'s dependency tree, and [`bwoc-core`](../bwoc-core/) keeps its no-HTTP dep-quarantine.

## Scope

- **`types`** — 1.0.0 wire types (`AgentCard`, `Message`, `Part`, `Task`, `TaskState`, JSON-RPC envelopes) and the `method::*` name constants. camelCase fields, proto-derived enum strings (`ROLE_USER`, `TASK_STATE_SUBMITTED`).
- **`card`** — `card_from_manifest()`: render `/.well-known/agent-card.json` from an agent's `config.manifest.json`.
- **`rpc`** — transport-agnostic `dispatch()`. `SendMessage` appends a BWOC envelope to the recipient's `inbox.jsonl` (dedup by `messageId`, 64 MiB cap); `GetTask`/`ListTasks` read the team task list; `CancelTask` reports that BWOC tasks are not A2A-cancelable; the four push-config methods do CRUD.
- **`serve`** — the axum listener: Agent Card `GET`, JSON-RPC `POST`, and SSE for `SendStreamingMessage`/`SubscribeToTask`. Bearer auth, 1 MiB body cap, global token-bucket rate limit, subscription concurrency cap; a non-loopback bind refuses to start without a token unless overridden.
- **`client`** — outbound `fetch_card()`, `send_message()`, `deliver_push()` (async, reqwest).
- **`tasks`** — maps a Saṅgha team's `tasks.jsonl` (`Pending → InProgress → Completed`) onto A2A tasks. Deliberately lossy-but-honest: the five A2A states with no BWOC equivalent are never synthesized.
- **`push`** — per-task webhook config store (`push-configs.json`, atomic tmp+rename).
- **`ssrf`** — egress guard for webhook delivery: require `https`, reject any resolved loopback/private/CGNAT/link-local/metadata/ULA address, and hand back the validated addresses so the connection can be pinned against DNS rebinding.
- **`creds`** — per-origin outbound bearer tokens from `.bwoc/a2a-credentials.json` (on Unix the file must be `0600` or stricter; group/world-accessible is refused).

## Usage

Normally driven through the CLI, which resolves and execs this binary:

```bash
bwoc a2a card <agent>                        # print the Agent Card JSON
bwoc a2a serve <agent> --team <id>           # listen on 127.0.0.1:41241
bwoc a2a fetch-card https://peer.example     # discover a remote agent
bwoc a2a send https://peer.example/rpc "hi"  # SendMessage to a remote agent
```

As a library within the workspace:

```toml
[dependencies]
bwoc-a2a = { workspace = true }
```

```rust
use bwoc_a2a::card::card_from_manifest;
use bwoc_core::manifest::Manifest;
use std::path::Path;

let manifest = Manifest::load_from_path(Path::new("config.manifest.json"))?;
let card = card_from_manifest(&manifest, "http://127.0.0.1:41241/");
```

## Status

Working end to end: Agent Card discovery, `SendMessage` into the inbox, team `tasks/*`, SSE streaming, push-config CRUD, authenticated webhook delivery behind the SSRF guard, and the outbound client with per-origin credentials. Not implemented: non-text message parts (file/data) are flagged rather than silently dropped, and `CancelTask` always declines — the human lead owns a BWOC task's lifecycle.

## License

[MIT](../../LICENSE).
