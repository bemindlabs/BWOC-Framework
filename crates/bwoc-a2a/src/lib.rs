//! A2A (Agent2Agent) protocol interop for BWOC — **pinned to A2A spec v1.0.0**
//! (epic #48). Lets BWOC agents interoperate with non-BWOC agents over the open
//! A2A protocol.
//!
//! Phased delivery (#48):
//! - **P1-core** — protocol core: 1.0.0 wire [`types`], Agent [`card`]
//!   generation from the manifest, and JSON-RPC [`rpc`] dispatch handling
//!   `SendMessage` → the recipient's BWOC inbox. Transport-agnostic + tested.
//! - **P1-serve** (here) — the [`serve`] axum listener: Agent Card at the
//!   well-known path + a JSON-RPC endpoint, bound loopback-only by default
//!   (no auth yet). Driven by `bwoc a2a card` / `bwoc a2a serve`.
//! - **P2** (here) — [`tasks`] bridges A2A `GetTask`/`ListTasks` to a team's
//!   Saṅgha task list; `CancelTask` honestly reports BWOC tasks aren't
//!   A2A-cancelable. Selected with `bwoc a2a serve --team <id>`.
//! - **P3** — streaming (`SendStreamingMessage`, SSE).
//! - **P4** (here) — outbound [`client`]: BWOC initiates A2A calls to an
//!   external endpoint (fetch its Agent Card, `SendMessage`). Driven by
//!   `bwoc a2a fetch-card`/`send`.
//! - **P5** (here) — [`push`] notification **config** management
//!   (`CreateTaskPushNotificationConfig` + Get/List/Delete). Webhook *delivery*
//!   is deferred to the auth phase (an SSRF/exfil egress under no-auth).
//! - **AP3** (here) — webhook **delivery**: when auth is on, the [`serve`]
//!   listener runs a watcher that POSTs `TaskStatusUpdateEvent`s to registered
//!   webhooks, guarded by the [`ssrf`] egress filter.
//!
//! HTTP deps (axum listener, reqwest client) stay isolated to this crate —
//! `bwoc-core` keeps the dep-quarantine (no HTTP).

pub mod card;
pub mod client;
pub mod push;
pub mod rpc;
pub mod serve;
pub mod ssrf;
pub mod tasks;
pub mod types;
