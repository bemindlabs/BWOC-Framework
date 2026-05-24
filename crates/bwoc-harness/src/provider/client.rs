//! OpenAI-compatible HTTP client.
//!
//! [`ProviderClient`] is the injectable trait — tests swap in a mock.
//! [`OllamaClient`] is the real implementation backed by `reqwest`.
//!
//! ## Retry classification
//!
//! HTTP errors are split into two buckets:
//!
//! - **Transient** (retry-safe): connection errors, 5xx responses, request
//!   timeouts.  Callers with exponential-backoff retry loops use
//!   [`HarnessError::is_transient`] to gate retries.
//! - **Fatal** (fail-fast): 404 (`ModelNotFound`), other 4xx, JSON parse
//!   failures.  Retrying these is pointless and misleading.
//!
//! The retry loop itself lives in `agent_loop` — the provider just classifies.

use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use reqwest::Client;
use serde_json::{Value, json};

use super::types::{ChatCompletion, ChatMessage, StreamChunk, Tool};
use crate::error::HarnessError;

// ---------------------------------------------------------------------------
// Trait (injectable / mockable)
// ---------------------------------------------------------------------------

/// The interface the agent loop uses to call the model.
///
/// Implementors: [`OllamaClient`] (real HTTP) + any mock in tests.
#[async_trait]
pub trait ProviderClient: Send + Sync {
    /// Blocking (stream=false) completion.  Returns the full response.
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
        model: &str,
    ) -> Result<ChatCompletion, HarnessError>;

    /// Streaming (stream=true) completion.  Returns an SSE chunk stream.
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
        model: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>, HarnessError>;

    /// Validate that `model` is available at this endpoint.
    /// Returns `Ok(())` if found, `Err(HarnessError::ModelNotFound)` otherwise.
    async fn validate_model(&self, model: &str) -> Result<(), HarnessError>;

    /// Query the provider for the context-window size of `model`.
    ///
    /// Best-effort: network or parse failures return `None` rather than
    /// propagating an error — the loop treats `None` as "unknown" and falls
    /// back to the configured default.
    ///
    /// The default implementation returns `None` so that providers that do
    /// not expose this information degrade gracefully without any code change.
    async fn model_context_limit(&self, _model: &str) -> Option<u32> {
        None
    }
}

// ---------------------------------------------------------------------------
// Real implementation
// ---------------------------------------------------------------------------

/// Real HTTP client speaking the OpenAI-compat API.
///
/// Default endpoint: `http://localhost:11434/v1` (Ollama).
#[derive(Debug, Clone)]
pub struct OllamaClient {
    pub base_url: String,
    client: Client,
}

impl OllamaClient {
    /// Create a client with an explicit base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: Client::new(),
        }
    }

    /// Create a client pointing at the default Ollama endpoint.
    pub fn default_endpoint() -> Self {
        Self::new("http://localhost:11434/v1")
    }

    fn completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    /// Derive the Ollama native API root from the configured base URL.
    ///
    /// `base_url` ends in `/v1` (OpenAI-compat path); strip it to get the
    /// Ollama root so we can reach native endpoints like `POST /api/show`.
    fn ollama_root(&self) -> String {
        self.base_url
            .strip_suffix("/v1")
            .unwrap_or(&self.base_url)
            .to_string()
    }

    /// URL for Ollama's native model-info endpoint.
    fn show_url(&self) -> String {
        format!("{}/api/show", self.ollama_root())
    }
}

