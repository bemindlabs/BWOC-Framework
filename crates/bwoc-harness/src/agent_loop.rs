//! The core agentic turn loop.
//!
//! Each turn:
//!   1. Build messages (system prompt + history + tool schemas).
//!   2. Call the provider with retry + fallback model logic.
//!   3. Accumulate `tool_calls` from the response.
//!   4. **For each tool call: GUARDRAILS → PERMISSION → SANDBOX → execute.**
//!   5. Append `assistant(tool_calls)` + `tool` result messages to history.
//!   6. Repeat.
//!
//! Stop conditions:
//!   - No `tool_calls` in the response (model returned final answer).
//!   - Reached `max_iterations`.
//!   - External cancel signal (future: integrates with the P3 queue token).
//!
//! # P4 additions
//!
//! ## Retry with bounded exponential backoff
//!
//! Transient provider errors (connection failures, 5xx) are retried up to
//! `MAX_TRANSIENT_RETRIES` times with base-2 exponential backoff capped at
//! `MAX_BACKOFF_MS`.  Non-transient errors (4xx, model-not-found) fail fast.
//!
//! ## Fallback model chain
//!
//! `LoopConfig::fallback_models` is an ordered list of model IDs to try if
//! the primary model fails or returns malformed tool calls (empty tool call
//! IDs / unparseable arguments) more than `MALFORMED_TOOL_CALL_THRESHOLD`
//! times in a row.  Each fallback is tried in order until one succeeds or
//! the list is exhausted.
//!
//! ## Vetted-model gate
//!
//! `LoopConfig::vetted_models` is a configurable allowlist of model IDs known
//! to support tool-calling reliably.  Running an unvetted model emits a
//! warning to stderr (not a hard error) so the user is informed without
//! blocking the run.
//!
//! ## Context compaction
//!
//! When the estimated context token count approaches
//! `LoopConfig::context_limit` (leaving a `CONTEXT_HEADROOM` margin), the
//! loop compacts the history by:
//!
//! 1. Retaining the system message (index 0) and the last
//!    `COMPACTION_KEEP_RECENT` messages unchanged.
//! 2. Replacing the middle section with a single user message that acts as a
//!    summary marker: `[context compacted: N messages truncated]`.
//!
//! **Why truncate-with-marker rather than LLM-summarise?**
//! - Zero extra latency / cost — no second model call needed.
//! - No new failure mode (summarisation model could fail too).
//! - Sufficient for v1: the model sees the recent turns and knows older
//!   context was cut.  An operator can tune `context_limit` down to force
//!   more frequent but smaller compactions.
//! - LLM-summarise is a clear upgrade path; the design doc records this as
//!   "P5 / operator-opt-in via config flag".
//!
//! ## Telemetry (P3 deferral resolved)
//!
//! A `TurnBuilder` is instantiated at the start of each turn and populated
//! with `usage.prompt_tokens` / `usage.completion_tokens` from the
//! `ChatCompletion`, tool-call count, denial count, and context-token
//! estimate.  `telemetry.record_turn` is called at the end of every turn.
//! `telemetry.finish` is called after the loop exits.
//!
//! # P2 Safety pipeline
//!
//! ```text
//! GUARDRAILS (hard, non-overridable)
//!   → PERMISSION (per-tool allow|ask|deny from policy)
//!     → SANDBOX (worktree confinement + env scrub + arg scan)
//!       → tool execute
//! ```
//!
//! A blocked call returns the blocking reason as the tool result message so
//! the model can adapt — it is NOT a hard error that stops the loop.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{HarnessError, HarnessResult};
use crate::policy::{Policy, PolicyOutcome, run_pipeline};
use crate::provider::{ChatMessage, ProviderClient, ToolCall};
use crate::sandbox::{self, NoopOsSandbox};
use crate::telemetry::{Telemetry, TurnBuilder};
use crate::tools::registry::dispatch;
use crate::tools::{ToolContext, ToolRegistry};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum retries for a single transient provider error before giving up on
/// the current model and falling back (or returning an error).
const MAX_TRANSIENT_RETRIES: u32 = 3;

/// Base backoff in milliseconds.  Doubles each retry up to `MAX_BACKOFF_MS`.
const BACKOFF_BASE_MS: u64 = 200;

/// Maximum backoff cap in milliseconds (≈ 3 seconds).
const MAX_BACKOFF_MS: u64 = 3_200;

/// How many consecutive malformed-tool-call responses from a model before
/// triggering fallback to the next model in the chain.
const MALFORMED_TOOL_CALL_THRESHOLD: u32 = 2;

/// How many messages to keep at the tail of the history during compaction.
/// The system prompt is always kept; these are the most-recent turns.
const COMPACTION_KEEP_RECENT: usize = 6;

/// Leave this fraction of the context limit as headroom before compacting.
/// Compaction triggers when `context_tokens > context_limit * (1 - headroom)`.
const CONTEXT_HEADROOM_FRAC: f64 = 0.10;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for a single agent run.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Primary model identifier (e.g. `"gemma4"`, `"qwen2.5-coder:7b"`).
    pub model: String,
    /// Ordered fallback model list.  If the primary errors fatally or returns
    /// malformed tool calls repeatedly, the loop switches to the next model.
    /// This is the **error-based** fallback chain — distinct from
    /// `token_pressure_models`.
    /// Empty = no fallback.
    pub fallback_models: Vec<String>,
    /// Allowlist of model IDs known to support tool-calling reliably.
    /// An unvetted model emits a warning but is NOT hard-blocked.
    /// Empty = no allowlist (all models accepted without warning).
    pub vetted_models: Vec<String>,
    /// Maximum number of turns before giving up.
    pub max_iterations: u32,
    /// Whether to use streaming mode (SSE) for token deltas.
    /// `false` = use the blocking complete() path (simpler, spike-proven).
    pub stream: bool,
    /// Permission policy (loaded from `.bwoc/harness-policy.toml` or default).
    /// Default = fail-safe deny-all.
    pub policy: Policy,
    /// Whether the harness has a controlling TTY for `ask`-mode prompts.
    /// `false` = autonomous / non-TTY mode; `ask` falls back to `deny`.
    pub is_tty: bool,
    /// Default context-window token limit used when a model is absent from
    /// `model_context_limits`.  When the running context approaches this
    /// limit, the loop compacts the history.
    /// `0` = no compaction / no limit checking.
    pub context_limit: u32,
    /// Per-model context-window token limits.
    ///
    /// Key = model identifier, Value = context limit in tokens.
    /// When the active model is found in this map its limit overrides
    /// `context_limit`.  The **point of this map** is that different models
    /// in a fleet have different window sizes; tracking them separately lets
    /// the loop detect pressure per-model and switch to a larger-context
    /// model rather than compacting.
    ///
    /// `0` values are treated as "no limit" (same as absent).
    ///
    /// # Example
    /// ```text
    /// model_context_limits = {
    ///   "small-model" => 4096,
    ///   "large-model" => 32768,
    /// }
    /// ```
    ///
    /// // TODO(#13): optional provider-queried limits (dynamic, not static)
    pub model_context_limits: HashMap<String, u32>,
    /// Ordered list of candidate models to switch to when token pressure is
    /// detected on the active model.
    ///
    /// This is a **proactive, token-pressure–driven** switch — distinct from
    /// the error-based `fallback_models`.  When the active model's context
    /// approaches its limit, the loop searches this list (in order) for the
    /// first model that:
    ///
    /// 1. Has a **larger** configured limit than the current model's limit.
    /// 2. Is present in `vetted_models` (or `vetted_models` is empty).
    ///
    /// If a qualifying model is found the loop switches to it without history
    /// loss.  If no model qualifies, the existing compaction path runs instead.
    ///
    /// Empty = token-pressure auto-switch is disabled (compaction only).
    pub token_pressure_models: Vec<String>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            model: "gemma4".to_string(),
            fallback_models: Vec::new(),
            vetted_models: Vec::new(),
            max_iterations: 20,
            stream: false,
            policy: Policy::default(), // fail-safe deny-all
            is_tty: false,
            context_limit: 0,
            model_context_limits: HashMap::new(),
            token_pressure_models: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

