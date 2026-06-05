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
//! Streaming token deltas (`Token` events), then a final `Message`. No MCP /
//! checkpoint / eval / budget — those belong to the batch `run_loop`, not this
//! interactive driver.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

use crate::error::{HarnessError, HarnessResult};
use crate::policy::permission::{self, Mode};
use crate::policy::{Policy, guardrail_check};
use crate::provider::{ChatMessage, ProviderClient, Role, ToolCall};
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
    /// Context budget (heuristic tokens). When the conversation estimate exceeds
    /// this, the oldest turns are summarized before the next provider call. `0`
    /// disables compaction.
    pub max_context_tokens: usize,
    /// Team chat broadcast log (HV3-3a). `Some(path)` opts this session into a
    /// team's shared `chat.jsonl`: peer messages are injected into context
    /// before each user turn, and this agent's reply is appended after. `None`
    /// (the default) keeps the session solo — no broadcast, no behaviour change.
    pub team_chat_log: Option<PathBuf>,
}

/// Default chat context budget (heuristic tokens) — conservative for the local
/// models these sessions target. Overridable per session via [`ChatConfig`].
pub const DEFAULT_MAX_CONTEXT_TOKENS: usize = 8_000;

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            agent: "agent".to_string(),
            model: "gemma4".to_string(),
            backend: "ollama".to_string(),
            system_prompt: String::new(),
            policy: Policy::default(),
            max_turn_iterations: 20,
            max_context_tokens: DEFAULT_MAX_CONTEXT_TOKENS,
            team_chat_log: None,
        }
    }
}

/// Session-level permission mode, toggled live via [`ChatInput::SetMode`]. It
/// only relaxes `ask` decisions into auto-allow — `deny` rules and guardrails
/// (layer 1) are never affected. Mirrors openclaude's permission modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SessionMode {
    /// Prompt the frontend for every `ask`-mode tool (the safe default).
    #[default]
    Default,
    /// Auto-approve file write/edit tools; still prompt for everything else.
    AcceptEdits,
    /// Auto-approve every `ask`-mode tool (no prompts).
    Bypass,
    /// Planning: read-only tools run, but any **mutating** tool is refused with
    /// a note telling the model to present a plan instead of acting. Switch back
    /// to `default` (or `accept_edits`/`bypass`) to execute. Mirrors openclaude's
    /// plan mode.
    Plan,
}

/// Read-only tools allowed in [`SessionMode::Plan`]. Plan mode uses an
/// **allow-list**, not a deny-list: anything not here (writes, shell, git,
/// task/peer/run delegation, gates, memory writes — and any future tool) is
/// refused so the model observes and plans without acting or spawning processes.
const PLAN_READ_ONLY_TOOLS: &[&str] = &["read_file", "list_dir", "grep", "memory_read"];

