//! Anthropic (Claude) provider — native Messages API behind the same
//! [`ProviderClient`] trait the OpenAI-compatible [`OllamaClient`] implements.
//!
//! The harness, chat session, and agent loop are written against
//! [`ProviderClient`] + the OpenAI-shaped [`super::types`] structs. This client
//! is a pure **translation layer**: it maps those structs to the Anthropic
//! `POST /v1/messages` request, calls Claude, and maps the response (and the
//! SSE stream) back into [`ChatCompletion`] / [`StreamChunk`] so every existing
//! `chat_proto` event (Token / ToolCall / ToolResult / TurnEnd) flows unchanged.
//!
//! ## Shape differences this layer bridges
//!
//! - **System prompt** is a top-level field, not a `system` message — we lift
//!   every [`Role::System`] message out into `system`.
//! - **Content is block-structured**: assistant `tool_use` blocks carry the
//!   call; the result comes back in a **user** message as a `tool_result`
//!   block. We merge consecutive [`Role::Tool`] messages into one user turn,
//!   which is the shape Claude requires.
//! - **Tools** use `input_schema` rather than `function.parameters`.
//! - **Streaming** is a typed event protocol (`message_start` … `message_stop`)
//!   rather than OpenAI's `choices[].delta` chunks; we re-emit it as
//!   [`StreamChunk`]s the accumulator in `chat_session`/`agent_loop` understands.
//!
//! Auth is the `x-api-key` header. The key is resolved from the
//! `ANTHROPIC_API_KEY` env var (Anthropic SDK convention), falling back to
//! `~/.bwoc/secrets.toml` (`[anthropic] api_key`) so a GUI-launched `bwoc-chat`
//! works without an exported env. No key → a clear error at `validate_model`.

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use reqwest::Client;
use serde_json::{Value, json};

use super::client::ProviderClient;
use super::types::{
    ChatCompletion, ChatMessage, Choice, Delta, FinishReason, FunctionCall, FunctionDelta, Role,
    StreamChunk, StreamDelta, Tool, ToolCall, ToolCallDelta, Usage,
};
use crate::error::HarnessError;

/// Default Anthropic API base. Used when no `--endpoint` (or an unrelated
/// Ollama default) is supplied for a Claude-backed agent.
pub const ANTHROPIC_DEFAULT_ENDPOINT: &str = "https://api.anthropic.com";

/// API version pin (Anthropic's `anthropic-version` header).
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Env var carrying the API key — the Anthropic SDK convention.
const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// User home directory: `$HOME` on Unix, `%USERPROFILE%` on Windows — mirrors
/// the `home_dir()` helpers elsewhere in the workspace (no extra crate), so the
/// secrets fallback works on Windows where `HOME` is usually unset.
///
/// `pub(crate)` so sibling providers (e.g. the OpenRouter wiring) reuse the same
/// per-user secrets location.
pub(crate) fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

/// Resolve the API key: the `ANTHROPIC_API_KEY` env var wins; otherwise fall
/// back to the gitignored `~/.bwoc/secrets.toml` (`[anthropic] api_key`).
///
/// Unlike the Jira/figma resolvers — which read a **per-workspace**
/// `<workspace>/.bwoc/secrets.toml` — the Anthropic key is intentionally a
/// **per-user** secret keyed off the home directory, so a GUI-launched
/// `bwoc-chat` (with no workspace cwd) still finds it. The same `0600`
/// permission guard is applied (see [`api_key_from_secrets`]). Returns an empty
/// string when neither source has a key — surfaced as a clear error at first use.
fn resolve_api_key() -> String {
    resolve_provider_api_key(API_KEY_ENV, "anthropic")
}

/// Resolve a provider API key: the `env_var` wins; otherwise fall back to the
/// gitignored per-user `~/.bwoc/secrets.toml` `[<section>] api_key`.
///
/// Shared by every HTTP provider that needs a key (Anthropic via
/// [`resolve_api_key`], OpenRouter via [`resolve_openrouter_api_key`]). The key
/// is intentionally a **per-user** secret keyed off the home directory — not a
/// per-workspace one — so a GUI-launched `bwoc-chat` (no workspace cwd) still
/// finds it. The same `0600` permission guard applies (see
/// [`api_key_from_secrets`]). Returns an empty string when neither source has a
/// key — surfaced as a clear error at first use.
pub(crate) fn resolve_provider_api_key(env_var: &str, section: &str) -> String {
    if let Ok(k) = std::env::var(env_var) {
        if !k.trim().is_empty() {
            return k;
        }
    }
    home_dir()
        .map(|home| home.join(".bwoc").join("secrets.toml"))
        .and_then(|p| api_key_from_secrets(&p, section))
        .unwrap_or_default()
}

/// Read `[<section>] api_key` from a `secrets.toml` at `path`. Returns `None` on
/// any error (missing file, parse failure, absent key). On Unix a
/// group/world-accessible file is **refused** with a warning — the same
/// "Adinnādāna at the file boundary" guard the Jira/figma secret resolvers
/// apply (`chmod 600` required). Pure + tested.
pub(crate) fn api_key_from_secrets(path: &std::path::Path, section: &str) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.permissions().mode() & 0o077 != 0 {
                eprintln!(
                    "bwoc-harness: ignoring {} — it is group/world-accessible; `chmod 600` it.",
                    path.display()
                );
                return None;
            }
        }
    }
    let content = std::fs::read_to_string(path).ok()?;
    let value: toml::Value = toml::from_str(&content).ok()?;
    let key = value
        .get(section)?
        .get("api_key")?
        .as_str()?
        .trim()
        .to_string();
    (!key.is_empty()).then_some(key)
}

/// `max_tokens` is **required** by the Messages API (unlike OpenAI). A coding
/// agent emits long edits, so this is generous rather than minimal; Claude
/// stops at `end_turn` well before it for normal replies.
const DEFAULT_MAX_TOKENS: u32 = 8192;

