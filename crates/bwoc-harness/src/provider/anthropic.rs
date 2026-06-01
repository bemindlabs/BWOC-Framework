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
fn home_dir() -> Option<std::path::PathBuf> {
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
    if let Ok(k) = std::env::var(API_KEY_ENV) {
        if !k.trim().is_empty() {
            return k;
        }
    }
    home_dir()
        .map(|home| home.join(".bwoc").join("secrets.toml"))
        .and_then(|p| api_key_from_secrets(&p))
        .unwrap_or_default()
}

/// Read `[anthropic] api_key` from a `secrets.toml` at `path`. Returns `None` on
/// any error (missing file, parse failure, absent key). On Unix a
/// group/world-accessible file is **refused** with a warning — the same
/// "Adinnādāna at the file boundary" guard the Jira/figma secret resolvers
/// apply (`chmod 600` required). Pure + tested.
fn api_key_from_secrets(path: &std::path::Path) -> Option<String> {
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
        .get("anthropic")?
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

/// Real HTTP client speaking Anthropic's Messages API.
#[derive(Debug, Clone)]
pub struct AnthropicClient {
    base_url: String,
    api_key: String,
    max_tokens: u32,
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
            client,
        }
    }

    /// Client at the default Anthropic endpoint.
    pub fn default_endpoint() -> Self {
        Self::new(ANTHROPIC_DEFAULT_ENDPOINT)
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
        let body = build_anthropic_body(messages, tools, model, self.max_tokens, false);

        let resp = self
            .auth(self.client.post(self.messages_url()))
            .json(&body)
            .send()
            .await
            .map_err(|e| HarnessError::TransientProvider(format!("HTTP request failed: {e}")))?;

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
        let mut body = build_anthropic_body(messages, tools, model, self.max_tokens, true);
        body["stream"] = json!(true);

        let resp = self
            .auth(self.client.post(self.messages_url()))
            .json(&body)
            .send()
            .await
            .map_err(|e| HarnessError::TransientProvider(format!("HTTP request failed: {e}")))?;

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
            let mut input_tokens: u32 = 0;
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
                    for out in translate_sse_event(&ev, &mut input_tokens) {
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
fn build_anthropic_body(
    messages: Vec<ChatMessage>,
    tools: Vec<Tool>,
    model: &str,
    max_tokens: u32,
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
        body["system"] = json!(system);
    }
    if !tools.is_empty() {
        let mapped: Vec<Value> = tools
            .into_iter()
            .map(|t| {
                json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "input_schema": t.function.parameters,
                })
            })
            .collect();
        body["tools"] = json!(mapped);
    }
    body
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

    if let Some(blocks) = data["content"].as_array() {
        for b in blocks {
            match b["type"].as_str() {
                Some("text") => text.push_str(b["text"].as_str().unwrap_or_default()),
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
    );
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
    Usage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
    }
}

