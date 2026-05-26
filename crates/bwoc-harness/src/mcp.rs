//! MCP client (HV2-5) — consume external MCP tool servers.
//!
//! The harness speaks the Model Context Protocol as a **client only** (the
//! server role is deferred — a network surface with no current need).  An MCP
//! server is launched as a subprocess; the harness speaks JSON-RPC 2.0 over its
//! stdio (line-delimited messages), discovers the server's tools, and registers
//! each as an ordinary [`ToolImpl`](crate::tools::ToolImpl).  Because they are
//! ordinary tools, MCP calls flow through `dispatch` → `execute_tool_calls` →
//! guardrails → permission → sandbox like every other tool — no bypass.
//!
//! Hand-rolled on `serde_json` + `tokio::process` (both already deps) rather
//! than an SDK, matching the hand-rolled OpenAI provider and keeping the
//! dep-quarantine clean.  stdio transport only; HTTP/SSE is deferred.

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
    /// Spawn `program args…` as an MCP server and connect to its stdio.
    pub fn spawn(program: &str, args: &[String]) -> Result<Self, HarnessError> {
        let mut child = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
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
// MCP client
// ---------------------------------------------------------------------------

/// A discovered MCP tool definition.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

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

    /// Perform the MCP `initialize` handshake + `notifications/initialized`.
    pub async fn initialize(&self) -> Result<(), HarnessError> {
        self.transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "bwoc-harness", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;
        self.transport
            .notify("notifications/initialized", json!({}))
            .await?;
        Ok(())
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
    }
    impl MockTransport {
        fn new() -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
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