impl SessionMode {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "default" => Some(Self::Default),
            "accept_edits" | "acceptedits" => Some(Self::AcceptEdits),
            "bypass" | "full_access" | "dont_ask" => Some(Self::Bypass),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }

    /// The wire string echoed in [`ChatEvent::ModeChanged`].
    fn wire(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "accept_edits",
            Self::Bypass => "bypass",
            Self::Plan => "plan",
        }
    }

    /// Does this mode auto-allow an `ask`-mode `tool` without prompting?
    fn auto_allows(self, tool: &str) -> bool {
        match self {
            Self::Default | Self::Plan => false,
            Self::AcceptEdits => matches!(tool, "edit_file" | "write_file"),
            Self::Bypass => true,
        }
    }

    /// In plan mode, any tool that is not read-only is refused outright (before
    /// the permission gate). Returns the refusal reason the model should see, or
    /// `None` if the tool may proceed to the normal permission flow.
    fn plan_block(self, tool: &str) -> Option<String> {
        if self == Self::Plan && !PLAN_READ_ONLY_TOOLS.contains(&tool) {
            Some(format!(
                "PLAN MODE: `{tool}` is disabled (read-only planning). Present your \
                 proposed plan for approval — do not act. The user will switch off \
                 plan mode to execute."
            ))
        } else {
            None
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

    // Session history: system prompt + every user/assistant/tool message. A
    // persisted conversation (if any) is reloaded so the agent *remembers* across
    // restarts, not just the displayed transcript.
    let session_path = ctx.workdir.join(".bwoc").join("chat-session.json");
    let mut history: Vec<ChatMessage> = vec![ChatMessage::system(&config.system_prompt)];
    history.extend(load_session(&session_path));

    // Cumulative session usage, reported on every TurnEnd.
    let mut prompt_tokens: u64 = 0;
    let mut completion_tokens: u64 = 0;

    // Session permission mode (live-toggled via `SetMode`); only relaxes `ask`.
    let mut session_mode = SessionMode::Default;

    // Team chat broadcast cursor (HV3-3a): how many lines of the shared log
    // this session has already injected. Starts at 0 so the first user turn
    // pulls in any backlog of peer messages already on the log.
    let mut team_seen: usize = 0;

    // Sorted so the `Ready.tools` list is stable across runs (the registry is a
    // HashMap → non-deterministic iteration order).
    let mut tool_names: Vec<String> = tools.iter().map(|t| t.function.name.clone()).collect();
    tool_names.sort();
    emit(
        &mut out,
        &ChatEvent::Ready {
            agent: config.agent.clone(),
            model: config.model.clone(),
            backend: config.backend.clone(),
            tools: tool_names,
        },
    )
    .await?;

    // Replay the restored conversation so the frontend shows it (the model
    // already has it in `history`). Skip the system prompt at [0].
    for msg in history.iter().skip(1) {
        if let Some((role, text)) = restored_display(msg) {
            emit(
                &mut out,
                &ChatEvent::Restored {
                    role: role.to_string(),
                    text,
                },
            )
            .await?;
        }
    }

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
            ChatInput::Forget => {
                // Drop everything but the system prompt and remove the on-disk
                // session — the agent starts fresh.
                history.truncate(1);
                let _ = std::fs::remove_file(&session_path);
            }
            ChatInput::SetMode { mode } => match SessionMode::parse(&mode) {
                Some(m) => {
                    session_mode = m;
                    emit(
                        &mut out,
                        &ChatEvent::ModeChanged {
                            mode: m.wire().to_string(),
                        },
                    )
                    .await?;
                }
                None => {
                    emit(
                        &mut out,
                        &ChatEvent::Error {
                            message: format!(
                                "unknown permission mode `{mode}` \
                                 (use default | accept_edits | bypass)"
                            ),
                        },
                    )
                    .await?;
                }
            },
            ChatInput::User { text } => {
                // Team chat (HV3-3a): pull in any teammate messages posted
                // since the last turn as a system note, before the new user
                // message, so the agent answers with the team's context.
                if let Some(log) = &config.team_chat_log {
                    inject_peer_messages(&mut history, log, &config.agent, &mut team_seen);
                }
                history.push(ChatMessage::user(text));
                // Keep the conversation under the context budget: summarize the
                // oldest turns when needed, before the provider sees them.
                if config.max_context_tokens > 0 {
                    // Unified context engine (HV3-2): summarize-first with a
                    // truncate fallback; folded content is mined into Tier 2
                    // when the manifest configures deepMemoryCmd.
                    let outcome = crate::compact::compact_context(
                        &*provider,
                        &config.model,
                        config.max_context_tokens,
                        &mut history,
                        &ctx.workdir,
                    )
                    .await;
                    let removed = outcome.removed();
                    if removed > 0 {
                        emit(&mut out, &ChatEvent::Compacted { removed }).await?;
                    }
                }
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
                    &mut session_mode,
                )
                .await?;
                // Persist the conversation after the turn settles (incl. tool
                // results) so the next launch resumes with full context.
                save_session(&session_path, &history);
                // Team chat (HV3-3a): broadcast this agent's reply to the
                // shared log so teammates see it on their next turn.
                if let Some(log) = &config.team_chat_log {
                    if let Some(reply) = last_assistant_text(&history) {
                        append_team_message(log, &config.agent, &reply);
                    }
                }
            }
        }
    }

    // stdin EOF without an explicit Quit — end the session cleanly.
    emit(&mut out, &ChatEvent::Bye).await?;
    Ok(())
}

