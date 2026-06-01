//! Interactive `--chat` session driver (PR1 of the chat TUI).
//!
//! Drives a multi-turn agentic chat over stdin/stdout using the JSON-line
//! protocol in [`bwoc_core::chat_proto`]:
//!
//! - **out** (stdout): one [`ChatEvent`] per line ([`ChatEvent::to_line`]).
//! - **in** (stdin): one [`ChatInput`] per line ([`ChatInput::from_line`]).
//!
//! This is the harness side of the dep-quarantine seam: `bwoc-cli` must not link
//! `bwoc-harness`, so the TUI frontend drives this session as a subprocess.
//!
//! # Reuse, not reinvent
//!
//! The per-turn shape mirrors [`crate::agent_loop::run_loop`]: same provider
//! ([`ProviderClient::complete`]), same [`ToolRegistry`] / [`ToolContext`], and
//! the same safety pipeline — guardrails ([`policy::guardrail_check`]) then the
//! permission policy ([`policy::permission`]). The one difference is `ask`-mode:
//! `run_loop` prompts the controlling TTY, whereas here an `ask` decision is
//! routed to the frontend via a [`ChatEvent::PermissionRequest`] and answered
//! with a [`ChatInput::Permission`].
//!
//! # Scope (v1)
//!
//! Non-streaming only. No MCP / checkpoint / eval / budget — those belong to the
//! batch `run_loop`, not this interactive driver.

use std::sync::Arc;

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

use crate::error::{HarnessError, HarnessResult};
use crate::policy::permission::{self, Mode};
use crate::policy::{Policy, guardrail_check};
use crate::provider::{ChatMessage, ProviderClient, ToolCall};
use crate::tools::registry::dispatch;
use crate::tools::{ToolContext, ToolRegistry};

/// Everything the driver needs to run a session, mirroring the locals
/// `main.rs::run()` assembles for the batch path.
pub struct ChatConfig {
    /// Agent id, for the [`ChatEvent::Ready`] status line.
    pub agent: String,
    /// Model identifier passed to the provider on every turn.
    pub model: String,
    /// Backend label (e.g. `"ollama"`), for the [`ChatEvent::Ready`] status line.
    pub backend: String,
    /// System prompt loaded from `AGENTS.md` / `CLAUDE.md`.
    pub system_prompt: String,
    /// Permission policy (`.bwoc/harness-policy.toml` or fail-safe default).
    pub policy: Policy,
    /// Max agentic sub-turns (provider calls) per user message before the turn
    /// is force-ended. Guards against a tool-call loop that never converges.
    pub max_turn_iterations: u32,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            agent: "agent".to_string(),
            model: "gemma4".to_string(),
            backend: "ollama".to_string(),
            system_prompt: String::new(),
            policy: Policy::default(),
            max_turn_iterations: 20,
        }
    }
}

/// Run an interactive chat session against real stdin/stdout.
///
/// Reads [`ChatInput`] lines from stdin and writes [`ChatEvent`] lines to
/// stdout until [`ChatInput::Quit`] (or EOF) ends the session.
pub async fn run(
    provider: Arc<dyn ProviderClient>,
    registry: Arc<ToolRegistry>,
    ctx: ToolContext,
    config: ChatConfig,
) -> HarnessResult<()> {
    let stdin = BufReader::new(tokio::io::stdin()).lines();
    let stdout = tokio::io::stdout();
    drive(provider, registry, ctx, config, stdin, stdout).await
}