/// Per-call deadline for the **non-streaming** `complete()` path. The shared
/// client deliberately carries no reqwest `.timeout()` (it would cut a
/// legitimately-minutes-long `stream()` body mid-flight), so `complete()` guards
/// itself with an explicit deadline instead. Without it, a server that completes
/// the TCP/TLS handshake then stalls before the first byte hangs the turn
/// forever, bypassing retry/backoff/budget.
///
/// Defined as the Ollama-compatible client's `REQUEST_TIMEOUT` so there is one
/// source of truth — the compiler keeps the two in lock-step (no doc-only
/// "matches …" claim that can silently drift).
const COMPLETE_TIMEOUT: Duration = super::client::REQUEST_TIMEOUT;

/// Real HTTP client speaking Anthropic's Messages API.
#[derive(Debug, Clone)]
pub struct AnthropicClient {
    base_url: String,
    api_key: String,
    max_tokens: u32,
    /// Optional effort passed through as `output_config.effort` on every
    /// request. `None` leaves the field off (backend default). The value space
    /// is Anthropic's (`low`..`max`); the operator sets a valid literal via the
    /// manifest `reasoningEffort`, mirroring the OpenAI-compat path.
    reasoning_effort: Option<String>,
    /// Whether to mark the system prompt with `cache_control` (prompt caching).
    /// On by default; the manifest `promptCache: false` opts out. When on, the
    /// stable system prefix (which also covers the preceding `tools` block) is
    /// cached, so an agentic loop resending it pays cache-read, not full input.
    prompt_cache: bool,
    /// Whether to request adaptive extended thinking (`thinking:{type:adaptive}`)
    /// on the **non-streaming** `complete()` path. Off by default; the manifest
    /// `thinking: true` opts in. When on, `parse_completion` preserves the
    /// returned thinking blocks on the assistant message and the request builder
    /// replays them (required for the tool path — see [`ChatMessage::thinking_blocks`]).
    /// Deliberately **not** applied to `stream()`: streaming thinking-block
    /// preservation is a follow-up, and enabling it there without replay would
    /// 400 the next tool turn.
    thinking: bool,
    client: Client,
}

impl AnthropicClient {
    /// Build a client for `base_url`, reading the API key from
    /// [`API_KEY_ENV`]. A missing key is **not** fatal here — it surfaces as a
    /// clear error at [`Self::validate_model`] / the first call, so construction
    /// stays infallible like [`super::client::OllamaClient::new`].
    pub fn new(base_url: impl Into<String>) -> Self {
        // No total request timeout: a streamed completion can legitimately run
        // for minutes, and reqwest's `.timeout()` would cut the body mid-stream.
        // A connect timeout still fails fast on an unreachable endpoint.
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        let base = base_url.into();
        let base = base.strip_suffix('/').map(str::to_string).unwrap_or(base);
        Self {
            base_url: base,
            api_key: resolve_api_key(),
            max_tokens: DEFAULT_MAX_TOKENS,
            reasoning_effort: None,
            prompt_cache: true,
            thinking: false,
            client,
        }
    }

    /// Client at the default Anthropic endpoint.
    pub fn default_endpoint() -> Self {
        Self::new(ANTHROPIC_DEFAULT_ENDPOINT)
    }