#[async_trait]
impl ProviderClient for OllamaClient {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
        model: &str,
    ) -> Result<ChatCompletion, HarnessError> {
        let body = build_request_body(messages, tools, model, false);

        let resp = self
            .client
            .post(self.completions_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| HarnessError::TransientProvider(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(HarnessError::ModelNotFound(model.to_string()));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            // 5xx = transient; 4xx = fatal.
            return Err(classify_http_error(status.as_u16(), &text));
        }

        resp.json::<ChatCompletion>()
            .await
            .map_err(|e| HarnessError::Provider(format!("Failed to parse response: {e}")))
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
        model: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>, HarnessError>
    {
        use bytes::Bytes;
        use futures_util::{StreamExt, TryStreamExt};

        let body = build_request_body(messages, tools, model, true);

        let resp = self
            .client
            .post(self.completions_url())
            .json(&body)
            .send()
            .await
            .map_err(|e| HarnessError::TransientProvider(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(HarnessError::ModelNotFound(model.to_string()));
        }
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(classify_http_error(status.as_u16(), &text));
        }

        // Parse SSE: each line starting with "data: " is a JSON chunk.
        // "[DONE]" signals end of stream.
        let byte_stream = resp.bytes_stream();
        let stream = byte_stream
            .map_err(|e| HarnessError::Provider(format!("Stream error: {e}")))
            .flat_map(|chunk_result: Result<Bytes, HarnessError>| {
                let lines: Vec<Result<StreamChunk, HarnessError>> = match chunk_result {
                    Err(e) => vec![Err(e)],
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        text.lines()
                            .filter_map(|line| {
                                let line = line.trim();
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data == "[DONE]" {
                                        return None; // end-of-stream sentinel
                                    }
                                    Some(serde_json::from_str::<StreamChunk>(data).map_err(|e| {
                                        HarnessError::Provider(format!(
                                            "SSE parse error on `{data}`: {e}"
                                        ))
                                    }))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    }
                };
                futures_util::stream::iter(lines)
            });

        Ok(Box::pin(stream))
    }

    async fn validate_model(&self, model: &str) -> Result<(), HarnessError> {
        // GET /v1/models returns a list; check the model is present.
        let resp = self
            .client
            .get(self.models_url())
            .send()
            .await
            .map_err(|e| HarnessError::Provider(format!("Model list request failed: {e}")))?;

        if !resp.status().is_success() {
            // If the endpoint doesn't implement /models, fall through and
            // let the first completion call surface the 404.
            return Ok(());
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|e| HarnessError::Provider(format!("Failed to parse models list: {e}")))?;

        let found = body["data"]
            .as_array()
            .map(|arr| arr.iter().any(|m| m["id"].as_str() == Some(model)))
            .unwrap_or(false);

        if found {
            Ok(())
        } else {
            Err(HarnessError::ModelNotFound(model.to_string()))
        }
    }

    /// Query Ollama's native `POST /api/show` endpoint for the model's
    /// context-window size.
    ///
    /// Ollama returns a JSON object where the context length appears in one
    /// of two places (in priority order):
    ///
    /// 1. `model_info["llama.context_length"]` (or similar architecture
    ///    prefix — we scan all keys ending in `".context_length"`).
    /// 2. The `parameters` string, which contains `num_ctx <N>` lines when
    ///    the model was loaded with a custom context override.
    ///
    /// If neither is present, or if the request fails for any reason, we
    /// return `None` — best-effort, never hard-fails the loop.
    async fn model_context_limit(&self, model: &str) -> Option<u32> {
        let body = json!({"name": model});

        let resp = self
            .client
            .post(self.show_url())
            .json(&body)
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let data: Value = resp.json().await.ok()?;

        // Priority 1: model_info object — scan for any key ending in
        // ".context_length" (covers llama, mistral, gemma architecture prefixes).
        if let Some(info) = data.get("model_info").and_then(|v| v.as_object()) {
            for (key, val) in info {
                if key.ends_with(".context_length") {
                    if let Some(n) = val.as_u64() {
                        return u32::try_from(n).ok();
                    }
                }
            }
        }

        // Priority 2: parameters string — look for a `num_ctx <N>` line.
        if let Some(params) = data.get("parameters").and_then(|v| v.as_str()) {
            for line in params.lines() {
                let mut parts = line.split_whitespace();
                if parts.next() == Some("num_ctx") {
                    if let Some(n_str) = parts.next() {
                        if let Ok(n) = n_str.parse::<u32>() {
                            return Some(n);
                        }
                    }
                }
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_request_body(
    messages: Vec<ChatMessage>,
    tools: Vec<Tool>,
    model: &str,
    stream: bool,
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": stream,
    });

    if !tools.is_empty() {
        body["tools"] = serde_json::to_value(tools).unwrap_or(Value::Array(vec![]));
    }

    body
}

// ---------------------------------------------------------------------------
// HTTP error classification helper
// ---------------------------------------------------------------------------

/// Classify an HTTP error as transient (5xx) or fatal (4xx).
///
/// - **5xx** — server-side error, may be transient: retry with backoff.
/// - **4xx** (non-404) — client-side error (bad request, auth failure, rate
///   limit exceeded with no retry-after, etc.) — fail fast.
///
/// 404 is handled before this function is called and maps to
/// [`HarnessError::ModelNotFound`].
pub(crate) fn classify_http_error(status: u16, body: &str) -> HarnessError {
    if status >= 500 {
        HarnessError::TransientProvider(format!("HTTP {status}: {body}"))
    } else {
        HarnessError::Provider(format!("HTTP {status}: {body}"))
    }
}

// ---------------------------------------------------------------------------
// async_trait re-export helper — keep the dep inside this crate
// ---------------------------------------------------------------------------
// We use async_trait from the futures ecosystem; declare it in Cargo.toml.
// The attribute is applied above — this comment is a reminder, not code.
