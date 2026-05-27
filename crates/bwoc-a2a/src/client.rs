//! Outbound A2A client (#48 **P4**) — lets a BWOC agent *initiate* A2A calls to
//! an external agent, the complement of the inbound [`crate::serve`] listener.
//!
//! Two operations: fetch a remote Agent Card (discovery) and send it a message
//! (`SendMessage`). Both are async (reqwest); the `bwoc-a2a` binary wraps them
//! in a blocking runtime so the CLI stays synchronous. reqwest lives here, never
//! in `bwoc-cli` (dep-quarantine).

use serde_json::Value;

use crate::types::{AGENT_CARD_WELL_KNOWN_PATH, AgentCard, method};

/// How long an outbound A2A request may take before it's abandoned.
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error talking to {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("agent at {url} returned HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("could not parse the response from {url}: {message}")]
    Decode { url: String, message: String },
    #[error("agent returned a JSON-RPC error {code}: {message}")]
    Rpc { code: i64, message: String },
}

fn http_client() -> Result<reqwest::Client, ClientError> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("bwoc-a2a/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|source| ClientError::Http {
            url: "(client init)".to_string(),
            source,
        })
}

/// The Agent Card well-known URL for a base endpoint. `base` may include or omit
/// a trailing slash; if it already points at the well-known path, it's used as-is.
fn card_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with(AGENT_CARD_WELL_KNOWN_PATH) {
        return trimmed.to_string();
    }
    format!("{trimmed}{AGENT_CARD_WELL_KNOWN_PATH}")
}

/// Fetch and parse a remote agent's Agent Card from its base URL.
pub async fn fetch_card(base: &str) -> Result<AgentCard, ClientError> {
    let url = card_url(base);
    let resp = http_client()?
        .get(&url)
        .send()
        .await
        .map_err(|source| ClientError::Http {
            url: url.clone(),
            source,
        })?;
    if !resp.status().is_success() {
        return Err(ClientError::Status {
            url,
            status: resp.status().as_u16(),
        });
    }
    resp.json::<AgentCard>()
        .await
        .map_err(|e| ClientError::Decode {
            url,
            message: e.to_string(),
        })
}

/// Send a text message to a remote A2A endpoint via `SendMessage`. Returns the
/// JSON-RPC `result` (a `Task` or `Message`), or a [`ClientError::Rpc`] if the
/// peer answered with a JSON-RPC error.
pub async fn send_message(
    endpoint: &str,
    text: &str,
    context_id: Option<&str>,
    message_id: &str,
) -> Result<Value, ClientError> {
    let mut message = serde_json::json!({
        "role": "ROLE_USER",
        "parts": [{ "text": text }],
        "messageId": message_id,
    });
    if let Some(ctx) = context_id {
        message["contextId"] = Value::String(ctx.to_string());
    }
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": message_id,
        "method": method::SEND_MESSAGE,
        "params": { "message": message },
    });

    let resp = http_client()?
        .post(endpoint)
        .json(&request)
        .send()
        .await
        .map_err(|source| ClientError::Http {
            url: endpoint.to_string(),
            source,
        })?;
    if !resp.status().is_success() {
        return Err(ClientError::Status {
            url: endpoint.to_string(),
            status: resp.status().as_u16(),
        });
    }
    let parsed: JsonRpcResponseOwned = resp.json().await.map_err(|e| ClientError::Decode {
        url: endpoint.to_string(),
        message: e.to_string(),
    })?;
    let decode = |message: String| ClientError::Decode {
        url: endpoint.to_string(),
        message,
    };
    // Validate the JSON-RPC envelope: version must be "2.0" (when present), and
    // the response `id` must echo our request `id` so we can't accept a
    // mismatched/replayed reply.
    if let Some(v) = &parsed.jsonrpc {
        if v != "2.0" {
            return Err(decode(format!("unexpected jsonrpc version `{v}`")));
        }
    }
    if let Some(id) = &parsed.id {
        if id != &Value::String(message_id.to_string()) {
            return Err(decode(format!(
                "response id {id} does not match request id `{message_id}`"
            )));
        }
    }
    match (parsed.result, parsed.error) {
        // result XOR error — a response carrying both is malformed.
        (Some(_), Some(_)) => Err(decode(
            "response contains both `result` and `error`".to_string(),
        )),
        (Some(result), None) => Ok(result),
        (None, Some(err)) => Err(ClientError::Rpc {
            code: err.code,
            message: err.message,
        }),
        (None, None) => Err(decode("response had neither result nor error".to_string())),
    }
}

