//! A2A HTTP transport (#48 **P1-serve**). A minimal axum listener exposing one
//! local BWOC agent over A2A: the Agent Card at the well-known path and a
//! JSON-RPC endpoint that hands requests to [`crate::rpc::dispatch`].
//!
//! **Security posture (P1):** there is no authentication yet, so the listener
//! is meant to bind **loopback only** — the CLI defaults to `127.0.0.1` and
//! warns on any non-loopback `--bind`. Bounded-growth guards are in place: a
//! per-request body-size limit ([`MAX_REQUEST_BYTES`]) and the inbox size cap
//! in [`crate::rpc`]. Per-peer rate limiting and auth land in a later phase
//! (P1 has no peer identity — every inbound message is `from:"a2a"`).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};

use crate::rpc::{ServeContext, dispatch};
use crate::types::{AGENT_CARD_WELL_KNOWN_PATH, AgentCard, JsonRpcRequest, JsonRpcResponse};

/// Max bytes accepted in a single JSON-RPC request body (1 MiB).
pub const MAX_REQUEST_BYTES: usize = 1 << 20;

/// JSON-RPC parse / invalid-request error codes (the rest live in [`crate::rpc`]).
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;

/// Everything the listener needs to represent one local agent over A2A.
pub struct ServeConfig {
    /// The local agent this server speaks for (the envelope `to`).
    pub agent_id: String,
    /// That agent's `inbox.jsonl`.
    pub inbox_path: PathBuf,
    /// The Agent Card served at the well-known path.
    pub card: AgentCard,
    /// Address to bind. Callers default this to loopback.
    pub addr: SocketAddr,
    /// Team task list to expose over `tasks/*` (P2): `(team_id, tasks.jsonl)`.
    /// `None` when no `--team` was selected.
    pub team: Option<(String, PathBuf)>,
}

struct ServeState {
    agent_id: String,
    inbox_path: PathBuf,
    card: AgentCard,
    team: Option<(String, PathBuf)>,
}

/// Run the A2A listener, blocking until shutdown. Creates its own current-thread
/// tokio runtime so callers (the CLI) stay synchronous and tokio does not leak
/// into their public surface.
pub fn serve_blocking(cfg: ServeConfig) -> std::io::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(run(cfg))
}

async fn run(cfg: ServeConfig) -> std::io::Result<()> {
    let state = Arc::new(ServeState {
        agent_id: cfg.agent_id,
        inbox_path: cfg.inbox_path,
        card: cfg.card,
        team: cfg.team,
    });
    let listener = tokio::net::TcpListener::bind(cfg.addr).await?;
    axum::serve(listener, app(state)).await
}

/// Build the router for a given agent state. Factored out so tests can drive it
/// via `tower::ServiceExt::oneshot` without binding a socket.
fn app(state: Arc<ServeState>) -> Router {
    Router::new()
        .route(AGENT_CARD_WELL_KNOWN_PATH, get(agent_card))
        .route("/", post(json_rpc))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}

async fn agent_card(State(state): State<Arc<ServeState>>) -> Json<AgentCard> {
    Json(state.card.clone())
}

