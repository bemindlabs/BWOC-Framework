//! Harness error types.

use thiserror::Error;

/// Top-level error type for `bwoc-harness`.
#[derive(Debug, Error)]
pub enum HarnessError {
    /// HTTP/provider error (non-404 failure, parse error, network error).
    #[error("provider error: {0}")]
    Provider(String),

    /// Transient provider error — network connectivity, 5xx, or timeout.
    /// These are safe to retry with exponential backoff.
    /// Non-transient errors (4xx, model-not-found) use other variants.
    #[error("transient provider error (retryable): {0}")]
    TransientProvider(String),

    /// The requested model was not found at the endpoint (HTTP 404 or absent
    /// from the models list).  The spike confirmed Ollama returns 404 for
    /// wrong model tags — surface this clearly rather than letting it
    /// manifest as a mysterious JSON parse failure.
    #[error("model not found: `{0}` — check the model tag with `ollama list`")]
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
    /// Returns `true` if this error is transient and safe to retry.
    ///
    /// Transient: network failures, 5xx responses, request timeouts.
    /// Non-transient: 4xx, model-not-found, malformed tool calls, I/O errors
    /// from the harness itself, path escapes.
    pub fn is_transient(&self) -> bool {
        matches!(self, HarnessError::TransientProvider(_))
    }
}

/// Convenience alias.
pub type HarnessResult<T> = Result<T, HarnessError>;
