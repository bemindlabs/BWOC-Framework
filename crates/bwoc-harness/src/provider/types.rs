//! OpenAI-compatible chat completion types.

use bwoc_core::trust::{Principal, TrustLevel};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Request side
// ---------------------------------------------------------------------------

/// A message in the conversation history.
///
/// ## Ingress trust labeling (Phase 5 t1)
///
/// Every message carries an immutable [`Principal`] — *who/what produced its
/// content*. The field is **module-private**: the only way to obtain a
/// `ChatMessage` is through a constructor, and every constructor pins a
/// `Principal`, so the labeling is **compiler-enforced total** — you cannot
/// build an unlabeled message. The lone deserialize gap (a persisted line or a
/// wire input that omits the field) is closed by `#[serde(default)]` →
/// [`Principal::Unknown`] → [`TrustLevel::Untrusted`] (fail-closed).
///
/// `principal` is **retained on disk** (it serializes) so a reloaded session
/// keeps the provenance it had; it is **never sent to the provider** — the
/// request body is built from a separate egress DTO that omits it (see
/// `provider::client`), so no OpenAI-compatible endpoint ever sees the field.
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
    /// Immutable provenance stamp. Retained on disk (legacy lines without it
    /// default to `Unknown` → Untrusted); stripped before egress.
    #[serde(default)]
    principal: Principal,
}