/// Translate one Anthropic SSE event into zero or more [`StreamChunk`]s the
/// accumulator understands. `input_tokens` is carried across events so the
/// final `message_delta` can emit a complete [`Usage`] (Anthropic splits input
/// tokens into `message_start` and output tokens into `message_delta`).
fn translate_sse_event(
    ev: &Value,
    input_tokens: &mut u32,
) -> Vec<Result<StreamChunk, HarnessError>> {
    let mk = |choices: Vec<StreamDelta>, usage: Option<Usage>| StreamChunk {
        id: "anthropic".to_string(),
        choices,
        usage,
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
            *input_tokens = ev["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
            Vec::new()
        }
        Some("content_block_start") => {
            // A tool_use block opening carries its id + name; emit them as the
            // start of a tool-call delta keyed by the block index.
            if ev["content_block"]["type"].as_str() == Some("tool_use") {
                let index = ev["index"].as_u64().unwrap_or(0) as u32;
                vec![Ok(text_chunk(
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
                ))]
            } else {
                Vec::new()
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
        Some("message_delta") => {
            let output = ev["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;
            vec![Ok(mk(
                Vec::new(),
                Some(Usage {
                    prompt_tokens: *input_tokens,
                    completion_tokens: output,
                    total_tokens: *input_tokens + output,
                }),
            ))]
        }
        Some("error") => {
            let msg = ev["error"]["message"]
                .as_str()
                .unwrap_or("anthropic stream error");
            vec![Err(HarnessError::Provider(msg.to_string()))]
        }
        // ping / content_block_stop / message_stop carry nothing we re-emit.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::FunctionCall;

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
            "[jira]\ntoken = \"x\"\n\n[anthropic]\napi_key = \"sk-ant-test123\"\n",
        );
        assert_eq!(
            api_key_from_secrets(&path),
            Some("sk-ant-test123".to_string())
        );
    }

    #[test]
    fn api_key_from_secrets_none_when_missing_or_absent() {
        // Missing file.
        assert_eq!(
            api_key_from_secrets(std::path::Path::new("/no/such/secrets.toml")),
            None
        );
        // File present but no [anthropic] api_key.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("secrets.toml");
        write_secret(&path, "[jira]\ntoken = \"x\"\n");
        assert_eq!(api_key_from_secrets(&path), None);
        // Empty key is treated as absent.
        let path2 = dir.path().join("s2.toml");
        write_secret(&path2, "[anthropic]\napi_key = \"\"\n");
        assert_eq!(api_key_from_secrets(&path2), None);
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
        assert_eq!(api_key_from_secrets(&path), None);
    }

    #[test]
    fn system_is_lifted_and_roles_mapped() {
        let msgs = vec![ChatMessage::system("be terse"), ChatMessage::user("hi")];
        let body = build_anthropic_body(msgs, Vec::new(), "claude-x", 100, false);
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
            ChatMessage::tool_result("t1", "A"),
            ChatMessage::tool_result("t2", "B"),
        ];
        let body = build_anthropic_body(msgs, Vec::new(), "claude-x", 100, false);
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
        let body = build_anthropic_body(vec![ChatMessage::user("x")], tools, "claude-x", 50, false);
        assert_eq!(body["tools"][0]["name"], "grep");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert!(body["tools"][0].get("parameters").is_none());
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
        let mut input = 0u32;
        let ev = json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "text_delta", "text": "Hi"}});
        let out = translate_sse_event(&ev, &mut input);
        assert_eq!(out.len(), 1);
        let chunk = out.into_iter().next().unwrap().unwrap();
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hi"));
    }

    #[test]
    fn sse_usage_combines_input_start_with_output_delta() {
        let mut input = 0u32;
        let start = json!({"type": "message_start", "message": {"usage": {"input_tokens": 42}}});
        assert!(translate_sse_event(&start, &mut input).is_empty());
        assert_eq!(input, 42);
        let delta = json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 7}});
        let out = translate_sse_event(&delta, &mut input);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let u = chunk.usage.unwrap();
        assert_eq!(u.prompt_tokens, 42);
        assert_eq!(u.completion_tokens, 7);
        assert_eq!(u.total_tokens, 49);
    }

    #[test]
    fn sse_tool_use_start_and_json_delta_accumulate() {
        let mut input = 0u32;
        let start = json!({"type": "content_block_start", "index": 1,
            "content_block": {"type": "tool_use", "id": "tu_9", "name": "edit"}});
        let out = translate_sse_event(&start, &mut input);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.index, 1);
        assert_eq!(tc.id.as_deref(), Some("tu_9"));
        assert_eq!(tc.function.as_ref().unwrap().name.as_deref(), Some("edit"));

        let jd = json!({"type": "content_block_delta", "index": 1,
            "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}});
        let out = translate_sse_event(&jd, &mut input);
        let chunk = out.into_iter().next().unwrap().unwrap();
        let tc = &chunk.choices[0].delta.tool_calls.as_ref().unwrap()[0];
        assert_eq!(
            tc.function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"path\":")
        );
    }
}
