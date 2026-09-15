//! Session-scoped tools for `--chat`: `todo` and `subagent`.
//!
//! Both hold per-session state (the todo list; the provider handle), so the
//! chat driver registers them and runs them in-process. They are not part of
//! `default_registry`, so they are never marshalled into the turn-executor child.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use super::extra_tools::{Glob, Grep};
use super::impls::{ListDir, ReadFile};
use super::registry::dispatch;
use super::{ToolContext, ToolImpl, ToolRegistry};
use crate::error::HarnessError;
use crate::provider::{ChatMessage, ProviderClient};

fn err(tool: &str, reason: impl Into<String>) -> HarnessError {
    HarnessError::ToolExecution {
        tool: tool.to_string(),
        reason: reason.into(),
    }
}

// ---------------------------------------------------------------------------
// todo — an in-memory task list for the session
// ---------------------------------------------------------------------------

/// The session's todo list. Lives as long as the chat process; never written
/// to disk (and not cleared by `forget`).
#[derive(Default)]
pub struct Todo {
    items: Mutex<Vec<(String, &'static str)>>,
}

const TODO_MAX_ITEMS: usize = 100;
const TODO_STATUSES: [&str; 3] = ["pending", "in_progress", "completed"];

#[async_trait]
impl ToolImpl for Todo {
    fn name(&self) -> &'static str {
        "todo"
    }

    fn description(&self) -> &'static str {
        "Keep a task list for this session (held in memory, not saved to the \
         project). `action: \"write\"` replaces the whole list with `todos`, each \
         `{content, status}` where status is `pending`, `in_progress` or \
         `completed`; `action: \"read\"` returns the list. Use it to plan multi-step \
         work and mark progress as you go."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["read", "write"] },
                "todos": {
                    "type": "array",
                    "description": "The full new list (for `write`).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": { "type": "string", "enum": TODO_STATUSES }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<String, HarnessError> {
        let mut items = self.items.lock().unwrap_or_else(|p| p.into_inner());
        match args["action"].as_str() {
            Some("read") => {}
            Some("write") => {
                let todos = args["todos"]
                    .as_array()
                    .ok_or_else(|| err("todo", "`write` needs a `todos` array"))?;
                if todos.len() > TODO_MAX_ITEMS {
                    return Err(err(
                        "todo",
                        format!("too many todos ({}; max {TODO_MAX_ITEMS})", todos.len()),
                    ));
                }
                let mut next = Vec::with_capacity(todos.len());
                for (i, t) in todos.iter().enumerate() {
                    let content = t["content"].as_str().map(str::trim).unwrap_or("");
                    let status = t["status"].as_str().unwrap_or("");
                    let Some(status) = TODO_STATUSES.iter().find(|s| **s == status) else {
                        return Err(err(
                            "todo",
                            format!(
                                "todo #{}: status must be pending, in_progress or completed",
                                i + 1
                            ),
                        ));
                    };
                    if content.is_empty() {
                        return Err(err("todo", format!("todo #{}: empty `content`", i + 1)));
                    }
                    next.push((content.to_string(), *status));
                }
                *items = next;
            }
            _ => return Err(err("todo", "`action` must be `read` or `write`")),
        }
        Ok(render_todos(&items))
    }
}

