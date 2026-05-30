//! `bwoc-harness` — BWOC self-hosted agentic harness.
//!
//! Provides an OpenAI-compatible agentic loop that lets self-hosted models
//! (Ollama, vLLM, LM Studio, llama.cpp server) act as full BWOC coding agents.
//!
//! # Phasing
//! - **P1:** core loop + tool set + streaming, single task, inside a worktree.
//! - **P2 (this increment):** safety — guardrails + sandbox + permission system.
//!   The three-layer pipeline (GUARDRAILS → PERMISSION → SANDBOX → execute)
//!   is wired into every tool dispatch in `agent_loop`.
//! - **P3:** task queue + telemetry + tool authentication.
//! - **P4:** eval framework + hardening.
//! - **P5:** backend wiring (`bwoc spawn --backend ollama`).
//!
//! # Safety pipeline (P2)
//!
//! ```text
//! GUARDRAILS (Sīla 5 + Taṇhā 3 — non-overridable hard floor)
//!   → PERMISSION (per-tool allow|ask|deny from .bwoc/harness-policy.toml)
//!     → SANDBOX (worktree confinement + env scrub + arg scan)
//!       → tool execute
//! ```
//!
//! Denied calls are fed back to the model as tool results — not hard errors.
//!
//! # Architecture
//! See `notes/2026-05-23_ollama-agentic-harness-design.md` for the full
//! design rationale and phasing decisions.

pub mod agent_loop;
pub mod budget;
pub mod checkpoint;
pub mod error;
pub mod eval;
pub mod lead;
pub mod mcp;
pub mod model_select;
pub mod policy;
pub mod provider;
pub mod queue;
pub mod retrospective;
pub mod sandbox;
pub mod telemetry;
pub mod tools;
pub mod worker;
