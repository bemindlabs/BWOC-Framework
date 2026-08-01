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
pub mod cgroup;
pub mod chat_session;
pub mod checkpoint;
pub mod compact;
pub mod deep_memory;
pub mod error;
pub mod eval;
pub mod jail;
pub mod lead;
pub mod mcp;
pub mod model_select;
pub mod policy;
pub mod provider;
pub mod queue;
pub mod result;
pub mod retrospective;
pub mod review;
pub mod sandbox;
pub mod seccomp;
pub mod session_trust;
pub mod telemetry;
pub mod tools;
pub mod turn_executor;
pub mod worker;

/// Control tokens that models emit as **plain text** when they attempt a tool
/// call the API layer didn't turn into a structured `tool_calls` field (weak or
/// mis-templated models). Each is a tool-call *syntax marker* that never occurs
/// in normal prose or a legitimate JSON answer, so matching them is a
/// low-false-positive signal that a call was dropped. See
/// [`looks_like_text_tool_call`].
const TEXT_TOOL_CALL_MARKERS: &[&str] = &[
    "<tool_call>",    // Qwen, Hermes, many chat templates
    "</tool_call>",   //   (closing tag, in case the opener was streamed off)
    "<|python_tag|>", // Llama 3.1 tool-call prefix
    "<function=",     // Llama 3.1 `<function=name>{...}</function>`
    "[TOOL_CALLS]",   // Mistral / Mixtral tool-call token
    "functools[",     // Firefunction / Fireworks
];

/// Whether `content` looks like a tool call the model emitted as plain text
/// instead of a structured `tool_calls` field — i.e. it contains one of the
/// unambiguous [`TEXT_TOOL_CALL_MARKERS`]. Deliberately conservative: it keys on
/// tool-call *control tokens*, not on "looks like JSON", so a legitimate JSON
/// answer never trips it. Callers only **warn** on a hit — they never try to
/// parse or execute the text (injection/ambiguity risk).
pub(crate) fn looks_like_text_tool_call(content: &str) -> bool {
    TEXT_TOOL_CALL_MARKERS
        .iter()
        .any(|marker| content.contains(marker))
}

#[cfg(test)]
mod text_tool_call_tests {
    use super::looks_like_text_tool_call;

    #[test]
    fn detects_known_control_markers() {
        for s in [
            "<tool_call>{\"name\":\"read_file\"}</tool_call>",
            "sure — <|python_tag|>read_file(path=\"x\")",
            "<function=read_file>{\"path\":\"x\"}</function>",
            "[TOOL_CALLS] read_file",
            "functools[{\"name\":\"read_file\"}]",
        ] {
            assert!(looks_like_text_tool_call(s), "should flag: {s}");
        }
    }

    #[test]
    fn does_not_flag_prose_or_legit_json() {
        // Conservative: a normal answer, or a genuine JSON payload the user asked
        // for, must not trip the warning (no control markers present).
        for s in [
            "The file contains three functions.",
            "Here is the config: {\"name\": \"anna\", \"port\": 8080}",
            "Use `git status` to check the tree.",
            "",
        ] {
            assert!(!looks_like_text_tool_call(s), "should NOT flag: {s}");
        }
    }
}
