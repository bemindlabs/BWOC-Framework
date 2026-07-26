//! MCP client (HV2-5) — consume external MCP tool servers.
//!
//! The harness speaks the Model Context Protocol as a **client only** (the
//! server role is deferred — a network surface with no current need).  Two
//! transports implement the same [`RpcTransport`] seam: a **stdio** transport
//! that launches an MCP server as a subprocess and speaks JSON-RPC 2.0 over its
//! stdio (line-delimited), and a **Streamable HTTP** transport that POSTs
//! JSON-RPC to a remote endpoint (response as JSON or SSE). Either way the
//! harness discovers the server's tools and registers each as an ordinary
//! [`ToolImpl`](crate::tools::ToolImpl), so MCP calls flow through `dispatch` →
//! `execute_tool_calls` → guardrails → permission → sandbox like every other
//! tool — no bypass.
//!
//! Hand-rolled on `serde_json` + `tokio::process` + `reqwest` (all already deps)
//! rather than an SDK, matching the hand-rolled OpenAI provider and keeping the
//! dep-quarantine clean. The HTTP transport enforces https (except loopback) and
//! disables redirects as SSRF hardening.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use crate::error::HarnessError;
use crate::tools::{ToolContext, ToolImpl, ToolRegistry};

/// JSON-RPC transport to an MCP server.  Abstracted so the client logic can be
/// tested without spawning a real server.
#[async_trait]
pub trait RpcTransport: Send + Sync {
    /// Send a request and await its result (the JSON-RPC `result` field).
    async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError>;
    /// Send a notification (no id, no response awaited).
    async fn notify(&self, method: &str, params: Value) -> Result<(), HarnessError>;
}

// ---------------------------------------------------------------------------
// stdio transport
// ---------------------------------------------------------------------------

/// Per-request response timeout — bounds a hung or misbehaving server (one
/// that never answers, or echoes a malformed id) so a `tools/call` can't block
/// the agent loop forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Line-delimited JSON-RPC 2.0 over a subprocess's stdio.
pub struct StdioTransport {
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    next_id: AtomicU64,
    // Serializes a whole request/response cycle (write → read-until-our-id).
    // The line protocol has no per-request response routing — a concurrent
    // request would write while we read and we'd consume (and discard) ITS
    // response line.  One in-flight `request` per transport keeps each cycle
    // matched to its own reply.  `notify` (no response) need not take this.
    req_lock: Mutex<()>,
    // Keep the child alive for the transport's lifetime; killed on drop.
    _child: tokio::process::Child,
}

impl StdioTransport {
    /// Build the child `Command` with a **scrubbed** environment and piped
    /// stdio. Factored out of [`Self::spawn`] so the env hygiene is unit-testable
    /// without actually spawning a process.
    fn scrubbed_command(program: &str, args: &[String]) -> tokio::process::Command {
        let safe_env = crate::sandbox::scrub_env();
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .env_clear()
            .envs(&safe_env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        cmd
    }

    /// Spawn `program args…` as an MCP server and connect to its stdio.
    ///
    /// The child's environment is **scrubbed** (`env_clear` + `scrub_env`
    /// allowlist) before spawn, exactly like `run_sandboxed` and the
    /// turn-executor grandchild. Without this a third-party MCP server binary
    /// would inherit the harness's full environment — including
    /// `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, and every other
    /// `*TOKEN*/*SECRET*/*KEY*` var — which it never needs and could exfiltrate.
    /// A server that legitimately needs a service credential should receive it
    /// via explicit per-server config (a future slice), never by ambient
    /// inheritance.
    pub fn spawn(program: &str, args: &[String]) -> Result<Self, HarnessError> {
        let mut child = Self::scrubbed_command(program, args)
            .spawn()
            .map_err(|e| HarnessError::Other(format!("MCP spawn `{program}` failed: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HarnessError::Other("MCP child has no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HarnessError::Other("MCP child has no stdout".to_string()))?;
        Ok(Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
            req_lock: Mutex::new(()),
            _child: child,
        })
    }

    async fn write_message(&self, msg: &Value) -> Result<(), HarnessError> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }
}

#[async_trait]
impl RpcTransport for StdioTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        // Hold the cycle lock across write+read: no other request may write
        // until we've consumed our own response (see `req_lock`).
        let _cycle = self.req_lock.lock().await;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write_message(&req).await?;