/// Load a persisted conversation (the non-system messages) from `path`. Returns
/// empty on any error — a missing or corrupt session simply starts fresh.
fn load_session(path: &std::path::Path) -> Vec<ChatMessage> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the conversation (everything after the system prompt) to `path`,
/// atomically (temp + rename). Best-effort — a write failure must not break the
/// live chat.
fn save_session(path: &std::path::Path, history: &[ChatMessage]) {
    let convo = history.get(1..).unwrap_or(&[]);
    let Ok(json) = serde_json::to_string(convo) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

// ---------------------------------------------------------------------------
// Team chat broadcast (HV3-3a)
// ---------------------------------------------------------------------------

/// Inject team-chat messages that have appeared since `seen` into `history` as
/// one system note, skipping this agent's own messages, and advance `seen` to
/// the current log length. Best-effort: a missing/unparseable log injects
/// nothing. Returns the number of peer messages injected (for tests/logging).
fn inject_peer_messages(
    history: &mut Vec<ChatMessage>,
    log: &Path,
    agent: &str,
    seen: &mut usize,
) -> usize {
    let Ok(raw) = std::fs::read_to_string(log) else {
        return 0;
    };
    let Ok(msgs) = bwoc_core::team::parse_chat(&raw) else {
        return 0;
    };
    let total = msgs.len();
    if total < *seen {
        // The log is shorter than our cursor — it was rotated/compacted/rewritten
        // out from under us. Treat it as a fresh log and re-read from the start
        // so post-rotation messages aren't silently skipped (re-injecting some
        // already-seen peer context once is harmless; missing new messages is not).
        *seen = 0;
    }
    if total == *seen {
        return 0; // nothing new since the last turn
    }
    let fresh: Vec<&bwoc_core::team::TeamChatMessage> =
        msgs[*seen..].iter().filter(|m| m.from != agent).collect();
    *seen = total;
    if fresh.is_empty() {
        return 0;
    }
    let mut block = String::from(
        "## Team conversation (Saṅgha)\n\nMessages from teammates since your last turn:\n",
    );
    for m in &fresh {
        block.push_str(&format!("\n- **{}**: {}", m.from, m.text));
    }
    history.push(ChatMessage::system(block));
    fresh.len()
}

/// Append this agent's reply to the team chat log (best-effort; creates the
/// parent dir).
///
/// The line (one JSON record + `\n`) is built first and written in a **single**
/// `write_all` to a file opened `O_APPEND`. On a local filesystem an O_APPEND
/// write of a small buffer (a chat line is well under `PIPE_BUF`) lands
/// atomically, so concurrent teammates' lines never interleave mid-record —
/// the same shared-file discipline the task list relies on (a host-layer
/// advisory lock, like `tasks.lock`, is the future hardening for high-contention
/// or network filesystems; one append per human-paced turn doesn't warrant it).
fn append_team_message(log: &Path, agent: &str, text: &str) {
    let Ok(mut line) = bwoc_core::team::TeamChatMessage::new(agent, text).to_line() else {
        return;
    };
    line.push('\n');
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        // One syscall-sized buffer, not a formatting macro that may write in parts.
        let _ = f.write_all(line.as_bytes());
    }
}

/// The final assistant reply of the last turn — the text to broadcast. `None`
/// if the turn produced no non-empty assistant message (e.g. tool-calls only).
fn last_assistant_text(history: &[ChatMessage]) -> Option<String> {
    history.iter().rev().find_map(|m| match &m.content {
        Some(c) if matches!(m.role, Role::Assistant) && !c.trim().is_empty() => Some(c.clone()),
        _ => None,
    })
}

/// The `(role, text)` to replay for a restored message — `Some` only for
/// user/assistant messages that carry visible content (skip the system prompt,
/// tool-result messages, and tool-call-only assistant turns).
fn restored_display(msg: &ChatMessage) -> Option<(&'static str, String)> {
    let content = msg.content.as_ref().filter(|c| !c.is_empty())?;
    match msg.role {
        Role::User => Some(("user", content.clone())),
        Role::Assistant => Some(("assistant", content.clone())),
        _ => None,
    }
}

