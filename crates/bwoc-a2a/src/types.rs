//! A2A protocol wire types — **pinned to A2A spec v1.0.0**
//! (<https://a2a-protocol.org/latest/specification/>).
//!
//! Field names match the 1.0.0 JSON shapes (camelCase; proto-derived enum
//! strings like `ROLE_USER` / `TASK_STATE_SUBMITTED`). This is the subset P1
//! needs (Agent Card discovery + `SendMessage`); artifacts, push-config, and
//! the remaining task plumbing grow with P2–P5. Verify any field added later
//! against the published schema — the spec evolves (the original design note
//! predated 1.0.0 and used the retired `message/send` naming).

use serde::{Deserialize, Serialize};

/// Standard discovery path for the Agent Card (1.0.0 renamed this from the
/// pre-1.0 `/.well-known/agent.json`).
pub const AGENT_CARD_WELL_KNOWN_PATH: &str = "/.well-known/agent-card.json";

// ── Agent Card ────────────────────────────────────────────────────────────────

/// A2A Agent Card — served at [`AGENT_CARD_WELL_KNOWN_PATH`]. Subset sufficient
/// for discovery + the JSON-RPC interface; more fields (provider, security
/// schemes, signatures) land as later phases need them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    /// Base URL the agent's A2A endpoint is reachable at.
    pub url: String,
    /// Agent (not protocol) version — sourced from the BWOC manifest/build.
    pub version: String,
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: AgentCapabilities,
    #[serde(rename = "defaultInputModes")]
    pub default_input_modes: Vec<String>,
    #[serde(rename = "defaultOutputModes")]
    pub default_output_modes: Vec<String>,
    pub skills: Vec<AgentSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentCapabilities {
    pub streaming: bool,
    #[serde(rename = "pushNotifications")]
    pub push_notifications: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
}

// ── Message + Part ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    #[serde(rename = "ROLE_USER")]
    User,
    #[serde(rename = "ROLE_AGENT")]
    Agent,
}

/// A message Part. v1 handles **text** parts; non-text parts (file/data) are
/// surfaced as a documented limit rather than silently dropped (Musāvāda).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Non-text content (file/data) carried opaquely; v1 does not interpret it.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

impl Part {
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            text: Some(s.into()),
            other: serde_json::Map::new(),
        }
    }
    /// A pure text Part: has `text` and carries no other (file/data) fields.
    /// A part that mixes `text` with non-text fields is treated as non-text so
    /// its extra content is flagged (not silently dropped) — even though the
    /// 1.0.0 spec's Part is a strict oneOf and a conformant peer won't send one.
    pub fn is_text(&self) -> bool {
        self.text.is_some() && self.other.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
    #[serde(rename = "messageId")]
    pub message_id: String,
    #[serde(rename = "taskId", skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(rename = "contextId", skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
}

impl Message {
    /// Concatenate the text Parts (newline-joined). The v1 inbox mapping uses
    /// this; non-text parts are dropped with the documented limit.
    pub fn text_body(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether any non-text part is present (so the caller can warn about the
    /// v1 text-only limit rather than silently dropping content).
    pub fn has_non_text_parts(&self) -> bool {
        self.parts.iter().any(|p| !p.is_text())
    }
}

// ── Task ──────────────────────────────────────────────────────────────────────

/// A2A 1.0.0 task lifecycle states. Mapped to/from BWOC Saṅgha task states in
/// P2 (`tasks/*` ↔ `team.rs`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStatus {
    pub state: TaskState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    #[serde(rename = "contextId")]
    pub context_id: String,
    pub status: TaskStatus,
}

// ── JSON-RPC 2.0 envelope ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    /// `None` ⇒ a JSON-RPC **notification** (the server must not reply).
    /// `Some(Null)` ⇒ an explicit `"id": null`. Conflating the two would let a
    /// notification draw a response, so they stay distinct (`Option`, not a
    /// defaulted `Value`).
    #[serde(default)]
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }
    }
    pub fn err(id: serde_json::Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
            id,
        }
    }
}

/// A2A JSON-RPC method strings (1.0.0). The wire `method` field carries these.
pub mod method {
    pub const SEND_MESSAGE: &str = "SendMessage";
    pub const SEND_STREAMING_MESSAGE: &str = "SendStreamingMessage";
    pub const GET_TASK: &str = "GetTask";
    pub const LIST_TASKS: &str = "ListTasks";
    pub const CANCEL_TASK: &str = "CancelTask";
    pub const SUBSCRIBE_TO_TASK: &str = "SubscribeToTask";
    // Push notification config management (P5). Delivery is deferred to the
    // auth phase; these manage the per-task webhook configs.
    pub const CREATE_TASK_PUSH_CONFIG: &str = "CreateTaskPushNotificationConfig";
    pub const GET_TASK_PUSH_CONFIG: &str = "GetTaskPushNotificationConfig";
    pub const LIST_TASK_PUSH_CONFIGS: &str = "ListTaskPushNotificationConfigs";
    pub const DELETE_TASK_PUSH_CONFIG: &str = "DeleteTaskPushNotificationConfig";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_round_trips_with_1_0_0_field_names() {
        let json = r#"{"role":"ROLE_USER","parts":[{"text":"hello"}],"messageId":"m1"}"#;
        let m: Message = serde_json::from_str(json).unwrap();
        assert_eq!(m.role, Role::User);
        assert_eq!(m.text_body(), "hello");
        assert!(!m.has_non_text_parts());
        // messageId (not message_id) on the wire.
        let back = serde_json::to_string(&m).unwrap();
        assert!(back.contains("\"messageId\":\"m1\""));
        assert!(back.contains("\"role\":\"ROLE_USER\""));
    }

    #[test]
    fn non_text_part_is_flagged_not_dropped_silently() {
        let json = r#"{"role":"ROLE_USER","parts":[{"text":"hi"},{"url":"https://x/y.png","mediaType":"image/png"}],"messageId":"m2"}"#;
        let m: Message = serde_json::from_str(json).unwrap();
        assert_eq!(m.text_body(), "hi"); // text extracted
        assert!(m.has_non_text_parts()); // and the file part is visible to warn on
    }

    #[test]
    fn mixed_text_and_nontext_part_counts_as_non_text() {
        // A part carrying both text and a non-text field is flagged (so its
        // extra content isn't silently dropped) while its text still extracts.
        let json =
            r#"{"role":"ROLE_USER","parts":[{"text":"hi","url":"http://x/y"}],"messageId":"m3"}"#;
        let m: Message = serde_json::from_str(json).unwrap();
        assert_eq!(m.text_body(), "hi");
        assert!(m.has_non_text_parts());
    }

    #[test]
    fn task_state_uses_proto_enum_strings() {
        assert_eq!(
            serde_json::to_string(&TaskState::Submitted).unwrap(),
            "\"TASK_STATE_SUBMITTED\""
        );
        let s: TaskState = serde_json::from_str("\"TASK_STATE_WORKING\"").unwrap();
        assert_eq!(s, TaskState::Working);
    }

    #[test]
    fn well_known_path_is_agent_card_json() {
        assert_eq!(AGENT_CARD_WELL_KNOWN_PATH, "/.well-known/agent-card.json");
    }
}