        // Read lines until the response with our id arrives, skipping
        // notifications / server log lines that lack a matching id.  Bounded by
        // REQUEST_TIMEOUT so a server that never answers can't hang the caller.
        let read = async {
            let mut reader = self.stdout.lock().await;
            let mut line = String::new();
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    return Err(HarnessError::Other(format!(
                        "MCP server closed stdout before answering `{method}`"
                    )));
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let msg: Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue, // non-JSON server chatter
                };
                if msg.get("id").and_then(|v| v.as_u64()) != Some(id) {
                    continue; // not our response
                }
                if let Some(err) = msg.get("error") {
                    return Err(HarnessError::Other(format!("MCP `{method}` error: {err}")));
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }
        };
        match tokio::time::timeout(REQUEST_TIMEOUT, read).await {
            Ok(res) => res,
            Err(_) => Err(HarnessError::Other(format!(
                "MCP `{method}` timed out after {}s",
                REQUEST_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), HarnessError> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_message(&msg).await
    }
}

// ---------------------------------------------------------------------------
// Streamable HTTP transport
// ---------------------------------------------------------------------------

/// Reject a non-HTTPS MCP endpoint unless it targets loopback (SSRF / cleartext
/// guard). Operator-configured URLs still must not silently downgrade to
/// cleartext on the open network. Pure + unit-tested.
fn validate_http_url(url: &str) -> Result<(), HarnessError> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = http_host(rest);
        // Accept loopback only, including the bracketed IPv6 literal.
        if host == "localhost" || host == "127.0.0.1" || host == "[::1]" {
            return Ok(());
        }
        return Err(HarnessError::Other(format!(
            "MCP HTTP endpoint must be https (got cleartext `{url}`); only http://localhost is allowed"
        )));
    }
    Err(HarnessError::Other(format!(
        "MCP endpoint must be an http(s) URL: `{url}`"
    )))
}

/// Extract the host from a URL authority (scheme already stripped), preserving a
/// bracketed IPv6 literal (`[::1]:8080/x` → `[::1]`) instead of splitting it on
/// the inner `:`. For a normal authority the host ends at the first `:` or `/`.
fn http_host(authority: &str) -> &str {
    if authority.starts_with('[') {
        if let Some(end) = authority.find(']') {
            return &authority[..=end]; // include the brackets
        }
    }
    authority.split(['/', ':']).next().unwrap_or("")
}

