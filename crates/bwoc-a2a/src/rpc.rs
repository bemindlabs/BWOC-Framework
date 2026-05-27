//! A2A JSON-RPC dispatch (1.0.0). P1 handles `SendMessage` by dropping the
//! inbound message into the recipient agent's BWOC `inbox.jsonl`; the other
//! task methods land in P2–P5. Transport-agnostic + testable: the HTTP
//! (axum) listener calls [`dispatch`] with the parsed request.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::types::{JsonRpcRequest, JsonRpcResponse, Message, method};

/// JSON-RPC standard error codes used here.
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

/// Context the dispatcher needs to serve one local agent over A2A.
pub struct ServeContext<'a> {
    /// The local agent this server represents (becomes the envelope `to`).
    pub agent_id: &'a str,
    /// Path to that agent's `inbox.jsonl`.
    pub inbox_path: &'a Path,
}

/// Dispatch a single A2A JSON-RPC request. Returns `None` for a **notification**
/// (a request with no `id`): per JSON-RPC 2.0 the server emits no reply, though
/// the side effect (e.g. the inbox write) still runs. Unknown methods return a
/// `method not found` error (the task methods are wired in P2–P5).
pub fn dispatch(req: &JsonRpcRequest, ctx: &ServeContext) -> Option<JsonRpcResponse> {
    let resp = handle(req, ctx);
    // Suppress the response for notifications; the work above already happened.
    req.id.as_ref().map(|_| resp)
}

fn handle(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    match req.method.as_str() {
        method::SEND_MESSAGE => handle_send_message(req, ctx),
        method::GET_TASK
        | method::LIST_TASKS
        | method::CANCEL_TASK
        | method::SEND_STREAMING_MESSAGE
        | method::SUBSCRIBE_TO_TASK => JsonRpcResponse::err(
            resolved_id(req),
            METHOD_NOT_FOUND,
            format!(
                "`{}` is not implemented yet (lands in a later #48 phase)",
                req.method
            ),
        ),
        other => JsonRpcResponse::err(
            resolved_id(req),
            METHOD_NOT_FOUND,
            format!("unknown A2A method `{other}`"),
        ),
    }
}

/// The id to echo back on a response. A notification's reply is dropped by
/// [`dispatch`], so the `Null` fallback only ever surfaces for an explicit
/// `"id": null`.
fn resolved_id(req: &JsonRpcRequest) -> serde_json::Value {
    req.id.clone().unwrap_or(serde_json::Value::Null)
}

/// `SendMessage` → append a BWOC envelope to the recipient's inbox.
///
/// The A2A message's text Parts become the envelope `message`; non-text parts
/// are noted (v1 text-only limit, surfaced — not silently dropped). Returns a
/// `Message` ack (role=agent) so the caller knows it was delivered.
fn handle_send_message(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let message: Message =
        match serde_json::from_value(req.params.get("message").cloned().unwrap_or_default()) {
            Ok(m) => m,
            Err(e) => {
                return JsonRpcResponse::err(
                    resolved_id(req),
                    INVALID_PARAMS,
                    format!("invalid `message`: {e}"),
                );
            }
        };

    let mut body = message.text_body();
    if message.has_non_text_parts() {
        // Honest about the v1 limit rather than dropping content silently.
        body.push_str("\n[a2a: non-text parts omitted — v1 handles text only]");
    }

    let ts = bwoc_core::time::utc_now_iso8601();
    let envelope = serde_json::json!({
        "ts": ts,
        "messageId": message.message_id,
        "from": "a2a",
        "to": ctx.agent_id,
        "message": body,
        "kind": "a2a",
    });

    if let Err(e) = append_line(ctx.inbox_path, &envelope.to_string()) {
        return JsonRpcResponse::err(
            resolved_id(req),
            INTERNAL_ERROR,
            format!("inbox write failed: {e}"),
        );
    }

    // Minimal A2A ack: a Message from the agent confirming receipt.
    let ack = serde_json::json!({
        "role": "ROLE_AGENT",
        "parts": [{ "text": format!("delivered to {} inbox", ctx.agent_id) }],
        "messageId": format!("ack-{}", message.message_id),
        "contextId": message.context_id,
    });
    JsonRpcResponse::ok(resolved_id(req), ack)
}

