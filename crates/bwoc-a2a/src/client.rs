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

/// Webhook delivery is best-effort and must not stall the watcher loop, so it
/// uses a tighter timeout than interactive client calls.
const WEBHOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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
    #[error("webhook blocked by SSRF guard: {0}")]
    Ssrf(#[source] crate::ssrf::SsrfError),
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

/// Fetch and parse a remote agent's Agent Card from its base URL. `auth`
/// presents `Authorization: Bearer` (best-effort — the card GET is public by
/// the A2A spec, but a peer may protect its own card).
pub async fn fetch_card(base: &str, auth: Option<&str>) -> Result<AgentCard, ClientError> {
    let url = card_url(base);
    let mut request = http_client()?.get(&url);
    if let Some(token) = auth {
        request = request.bearer_auth(token);
    }
    let resp = request.send().await.map_err(|source| ClientError::Http {
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
    auth: Option<&str>,
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

    let mut http_req = http_client()?.post(endpoint).json(&request);
    if let Some(token) = auth {
        http_req = http_req.bearer_auth(token);
    }
    let resp = http_req.send().await.map_err(|source| ClientError::Http {
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

/// POST a task-status event to a registered push webhook (AP3 delivery). The
/// URL is cleared by the [`crate::ssrf`] guard first, and the connection is
/// **pinned** to the validated address(es) so a DNS rebind can't redirect the
/// POST to an internal service. The config's token (when set) is presented as
/// `Authorization: Bearer`. `allow_loopback` is test-only (target a local mock).
///
/// Best-effort: any non-2xx is a [`ClientError::Status`]; the caller logs and
/// moves on (no retry in this phase).
pub async fn deliver_push(
    webhook_url: &str,
    token: Option<&str>,
    event: &Value,
    allow_loopback: bool,
) -> Result<(), ClientError> {
    let validated = crate::ssrf::validate(webhook_url, allow_loopback)
        .await
        .map_err(ClientError::Ssrf)?;
    let client = reqwest::Client::builder()
        .timeout(WEBHOOK_TIMEOUT)
        .user_agent(concat!("bwoc-a2a/", env!("CARGO_PKG_VERSION")))
        .resolve_to_addrs(&validated.host, &validated.addrs)
        .build()
        .map_err(|source| ClientError::Http {
            url: webhook_url.to_string(),
            source,
        })?;
    let mut req = client.post(webhook_url).json(event);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|source| ClientError::Http {
        url: webhook_url.to_string(),
        source,
    })?;
    if !resp.status().is_success() {
        return Err(ClientError::Status {
            url: webhook_url.to_string(),
            status: resp.status().as_u16(),
        });
    }
    Ok(())
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

        let got = fetch_card(&server.uri(), None).await.unwrap();
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

        let result = send_message(&format!("{}/", server.uri()), "hello", None, "m1", None)
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
        let result = send_message(
            &format!("{}/", server.uri()),
            "hi",
            Some("ctx-1"),
            "m1",
            None,
        )
        .await
        .unwrap();
        assert_eq!(result["ok"], true);
    }

    #[tokio::test]
    async fn send_message_presents_bearer_when_auth_given() {
        use wiremock::matchers::header;
        let server = MockServer::start().await;
        // The mock only matches with the bearer header, so a success proves the
        // client presented it.
        Mock::given(http_method("POST"))
            .and(path("/"))
            .and(header("authorization", "Bearer out-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0", "id": "m1", "result": { "ok": true }
            })))
            .mount(&server)
            .await;
        let result = send_message(
            &format!("{}/", server.uri()),
            "hi",
            None,
            "m1",
            Some("out-tok"),
        )
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
        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1", None)
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

        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1", None)
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
        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1", None)
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
        let err = send_message(&format!("{}/", server.uri()), "hi", None, "m1", None)
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
        let err = fetch_card(&server.uri(), None).await.unwrap_err();
        assert!(matches!(err, ClientError::Status { status: 404, .. }));
    }

    #[tokio::test]
    async fn deliver_push_posts_event_with_bearer_and_pins_loopback() {
        use wiremock::matchers::{body_partial_json, header};
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/hook"))
            .and(header("authorization", "Bearer s3cr3t"))
            .and(body_partial_json(serde_json::json!({
                "taskId": "t1", "kind": "status-update",
                "status": { "state": "TASK_STATE_COMPLETED" }, "final": true
            })))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let event =
            crate::push::status_event("t1", "team-sec", crate::types::TaskState::Completed, true);
        // allow_loopback=true: target the local mock past the SSRF guard.
        deliver_push(
            &format!("{}/hook", server.uri()),
            Some("s3cr3t"),
            &event,
            true,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn deliver_push_refuses_private_target_via_ssrf() {
        let event = serde_json::json!({ "kind": "status-update" });
        let err = deliver_push("https://10.0.0.1/hook", None, &event, false)
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Ssrf(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn deliver_push_maps_non_2xx_to_status_error() {
        let server = MockServer::start().await;
        Mock::given(http_method("POST"))
            .and(path("/hook"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let event = serde_json::json!({ "kind": "status-update" });
        let err = deliver_push(&format!("{}/hook", server.uri()), None, &event, true)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ClientError::Status { status: 500, .. }),
            "got {err:?}"
        );
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