/// Extract the JSON-RPC `result` for `expected_id` from an MCP HTTP response
/// body, which per the Streamable HTTP transport may be **either** a single
/// `application/json` object (or batch array) **or** a `text/event-stream` whose
/// `data:` lines carry the JSON-RPC messages. Pure + unit-tested — the socket
/// stays out of the framing logic.
fn parse_http_rpc_response(
    content_type: &str,
    body: &str,
    expected_id: u64,
    method: &str,
) -> Result<Value, HarnessError> {
    let candidates: Vec<Value> = if content_type.contains("text/event-stream") {
        body.lines()
            .filter_map(|l| l.trim().strip_prefix("data:"))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect()
    } else {
        match serde_json::from_str::<Value>(body.trim()) {
            Ok(Value::Array(a)) => a,
            Ok(v) => vec![v],
            Err(e) => {
                return Err(HarnessError::Other(format!(
                    "MCP `{method}` invalid JSON response: {e}"
                )));
            }
        }
    };
    for msg in candidates {
        if msg.get("id").and_then(|v| v.as_u64()) != Some(expected_id) {
            continue; // server->client request / other id — not our response
        }
        if let Some(err) = msg.get("error") {
            return Err(HarnessError::Other(format!("MCP `{method}` error: {err}")));
        }
        return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(HarnessError::Other(format!(
        "MCP `{method}`: no JSON-RPC response with id {expected_id}"
    )))
}

/// JSON-RPC 2.0 over the MCP **Streamable HTTP** transport: one POST per request
/// to a single endpoint, response as JSON or an SSE stream. Redirects are
/// disabled (SSRF hardening) and the server-assigned `Mcp-Session-Id` is echoed
/// on subsequent requests.
pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    /// Optional bearer token (`Authorization: Bearer …`).
    auth: Option<String>,
    /// Server-assigned session id (from the `initialize` response header),
    /// echoed on every later request.
    session_id: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl HttpTransport {
    /// Build a transport for `url`. Enforces https (except loopback) and
    /// disables redirects so a server can't bounce the client at an internal
    /// address. `auth` is an optional bearer token.
    pub fn new(url: impl Into<String>, auth: Option<String>) -> Result<Self, HarnessError> {
        let url = url.into();
        validate_http_url(&url)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| HarnessError::Other(format!("MCP HTTP client build failed: {e}")))?;
        Ok(Self {
            client,
            url,
            auth,
            session_id: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    async fn post(&self, body: &Value, method: &str) -> Result<reqwest::Response, HarnessError> {
        let mut req = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .json(body);
        if let Some(tok) = &self.auth {
            req = req.header("authorization", format!("Bearer {tok}"));
        }
        if let Some(sid) = self.session_id.lock().await.as_ref() {
            req = req.header("mcp-session-id", sid.as_str());
        }
        req.send()
            .await
            .map_err(|e| HarnessError::Other(format!("MCP HTTP `{method}` request failed: {e}")))
    }
}

#[async_trait]
impl RpcTransport for HttpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let call = async {
            let resp = self.post(&body, method).await?;
            let status = resp.status();
            // Capture a server-assigned session id (sent on the initialize
            // response) so subsequent requests are bound to the same session.
            if let Some(sid) = resp
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
            {
                *self.session_id.lock().await = Some(sid.to_string());
            }
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            let text = resp.text().await.map_err(|e| {
                HarnessError::Other(format!("MCP HTTP `{method}` body read failed: {e}"))
            })?;
            if !status.is_success() {
                let snippet: String = text.chars().take(200).collect();
                return Err(HarnessError::Other(format!(
                    "MCP HTTP `{method}` status {status}: {snippet}"
                )));
            }
            parse_http_rpc_response(&content_type, &text, id, method)
        };
        // Same bound as stdio: a server that never answers (or holds an SSE
        // stream open) can't hang the agent loop past REQUEST_TIMEOUT.
        match tokio::time::timeout(REQUEST_TIMEOUT, call).await {
            Ok(res) => res,
            Err(_) => Err(HarnessError::Other(format!(
                "MCP `{method}` timed out after {}s",
                REQUEST_TIMEOUT.as_secs()
            ))),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), HarnessError> {
        let body = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        // Fire-and-forget: a notification has no response channel. A transport
        // failure is still surfaced; a non-2xx body is ignored. Bounded by the
        // same REQUEST_TIMEOUT as `request` so a wedged server can't hang the
        // loop (the initialize handshake sends `notifications/initialized`).
        let send = async {
            self.post(&body, method).await?;
            Ok::<(), HarnessError>(())
        };
        match tokio::time::timeout(REQUEST_TIMEOUT, send).await {
            Ok(res) => res,
            Err(_) => Err(HarnessError::Other(format!(
                "MCP `{method}` notify timed out after {}s",
                REQUEST_TIMEOUT.as_secs()
            ))),
        }
    }
}

/// Resolve an MCP server's bearer token from the per-user
/// `~/.bwoc/secrets.toml` (`[mcp] <label>_token`) — the same location and
/// `0600` guard as the provider API keys. `None` when unset (an unauthenticated
/// server still works).
pub fn token_from_secrets(label: &str) -> Option<String> {
    let path = crate::provider::anthropic::home_dir()?
        .join(".bwoc")
        .join("secrets.toml");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.permissions().mode() & 0o077 != 0 {
                eprintln!(
                    "bwoc-harness: ignoring {} — it is group/world-accessible; `chmod 600` it.",
                    path.display()
                );
                return None;
            }
        }
    }
    let content = std::fs::read_to_string(&path).ok()?;
    let value: toml::Value = toml::from_str(&content).ok()?;
    let key = format!("{label}_token");
    let tok = value.get("mcp")?.get(&key)?.as_str()?.trim().to_string();
    (!tok.is_empty()).then_some(tok)
}