/// The IO-generic session loop — the testable core.
///
/// `lines` yields stdin lines; `out` receives serialized events. Splitting this
/// from [`run`] lets a unit test feed a scripted `&[u8]` and capture the event
/// stream without touching real stdio (mirrors the mock-provider pattern in
/// `agent_loop` tests).
async fn drive<R, W>(
    provider: Arc<dyn ProviderClient>,
    registry: Arc<ToolRegistry>,
    ctx: ToolContext,
    config: ChatConfig,
    mut lines: Lines<R>,
    mut out: W,
) -> HarnessResult<()>
where
    R: AsyncBufReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let tools = registry.tool_schemas();

    // Session history: system prompt + every user/assistant/tool message.
    let mut history: Vec<ChatMessage> = vec![ChatMessage::system(&config.system_prompt)];

    // Cumulative session usage, reported on every TurnEnd.
    let mut prompt_tokens: u64 = 0;
    let mut completion_tokens: u64 = 0;

    emit(
        &mut out,
        &ChatEvent::Ready {
            agent: config.agent.clone(),
            model: config.model.clone(),
            backend: config.backend.clone(),
        },
    )
    .await?;

    while let Some(line) = lines.next_line().await.map_err(HarnessError::Io)? {
        let line = line.trim();
        if line.is_empty() {
            continue; // blank lines are not valid input (per chat_proto contract)
        }
        let input = match ChatInput::from_line(line) {
            Ok(i) => i,
            Err(e) => {
                emit(
                    &mut out,
                    &ChatEvent::Error {
                        message: format!("malformed input line: {e}"),
                    },
                )
                .await?;
                continue;
            }
        };

        match input {
            ChatInput::Quit => {
                emit(&mut out, &ChatEvent::Bye).await?;
                return Ok(());
            }
            ChatInput::Permission { .. } => {
                // A stray permission answer with no outstanding request. Surface
                // it rather than silently dropping; the session stays alive.
                emit(
                    &mut out,
                    &ChatEvent::Error {
                        message: "unexpected permission answer (no pending request)".to_string(),
                    },
                )
                .await?;
            }
            ChatInput::User { text } => {
                history.push(ChatMessage::user(text));
                run_turn(
                    &*provider,
                    &registry,
                    &ctx,
                    &config,
                    &tools,
                    &mut history,
                    &mut lines,
                    &mut out,
                    &mut prompt_tokens,
                    &mut completion_tokens,
                )
                .await?;
            }
        }
    }

    // stdin EOF without an explicit Quit — end the session cleanly.
    emit(&mut out, &ChatEvent::Bye).await?;
    Ok(())
}

/// Run one agentic turn: call the provider, dispatch any tool calls (each
/// through the safety pipeline), and repeat until the assistant returns no tool
/// calls. Emits `Message` + `TurnEnd` on success, or `Error` (and returns) on a
/// provider failure — the caller keeps the session alive either way.
#[allow(clippy::too_many_arguments)]
async fn run_turn<R, W>(
    provider: &dyn ProviderClient,
    registry: &ToolRegistry,
    ctx: &ToolContext,
    config: &ChatConfig,
    tools: &[crate::provider::Tool],
    history: &mut Vec<ChatMessage>,
    lines: &mut Lines<R>,
    out: &mut W,
    prompt_tokens: &mut u64,
    completion_tokens: &mut u64,
) -> HarnessResult<()>
where
    R: AsyncBufReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut iterations = 0u32;
    loop {
        iterations += 1;
        if iterations > config.max_turn_iterations {
            emit(
                out,
                &ChatEvent::Error {
                    message: format!(
                        "turn exceeded {} iterations without a final answer",
                        config.max_turn_iterations
                    ),
                },
            )
            .await?;
            emit(
                out,
                &ChatEvent::TurnEnd {
                    prompt_tokens: *prompt_tokens,
                    completion_tokens: *completion_tokens,
                },
            )
            .await?;
            return Ok(());
        }

        // ── Provider call (non-streaming) ────────────────────────────────────
        let completion = match provider
            .complete(history.clone(), tools.to_vec(), &config.model)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                // Recoverable: surface and end the turn; the session continues.
                emit(
                    out,
                    &ChatEvent::Error {
                        message: e.to_string(),
                    },
                )
                .await?;
                return Ok(());
            }
        };

        if let Some(usage) = &completion.usage {
            *prompt_tokens += u64::from(usage.prompt_tokens);
            *completion_tokens += u64::from(usage.completion_tokens);
        }

        let Some(choice) = completion.choices.into_iter().next() else {
            emit(
                out,
                &ChatEvent::Error {
                    message: "provider returned empty choices".to_string(),
                },
            )
            .await?;
            return Ok(());
        };
        let message = choice.message;
        let tool_calls = message.tool_calls.clone().unwrap_or_default();

        if tool_calls.is_empty() {
            // Final answer for this turn.
            let final_text = message.content.clone().unwrap_or_default();
            history.push(message);
            emit(out, &ChatEvent::Message { text: final_text }).await?;
            emit(
                out,
                &ChatEvent::TurnEnd {
                    prompt_tokens: *prompt_tokens,
                    completion_tokens: *completion_tokens,
                },
            )
            .await?;
            return Ok(());
        }

        // Append the assistant(tool_calls) message before its results (OpenAI
        // ordering), then run each call through the pipeline.
        history.push(message);
        for call in &tool_calls {
            let result = dispatch_call(registry, ctx, &config.policy, call, lines, out).await?;
            history.push(ChatMessage::tool_result(call.id.clone(), result));
        }
        // Loop: feed the tool results back for the next provider call.
    }
}