fn render_todos(items: &[(String, &'static str)]) -> String {
    if items.is_empty() {
        return "todo list is empty".to_string();
    }
    let done = items.iter().filter(|(_, s)| *s == "completed").count();
    let mut out = format!("todos ({done}/{} completed):", items.len());
    for (content, status) in items {
        let mark = match *status {
            "completed" => "[x]",
            "in_progress" => "[~]",
            _ => "[ ]",
        };
        out.push_str(&format!("\n{mark} {content}"));
    }
    out
}

// ---------------------------------------------------------------------------
// subagent — a read-only child session that returns its final answer
// ---------------------------------------------------------------------------

/// Runs a prompt to completion in a fresh context with the session's provider
/// and model, using only read-only tools, and returns the final text.
///
/// Limits (deliberately minimal): depth 1 — the child registry has no
/// `subagent`, so a child cannot spawn another; at most
/// [`SUBAGENT_MAX_STEPS`] provider calls; non-streaming; the child's tool calls
/// are not shown to the frontend and its token usage is not added to the
/// session's `TurnEnd` totals. Child tool calls pass the same guardrail check
/// and the same confined `ToolContext` as the parent's.
pub struct Subagent {
    provider: Arc<dyn ProviderClient>,
    model: String,
    /// The parent session's permission policy. A child cannot prompt the
    /// operator, so any inner call the policy does not `allow` outright is
    /// refused rather than run.
    policy: Option<crate::policy::permission::Policy>,
}

/// Provider calls one subagent run may make before it is stopped.
pub const SUBAGENT_MAX_STEPS: usize = 15;

const SUBAGENT_PROMPT: &str = "You are a subagent doing a research task for another \
agent. You can only read: list, search and read files in the working directory. You \
cannot change files or run commands. Work through the task, then reply with a concise \
final answer the calling agent can use directly, citing file paths and line numbers \
where they matter.";

impl Subagent {
    pub fn new(provider: Arc<dyn ProviderClient>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            policy: None,
        }
    }

    /// Bind the parent session's permission policy (see [`Subagent::policy`]).
    pub fn with_policy(mut self, policy: crate::policy::permission::Policy) -> Self {
        self.policy = Some(policy);
        self
    }
}

/// Why the parent's policy refuses an inner subagent call, or `None` when it
/// allows it. `ask` counts as a refusal: nobody can answer a prompt here.
fn policy_refusal(
    policy: Option<&crate::policy::permission::Policy>,
    tool: &str,
    arguments: &str,
) -> Option<String> {
    use crate::policy::permission::{Mode, resolve_effective_mode};
    let mode = resolve_effective_mode(policy?, tool, arguments);
    (mode != Mode::Allow).then(|| {
        format!(
            "DENIED by policy: `{tool}` is `{mode}` for this session, and a subagent \
             cannot ask the operator"
        )
    })
}

/// The child's tool set: read-only, and without `subagent` (depth limit 1).
pub fn subagent_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(ReadFile);
    reg.register(ListDir);
    reg.register(Grep);
    reg.register(Glob);
    reg
}