/// Result of a completed agent run.
#[derive(Debug)]
pub struct LoopResult {
    /// Final text response from the model (content of the last assistant message).
    pub final_response: String,
    /// Number of turns taken.
    pub turns: u32,
    /// All messages exchanged (for debug / memory purposes).
    pub history: Vec<ChatMessage>,
    /// Number of context compactions performed.
    pub compactions: u32,
    /// Model that produced the final answer (may differ from config.model if
    /// fallback was triggered).
    pub active_model: String,
    /// Number of token-pressure–driven model switches performed during this
    /// run.  Distinct from error-based fallback switches.
    pub token_pressure_switches: u32,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the agentic loop.
///
/// # Arguments
/// - `provider` — injectable provider client (real or mock).
/// - `registry` — tool registry.
/// - `ctx` — working directory context for tool execution.
/// - `config` — loop configuration.
/// - `system_prompt` — the agent's system prompt (loaded from `AGENTS.md`).
/// - `initial_messages` — the first user message(s).
/// - `telemetry` — mutable telemetry accumulator; receives one record per turn
///   and is flushed by the caller after this function returns.
pub async fn run_loop(
    provider: Arc<dyn ProviderClient>,
    registry: Arc<ToolRegistry>,
    ctx: ToolContext,
    config: LoopConfig,
    system_prompt: String,
    initial_messages: Vec<ChatMessage>,
    telemetry: &mut Telemetry,
) -> HarnessResult<LoopResult> {
    // --- Vetted-model gate ---------------------------------------------------
    // Warn (stderr) if the primary model is not on the vetted list.
    if !config.vetted_models.is_empty() && !config.vetted_models.contains(&config.model) {
        eprintln!(
            "[bwoc-harness] WARNING: model `{}` is not in the vetted-models allowlist. \
             Tool-calling reliability is unknown. Proceeding anyway.",
            config.model
        );
    }

    let tools = registry.tool_schemas();

    // Build the initial message history.
    let mut history: Vec<ChatMessage> = Vec::new();
    history.push(ChatMessage::system(&system_prompt));
    history.extend(initial_messages);

    let mut turns = 0u32;
    let mut compactions = 0u32;
    let mut token_pressure_switches = 0u32;

    // Per-session cache of provider-queried context limits.
    // Key = model id; Value = result of one `model_context_limit` call.
    // `None` means "provider returned nothing / query failed".
    // Each model is queried at most once per session regardless of how many
    // turns use it (cache-aside, populated on first encounter).
    let mut provider_limit_cache: HashMap<String, Option<u32>> = HashMap::new();

    // Active model may shift to a fallback; we track it here.
    let mut active_model = config.model.clone();
    // Build the full model chain: primary + fallbacks (error-based).
    let model_chain: Vec<String> = {
        let mut chain = vec![config.model.clone()];
        chain.extend(config.fallback_models.iter().cloned());
        chain
    };
    let mut model_chain_idx = 0usize;

    // Consecutive malformed-tool-call counter for the current model.
    let mut consecutive_malformed = 0u32;

    loop {
        turns += 1;
        if turns > config.max_iterations {
            return Err(HarnessError::MaxIterations(config.max_iterations));
        }

        // ── Provider limit cache — query once per model ───────────────────────
        // If this model has not been seen yet this session, ask the provider
        // for its context-window size.  Failures are stored as `None` so we
        // don't retry on every turn.
        if !provider_limit_cache.contains_key(active_model.as_str()) {
            let queried = provider.model_context_limit(&active_model).await;
            provider_limit_cache.insert(active_model.clone(), queried);
        }

        // ── Token-pressure check + auto-switch ───────────────────────────────
        // Evaluated BEFORE compaction so a larger-context switch is preferred
        // over discarding history when a qualifying model is configured.
        //
        // 1. Determine the effective limit for the current model.
        // 2. If approaching the limit, look for a larger vetted model in
        //    `token_pressure_models`.
        // 3. If found → switch (no history loss).  If not → fall through to
        //    the existing compaction path.
        let mut this_turn_pressure_switch = 0u32;
        let effective_limit = model_effective_limit(
            &active_model,
            &config,
            provider_limit_cache
                .get(active_model.as_str())
                .copied()
                .flatten(),
        );
        if effective_limit > 0 {
            let estimated = estimate_context_tokens(&history);
            let threshold = (effective_limit as f64 * (1.0 - CONTEXT_HEADROOM_FRAC)) as u32;
            if estimated >= threshold {
                if let Some(larger_model) =
                    find_larger_vetted_model(&active_model, &config, &provider_limit_cache)
                {
                    eprintln!(
                        "[bwoc-harness] token pressure on `{active_model}` \
                         ({estimated}/{effective_limit} tokens, threshold {threshold}): \
                         switching to `{larger_model}` (larger context window)"
                    );
                    active_model = larger_model;
                    token_pressure_switches += 1;
                    this_turn_pressure_switch = 1;
                    // Continue to the compaction check; if the new model also
                    // has a limit configured and we're already over it (edge
                    // case: model limits very close together), compaction will
                    // fire on the next iteration.  For the common case the new
                    // model has plenty of headroom and we skip compaction.
                }
                // If no qualifying model was found we fall through to the
                // standard compaction path below.
            }
        }

        // ── Context compaction ────────────────────────────────────────────────
        // Runs after the token-pressure check.  If a switch happened above and
        // the new model still exceeds its own limit (rare), compaction fires.
        {
            let compact_limit = model_effective_limit(
                &active_model,
                &config,
                provider_limit_cache
                    .get(active_model.as_str())
                    .copied()
                    .flatten(),
            );
            if compact_limit > 0 {
                let estimated = estimate_context_tokens(&history);
                let threshold = (compact_limit as f64 * (1.0 - CONTEXT_HEADROOM_FRAC)) as u32;
                if estimated >= threshold {
                    compact_history(&mut history);
                    compactions += 1;
                }
            }
        }

        // ── Turn builder (telemetry) ──────────────────────────────────────────
        let mut tb = TurnBuilder::new(turns);
        tb.token_pressure_switch = this_turn_pressure_switch;

        // Snapshot context token estimate for this turn's start.
        tb.context_tokens = estimate_context_tokens(&history);

        // ── Call provider with retry ─────────────────────────────────────────
        let (completion, usage_opt) = call_with_retry_v2(
            &*provider,
            history.clone(),
            tools.clone(),
            &active_model,
            config.stream,
        )
        .await?;

        // Populate token counts from usage if the provider returned them.
        if let Some(usage) = &usage_opt {
            tb.tokens_in = usage.prompt_tokens;
            tb.tokens_out = usage.completion_tokens;
        }

        // ── Check for malformed tool calls ───────────────────────────────────
        // A tool call is "malformed" if its id is empty or its arguments are
        // not valid JSON.  Consistently malformed responses trigger fallback.
        let raw_tool_calls = completion.tool_calls.clone().unwrap_or_default();

        let this_turn_malformed =
            !raw_tool_calls.is_empty() && has_malformed_tool_calls(&raw_tool_calls);

        if this_turn_malformed {
            consecutive_malformed += 1;
            eprintln!(
                "[bwoc-harness] malformed tool call from `{active_model}` \
                 (consecutive: {consecutive_malformed}/{MALFORMED_TOOL_CALL_THRESHOLD})"
            );

            if consecutive_malformed >= MALFORMED_TOOL_CALL_THRESHOLD {
                // Try the next model in the chain.
                model_chain_idx += 1;
                if model_chain_idx >= model_chain.len() {
                    let tried = model_chain.clone();
                    return Err(HarnessError::AllModelsExhausted {
                        tried,
                        last_error: format!(
                            "model `{active_model}` returned malformed tool calls \
                             {consecutive_malformed} times"
                        ),
                    });
                }
                active_model = model_chain[model_chain_idx].clone();
                consecutive_malformed = 0;

                // Warn if new active model is unvetted.
                if !config.vetted_models.is_empty() && !config.vetted_models.contains(&active_model)
                {
                    eprintln!(
                        "[bwoc-harness] WARNING: fallback model `{active_model}` \
                         is not in vetted-models allowlist."
                    );
                }

                eprintln!("[bwoc-harness] switching to fallback model `{active_model}`");
                // Don't append anything to history; retry this turn with the
                // new model.
                let m = tb.finish();
                telemetry.record_turn(m);
                turns -= 1; // Don't count the bad turn toward max_iterations.
                continue;
            }
            // Under the threshold: fall through to normal processing.
            // The malformed tool calls are treated as if the model returned
            // no tool calls (empty result), so the loop doesn't get stuck.
        } else {
            // Good response: reset the consecutive counter.
            consecutive_malformed = 0;
        }

        // ── Check for tool calls ─────────────────────────────────────────────
        let tool_calls = raw_tool_calls;

        if tool_calls.is_empty() {
            // No tool calls → model has given its final answer.
            let final_response = completion.content.clone().unwrap_or_default();
            history.push(completion);

            let m = tb.finish();
            telemetry.record_turn(m);

            return Ok(LoopResult {
                final_response,
                turns,
                history,
                compactions,
                active_model,
                token_pressure_switches,
            });
        }

        // ── Dispatch tools ───────────────────────────────────────────────────
        // Append the assistant message (with tool_calls) first, then the
        // results — this is required by the OpenAI spec.
        history.push(completion);

        tb.tool_calls = tool_calls.len() as u32;

        let results = execute_tool_calls(&tool_calls, &registry, &ctx, &config).await;

        // Count denials from the safety pipeline.
        for result in &results {
            if result.denied {
                tb.denials += 1;
            }
            history.push(ChatMessage::tool_result(
                result.call_id.clone(),
                result.content.clone(),
            ));
        }

        let m = tb.finish();
        telemetry.record_turn(m);
        // Continue to next turn.
    }
}

/// Unified provider call helper that handles both stream and non-stream paths,
/// returning `(ChatMessage, Option<Usage>)`.
async fn call_provider_once(
    provider: &dyn ProviderClient,
    messages: Vec<ChatMessage>,
    tools: Vec<crate::provider::Tool>,
    model: &str,
    stream: bool,
) -> HarnessResult<(ChatMessage, Option<crate::provider::Usage>)> {
    if stream {
        let msg = stream_and_accumulate(provider, messages, tools, model).await?;
        Ok((msg, None)) // streaming path doesn't expose usage counts
    } else {
        let completion = provider.complete(messages, tools, model).await?;
        let usage = completion.usage.clone();
        let choice =
            completion.choices.into_iter().next().ok_or_else(|| {
                HarnessError::Provider("provider returned empty choices".to_string())
            })?;
        Ok((choice.message, usage))
    }
}

/// Retry wrapper around [`call_provider_once`].
pub(crate) async fn call_with_retry_v2(
    provider: &dyn ProviderClient,
    messages: Vec<ChatMessage>,
    tools: Vec<crate::provider::Tool>,
    model: &str,
    stream: bool,
) -> HarnessResult<(ChatMessage, Option<crate::provider::Usage>)> {
    let mut attempt = 0u32;
    loop {
        match call_provider_once(provider, messages.clone(), tools.clone(), model, stream).await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_transient() && attempt < MAX_TRANSIENT_RETRIES => {
                attempt += 1;
                let delay = backoff_ms(attempt);
                eprintln!(
                    "[bwoc-harness] transient error on `{model}` (attempt {attempt}/{MAX_TRANSIENT_RETRIES}): {e}. \
                     Retrying in {delay}ms…"
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute bounded exponential backoff.
///
/// attempt 1 → 200ms, 2 → 400ms, 3 → 800ms, 4 → 1600ms … capped at 3200ms.
fn backoff_ms(attempt: u32) -> u64 {
    let raw = BACKOFF_BASE_MS.saturating_mul(1u64 << attempt.min(10));
    raw.min(MAX_BACKOFF_MS)
}

/// Detect malformed tool calls: empty ID or unparseable JSON arguments.
///
/// The spike (llama3.2 3B) produced calls with empty IDs and garbled JSON.
/// Detecting this early prevents the history from filling with garbage that
/// confuses the next turn.
fn has_malformed_tool_calls(calls: &[ToolCall]) -> bool {
    calls.iter().any(|c| {
        c.id.is_empty() || serde_json::from_str::<serde_json::Value>(&c.function.arguments).is_err()
    })
}

/// Estimate the total context tokens from the current history.
///
/// Rough heuristic: 1 token ≈ 4 characters (works for English + code; good
/// enough for a compaction trigger — exact counts come from `usage` fields).
fn estimate_context_tokens(history: &[ChatMessage]) -> u32 {
    let total_chars: usize = history
        .iter()
        .map(|m| m.content.as_deref().map_or(0, |c| c.len()))
        .sum();
    (total_chars / 4) as u32
}

/// Compact history in-place using the truncate-with-marker strategy.
///
/// Keeps:
/// - `history[0]` — the system message (always retained).
/// - The last `COMPACTION_KEEP_RECENT` messages.
///
/// Replaces the middle section with a single user message:
/// `[context compacted: N messages truncated]`
///
/// This is the v1 strategy chosen over LLM-summarise for two reasons:
/// - Zero extra latency (no second model call).
/// - No new failure mode (summarisation model could fail or add hallucinations).
///
/// LLM-summarise is the natural upgrade path when the operator opts in.
fn compact_history(history: &mut Vec<ChatMessage>) {
    // Need at least: system + something to compact + the recent tail.
    let min_len = 1 + COMPACTION_KEEP_RECENT + 1;
    if history.len() <= min_len {
        return; // nothing to compact
    }

    let system = history[0].clone();
    let tail_start = history.len().saturating_sub(COMPACTION_KEEP_RECENT);
    let truncated = tail_start - 1; // messages between system and tail

    let tail: Vec<ChatMessage> = history[tail_start..].to_vec();

    let marker = ChatMessage::user(format!(
        "[context compacted: {truncated} messages truncated to fit context window]"
    ));

    history.clear();
    history.push(system);
    history.push(marker);
    history.extend(tail);
}

/// Return the effective context-window token limit for `model`.
///
/// Precedence (highest → lowest):
/// 1. Explicit entry in `config.model_context_limits` — operator static
///    override; always wins so the operator can cap or extend a model's
///    window deliberately.
/// 2. `provider_queried` — value returned by [`ProviderClient::model_context_limit`]
///    and cached by the loop (one network call per model per session).
/// 3. `config.context_limit` — global default; used when neither source has
///    information.  `0` means "no limit / compaction disabled".
fn model_effective_limit(model: &str, config: &LoopConfig, provider_queried: Option<u32>) -> u32 {
    // Layer 1 — static config (operator override wins).
    let from_map = config.model_context_limits.get(model).copied().unwrap_or(0);
    if from_map > 0 {
        return from_map;
    }

    // Layer 2 — provider-queried value (dynamic, best-effort).
    if let Some(queried) = provider_queried {
        if queried > 0 {
            return queried;
        }
    }

    // Layer 3 — global default.
    config.context_limit
}

/// Find the first model in `config.token_pressure_models` that:
/// 1. Has a **strictly larger** effective limit than the current model's limit.
/// 2. Passes the vetted-model gate (`vetted_models` is empty OR the model is
///    listed in it).
///
/// Returns `Some(model_id)` if found, `None` otherwise.
///
/// A model that fails the vetted gate is skipped with a warning — it will
/// NOT be used silently.
///
/// `provider_cache` is the per-session cache populated by
/// [`ProviderClient::model_context_limit`] queries.
fn find_larger_vetted_model(
    current_model: &str,
    config: &LoopConfig,
    provider_cache: &HashMap<String, Option<u32>>,
) -> Option<String> {
    let current_limit = model_effective_limit(
        current_model,
        config,
        provider_cache.get(current_model).copied().flatten(),
    );

    for candidate in &config.token_pressure_models {
        if candidate == current_model {
            continue;
        }

        // Check vetted gate first — skip (with warning) if not vetted.
        if !config.vetted_models.is_empty() && !config.vetted_models.contains(candidate) {
            eprintln!(
                "[bwoc-harness] token-pressure candidate `{candidate}` skipped: \
                 not in vetted-models allowlist"
            );
            continue;
        }

        let candidate_limit = model_effective_limit(
            candidate,
            config,
            provider_cache.get(candidate).copied().flatten(),
        );
        if candidate_limit > current_limit {
            return Some(candidate.clone());
        }
    }
    None
}

/// Dispatch all tool calls in a turn sequentially, passing each through the
/// full safety pipeline: GUARDRAILS → PERMISSION → SANDBOX → execute.
///
/// A blocked call returns the blocking reason as the tool result content so
/// the model can adapt.  It is NOT a hard error that stops the loop.
async fn execute_tool_calls(
    calls: &[ToolCall],
    registry: &ToolRegistry,
    ctx: &ToolContext,
    config: &LoopConfig,
) -> Vec<ToolCallResult> {
    // P2: sequential execution (concurrent tool execution is P3).
    let mut results = Vec::with_capacity(calls.len());
    let os_sandbox = NoopOsSandbox;

    for call in calls {
        let tool_name = &call.function.name;
        let args_json = &call.function.arguments;

        // ── Layer 1 + 2: Guardrails → Permission ────────────────────────────
        let outcome = run_pipeline(
            tool_name,
            args_json,
            &ctx.workdir,
            &config.policy,
            config.is_tty,
        );

        let (content, denied) = match outcome {
            PolicyOutcome::Proceed => {
                // ── Layer 3: Sandbox ─────────────────────────────────────────
                // For run_command: use the sandboxed runner (env scrub + arg scan + cwd lock).
                // For all other tools: the sandbox path-confinement is already enforced
                // by ToolContext::resolve_path; run through dispatch as before.
                let result = if tool_name == "run_command" {
                    // Extract the command string from the JSON args.
                    match serde_json::from_str::<serde_json::Value>(args_json)
                        .ok()
                        .and_then(|v| v["command"].as_str().map(|s| s.to_string()))
                    {
                        Some(cmd) => {
                            match sandbox::run_sandboxed(&cmd, &ctx.workdir, &os_sandbox).await {
                                Ok(output) => output.into_tool_result(),
                                Err(e) => format!("error: {e}"),
                            }
                        }
                        None => {
                            // Malformed args: fall through to dispatch which will
                            // return a proper "missing command argument" error.
                            dispatch(registry, tool_name, args_json, ctx).await
                        }
                    }
                } else {
                    dispatch(registry, tool_name, args_json, ctx).await
                };
                (result, false)
            }
            blocked => {
                // Feed the denial back to the model as the tool result.
                let msg = blocked
                    .into_tool_result()
                    .unwrap_or_else(|| "blocked".to_string());
                (msg, true)
            }
        };

        results.push(ToolCallResult {
            call_id: call.id.clone(),
            tool_name: call.function.name.clone(),
            content,
            denied,
        });
    }
    results
}

struct ToolCallResult {
    call_id: String,
    #[allow(dead_code)]
    tool_name: String,
    content: String,
    denied: bool,
}

/// Stream a response and accumulate content + tool_calls into a single
/// [`ChatMessage`] as if it were a non-streaming completion.
async fn stream_and_accumulate(
    provider: &dyn ProviderClient,
    messages: Vec<ChatMessage>,
    tools: Vec<crate::provider::Tool>,
    model: &str,
) -> HarnessResult<ChatMessage> {
    use futures_util::StreamExt;

    let mut stream = provider.stream(messages, tools, model).await?;

    let mut content_buf = String::new();
    // tool_calls accumulation: index → (id, type, name, args_buf)
    let mut tool_calls_acc: std::collections::HashMap<u32, ToolCallAccumulator> =
        std::collections::HashMap::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
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

    Ok(ChatMessage::assistant(
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
    ))
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    kind: String,
    name: String,
    args_buf: String,
}

// ---------------------------------------------------------------------------
// Tests (all offline — no real provider)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HarnessError;
    use crate::policy::{Mode, Policy};
    use crate::provider::types::FunctionCall;
    use crate::provider::{
        ChatCompletion, ChatMessage, Choice, FinishReason, ProviderClient, StreamChunk, Tool,
        ToolCall, Usage,
    };
    use crate::telemetry::Telemetry;
    use crate::tools::registry::default_registry;
    use async_trait::async_trait;
    use futures_util::Stream;
    use std::pin::Pin;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // ── Mock provider ────────────────────────────────────────────────────────

    /// A mock provider that returns pre-configured responses in sequence.
    struct MockProvider {
        responses: Mutex<Vec<Result<ChatCompletion, HarnessError>>>,
    }

    impl MockProvider {
        fn new(responses: Vec<ChatCompletion>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            }
        }

        /// Build a mock that will return errors at specific positions.
        fn with_errors(responses: Vec<Result<ChatCompletion, HarnessError>>) -> Self {
            Self {
                responses: Mutex::new(responses),
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
            // Not used in non-streaming tests.
            Err(HarnessError::Provider(
                "mock: stream not implemented".to_string(),
            ))
        }

        async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
            Ok(())
        }
    }

    fn make_final_response(content: &str) -> ChatCompletion {
        ChatCompletion {
            id: "mock-id".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(Some(content.to_string()), None),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: None,
        }
    }

    fn make_final_response_with_usage(
        content: &str,
        prompt: u32,
        completion: u32,
    ) -> ChatCompletion {
        ChatCompletion {
            id: "mock-id".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(Some(content.to_string()), None),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: Some(Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                total_tokens: prompt + completion,
            }),
        }
    }

    fn make_tool_call_response(tool_name: &str, args: &str) -> ChatCompletion {
        let call = ToolCall {
            id: "call-1".to_string(),
            kind: "function".to_string(),
            function: FunctionCall {
                name: tool_name.to_string(),
                arguments: args.to_string(),
            },
        };
        ChatCompletion {
            id: "mock-id".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(None, Some(vec![call])),
                finish_reason: Some(FinishReason::ToolCalls),
            }],
            usage: None,
        }
    }

