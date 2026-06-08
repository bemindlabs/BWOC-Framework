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

use crate::trust::Principal;
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
    /// A prior conversation turn replayed on connect to restore a persisted
    /// session, so the frontend can show the history (the harness has already
    /// reloaded it into the model's context). Sent after [`Ready`], before the
    /// session accepts new input. `role` is `"user"` or `"assistant"`.
    ///
    /// [`Ready`]: ChatEvent::Ready
    Restored { role: String, text: String },
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
    /// Acknowledges a [`ChatInput::SetMode`]: the session permission mode is now
    /// `mode` (`"default"` | `"accept_edits"` | `"bypass"`). Sent once on change
    /// so the frontend can reflect it (e.g. a status indicator).
    ModeChanged { mode: String },
    /// The session compacted its context: `removed` older messages were folded
    /// into a single summary to stay under the model's context window. Emitted
    /// just before the turn proceeds, so the frontend can show a notice. Purely
    /// informational — the conversation continues seamlessly.
    Compacted { removed: usize },
    /// The assistant turn is complete; the session is ready for the next
    /// [`ChatInput::User`]. Usage counts are cumulative for the session.
    TurnEnd {
        prompt_tokens: u64,
        completion_tokens: u64,
    },
    /// A teammate's message from the shared team chat log (HV3-3a), surfaced so
    /// the frontend can render it distinctly from this agent's own turns.
    /// Emitted once per previously-unseen peer message before a turn proceeds;
    /// only when the session joined a team channel (`--team-chat`). Purely
    /// informational — the agent already has the same text in its context.
    TeamMessage {
        from: String,
        text: String,
        ts: String,
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
    ///
    /// `User` is the **only external-content carrier** among the inputs (the
    /// audit below proves the rest carry no content that reaches the model).
    /// `principal` is its Phase-5 t1 ingress label: the trusted local TUI stamps
    /// [`Principal::LocalOperator`]; chat connectors omit it, so it defaults
    /// (via `#[serde(default)]`) to [`Principal::Unknown`] → Untrusted —
    /// fail-closed for unauthenticated platform ingress.
    User {
        text: String,
        #[serde(default)]
        principal: Principal,
    },
    /// The answer to a pending [`ChatEvent::PermissionRequest`] (same `id`).
    Permission { id: String, allow: bool },
    /// Forget the persisted conversation — clears the in-memory history back to
    /// the system prompt and deletes the on-disk session file.
    Forget,
    /// Switch the session permission mode. `mode` is one of `"default"` (prompt
    /// for every `ask`-mode tool), `"accept_edits"` (auto-approve file
    /// write/edit tools, still prompt for the rest), or `"bypass"` (auto-approve
    /// every `ask`-mode tool). Hard `deny` rules and guardrails are unaffected.
    /// The harness replies with [`ChatEvent::ModeChanged`].
    SetMode { mode: String },
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
            ChatInput::User {
                text: "hi".into(),
                principal: Principal::LocalOperator,
            },
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
    fn set_mode_and_mode_changed_roundtrip() {
        let inp = ChatInput::SetMode {
            mode: "accept_edits".into(),
        };
        assert_eq!(ChatInput::from_line(&inp.to_line().unwrap()).unwrap(), inp);
        let ev = ChatEvent::ModeChanged {
            mode: "bypass".into(),
        };
        let line = ev.to_line().unwrap();
        assert!(line.contains(r#""type":"mode_changed""#));
        assert_eq!(serde_json::from_str::<ChatEvent>(&line).unwrap(), ev);
    }

    #[test]
    fn team_message_roundtrips() {
        let ev = ChatEvent::TeamMessage {
            from: "agent-a".into(),
            text: "found the root cause".into(),
            ts: "2026-06-06T00:00:00Z".into(),
        };
        let line = ev.to_line().unwrap();
        assert!(line.contains(r#""type":"team_message""#));
        assert_eq!(serde_json::from_str::<ChatEvent>(&line).unwrap(), ev);
    }

    #[test]
    fn compacted_roundtrips() {
        let ev = ChatEvent::Compacted { removed: 12 };
        let line = ev.to_line().unwrap();
        assert!(line.contains(r#""type":"compacted""#));
        assert_eq!(serde_json::from_str::<ChatEvent>(&line).unwrap(), ev);
    }

    #[test]
    fn user_input_without_principal_defaults_to_untrusted() {
        // Phase 5 t1, C3: a connector (or any older frontend) that omits the
        // principal must deserialize fail-closed to Unknown → Untrusted.
        let line = r#"{"type":"user","text":"do the thing"}"#;
        let inp: ChatInput = serde_json::from_str(line).unwrap();
        match inp {
            ChatInput::User { principal, .. } => {
                assert_eq!(principal, Principal::Unknown);
                assert_eq!(principal.trust(), crate::trust::TrustLevel::Untrusted);
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn local_operator_principal_round_trips_on_the_wire() {
        let inp = ChatInput::User {
            text: "hi".into(),
            principal: Principal::LocalOperator,
        };
        let back = ChatInput::from_line(&inp.to_line().unwrap()).unwrap();
        assert_eq!(back, inp);
    }

    /// C3 audit: every non-`User` `ChatInput` variant is control-plane only —
    /// it carries no free-text content that ever enters the model's context, so
    /// it needs no trust label. This test is the executable record of that audit;
    /// adding a content-bearing field to any of these variants breaks it,
    /// forcing a re-audit (and a principal, if the field is external content).
    #[test]
    fn non_user_inputs_are_control_plane_carry_no_model_content() {
        // `Permission`: an echoed id + a bool decision — no model content.
        // `SetMode`: a fixed-vocabulary mode string — control-plane, not context.
        // `Forget` / `Quit`: unit — no payload at all.
        for inp in [
            ChatInput::Permission {
                id: "p1".into(),
                allow: true,
            },
            ChatInput::SetMode {
                mode: "bypass".into(),
            },
            ChatInput::Forget,
            ChatInput::Quit,
        ] {
            // They round-trip without ever naming a principal/provenance tag.
            let line = inp.to_line().unwrap();
            assert!(
                !line.contains("principal") && !line.contains("\"kind\""),
                "non-User input unexpectedly carries provenance: {line}"
            );
            assert_eq!(ChatInput::from_line(&line).unwrap(), inp);
        }
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