/// JSON-RPC endpoint. Accepts the body as raw JSON first so a malformed request
/// gets a spec-conformant JSON-RPC error (carried over HTTP 200) rather than a
/// bare 400. A well-formed notification (no `id`) gets `204 No Content`.
async fn json_rpc(
    State(state): State<Arc<ServeState>>,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> Response {
    let raw = match body {
        Ok(Json(v)) => v,
        Err(rej) => {
            // The body-size limit surfaces as a 413 rejection. Report that as a
            // clear "too large" rather than a misleading JSON-RPC parse error;
            // genuinely malformed JSON stays `-32700` (id unknown).
            return if rej.status() == StatusCode::PAYLOAD_TOO_LARGE {
                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!("request body exceeds the {MAX_REQUEST_BYTES}-byte limit"),
                )
                    .into_response()
            } else {
                rpc_error(serde_json::Value::Null, PARSE_ERROR, "parse error")
            };
        }
    };
    let req: JsonRpcRequest = match serde_json::from_value(raw.clone()) {
        Ok(r) => r,
        Err(e) => {
            // Echo the caller's id if we can still see it in the raw JSON.
            let id = raw.get("id").cloned().unwrap_or(serde_json::Value::Null);
            return rpc_error(id, INVALID_REQUEST, format!("invalid request: {e}"));
        }
    };

    let ctx = ServeContext {
        agent_id: &state.agent_id,
        inbox_path: &state.inbox_path,
        tasks: state
            .team
            .as_ref()
            .map(|(team_id, path)| crate::rpc::TasksContext {
                team_id,
                tasks_path: path,
            }),
    };
    match dispatch(&req, &ctx) {
        Some(resp) => Json(resp).into_response(),
        // Notification: per JSON-RPC 2.0 the server emits no body.
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

fn rpc_error(id: serde_json::Value, code: i64, message: impl Into<String>) -> Response {
    Json(JsonRpcResponse::err(id, code, message)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentCapabilities, method};
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, header};
    use tower::ServiceExt; // for `oneshot`

    fn test_state() -> (Arc<ServeState>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let card = AgentCard {
            name: "agent-yudi".into(),
            description: "test".into(),
            url: "http://127.0.0.1:0/".into(),
            version: "2.8.0".into(),
            protocol_version: crate::card::A2A_PROTOCOL_VERSION.into(),
            capabilities: AgentCapabilities::default(),
            default_input_modes: vec!["text/plain".into()],
            default_output_modes: vec!["text/plain".into()],
            skills: vec![],
        };
        let state = Arc::new(ServeState {
            agent_id: "agent-yudi".into(),
            inbox_path: dir.path().join(".bwoc/inbox.jsonl"),
            card,
            team: None,
        });
        (state, dir)
    }

    /// State exposing a team whose `tasks.jsonl` holds one pending task `t1`.
    fn test_state_with_team() -> (Arc<ServeState>, tempfile::TempDir) {
        use bwoc_core::team::{Task, render_tasks};
        let (base, dir) = test_state();
        let tasks_path = dir.path().join("teams/team-security/tasks.jsonl");
        std::fs::create_dir_all(tasks_path.parent().unwrap()).unwrap();
        std::fs::write(
            &tasks_path,
            render_tasks(&[Task::new("t1", "harden listener", vec![])]).unwrap(),
        )
        .unwrap();
        let state = Arc::new(ServeState {
            agent_id: base.agent_id.clone(),
            inbox_path: base.inbox_path.clone(),
            card: base.card.clone(),
            team: Some(("team-security".into(), tasks_path)),
        });
        (state, dir)
    }

    fn post_json(body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn well_known_serves_agent_card() {
        let (state, _d) = test_state();
        let resp = app(state)
            .oneshot(
                Request::builder()
                    .uri(AGENT_CARD_WELL_KNOWN_PATH)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), MAX_REQUEST_BYTES).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["name"], "agent-yudi");
        assert_eq!(v["protocolVersion"], crate::card::A2A_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn send_message_over_http_delivers_to_inbox() {
        let (state, _d) = test_state();
        let inbox = state.inbox_path.clone();
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":method::SEND_MESSAGE,
                "params":{"message":{"role":"ROLE_USER","parts":[{"text":"hi"}],"messageId":"m1"}}
            })))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let line = std::fs::read_to_string(&inbox).unwrap();
        assert!(line.contains("\"messageId\":\"m1\""));
        assert!(line.contains("\"from\":\"a2a\""));
    }

    #[tokio::test]
    async fn notification_gets_204_no_body() {
        let (state, _d) = test_state();
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","method":method::SEND_MESSAGE,
                "params":{"message":{"role":"ROLE_USER","parts":[{"text":"hi"}],"messageId":"n1"}}
            })))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn malformed_json_returns_jsonrpc_parse_error() {
        let (state, _d) = test_state();
        let resp = app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        // JSON-RPC errors ride a 200; the error is in the body.
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), MAX_REQUEST_BYTES).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], PARSE_ERROR);
    }

    #[tokio::test]
    async fn list_and_get_tasks_bridge_the_team_list() {
        let (state, _d) = test_state_with_team();
        // ListTasks → one task mapped to TASK_STATE_SUBMITTED (pending).
        let resp = app(state.clone())
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":method::LIST_TASKS,"params":{}
            })))
            .await
            .unwrap();
        let v = body_json(resp).await;
        assert_eq!(v["result"]["tasks"][0]["id"], "t1");
        assert_eq!(v["result"]["tasks"][0]["contextId"], "team-security");
        assert_eq!(
            v["result"]["tasks"][0]["status"]["state"],
            "TASK_STATE_SUBMITTED"
        );
        // GetTask t1 → the task; an unknown id → TASK_NOT_FOUND (-32001).
        let got = app(state.clone())
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":2,"method":method::GET_TASK,"params":{"id":"t1"}
            })))
            .await
            .unwrap();
        let task = body_json(got).await;
        assert_eq!(task["result"]["id"], "t1");
        // No-leak contract: the A2A Task must expose only id/contextId/status —
        // never the BWOC task's title, claimant, plan, or deps.
        let obj = task["result"].as_object().unwrap();
        for leaked in ["title", "claimed_by", "claimedBy", "plan", "deps"] {
            assert!(!obj.contains_key(leaked), "{leaked} must not be exposed");
        }
        let missing = app(state.clone())
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":3,"method":method::GET_TASK,"params":{"id":"nope"}
            })))
            .await
            .unwrap();
        assert_eq!(body_json(missing).await["error"]["code"], -32001);
        // CancelTask → TASK_NOT_CANCELABLE (-32002), never a fake cancel.
        let cancel = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":4,"method":method::CANCEL_TASK,"params":{"id":"t1"}
            })))
            .await
            .unwrap();
        assert_eq!(body_json(cancel).await["error"]["code"], -32002);
    }

    #[tokio::test]
    async fn list_tasks_is_empty_when_no_team_selected() {
        let (state, _d) = test_state(); // team: None
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":method::LIST_TASKS,"params":{}
            })))
            .await
            .unwrap();
        let v = body_json(resp).await;
        assert_eq!(v["result"]["tasks"].as_array().unwrap().len(), 0);
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = to_bytes(resp.into_body(), MAX_REQUEST_BYTES).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn oversize_body_returns_413_not_parse_error() {
        let (state, _d) = test_state();
        let big = "x".repeat(MAX_REQUEST_BYTES + 1);
        let resp = app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(big))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
