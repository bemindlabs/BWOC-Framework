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
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use reqwest::Client;
use serde::Serialize;
use serde_json::{Value, json};

use super::types::{ChatCompletion, ChatMessage, Role, StreamChunk, Tool, ToolCall};
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

    /// List the model IDs this endpoint currently serves.
    ///
    /// Used by `primaryModel: "auto"` resolution to filter the candidate pool
    /// down to models the provider can actually reach. Best-effort, like
    /// [`Self::model_context_limit`]: a network/parse failure or an endpoint
    /// that doesn't implement model listing returns an **empty** Vec, which the
    /// resolver reads as "availability unknown — don't filter on it" rather than
    /// "nothing available". The default returns empty so non-listing providers
    /// degrade gracefully.
    async fn list_models(&self) -> Vec<String> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Real implementation
// ---------------------------------------------------------------------------

/// Default OpenAI-compatible endpoint (Ollama's local server). Single source
/// of truth — the CLI `--endpoint` default, worker config defaults, and
/// [`OllamaClient::default_endpoint`] all reference this so they cannot drift.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434/v1";

/// Default OpenRouter base URL. OpenRouter is a hosted, OpenAI-compatible
/// aggregator that routes one key to any vendor's models (`openai/…`,
/// `anthropic/…`, `google/…`, …). It speaks the exact OpenAI shape this client
/// already implements; the only addition over plain Ollama is bearer auth.
pub const OPENROUTER_DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/v1";

/// Env var carrying the OpenRouter API key (OpenRouter's documented convention).
pub const OPENROUTER_API_KEY_ENV: &str = "OPENROUTER_API_KEY";

/// Resolve the OpenRouter API key: `OPENROUTER_API_KEY` env wins, else the
/// per-user `~/.bwoc/secrets.toml` `[openrouter] api_key` (chmod-600 guarded).
/// Reuses the shared resolver so the security guard is identical to Anthropic's.
pub fn resolve_openrouter_api_key() -> String {
    super::anthropic::resolve_provider_api_key(OPENROUTER_API_KEY_ENV, "openrouter")
}

/// OpenRouter's optional app-attribution / ranking headers. Sent on every
/// request so usage shows up under the BWOC project on openrouter.ai; harmless
/// to any other OpenAI-compatible endpoint, which simply ignores them.
pub fn openrouter_headers() -> Vec<(String, String)> {
    vec![
        (
            "HTTP-Referer".to_string(),
            "https://github.com/bemindlabs/bwoc".to_string(),
        ),
        ("X-Title".to_string(), "BWOC".to_string()),
    ]
}

/// Env var carrying the LiteLLM proxy base URL (LiteLLM's own documented
/// `LITELLM_API_BASE` convention). LiteLLM is self-hosted and has no canonical
/// URL, so the base is resolved from the environment rather than hardcoded —
/// keeping the framework backend-neutral and portable (a deployment points this
/// at its own proxy; no infra host ever enters the source).
pub const LITELLM_API_BASE_ENV: &str = "LITELLM_API_BASE";

/// Fallback LiteLLM proxy endpoint — the proxy's documented default port — used
/// only when neither `--endpoint` nor `LITELLM_API_BASE` is set.
pub const LITELLM_DEFAULT_ENDPOINT: &str = "http://localhost:4000/v1";

/// Env var carrying the LiteLLM API key (a master or virtual key). Optional: a
/// keyless local proxy needs none.
pub const LITELLM_API_KEY_ENV: &str = "LITELLM_API_KEY";

/// Resolve the LiteLLM proxy base URL for the `litellm` backend when the caller
/// left `--endpoint` unset: `LITELLM_API_BASE` env wins, else
/// [`LITELLM_DEFAULT_ENDPOINT`]. Never a hardcoded infra host.
pub fn resolve_litellm_endpoint() -> String {
    std::env::var(LITELLM_API_BASE_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| LITELLM_DEFAULT_ENDPOINT.to_string())
}