#[async_trait]
impl ToolImpl for Subagent {
    fn name(&self) -> &'static str {
        "subagent"
    }

    fn description(&self) -> &'static str {
        "Delegate a self-contained research task to a subagent with a fresh \
         context. It uses the same model, can only read (read_file, list_dir, grep, \
         glob), makes at most 15 model calls and returns its final answer as text. \
         Use it to explore broadly without filling your own context. Give it \
         everything it needs in `prompt`; it cannot see this conversation and \
         cannot start subagents of its own."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The complete task for the subagent."
                }
            },
            "required": ["prompt"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let prompt = args["prompt"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| err("subagent", "missing `prompt` argument"))?;

        let registry = subagent_registry();
        let tools = registry.tool_schemas();
        let mut messages = vec![
            ChatMessage::system(SUBAGENT_PROMPT),
            ChatMessage::user(prompt),
        ];

        for _ in 0..SUBAGENT_MAX_STEPS {
            let completion = self
                .provider
                .complete(messages.clone(), tools.clone(), &self.model)
                .await
                .map_err(|e| err("subagent", format!("provider call failed: {e}")))?;
            let message = completion
                .choices
                .into_iter()
                .next()
                .map(|c| c.message)
                .ok_or_else(|| err("subagent", "provider returned no choices"))?;
            let calls = message.tool_calls.clone().unwrap_or_default();
            if calls.is_empty() {
                let text = message.content.unwrap_or_default();
                if text.trim().is_empty() {
                    return Err(err("subagent", "subagent ended without an answer"));
                }
                return Ok(text);
            }
            messages.push(message);
            for call in &calls {
                let (name, arguments) = (&call.function.name, &call.function.arguments);
                let result = match crate::policy::guardrail_check(name, arguments, &ctx.workdir) {
                    Err(v) => format!("BLOCKED by safety guardrail [{}]: {}", v.rule, v.reason),
                    Ok(()) => match policy_refusal(self.policy.as_ref(), name, arguments) {
                        Some(refusal) => refusal,
                        None => dispatch(&registry, name, arguments, ctx).await,
                    },
                };
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    name.clone(),
                    result,
                ));
            }
        }
        Err(err(
            "subagent",
            format!("subagent did not finish within {SUBAGENT_MAX_STEPS} model calls"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::types::FunctionCall;
    use crate::provider::{ChatCompletion, Choice, FinishReason, StreamChunk, Tool, ToolCall};
    use futures_util::Stream;
    use std::pin::Pin;
    use tempfile::TempDir;

    // ── todo ──────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn todo_write_then_read() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::new(tmp.path());
        let todo = Todo::default();
        let empty = todo.execute(json!({"action": "read"}), &ctx).await.unwrap();
        assert_eq!(empty, "todo list is empty");
        let out = todo
            .execute(
                json!({"action": "write", "todos": [
                    {"content": "read the code", "status": "completed"},
                    {"content": "fix the bug", "status": "in_progress"},
                    {"content": "run tests", "status": "pending"}
                ]}),
                &ctx,
            )
            .await
            .unwrap();
        let expected = "todos (1/3 completed):\n[x] read the code\n[~] fix the bug\n[ ] run tests";
        assert_eq!(out, expected);
        assert_eq!(
            todo.execute(json!({"action": "read"}), &ctx).await.unwrap(),
            expected
        );
        // Nothing lands in the project.
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
        // A separate instance (another session) has its own list.
        let other = Todo::default();
        assert_eq!(
            other
                .execute(json!({"action": "read"}), &ctx)
                .await
                .unwrap(),
            "todo list is empty"
        );
    }

    #[tokio::test]
    async fn todo_rejects_bad_input_and_keeps_the_list() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let todo = Todo::default();
        todo.execute(
            json!({"action": "write", "todos": [{"content": "a", "status": "pending"}]}),
            &ctx,
        )
        .await
        .unwrap();
        for bad in [
            json!({"action": "write", "todos": [{"content": "b", "status": "done"}]}),
            json!({"action": "write", "todos": [{"content": " ", "status": "pending"}]}),
            json!({"action": "write"}),
            json!({"action": "delete"}),
        ] {
            assert!(todo.execute(bad, &ctx).await.is_err());
        }
        assert_eq!(
            todo.execute(json!({"action": "read"}), &ctx).await.unwrap(),
            "todos (0/1 completed):\n[ ] a"
        );
    }

    // ── subagent ──────────────────────────────────────────────────────────────

    /// Replays scripted completions and records the tool names offered.
    struct Scripted {
        responses: Mutex<Vec<ChatCompletion>>,
        offered: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait]
    impl ProviderClient for Scripted {
        async fn complete(
            &self,
            _messages: Vec<ChatMessage>,
            tools: Vec<Tool>,
            _model: &str,
        ) -> Result<ChatCompletion, HarnessError> {
            self.offered
                .lock()
                .unwrap()
                .push(tools.iter().map(|t| t.function.name.clone()).collect());
            let mut r = self.responses.lock().unwrap();
            if r.is_empty() {
                return Err(HarnessError::Provider("script exhausted".to_string()));
            }
            Ok(r.remove(0))
        }

        async fn stream(
            &self,
            _messages: Vec<ChatMessage>,
            _tools: Vec<Tool>,
            _model: &str,
        ) -> Result<
            Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>,
            HarnessError,
        > {
            Err(HarnessError::Provider("not used".to_string()))
        }

        async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
            Ok(())
        }
    }

    fn completion(content: Option<&str>, call: Option<(&str, &str)>) -> ChatCompletion {
        let tool_calls = call.map(|(name, args)| {
            vec![ToolCall {
                id: "c1".to_string(),
                kind: "function".to_string(),
                function: FunctionCall {
                    name: name.to_string(),
                    arguments: args.to_string(),
                },
            }]
        });
        ChatCompletion {
            id: "x".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(content.map(str::to_string), tool_calls),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: None,
        }
    }

    fn scripted(responses: Vec<ChatCompletion>) -> Arc<Scripted> {
        Arc::new(Scripted {
            responses: Mutex::new(responses),
            offered: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn subagent_runs_read_only_tools_and_returns_the_answer() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("lib.rs"), "fn needle() {}\n").unwrap();
        let provider = scripted(vec![
            completion(None, Some(("grep", r#"{"pattern":"needle"}"#))),
            completion(Some("needle is defined in lib.rs:1"), None),
        ]);
        let tool = Subagent::new(provider.clone(), "m");
        let out = tool
            .execute(
                json!({"prompt": "where is needle?"}),
                &ToolContext::new(tmp.path()),
            )
            .await
            .unwrap();
        assert_eq!(out, "needle is defined in lib.rs:1");
        let offered = provider.offered.lock().unwrap();
        let mut names = offered[0].clone();
        names.sort();
        assert_eq!(names, ["glob", "grep", "list_dir", "read_file"]);
        assert!(!names.contains(&"subagent".to_string()), "depth limit 1");
    }

    #[tokio::test]
    async fn subagent_cannot_write_or_escape() {
        let tmp = TempDir::new().unwrap();
        let provider = scripted(vec![
            completion(
                None,
                Some(("write_file", r#"{"path":"x.txt","content":"x"}"#)),
            ),
            completion(None, Some(("read_file", r#"{"path":"../../etc/passwd"}"#))),
            completion(None, Some(("subagent", r#"{"prompt":"recurse"}"#))),
            completion(Some("done"), None),
        ]);
        let out = Subagent::new(provider, "m")
            .execute(json!({"prompt": "try"}), &ToolContext::new(tmp.path()))
            .await
            .unwrap();
        assert_eq!(out, "done");
        assert!(!tmp.path().join("x.txt").exists(), "write tool unavailable");
    }

    #[test]
    fn subagent_inner_calls_follow_the_parent_policy() {
        use crate::policy::permission::{Mode, Policy};
        let mut policy = Policy {
            default_mode: Mode::Allow,
            ..Default::default()
        };
        policy.tools.insert("read_file".into(), Mode::Deny);
        policy.tools.insert("grep".into(), Mode::Ask);
        let args = r#"{"path":"a.txt"}"#;
        assert!(
            policy_refusal(Some(&policy), "read_file", args)
                .unwrap()
                .contains("`deny`")
        );
        assert!(
            policy_refusal(Some(&policy), "grep", args)
                .unwrap()
                .contains("`ask`")
        );
        assert_eq!(policy_refusal(Some(&policy), "list_dir", args), None);
        assert_eq!(policy_refusal(None, "read_file", args), None);
    }

    #[tokio::test]
    async fn subagent_stops_after_the_step_cap() {
        let tmp = TempDir::new().unwrap();
        let looping = (0..SUBAGENT_MAX_STEPS + 1)
            .map(|_| completion(None, Some(("list_dir", "{}"))))
            .collect();
        let e = Subagent::new(scripted(looping), "m")
            .execute(json!({"prompt": "loop"}), &ToolContext::new(tmp.path()))
            .await
            .unwrap_err();
        assert!(e.to_string().contains("did not finish"), "{e}");
        let e = Subagent::new(scripted(vec![]), "m")
            .execute(json!({"prompt": " "}), &ToolContext::new(tmp.path()))
            .await
            .unwrap_err();
        assert!(e.to_string().contains("missing `prompt`"), "{e}");
    }
}
