//! Model provider clients behind the injectable [`ProviderClient`] trait.
//!
//! - [`client`] — OpenAI-compatible HTTP (`POST /v1/chat/completions` with
//!   `tools` + `tool_calls`, `stream=false`/`true`). Backs `ollama` and
//!   `openai-compatible` agents.
//! - [`anthropic`] — Anthropic Messages API (`POST /v1/messages`), translated
//!   into the same OpenAI-shaped [`types`] so the chat/agent loops are unchanged.
//!   Backs `claude` agents.
//!
//! The trait makes the HTTP transport injectable so unit tests do NOT require a
//! live endpoint.  Any test that hits a real endpoint must be `#[ignore]`d.

pub mod anthropic;
pub mod client;
pub mod types;

pub use anthropic::AnthropicClient;
pub use client::{OllamaClient, ProviderClient};
pub use types::{
    ChatCompletion, ChatMessage, Choice, Delta, FinishReason, Function, FunctionCall, Role,
    StreamChunk, StreamDelta, Tool, ToolCall, ToolCallResult, Usage,
};