/// Resolve the LiteLLM API key: `LITELLM_API_KEY` env wins, else the per-user
/// `~/.bwoc/secrets.toml` `[litellm] api_key` (chmod-600 guarded). Reuses the
/// shared resolver so the security guard is identical to Anthropic's. Returns an
/// empty string when unconfigured — the backend treats the key as **optional**
/// (unlike OpenRouter's required key), so a keyless local proxy just works.
pub fn resolve_litellm_api_key() -> String {
    super::anthropic::resolve_provider_api_key(LITELLM_API_KEY_ENV, "litellm")
}

/// Per-request timeout applied to every HTTP call the client makes.
///
/// Without it, `reqwest::Client::new()` has no request timeout, so a hung
/// blocking completion never resolves — it bypasses the agent loop's
/// retry/backoff/budget logic entirely. Bounding the request lets the timeout
/// surface as a `reqwest` error, which the `send().await` arms below map to
/// [`HarnessError::TransientProvider`] so the existing retry path can see it.
///
/// `pub(crate)` so the Anthropic client — which cannot use reqwest's
/// `.timeout()` (it would cut the SSE stream body) and instead wraps its
/// non-streaming `complete()` in a `tokio::time::timeout` — shares this single
/// source of truth rather than duplicating the value.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Real HTTP client speaking the OpenAI-compat API.
///
/// Default endpoint: [`DEFAULT_ENDPOINT`] (`http://localhost:11434/v1`, Ollama).
#[derive(Debug, Clone)]
pub struct OllamaClient {
    pub base_url: String,
    /// Optional `reasoning_effort` sent on every completion request. `None`
    /// leaves the field off the body (provider default). Set via
    /// [`Self::with_reasoning_effort`] from the agent's manifest.
    reasoning_effort: Option<String>,
    /// Optional bearer token attached as `Authorization: Bearer <key>` to every
    /// request. `None` (the Ollama default) sends no auth header — a plain local
    /// Ollama server needs none. Set via [`Self::with_api_key`] for hosted
    /// OpenAI-compatible providers that require a key (e.g. OpenRouter).
    api_key: Option<String>,
    /// Extra HTTP headers sent on every request. Empty by default. Used for
    /// provider-specific attribution headers (e.g. OpenRouter's optional
    /// `HTTP-Referer` / `X-Title` ranking headers). Set via [`Self::with_headers`].
    extra_headers: Vec<(String, String)>,
    client: Client,
}