// ---------------------------------------------------------------------------
// MCP client
// ---------------------------------------------------------------------------

/// A discovered MCP tool definition.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// The MCP revision this harness implements and advertises in `initialize`.
/// See <https://modelcontextprotocol.io/specification>.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Protocol revisions this harness can speak, newest first. A server may
/// legitimately downgrade to an older shared revision during the `initialize`
/// handshake; a version outside this set has undefined semantics for us, so we
/// refuse the handshake rather than guess.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// MCP client over a [`RpcTransport`].
pub struct McpClient {
    transport: Arc<dyn RpcTransport>,
}

impl McpClient {
    pub fn new(transport: Arc<dyn RpcTransport>) -> Self {
        Self { transport }
    }

    /// Spawn an MCP server subprocess and complete the initialize handshake.
    pub async fn connect_stdio(program: &str, args: &[String]) -> Result<Self, HarnessError> {
        let client = Self::new(Arc::new(StdioTransport::spawn(program, args)?));
        client.initialize().await?;
        Ok(client)
    }

    /// Connect to a remote MCP server over the Streamable HTTP transport and
    /// complete the initialize handshake. `auth` is an optional bearer token.
    pub async fn connect_http(url: &str, auth: Option<String>) -> Result<Self, HarnessError> {
        let client = Self::new(Arc::new(HttpTransport::new(url, auth)?));
        client.initialize().await?;
        Ok(client)
    }