    /// Set the effort emitted as `output_config.effort`. `None` (or an
    /// empty/whitespace value) leaves it off. Mirrors
    /// [`super::client::OllamaClient::with_reasoning_effort`] so the manifest
    /// `reasoningEffort` reaches Anthropic too. Returns `self` for chaining.
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort.filter(|e| !e.trim().is_empty());
        self
    }

    /// Override the required `max_tokens` output ceiling. `None` keeps the
    /// [`DEFAULT_MAX_TOKENS`] default. Fed from the manifest `maxTokens` so long
    /// outputs are no longer capped at the hardcoded default. `Some(0)` is
    /// treated as unset (the Messages API rejects `max_tokens: 0`, so a config
    /// typo keeps the default rather than producing a confusing provider 400).
    /// Returns `self`.
    pub fn with_max_tokens(mut self, max_tokens: Option<u32>) -> Self {
        if let Some(n) = max_tokens.filter(|&n| n > 0) {
            self.max_tokens = n;
        }
        self
    }

    /// Enable/disable prompt caching (system-prefix `cache_control`). Defaults
    /// to on; fed from the manifest `promptCache` (`false` opts out). Returns
    /// `self` for chaining.
    pub fn with_prompt_cache(mut self, enabled: bool) -> Self {
        self.prompt_cache = enabled;
        self
    }

    /// Enable/disable adaptive extended thinking on `complete()`. Off by
    /// default; fed from the manifest `thinking` (`true` opts in). Returns
    /// `self` for chaining.
    pub fn with_thinking(mut self, enabled: bool) -> Self {
        self.thinking = enabled;
        self
    }

    /// Override the API key explicitly instead of resolving it from the
    /// environment / `secrets.toml`. Mirrors [`super::client::OllamaClient::with_api_key`]
    /// for parity; lets a caller (or a hermetic test against a mock endpoint)
    /// inject a key without mutating the process-global `ANTHROPIC_API_KEY` env.
    /// A whitespace-only (or empty) key normalizes to the empty string — matching
    /// `OllamaClient::with_api_key`'s `trim().is_empty()` filter — so `require_key`
    /// reports the missing-key error path rather than sending a blank `x-api-key`.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        let key = api_key.into();
        self.api_key = if key.trim().is_empty() {
            String::new()
        } else {
            key
        };
        self
    }

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    fn models_url(&self) -> String {
        format!("{}/v1/models", self.base_url)
    }

    /// Apply the auth + version headers Claude requires on every request.
    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
    }

    /// Guard that an API key is present, with an actionable message.
    fn require_key(&self) -> Result<(), HarnessError> {
        if self.api_key.is_empty() {
            Err(HarnessError::Provider(format!(
                "no Anthropic API key — set `{API_KEY_ENV}` (e.g. `export {API_KEY_ENV}=sk-ant-...`) \
                 or add an `[anthropic] api_key = \"sk-ant-...\"` entry to ~/.bwoc/secrets.toml (chmod 600)"
            )))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl ProviderClient for AnthropicClient {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
        model: &str,
    ) -> Result<ChatCompletion, HarnessError> {
        self.require_key()?;
        let beta = anthropic_beta_for(&tools);
        let body = build_anthropic_body(
            messages,
            tools,
            model,
            self.max_tokens,
            self.reasoning_effort.as_deref(),
            self.thinking,
            self.prompt_cache,
            false,
        );

        let mut req = self.auth(self.client.post(self.messages_url()));
        if let Some(b) = beta {
            req = req.header("anthropic-beta", b);
        }

        // Bound the whole non-streaming call (connect → send → read body →
        // parse) with a per-call deadline — see COMPLETE_TIMEOUT. A stall
        // surfaces as TransientProvider so the existing retry/backoff path sees
        // it instead of the turn hanging forever.
        let call = async {
            let resp = req.json(&body).send().await.map_err(|e| {
                HarnessError::TransientProvider(format!("HTTP request failed: {e}"))
            })?;

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(super::client::classify_http_error(status.as_u16(), &text));
            }

            let data: Value = resp
                .json()
                .await
                .map_err(|e| HarnessError::Provider(format!("Failed to parse response: {e}")))?;
            Ok(parse_completion(&data))
        };
        match tokio::time::timeout(COMPLETE_TIMEOUT, call).await {
            Ok(res) => res,
            Err(_) => Err(HarnessError::TransientProvider(format!(
                "Anthropic completion timed out after {}s",
                COMPLETE_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Tool>,
        model: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>, HarnessError>
    {
        use futures_util::StreamExt;

        self.require_key()?;
        let beta = anthropic_beta_for(&tools);
        let mut body = build_anthropic_body(
            messages,
            tools,
            model,
            self.max_tokens,
            self.reasoning_effort.as_deref(),
            // Streaming now preserves thinking blocks (with signature) for replay
            // via SseState → StreamChunk.thinking_block, so it can request thinking
            // like the non-streaming path without 400-ing the next tool turn.
            self.thinking,
            self.prompt_cache,
            true,
        );
        body["stream"] = json!(true);

        let mut req = self.auth(self.client.post(self.messages_url()));
        if let Some(b) = beta {
            req = req.header("anthropic-beta", b);
        }
        let resp =
            req.json(&body).send().await.map_err(|e| {
                HarnessError::TransientProvider(format!("HTTP request failed: {e}"))
            })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(super::client::classify_http_error(status.as_u16(), &text));
        }

        // Anthropic's SSE is a stateful event protocol (input tokens arrive at
        // `message_start`, output tokens at `message_delta`), so translation
        // can't be a stateless line filter like the OpenAI path. A spawned task
        // owns the byte stream, parses events, and forwards `StreamChunk`s over
        // a channel; we expose the receiver as a `Stream` via `unfold`.
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<StreamChunk, HarnessError>>(64);
        tokio::spawn(async move {
            let mut bytes = resp.bytes_stream();
            let mut buf = String::new();
            let mut state = SseState::default();
            while let Some(chunk) = bytes.next().await {
                match chunk {
                    Err(e) => {
                        let _ = tx
                            .send(Err(HarnessError::Provider(format!("Stream error: {e}"))))
                            .await;
                        return;
                    }
                    Ok(b) => buf.push_str(&String::from_utf8_lossy(&b)),
                }
                // Process every complete SSE line accumulated so far.
                while let Some(nl) = buf.find('\n') {
                    let line: String = buf.drain(..=nl).collect();
                    let line = line.trim();
                    let Some(data) = line.strip_prefix("data:") else {
                        continue; // `event:` lines / blanks carry no payload we need
                    };
                    let data = data.trim();
                    if data.is_empty() {
                        continue;
                    }
                    let Ok(ev) = serde_json::from_str::<Value>(data) else {
                        continue;
                    };
                    for out in translate_sse_event(&ev, &mut state) {
                        if tx.send(out).await.is_err() {
                            return; // receiver dropped — stop early
                        }
                    }
                }
            }
        });

        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        Ok(Box::pin(stream))
    }

    async fn validate_model(&self, model: &str) -> Result<(), HarnessError> {
        self.require_key()?;
        // Best-effort membership check against GET /v1/models (mirrors the
        // OpenAI path's leniency): any non-success / network / parse failure is
        // treated as "unknown — let the first real call surface a 4xx" rather
        // than a hard block, so transient list outages don't break chat.
        let Ok(resp) = self.auth(self.client.get(self.models_url())).send().await else {
            return Ok(());
        };
        if !resp.status().is_success() {
            return Ok(());
        }
        let Ok(body) = resp.json::<Value>().await else {
            return Ok(());
        };
        let known = body["data"]
            .as_array()
            .map(|arr| arr.iter().any(|m| m["id"].as_str() == Some(model)));
        match known {
            Some(true) | None => Ok(()),
            Some(false) => Err(HarnessError::ModelNotFound(model.to_string())),
        }
    }

    async fn list_models(&self) -> Vec<String> {
        if self.api_key.is_empty() {
            return Vec::new();
        }
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
}

// ---------------------------------------------------------------------------
// Request translation: OpenAI-shaped structs → Anthropic Messages body
// ---------------------------------------------------------------------------

/// Build the `POST /v1/messages` request body from the OpenAI-shaped history.
///
/// Splits out `system`, maps roles to Anthropic content blocks, merges
/// consecutive tool results into one user turn, and rewrites tools to
/// `input_schema`.
// Internal per-request builder: each arg is an independent, orthogonal wire knob
// (max_tokens / effort / thinking / cache / stream). Bundling them into a config
// struct would add indirection without removing any real coupling.
#[allow(clippy::too_many_arguments)]
fn build_anthropic_body(
    messages: Vec<ChatMessage>,
    tools: Vec<Tool>,
    model: &str,
    max_tokens: u32,
    reasoning_effort: Option<&str>,
    thinking: bool,
    cache: bool,
    stream: bool,
) -> Value {
    let mut system = String::new();
    let mut out: Vec<Value> = Vec::new();
    // True when the last pushed message is a user turn made of tool_result
    // blocks, so the next consecutive tool result merges into it.
    let mut last_is_tool_results = false;

    for msg in messages {
        match msg.role {
            Role::System => {
                if let Some(c) = msg.content {
                    if !system.is_empty() {
                        system.push_str("\n\n");
                    }
                    system.push_str(&c);
                }
                last_is_tool_results = false;
            }
            Role::User => {
                out.push(json!({
                    "role": "user",
                    "content": [{"type": "text", "text": msg.content.unwrap_or_default()}],
                }));
                last_is_tool_results = false;
            }
            Role::Assistant => {
                let mut blocks: Vec<Value> = Vec::new();
                // Replay preserved thinking blocks FIRST, unchanged (incl. their
                // `signature`). The Messages API requires the thinking block that
                // preceded a `tool_use` to be present in this assistant turn when
                // the following `tool_result` is sent — dropping it 400s. Safe
                // when caching is on: thinking blocks precede the system prefix's
                // effect and carry their own signature.
                if let Some(tb) = msg.thinking_blocks {
                    blocks.extend(tb);
                }
                if let Some(c) = msg.content {
                    if !c.is_empty() {
                        blocks.push(json!({"type": "text", "text": c}));
                    }
                }
                if let Some(calls) = msg.tool_calls {
                    for tc in calls {
                        let input: Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or_else(|_| json!({}));
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.function.name,
                            "input": input,
                        }));
                    }
                }
                // Claude rejects an assistant turn with empty content.
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": ""}));
                }
                out.push(json!({"role": "assistant", "content": blocks}));
                last_is_tool_results = false;
            }
            Role::Tool => {
                let block = json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id.unwrap_or_default(),
                    "content": msg.content.unwrap_or_default(),
                });
                if last_is_tool_results {
                    if let Some(arr) = out.last_mut().and_then(|m| m["content"].as_array_mut()) {
                        arr.push(block);
                    }
                } else {
                    out.push(json!({"role": "user", "content": [block]}));
                    last_is_tool_results = true;
                }
            }
        }
    }

    let mut body = json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": out,
        "stream": stream,
    });
    if !system.is_empty() {
        // Prompt caching: send `system` as a single text block carrying
        // `cache_control` so Claude caches the stable prefix (tools render
        // before system, so this breakpoint covers the tools block too). SBPL
        // of the API: last-block-of-prefix cache. Below the provider's minimum
        // cacheable size the marker is a silent no-op. When caching is off we
        // keep the plain-string form (byte-identical to the prior behaviour).
        if cache {
            body["system"] = json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" },
            }]);
        } else {
            body["system"] = json!(system);
        }
    }
    if !tools.is_empty() {
        let mapped: Vec<Value> = tools
            .into_iter()
            .map(|t| {
                // The `computer` tool is a *provider-defined* native tool keyed by
                // `type` (not a custom function), and the request must also carry
                // the computer-use beta header (see `anthropic_beta_for`). The
                // display geometry matches the headless-browser executor viewport
                // so the model reasons in the right coordinate space.
                if t.function.name == "computer" {
                    let (w, h) = crate::tools::browser::DEFAULT_VIEWPORT;
                    crate::tools::computer::anthropic_tool_spec(w, h, None)
                } else {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                }
            })
            .collect();
        body["tools"] = json!(mapped);
    }
    // Effort control — the Anthropic analogue of the OpenAI-compat
    // `reasoning_effort` field, nested under `output_config` (GA on Opus 4.6+,
    // Sonnet 4.6+, Fable 5). Only emitted when the manifest configured it;
    // models that don't support the value reject the request, exactly as the
    // OpenAI-compat path already behaves.
    if let Some(effort) = reasoning_effort {
        body["output_config"] = json!({ "effort": effort });
    }
    // Adaptive extended thinking (opt-in). `{type:"adaptive"}` lets Claude decide
    // when/how much to think (Opus 4.6+, Sonnet 4.6+, Fable 5 — `budget_tokens`
    // is removed on 4.7+); depth is governed by `output_config.effort` above.
    // The returned thinking blocks are preserved by `parse_completion` and
    // replayed on the next assistant turn (see the Assistant arm).
    if thinking {
        body["thinking"] = json!({ "type": "adaptive" });
    }
    body
}