impl OllamaClient {
    /// Create a client with an explicit base URL.
    pub fn new(base_url: impl Into<String>) -> Self {
        // A request timeout (vs. `Client::new()`'s unbounded default) ensures a
        // hung completion fails instead of stalling forever; `build()` only
        // errors on TLS/system init, so fall back to the default client.
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.into(),
            reasoning_effort: None,
            api_key: None,
            extra_headers: Vec::new(),
            client,
        }
    }

    /// Create a client pointing at the default Ollama endpoint.
    pub fn default_endpoint() -> Self {
        Self::new(DEFAULT_ENDPOINT)
    }

    /// Set the `reasoning_effort` sent on every completion request (OpenAI-
    /// compatible effort control). `None` is a no-op. Returns `self` for
    /// chaining at construction.
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
        self
    }

    /// Set the bearer token attached as `Authorization: Bearer <key>` to every
    /// request. `None` or an empty/whitespace key leaves auth off (Ollama
    /// behaviour). Returns `self` for chaining at construction.
    pub fn with_api_key(mut self, api_key: Option<String>) -> Self {
        self.api_key = api_key.filter(|k| !k.trim().is_empty());
        self
    }

    /// Set extra HTTP headers attached to every request (e.g. OpenRouter's
    /// optional `HTTP-Referer` / `X-Title` ranking headers). Returns `self` for
    /// chaining at construction.
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_headers = headers;
        self
    }

    /// Apply this client's auth + extra headers to an outgoing request builder.
    ///
    /// Attaches `Authorization: Bearer <key>` when an [`Self::api_key`] is set,
    /// then every [`Self::extra_headers`] entry. A `None` key leaves the request
    /// unauthenticated (plain Ollama), so the default path is byte-for-byte
    /// unchanged. Called at every request site so completion, streaming, and the
    /// `/models` probes all carry the same credentials.
    fn auth(&self, mut rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref key) = self.api_key {
            rb = rb.bearer_auth(key);
        }
        for (name, value) in &self.extra_headers {
            rb = rb.header(name, value);
        }
        rb
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
        let body = build_request_body(
            messages,
            tools,
            model,
            false,
            self.reasoning_effort.as_deref(),
        );

        let resp = self
            .auth(self.client.post(self.completions_url()).json(&body))
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

        let body = build_request_body(
            messages,
            tools,
            model,
            true,
            self.reasoning_effort.as_deref(),
        );

        let resp = self
            .auth(self.client.post(self.completions_url()).json(&body))
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
            .auth(self.client.get(self.models_url()))
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

    /// List served model IDs via `GET /v1/models`.
    ///
    /// Mirrors [`Self::validate_model`]'s parsing of the OpenAI-compat list
    /// shape (`{"data": [{"id": ...}, ...]}`). Any failure — request error,
    /// non-2xx, or parse error — yields an empty Vec so the auto-resolver
    /// degrades to "availability unknown" instead of failing the run.
    async fn list_models(&self) -> Vec<String> {
        let Ok(resp) = self.auth(self.client.get(self.models_url())).send().await else {
            return Vec::new();
        };
        if !resp.status().is_success() {
            return Vec::new();
        }
        let Ok(body) = resp.json::<Value>().await else {
            return Vec::new();
        };
        body["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
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

/// Egress projection of a [`ChatMessage`] — the exact OpenAI-compatible wire
/// shape, deliberately **without** the `principal` provenance field.
///
/// Phase 5 t1 (yudi's ruling): a `ChatMessage` is never serialized directly into
/// the provider body — its `principal` is an internal trust stamp that some
/// OpenAI-compatible endpoints would reject as an unknown field, and which must
/// never leak off-box. Serializing through this borrowing DTO is the single
/// chokepoint that strips it, rather than a blanket `skip_serializing` (which
/// would also drop it from the on-disk session, where it must be retained).
#[derive(Serialize)]
struct EgressMessage<'a> {
    role: &'a Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<&'a Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a String>,
}

impl<'a> From<&'a ChatMessage> for EgressMessage<'a> {
    fn from(m: &'a ChatMessage) -> Self {
        Self {
            role: &m.role,
            content: m.content.as_ref(),
            tool_calls: m.tool_calls.as_ref(),
            tool_call_id: m.tool_call_id.as_ref(),
            name: m.name.as_ref(),
        }
    }
}

