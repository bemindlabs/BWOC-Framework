//! A2A (Agent2Agent) protocol interop for BWOC — **pinned to A2A spec v1.0.0**
//! (epic #48). Lets BWOC agents interoperate with non-BWOC agents over the open
//! A2A protocol.
//!
//! Phased delivery (#48):
//! - **P1** (here) — protocol core: 1.0.0 wire [`types`], Agent [`card`]
//!   generation from the manifest, and JSON-RPC [`rpc`] dispatch handling
//!   `SendMessage` → the recipient's BWOC inbox. Transport-agnostic + tested.
//! - **P2** — `tasks/*` ↔ Saṅgha task states (`bwoc-core::team`).
//! - **P3** — streaming (`SendStreamingMessage`, SSE).
//! - **P4** — outbound client (BWOC → external A2A endpoint).
//! - **P5** — push notifications + remaining task states.
//!
//! HTTP/network deps (axum listener, reqwest client) are added with the
//! `serve`/client phases and stay isolated to this crate — `bwoc-core` keeps
//! the dep-quarantine (no HTTP).

pub mod card;
pub mod rpc;
pub mod types;