impl ChatMessage {
    /// Construct a system message — the agent's own constitution
    /// ([`Principal::SelfAgent`], Trusted). This is the *only* constructor that
    /// produces `SelfAgent`, which keeps the `role:System ⇔ principal:SelfAgent`
    /// invariant true.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            principal: Principal::SelfAgent,
        }
    }

    /// Construct a user message with **undeclared** provenance
    /// ([`Principal::Unknown`], Untrusted) — the fail-closed default. Any caller
    /// that has not established provenance (e.g. a one-shot/queue task) gets
    /// Untrusted automatically. Use [`Self::operator`] for the trusted local
    /// operator, or [`Self::ingest`] to stamp an explicit external provenance.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            principal: Principal::Unknown,
        }
    }

    /// Construct a user message from the **local operator**
    /// ([`Principal::LocalOperator`], Trusted) — the only trusted ingress.
    pub fn operator(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            principal: Principal::LocalOperator,
        }
    }

    /// Construct a user-role message stamped with an explicit external
    /// [`Principal`] — the typed ingress path for connectors, teammates, MCP,
    /// and A2A. `SelfAgent` is clamped to `Unknown` so external content can
    /// never launder into the agent's own trust (and the System⇔SelfAgent
    /// invariant cannot be broken through this path).
    pub fn ingest(principal: Principal, content: impl Into<String>) -> Self {
        let principal = match principal {
            Principal::SelfAgent => Principal::Unknown,
            other => other,
        };
        Self {
            role: Role::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            principal,
        }
    }

    /// Construct an assistant message (may carry tool_calls). The agent's own
    /// model turn ([`Principal::Assistant`], Untrusted — model output can
    /// reflect untrusted context; a later gate refines this).
    pub fn assistant(content: Option<String>, tool_calls: Option<Vec<ToolCall>>) -> Self {
        Self {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
            name: None,
            principal: Principal::Assistant,
        }
    }

    /// Construct a tool result message — external tool output
    /// ([`Principal::Tool`], Untrusted). `tool_name` records *which* tool
    /// produced the content (provenance only; it does not change the wire
    /// `name` field, kept `None` for OpenAI-compat parity).
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        result: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: Some(result.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
            principal: Principal::Tool {
                name: tool_name.into(),
            },
        }
    }

    /// The immutable provenance of this message.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The derived trust level of this message (never stored).
    pub fn trust(&self) -> TrustLevel {
        self.principal.trust()
    }

    /// True when this message's content is [`TrustLevel::Untrusted`].
    pub fn is_untrusted(&self) -> bool {
        self.principal.is_untrusted()
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

#[cfg(test)]
mod trust_tests {
    use super::*;
    use proptest::prelude::*;

    /// The Phase-5 t1 ingress invariant: a message is `SelfAgent`-principal
    /// **if and only if** it is a `System`-role message. `SelfAgent` (the
    /// agent's own constitution, the highest trust) is reserved exclusively for
    /// the system prompt; nothing else — no assistant turn, no ingested external
    /// content — may wear it, and the system prompt may wear nothing else.
    fn invariant_holds(m: &ChatMessage) -> bool {
        (m.role == Role::System) == (*m.principal() == Principal::SelfAgent)
    }

    #[test]
    fn every_constructor_upholds_system_selfagent_invariant() {
        let msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("u"),
            ChatMessage::operator("op"),
            ChatMessage::assistant(Some("a".into()), None),
            ChatMessage::tool_result("id", "read_file", "out"),
            ChatMessage::ingest(Principal::TeamPeer { agent: "x".into() }, "peer"),
            ChatMessage::ingest(
                Principal::Platform {
                    platform: "telegram".into(),
                    user_id: 7,
                },
                "tg",
            ),
        ];
        for m in &msgs {
            assert!(invariant_holds(m), "invariant broken by {:?}", m.role);
        }
    }

    #[test]
    fn constructors_pin_the_expected_trust() {
        assert_eq!(ChatMessage::system("s").trust(), TrustLevel::Trusted);
        assert_eq!(ChatMessage::operator("o").trust(), TrustLevel::Trusted);
        assert_eq!(ChatMessage::user("u").trust(), TrustLevel::Untrusted);
        assert_eq!(
            ChatMessage::assistant(Some("a".into()), None).trust(),
            TrustLevel::Untrusted
        );
        assert_eq!(
            ChatMessage::tool_result("i", "t", "o").trust(),
            TrustLevel::Untrusted
        );
    }

    #[test]
    fn ingest_clamps_selfagent_so_external_cannot_launder_to_trusted() {
        let m = ChatMessage::ingest(Principal::SelfAgent, "evil");
        assert_eq!(*m.principal(), Principal::Unknown);
        assert_eq!(m.trust(), TrustLevel::Untrusted);
        assert!(invariant_holds(&m));
    }

    #[test]
    fn principal_is_retained_across_disk_round_trip() {
        let m = ChatMessage::tool_result("id", "read_file", "out");
        let line = serde_json::to_string(&m).unwrap();
        let back: ChatMessage = serde_json::from_str(&line).unwrap();
        assert_eq!(
            *back.principal(),
            Principal::Tool {
                name: "read_file".into()
            }
        );
    }

    #[test]
    fn legacy_line_without_principal_loads_as_untrusted() {
        // A session line written before the field existed: fail-closed on load.
        let legacy = r#"{"role":"user","content":"hi"}"#;
        let m: ChatMessage = serde_json::from_str(legacy).unwrap();
        assert_eq!(*m.principal(), Principal::Unknown);
        assert_eq!(m.trust(), TrustLevel::Untrusted);
    }

    proptest! {
        /// Totality fuzz: for arbitrary text, every constructor yields a message
        /// that upholds the invariant and pins the documented trust.
        #[test]
        fn constructor_totality(text in ".*") {
            prop_assert_eq!(ChatMessage::operator(&text).trust(), TrustLevel::Trusted);
            prop_assert_eq!(ChatMessage::user(&text).trust(), TrustLevel::Untrusted);
            prop_assert_eq!(
                ChatMessage::tool_result("id", "tool", &text).trust(),
                TrustLevel::Untrusted
            );
            for m in [
                ChatMessage::system(&text),
                ChatMessage::user(&text),
                ChatMessage::operator(&text),
                ChatMessage::assistant(Some(text.clone()), None),
            ] {
                prop_assert!(invariant_holds(&m));
            }
        }
    }
}
