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

use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};

use crate::rpc::{ServeContext, TasksContext, dispatch};
use crate::types::{
    AGENT_CARD_WELL_KNOWN_PATH, AgentCard, JsonRpcRequest, JsonRpcResponse, method,
};

/// Max bytes accepted in a single JSON-RPC request body (1 MiB).
pub const MAX_REQUEST_BYTES: usize = 1 << 20;

/// JSON-RPC parse / invalid-request error codes (the rest live in [`crate::rpc`]).
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const TASK_NOT_FOUND: i64 = -32001;

/// `SubscribeToTask` SSE poll interval + max lifetime. The cap bounds each
/// stream so a never-completing task can't hold a connection open forever
/// (a network-exposed resource guard, like the inbox cap). A *concurrency* cap
/// (limit on simultaneous subscriptions per peer) waits for the auth phase,
/// alongside per-peer rate limiting — P1 has no peer identity.
const SUBSCRIBE_POLL: Duration = Duration::from_secs(1);
const SUBSCRIBE_MAX: Duration = Duration::from_secs(300);

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

    // Streaming methods (P3) answer with an SSE stream rather than one JSON
    // response, so they branch off before the unary dispatch.
    match req.method.as_str() {
        method::SEND_STREAMING_MESSAGE => return stream_send_message(&state, &req),
        method::SUBSCRIBE_TO_TASK => return subscribe_task(&state, &req),
        _ => {}
    }

    let ctx = make_ctx(&state);
    match dispatch(&req, &ctx) {
        Some(resp) => Json(resp).into_response(),
        // Notification: per JSON-RPC 2.0 the server emits no body.
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Borrow the per-request dispatch context out of the shared server state.
fn make_ctx(state: &ServeState) -> ServeContext<'_> {
    ServeContext {
        agent_id: &state.agent_id,
        inbox_path: &state.inbox_path,
        tasks: state.team.as_ref().map(|(team_id, path)| TasksContext {
            team_id,
            tasks_path: path,
        }),
    }
}

fn rpc_error(id: serde_json::Value, code: i64, message: impl Into<String>) -> Response {
    Json(JsonRpcResponse::err(id, code, message)).into_response()
}