/// Pass one tool call through GUARDRAILS → PERMISSION, then dispatch it.
///
/// Returns the string that becomes the `tool` result message (a denial reason
/// when blocked — fed back to the model exactly like `run_loop` does, never a
/// hard error). Emits `ToolCall` / `ToolResult`, and on an `ask`-mode tool the
/// `PermissionRequest` + blocking read of the matching `Permission` answer.
#[allow(clippy::too_many_arguments)]
async fn dispatch_call<R, W>(
    registry: &ToolRegistry,
    ctx: &ToolContext,
    policy: &Policy,
    call: &ToolCall,
    lines: &mut Lines<R>,
    out: &mut W,
) -> HarnessResult<String>
where
    R: AsyncBufReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let name = &call.function.name;
    let args = &call.function.arguments;

    // ── Layer 1: Guardrails (non-overridable) ────────────────────────────────
    if let Err(violation) = guardrail_check(name, args, &ctx.workdir) {
        let msg = format!(
            "BLOCKED by safety guardrail [{rule}]: {reason}",
            rule = violation.rule,
            reason = violation.reason,
        );
        emit_tool_result(out, call, false, &msg).await?;
        return Ok(msg);
    }

    // ── Layer 2: Permission ──────────────────────────────────────────────────
    // Resolve the bare mode so `ask` can be routed to the frontend rather than
    // the TTY that `permission::evaluate` assumes.
    match permission::resolve_effective_mode(policy, name, args) {
        Mode::Allow => {}
        Mode::Deny => {
            // Re-run evaluate to reuse its reason string (pattern/tool/default).
            let msg = match permission::evaluate(policy, name, args, false) {
                permission::PermissionDecision::Deny { reason } => {
                    format!("DENIED by permission policy: {reason}")
                }
                permission::PermissionDecision::Allow => {
                    // Shouldn't happen (mode was Deny), but stay safe.
                    "DENIED by permission policy".to_string()
                }
            };
            emit_tool_result(out, call, false, &msg).await?;
            return Ok(msg);
        }
        Mode::Ask => {
            // Route to the frontend: emit a request, block for the answer.
            emit(
                out,
                &ChatEvent::PermissionRequest {
                    id: call.id.clone(),
                    tool: name.clone(),
                    detail: args.clone(),
                },
            )
            .await?;
            let allowed = read_permission(lines, &call.id).await?;
            if !allowed {
                let msg = format!("DENIED by operator: `{name}` was declined");
                emit_tool_result(out, call, false, &msg).await?;
                return Ok(msg);
            }
        }
    }

    // ── Dispatch (approved) ──────────────────────────────────────────────────
    emit(
        out,
        &ChatEvent::ToolCall {
            id: call.id.clone(),
            name: name.clone(),
            args: args.clone(),
        },
    )
    .await?;
    let output = dispatch(registry, name, args, ctx).await;
    // The registry/dispatch convention: an "error:"-prefixed string is a failed
    // tool. Surface that as ok=false so the frontend can render it distinctly,
    // while still feeding the same text back to the model as the tool result.
    let ok = !output.starts_with("error:");
    emit_tool_result(out, call, ok, &output).await?;
    Ok(output)
}

/// Block reading stdin until the frontend answers the pending permission
/// request for `expect_id`. Skips blank lines; a `Quit` mid-prompt is treated as
/// a deny so the call does not execute. A mismatched id is also a deny (the
/// frontend answered the wrong request — fail safe).
async fn read_permission<R>(lines: &mut Lines<R>, expect_id: &str) -> HarnessResult<bool>
where
    R: AsyncBufReadExt + Unpin,
{
    while let Some(line) = lines.next_line().await.map_err(HarnessError::Io)? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match ChatInput::from_line(line) {
            Ok(ChatInput::Permission { id, allow }) => {
                return Ok(allow && id == expect_id);
            }
            Ok(ChatInput::Quit) => return Ok(false),
            // A User message or a malformed line mid-prompt: fail safe (deny)
            // rather than silently dropping it or executing unapproved.
            _ => return Ok(false),
        }
    }
    // EOF before an answer: deny.
    Ok(false)
}