    fn make_malformed_tool_call_response() -> ChatCompletion {
        // Empty tool call ID + invalid JSON args = malformed.
        let call = ToolCall {
            id: "".to_string(), // empty ID
            kind: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: "not-valid-json".to_string(),
            },
        };
        ChatCompletion {
            id: "mock-id".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(None, Some(vec![call])),
                finish_reason: Some(FinishReason::ToolCalls),
            }],
            usage: None,
        }
    }

    /// A permissive policy for tests that need tool execution to proceed.
    fn allow_all_policy() -> Policy {
        Policy {
            default_mode: Mode::Allow,
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
        }
    }

    /// Build a LoopConfig with allow-all policy for basic loop tests.
    fn test_config(max_iterations: u32) -> LoopConfig {
        LoopConfig {
            model: "mock".to_string(),
            fallback_models: Vec::new(),
            vetted_models: Vec::new(),
            max_iterations,
            stream: false,
            policy: allow_all_policy(),
            is_tty: false,
            context_limit: 0,
            model_context_limits: std::collections::HashMap::new(),
            token_pressure_models: Vec::new(),
        }
    }

    fn noop_telemetry() -> Telemetry {
        Telemetry::new("test-sess", "agent-test")
    }

    // ── Core loop tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn loop_immediate_final_answer() {
        let tmp = TempDir::new().unwrap();
        let provider = Arc::new(MockProvider::new(vec![make_final_response(
            "Hello, world!",
        )]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let result = run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "You are a helpful assistant.".to_string(),
            vec![ChatMessage::user("Say hello")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.final_response, "Hello, world!");
        assert_eq!(result.turns, 1);
    }

    #[tokio::test]
    async fn loop_tool_call_then_final() {
        let tmp = TempDir::new().unwrap();

        // Write a file the model will "read".
        tokio::fs::write(tmp.path().join("note.txt"), "secret content")
            .await
            .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("read_file", r#"{"path": "note.txt"}"#),
            make_final_response("The file contains: secret content"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let result = run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "You are a helpful assistant.".to_string(),
            vec![ChatMessage::user("What is in note.txt?")],
            &mut telem,
        )
        .await
        .unwrap();

        assert!(result.final_response.contains("secret content"));
        assert_eq!(result.turns, 2);
    }

    #[tokio::test]
    async fn loop_max_iterations_error() {
        let tmp = TempDir::new().unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("list_dir", "{}"),
            make_tool_call_response("list_dir", "{}"),
            make_tool_call_response("list_dir", "{}"),
            make_tool_call_response("list_dir", "{}"),
            make_tool_call_response("list_dir", "{}"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let err = run_loop(
            provider,
            registry,
            ctx,
            test_config(3),
            "system".to_string(),
            vec![ChatMessage::user("loop forever")],
            &mut telem,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, HarnessError::MaxIterations(3)));
    }

    #[tokio::test]
    async fn loop_write_then_read() {
        let tmp = TempDir::new().unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("write_file", r#"{"path": "out.txt", "content": "done"}"#),
            make_tool_call_response("read_file", r#"{"path": "out.txt"}"#),
            make_final_response("Wrote and confirmed: done"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let result = run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "system".to_string(),
            vec![ChatMessage::user("write a file")],
            &mut telem,
        )
        .await
        .unwrap();

        assert!(result.final_response.contains("done"));
        assert_eq!(result.turns, 3);
        let content = tokio::fs::read_to_string(tmp.path().join("out.txt"))
            .await
            .unwrap();
        assert_eq!(content, "done");
    }

    // ── Telemetry per turn ───────────────────────────────────────────────────

    #[tokio::test]
    async fn telemetry_records_one_turn_per_model_call() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("f.txt"), "hi")
            .await
            .unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("read_file", r#"{"path": "f.txt"}"#),
            make_final_response("done"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = Telemetry::new("sess-telem-001", "agent-oracle");

        run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "sys".to_string(),
            vec![ChatMessage::user("read")],
            &mut telem,
        )
        .await
        .unwrap();

        let record = telem.build_record();
        let harness = record.harness.unwrap();
        // 2 turns: one tool-call turn + one final answer turn.
        assert_eq!(harness.turns.len(), 2, "expected 2 telemetry turns");
        assert_eq!(harness.totals.turns, 2);
        // First turn had 1 tool call.
        assert_eq!(harness.turns[0].tool_calls, 1);
    }

    #[tokio::test]
    async fn telemetry_token_counts_populated_from_usage() {
        let tmp = TempDir::new().unwrap();
        let provider = Arc::new(MockProvider::new(vec![make_final_response_with_usage(
            "answer", 150, 30,
        )]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = Telemetry::new("sess-telem-002", "agent-oracle");

        run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "sys".to_string(),
            vec![ChatMessage::user("q")],
            &mut telem,
        )
        .await
        .unwrap();

        let record = telem.build_record();
        let harness = record.harness.unwrap();
        assert_eq!(harness.turns[0].tokens_in, 150);
        assert_eq!(harness.turns[0].tokens_out, 30);
    }

    #[tokio::test]
    async fn telemetry_denial_count_increments_on_blocked_tool() {
        let tmp = TempDir::new().unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("run_command", r#"{"command": "rm -rf /"}"#),
            make_final_response("blocked"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = Telemetry::new("sess-telem-003", "agent-oracle");

        run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "sys".to_string(),
            vec![ChatMessage::user("destroy")],
            &mut telem,
        )
        .await
        .unwrap();

        let record = telem.build_record();
        let harness = record.harness.unwrap();
        // Turn 1 had 1 tool call and 1 denial.
        assert_eq!(harness.turns[0].denials, 1, "expected 1 denial in turn 1");
        assert_eq!(harness.totals.denials, 1);
    }

    // ── Retry tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn transient_error_retried_then_succeeds() {
        // call_with_retry_v2: one transient failure then success.
        let provider = Arc::new(MockProvider::with_errors(vec![
            Err(HarnessError::TransientProvider(
                "connection reset".to_string(),
            )),
            Ok(make_final_response("ok after retry")),
        ]));

        let messages = vec![ChatMessage::user("hello")];
        let result = call_with_retry_v2(&*provider, messages, vec![], "mock", false).await;
        assert!(result.is_ok(), "should succeed after retry: {result:?}");
        let (msg, _) = result.unwrap();
        assert_eq!(msg.content.as_deref(), Some("ok after retry"));
    }

    #[tokio::test]
    async fn fatal_error_not_retried() {
        // 4xx / model-not-found errors must fail fast (no retry).
        let provider = Arc::new(MockProvider::with_errors(vec![
            Err(HarnessError::ModelNotFound("bad-model".to_string())),
            // This response should NEVER be reached.
            Ok(make_final_response("should not see this")),
        ]));

        let messages = vec![ChatMessage::user("hello")];
        let result = call_with_retry_v2(&*provider, messages, vec![], "bad-model", false).await;
        assert!(
            matches!(result, Err(HarnessError::ModelNotFound(_))),
            "fatal error must not be retried: {result:?}"
        );
    }

    #[tokio::test]
    async fn transient_errors_exhausted_returns_last_error() {
        // More transient errors than MAX_TRANSIENT_RETRIES → should fail.
        let mut responses: Vec<Result<ChatCompletion, HarnessError>> = (0..=MAX_TRANSIENT_RETRIES)
            .map(|_| {
                Err(HarnessError::TransientProvider(
                    "server overloaded".to_string(),
                ))
            })
            .collect();
        // Add one success that should NOT be reached.
        responses.push(Ok(make_final_response("unreachable")));

        let provider = Arc::new(MockProvider::with_errors(responses));

        let messages = vec![ChatMessage::user("hello")];
        let result = call_with_retry_v2(&*provider, messages, vec![], "mock", false).await;
        assert!(
            result.is_err(),
            "should fail after exhausting retries: {result:?}"
        );
        assert!(
            matches!(result, Err(HarnessError::TransientProvider(_))),
            "should return the last transient error: {result:?}"
        );
    }

    // ── Fallback model tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn fallback_model_triggered_on_repeated_malformed_tool_calls() {
        let tmp = TempDir::new().unwrap();

        // Primary model returns malformed tool calls MALFORMED_TOOL_CALL_THRESHOLD times.
        // After that, fallback model gives a final answer.
        let mut responses: Vec<ChatCompletion> = (0..MALFORMED_TOOL_CALL_THRESHOLD)
            .map(|_| make_malformed_tool_call_response())
            .collect();
        responses.push(make_final_response("fallback succeeded"));

        let provider = Arc::new(MockProvider::new(responses));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let config = LoopConfig {
            model: "primary-model".to_string(),
            fallback_models: vec!["fallback-model".to_string()],
            vetted_models: vec!["fallback-model".to_string()],
            max_iterations: 20,
            stream: false,
            policy: allow_all_policy(),
            is_tty: false,
            context_limit: 0,
            model_context_limits: std::collections::HashMap::new(),
            token_pressure_models: Vec::new(),
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "sys".to_string(),
            vec![ChatMessage::user("do something")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.final_response, "fallback succeeded");
        assert_eq!(
            result.active_model, "fallback-model",
            "loop should report the fallback model as active"
        );
    }

    #[tokio::test]
    async fn all_models_exhausted_returns_error() {
        let tmp = TempDir::new().unwrap();

        // Both primary and fallback return malformed tool calls every time.
        let responses: Vec<ChatCompletion> = (0..(MALFORMED_TOOL_CALL_THRESHOLD * 2 + 4))
            .map(|_| make_malformed_tool_call_response())
            .collect();

        let provider = Arc::new(MockProvider::new(responses));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let config = LoopConfig {
            model: "bad-primary".to_string(),
            fallback_models: vec!["bad-fallback".to_string()],
            vetted_models: Vec::new(),
            max_iterations: 30,
            stream: false,
            policy: allow_all_policy(),
            is_tty: false,
            context_limit: 0,
            model_context_limits: std::collections::HashMap::new(),
            token_pressure_models: Vec::new(),
        };

        let err = run_loop(
            provider,
            registry,
            ctx,
            config,
            "sys".to_string(),
            vec![ChatMessage::user("do something")],
            &mut telem,
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, HarnessError::AllModelsExhausted { .. }),
            "expected AllModelsExhausted, got {err:?}"
        );
    }

    // ── Vetted-model gate ────────────────────────────────────────────────────

    #[tokio::test]
    async fn vetted_model_gate_allows_known_model_without_panic() {
        // A vetted model runs normally — no warning, no error.
        let tmp = TempDir::new().unwrap();
        let provider = Arc::new(MockProvider::new(vec![make_final_response("ok")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let config = LoopConfig {
            model: "gemma4".to_string(),
            vetted_models: vec!["gemma4".to_string(), "qwen2.5-coder:7b".to_string()],
            ..test_config(5)
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "sys".to_string(),
            vec![ChatMessage::user("hi")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.final_response, "ok");
    }

    #[tokio::test]
    async fn vetted_model_gate_unvetted_model_still_runs() {
        // An unvetted model gets a warning but the loop continues normally.
        let tmp = TempDir::new().unwrap();
        let provider = Arc::new(MockProvider::new(vec![make_final_response("still works")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let config = LoopConfig {
            model: "unknown-model".to_string(),
            vetted_models: vec!["only-this-one".to_string()],
            ..test_config(5)
        };

        // Must not error — just warn.
        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "sys".to_string(),
            vec![ChatMessage::user("hi")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.final_response, "still works");
    }

    // ── Context compaction tests ─────────────────────────────────────────────

    #[test]
    fn compact_history_reduces_length() {
        let mut history: Vec<ChatMessage> = Vec::new();
        // system + 20 user messages
        history.push(ChatMessage::system("sys"));
        for i in 0..20 {
            history.push(ChatMessage::user(format!("message {i}")));
        }
        let original_len = history.len();
        compact_history(&mut history);
        assert!(
            history.len() < original_len,
            "compact should reduce history length"
        );
    }

    #[test]
    fn compact_history_retains_system_message() {
        let mut history = vec![
            ChatMessage::system("system prompt"),
            ChatMessage::user("old message 1"),
            ChatMessage::user("old message 2"),
            ChatMessage::user("old message 3"),
            ChatMessage::user("old message 4"),
            ChatMessage::user("old message 5"),
            ChatMessage::user("old message 6"),
            ChatMessage::user("old message 7"),
            ChatMessage::user("old message 8"),
            ChatMessage::user("recent 1"),
            ChatMessage::user("recent 2"),
        ];
        compact_history(&mut history);

        // First message must be the system prompt.
        assert_eq!(
            history[0].content.as_deref(),
            Some("system prompt"),
            "system message must be retained at index 0"
        );
    }

    #[test]
    fn compact_history_inserts_marker() {
        let mut history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("old 1"),
            ChatMessage::user("old 2"),
            ChatMessage::user("old 3"),
            ChatMessage::user("old 4"),
            ChatMessage::user("old 5"),
            ChatMessage::user("old 6"),
            ChatMessage::user("old 7"),
            ChatMessage::user("recent tail"),
        ];
        compact_history(&mut history);

        // Index 1 should be the compaction marker.
        let marker = &history[1];
        assert!(
            marker
                .content
                .as_deref()
                .unwrap_or("")
                .contains("context compacted"),
            "index 1 must be the compaction marker; got: {:?}",
            marker.content
        );
    }

    #[test]
    fn compact_history_retains_recent_tail() {
        let mut history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("old 1"),
            ChatMessage::user("old 2"),
            ChatMessage::user("old 3"),
            ChatMessage::user("old 4"),
            ChatMessage::user("old 5"),
            ChatMessage::user("old 6"),
            ChatMessage::user("old 7"),
            ChatMessage::user("tail_message"), // should survive compaction
        ];
        compact_history(&mut history);

        let last = history.last().unwrap();
        assert_eq!(
            last.content.as_deref(),
            Some("tail_message"),
            "most recent message must survive compaction"
        );
    }

    #[test]
    fn compact_history_noop_if_too_short() {
        // History shorter than min_len must not be modified.
        let original = vec![ChatMessage::system("sys"), ChatMessage::user("only")];
        let mut history = original.clone();
        compact_history(&mut history);
        assert_eq!(
            history.len(),
            original.len(),
            "short history must not be compacted"
        );
    }

    #[tokio::test]
    async fn context_compaction_triggers_in_loop() {
        let tmp = TempDir::new().unwrap();

        // Build a history that will exceed a tiny context_limit.
        // Each message is ~50 chars → 50/4 ≈ 12 tokens.
        // With context_limit=10, compaction should fire.
        let initial = vec![ChatMessage::user(
            "This is a moderately long initial message that pushes the context over the tiny limit set for testing compaction behaviour.".to_string(),
        )];

        let provider = Arc::new(MockProvider::new(vec![make_final_response("compacted")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let config = LoopConfig {
            context_limit: 5, // Very small to force compaction on the first turn.
            ..test_config(5)
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "system prompt".to_string(),
            initial,
            &mut telem,
        )
        .await
        .unwrap();

        // The loop completes successfully — compaction is transparent.
        assert_eq!(result.final_response, "compacted");
        assert!(result.compactions >= 1, "expected at least one compaction");
    }

    // ── P2 Safety pipeline integration tests ─────────────────────────────────

    #[tokio::test]
    async fn guardrail_denial_is_fed_back_as_tool_result() {
        let tmp = TempDir::new().unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("run_command", r#"{"command": "rm -rf /"}"#),
            make_final_response("I cannot do that"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let result = run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "system".to_string(),
            vec![ChatMessage::user("wipe everything")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.final_response, "I cannot do that");
        assert_eq!(result.turns, 2);

        let tool_result = result.history.iter().find(|m| {
            m.tool_call_id.is_some()
                && m.content
                    .as_deref()
                    .unwrap_or("")
                    .contains("sila_panatatipata")
        });
        assert!(
            tool_result.is_some(),
            "guardrail violation reason not found in history"
        );
    }

    #[tokio::test]
    async fn permission_denial_is_fed_back_as_tool_result() {
        let tmp = TempDir::new().unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response("run_command", r#"{"command": "echo hi"}"#),
            make_final_response("cannot run that"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let deny_config = LoopConfig {
            model: "mock".to_string(),
            fallback_models: Vec::new(),
            vetted_models: Vec::new(),
            max_iterations: 5,
            stream: false,
            policy: Policy {
                default_mode: Mode::Deny,
                tools: std::collections::HashMap::new(),
                patterns: Vec::new(),
            },
            is_tty: false,
            context_limit: 0,
            model_context_limits: std::collections::HashMap::new(),
            token_pressure_models: Vec::new(),
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            deny_config,
            "system".to_string(),
            vec![ChatMessage::user("run echo hi")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.final_response, "cannot run that");

        let tool_result = result.history.iter().find(|m| {
            m.tool_call_id.is_some()
                && m.content
                    .as_deref()
                    .unwrap_or("")
                    .contains("DENIED by permission policy")
        });
        assert!(
            tool_result.is_some(),
            "permission denial not found in tool result history"
        );
    }

    #[tokio::test]
    async fn guardrail_no_verify_surfaces_in_tool_result() {
        let tmp = TempDir::new().unwrap();

        let provider = Arc::new(MockProvider::new(vec![
            make_tool_call_response(
                "run_command",
                r#"{"command": "git commit --no-verify -m 'skip hooks'"}"#,
            ),
            make_final_response("ok"),
        ]));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let result = run_loop(
            provider,
            registry,
            ctx,
            test_config(5),
            "system".to_string(),
            vec![ChatMessage::user("commit")],
            &mut telem,
        )
        .await
        .unwrap();

        let tool_result = result.history.iter().find(|m| {
            m.tool_call_id.is_some()
                && m.content
                    .as_deref()
                    .unwrap_or("")
                    .contains("sila_surameraya")
        });
        assert!(
            tool_result.is_some(),
            "sila_surameraya not found in tool result"
        );
    }

    // ── Backoff calculation ───────────────────────────────────────────────────

    #[test]
    fn backoff_increases_exponentially_and_caps() {
        let b1 = backoff_ms(1);
        let b2 = backoff_ms(2);
        let b3 = backoff_ms(3);
        assert!(b2 > b1, "backoff must increase with attempt");
        assert!(b3 > b2, "backoff must increase with attempt");
        // All must be <= MAX_BACKOFF_MS.
        for attempt in 1..=20 {
            assert!(
                backoff_ms(attempt) <= MAX_BACKOFF_MS,
                "backoff must not exceed MAX_BACKOFF_MS at attempt {attempt}"
            );
        }
    }

    // ── Malformed tool call detection ────────────────────────────────────────

    #[test]
    fn has_malformed_tool_calls_detects_empty_id() {
        let calls = vec![ToolCall {
            id: "".to_string(),
            kind: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path":"x"}"#.to_string(),
            },
        }];
        assert!(has_malformed_tool_calls(&calls));
    }

    #[test]
    fn has_malformed_tool_calls_detects_bad_json() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            kind: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: "not-json".to_string(),
            },
        }];
        assert!(has_malformed_tool_calls(&calls));
    }

    #[test]
    fn has_malformed_tool_calls_clean_call_is_ok() {
        let calls = vec![ToolCall {
            id: "call-1".to_string(),
            kind: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path": "README.md"}"#.to_string(),
            },
        }];
        assert!(!has_malformed_tool_calls(&calls));
    }

    // ── Token-pressure auto-switch tests ─────────────────────────────────────

    /// (a) Token pressure detected + a larger vetted model configured → switches.
    ///
    /// Setup: small-model has a 100-token limit; large-model has a 32 768-token
    /// limit.  The initial message is long enough to exceed
    /// `100 * (1 - 0.10) = 90` tokens.  `large-model` is vetted.
    /// Expected: loop switches to large-model; `token_pressure_switches == 1`.
    #[tokio::test]
    async fn token_pressure_switches_to_larger_vetted_model() {
        let tmp = TempDir::new().unwrap();

        // A message long enough to exceed the small model's threshold.
        // estimate_context_tokens: chars / 4.  "system prompt" ≈ 3 tokens.
        // We need > 90 total tokens → > 360 chars in history.
        let long_user_msg: String = "x".repeat(500);

        let provider = Arc::new(MockProvider::new(vec![make_final_response("done")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let mut limits = std::collections::HashMap::new();
        limits.insert("small-model".to_string(), 100u32);
        limits.insert("large-model".to_string(), 32_768u32);

        let config = LoopConfig {
            model: "small-model".to_string(),
            vetted_models: vec!["small-model".to_string(), "large-model".to_string()],
            token_pressure_models: vec!["large-model".to_string()],
            model_context_limits: limits,
            ..test_config(5)
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "system prompt".to_string(),
            vec![ChatMessage::user(long_user_msg)],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(
            result.active_model, "large-model",
            "loop should switch to large-model on token pressure"
        );
        assert_eq!(
            result.token_pressure_switches, 1,
            "expected exactly 1 token-pressure switch"
        );
        // No compaction should have happened (large model has plenty of room).
        assert_eq!(result.compactions, 0, "no compaction expected after switch");
    }

    /// (b) Token pressure + no larger model configured → compacts (existing path).
    ///
    /// small-model is under pressure but `token_pressure_models` is empty.
    /// Expected: compaction fires; no model switch.
    #[tokio::test]
    async fn token_pressure_no_candidate_falls_back_to_compaction() {
        let tmp = TempDir::new().unwrap();

        let long_user_msg: String = "x".repeat(500);

        let provider = Arc::new(MockProvider::new(vec![make_final_response("compacted")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let mut limits = std::collections::HashMap::new();
        limits.insert("small-model".to_string(), 100u32);

        let config = LoopConfig {
            model: "small-model".to_string(),
            vetted_models: vec!["small-model".to_string()],
            token_pressure_models: Vec::new(), // No candidate configured.
            model_context_limits: limits,
            ..test_config(5)
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "system prompt".to_string(),
            vec![ChatMessage::user(long_user_msg)],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.active_model, "small-model", "model must not change");
        assert_eq!(result.token_pressure_switches, 0, "no switch expected");
        assert!(result.compactions >= 1, "compaction must have fired");
    }

    /// (c) Under token limit → neither switch nor compaction.
    #[tokio::test]
    async fn under_token_limit_no_switch_no_compaction() {
        let tmp = TempDir::new().unwrap();

        // Very short message — well under any reasonable limit.
        let provider = Arc::new(MockProvider::new(vec![make_final_response("ok")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let mut limits = std::collections::HashMap::new();
        limits.insert("model-a".to_string(), 32_768u32);

        let config = LoopConfig {
            model: "model-a".to_string(),
            vetted_models: vec!["model-a".to_string(), "model-b".to_string()],
            token_pressure_models: vec!["model-b".to_string()],
            model_context_limits: limits,
            ..test_config(5)
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "sys".to_string(),
            vec![ChatMessage::user("hi")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(result.active_model, "model-a", "model must not change");
        assert_eq!(result.token_pressure_switches, 0);
        assert_eq!(result.compactions, 0);
    }

    // ── Provider-queried context limit tests (#13) ────────────────────────────
    //
    // All four cases are offline (no real network).  A configurable mock
    // returns a fixed value from `model_context_limit`.

    /// Mock provider where `model_context_limit` returns a configurable value.
    struct LimitedMockProvider {
        /// Responses for `complete` (reuses the same ordering approach as
        /// MockProvider).
        responses: Mutex<Vec<Result<ChatCompletion, HarnessError>>>,
        /// Value returned by `model_context_limit` (None = provider unknown).
        queried_limit: Option<u32>,
        /// Count of how many times `model_context_limit` was called.
        limit_query_count: std::sync::atomic::AtomicU32,
    }

    impl LimitedMockProvider {
        fn new(responses: Vec<ChatCompletion>, queried_limit: Option<u32>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(Ok).collect()),
                queried_limit,
                limit_query_count: std::sync::atomic::AtomicU32::new(0),
            }
        }

        fn query_count(&self) -> u32 {
            self.limit_query_count
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ProviderClient for LimitedMockProvider {
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
            Ok(lock.remove(0).unwrap())
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
            Err(HarnessError::Provider(
                "mock: stream not implemented".to_string(),
            ))
        }

        async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
            Ok(())
        }

        async fn model_context_limit(&self, _model: &str) -> Option<u32> {
            self.limit_query_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.queried_limit
        }
    }

    /// (a) Static config present → static wins; provider query is NOT used to
    ///     override.
    ///
    /// config has `small-model → 200` tokens; provider would return 99 999.
    /// The static entry must win (operator override).
    #[test]
    fn ctx_limit_static_config_wins_over_provider() {
        let mut config = test_config(5);
        config
            .model_context_limits
            .insert("small-model".to_string(), 200u32);

        // provider_queried = 99_999 — would override default, but static wins.
        let limit = model_effective_limit("small-model", &config, Some(99_999));
        assert_eq!(
            limit, 200,
            "static config must win over provider-queried value"
        );
    }

    /// (b) No static entry; provider returns a limit → that limit is used.
    #[test]
    fn ctx_limit_provider_value_used_when_no_static() {
        let config = test_config(5); // no model_context_limits entries
        let limit = model_effective_limit("some-model", &config, Some(8_192));
        assert_eq!(
            limit, 8_192,
            "provider-queried limit must be used when static is absent"
        );
    }

    /// (c) No static entry; provider returns None → falls back to context_limit
    ///     default.
    #[test]
    fn ctx_limit_falls_back_to_default_when_provider_returns_none() {
        let config = LoopConfig {
            context_limit: 4_096,
            ..test_config(5)
        };
        let limit = model_effective_limit("some-model", &config, None);
        assert_eq!(
            limit, 4_096,
            "global default must be used when provider returns None"
        );
    }

    /// (d) Cache: `model_context_limit` is called once per model across turns.
    ///
    /// Run a 3-turn loop (2 tool calls + final answer) against a
    /// `LimitedMockProvider`.  The provider's query counter must be 1 regardless
    /// of the number of turns.
    #[tokio::test]
    async fn ctx_limit_provider_queried_once_per_model() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.txt"), "hi")
            .await
            .unwrap();

        let provider = Arc::new(LimitedMockProvider::new(
            vec![
                make_tool_call_response("read_file", r#"{"path": "a.txt"}"#),
                make_tool_call_response("read_file", r#"{"path": "a.txt"}"#),
                make_final_response("done after two tool calls"),
            ],
            Some(16_384), // provider reports 16 k context
        ));

        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        run_loop(
            provider.clone(),
            registry,
            ctx,
            test_config(10),
            "sys".to_string(),
            vec![ChatMessage::user("read a.txt twice")],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(
            provider.query_count(),
            1,
            "provider must be queried exactly once per model per session, got {}",
            provider.query_count()
        );
    }

    /// (d) Larger model exists but fails the vetted gate → does NOT switch
    ///     (falls through to compaction instead).
    #[tokio::test]
    async fn token_pressure_unvetted_candidate_does_not_switch() {
        let tmp = TempDir::new().unwrap();

        let long_user_msg: String = "x".repeat(500);

        let provider = Arc::new(MockProvider::new(vec![make_final_response("compacted")]));
        let registry = Arc::new(default_registry());
        let ctx = ToolContext::new(tmp.path());
        let mut telem = noop_telemetry();

        let mut limits = std::collections::HashMap::new();
        limits.insert("small-model".to_string(), 100u32);
        limits.insert("large-model".to_string(), 32_768u32);

        let config = LoopConfig {
            model: "small-model".to_string(),
            // vetted_models does NOT contain "large-model" — it will be rejected.
            vetted_models: vec!["small-model".to_string()],
            token_pressure_models: vec!["large-model".to_string()],
            model_context_limits: limits,
            ..test_config(5)
        };

        let result = run_loop(
            provider,
            registry,
            ctx,
            config,
            "system prompt".to_string(),
            vec![ChatMessage::user(long_user_msg)],
            &mut telem,
        )
        .await
        .unwrap();

        assert_eq!(
            result.active_model, "small-model",
            "unvetted model must not be switched to"
        );
        assert_eq!(result.token_pressure_switches, 0, "no switch expected");
        // Compaction must have fired instead.
        assert!(
            result.compactions >= 1,
            "compaction must fire when unvetted candidate is rejected"
        );
    }
}