/// The `anthropic-beta` header value required when the request carries the
/// `computer` native tool, or `None` when no computer tool is present. Keeping
/// this a pure function of the tool list lets `complete`/`stream` attach the
/// header without re-inspecting the (already-moved) tools.
fn anthropic_beta_for(tools: &[Tool]) -> Option<&'static str> {
    tools
        .iter()
        .any(|t| t.function.name == "computer")
        .then_some(crate::tools::computer::ANTHROPIC_COMPUTER_BETA)
}

// ---------------------------------------------------------------------------
// Response translation: Anthropic → OpenAI-shaped structs
// ---------------------------------------------------------------------------

/// Map an Anthropic `stop_reason` to the OpenAI-shaped [`FinishReason`].
fn map_stop_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("tool_use") => FinishReason::ToolCalls,
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        _ => FinishReason::Other,
    }
}

/// Parse a full (non-streaming) Messages response into a [`ChatCompletion`].
fn parse_completion(data: &Value) -> ChatCompletion {
    let id = data["id"].as_str().unwrap_or("anthropic").to_string();
    let mut text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    // Extended-thinking blocks are preserved VERBATIM (incl. `signature`) so the
    // same-model next turn can replay them — required before a `tool_use`'s
    // `tool_result`, or the API 400s. See `ChatMessage::thinking_blocks`.
    let mut thinking_blocks: Vec<Value> = Vec::new();

    if let Some(blocks) = data["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => text.push_str(b["text"].as_str().unwrap_or_default()),
                Some("thinking") | Some("redacted_thinking") => thinking_blocks.push(b.clone()),
                Some("tool_use") => tool_calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or_default().to_string(),
                    kind: "function".to_string(),
                    function: FunctionCall {
                        name: b["name"].as_str().unwrap_or_default().to_string(),
                        arguments: b["input"].to_string(),
                    },
                }),
                _ => {}
            }
        }
    }

    let message = ChatMessage::assistant(
        (!text.is_empty()).then_some(text),
        (!tool_calls.is_empty()).then_some(tool_calls),
    )
    .with_thinking_blocks(thinking_blocks);
    let usage = data.get("usage").map(parse_usage);

    ChatCompletion {
        id,
        choices: vec![Choice {
            index: 0,
            message,
            finish_reason: Some(map_stop_reason(data["stop_reason"].as_str())),
        }],
        usage,
    }
}