// NOTE (track for the network-exposed phase, P1-serve/P4): this append is
// uncapped. Once an HTTP listener accepts remote A2A peers, add a per-peer
// rate/size limit so an unauthenticated peer can't grow `inbox.jsonl`
// unboundedly. No listener is wired in P1, so it isn't reachable yet.
fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::JsonRpcRequest;

    fn req(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0", "method": method, "params": params, "id": 1
        }))
        .unwrap()
    }

    /// A notification: same shape as [`req`] but with no `id` field.
    fn notification(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0", "method": method, "params": params
        }))
        .unwrap()
    }

    #[test]
    fn send_message_appends_envelope_and_acks() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join(".bwoc/inbox.jsonl");
        let ctx = ServeContext {
            agent_id: "agent-me",
            inbox_path: &inbox,
        };
        let resp = dispatch(
            &req(
                method::SEND_MESSAGE,
                serde_json::json!({"message":{"role":"ROLE_USER","parts":[{"text":"review my design"}],"messageId":"m1"}}),
            ),
            &ctx,
        )
        .expect("a request with an id gets a response");
        assert!(resp.error.is_none(), "ok response");
        // Inbox got a BWOC envelope with the text body + a2a markers.
        let line = std::fs::read_to_string(&inbox).unwrap();
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["to"], "agent-me");
        assert_eq!(v["from"], "a2a");
        assert_eq!(v["kind"], "a2a");
        assert_eq!(v["message"], "review my design");
        assert_eq!(v["messageId"], "m1");
    }

    #[test]
    fn non_text_parts_noted_not_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox.jsonl");
        let ctx = ServeContext {
            agent_id: "a",
            inbox_path: &inbox,
        };
        dispatch(
            &req(
                method::SEND_MESSAGE,
                serde_json::json!({"message":{"role":"ROLE_USER","parts":[{"text":"hi"},{"url":"http://x/y.bin"}],"messageId":"m2"}}),
            ),
            &ctx,
        );
        let line = std::fs::read_to_string(&inbox).unwrap();
        assert!(line.contains("non-text parts omitted"));
    }

    #[test]
    fn unimplemented_and_unknown_methods_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ServeContext {
            agent_id: "a",
            inbox_path: &dir.path().join("i.jsonl"),
        };
        for m in [method::GET_TASK, "Frobnicate"] {
            let r = dispatch(&req(m, serde_json::json!({})), &ctx)
                .expect("a request with an id gets a response");
            assert_eq!(r.error.as_ref().unwrap().code, METHOD_NOT_FOUND);
        }
    }

    #[test]
    fn notification_runs_side_effect_but_emits_no_response() {
        // A request with no `id` is a JSON-RPC notification: per spec the server
        // must not reply, but the inbox write (the side effect) still happens.
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox.jsonl");
        let ctx = ServeContext {
            agent_id: "a",
            inbox_path: &inbox,
        };
        let resp = dispatch(
            &notification(
                method::SEND_MESSAGE,
                serde_json::json!({"message":{"role":"ROLE_USER","parts":[{"text":"hi"}],"messageId":"n1"}}),
            ),
            &ctx,
        );
        assert!(resp.is_none(), "notifications get no response");
        // …yet the message was still delivered.
        let line = std::fs::read_to_string(&inbox).unwrap();
        assert!(line.contains("\"messageId\":\"n1\""));
    }

    #[test]
    fn bad_params_returns_invalid_params() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ServeContext {
            agent_id: "a",
            inbox_path: &dir.path().join("i.jsonl"),
        };
        // message missing required fields → invalid params.
        let r = dispatch(
            &req(
                method::SEND_MESSAGE,
                serde_json::json!({"message":{"role":"ROLE_USER"}}),
            ),
            &ctx,
        )
        .expect("a request with an id gets a response");
        assert_eq!(r.error.as_ref().unwrap().code, INVALID_PARAMS);
    }
}