/// A deserializable mirror of the server-side `JsonRpcResponse` (which is
/// serialize-only) — just the fields the client reads back + the envelope
/// fields it validates.
#[derive(serde::Deserialize)]
struct JsonRpcResponseOwned {
    jsonrpc: Option<String>,
    id: Option<Value>,
    result: Option<Value>,
    error: Option<RpcErrorOwned>,
}

#[derive(serde::Deserialize)]
struct RpcErrorOwned {
    code: i64,
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method as http_method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_card_parses_a_remote_card() {
        let server = MockServer::start().await;
        let card = serde_json::json!({
            "name": "remote-oracle",
            "description": "an external A2A agent",
            "url": format!("{}/", server.uri()),
            "version": "9.9",
            "protocolVersion": "1.0.0",
            "capabilities": { "streaming": true, "pushNotifications": false },
            "defaultInputModes": ["text/plain"],
            "defaultOutputModes": ["text/plain"],
            "skills": []
        });
        Mock::given(http_method("GET"))
            .and(path(AGENT_CARD_WELL_KNOWN_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(&card))
            .mount(&server)
            .await;

        let got = fetch_card(&server.uri()).await.unwrap();
        assert_eq!(got.name, "remote-oracle");
        assert_eq!(got.protocol_version, "1.0.0");
        assert!(got.capabilities.streaming);
    }

    #[tokio::test]
    async fn send_message_returns_the_result() {
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": "m1",
                "result": { "role": "ROLE_AGENT", "parts": [{ "text": "got it" }], "messageId": "ack-m1" }
            })))
            .mount(&server)
            .await;

        let result = send_message(&format!("{}/", server.uri()), "hello", None, "m1")
            .await
            .unwrap();
        assert_eq!(result["parts"][0]["text"], "got it");
    }

    #[tokio::test]
    async fn send_message_emits_context_id_when_provided() {
        use wiremock::matchers::body_partial_json;
        let server = MockServer::start().await;
        // The mock only matches if the request body carries the contextId, so a
        // successful call proves the client emitted it.
        Mock::given(http_method("POST"))
            .and(path("/"))
            .and(body_partial_json(serde_json::json!({
                "params": { "message": { "contextId": "ctx-1" } }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0", "id": "m1", "result": { "ok": true }
            })))
            .mount(&server)
            .await;
        let result = send_message(&format!("{}/", server.uri()), "hi", Some("ctx-1"), "m1")
            .await
            .unwrap();
        assert_eq!(result["ok"], true);
    }

    #[tokio::test]
    async fn send_message_maps_non_2xx_to_status_error() {
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1")
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Status { status: 500, .. }));
    }

    #[tokio::test]
    async fn send_message_surfaces_a_jsonrpc_error() {
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": "m1",
                "error": { "code": -32602, "message": "invalid `message`" }
            })))
            .mount(&server)
            .await;

        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1")
            .await
            .unwrap_err();
        match err {
            ClientError::Rpc { code, .. } => assert_eq!(code, -32602),
            other => panic!("expected Rpc error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_response_with_mismatched_id() {
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0", "id": "WRONG", "result": { "ok": true }
            })))
            .mount(&server)
            .await;
        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1")
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Decode { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn rejects_response_with_both_result_and_error() {
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0", "id": "m1",
                "result": { "ok": true }, "error": { "code": -1, "message": "x" }
            })))
            .mount(&server)
            .await;
        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1")
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Decode { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn non_2xx_status_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(http_method("GET"))
            .and(path(AGENT_CARD_WELL_KNOWN_PATH))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = fetch_card(&server.uri()).await.unwrap_err();
        assert!(matches!(err, ClientError::Status { status: 404, .. }));
    }

    #[test]
    fn card_url_handles_trailing_slash_and_well_known() {
        assert_eq!(
            card_url("http://x:1"),
            "http://x:1/.well-known/agent-card.json"
        );
        assert_eq!(
            card_url("http://x:1/"),
            "http://x:1/.well-known/agent-card.json"
        );
        assert_eq!(
            card_url("http://x:1/.well-known/agent-card.json"),
            "http://x:1/.well-known/agent-card.json"
        );
    }
}
