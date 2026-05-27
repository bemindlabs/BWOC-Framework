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
//! - **P4** — outbound client (BWOC → external A2A endpoint).
//! - **P5** — push notifications + remaining task states.
//!
//! HTTP deps (axum here; reqwest in P4) stay isolated to this crate —
//! `bwoc-core` keeps the dep-quarantine (no HTTP).

pub mod card;
pub mod rpc;
pub mod serve;
pub mod tasks;
pub mod types;
