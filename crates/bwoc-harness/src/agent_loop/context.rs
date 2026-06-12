//! Context-window + token management and malformed-tool detection.
//!
//! Helpers the loop driver (`super`) calls to decide when to auto-switch to a
//! larger model under token pressure, when to compact, and when a model's
//! tool-call output is malformed enough to trigger fallback. Pure functions
//! (no I/O); split out so the loop driver stays focused on orchestration.

use std::collections::HashMap;

use crate::provider::{ChatMessage, ToolCall};

use super::{LoopConfig, VettedMode};

/// How many consecutive malformed-tool-call responses from a model before
/// triggering fallback to the next model in the chain.
pub(super) const MALFORMED_TOOL_CALL_THRESHOLD: u32 = 2;

/// Leave this fraction of the context limit as headroom before compacting.
/// Compaction triggers when `context_tokens > context_limit * (1 - headroom)`.
pub(super) const CONTEXT_HEADROOM_FRAC: f64 = 0.10;

/// Detect malformed tool calls: empty ID or unparseable JSON arguments.
///
/// The spike (llama3.2 3B) produced calls with empty IDs and garbled JSON.
/// Detecting this early prevents the history from filling with garbage that
/// confuses the next turn.
pub(super) fn has_malformed_tool_calls(calls: &[ToolCall]) -> bool {
    calls.iter().any(|c| {
        c.id.is_empty() || serde_json::from_str::<serde_json::Value>(&c.function.arguments).is_err()
    })
}

/// Estimate the total context tokens from the current history.
///
/// Rough heuristic: 1 token ≈ 4 characters (works for English + code; good
/// enough for a compaction trigger — exact counts come from `usage` fields).
pub(super) fn estimate_context_tokens(history: &[ChatMessage]) -> u32 {
    let total_chars: usize = history
        .iter()
        .map(|m| m.content.as_deref().map_or(0, |c| c.len()))
        .sum();
    (total_chars / 4) as u32
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
///
/// [`ProviderClient::model_context_limit`]: crate::provider::ProviderClient::model_context_limit
pub(super) fn model_effective_limit(
    model: &str,
    config: &LoopConfig,
    provider_queried: Option<u32>,
) -> u32 {
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
///    listed in it), unless `vetted_mode` is `Off` (gate skipped entirely).
///
/// Returns `Some(model_id)` if found, `None` otherwise.
///
/// Site 3 — vetted-mode behaviour for token-pressure candidates:
/// - `Off` — accept any candidate with a larger limit; no vetted check.
/// - `Warn` — skip unvetted candidates with a warning (existing behaviour).
/// - `Enforce` — same as `Warn` for candidates; the hard-refuse only applies
///   to the primary model (site 1).
///
/// `provider_cache` is the per-session cache populated by
/// [`ProviderClient::model_context_limit`] queries.
///
/// [`ProviderClient::model_context_limit`]: crate::provider::ProviderClient::model_context_limit
pub(super) fn find_larger_vetted_model(
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

        // Site 3: apply vetted gate according to mode.
        // Off → skip the gate entirely (any model is acceptable).
        // Warn / Enforce → skip unvetted candidates with a warning.
        if config.vetted_mode != VettedMode::Off
            && !config.vetted_models.is_empty()
            && !config.vetted_models.contains(candidate)
        {
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