/// Read Anthropic's `{input_tokens, output_tokens}` into the OpenAI-shaped
/// [`Usage`] (`prompt`/`completion`/`total`).
fn parse_usage(u: &Value) -> Usage {
    let prompt = u["input_tokens"].as_u64().unwrap_or(0) as u32;
    let completion = u["output_tokens"].as_u64().unwrap_or(0) as u32;
    // Anthropic reports prompt-cache accounting flat on the message usage
    // (not nested like OpenAI-compat). Capture it so caching telemetry isn't
    // dropped; `None` when the field is absent (uncached request / older API).
    let cache_read = u["cache_read_input_tokens"].as_u64().map(|v| v as u32);
    let cache_creation = u["cache_creation_input_tokens"].as_u64().map(|v| v as u32);
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        ..Usage::default()
    }
}

/// Translate one Anthropic SSE event into zero or more [`StreamChunk`]s the
/// accumulator understands. `input_tokens` is carried across events so the
/// final `message_delta` can emit a complete [`Usage`] (Anthropic splits input
/// tokens into `message_start` and output tokens into `message_delta`).
/// Mutable state threaded through the Anthropic SSE translation: input-side
/// token/cache accounting (from `message_start`) and the in-flight thinking
/// block accumulated across deltas (finalized at `content_block_stop`).
#[derive(Default)]
struct SseState {
    input_tokens: u32,
    cache_read: Option<u32>,
    cache_creation: Option<u32>,
    thinking: Option<ThinkingAccum>,
}

/// A `thinking` / `redacted_thinking` block accumulated across streamed deltas.
struct ThinkingAccum {
    index: u32,
    redacted: bool,
    thinking: String,
    signature: String,
    data: String,
}

impl ThinkingAccum {
    /// Finalize into the raw block Value Anthropic sends — the exact shape
    /// [`parse_completion`] collects and `build_anthropic_body` replays.
    fn into_block(self) -> Value {
        if self.redacted {
            json!({ "type": "redacted_thinking", "data": self.data })
        } else {
            json!({ "type": "thinking", "thinking": self.thinking, "signature": self.signature })
        }
    }
}

