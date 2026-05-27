//! A2A JSON-RPC dispatch (1.0.0). `SendMessage` (P1) drops the inbound message
//! into the recipient agent's BWOC `inbox.jsonl`; `GetTask`/`ListTasks` (P2)
//! bridge a team's Saṅgha task list and `CancelTask` reports it isn't
//! A2A-cancelable; streaming (`SendStreamingMessage`/`SubscribeToTask`) lands in
//! P3. Transport-agnostic + testable: the HTTP (axum) listener calls
//! [`dispatch`] with the parsed request.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::types::{JsonRpcRequest, JsonRpcResponse, Message, method};

/// JSON-RPC standard error codes used here.
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
/// A2A-specific error codes (spec §"A2A-Specific Errors").
const TASK_NOT_FOUND: i64 = -32001;
const TASK_NOT_CANCELABLE: i64 = -32002;

/// The team task list an A2A server exposes via `tasks/*` (P2). Set with
/// `bwoc a2a serve --team <id>`; absent when no team is selected.
pub struct TasksContext<'a> {
    /// Team id — becomes the A2A `contextId` of each task.
    pub team_id: &'a str,
    /// Path to that team's `tasks.jsonl`.
    pub tasks_path: &'a Path,
}

/// Context the dispatcher needs to serve one local agent over A2A.
pub struct ServeContext<'a> {
    /// The local agent this server represents (becomes the envelope `to`).
    pub agent_id: &'a str,
    /// Path to that agent's `inbox.jsonl`.
    pub inbox_path: &'a Path,
    /// The team task list to expose over `tasks/*`, if one was selected.
    pub tasks: Option<TasksContext<'a>>,
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
    // JSON-RPC 2.0 requires `jsonrpc` to be exactly "2.0".
    if req.jsonrpc != "2.0" {
        return JsonRpcResponse::err(
            resolved_id(req),
            INVALID_REQUEST,
            format!(
                "unsupported jsonrpc version `{}` (must be \"2.0\")",
                req.jsonrpc
            ),
        );
    }
    match req.method.as_str() {
        method::SEND_MESSAGE => handle_send_message(req, ctx),
        method::GET_TASK => handle_get_task(req, ctx),
        method::LIST_TASKS => handle_list_tasks(req, ctx),
        method::CANCEL_TASK => handle_cancel_task(req, ctx),
        method::CREATE_TASK_PUSH_CONFIG => handle_create_push_config(req, ctx),
        method::GET_TASK_PUSH_CONFIG => handle_get_push_config(req, ctx),
        method::LIST_TASK_PUSH_CONFIGS => handle_list_push_configs(req, ctx),
        method::DELETE_TASK_PUSH_CONFIG => handle_delete_push_config(req, ctx),
        method::SEND_STREAMING_MESSAGE | method::SUBSCRIBE_TO_TASK => JsonRpcResponse::err(
            resolved_id(req),
            METHOD_NOT_FOUND,
            format!(
                "`{}` is a streaming method — call it over the SSE transport, \
                 not unary JSON-RPC (#48 P3)",
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

/// Pull the task id out of a `tasks/*` request's params, accepting the field
/// aliases the proto/JSON-RPC bindings use (`id`, `taskId`, `name`).
pub(crate) fn task_id_param(req: &JsonRpcRequest) -> Option<String> {
    for key in ["id", "taskId", "name"] {
        if let Some(v) = req.params.get(key).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    None
}

/// `GetTask` → the matching team task as an A2A `Task`, or `TaskNotFound`.
fn handle_get_task(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let Some(id) = task_id_param(req) else {
        return JsonRpcResponse::err(resolved_id(req), INVALID_PARAMS, "missing task `id`");
    };
    let Some(tasks) = &ctx.tasks else {
        return JsonRpcResponse::err(
            resolved_id(req),
            TASK_NOT_FOUND,
            format!("task `{id}` not found (this server exposes no team task list)"),
        );
    };
    let team_tasks = match crate::tasks::load_team_tasks(tasks.tasks_path) {
        Ok(t) => t,
        Err(e) => {
            return JsonRpcResponse::err(
                resolved_id(req),
                INTERNAL_ERROR,
                format!("task list read failed: {e}"),
            );
        }
    };
    match team_tasks.iter().find(|t| t.id == id) {
        Some(t) => JsonRpcResponse::ok(
            resolved_id(req),
            serde_json::to_value(crate::tasks::to_a2a_task(t, tasks.team_id))
                .unwrap_or(serde_json::Value::Null),
        ),
        None => JsonRpcResponse::err(
            resolved_id(req),
            TASK_NOT_FOUND,
            format!("task `{id}` not found in team `{}`", tasks.team_id),
        ),
    }
}

/// `ListTasks` → `{ "tasks": [...] }` for the exposed team (empty if none).
fn handle_list_tasks(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let mapped: Vec<_> = match &ctx.tasks {
        None => Vec::new(),
        Some(tasks) => match crate::tasks::load_team_tasks(tasks.tasks_path) {
            Ok(team_tasks) => team_tasks
                .iter()
                .map(|t| crate::tasks::to_a2a_task(t, tasks.team_id))
                .collect(),
            Err(e) => {
                return JsonRpcResponse::err(
                    resolved_id(req),
                    INTERNAL_ERROR,
                    format!("task list read failed: {e}"),
                );
            }
        },
    };
    JsonRpcResponse::ok(resolved_id(req), serde_json::json!({ "tasks": mapped }))
}

/// `CancelTask` → BWOC tasks aren't A2A-cancelable; the human lead owns the
/// lifecycle. Report `TaskNotCancelable` rather than faking a cancel.
fn handle_cancel_task(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let Some(id) = task_id_param(req) else {
        return JsonRpcResponse::err(resolved_id(req), INVALID_PARAMS, "missing task `id`");
    };
    // If the task genuinely doesn't exist, TaskNotFound is the more precise
    // answer; otherwise it exists but can't be canceled over A2A.
    if let Some(tasks) = &ctx.tasks {
        if let Ok(team_tasks) = crate::tasks::load_team_tasks(tasks.tasks_path) {
            if !team_tasks.iter().any(|t| t.id == id) {
                return JsonRpcResponse::err(
                    resolved_id(req),
                    TASK_NOT_FOUND,
                    format!("task `{id}` not found in team `{}`", tasks.team_id),
                );
            }
        }
    }
    JsonRpcResponse::err(
        resolved_id(req),
        TASK_NOT_CANCELABLE,
        "BWOC tasks cannot be canceled over A2A — the team lead manages task \
         lifecycle (`bwoc task`)",
    )
}

// ── Push notification config management (P5) ────────────────────────────────
//
// CRUD only — delivery (POSTing task updates to the webhook) is deferred to the
// auth phase, since it's an SSRF/exfil egress under P1's no-auth posture. See
// `crate::push`.

/// The push-config id from a `*PushNotificationConfig` request's params.
fn config_id_param(req: &JsonRpcRequest) -> Option<String> {
    for key in ["pushNotificationConfigId", "configId", "id"] {
        if let Some(v) = req.params.get(key).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    None
}

/// `{ "taskId", "pushNotificationConfig": { "id", "url" } }` — the A2A
/// `TaskPushNotificationConfig` shape. The registrant's `token` is intentionally
/// **not** echoed: it's a stored secret, and under P1's no-auth listener any
/// local caller could `List` and read tokens another caller registered. It's
/// persisted for the (auth-phase) delivery path, never returned over the wire.
fn push_config_json(c: &crate::push::PushConfig) -> serde_json::Value {
    serde_json::json!({
        "taskId": c.task_id,
        "pushNotificationConfig": { "id": c.config_id, "url": c.url },
    })
}

/// Resolve the team's push-configs path, or an error response if no team task
/// list is exposed (push configs are task-scoped, and tasks need a team).
fn push_path<'a>(
    req: &JsonRpcRequest,
    ctx: &'a ServeContext,
) -> Result<(&'a TasksContext<'a>, std::path::PathBuf), JsonRpcResponse> {
    match &ctx.tasks {
        Some(tasks) => Ok((tasks, crate::push::configs_path(tasks.tasks_path))),
        None => Err(JsonRpcResponse::err(
            resolved_id(req),
            TASK_NOT_FOUND,
            "no team task list is exposed (start with `--team <id>`)",
        )),
    }
}

fn load_configs_or_err(
    req: &JsonRpcRequest,
    path: &std::path::Path,
) -> Result<Vec<crate::push::PushConfig>, JsonRpcResponse> {
    crate::push::load(path).map_err(|e| {
        JsonRpcResponse::err(
            resolved_id(req),
            INTERNAL_ERROR,
            format!("push config: {e}"),
        )
    })
}

/// `CreateTaskPushNotificationConfig` — register a webhook config for a task.
fn handle_create_push_config(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let (tasks, path) = match push_path(req, ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(task_id) = task_id_param(req) else {
        return JsonRpcResponse::err(resolved_id(req), INVALID_PARAMS, "missing `taskId`");
    };
    // The task must exist in the team list (consistent with GetTask).
    let team_tasks = match crate::tasks::load_team_tasks(tasks.tasks_path) {
        Ok(t) => t,
        Err(e) => {
            return JsonRpcResponse::err(
                resolved_id(req),
                INTERNAL_ERROR,
                format!("task list read failed: {e}"),
            );
        }
    };
    if !team_tasks.iter().any(|t| t.id == task_id) {
        return JsonRpcResponse::err(
            resolved_id(req),
            TASK_NOT_FOUND,
            format!("task `{task_id}` not found in team `{}`", tasks.team_id),
        );
    }
    // Pull the config out of `pushNotificationConfig` (or `config`).
    let cfg = req
        .params
        .get("pushNotificationConfig")
        .or_else(|| req.params.get("config"));
    let Some(url) = cfg.and_then(|c| c.get("url")).and_then(|u| u.as_str()) else {
        return JsonRpcResponse::err(
            resolved_id(req),
            INVALID_PARAMS,
            "missing `pushNotificationConfig.url`",
        );
    };
    let token = cfg
        .and_then(|c| c.get("token"))
        .and_then(|t| t.as_str())
        .map(str::to_string);
    // nanos + a per-process counter — unique even for two creates in the same
    // clock tick (the counter), and across restarts (the nanos).
    static PUSH_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let config_id = format!(
        "pnc-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        PUSH_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let new = crate::push::PushConfig {
        task_id,
        config_id,
        url: url.to_string(),
        token,
    };
    let mut configs = match load_configs_or_err(req, &path) {
        Ok(c) => c,
        Err(e) => return e,
    };
    configs.push(new.clone());
    if let Err(e) = crate::push::save(&path, &configs) {
        return JsonRpcResponse::err(
            resolved_id(req),
            INTERNAL_ERROR,
            format!("push config write failed: {e}"),
        );
    }
    JsonRpcResponse::ok(resolved_id(req), push_config_json(&new))
}

/// `GetTaskPushNotificationConfig` — fetch one config by (taskId, configId).
fn handle_get_push_config(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let (_tasks, path) = match push_path(req, ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(config_id) = config_id_param(req) else {
        return JsonRpcResponse::err(
            resolved_id(req),
            INVALID_PARAMS,
            "missing `pushNotificationConfigId`",
        );
    };
    // A2A keys a config by (taskId, configId). If a taskId is given, require the
    // config to belong to it, so a caller can't read a config under a task they
    // didn't name.
    let task_id = task_id_param(req);
    let configs = match load_configs_or_err(req, &path) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match configs
        .iter()
        .find(|c| c.config_id == config_id && task_id.as_deref().is_none_or(|t| c.task_id == t))
    {
        Some(c) => JsonRpcResponse::ok(resolved_id(req), push_config_json(c)),
        None => JsonRpcResponse::err(
            resolved_id(req),
            TASK_NOT_FOUND,
            format!("push config `{config_id}` not found"),
        ),
    }
}

/// `ListTaskPushNotificationConfigs` — all configs for a task.
fn handle_list_push_configs(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let (_tasks, path) = match push_path(req, ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(task_id) = task_id_param(req) else {
        return JsonRpcResponse::err(resolved_id(req), INVALID_PARAMS, "missing `taskId`");
    };
    let configs = match load_configs_or_err(req, &path) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mapped: Vec<_> = configs
        .iter()
        .filter(|c| c.task_id == task_id)
        .map(push_config_json)
        .collect();
    JsonRpcResponse::ok(resolved_id(req), serde_json::json!({ "configs": mapped }))
}

/// `DeleteTaskPushNotificationConfig` — remove one config by id.
fn handle_delete_push_config(req: &JsonRpcRequest, ctx: &ServeContext) -> JsonRpcResponse {
    let (_tasks, path) = match push_path(req, ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(config_id) = config_id_param(req) else {
        return JsonRpcResponse::err(
            resolved_id(req),
            INVALID_PARAMS,
            "missing `pushNotificationConfigId`",
        );
    };
    // Same (taskId, configId) keying as Get: with a taskId given, only a config
    // belonging to it is removed.
    let task_id = task_id_param(req);
    let mut configs = match load_configs_or_err(req, &path) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let before = configs.len();
    configs.retain(|c| {
        !(c.config_id == config_id && task_id.as_deref().is_none_or(|t| c.task_id == t))
    });
    if configs.len() == before {
        return JsonRpcResponse::err(
            resolved_id(req),
            TASK_NOT_FOUND,
            format!("push config `{config_id}` not found"),
        );
    }
    if let Err(e) = crate::push::save(&path, &configs) {
        return JsonRpcResponse::err(
            resolved_id(req),
            INTERNAL_ERROR,
            format!("push config write failed: {e}"),
        );
    }
    JsonRpcResponse::ok(
        resolved_id(req),
        serde_json::json!({ "deleted": config_id }),
    )
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

/// Cap on total `inbox.jsonl` size. Now that the P1-serve listener can accept
/// inbound A2A messages, an append is refused once the inbox reaches this size
/// so a peer cannot grow it without bound. Generous — a runaway guard, not a
/// quota. (Per-peer *rate* limiting waits for the auth phase: P1 has no peer
/// identity, so every inbound message is `from:"a2a"` and can't be attributed.)
const MAX_INBOX_BYTES: u64 = 64 << 20; // 64 MiB

fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    append_line_capped(path, line, MAX_INBOX_BYTES)
}

fn append_line_capped(path: &Path, line: &str, cap: u64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if let Ok(meta) = std::fs::metadata(path) {
        // Reject when the *projected* post-append size (line + newline) would
        // exceed the cap, so the cap is a real ceiling rather than one that can
        // be overshot by an admitted final write.
        let projected = meta.len().saturating_add(line.len() as u64 + 1);
        if projected > cap {
            return Err(std::io::Error::other(format!(
                "inbox full: {} + {} bytes would exceed {cap} byte cap",
                meta.len(),
                line.len() + 1
            )));
        }
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
            tasks: None,
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
            tasks: None,
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
            tasks: None,
        };
        // Streaming (P3) is not implemented yet; an unknown method is unknown.
        for m in [
            method::SEND_STREAMING_MESSAGE,
            method::SUBSCRIBE_TO_TASK,
            "Frobnicate",
        ] {
            let r = dispatch(&req(m, serde_json::json!({})), &ctx)
                .expect("a request with an id gets a response");
            assert_eq!(r.error.as_ref().unwrap().code, METHOD_NOT_FOUND);
        }
    }

    #[test]
    fn task_methods_without_a_team_are_not_found_or_not_cancelable() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ServeContext {
            agent_id: "a",
            inbox_path: &dir.path().join("i.jsonl"),
            tasks: None,
        };
        // GetTask with no team list → TaskNotFound (-32001).
        let g = dispatch(
            &req(method::GET_TASK, serde_json::json!({"id": "t1"})),
            &ctx,
        )
        .unwrap();
        assert_eq!(g.error.as_ref().unwrap().code, TASK_NOT_FOUND);
        // ListTasks → empty result, not an error.
        let l = dispatch(&req(method::LIST_TASKS, serde_json::json!({})), &ctx).unwrap();
        assert_eq!(l.result.unwrap()["tasks"].as_array().unwrap().len(), 0);
        // CancelTask → TaskNotCancelable (-32002).
        let c = dispatch(
            &req(method::CANCEL_TASK, serde_json::json!({"id": "t1"})),
            &ctx,
        )
        .unwrap();
        assert_eq!(c.error.as_ref().unwrap().code, TASK_NOT_CANCELABLE);
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
            tasks: None,
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
            tasks: None,
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

    #[test]
    fn wrong_jsonrpc_version_is_invalid_request() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ServeContext {
            agent_id: "a",
            inbox_path: &dir.path().join("i.jsonl"),
            tasks: None,
        };
        let bad: JsonRpcRequest = serde_json::from_value(serde_json::json!({
            "jsonrpc": "1.0", "method": method::SEND_MESSAGE, "params": {}, "id": 1
        }))
        .unwrap();
        let r = dispatch(&bad, &ctx).expect("a request with an id gets a response");
        assert_eq!(r.error.as_ref().unwrap().code, INVALID_REQUEST);
    }

    #[test]
    fn inbox_cap_refuses_append_once_full() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("inbox.jsonl");
        std::fs::write(&p, "12345678").unwrap(); // 8 bytes, over the tiny cap
        let err = append_line_capped(&p, "more", 4).unwrap_err();
        assert!(err.to_string().contains("inbox full"));
        // Under-cap append still works.
        append_line_capped(&dir.path().join("fresh.jsonl"), "ok", 4).unwrap();
    }
}
