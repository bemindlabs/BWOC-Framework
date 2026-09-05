//! Harness error types.

use thiserror::Error;

/// Top-level error type for `bwoc-harness`.
#[derive(Debug, Error)]
pub enum HarnessError {
    /// HTTP/provider error (non-404 failure, parse error, network error).
    #[error("provider error: {0}")]
    Provider(String),

    /// Transient provider error — network connectivity, 5xx, 429/408, or
    /// timeout. These are safe to retry with exponential backoff.
    /// Non-transient errors (other 4xx, model-not-found) use other variants.
    ///
    /// `retry_after` carries the server's own `Retry-After` hint when present
    /// (a 429 usually ships one); the retry loop prefers it over its computed
    /// backoff, clamped so a hostile endpoint cannot park the run.
    #[error("transient provider error (retryable): {msg}")]
    TransientProvider {
        msg: String,
        retry_after: Option<std::time::Duration>,
    },

    /// The requested model was not found at the endpoint (HTTP 404 or absent
    /// from the models list).  The spike confirmed Ollama returns 404 for
    /// wrong model tags — surface this clearly rather than letting it
    /// manifest as a mysterious JSON parse failure.
    ///
    /// For OpenAI-compatible backends a bare 404 on the completions call is
    /// ambiguous: it means either a wrong model tag **or** a wrong endpoint path
    /// (e.g. a `baseUrl` missing the trailing `/v1`, which returns 404 for any
    /// model). The message stays general (this variant is shared with the
    /// Anthropic / CLI backends, which have no `/v1`) but scopes the endpoint
    /// hint to OpenAI-compatible endpoints so the operator doesn't chase the
    /// model tag when the endpoint is the real fault (#402).
    #[error(
        "model `{0}` not found — check the model name is correct; \
         for OpenAI-compatible endpoints also verify the endpoint URL \
         (it must end in `/v1`)"
    )]
    ModelNotFound(String),

    /// All models in the fallback chain failed or were exhausted.
    #[error("all models exhausted: tried {tried:?}; last error: {last_error}")]
    AllModelsExhausted {
        tried: Vec<String>,
        last_error: String,
    },

    /// `primaryModel: "auto"` resolution could not pick a model: either the
    /// candidate pool was empty/missing, or none of the candidates survived the
    /// availability + context-fit filters.
    #[error(
        "auto model selection found no usable candidate: {reason} \
         (candidates: {candidates:?})"
    )]
    NoAutoCandidate {
        reason: String,
        candidates: Vec<String>,
    },

    /// The configured model is not in the vetted-models allowlist and
    /// `vetted_mode` is `Enforce`.  The loop refused to start.
    #[error(
        "model `{model}` is not in the vetted-models allowlist \
         and vetted_mode=enforce; refusing to run"
    )]
    UnvettedModel { model: String },

    /// The model returned malformed or unparseable tool calls repeatedly
    /// and all retry/fallback attempts were exhausted.
    #[error("malformed tool calls from model `{model}` after {attempts} attempts")]
    MalformedToolCalls { model: String, attempts: u32 },

    /// A tool invocation failed.
    #[error("tool `{tool}` failed: {reason}")]
    ToolExecution { tool: String, reason: String },

    /// A file-path was rejected by the path confinement check.
    #[error("path `{0}` is outside the allowed working directory")]
    PathEscape(String),

    /// The agent loop hit the maximum iteration limit.
    #[error("agent loop exceeded max iterations ({0})")]
    MaxIterations(u32),

    /// A per-run budget (token or cost) was exhausted — the hard gate (HV2-6).
    #[error("budget exceeded: {kind} usage {used} > limit {limit}")]
    BudgetExceeded {
        kind: &'static str,
        used: f64,
        limit: f64,
    },

    /// I/O error from the filesystem tools or config loading.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization / deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Catch-all for unexpected conditions.
    #[error("{0}")]
    Other(String),
}

impl HarnessError {
    /// Construct a transient provider error with no server retry hint — the
    /// common case (network failure, timeout, a 5xx without a `Retry-After`).
    /// Keeps the ~dozen call sites terse now that the variant carries a second
    /// field.
    pub fn transient(msg: impl Into<String>) -> Self {
        HarnessError::TransientProvider {
            msg: msg.into(),
            retry_after: None,
        }
    }

    /// Construct a transient provider error carrying the server's own
    /// `Retry-After` hint (parsed from the HTTP response).
    pub fn transient_after(
        msg: impl Into<String>,
        retry_after: Option<std::time::Duration>,
    ) -> Self {
        HarnessError::TransientProvider {
            msg: msg.into(),
            retry_after,
        }
    }

    /// Returns `true` if this error is transient and safe to retry.
    ///
    /// Transient: network failures, 5xx / 429 / 408 responses, request
    /// timeouts. Non-transient: other 4xx, model-not-found, malformed tool
    /// calls, I/O errors from the harness itself, path escapes.
    pub fn is_transient(&self) -> bool {
        matches!(self, HarnessError::TransientProvider { .. })
    }

    /// The server's `Retry-After` hint, if this is a transient error that
    /// carried one. The retry loop prefers this over its computed backoff.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            HarnessError::TransientProvider { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

/// Convenience alias.
pub type HarnessResult<T> = Result<T, HarnessError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_not_found_message_names_both_causes() {
        // A 404 is ambiguous between a wrong model name and a wrong endpoint;
        // the message must point at both, but keep the endpoint hint scoped to
        // OpenAI-compatible backends (this variant is shared with others) (#402).
        let msg = HarnessError::ModelNotFound("qwen3:8b".to_string()).to_string();
        assert!(msg.contains("qwen3:8b"), "names the model: {msg}");
        assert!(msg.contains("model name"), "mentions the model name: {msg}");
        assert!(
            msg.contains("OpenAI-compatible"),
            "scopes the endpoint hint: {msg}"
        );
        assert!(msg.contains("/v1"), "mentions the endpoint path: {msg}");
    }
}
