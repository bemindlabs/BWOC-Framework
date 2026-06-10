//! Model provider clients behind the injectable [`ProviderClient`] trait.
//!
//! - [`client`] — OpenAI-compatible HTTP (`POST /v1/chat/completions` with
//!   `tools` + `tool_calls`, `stream=false`/`true`). Backs `ollama` and
//!   `openai-compatible` agents.
//! - [`anthropic`] — Anthropic Messages API (`POST /v1/messages`), translated
//!   into the same OpenAI-shaped [`types`] so the chat/agent loops are unchanged.
//!   Backs `claude` agents.
//! - [`cli`] — local subscription-authenticated vendor CLI (`claude -p`, …),
//!   one subprocess per turn, **no API key**. Chat-only (the CLI runs its own
//!   tools internally). Backs `cli` agents (#277).
//!
//! The trait makes the HTTP transport injectable so unit tests do NOT require a
//! live endpoint.  Any test that hits a real endpoint must be `#[ignore]`d.

pub mod anthropic;
pub mod cli;
pub mod client;
pub mod types;

pub use anthropic::AnthropicClient;
pub use cli::CliClient;
pub use client::{OllamaClient, ProviderClient};
pub use types::{
    ChatCompletion, ChatMessage, Choice, Delta, FinishReason, Function, FunctionCall, Role,
    StreamChunk, StreamDelta, Tool, ToolCall, ToolCallResult, Usage,
};
