//! Tool-call dispatch and streaming accumulation.
//!
//! `execute_tool_calls` runs every tool call in a turn through the full safety
//! pipeline (GUARDRAILS → PERMISSION → SANDBOX → execute), and
//! `stream_and_accumulate` folds a streaming response into one [`ChatMessage`].
//! Both are the loop driver's (`super`) hand-offs to the policy/sandbox/executor
//! stack and the streaming provider path respectively; split out so the driver
//! stays focused on orchestration.

use crate::error::HarnessResult;
use crate::policy::{PolicyOutcome, run_pipeline};
use crate::provider::{ChatMessage, ProviderClient, ToolCall};
use crate::sandbox::make_os_sandbox;
use crate::tools::{ToolContext, ToolRegistry};
use crate::turn_executor::execute_proceeded;
use bwoc_core::trust::TrustLevel;

use super::LoopConfig;

/// Dispatch all tool calls in a turn, passing each through the full safety
/// pipeline: GUARDRAILS → PERMISSION → SANDBOX → execute.
///
/// Two phases: the guardrails→permission *decision* runs SEQUENTIALLY (the
/// permission layer may prompt the operator on a TTY in `ask` mode, and
/// concurrent prompts on one terminal would interleave and could misattribute
/// an approval to the wrong call); the approved calls then SANDBOX→execute
/// CONCURRENTLY (the HV2-7 win — the expensive step parallelises).
///
/// A blocked call returns the blocking reason as the tool result content so
/// the model can adapt.  It is NOT a hard error that stops the loop.
pub(super) async fn execute_tool_calls(
    calls: &[ToolCall],
    registry: &ToolRegistry,
    ctx: &ToolContext,
    config: &LoopConfig,
    turn_trust: TrustLevel,
) -> Vec<ToolCallResult> {
    let os_sandbox = make_os_sandbox(&ctx.workdir);

    // ── Phase 0/1: Capability gate → Guardrails → Permission, SEQUENTIALLY ──
    // Decide every call in order before any execution so an interactive `ask`
    // prompt can't race another call's prompt for the same stdin/terminal. The
    // Layer-0 capability gate (Phase 5 t2) consumes this turn's trust verdict.
    let decisions: Vec<PolicyOutcome> = calls
        .iter()
        .map(|call| {
            run_pipeline(
                &call.function.name,
                &call.function.arguments,
                &ctx.workdir,
                &config.policy,
                config.is_tty,
                turn_trust,
            )
        })
        .collect();

    // ── Phase 2: Sandbox → execute, CONCURRENTLY ────────────────────────────
    // `join_all` preserves input order so results line up with `calls`; each
    // call carries the decision already made for it in phase 1.
    let futures = calls.iter().zip(decisions).map(|(call, outcome)| {
        let os_sandbox = &*os_sandbox;
        async move {
            let tool_name = &call.function.name;
            let args_json = &call.function.arguments;

            let (content, images, denied, capability_denied) = match outcome {
                PolicyOutcome::Proceed => {
                    // ── Layer 3: Process isolation (Phase 5 t5) ──────────────────
                    // Execution of an approved call no longer happens in this
                    // process. `execute_proceeded` re-execs the binary as an
                    // isolated turn-executor child for marshallable (default-
                    // registry) tools — run_command still goes through the
                    // sandboxed runner, but now inside the child (child-of-child).
                    // Un-marshallable (dynamic/MCP/credential) tools are denied
                    // fail-closed; the child never reaches in-parent execution.
                    let r = execute_proceeded(
                        tool_name,
                        args_json,
                        ctx,
                        registry,
                        &*os_sandbox,
                        turn_trust,
                    )
                    .await;
                    (r.content, r.images, r.denied, r.capability_denied)
                }
                PolicyOutcome::CapabilityDenied { tool, reason } => {
                    // C4: structured log line — tool + reason + turn-trust — so a
                    // capability refusal is observable in the harness log, not
                    // only as a tool result fed back to the model.
                    eprintln!(
                        "[bwoc-harness] capability-gate DENY tool=`{tool}` \
                         turn_trust={turn_trust:?} reason=`{reason}`"
                    );
                    let msg = PolicyOutcome::CapabilityDenied { tool, reason }
                        .into_tool_result()
                        .unwrap_or_else(|| "blocked".to_string());
                    (msg, Vec::new(), true, true)
                }
                blocked => {
                    // Feed the denial back to the model as the tool result.
                    let msg = blocked
                        .into_tool_result()
                        .unwrap_or_else(|| "blocked".to_string());
                    (msg, Vec::new(), true, false)
                }
            };

            ToolCallResult {
                call_id: call.id.clone(),
                tool_name: call.function.name.clone(),
                content,
                images,
                denied,
                capability_denied,
            }
        }
    });

    futures_util::future::join_all(futures).await
}