fn translate_sse_event(ev: &Value, state: &mut SseState) -> Vec<Result<StreamChunk, HarnessError>> {
    let mk = |choices: Vec<StreamDelta>, usage: Option<Usage>| StreamChunk {
        id: "anthropic".to_string(),
        choices,
        usage,
        thinking_block: None,
    };
    let text_chunk = |content: Option<String>, tool_calls: Option<Vec<ToolCallDelta>>| {
        mk(
            vec![StreamDelta {
                index: 0,
                delta: Delta {
                    role: None,
                    content,
                    tool_calls,
                },
                finish_reason: None,
            }],
            None,
        )
    };

    match ev["type"].as_str() {
        Some("message_start") => {
            let u = &ev["message"]["usage"];
            state.input_tokens = u["input_tokens"].as_u64().unwrap_or(0) as u32;
            state.cache_read = u["cache_read_input_tokens"].as_u64().map(|v| v as u32);
            state.cache_creation = u["cache_creation_input_tokens"].as_u64().map(|v| v as u32);
            Vec::new()
        }
        Some("content_block_start") => {
            let index = ev["index"].as_u64().unwrap_or(0) as u32;
            match ev["content_block"]["type"].as_str() {
                // A tool_use block opening carries its id + name; emit them as
                // the start of a tool-call delta keyed by the block index.
                Some("tool_use") => vec![Ok(text_chunk(
                    None,
                    Some(vec![ToolCallDelta {
                        index,
                        id: ev["content_block"]["id"].as_str().map(str::to_string),
                        r#type: Some("function".to_string()),
                        function: Some(FunctionDelta {
                            name: ev["content_block"]["name"].as_str().map(str::to_string),
                            arguments: None,
                        }),
                    }]),
                ))],
                Some("thinking") => {
                    state.thinking = Some(ThinkingAccum {
                        index,
                        redacted: false,
                        thinking: String::new(),
                        signature: String::new(),
                        data: String::new(),
                    });
                    Vec::new()
                }
                Some("redacted_thinking") => {
                    state.thinking = Some(ThinkingAccum {
                        index,
                        redacted: true,
                        thinking: String::new(),
                        signature: String::new(),
                        data: ev["content_block"]["data"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                    });
                    Vec::new()
                }
                _ => Vec::new(),
            }
        }
        Some("content_block_delta") => match ev["delta"]["type"].as_str() {
            Some("text_delta") => {
                let text = ev["delta"]["text"].as_str().unwrap_or_default().to_string();
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![Ok(text_chunk(Some(text), None))]
                }
            }
            Some("thinking_delta") => {
                if let Some(t) = state.thinking.as_mut() {
                    t.thinking
                        .push_str(ev["delta"]["thinking"].as_str().unwrap_or_default());
                }
                Vec::new()
            }
            Some("signature_delta") => {
                if let Some(t) = state.thinking.as_mut() {
                    t.signature
                        .push_str(ev["delta"]["signature"].as_str().unwrap_or_default());
                }
                Vec::new()
            }
            Some("input_json_delta") => {
                let index = ev["index"].as_u64().unwrap_or(0) as u32;
                let partial = ev["delta"]["partial_json"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                vec![Ok(text_chunk(
                    None,
                    Some(vec![ToolCallDelta {
                        index,
                        id: None,
                        r#type: None,
                        function: Some(FunctionDelta {
                            name: None,
                            arguments: Some(partial),
                        }),
                    }]),
                ))]
            }
            _ => Vec::new(),
        },
        Some("content_block_stop") => {
            // Finalize an in-flight thinking block into a carrier chunk so the
            // accumulator can preserve it (with signature) for replay. A stop
            // for any other block type (tool_use / text) leaves state untouched.
            let idx = ev["index"].as_u64().unwrap_or(0) as u32;
            match state.thinking.take() {
                Some(t) if t.index == idx => {
                    let mut chunk = mk(Vec::new(), None);
                    chunk.thinking_block = Some(t.into_block());
                    vec![Ok(chunk)]
                }
                other => {
                    state.thinking = other;
                    Vec::new()
                }
            }
        }
        Some("message_delta") => {
            let output = ev["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
            vec![Ok(mk(
                Vec::new(),
                Some(Usage {
                    prompt_tokens: state.input_tokens,
                    completion_tokens: output,
                    total_tokens: state.input_tokens + output,
                    cache_read_tokens: state.cache_read,
                    cache_creation_tokens: state.cache_creation,
                    ..Usage::default()
                }),
            ))]
        }
        Some("error") => {
            let msg = ev["error"]["message"]
                .as_str()
                .unwrap_or("anthropic stream error");
            vec![Err(HarnessError::Provider(msg.to_string()))]
        }
        // ping / message_stop carry nothing we re-emit.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::FunctionCall;

    #[test]
    fn complete_timeout_matches_shared_request_timeout() {
        // complete() carries its own deadline because the shared client has no
        // reqwest `.timeout()` (that would cut the stream() body mid-flight). It
        // reuses REQUEST_TIMEOUT as the single source of truth (compiler-enforced,
        // not a doc claim); assert the concrete value too so an unintended change
        // to the shared const is visible here.
        assert_eq!(COMPLETE_TIMEOUT, super::super::client::REQUEST_TIMEOUT);
        assert_eq!(COMPLETE_TIMEOUT, Duration::from_secs(120));
    }

    /// A whitespace-only key normalizes to empty (parity with
    /// `OllamaClient::with_api_key`), so `require_key` fails fast — the request is
    /// never sent, and the error is the missing-key `Provider` error, not a
    /// `ModelNotFound` from a blank-`x-api-key` round-trip.
    #[tokio::test]
    async fn with_api_key_treats_whitespace_as_missing() {
        // Unroutable base URL: if require_key did NOT fire first, the call would
        // instead surface a transient connection error — so a Provider error here
        // proves the guard ran before any HTTP.
        let c = AnthropicClient::new("http://127.0.0.1:1").with_api_key("   ");
        let err = c
            .validate_model("m")
            .await
            .expect_err("blank key must error before any request");
        assert!(
            matches!(err, HarnessError::Provider(ref m) if m.contains("API key")),
            "whitespace key must hit the missing-key guard, got {err:?}"
        );
    }

    /// Write `body` to `path` with `0600` perms so it passes the on-disk guard.
    fn write_secret(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn api_key_from_secrets_reads_anthropic_table() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("secrets.toml");
        write_secret(
            &path,
            "[jira]\ntoken = \"x\"\n\n[anthropic]\napi_key = \"sk-ant-test123\"\n\n[openrouter]\napi_key = \"sk-or-test456\"\n",
        );
        assert_eq!(
            api_key_from_secrets(&path, "anthropic"),
            Some("sk-ant-test123".to_string())
        );
        // The shared helper keys off the section name — a second provider's key
        // reads from the same file without colliding.
        assert_eq!(
            api_key_from_secrets(&path, "openrouter"),
            Some("sk-or-test456".to_string())
        );
    }

    #[test]
    fn api_key_from_secrets_none_when_missing_or_absent() {
        // Missing file.
        assert_eq!(
            api_key_from_secrets(std::path::Path::new("/no/such/secrets.toml"), "anthropic"),
            None
        );
        // File present but no [anthropic] api_key.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("secrets.toml");
        write_secret(&path, "[jira]\ntoken = \"x\"\n");
        assert_eq!(api_key_from_secrets(&path, "anthropic"), None);
        // Empty key is treated as absent.
        let path2 = dir.path().join("s2.toml");
        write_secret(&path2, "[anthropic]\napi_key = \"\"\n");
        assert_eq!(api_key_from_secrets(&path2, "anthropic"), None);
    }

    #[cfg(unix)]
    #[test]
    fn api_key_from_secrets_refuses_group_world_accessible_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("secrets.toml");
        std::fs::write(&path, "[anthropic]\napi_key = \"sk-ant-leaky\"\n").unwrap();
        // 0644 → group/world-readable → must be refused even though the key is valid.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(api_key_from_secrets(&path, "anthropic"), None);
    }

    #[test]
    fn system_is_lifted_and_roles_mapped() {
        let msgs = vec![ChatMessage::system("be terse"), ChatMessage::user("hi")];
        let body =
            build_anthropic_body(msgs, Vec::new(), "claude-x", 100, None, false, false, false);
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
        // System must not leak into the messages array.
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["max_tokens"], 100);
    }

    #[test]
    fn tool_use_and_consecutive_results_merge_into_one_user_turn() {
        let assistant = ChatMessage::assistant(
            Some("calling".into()),
            Some(vec![
                ToolCall {
                    id: "t1".into(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: "read".into(),
                        arguments: "{\"p\":1}".into(),
                    },
                },
                ToolCall {
                    id: "t2".into(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: "ls".into(),
                        arguments: "{}".into(),
                    },
                },
            ]),
        );
        let msgs = vec![
            ChatMessage::user("go"),
            assistant,
            ChatMessage::tool_result("t1", "tool_a", "A"),
            ChatMessage::tool_result("t2", "tool_b", "B"),
        ];
        let body =
            build_anthropic_body(msgs, Vec::new(), "claude-x", 100, None, false, false, false);
        let arr = body["messages"].as_array().unwrap();
        // user, assistant(tool_use x2), user(tool_result x2 merged) = 3 turns.
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[1]["content"][1]["type"], "tool_use");
        assert_eq!(arr[1]["content"][1]["input"]["p"], 1);
        assert_eq!(arr[2]["role"], "user");
        let results = arr[2]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["type"], "tool_result");
        assert_eq!(results[0]["tool_use_id"], "t1");
        assert_eq!(results[1]["tool_use_id"], "t2");
    }

    #[test]
    fn tools_use_input_schema() {
        let tools = vec![Tool::function("grep", "search", json!({"type": "object"}))];
        let body = build_anthropic_body(
            vec![ChatMessage::user("x")],
            tools,
            "claude-x",
            50,
            None,
            false,
            false,
            false,
        );
        assert_eq!(body["tools"][0]["name"], "grep");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert!(body["tools"][0].get("parameters").is_none());
    }

    #[test]
    fn output_config_effort_and_max_tokens() {
        // Effort is emitted under output_config only when configured; max_tokens
        // is the required top-level field carrying the configured ceiling.
        let plain = build_anthropic_body(
            vec![ChatMessage::user("x")],
            Vec::new(),
            "claude-x",
            50,
            None,
            false,
            false,
            false,
        );
        assert!(plain.get("output_config").is_none());
        assert_eq!(plain["max_tokens"], 50);

        let with_effort = build_anthropic_body(
            vec![ChatMessage::user("x")],
            Vec::new(),
            "claude-x",
            64000,
            Some("high"),
            false,
            false,
            false,
        );
        assert_eq!(with_effort["output_config"]["effort"], "high");
        assert_eq!(with_effort["max_tokens"], 64000);
    }

    #[test]
    fn client_builders_thread_effort_and_max_tokens() {
        // The builders reach the request body: effort filters blanks; max_tokens
        // overrides the hardcoded default.
        let c = AnthropicClient::new("http://x")
            .with_reasoning_effort(Some("  ".to_string()))
            .with_max_tokens(Some(120_000));
        assert!(c.reasoning_effort.is_none(), "blank effort is dropped");
        assert_eq!(c.max_tokens, 120_000);

        let d = AnthropicClient::new("http://x").with_reasoning_effort(Some("max".to_string()));
        assert_eq!(d.reasoning_effort.as_deref(), Some("max"));
        assert_eq!(
            d.max_tokens, DEFAULT_MAX_TOKENS,
            "unset max_tokens keeps default"
        );

        // `Some(0)` is a config typo — keep the default rather than sending
        // `max_tokens: 0` (the Messages API rejects it).
        let z = AnthropicClient::new("http://x").with_max_tokens(Some(0));
        assert_eq!(z.max_tokens, DEFAULT_MAX_TOKENS);

        // Prompt caching defaults on; opt-out flips it.
        assert!(AnthropicClient::new("http://x").prompt_cache);
        assert!(
            !AnthropicClient::new("http://x")
                .with_prompt_cache(false)
                .prompt_cache
        );
    }

    #[test]
    fn thinking_config_and_builder() {
        assert!(!AnthropicClient::new("http://x").thinking, "off by default");
        assert!(
            AnthropicClient::new("http://x")
                .with_thinking(true)
                .thinking
        );

        // Top-level thinking config emitted only when enabled.
        let on = build_anthropic_body(
            vec![ChatMessage::user("x")],
            Vec::new(),
            "claude-x",
            100,
            None,
            true,
            false,
            false,
        );
        assert_eq!(on["thinking"]["type"], "adaptive");
        let off = build_anthropic_body(
            vec![ChatMessage::user("x")],
            Vec::new(),
            "claude-x",
            100,
            None,
            false,
            false,
            false,
        );
        assert!(off.get("thinking").is_none());
    }

    #[test]
    fn thinking_blocks_round_trip_and_replay() {
        // parse_completion preserves thinking/redacted_thinking blocks verbatim.
        let completion = parse_completion(&json!({
            "id": "m1",
            "content": [
                {"type": "thinking", "thinking": "let me reason", "signature": "sig-abc"},
                {"type": "text", "text": "answer"},
                {"type": "tool_use", "id": "tu1", "name": "grep", "input": {"q": "x"}}
            ],
            "stop_reason": "tool_use"
        }));
        let msg = &completion.choices[0].message;
        let tb = msg.thinking_blocks.as_ref().expect("thinking preserved");
        assert_eq!(tb.len(), 1);
        assert_eq!(tb[0]["signature"], "sig-abc");

        // Replaying that assistant message emits the thinking block FIRST,
        // unchanged (before text / tool_use) — the shape the API requires.
        let body = build_anthropic_body(
            vec![msg.clone()],
            Vec::new(),
            "claude-x",
            100,
            None,
            true,
            false,
            false,
        );
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "thinking");
        assert_eq!(content[0]["signature"], "sig-abc");
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[2]["type"], "tool_use");
    }

    #[test]
    fn system_cache_control_toggles() {
        let msgs = vec![
            ChatMessage::system("stable prefix"),
            ChatMessage::user("hi"),
        ];

        // Caching on → system is a text-block array carrying cache_control.
        let cached = build_anthropic_body(
            msgs.clone(),
            Vec::new(),
            "claude-x",
            100,
            None,
            false,
            true,
            false,
        );
        assert_eq!(cached["system"][0]["type"], "text");
        assert_eq!(cached["system"][0]["text"], "stable prefix");
        assert_eq!(cached["system"][0]["cache_control"]["type"], "ephemeral");

        // Caching off → plain string (byte-identical to the pre-caching form).
        let plain =
            build_anthropic_body(msgs, Vec::new(), "claude-x", 100, None, false, false, false);
        assert_eq!(plain["system"], "stable prefix");
    }

    #[test]
    fn parse_usage_captures_cache_tokens() {
        let u = parse_usage(&json!({
            "input_tokens": 1000,
            "output_tokens": 200,
            "cache_read_input_tokens": 640,
            "cache_creation_input_tokens": 128
        }));
        assert_eq!(u.prompt_tokens, 1000);
        assert_eq!(u.completion_tokens, 200);
        assert_eq!(u.cached_tokens(), Some(640));
        assert_eq!(u.cache_creation_tokens, Some(128));
        // Absent cache fields → None, no panic.
        let bare = parse_usage(&json!({"input_tokens": 5, "output_tokens": 3}));
        assert_eq!(bare.cached_tokens(), None);
        assert_eq!(bare.cache_creation_tokens, None);
    }

    #[test]
    fn computer_tool_serializes_as_native_not_custom_function() {
        // A `computer` tool must become the provider-native spec (keyed by
        // `type`, no `input_schema`), while sibling tools keep the custom shape.
        let tools = vec![
            Tool::function("grep", "search", json!({"type": "object"})),
            Tool::function("computer", "drive a gui", json!({"type": "object"})),
        ];
        let body = build_anthropic_body(
            vec![ChatMessage::user("x")],
            tools,
            "claude-x",
            50,
            None,
            false,
            false,
            false,
        );
        let arr = body["tools"].as_array().unwrap();
        let comp = arr.iter().find(|t| t["name"] == "computer").unwrap();
        assert_eq!(comp["type"], "computer_20250124");
        assert!(comp.get("input_schema").is_none());
        let (w, h) = crate::tools::browser::DEFAULT_VIEWPORT;
        assert_eq!(comp["display_width_px"], w);
        assert_eq!(comp["display_height_px"], h);
        // The sibling custom function is untouched.
        let grep = arr.iter().find(|t| t["name"] == "grep").unwrap();
        assert_eq!(grep["input_schema"]["type"], "object");
    }

    #[test]
    fn beta_header_only_when_computer_present() {
        let no_comp = vec![Tool::function("grep", "s", json!({}))];
        assert!(anthropic_beta_for(&no_comp).is_none());
        let with_comp = vec![Tool::function("computer", "c", json!({}))];
        assert_eq!(
            anthropic_beta_for(&with_comp),
            Some(crate::tools::computer::ANTHROPIC_COMPUTER_BETA)
        );
    }

    #[test]
    fn completion_parses_text_tools_and_usage() {
        let data = json!({
            "id": "msg_1",
            "content": [
                {"type": "text", "text": "hello"},
                {"type": "tool_use", "id": "tu_1", "name": "read", "input": {"p": 2}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let c = parse_completion(&data);
        assert_eq!(c.id, "msg_1");
        let m = &c.choices[0].message;
        assert_eq!(m.content.as_deref(), Some("hello"));
        let calls = m.tool_calls.as_ref().unwrap();
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].function.name, "read");
        assert_eq!(calls[0].function.arguments, "{\"p\":2}");
        assert_eq!(c.choices[0].finish_reason, Some(FinishReason::ToolCalls));
        let u = c.usage.unwrap();
        assert_eq!(u.prompt_tokens, 10);
        assert_eq!(u.completion_tokens, 5);
        assert_eq!(u.total_tokens, 15);
    }

    #[test]
    fn sse_text_delta_becomes_token_content() {
        let mut st = SseState::default();
        let ev = json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "text_delta", "text": "Hi"}});
        let out = translate_sse_event(&ev, &mut st);
        assert_eq!(out.len(), 1);
        let chunk = out.into_iter().next().unwrap().unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hi"));
    }

    #[test]
    fn sse_usage_combines_input_start_with_output_delta() {
        let mut st = SseState::default();
        let start = json!({"type": "message_start", "message": {"usage": {"input_tokens": 42}}});
        assert!(translate_sse_event(&start, &mut st).is_empty());
        assert_eq!(st.input_tokens, 42);
        let delta = json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 7}});
        let out = translate_sse_event(&delta, &mut st);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let u = chunk.usage.unwrap();
        assert_eq!(u.prompt_tokens, 42);
        assert_eq!(u.completion_tokens, 7);
        assert_eq!(u.total_tokens, 49);
    }

    #[test]
    fn sse_tool_use_start_and_json_delta_accumulate() {
        let mut st = SseState::default();
        let start = json!({"type": "content_block_start", "index": 1,
            "content_block": {"type": "tool_use", "id": "tu_9", "name": "edit"}});
        let out = translate_sse_event(&start, &mut st);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 1);
        assert_eq!(tc.id.as_deref(), Some("tu_9"));
        assert_eq!(tc.function.as_ref().unwrap().name.as_deref(), Some("edit"));

        let jd = json!({"type": "content_block_delta", "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}});
        let out = translate_sse_event(&jd, &mut st);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(
            tc.function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"path\":")
        );
    }

    #[test]
    fn sse_message_start_captures_cache_tokens() {
        let mut st = SseState::default();
        let start = json!({"type": "message_start", "message": {"usage": {
            "input_tokens": 100,
            "cache_read_input_tokens": 640,
            "cache_creation_input_tokens": 128
        }}});
        assert!(translate_sse_event(&start, &mut st).is_empty());
        let delta = json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 9}});
        let chunk = translate_sse_event(&delta, &mut st)
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        let u = chunk.usage.unwrap();
        assert_eq!(u.prompt_tokens, 100);
        assert_eq!(u.cached_tokens(), Some(640));
        assert_eq!(u.cache_creation_tokens, Some(128));
    }

    #[test]
    fn sse_thinking_block_round_trips_for_replay() {
        // A thinking block streamed as start → thinking_delta(s) → signature_delta
        // → stop must be finalized into a carrier chunk whose thinking_block is the
        // exact shape parse_completion produces (so the accumulator can replay it).
        let mut st = SseState::default();

        let start = json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "thinking", "thinking": ""}});
        assert!(translate_sse_event(&start, &mut st).is_empty());
        assert!(st.thinking.is_some());

        for piece in ["Let me ", "reason."] {
            let d = json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "thinking_delta", "thinking": piece}});
            assert!(translate_sse_event(&d, &mut st).is_empty());
        }
        let sig = json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "signature_delta", "signature": "SIG=="}});
        assert!(translate_sse_event(&sig, &mut st).is_empty());

        let stop = json!({"type": "content_block_stop", "index": 0});
        let out = translate_sse_event(&stop, &mut st);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let block = chunk
            .thinking_block
            .expect("carrier chunk carries the block");
        assert_eq!(block["type"], "thinking");
        assert_eq!(block["thinking"], "Let me reason.");
        assert_eq!(block["signature"], "SIG==");
        // State is cleared so a following block doesn't leak into it.
        assert!(st.thinking.is_none());
        // No token deltas emitted for a pure thinking block.
        assert!(chunk.choices.is_empty());
    }

    #[test]
    fn sse_redacted_thinking_block_preserves_data() {
        let mut st = SseState::default();
        let start = json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "redacted_thinking", "data": "ENCRYPTED"}});
        assert!(translate_sse_event(&start, &mut st).is_empty());
        let stop = json!({"type": "content_block_stop", "index": 0});
        let chunk = translate_sse_event(&stop, &mut st)
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        let block = chunk.thinking_block.unwrap();
        assert_eq!(block["type"], "redacted_thinking");
        assert_eq!(block["data"], "ENCRYPTED");
    }
}
