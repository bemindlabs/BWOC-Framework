//! OpenAI-compatible chat completion types.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request side
// ---------------------------------------------------------------------------

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Present when role == assistant and the model called tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// Present when role == tool (a tool result).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Name for tool result messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    /// Construct a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// Construct a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// Construct an assistant message (may carry tool_calls).
    pub fn assistant(content: Option<String>, tool_calls: Option<Vec<ToolCall>>) -> Self {
        Self {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
            name: None,
        }
    }

    /// Construct a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, result: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(result.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
        }
    }
}

/// Message role.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool the model may call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub kind: String, // always "function"
    pub function: Function,
}

impl Tool {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function".into(),
            function: Function {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

/// Function schema inside a Tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Response side — non-streaming
// ---------------------------------------------------------------------------

/// A non-streaming chat completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletion {
    pub id: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// One choice in a completion response.
#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<FinishReason>,
}

/// Token usage.
#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Why the model stopped.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    #[serde(other)]
    Other,
}

// ---------------------------------------------------------------------------
// Response side — streaming
// ---------------------------------------------------------------------------

/// One SSE data chunk in a streaming response.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub id: String,
    /// Content/tool-call deltas.  Empty on the final usage-only chunk that
    /// OpenAI-compatible providers emit when `stream_options.include_usage` is
    /// set, so it defaults rather than failing to parse.
    #[serde(default)]
    pub choices: Vec<StreamDelta>,
    /// Token usage — present only on the final chunk when the provider supports
    /// usage in streams (`stream_options.include_usage`); `None` on content
    /// chunks.
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// One streaming choice delta.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamDelta {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<FinishReason>,
}

/// The incremental content in a streaming chunk.
#[derive(Debug, Clone, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// Incremental tool call in a streaming delta (index-keyed for accumulation).
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub function: Option<FunctionDelta>,
}

/// Incremental function data in a streaming delta.
#[derive(Debug, Clone, Deserialize)]
pub struct FunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// A fully-assembled tool call from the model (non-streaming or accumulated from stream).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

/// The function name + arguments (JSON string) from a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// The result of executing a tool call, ready to append as a `tool` message.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub call_id: String,
    pub tool_name: String,
    pub content: String,
}