/// Stream the assistant response, emitting a `Token` event for every content
/// delta as it arrives, and accumulate the full message (content, tool_calls,
/// usage) — the streaming analogue of `provider.complete()`, but the frontend
/// renders tokens live. Mirrors `agent_loop::stream_and_accumulate`, adding the
/// per-delta `Token` emit.
async fn stream_turn<W>(
    provider: &dyn ProviderClient,
    messages: Vec<ChatMessage>,
    tools: Vec<crate::provider::Tool>,
    model: &str,
    out: &mut W,
) -> HarnessResult<(ChatMessage, Option<crate::provider::Usage>)>
where
    W: AsyncWriteExt + Unpin,
{
    use futures_util::StreamExt;

    #[derive(Default)]
    struct Acc {
        id: String,
        kind: String,
        name: String,
        args: String,
    }

    let mut stream = provider.stream(messages, tools, model).await?;
    let mut content = String::new();
    let mut calls: std::collections::HashMap<u32, Acc> = std::collections::HashMap::new();
    let mut usage: Option<crate::provider::Usage> = None;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
        for sd in chunk.choices {
            if let Some(text) = sd.delta.content {
                if !text.is_empty() {
                    // Accumulate by borrow, then move the owned delta into the
                    // event — no per-token clone.
                    content.push_str(&text);
                    emit(out, &ChatEvent::Token { text }).await?;
                }
            }
            if let Some(tcs) = sd.delta.tool_calls {
                for tc in tcs {
                    let acc = calls.entry(tc.index).or_default();
                    if let Some(id) = tc.id {
                        acc.id = id;
                    }
                    if let Some(kind) = tc.r#type {
                        acc.kind = kind;
                    }
                    if let Some(func) = tc.function {
                        if let Some(name) = func.name {
                            acc.name = name;
                        }
                        if let Some(args) = func.arguments {
                            acc.args.push_str(&args);
                        }
                    }
                }
            }
        }
    }

    let tool_calls: Vec<ToolCall> = if calls.is_empty() {
        Vec::new()
    } else {
        let mut sorted: Vec<_> = calls.into_iter().collect();
        sorted.sort_by_key(|(idx, _)| *idx);
        sorted
            .into_iter()
            .map(|(_, a)| ToolCall {
                id: a.id,
                kind: a.kind,
                function: crate::provider::FunctionCall {
                    name: a.name,
                    arguments: a.args,
                },
            })
            .collect()
    };

    // A stream that yielded neither content nor tool calls (e.g. a usage-only
    // chunk, or an early termination) is a provider fault — surface it as an
    // error so the caller emits `Error` + `TurnEnd`, rather than an empty
    // `Message` that masks the failure. (Matches the old empty-completion guard.)
    if content.is_empty() && tool_calls.is_empty() {
        return Err(HarnessError::Provider(
            "provider returned an empty response (no content, no tool calls)".to_string(),
        ));
    }

    let message = ChatMessage::assistant(
        (!content.is_empty()).then_some(content),
        (!tool_calls.is_empty()).then_some(tool_calls),
    );
    Ok((message, usage))
}

