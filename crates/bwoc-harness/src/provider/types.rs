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
    /// Anthropic extended-thinking blocks (raw `thinking` / `redacted_thinking`
    /// JSON, incl. `signature`) produced on an assistant turn. Preserved so the
    /// **same-model** next turn can replay them unchanged — the Messages API
    /// requires the thinking block that precedes a `tool_use` to be present when
    /// the following `tool_result` is sent, or it 400s. Retained on disk;
    /// **stripped before egress** on OpenAI-compat (Anthropic-specific, and the
    /// egress DTO omits it), and only re-emitted by the Anthropic request
    /// builder. `None` for every non-Anthropic / non-thinking turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<serde_json::Value>>,
    /// Multimodal image inputs riding with this message (e.g. a screenshot in a
    /// tool result, or an image in a user turn). Each is base64-encoded with its
    /// media type. Retained on disk; emitted provider-neutrally — Anthropic
    /// `image` blocks on the native path, OpenAI `image_url` data-URI parts on
    /// the OpenAI-compat path. `None` for the text-only common case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageBlock>>,
    /// Immutable provenance stamp. Retained on disk (legacy lines without it
    /// default to `Unknown` → Untrusted); stripped before egress.
    #[serde(default)]
    principal: Principal,
}

/// A base64-encoded image carried on a [`ChatMessage`] for multimodal input.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageBlock {
    /// IANA media type, e.g. `image/png`.
    pub media_type: String,
    /// Base64-encoded image bytes (no `data:` prefix; the provider builders add
    /// the wire wrapper each backend expects).
    pub data: String,
}

impl ChatMessage {
    /// Construct a system message — the agent's own constitution
    /// ([`Principal::SelfAgent`], Trusted). This is the *only* constructor that
    /// produces `SelfAgent`, which keeps the `role:System ⇔ principal:SelfAgent`
    /// invariant true.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            thinking_blocks: None,
            images: None,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            principal: Principal::SelfAgent,
        }
    }

    /// Construct a compaction **summary** note (gate t3).
    ///
    /// Folded as a `System`-role message (so the provider sees it as a system
    /// note, exactly as the pre-t3 summary was), but stamped
    /// [`Principal::Summary`] rather than [`Principal::SelfAgent`].
    /// `folded_untrusted` is the **max-taint** of the folded window — `true` iff
    /// any folded message carried untrusted taint — so summarizing an Untrusted
    /// window can no longer launder it into the agent's own trust. A tainted
    /// summary therefore carries untrusted taint
    /// ([`Principal::carries_untrusted_taint`]); a clean one does not. This is
    /// the only constructor producing a non-`SelfAgent` `System` message — the
    /// reason the t3 invariant is `SelfAgent ⇒ System` (one-way), not `⇔`.
    pub fn summary(content: impl Into<String>, folded_untrusted: bool) -> Self {
        Self {
            role: Role::System,
            thinking_blocks: None,
            images: None,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            principal: Principal::Summary {
                tainted: folded_untrusted,
            },
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
            thinking_blocks: None,
            images: None,
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
            thinking_blocks: None,
            images: None,
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
            thinking_blocks: None,
            images: None,
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
            thinking_blocks: None,
            images: None,
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
            thinking_blocks: None,
            images: None,
            content: Some(result.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: None,
            principal: Principal::Tool {
                name: tool_name.into(),
            },
        }
    }

    /// Attach Anthropic extended-thinking blocks to an assistant message so the
    /// same-model next turn can replay them (empty → left as `None`). Chainable.
    pub fn with_thinking_blocks(mut self, blocks: Vec<serde_json::Value>) -> Self {
        self.thinking_blocks = (!blocks.is_empty()).then_some(blocks);
        self
    }

    /// Attach multimodal image inputs to this message (empty → left as `None`).
    /// Chainable. The provider request builders render them per-backend.
    pub fn with_images(mut self, images: Vec<ImageBlock>) -> Self {
        self.images = (!images.is_empty()).then_some(images);
        self
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

/// OpenAI-compat `prompt_tokens_details` — the nested object carrying
/// prompt-cache accounting on providers that report it (OpenAI, DeepSeek, …).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

/// OpenAI-compat `completion_tokens_details` — carries the reasoning-token
/// count on reasoning models (OpenAI o-series, DeepSeek-R1, Qwen, …).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
}

/// Token usage.
///
/// The three top-level counts are the OpenAI-compat baseline. The optional
/// fields capture modern accounting that older code dropped: prompt-cache
/// read/write and reasoning tokens. OpenAI-compat reports cache/reasoning
/// **nested** (`*_tokens_details`) — serde populates those directly; Anthropic
/// reports cache tokens **flat** on the message usage, so its parser sets
/// [`Self::cache_read_tokens`] / [`Self::cache_creation_tokens`] by hand. Read
/// cache-**read** and reasoning tokens provider-agnostically via
/// [`Self::cached_tokens`] / [`Self::reasoning_tokens`]; cache-**write** tokens
/// are Anthropic-only (no OpenAI-compat equivalent) — read
/// [`Self::cache_creation_tokens`] directly.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// OpenAI-compat nested cache accounting (`prompt_tokens_details`).
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    /// OpenAI-compat nested reasoning accounting (`completion_tokens_details`).
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
    /// Anthropic `cache_read_input_tokens` (flat). `None` on OpenAI-compat.
    #[serde(default)]
    pub cache_read_tokens: Option<u32>,
    /// Anthropic `cache_creation_input_tokens` (flat). `None` on OpenAI-compat.
    #[serde(default)]
    pub cache_creation_tokens: Option<u32>,
}