pub(super) struct ToolCallResult {
    pub(super) call_id: String,
    pub(super) tool_name: String,
    pub(super) content: String,
    /// Multimodal images the tool produced (e.g. a screenshot), attached to the
    /// `tool_result` message. Empty for text-only tools and denial paths.
    pub(super) images: Vec<crate::provider::types::ImageBlock>,
    /// Blocked by any policy layer (guardrail / permission / capability gate) —
    /// the content is the refusal fed back to the model.
    pub(super) denied: bool,
    /// Specifically refused by the Layer-0 capability gate (Phase 5 t2). Counted
    /// into `capability_denials`, NOT `denials` (kept mutually exclusive).
    pub(super) capability_denied: bool,
}

/// Stream a response and accumulate content + tool_calls into a single
/// [`ChatMessage`] as if it were a non-streaming completion.
pub(super) async fn stream_and_accumulate(
    provider: &dyn ProviderClient,
    messages: Vec<ChatMessage>,
    tools: Vec<crate::provider::Tool>,
    model: &str,
) -> HarnessResult<(ChatMessage, Option<crate::provider::Usage>)> {
    use futures_util::StreamExt;

    let mut stream = provider.stream(messages, tools, model).await?;

    let mut content_buf = String::new();
    // tool_calls accumulation: index → (id, type, name, args_buf)
    let mut tool_calls_acc: std::collections::HashMap<u32, ToolCallAccumulator> =
        std::collections::HashMap::new();
    // Usage arrives on the final chunk (stream_options.include_usage); keep the
    // last non-empty one (HV2-7 — closes the streaming-usage gap).
    let mut usage: Option<crate::provider::Usage> = None;
    // Thinking blocks arrive as carrier chunks (Anthropic streaming). Preserved
    // verbatim in stream order so the next tool turn can replay them (else 400).
    let mut thinking_blocks: Vec<serde_json::Value> = Vec::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
        if let Some(block) = chunk.thinking_block {
            thinking_blocks.push(block);
        }
        for delta_choice in chunk.choices {
            let delta = delta_choice.delta;

            if let Some(content) = delta.content {
                content_buf.push_str(&content);
            }

            if let Some(tc_deltas) = delta.tool_calls {
                for tc_delta in tc_deltas {
                    let acc = tool_calls_acc.entry(tc_delta.index).or_default();
                    if let Some(id) = tc_delta.id {
                        acc.id = id;
                    }
                    if let Some(kind) = tc_delta.r#type {
                        acc.kind = kind;
                    }
                    if let Some(func) = tc_delta.function {
                        if let Some(name) = func.name {
                            acc.name = name;
                        }
                        if let Some(args) = func.arguments {
                            acc.args_buf.push_str(&args);
                        }
                    }
                }
            }
        }
    }

    // Assemble tool calls if any were accumulated.
    let tool_calls: Vec<ToolCall> = if tool_calls_acc.is_empty() {
        vec![]
    } else {
        let mut sorted: Vec<_> = tool_calls_acc.into_iter().collect();
        sorted.sort_by_key(|(idx, _)| *idx);
        sorted
            .into_iter()
            .map(|(_, acc)| ToolCall {
                id: acc.id,
                kind: acc.kind,
                function: crate::provider::FunctionCall {
                    name: acc.name,
                    arguments: acc.args_buf,
                },
            })
            .collect()
    };

    let mut message = ChatMessage::assistant(
        if content_buf.is_empty() {
            None
        } else {
            Some(content_buf)
        },
        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
    );
    if !thinking_blocks.is_empty() {
        message = message.with_thinking_blocks(thinking_blocks);
    }
    Ok((message, usage))
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    kind: String,
    name: String,
    args_buf: String,
}