    /// Perform the MCP `initialize` handshake + `notifications/initialized`,
    /// negotiating the protocol revision. We advertise
    /// [`LATEST_PROTOCOL_VERSION`]; the server echoes the revision it will
    /// speak. Returns the negotiated version.
    pub async fn initialize(&self) -> Result<String, HarnessError> {
        let result = self
            .transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": LATEST_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "bwoc-harness", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        // The server MAY downgrade to an older shared revision; accept any we
        // still support. A missing field means a pre-negotiation server — assume
        // the version we asked for. A present-but-non-string value is a
        // malformed/hostile handshake — refuse rather than mask it as "missing".
        // An unknown (but well-typed) version has undefined semantics for us and
        // is refused below.
        let negotiated = match result.get("protocolVersion") {
            Some(Value::String(v)) => v.clone(),
            None => LATEST_PROTOCOL_VERSION.to_string(),
            Some(other) => {
                return Err(HarnessError::Provider(format!(
                    "MCP server returned a non-string protocolVersion: {other}"
                )));
            }
        };
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&negotiated.as_str()) {
            return Err(HarnessError::Provider(format!(
                "MCP server negotiated unsupported protocol version {negotiated:?}; \
                 this harness supports {SUPPORTED_PROTOCOL_VERSIONS:?}"
            )));
        }
        self.transport
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(negotiated)
    }

    /// List the server's tools (`tools/list`).
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>, HarnessError> {
        let result = self.transport.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(tools
            .into_iter()
            .filter_map(|t| {
                let name = t.get("name")?.as_str()?.to_string();
                let description = t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object" }));
                Some(McpToolDef {
                    name,
                    description,
                    input_schema,
                })
            })
            .collect())
    }

    /// Register every tool the server exposes into `registry`.  Returns the
    /// number registered.  Tool names are prefixed (`mcp__<server>__<tool>`) to
    /// avoid collisions with built-in tools and across servers.
    pub async fn register_tools(
        &self,
        registry: &mut ToolRegistry,
        server_label: &str,
    ) -> Result<usize, HarnessError> {
        let defs = self.list_tools().await?;
        let count = defs.len();
        for def in defs {
            registry.register(McpTool::new(self.transport.clone(), server_label, def));
        }
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// MCP-backed tool
// ---------------------------------------------------------------------------

/// A [`ToolImpl`] that forwards `execute` to an MCP server via `tools/call`.
pub struct McpTool {
    transport: Arc<dyn RpcTransport>,
    /// Prefixed display name, leaked to `&'static str` for the `ToolImpl` API.
    /// Bounded (one per discovered tool) and lives for the process — acceptable.
    name: &'static str,
    description: &'static str,
    /// The unprefixed tool name the server expects in `tools/call`.
    remote_name: String,
    schema: Value,
}

impl McpTool {
    fn new(transport: Arc<dyn RpcTransport>, server_label: &str, def: McpToolDef) -> Self {
        let display = format!("mcp__{server_label}__{}", def.name);
        Self {
            transport,
            name: Box::leak(display.into_boxed_str()),
            description: Box::leak(def.description.into_boxed_str()),
            remote_name: def.name,
            schema: def.input_schema,
        }
    }

    /// Extract human-readable text from an MCP `tools/call` result
    /// (`result.content[].text` joined), falling back to the raw JSON.
    fn extract_text(result: &Value) -> String {
        if let Some(items) = result.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<&str> = items
                .iter()
                .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
                .collect();
            if !texts.is_empty() {
                return texts.join("\n");
            }
        }
        result.to_string()
    }
}

