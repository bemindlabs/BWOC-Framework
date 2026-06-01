//! Interactive chat protocol — the JSON-line wire format between a `bwoc-harness`
//! `--chat` session and a frontend (the `bwoc chat --tui` ratatui client today;
//! a web frontend tomorrow).
//!
//! Why a protocol and not an in-process call: `bwoc-cli` must not depend on
//! `bwoc-harness` (the dep-quarantine — `bwoc-core` stays lean and `bwoc`'s
//! build stays free of the runtime graph). So the frontend drives the harness
//! as a subprocess: one JSON object per line, harness↔frontend.
//!
//! - Frontend → harness: [`ChatInput`] on the harness's **stdin**.
//! - Harness → frontend: [`ChatEvent`] on the harness's **stdout**.
//!
//! Both are internally tagged (`{"type":"token","text":"…"}`) so the stream is
//! self-describing and forward-compatible (unknown variants can be skipped).
//! Pure `serde` — no new dependencies, safe for the lean core crate.

use serde::{Deserialize, Serialize};

/// An event emitted by the harness session (one per stdout line).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// Session is initialized and waiting for the first [`ChatInput::User`].
    /// `agent`/`model`/`backend` let the frontend render a status line; `tools`
    /// lists the registered tool names available to the agent (for a `/tools`
    /// view). `#[serde(default)]` so an older harness that omits it still parses.
    Ready {
        agent: String,
        model: String,
        backend: String,
        #[serde(default)]
        tools: Vec<String>,
    },
    /// A streaming assistant token delta (only when streaming is on).
    Token { text: String },
    /// A complete assistant message for this turn (always sent at turn end,
    /// so non-streaming frontends can render without accumulating `Token`s).
    Message { text: String },
    /// The model requested a tool call (after any permission gate passed).
    ToolCall {
        id: String,
        name: String,
        args: String,
    },
    /// A tool finished. `ok=false` means it errored (the text is the error).
    ToolResult {
        id: String,
        name: String,
        ok: bool,
        output: String,
    },
    /// An `ask`-mode tool needs operator approval. The frontend must reply with
    /// [`ChatInput::Permission`] carrying the same `id` before the turn proceeds.
    PermissionRequest {
        id: String,
        tool: String,
        /// Human-readable summary of what the tool will do (args preview).
        detail: String,
    },
    /// The assistant turn is complete; the session is ready for the next
    /// [`ChatInput::User`]. Usage counts are cumulative for the session.
    TurnEnd {
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    /// A recoverable error surfaced to the user (the session stays alive).
    Error { message: String },
    /// The session is exiting (after [`ChatInput::Quit`] or a fatal error).
    Bye,
}

/// An input the frontend sends to the harness session (one per stdin line).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatInput {
    /// The user's next message / task for the agent.
    User { text: String },
    /// The answer to a pending [`ChatEvent::PermissionRequest`] (same `id`).
    Permission { id: String, allow: bool },
    /// End the session.
    Quit,
}

impl ChatEvent {
    /// Serialize as a single newline-free JSON line (caller appends `\n`).
    /// Infallible in practice — these types always serialize — but returns a
    /// `Result` so a caller never has to `unwrap` on the hot path.
    pub fn to_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

impl ChatInput {
    pub fn to_line(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Parse one stdin line into a [`ChatInput`]. Blank lines are not valid
    /// input; the caller should skip them before calling this.
    pub fn from_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_roundtrips_tagged() {
        let e = ChatEvent::ToolCall {
            id: "t1".into(),
            name: "write_file".into(),
            args: r#"{"path":"x"}"#.into(),
        };
        let line = e.to_line().unwrap();
        assert!(line.contains(r#""type":"tool_call""#));
        let back: ChatEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn input_roundtrips_tagged() {
        for inp in [
            ChatInput::User { text: "hi".into() },
            ChatInput::Permission {
                id: "p1".into(),
                allow: true,
            },
            ChatInput::Quit,
        ] {
            let line = inp.to_line().unwrap();
            assert_eq!(ChatInput::from_line(&line).unwrap(), inp);
        }
    }

    #[test]
    fn permission_request_pairs_with_decision_by_id() {
        let req = ChatEvent::PermissionRequest {
            id: "abc".into(),
            tool: "run_command".into(),
            detail: "rm -rf build/".into(),
        };
        let line = req.to_line().unwrap();
        // The frontend echoes the id back in its decision.
        let ans = ChatInput::Permission {
            id: "abc".into(),
            allow: false,
        };
        assert!(line.contains("abc"));
        assert!(ans.to_line().unwrap().contains("abc"));
    }

    #[test]
    fn ready_without_tools_defaults_to_empty() {
        // Wire-compat: an older harness emits `ready` without `tools`; it must
        // still parse, defaulting the field to `[]` (the `#[serde(default)]`).
        let line = r#"{"type":"ready","agent":"a","model":"m","backend":"ollama"}"#;
        let ev: ChatEvent = serde_json::from_str(line).unwrap();
        assert_eq!(
            ev,
            ChatEvent::Ready {
                agent: "a".into(),
                model: "m".into(),
                backend: "ollama".into(),
                tools: vec![],
            }
        );
    }

    #[test]
    fn snake_case_wire_names_are_stable() {
        // These strings are the wire contract — guard against rename drift.
        assert!(
            ChatEvent::Bye
                .to_line()
                .unwrap()
                .contains(r#""type":"bye""#)
        );
        assert!(
            ChatInput::Quit
                .to_line()
                .unwrap()
                .contains(r#""type":"quit""#)
        );
    }
}
