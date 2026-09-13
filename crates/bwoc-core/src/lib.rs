//! BWOC framework — shared types.
//!
//! The types every BWOC binary agrees on: the agent manifest, the workspace
//! registry, lifecycle and routing, inbox/outbox envelopes, trust, and the
//! on-disk [`schema`] versioning seam they all share.

pub mod chat_proto;
pub mod deep_memory;
pub mod design;
pub mod doc_kind;
pub mod env_scrub;
pub mod error;
pub mod exec;
pub mod idempotency;
pub mod identity;
pub mod inbox;
pub mod ipc;
pub mod lifecycle;
pub mod loop_control;
pub mod manifest;
pub mod outbox;
pub mod routing;
pub mod schema;
pub mod team;
pub mod time;
pub mod trust;
pub mod workspace;