/// Run one agentic turn: call the provider, dispatch any tool calls (each
/// through the safety pipeline), and repeat until the assistant returns no tool
/// calls. Emits streamed `Token`s + a final `Message` + `TurnEnd` on success, or
/// `Error` (and returns) on a provider failure — the caller keeps the session
/// alive either way.
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
    session_mode: &mut SessionMode,
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

        // ── Provider call (streaming) — emit `Token` deltas live ─────────────
        let (message, usage) = match stream_turn(
            provider,
            history.clone(),
            tools.to_vec(),
            &config.model,
            out,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                // Recoverable: surface, close the turn, keep the session.
                emit(
                    out,
                    &ChatEvent::Error {
                        message: e.to_string(),
                    },
                )
                .await?;
                emit_turn_end(out, *prompt_tokens, *completion_tokens).await?;
                return Ok(());
            }
        };

        if let Some(usage) = &usage {
            *prompt_tokens += u64::from(usage.prompt_tokens);
            *completion_tokens += u64::from(usage.completion_tokens);
        }

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
            let result = dispatch_call(
                registry,
                ctx,
                &config.policy,
                call,
                lines,
                out,
                session_mode,
            )
            .await?;
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
    session_mode: &mut SessionMode,
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

    // ── Plan mode: refuse mutating tools before the permission gate ──────────
    if let Some(reason) = session_mode.plan_block(name) {
        emit_tool_result(out, call, false, &reason).await?;
        return Ok(reason);
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
            // The session permission mode may auto-approve this `ask` (e.g.
            // accept_edits for write/edit tools, or bypass for everything) —
            // skip the prompt. `deny`/guardrails already handled above, so the
            // mode never widens past `ask`.
            if !session_mode.auto_allows(name) {
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
                let allowed = read_permission(lines, &call.id, session_mode).await?;
                if !allowed {
                    let msg = format!("DENIED by operator: `{name}` was declined");
                    emit_tool_result(out, call, false, &msg).await?;
                    return Ok(msg);
                }
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
///
/// A `SetMode` arriving while the prompt is outstanding is not an answer — it
/// updates `session_mode` in place (so it takes effect for *subsequent* tool
/// calls, exactly as it would between turns) and keeps waiting for the decision.
/// No `ModeChanged` ack is emitted mid-prompt; the change is silent until the
/// next call observes it.
async fn read_permission<R>(
    lines: &mut Lines<R>,
    expect_id: &str,
    session_mode: &mut SessionMode,
) -> HarnessResult<bool>
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
            // A mode switch mid-prompt is applied to `session_mode` (effective
            // for later calls) but does not answer this prompt — keep waiting.
            Ok(ChatInput::SetMode { mode }) => {
                if let Some(m) = SessionMode::parse(&mode) {
                    *session_mode = m;
                }
                continue;
            }
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

/// Emit the `TurnEnd` that closes every `ChatInput::User` turn — success *or*
/// error — so a frontend can treat it as the single "ready for next input"
/// delimiter without ever blocking.
async fn emit_turn_end<W>(
    out: &mut W,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> HarnessResult<()>
where
    W: AsyncWriteExt + Unpin,
{
    emit(
        out,
        &ChatEvent::TurnEnd {
            prompt_tokens,
            completion_tokens,
        },
    )
    .await
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

    #[test]
    fn session_mode_parse_and_auto_allow() {
        assert_eq!(SessionMode::parse("default"), Some(SessionMode::Default));
        assert_eq!(
            SessionMode::parse("accept-edits"),
            Some(SessionMode::AcceptEdits)
        );
        assert_eq!(SessionMode::parse("BYPASS"), Some(SessionMode::Bypass));
        assert_eq!(SessionMode::parse("full_access"), Some(SessionMode::Bypass));
        assert_eq!(SessionMode::parse("nope"), None);

        // Default prompts for everything.
        assert!(!SessionMode::Default.auto_allows("edit_file"));
        // accept_edits auto-allows write/edit only.
        assert!(SessionMode::AcceptEdits.auto_allows("edit_file"));
        assert!(SessionMode::AcceptEdits.auto_allows("write_file"));
        assert!(!SessionMode::AcceptEdits.auto_allows("run_command"));
        // bypass auto-allows anything.
        assert!(SessionMode::Bypass.auto_allows("run_command"));
        assert_eq!(SessionMode::AcceptEdits.wire(), "accept_edits");

        // plan: parses, never auto-allows, allow-lists read-only tools only.
        assert_eq!(SessionMode::parse("plan"), Some(SessionMode::Plan));
        assert_eq!(SessionMode::Plan.wire(), "plan");
        assert!(!SessionMode::Plan.auto_allows("read_file"));
        assert!(SessionMode::Plan.plan_block("write_file").is_some());
        assert!(SessionMode::Plan.plan_block("edit_file").is_some());
        assert!(SessionMode::Plan.plan_block("run_command").is_some());
        // Allow-list: every non-read-only tool is blocked, incl. delegation /
        // gates / git / memory writes (and any future tool).
        assert!(SessionMode::Plan.plan_block("git").is_some());
        assert!(SessionMode::Plan.plan_block("run_gates").is_some());
        assert!(SessionMode::Plan.plan_block("bwoc_task").is_some());
        assert!(SessionMode::Plan.plan_block("bwoc_send").is_some());
        assert!(SessionMode::Plan.plan_block("bwoc_run").is_some());
        assert!(SessionMode::Plan.plan_block("memory_write").is_some());
        // Read-only tools pass.
        assert!(SessionMode::Plan.plan_block("read_file").is_none());
        assert!(SessionMode::Plan.plan_block("list_dir").is_none());
        assert!(SessionMode::Plan.plan_block("grep").is_none());
        assert!(SessionMode::Plan.plan_block("memory_read").is_none());
        // Other modes never plan-block.
        assert!(SessionMode::Default.plan_block("write_file").is_none());
        assert!(SessionMode::Bypass.plan_block("run_command").is_none());
    }

    use crate::provider::types::FunctionCall;
    use crate::provider::types::{FunctionDelta, ToolCallDelta};
    use crate::provider::{
        ChatCompletion, Choice, Delta, FinishReason, StreamChunk, StreamDelta, Tool, Usage,
    };
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
            // Convert the next queued completion into a single stream chunk so
            // the streaming driver sees the same turn the non-streaming path did.
            let mut lock = self.responses.lock().unwrap();
            if lock.is_empty() {
                return Err(HarnessError::Provider("mock exhausted".to_string()));
            }
            let completion = lock.remove(0)?;
            let usage = completion.usage.clone();
            let msg = completion.choices.into_iter().next().map(|c| c.message);
            let content = msg.as_ref().and_then(|m| m.content.clone());
            let tool_calls = msg.and_then(|m| m.tool_calls).unwrap_or_default();
            let tc_deltas: Vec<ToolCallDelta> = tool_calls
                .into_iter()
                .enumerate()
                .map(|(i, tc)| ToolCallDelta {
                    index: i as u32,
                    id: Some(tc.id),
                    r#type: Some(tc.kind),
                    function: Some(FunctionDelta {
                        name: Some(tc.function.name),
                        arguments: Some(tc.function.arguments),
                    }),
                })
                .collect();
            let chunk = StreamChunk {
                id: "mock".to_string(),
                choices: vec![StreamDelta {
                    index: 0,
                    delta: Delta {
                        role: None,
                        content,
                        tool_calls: (!tc_deltas.is_empty()).then_some(tc_deltas),
                    },
                    finish_reason: Some(FinishReason::Stop),
                }],
                usage,
            };
            Ok(Box::pin(futures_util::stream::once(
                async move { Ok(chunk) },
            )))
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
            // Disabled by default so existing scripted tests are unaffected; the
            // compaction tests set their own budget.
            max_context_tokens: 0,
            team_chat_log: None,
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
    async fn session_persists_restores_and_forgets() {
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join(".bwoc/chat-session.json");
        // Run 1: one user turn, then quit — persists the conversation.
        let _ = run_scripted(
            vec![final_response("hello there")],
            allow_all(),
            "{\"type\":\"user\",\"text\":\"hi\"}\n{\"type\":\"quit\"}\n",
            ToolContext::new(tmp.path()),
        )
        .await;
        assert!(session.is_file(), "session should be persisted");

        // Run 2: quit immediately — the prior turn is replayed as `Restored`.
        let lines = run_scripted(
            vec![],
            allow_all(),
            "{\"type\":\"quit\"}\n",
            ToolContext::new(tmp.path()),
        )
        .await;
        let restored: Vec<(String, String)> = parse(&lines)
            .into_iter()
            .filter_map(|e| match e {
                ChatEvent::Restored { role, text } => Some((role, text)),
                _ => None,
            })
            .collect();
        assert_eq!(
            restored,
            vec![
                ("user".to_string(), "hi".to_string()),
                ("assistant".to_string(), "hello there".to_string()),
            ]
        );

        // Run 3: `forget` deletes the on-disk session.
        let _ = run_scripted(
            vec![],
            allow_all(),
            "{\"type\":\"forget\"}\n{\"type\":\"quit\"}\n",
            ToolContext::new(tmp.path()),
        )
        .await;
        assert!(!session.is_file(), "forget should delete the session file");
    }

    #[tokio::test]
    async fn user_message_round_trips_to_final() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let stdin = "{\"type\":\"user\",\"text\":\"hi\"}\n{\"type\":\"quit\"}\n";
        let lines =
            run_scripted(vec![final_response("Hello back!")], allow_all(), stdin, ctx).await;
        let events = parse(&lines);
        // Ready, streamed Token(s), final Message, TurnEnd, Bye.
        assert!(matches!(events[0], ChatEvent::Ready { .. }));
        // The content streams as `Token` deltas (the mock emits it in one chunk).
        let streamed: String = events
            .iter()
            .filter_map(|e| match e {
                ChatEvent::Token { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(streamed, "Hello back!", "tokens should stream the content");
        // A final `Message` with the full text still closes the turn.
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ChatEvent::Message { text } if text == "Hello back!")),
            "got {events:?}"
        );
        assert!(events.iter().any(|e| matches!(
            e,
            ChatEvent::TurnEnd {
                prompt_tokens: 10,
                completion_tokens: 5
            }
        )));
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

    #[tokio::test]
    async fn accept_edits_auto_allows_edit_without_prompt() {
        // write_file is `ask`, but a prior `set_mode accept_edits` auto-approves
        // it: no PermissionRequest, the file is written, the turn completes.
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let mut policy = allow_all();
        policy.tools.insert("write_file".to_string(), Mode::Ask);
        let stdin = concat!(
            "{\"type\":\"set_mode\",\"mode\":\"accept_edits\"}\n",
            "{\"type\":\"user\",\"text\":\"write\"}\n",
            "{\"type\":\"quit\"}\n"
        );
        let lines = run_scripted(
            vec![
                tool_call_response("write_file", r#"{"path":"x.txt","content":"hi"}"#),
                final_response("done"),
            ],
            policy,
            stdin,
            ctx,
        )
        .await;
        let events = parse(&lines);
        // Mode was acked (set between turns).
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ChatEvent::ModeChanged { mode } if mode == "accept_edits"))
        );
        // No prompt was raised — the edit auto-approved.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ChatEvent::PermissionRequest { .. })),
            "accept_edits must not prompt for a write/edit tool"
        );
        assert!(events.iter().any(
            |e| matches!(e, ChatEvent::ToolResult { ok, name, .. } if *ok && name == "write_file")
        ));
        assert!(
            tmp.path().join("x.txt").exists(),
            "the file must be written"
        );
    }

    #[tokio::test]
    async fn set_mode_mid_prompt_applies_to_later_call() {
        // Call 1 (Default) prompts; mid-prompt `set_mode bypass` is applied but
        // does NOT answer the prompt (the following `permission allow` does).
        // Call 2 is then auto-approved by bypass — only ONE PermissionRequest.
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let mut policy = allow_all();
        policy.tools.insert("write_file".to_string(), Mode::Ask);
        let stdin = concat!(
            "{\"type\":\"user\",\"text\":\"write both\"}\n",
            "{\"type\":\"set_mode\",\"mode\":\"bypass\"}\n",
            "{\"type\":\"permission\",\"id\":\"call-1\",\"allow\":true}\n",
            "{\"type\":\"quit\"}\n"
        );
        let lines = run_scripted(
            vec![
                tool_call_response("write_file", r#"{"path":"a.txt","content":"a"}"#),
                tool_call_response("write_file", r#"{"path":"b.txt","content":"b"}"#),
                final_response("done"),
            ],
            policy,
            stdin,
            ctx,
        )
        .await;
        let events = parse(&lines);
        let prompts = events
            .iter()
            .filter(|e| matches!(e, ChatEvent::PermissionRequest { .. }))
            .count();
        assert_eq!(prompts, 1, "only the first call should prompt");
        // No ModeChanged ack is emitted for a mid-prompt switch (documented).
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ChatEvent::ModeChanged { .. })),
            "a mid-prompt set_mode is applied silently (no ack)"
        );
        // Both writes landed — the second via the now-bypass mode.
        assert!(tmp.path().join("a.txt").exists());
        assert!(tmp.path().join("b.txt").exists());
    }

    #[tokio::test]
    async fn plan_mode_blocks_mutating_tool_without_prompt() {
        // In plan mode a write_file is refused outright (no PermissionRequest,
        // no file), with a PLAN MODE result fed back to the model.
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let stdin = concat!(
            "{\"type\":\"set_mode\",\"mode\":\"plan\"}\n",
            "{\"type\":\"user\",\"text\":\"write the file\"}\n",
            "{\"type\":\"quit\"}\n"
        );
        let lines = run_scripted(
            vec![
                tool_call_response("write_file", r#"{"path":"x.txt","content":"hi"}"#),
                final_response("here is my plan instead"),
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
                .any(|e| matches!(e, ChatEvent::ModeChanged { mode } if mode == "plan"))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ChatEvent::PermissionRequest { .. })),
            "plan mode refuses without prompting"
        );
        assert!(events.iter().any(
            |e| matches!(e, ChatEvent::ToolResult { ok, output, .. } if !*ok && output.contains("PLAN MODE"))
        ));
        assert!(
            !tmp.path().join("x.txt").exists(),
            "no file written in plan mode"
        );
    }

    #[tokio::test]
    async fn plan_mode_allows_read_only_tool() {
        // Read-only tools still run in plan mode.
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("note.txt"), "hello")
            .await
            .unwrap();
        let ctx = ToolContext::new(tmp.path());
        let stdin = concat!(
            "{\"type\":\"set_mode\",\"mode\":\"plan\"}\n",
            "{\"type\":\"user\",\"text\":\"read note\"}\n",
            "{\"type\":\"quit\"}\n"
        );
        let lines = run_scripted(
            vec![
                tool_call_response("read_file", r#"{"path":"note.txt"}"#),
                final_response("it says hello"),
            ],
            allow_all(),
            stdin,
            ctx,
        )
        .await;
        let events = parse(&lines);
        assert!(events.iter().any(
            |e| matches!(e, ChatEvent::ToolResult { ok, output, .. } if *ok && output.contains("hello"))
        ));
    }

    #[tokio::test]
    async fn oversized_history_is_compacted_before_turn() {
        // A large persisted conversation + a small context budget triggers
        // compaction on the next user turn: the summarizer call is consumed,
        // a Compacted event is emitted, then the turn proceeds.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bwoc")).unwrap();
        let mut seed: Vec<ChatMessage> = Vec::new();
        for _ in 0..8 {
            seed.push(ChatMessage::user("x".repeat(2000)));
            seed.push(ChatMessage::assistant(Some("y".repeat(2000)), None));
        }
        std::fs::write(
            tmp.path().join(".bwoc/chat-session.json"),
            serde_json::to_string(&seed).unwrap(),
        )
        .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            final_response("SUMMARY of the earlier conversation"), // maybe_compact's complete()
            final_response("ok"),                                  // the turn's stream()
        ]));
        let registry = Arc::new(crate::tools::registry::default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut cfg = config(allow_all());
        cfg.max_context_tokens = 1_000; // force compaction

        let stdin = "{\"type\":\"user\",\"text\":\"hi\"}\n{\"type\":\"quit\"}\n";
        let lines = BufReader::new(stdin.as_bytes()).lines();
        let mut out: Vec<u8> = Vec::new();
        drive(provider, registry, ctx, cfg, lines, &mut out)
            .await
            .unwrap();
        let events: Vec<ChatEvent> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ChatEvent::Compacted { removed } if *removed > 0)),
            "an oversized history should emit Compacted before the turn"
        );
    }

    // ── Team chat broadcast (HV3-3a) ─────────────────────────────────────────

    #[test]
    fn inject_peer_messages_skips_self_and_advances_cursor() {
        use bwoc_core::team::TeamChatMessage;
        let tmp = TempDir::new().unwrap();
        let log = tmp.path().join("chat.jsonl");
        let body = format!(
            "{}\n{}\n",
            TeamChatMessage::new("agent-a", "peer says hi")
                .to_line()
                .unwrap(),
            TeamChatMessage::new("agent-self", "my own earlier line")
                .to_line()
                .unwrap(),
        );
        std::fs::write(&log, body).unwrap();

        let mut history = vec![ChatMessage::system("sys")];
        let mut seen = 0usize;
        let n = inject_peer_messages(&mut history, &log, "agent-self", &mut seen);

        assert_eq!(n, 1, "only the peer message injects, not self");
        assert_eq!(seen, 2, "cursor advances past every line read");
        let note = history.last().unwrap().content.as_deref().unwrap();
        assert!(note.contains("agent-a") && note.contains("peer says hi"));
        assert!(
            !note.contains("my own earlier line"),
            "self message excluded"
        );

        // No new lines → no injection, cursor steady, no extra history entry.
        let len_before = history.len();
        assert_eq!(
            inject_peer_messages(&mut history, &log, "agent-self", &mut seen),
            0
        );
        assert_eq!(history.len(), len_before);
    }

    #[test]
    fn inject_resyncs_when_log_rotated_shorter() {
        use bwoc_core::team::TeamChatMessage;
        let tmp = TempDir::new().unwrap();
        let log = tmp.path().join("chat.jsonl");
        // Two peer messages seen first → cursor at 2.
        std::fs::write(
            &log,
            format!(
                "{}\n{}\n",
                TeamChatMessage::new("agent-a", "one").to_line().unwrap(),
                TeamChatMessage::new("agent-a", "two").to_line().unwrap(),
            ),
        )
        .unwrap();
        let mut history = vec![ChatMessage::system("sys")];
        let mut seen = 0usize;
        assert_eq!(
            inject_peer_messages(&mut history, &log, "agent-self", &mut seen),
            2
        );
        assert_eq!(seen, 2);

        // Log rotated to a single NEW message (len 1 < cursor 2): must re-read
        // from the start, not silently skip it.
        std::fs::write(
            &log,
            format!(
                "{}\n",
                TeamChatMessage::new("agent-a", "post-rotation")
                    .to_line()
                    .unwrap()
            ),
        )
        .unwrap();
        let n = inject_peer_messages(&mut history, &log, "agent-self", &mut seen);
        assert_eq!(n, 1, "post-rotation message must be injected, not skipped");
        assert_eq!(seen, 1);
        assert!(
            history
                .last()
                .unwrap()
                .content
                .as_deref()
                .unwrap()
                .contains("post-rotation")
        );
    }

    #[test]
    fn append_then_peer_sees_it_but_sender_does_not() {
        let tmp = TempDir::new().unwrap();
        let log = tmp.path().join("nested").join("chat.jsonl"); // parent created
        append_team_message(&log, "agent-a", "found the root cause");

        // Peer B injects and sees A's line.
        let mut hist_b = vec![ChatMessage::system("sys")];
        let mut seen_b = 0usize;
        assert_eq!(
            inject_peer_messages(&mut hist_b, &log, "agent-b", &mut seen_b),
            1
        );

        // Sender A injects and sees nothing (its own line is filtered).
        let mut hist_a = vec![ChatMessage::system("sys")];
        let mut seen_a = 0usize;
        assert_eq!(
            inject_peer_messages(&mut hist_a, &log, "agent-a", &mut seen_a),
            0
        );
    }

    #[test]
    fn inject_missing_log_is_noop() {
        let tmp = TempDir::new().unwrap();
        let mut history = vec![ChatMessage::system("sys")];
        let mut seen = 0usize;
        let n = inject_peer_messages(
            &mut history,
            &tmp.path().join("absent.jsonl"),
            "agent-x",
            &mut seen,
        );
        assert_eq!(n, 0);
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn last_assistant_text_picks_final_nonempty_reply() {
        let history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("q"),
            ChatMessage::assistant(Some("first answer".into()), None),
            ChatMessage::user("q2"),
            ChatMessage::assistant(Some("  ".into()), None), // blank — skipped
            ChatMessage::tool_result("c1", "tool output"),
        ];
        assert_eq!(
            last_assistant_text(&history).as_deref(),
            Some("first answer")
        );
        assert!(last_assistant_text(&[ChatMessage::system("only sys")]).is_none());
    }
}
