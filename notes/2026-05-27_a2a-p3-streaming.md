# 2026-05-27 — A2A P3: SSE streaming

Adds the two A2A streaming methods over Server-Sent Events: `SendStreamingMessage`
and `SubscribeToTask`. Fourth slice of #48 (after P1-core #71, P1-serve #72,
P2 #73/#74).

## What changed

- **`serve.rs`** — the JSON-RPC endpoint now branches on the streaming methods
  and returns an SSE (`text/event-stream`) response instead of one JSON body:
  - **`SendStreamingMessage`** — delivers the message to the inbox via the exact
    unary `SendMessage` path, then emits **one** SSE event (the delivery ack)
    and closes.
  - **`SubscribeToTask`** — tails the exposed team's task: emits a
    `TaskStatusUpdateEvent` for the current state, then one more on each state
    change, closing (`final: true`) on `Completed` or after `SUBSCRIBE_MAX`
    (300 s; poll every 1 s). Pre-flight failures (no team / unknown task) answer
    with a unary JSON-RPC `-32001`, not an empty stream.
- **`card.rs`** — `capabilities.streaming = true`.
- Deps (bwoc-a2a only): `async-stream`, `futures-core`.

## Decisions

- **`SendStreamingMessage` is an honest single-event stream.** BWOC processes
  messages asynchronously — the agent reads its inbox out-of-band, so there are
  no incremental progress events to stream within the request. Rather than fake
  `WORKING`→`COMPLETED` events (Musāvāda), it delivers + acks + closes. The
  async limit is documented, not papered over. (User-chosen design.)
- **`SubscribeToTask` is the genuinely streaming method.** Watching a Saṅgha
  task progress is real incremental state, so it polls `tasks.jsonl` and emits
  on change. This is where SSE earns its place.
- **Bounded stream lifetime.** `SUBSCRIBE_MAX` (300 s) caps an open subscription
  so a never-completing task can't pin a connection forever — the streaming
  analogue of the inbox/body-size caps. Network-exposed resource guard.
- **Poll read runs off the executor (`spawn_blocking`).** The subscription's
  per-second `tasks.jsonl` read is a blocking syscall; run inline on the
  current-thread runtime it would stall the listener and every other live stream
  during each read (surfaced in review). `tokio::task::spawn_blocking` moves it
  to the blocking pool so the executor stays free. A read/parse error is treated
  as terminal (stream closes as Completed) — deliberate, since BWOC never
  synthesizes Failed/Canceled and an unreadable task is indistinguishable from
  completion-then-deletion. Per-peer concurrency caps wait for the auth phase.
- **Streaming stays at the transport layer.** `rpc::dispatch` (transport-agnostic,
  JSON-only) still returns `-32601` for the streaming methods with a message to
  use the SSE transport; only `serve` produces SSE. Keeps `rpc` axum-free.
- **SSE event shape.** Each `data:` line is a JSON-RPC response (`{jsonrpc,id,result}`)
  whose `result` is the streamed item — the ack `Message` for send, a
  `{taskId,contextId,kind:"status-update",status:{state},final}` event for
  subscribe.

## Alternatives considered

- *Fake progress events for `SendStreamingMessage`.* Rejected — dishonest about
  the async model; a peer would see `WORKING` for work that hasn't started.
- *`inotify`/file-watch instead of polling for `SubscribeToTask`.* Rejected for
  P3 — a 1 s poll is simple, portable, and adequate for a human-paced task list;
  no new platform-specific dep.

## Status / deferred

- Done: both streaming methods, `streaming: true`, 28 `bwoc-a2a` tests (incl.
  HTTP-layer SSE: single-event send, completed-task subscribe → one final event,
  unary error on missing/no-team), live curl SSE smoke test.
- Deferred: P4 outbound client (reqwest), P5 push notifications. Artifact-update
  events + `tasks/resubscribe` history replay are future polish; the dep-quarantine
  (axum/tokio/async-stream isolated to the `bwoc-a2a` binary) is unchanged —
  `bwoc-cli` verified HTTP-free.

## Related

- Epic #48. Builds on #71/#72/#73/#74.
- `crates/bwoc-a2a/src/serve.rs` (stream handlers), `card.rs`.