fn build_request_body(
    messages: Vec<ChatMessage>,
    tools: Vec<Tool>,
    model: &str,
    stream: bool,
    reasoning_effort: Option<&str>,
) -> Value {
    // Project to the egress DTO so `principal` never reaches the provider.
    let egress: Vec<EgressMessage> = messages.iter().map(EgressMessage::from).collect();
    let mut body = json!({
        "model": model,
        "messages": egress,
        "stream": stream,
    });

    if !tools.is_empty() {
        body["tools"] = serde_json::to_value(tools).unwrap_or(Value::Array(vec![]));
    }

    // OpenAI-compatible effort control. Only emitted when configured; providers
    // that don't understand it (e.g. plain Ollama) ignore the extra field.
    if let Some(effort) = reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }

    // Ask for token usage on the streaming path (HV2-7).  Providers that don't
    // support it ignore the field; those that do emit a final usage-only chunk.
    if stream {
        body["stream_options"] = json!({ "include_usage": true });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_body_omits_reasoning_effort_by_default() {
        let body = build_request_body(
            vec![ChatMessage::user("task")],
            Vec::new(),
            "gpt-5.5",
            false,
            None,
        );

        assert_eq!(body["model"], "gpt-5.5");
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn request_body_includes_configured_reasoning_effort() {
        let body = build_request_body(
            vec![ChatMessage::user("task")],
            Vec::new(),
            "gpt-5.5",
            false,
            Some("medium"),
        );

        assert_eq!(body["reasoning_effort"], "medium");
    }

    #[test]
    fn egress_body_never_leaks_principal() {
        // Phase 5 t1: the provider body must carry no provenance field. Build a
        // body from messages spanning every constructor and assert the wire form
        // has neither `principal` nor its `kind` tag, while the real OpenAI
        // fields survive.
        let body = build_request_body(
            vec![
                ChatMessage::system("you are an agent"),
                ChatMessage::operator("hi"),
                ChatMessage::tool_result("call-1", "read_file", "file body"),
            ],
            Vec::new(),
            "gpt-5.5",
            false,
            None,
        );
        let wire = serde_json::to_string(&body).unwrap();
        assert!(!wire.contains("principal"), "principal leaked: {wire}");
        assert!(!wire.contains("\"kind\""), "provenance tag leaked: {wire}");
        // The OpenAI-compat fields are still present.
        assert!(wire.contains("\"role\":\"system\""));
        assert!(wire.contains("\"tool_call_id\":\"call-1\""));
    }

    #[test]
    fn with_api_key_filters_blank_keys() {
        // A `Some("")` / whitespace key must be treated as "no auth" so a
        // mis-set env var doesn't send `Authorization: Bearer ` (empty).
        assert!(
            OllamaClient::new(DEFAULT_ENDPOINT)
                .with_api_key(None)
                .api_key
                .is_none()
        );
        assert!(
            OllamaClient::new(DEFAULT_ENDPOINT)
                .with_api_key(Some("  ".to_string()))
                .api_key
                .is_none()
        );
        assert_eq!(
            OllamaClient::new(DEFAULT_ENDPOINT)
                .with_api_key(Some("sk-or-abc".to_string()))
                .api_key
                .as_deref(),
            Some("sk-or-abc")
        );
    }

    #[test]
    fn auth_attaches_bearer_and_extra_headers_only_when_set() {
        use reqwest::header::AUTHORIZATION;

        // No key → no Authorization header (plain Ollama path unchanged).
        let plain = OllamaClient::new(DEFAULT_ENDPOINT);
        let req = plain
            .auth(plain.client.get(plain.models_url()))
            .build()
            .expect("request builds");
        assert!(req.headers().get(AUTHORIZATION).is_none());

        // Key + extra headers → both present (OpenRouter path).
        let authed = OllamaClient::new("https://openrouter.ai/api/v1")
            .with_api_key(Some("sk-or-xyz".to_string()))
            .with_headers(vec![("X-Title".to_string(), "bwoc".to_string())]);
        let req = authed
            .auth(authed.client.get(authed.models_url()))
            .build()
            .expect("request builds");
        assert_eq!(
            req.headers().get(AUTHORIZATION).unwrap(),
            "Bearer sk-or-xyz"
        );
        assert_eq!(req.headers().get("X-Title").unwrap(), "bwoc");
    }

    #[test]
    fn litellm_defaults_are_neutral() {
        // The fallback is the LiteLLM proxy's documented default port — never a
        // hardcoded infra host, so the framework stays portable + backend-neutral.
        assert_eq!(LITELLM_DEFAULT_ENDPOINT, "http://localhost:4000/v1");
        assert_eq!(LITELLM_API_BASE_ENV, "LITELLM_API_BASE");
        assert_eq!(LITELLM_API_KEY_ENV, "LITELLM_API_KEY");
        // Endpoint resolution always yields a usable base (env override or the
        // default), never an empty string.
        assert!(!resolve_litellm_endpoint().trim().is_empty());
    }

    #[test]
    fn client_builds_with_request_timeout() {
        // The const is the bound a hung completion fails against; if it were
        // zero/unset the request could stall past the retry/budget logic.
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(120));

        // `OllamaClient::new` must construct via the timeout-bearing builder, not
        // fall back to the unbounded default. `Client::builder().timeout(..)` only
        // fails on TLS/system init, so a successful build here proves the path.
        assert!(Client::builder().timeout(REQUEST_TIMEOUT).build().is_ok());

        // Construction itself stays infallible.
        let _ = OllamaClient::new(DEFAULT_ENDPOINT);
    }
}

// ---------------------------------------------------------------------------
// async_trait re-export helper — keep the dep inside this crate
// ---------------------------------------------------------------------------
// We use async_trait from the futures ecosystem; declare it in Cargo.toml.
// The attribute is applied above — this comment is a reminder, not code.
