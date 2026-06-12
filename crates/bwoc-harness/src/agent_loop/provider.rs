//! Provider call + transient-retry layer for the agent loop.
//!
//! `call_with_retry_v2` wraps a single provider turn (`call_provider_once`,
//! which dispatches to the streaming or blocking path) with bounded exponential
//! backoff on transient errors. Non-transient errors fail fast. Split out of the
//! loop driver (`super`) so the retry/backoff policy lives in one place.

use crate::error::{HarnessError, HarnessResult};
use crate::provider::{ChatMessage, ProviderClient};

use super::execute::stream_and_accumulate;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum retries for a single transient provider error before giving up on
/// the current model and falling back (or returning an error).
pub(super) const MAX_TRANSIENT_RETRIES: u32 = 3;

/// Base backoff in milliseconds.  Doubles each retry up to `MAX_BACKOFF_MS`.
pub(super) const BACKOFF_BASE_MS: u64 = 200;

/// Maximum backoff cap in milliseconds (≈ 3 seconds).
pub(super) const MAX_BACKOFF_MS: u64 = 3_200;

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
        // Streaming now exposes usage via stream_options.include_usage (HV2-7).
        stream_and_accumulate(provider, messages, tools, model).await
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

/// Compute bounded exponential backoff.
///
/// attempt 1 → 200ms, 2 → 400ms, 3 → 800ms, 4 → 1600ms … capped at 3200ms.
pub(super) fn backoff_ms(attempt: u32) -> u64 {
    let raw = BACKOFF_BASE_MS.saturating_mul(1u64 << attempt.min(10));
    raw.min(MAX_BACKOFF_MS)
}