/// Serialize and write one event as a single JSON line, then flush.
async fn emit<W>(out: &mut W, event: &ChatEvent) -> HarnessResult<()>
where
    W: AsyncWriteExt + Unpin,
{
    let line = event
        .to_line()
        .map_err(|e| HarnessError::Other(format!("serialize ChatEvent: {e}")))?;
    out.write_all(line.as_bytes())
        .await
        .map_err(HarnessError::Io)?;
    out.write_all(b"\n").await.map_err(HarnessError::Io)?;
    out.flush().await.map_err(HarnessError::Io)?;
    Ok(())
}

/// Convenience for the common `ToolResult` emit.
async fn emit_tool_result<W>(
    out: &mut W,
    call: &ToolCall,
    ok: bool,
    output: &str,
) -> HarnessResult<()>
where
    W: AsyncWriteExt + Unpin,
{
    emit(
        out,
        &ChatEvent::ToolResult {
            id: call.id.clone(),
            name: call.function.name.clone(),
            ok,
            output: output.to_string(),
        },
    )
    .await
}

// ---------------------------------------------------------------------------
// Tests (offline — mock provider, scripted stdin)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::FunctionCall;
    use crate::provider::{ChatCompletion, Choice, FinishReason, StreamChunk, Tool, Usage};
    use async_trait::async_trait;
    use futures_util::Stream;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // ── Mock provider (mirrors agent_loop tests) ─────────────────────────────

    struct MockProvider {
        responses: Mutex<Vec<Result<ChatCompletion, HarnessError>>>,
    }

    impl MockProvider {
        fn new(responses: Vec<ChatCompletion>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            }
        }
    }

    #[async_trait]
    impl ProviderClient for MockProvider {
        async fn complete(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<Tool>,
            _model: &str,
        ) -> Result<ChatCompletion, HarnessError> {
            let mut lock = self.responses.lock().unwrap();
            if lock.is_empty() {
                return Err(HarnessError::Provider("mock exhausted".to_string()));
            }
            lock.remove(0)
        }

        async fn stream(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<Tool>,
            _model: &str,
        ) -> Result<
            Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>,
            HarnessError,
        > {
            Err(HarnessError::Provider("mock: stream unused".to_string()))
        }

        async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
            Ok(())
        }
    }

    fn final_response(content: &str) -> ChatCompletion {
        ChatCompletion {
            id: "mock".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(Some(content.to_string()), None),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
        }
    }

    fn tool_call_response(name: &str, args: &str) -> ChatCompletion {
        ChatCompletion {
            id: "mock".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(
                    None,
                    Some(vec![ToolCall {
                        id: "call-1".to_string(),
                        kind: "function".to_string(),
                        function: FunctionCall {
                            name: name.to_string(),
                            arguments: args.to_string(),
                        },
                    }]),
                ),
                finish_reason: Some(FinishReason::ToolCalls),
            }],
            usage: None,
        }
    }

    fn allow_all() -> Policy {
        Policy {
            default_mode: Mode::Allow,
            tools: HashMap::new(),
            patterns: Vec::new(),
        }
    }

    fn config(policy: Policy) -> ChatConfig {
        ChatConfig {
            agent: "agent-test".to_string(),
            model: "mock".to_string(),
            backend: "mock".to_string(),
            system_prompt: "You are a test agent.".to_string(),
            policy,
            max_turn_iterations: 5,
        }
    }

    /// Run the driver over a scripted stdin and return the emitted lines.
    async fn run_scripted(
        responses: Vec<ChatCompletion>,
        policy: Policy,
        stdin: &str,
        ctx: ToolContext,
    ) -> Vec<String> {
        let provider = Arc::new(MockProvider::new(responses));
        let registry = Arc::new(crate::tools::registry::default_registry());
        let lines = BufReader::new(stdin.as_bytes()).lines();
        let mut out: Vec<u8> = Vec::new();
        drive(provider, registry, ctx, config(policy), lines, &mut out)
            .await
            .unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|s| s.to_string())
            .collect()
    }

    fn parse(lines: &[String]) -> Vec<ChatEvent> {
        lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn quit_ends_session() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let lines = run_scripted(vec![], allow_all(), "{\"type\":\"quit\"}\n", ctx).await;
        let events = parse(&lines);
        assert!(matches!(events.first(), Some(ChatEvent::Ready { .. })));
        assert!(matches!(events.last(), Some(ChatEvent::Bye)));
        // No turn ran (no provider responses consumed).
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn user_message_round_trips_to_final() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let stdin = "{\"type\":\"user\",\"text\":\"hi\"}\n{\"type\":\"quit\"}\n";
        let lines =
            run_scripted(vec![final_response("Hello back!")], allow_all(), stdin, ctx).await;
        let events = parse(&lines);
        // Ready, Message, TurnEnd, Bye.
        assert!(matches!(events[0], ChatEvent::Ready { .. }));
        assert!(
            matches!(&events[1], ChatEvent::Message { text } if text == "Hello back!"),
            "got {:?}",
            events[1]
        );
        assert!(
            matches!(
                events[2],
                ChatEvent::TurnEnd {
                    prompt_tokens: 10,
                    completion_tokens: 5
                }
            ),
            "got {:?}",
            events[2]
        );
        assert!(matches!(events.last(), Some(ChatEvent::Bye)));
    }

    #[tokio::test]
    async fn tool_call_turn_emits_call_and_result() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("note.txt"), "secret")
            .await
            .unwrap();
        let ctx = ToolContext::new(tmp.path());
        let stdin = "{\"type\":\"user\",\"text\":\"read it\"}\n{\"type\":\"quit\"}\n";
        let lines = run_scripted(
            vec![
                tool_call_response("read_file", r#"{"path":"note.txt"}"#),
                final_response("It says secret"),
            ],
            allow_all(),
            stdin,
            ctx,
        )
        .await;
        let events = parse(&lines);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ChatEvent::ToolCall { name, .. } if name == "read_file"))
        );
        assert!(events.iter().any(
            |e| matches!(e, ChatEvent::ToolResult { ok, output, .. } if *ok && output.contains("secret"))
        ));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ChatEvent::Message { text } if text == "It says secret"))
        );
    }

    #[tokio::test]
    async fn ask_mode_emits_permission_request_and_denies_on_false() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        // write_file is `ask`; the frontend answers allow=false → denied.
        let mut policy = allow_all();
        policy.tools.insert("write_file".to_string(), Mode::Ask);
        let stdin = concat!(
            "{\"type\":\"user\",\"text\":\"write\"}\n",
            "{\"type\":\"permission\",\"id\":\"call-1\",\"allow\":false}\n",
            "{\"type\":\"quit\"}\n"
        );
        let lines = run_scripted(
            vec![
                tool_call_response("write_file", r#"{"path":"x.txt","content":"hi"}"#),
                final_response("ok, skipped"),
            ],
            policy,
            stdin,
            ctx,
        )
        .await;
        let events = parse(&lines);
        assert!(events.iter().any(
            |e| matches!(e, ChatEvent::PermissionRequest { tool, .. } if tool == "write_file")
        ));
        // Denied → ToolResult ok=false; file must NOT exist.
        assert!(events.iter().any(
            |e| matches!(e, ChatEvent::ToolResult { ok, output, .. } if !*ok && output.contains("DENIED"))
        ));
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[tokio::test]
    async fn provider_error_keeps_session_alive() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        // No responses queued → first complete() returns "mock exhausted" error;
        // the driver emits Error, then the next User turn succeeds.
        let stdin = concat!(
            "{\"type\":\"user\",\"text\":\"first\"}\n",
            "{\"type\":\"user\",\"text\":\"second\"}\n",
            "{\"type\":\"quit\"}\n"
        );
        let lines = run_scripted(
            vec![final_response("second answer")],
            allow_all(),
            stdin,
            ctx,
        )
        .await;
        let events = parse(&lines);
        assert!(events.iter().any(|e| matches!(e, ChatEvent::Error { .. })));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ChatEvent::Message { text } if text == "second answer"))
        );
        assert!(matches!(events.last(), Some(ChatEvent::Bye)));
    }
}