/// `SendStreamingMessage` (SSE). BWOC processes messages asynchronously (the
/// agent reads its inbox out-of-band), so there is nothing to stream
/// incrementally: the message is delivered to the inbox exactly as the unary
/// `SendMessage` would, then a **single** event carrying the delivery ack is
/// emitted and the stream closes. Honest about the async model rather than
/// faking progress events.
fn stream_send_message(state: &Arc<ServeState>, req: &JsonRpcRequest) -> Response {
    // Reuse the unary SendMessage path for the inbox write + ack.
    let unary = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method::SEND_MESSAGE.to_string(),
        params: req.params.clone(),
        id: req.id.clone(),
    };
    let outcome = dispatch(&unary, &make_ctx(state));
    match outcome {
        // Normal request: stream the single ack event, then close.
        Some(ack) => {
            let data = serde_json::to_string(&ack).unwrap_or_default();
            let stream = async_stream::stream! {
                yield Ok::<_, Infallible>(Event::default().data(data));
            };
            Sse::new(stream).into_response()
        }
        // Notification (no `id`): the inbox write already happened inside
        // `dispatch`; per JSON-RPC 2.0 emit no body — no SSE response.
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// `SubscribeToTask` (SSE). Tails a team task's state: emits a
/// `TaskStatusUpdateEvent` for the current state, then one more whenever the
/// state changes, closing (`final: true`) when the task reaches `Completed` or
/// after [`SUBSCRIBE_MAX`]. Pre-flight failures (no team, unknown task) answer
/// with a unary JSON-RPC error instead of an empty stream.
fn subscribe_task(state: &Arc<ServeState>, req: &JsonRpcRequest) -> Response {
    // A subscription's events are correlated by request `id`, so a notification
    // (no `id`) can't be served — reject it rather than stream `null`-id events.
    let Some(id) = req.id.clone() else {
        return rpc_error(
            serde_json::Value::Null,
            INVALID_REQUEST,
            "SubscribeToTask requires a request `id` (cannot stream to a notification)",
        );
    };
    let Some((team_id, tasks_path)) = state.team.as_ref() else {
        return rpc_error(
            id,
            TASK_NOT_FOUND,
            "no team task list is exposed (start with `--team <id>`)",
        );
    };
    let Some(task_id) = crate::rpc::task_id_param(req) else {
        return rpc_error(id, INVALID_PARAMS, "missing task `id`");
    };
    // Pre-flight: distinguish "task absent" (-32001) from a real read/parse
    // failure (-32603) — masking the latter as not-found would hide a server
    // fault, and diverge from the unary task handlers.
    match crate::tasks::load_team_tasks(tasks_path) {
        Ok(tasks) if tasks.iter().any(|t| t.id == task_id) => {}
        Ok(_) => {
            return rpc_error(
                id,
                TASK_NOT_FOUND,
                format!("task `{task_id}` not found in team `{team_id}`"),
            );
        }
        Err(e) => return rpc_error(id, INTERNAL_ERROR, format!("task list read failed: {e}")),
    }

    let team_id = team_id.clone();
    let tasks_path = tasks_path.clone();
    let stream = async_stream::stream! {
        let start = Instant::now();
        let mut last: Option<crate::types::TaskState> = None;
        loop {
            // Read the task file off the executor: this is a blocking syscall
            // run once per poll for the connection's whole lifetime, so doing it
            // inline would stall the (current-thread) runtime — the listener and
            // every other live stream — during each read.
            let path = tasks_path.clone();
            let load = tokio::task::spawn_blocking(move || crate::tasks::load_team_tasks(&path)).await;
            let tasks = match load {
                Ok(Ok(t)) => t,
                // A read/parse error (or join error) is a real fault — surface
                // it as an error event and close, rather than a misleading
                // "Completed" terminal state.
                _ => {
                    let data = error_event(&id, INTERNAL_ERROR, "task list read failed");
                    yield Ok::<_, Infallible>(Event::default().data(data));
                    break;
                }
            };
            let timed_out = start.elapsed() >= SUBSCRIBE_MAX;
            match tasks.iter().find(|t| t.id == task_id) {
                Some(t) => {
                    let cur = crate::tasks::a2a_state(t.state);
                    let terminal =
                        matches!(t.state, bwoc_core::team::TaskState::Completed) || timed_out;
                    if last != Some(cur) || terminal {
                        let data = status_update(&id, &task_id, &team_id, cur, terminal);
                        yield Ok::<_, Infallible>(Event::default().data(data));
                        last = Some(cur);
                        if terminal {
                            break;
                        }
                    }
                }
                // Task deleted mid-subscription: report it honestly as gone
                // (TaskNotFound) and close — not a fabricated "Completed".
                None => {
                    let data = error_event(&id, TASK_NOT_FOUND, "task no longer exists");
                    yield Ok::<_, Infallible>(Event::default().data(data));
                    break;
                }
            }
            tokio::time::sleep(SUBSCRIBE_POLL).await;
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Serialize a JSON-RPC **error** response to an SSE `data:` payload (used to
/// close a subscription on a fault rather than fake a terminal task state).
fn error_event(id: &serde_json::Value, code: i64, message: &str) -> String {
    serde_json::to_string(&JsonRpcResponse::err(id.clone(), code, message)).unwrap_or_default()
}

/// Serialize one `TaskStatusUpdateEvent`, wrapped as the `result` of a JSON-RPC
/// response (the A2A SSE event shape), to the SSE `data:` payload.
fn status_update(
    id: &serde_json::Value,
    task_id: &str,
    context_id: &str,
    state: crate::types::TaskState,
    is_final: bool,
) -> String {
    let result = serde_json::json!({
        "taskId": task_id,
        "contextId": context_id,
        "kind": "status-update",
        "status": { "state": state },
        "final": is_final,
    });
    serde_json::to_string(&JsonRpcResponse::ok(id.clone(), result)).unwrap_or_default()
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

    /// Collect the JSON payloads of every `data:` line in an SSE body.
    async fn sse_events(resp: Response) -> Vec<serde_json::Value> {
        let bytes = to_bytes(resp.into_body(), MAX_REQUEST_BYTES).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        body.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(|d| serde_json::from_str(d.trim()).unwrap())
            .collect()
    }

    /// `test_state_with_team`, but with `t1` already `Completed`.
    fn state_with_completed_task() -> (Arc<ServeState>, tempfile::TempDir) {
        use bwoc_core::team::{Task, TaskState, render_tasks};
        let (base, dir) = test_state();
        let tasks_path = dir.path().join("teams/team-security/tasks.jsonl");
        std::fs::create_dir_all(tasks_path.parent().unwrap()).unwrap();
        let mut t = Task::new("t1", "done", vec![]);
        t.state = TaskState::Completed;
        std::fs::write(&tasks_path, render_tasks(&[t]).unwrap()).unwrap();
        let state = Arc::new(ServeState {
            agent_id: base.agent_id.clone(),
            inbox_path: base.inbox_path.clone(),
            card: base.card.clone(),
            team: Some(("team-security".into(), tasks_path)),
        });
        (state, dir)
    }

    #[tokio::test]
    async fn send_streaming_message_delivers_then_emits_one_ack_event() {
        let (state, _d) = test_state();
        let inbox = state.inbox_path.clone();
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":method::SEND_STREAMING_MESSAGE,
                "params":{"message":{"role":"ROLE_USER","parts":[{"text":"stream hi"}],"messageId":"s1"}}
            })))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("text/event-stream")
        );
        let events = sse_events(resp).await;
        assert_eq!(events.len(), 1, "degenerate single-event stream");
        assert!(
            events[0]["result"]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .contains("delivered")
        );
        // …and the message really hit the inbox (same path as unary SendMessage).
        assert!(
            std::fs::read_to_string(&inbox)
                .unwrap()
                .contains("\"messageId\":\"s1\"")
        );
    }

    #[tokio::test]
    async fn subscribe_to_completed_task_emits_one_final_event() {
        let (state, _d) = state_with_completed_task();
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":7,"method":method::SUBSCRIBE_TO_TASK,"params":{"id":"t1"}
            })))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let events = sse_events(resp).await;
        assert_eq!(events.len(), 1);
        let ev = &events[0]["result"];
        assert_eq!(ev["taskId"], "t1");
        assert_eq!(ev["contextId"], "team-security");
        assert_eq!(ev["kind"], "status-update");
        assert_eq!(ev["status"]["state"], "TASK_STATE_COMPLETED");
        assert_eq!(ev["final"], true);
    }

    #[tokio::test]
    async fn streaming_send_notification_delivers_but_returns_204() {
        // No `id` ⇒ JSON-RPC notification: inbox the message, emit no SSE body.
        let (state, _d) = test_state();
        let inbox = state.inbox_path.clone();
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","method":method::SEND_STREAMING_MESSAGE,
                "params":{"message":{"role":"ROLE_USER","parts":[{"text":"notif"}],"messageId":"sn1"}}
            })))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(
            std::fs::read_to_string(&inbox)
                .unwrap()
                .contains("\"messageId\":\"sn1\"")
        );
    }

    #[tokio::test]
    async fn subscribe_notification_is_rejected_invalid_request() {
        let (state, _d) = test_state_with_team();
        let resp = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","method":method::SUBSCRIBE_TO_TASK,"params":{"id":"t1"}
            })))
            .await
            .unwrap();
        assert_eq!(body_json(resp).await["error"]["code"], INVALID_REQUEST);
    }

    #[tokio::test]
    async fn subscribe_errors_unary_when_task_missing_or_no_team() {
        // Unknown task in an exposed team → -32001 (unary JSON, not an SSE stream).
        let (state, _d) = test_state_with_team();
        let missing = app(state)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":method::SUBSCRIBE_TO_TASK,"params":{"id":"nope"}
            })))
            .await
            .unwrap();
        assert_eq!(body_json(missing).await["error"]["code"], TASK_NOT_FOUND);
        // No team exposed at all → also -32001.
        let (no_team, _d2) = test_state();
        let resp = app(no_team)
            .oneshot(post_json(serde_json::json!({
                "jsonrpc":"2.0","id":2,"method":method::SUBSCRIBE_TO_TASK,"params":{"id":"t1"}
            })))
            .await
            .unwrap();
        assert_eq!(body_json(resp).await["error"]["code"], TASK_NOT_FOUND);
    }

    #[tokio::test]
    async fn push_config_crud_round_trip() {
        let (state, dir) = test_state_with_team(); // task t1 exists
        // Create a push config for t1.
        let created = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":1,"method":method::CREATE_TASK_PUSH_CONFIG,
                    "params":{"taskId":"t1","pushNotificationConfig":{"url":"https://hook.example/a","token":"secret"}}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(created["result"]["taskId"], "t1");
        assert_eq!(
            created["result"]["pushNotificationConfig"]["url"],
            "https://hook.example/a"
        );
        // The registrant's token must NOT be echoed over the wire…
        assert!(
            created["result"]["pushNotificationConfig"]
                .get("token")
                .is_none()
        );
        // …but it IS persisted on disk for the (auth-phase) delivery path.
        let store =
            std::fs::read_to_string(dir.path().join("teams/team-security/push-configs.json"))
                .unwrap();
        assert!(store.contains("secret"));
        let cfg_id = created["result"]["pushNotificationConfig"]["id"]
            .as_str()
            .unwrap()
            .to_string();

        // List → contains it.
        let listed = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":2,"method":method::LIST_TASK_PUSH_CONFIGS,"params":{"taskId":"t1"}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(listed["result"]["configs"].as_array().unwrap().len(), 1);

        // Get by id → the config.
        let got = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":3,"method":method::GET_TASK_PUSH_CONFIG,
                    "params":{"pushNotificationConfigId":cfg_id}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(got["result"]["pushNotificationConfig"]["id"], cfg_id);

        // Delete → then Get is -32001.
        let deleted = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":4,"method":method::DELETE_TASK_PUSH_CONFIG,
                    "params":{"pushNotificationConfigId":cfg_id}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(deleted["result"]["deleted"], cfg_id);
        let gone = body_json(
            app(state)
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":5,"method":method::GET_TASK_PUSH_CONFIG,
                    "params":{"pushNotificationConfigId":cfg_id}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(gone["error"]["code"], -32001);
    }

    #[tokio::test]
    async fn push_create_rejects_unknown_task_and_no_team() {
        // Unknown task → -32001.
        let (state, _d) = test_state_with_team();
        let bad_task = body_json(
            app(state)
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":1,"method":method::CREATE_TASK_PUSH_CONFIG,
                    "params":{"taskId":"ghost","pushNotificationConfig":{"url":"https://h/x"}}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(bad_task["error"]["code"], -32001);
        // No team exposed → -32001.
        let (no_team, _d2) = test_state();
        let resp = body_json(
            app(no_team)
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":2,"method":method::CREATE_TASK_PUSH_CONFIG,
                    "params":{"taskId":"t1","pushNotificationConfig":{"url":"https://h/x"}}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(resp["error"]["code"], -32001);
    }

    #[tokio::test]
    async fn push_config_error_branches() {
        let (state, _d) = test_state_with_team();
        // Create missing url → -32602.
        let no_url = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":1,"method":method::CREATE_TASK_PUSH_CONFIG,
                    "params":{"taskId":"t1","pushNotificationConfig":{}}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(no_url["error"]["code"], -32602);
        // Delete a non-existent config → -32001.
        let del = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":2,"method":method::DELETE_TASK_PUSH_CONFIG,
                    "params":{"pushNotificationConfigId":"nope"}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(del["error"]["code"], -32001);
        // Get with the wrong taskId → not found, even if the config id exists.
        let created = body_json(
            app(state.clone())
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":3,"method":method::CREATE_TASK_PUSH_CONFIG,
                    "params":{"taskId":"t1","pushNotificationConfig":{"url":"https://h/x"}}
                })))
                .await
                .unwrap(),
        )
        .await;
        let cfg_id = created["result"]["pushNotificationConfig"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let wrong_task = body_json(
            app(state)
                .oneshot(post_json(serde_json::json!({
                    "jsonrpc":"2.0","id":4,"method":method::GET_TASK_PUSH_CONFIG,
                    "params":{"taskId":"WRONG","pushNotificationConfigId":cfg_id}
                })))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(wrong_task["error"]["code"], -32001);
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