#[async_trait]
impl ToolImpl for McpTool {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<String, HarnessError> {
        let result = self
            .transport
            .request(
                "tools/call",
                json!({ "name": self.remote_name, "arguments": args }),
            )
            .await?;
        Ok(Self::extract_text(&result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Canned transport: records calls, returns scripted results per method.
    struct MockTransport {
        calls: StdMutex<Vec<(String, Value)>>,
        /// `Some(v)` → server echoes `protocolVersion: v` at initialize (any JSON
        /// type, so wrong-type handshakes are testable); `None` → server omits
        /// the field (pre-negotiation server).
        init_version: Option<Value>,
    }
    impl MockTransport {
        fn new() -> Self {
            Self::with_init_version(Some(LATEST_PROTOCOL_VERSION))
        }
        fn with_init_version(v: Option<&str>) -> Self {
            Self::with_init_value(v.map(|s| json!(s)))
        }
        fn with_init_value(v: Option<Value>) -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
                init_version: v,
            }
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
            self.calls
                .lock()
                .unwrap()
                .push((method.to_string(), params.clone()));
            match method {
                "initialize" => Ok(match &self.init_version {
                    Some(v) => json!({ "protocolVersion": v, "capabilities": {},
                        "serverInfo": { "name": "mock", "version": "0" } }),
                    None => json!({ "capabilities": {} }),
                }),
                "tools/list" => Ok(json!({
                    "tools": [
                        { "name": "echo", "description": "echoes input",
                          "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } } }
                    ]
                })),
                "tools/call" => Ok(json!({
                    "content": [ { "type": "text", "text": format!("echoed: {}", params["arguments"]["text"].as_str().unwrap_or("")) } ]
                })),
                _ => Ok(Value::Null),
            }
        }
        async fn notify(&self, _method: &str, _params: Value) -> Result<(), HarnessError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn list_tools_parses_server_response() {
        let client = McpClient::new(Arc::new(MockTransport::new()));
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "echoes input");
    }

    #[tokio::test]
    async fn initialize_advertises_latest_and_returns_negotiated() {
        let mock = Arc::new(MockTransport::new());
        let client = McpClient::new(mock.clone());
        let v = client.initialize().await.unwrap();
        assert_eq!(v, LATEST_PROTOCOL_VERSION);
        // We advertised the latest version in the request params.
        let calls = mock.calls.lock().unwrap();
        let (_, params) = calls.iter().find(|(m, _)| m == "initialize").unwrap();
        assert_eq!(params["protocolVersion"], LATEST_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn initialize_accepts_server_downgrade_to_older_shared_version() {
        let client = McpClient::new(Arc::new(MockTransport::with_init_version(Some(
            "2024-11-05",
        ))));
        assert_eq!(client.initialize().await.unwrap(), "2024-11-05");
    }

    #[tokio::test]
    async fn initialize_defaults_when_server_omits_version() {
        let client = McpClient::new(Arc::new(MockTransport::with_init_version(None)));
        assert_eq!(client.initialize().await.unwrap(), LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn http_url_requires_https_except_loopback() {
        assert!(validate_http_url("https://example.com/mcp").is_ok());
        assert!(validate_http_url("http://localhost:8080/mcp").is_ok());
        assert!(validate_http_url("http://127.0.0.1/mcp").is_ok());
        // IPv6 loopback literal (brackets must not be split on the inner `:`).
        assert!(validate_http_url("http://[::1]:9000/mcp").is_ok());
        assert_eq!(http_host("[::1]:9000/mcp"), "[::1]");
        // Cleartext to a non-loopback host is refused (SSRF / downgrade guard).
        assert!(validate_http_url("http://example.com/mcp").is_err());
        // A non-loopback IPv6 literal over cleartext is still refused.
        assert!(validate_http_url("http://[2001:db8::1]/mcp").is_err());
        // Non-http(s) schemes are refused.
        assert!(validate_http_url("ftp://example.com").is_err());
        assert!(validate_http_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn http_response_parses_plain_json() {
        let body = r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#;
        let r = parse_http_rpc_response("application/json", body, 7, "tools/call").unwrap();
        assert_eq!(r["ok"], true);
    }

    #[test]
    fn http_response_parses_sse_and_skips_other_ids() {
        // An SSE body may carry unrelated messages (a server->client ping with a
        // different id) before our response; we pick the one matching our id.
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"other\":1}}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"mine\":true}}\n\n";
        let r = parse_http_rpc_response("text/event-stream", body, 2, "tools/list").unwrap();
        assert_eq!(r["mine"], true);
    }

    #[test]
    fn http_response_surfaces_rpc_error_and_missing_id() {
        let err_body = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"nope"}}"#;
        let e = parse_http_rpc_response("application/json", err_body, 3, "tools/call").unwrap_err();
        assert!(e.to_string().contains("nope"), "got: {e}");
        // A body with no matching id is an error, not a silent null.
        let miss = parse_http_rpc_response("application/json", err_body, 99, "x").unwrap_err();
        assert!(
            miss.to_string().contains("no JSON-RPC response"),
            "got: {miss}"
        );
    }

    #[test]
    fn http_transport_rejects_cleartext_at_construction() {
        assert!(HttpTransport::new("http://evil.example/mcp", None).is_err());
        assert!(HttpTransport::new("https://ok.example/mcp", Some("t".into())).is_ok());
    }

    #[tokio::test]
    async fn initialize_rejects_non_string_version() {
        // A present-but-wrong-type protocolVersion is a malformed handshake and
        // must fail fast, not be masked as "missing" and defaulted.
        let client = McpClient::new(Arc::new(MockTransport::with_init_value(Some(json!(
            20250618
        )))));
        let err = client.initialize().await.unwrap_err();
        assert!(
            err.to_string().contains("non-string protocolVersion"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn initialize_rejects_unsupported_version() {
        let client = McpClient::new(Arc::new(MockTransport::with_init_version(Some(
            "1999-01-01",
        ))));
        let err = client.initialize().await.unwrap_err();
        assert!(
            err.to_string().contains("unsupported protocol version"),
            "got: {err}"
        );
    }

    #[test]
    fn spawn_command_scrubs_environment() {
        // The command an MCP server is spawned with must carry ONLY the
        // scrub_env allowlist — never the harness's ambient credentials
        // (issue #336). Inspect the built command's env without spawning.
        let cmd = StdioTransport::scrubbed_command("true", &[]);
        let envs: std::collections::HashMap<String, String> = cmd
            .as_std()
            .get_envs()
            .filter_map(|(k, v)| {
                v.map(|v| (k.to_string_lossy().into(), v.to_string_lossy().into()))
            })
            .collect();

        // No credential-shaped var may be present.
        for k in envs.keys() {
            let up = k.to_uppercase();
            assert!(
                !(up.contains("KEY")
                    || up.contains("SECRET")
                    || up.contains("TOKEN")
                    || up.contains("PASSWORD")),
                "sensitive-looking var `{k}` reached the MCP server command"
            );
        }
        // And the env is exactly the scrub_env allowlist (env_clear applied).
        assert_eq!(envs, crate::sandbox::scrub_env());
    }

    // Behavioral proof that `env_clear()` actually keeps inherited secrets out of
    // the child (the get_envs test above inspects only the explicitly-set vars,
    // so it alone would still pass if `.env_clear()` were removed). Unix-only:
    // spawns a real `/bin/sh -c env` and reads the child's actual environment.
    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_child_does_not_inherit_ambient_secrets() {
        // SAFETY: set/removed around a single spawn; the bwoc-harness suite runs
        // serially (--test-threads=1) in CI. The value is a marker, not a real
        // secret. The name is neither allowlisted nor sensitive-pattern-matched,
        // so only `env_clear` can keep it out of the child — exactly what we test.
        unsafe { std::env::set_var("BWOC_MCP_LEAK_CANARY", "must-not-reach-child") };
        let out = StdioTransport::scrubbed_command("/bin/sh", &["-c".into(), "env".into()])
            .output()
            .await
            .expect("spawn env dump");
        unsafe { std::env::remove_var("BWOC_MCP_LEAK_CANARY") };

        let env = String::from_utf8_lossy(&out.stdout);
        assert!(
            !env.contains("BWOC_MCP_LEAK_CANARY"),
            "ambient var leaked into the MCP child despite env_clear:\n{env}"
        );
        // PATH must still reach the child so the server can find its executable.
        assert!(env.contains("PATH="), "PATH should pass through:\n{env}");
    }

    #[tokio::test]
    async fn register_tools_into_registry_with_prefixed_name() {
        let client = McpClient::new(Arc::new(MockTransport::new()));
        let mut reg = ToolRegistry::new();
        let n = client.register_tools(&mut reg, "demo").await.unwrap();
        assert_eq!(n, 1);
        // Registered under the prefixed name and visible in the schema list.
        assert!(reg.get("mcp__demo__echo").is_some());
        let names: Vec<_> = reg
            .tool_schemas()
            .into_iter()
            .map(|t| t.function.name)
            .collect();
        assert!(names.contains(&"mcp__demo__echo".to_string()));
    }

    #[tokio::test]
    async fn mcp_tool_execute_forwards_to_tools_call() {
        let transport = Arc::new(MockTransport::new());
        let tool = McpTool::new(
            transport.clone(),
            "demo",
            McpToolDef {
                name: "echo".to_string(),
                description: "echoes".to_string(),
                input_schema: json!({ "type": "object" }),
            },
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let out = tool.execute(json!({ "text": "hi" }), &ctx).await.unwrap();
        assert_eq!(out, "echoed: hi");
        // The remote (unprefixed) name is what crosses the wire.
        let calls = transport.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .any(|(m, p)| m == "tools/call" && p["name"] == "echo")
        );
    }
}