impl Usage {
    /// Prompt-cache read tokens, whichever way the provider reported them.
    pub fn cached_tokens(&self) -> Option<u32> {
        self.cache_read_tokens.or_else(|| {
            self.prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)
        })
    }

    /// Reasoning/thinking tokens counted in the output, when reported.
    pub fn reasoning_tokens(&self) -> Option<u32> {
        self.completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens)
    }
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
    /// A **completed** Anthropic thinking / redacted_thinking block (raw JSON,
    /// incl. `signature`), emitted once when the streamed block closes so the
    /// accumulator can preserve it on the assistant message for same-model
    /// replay. `None` on OpenAI-compat and on every non-thinking chunk. Never
    /// sent by any provider — this is the harness's own stream-carrier field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_block: Option<serde_json::Value>,
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

    /// The Phase-5 trust/role invariant, **relaxed at t3** (condition b).
    ///
    /// t1 enforced the biconditional `System ⇔ SelfAgent`. t3 must let a
    /// compaction summary wear a `System` role while being a [`Principal::Summary`]
    /// (not `SelfAgent`), so the biconditional is split into two one-way rules:
    ///
    /// 1. `SelfAgent ⇒ System` — the constitution's highest trust is still
    ///    reserved exclusively for system-role messages; nothing ingested or
    ///    model-derived may wear `SelfAgent`. ([`ChatMessage::system`] is still
    ///    the only constructor that produces `SelfAgent`.)
    /// 2. `System ⇒ {SelfAgent | Summary}` — the only two principals allowed on a
    ///    system-role message are the constitution and a compaction summary note.
    ///    `Summary` is the ONLY principal (besides `SelfAgent`) that may add a
    ///    `System` role.
    fn invariant_holds(m: &ChatMessage) -> bool {
        let selfagent_implies_system =
            *m.principal() != Principal::SelfAgent || m.role == Role::System;
        let system_implies_selfagent_or_summary = m.role != Role::System
            || matches!(
                m.principal(),
                Principal::SelfAgent | Principal::Summary { .. }
            );
        selfagent_implies_system && system_implies_selfagent_or_summary
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
            // t3: a summary wears System + Summary (both taint flags).
            ChatMessage::summary("clean fold", false),
            ChatMessage::summary("dirty fold", true),
        ];
        for m in &msgs {
            assert!(invariant_holds(m), "invariant broken by {:?}", m.role);
        }
    }

    #[test]
    fn summary_note_is_system_role_but_not_selfagent() {
        // The relaxed t3 invariant in action: a summary is a System message that
        // is NOT the constitution, and its trust follows its taint flag for the
        // scan while staying fail-closed Untrusted at the trust() boundary.
        let dirty = ChatMessage::summary("folded untrusted window", true);
        assert_eq!(dirty.role, Role::System);
        assert!(matches!(
            dirty.principal(),
            Principal::Summary { tainted: true }
        ));
        assert_ne!(*dirty.principal(), Principal::SelfAgent);
        assert_eq!(dirty.trust(), TrustLevel::Untrusted);
        assert!(invariant_holds(&dirty));

        let clean = ChatMessage::summary("folded trusted-only window", false);
        // Even a clean summary is never Trusted (forge-proof), but it carries no
        // taint for the scan.
        assert_eq!(clean.trust(), TrustLevel::Untrusted);
        assert!(!clean.principal().carries_untrusted_taint());
        assert!(invariant_holds(&clean));
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
