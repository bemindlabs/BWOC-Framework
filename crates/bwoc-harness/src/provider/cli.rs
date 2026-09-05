//! CLI-backed provider: drive a local, subscription-authenticated vendor CLI
//! instead of an HTTP endpoint (#277).
//!
//! Why: orgs that block API keys still have seats on Claude/Codex
//! **subscriptions**, whose CLIs authenticate via their own OAuth login. This
//! provider lets `bwoc-harness --chat` (and therefore `bwoc-connect` bridges
//! and the TUI) run on those models with **zero API key**: each turn shells out
//! to `<cli> -p --model <model> --output-format json`, piping the conversation
//! on **stdin** (no `ARG_MAX` limit) and parsing the JSON envelope the CLI
//! prints (`{"result": "...", "is_error": false, ...}`).
//!
//! **Targets the `claude` CLI's print mode.** The flag set (`-p`, `--model`,
//! `--output-format json`) and the result envelope are Claude Code's
//! documented non-interactive contract. Another vendor CLI works here only if
//! it accepts the same flags — otherwise point `--cli-cmd` at a small wrapper
//! script that adapts them. The raw-stdout fallback below tolerates a
//! different *output* shape (plain text instead of the JSON envelope); it does
//! not adapt *input* flags.
//!
//! **Chat-only by design.** Harness tool-calling is NOT available on this
//! backend: a vendor CLI executes its *own* tools internally and its
//! print-mode output carries no `tool_calls`. The harness `tools` parameter is
//! deliberately ignored (never an error — the chat session always passes its
//! registry) so the CLI backend behaves as a plain conversational model. Use
//! an HTTP backend (`claude`/`openrouter`/`ollama`) for agentic tool use.
//!
//! **Stateless transcript.** Every call flattens the full message history into
//! one prompt (`System:`/`Human:`/`Assistant:` turns). This keeps the
//! harness's history as the single source of truth — no hidden CLI session
//! state to diverge from it. Per-chat `--resume` continuity (the cheaper
//! incremental path) is a possible follow-up, deliberately not in the MVP.

use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::HarnessError;

use super::client::ProviderClient;
use super::types::{
    ChatCompletion, ChatMessage, Choice, Delta, FinishReason, Role, StreamChunk, StreamDelta, Tool,
};

/// Hard ceiling on one CLI turn. A wedged CLI (e.g. one that fell back to an
/// interactive auth prompt despite `-p`) would otherwise hang the chat turn
/// forever — HTTP providers get this guard from their client timeouts.
const CLI_TURN_TIMEOUT: Duration = Duration::from_secs(600);

/// Default vendor CLI when `--cli-cmd` is not given.
pub const DEFAULT_CLI_CMD: &str = "claude";

/// [`ProviderClient`] that shells out to a local vendor CLI per turn.
pub struct CliClient {
    cmd: String,
}

impl CliClient {
    pub fn new(cmd: impl Into<String>) -> Self {
        Self { cmd: cmd.into() }
    }

    /// Flatten the OpenAI-shaped history into one print-mode prompt. Tool
    /// messages are folded in as labelled context so a mixed history (from a
    /// session that previously ran on an HTTP backend) still reads coherently:
    /// a tool-call-only assistant message (content `None`, `tool_calls` set)
    /// becomes a compact `[invoked tools: …]` note so the `Tool result:` lines
    /// that follow it keep their antecedent instead of dangling.
    fn flatten(messages: &[ChatMessage]) -> String {
        let mut out = String::new();
        for m in messages {
            let mut content = m.content.as_deref().unwrap_or("").to_string();
            if content.is_empty() {
                match &m.tool_calls {
                    Some(calls) if !calls.is_empty() => {
                        let names: Vec<&str> =
                            calls.iter().map(|c| c.function.name.as_str()).collect();
                        content = format!("[invoked tools: {}]", names.join(", "));
                    }
                    _ => continue,
                }
            }
            let label = match m.role {
                Role::System => "System",
                Role::User => "Human",
                Role::Assistant => "Assistant",
                Role::Tool => "Tool result",
            };
            out.push_str(label);
            out.push_str(":\n");
            out.push_str(&content);
            out.push_str("\n\n");
        }
        out.push_str("Assistant:\n");
        out
    }

    /// Run one CLI turn: spawn, pipe the prompt on stdin, collect stdout.
    async fn run_turn(&self, prompt: String, model: &str) -> Result<String, HarnessError> {
        let mut child = Command::new(&self.cmd)
            .arg("-p")
            .arg("--model")
            .arg(model)
            .arg("--output-format")
            .arg("json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                HarnessError::Provider(format!(
                    "cli backend: cannot exec `{}`: {e} — is the CLI installed and on PATH?",
                    self.cmd
                ))
            })?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            HarnessError::Provider("cli backend: failed to open child stdin".into())
        })?;
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|e| HarnessError::Provider(format!("cli backend: write prompt: {e}")))?;
        drop(stdin); // EOF so the CLI starts

        // Collect output with a deadline, keeping ownership of `child` so the
        // timeout branch can explicitly kill AND reap (no zombie left for the
        // runtime's background reaper; `kill_on_drop` stays as a safety net for
        // cancellation).
        let mut stdout_pipe = child.stdout.take().ok_or_else(|| {
            HarnessError::Provider("cli backend: failed to open child stdout".into())
        })?;
        let mut stderr_pipe = child.stderr.take().ok_or_else(|| {
            HarnessError::Provider("cli backend: failed to open child stderr".into())
        })?;
        let collect = async {
            use tokio::io::AsyncReadExt;
            let mut out_buf = Vec::new();
            let mut err_buf = Vec::new();
            let (o, e, status) = tokio::join!(
                stdout_pipe.read_to_end(&mut out_buf),
                stderr_pipe.read_to_end(&mut err_buf),
                child.wait()
            );
            o.map_err(|e| HarnessError::Provider(format!("cli backend: read stdout: {e}")))?;
            e.map_err(|e| HarnessError::Provider(format!("cli backend: read stderr: {e}")))?;
            let status =
                status.map_err(|e| HarnessError::Provider(format!("cli backend: wait: {e}")))?;
            Ok::<_, HarnessError>((status, out_buf, err_buf))
        };
        let (status, out_buf, err_buf) = match tokio::time::timeout(CLI_TURN_TIMEOUT, collect).await
        {
            Ok(res) => res?,
            Err(_) => {
                // `collect` (which borrowed `child`) is dropped; kill + reap.
                let _ = child.kill().await; // SIGKILL + await reaps the zombie
                return Err(HarnessError::transient(format!(
                    "cli backend: `{}` exceeded {}s for one turn (killed)",
                    self.cmd,
                    CLI_TURN_TIMEOUT.as_secs()
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&out_buf).into_owned();
        let stderr = String::from_utf8_lossy(&err_buf).into_owned();

        if !status.success() {
            let detail = if stderr.trim().is_empty() {
                &stdout
            } else {
                &stderr
            };
            let detail: String = detail.chars().take(600).collect();
            // Vendor CLIs print model errors to stderr with non-zero status;
            // surface unknown-model wording as ModelNotFound for the fallback
            // chain, everything else as a provider error.
            if detail.to_lowercase().contains("model") && detail.to_lowercase().contains("not") {
                return Err(HarnessError::ModelNotFound(model.to_string()));
            }
            return Err(HarnessError::Provider(format!(
                "cli backend: `{}` exited {}: {}",
                self.cmd, status, detail
            )));
        }

        // Print-mode JSON envelope: {"result": "...", "is_error": bool, ...}.
        // Fall back to raw stdout for CLIs without a JSON print mode.
        match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
            Ok(v) if v.is_object() => {
                if v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false) {
                    let msg = v
                        .get("result")
                        .and_then(|r| r.as_str())
                        .unwrap_or("(no detail)");
                    return Err(HarnessError::Provider(format!(
                        "cli backend: `{}` reported an error: {}",
                        self.cmd,
                        msg.chars().take(600).collect::<String>()
                    )));
                }
                let text = v
                    .get("result")
                    .and_then(|r| r.as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| stdout.trim().to_owned());
                Ok(text)
            }
            _ => Ok(stdout.trim().to_owned()),
        }
    }

    fn completion_from_text(text: String) -> ChatCompletion {
        ChatCompletion {
            id: format!("cli-{}", std::process::id()),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage::assistant(Some(text), None),
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: None,
        }
    }
}

#[async_trait]
impl ProviderClient for CliClient {
    async fn complete(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<Tool>, // chat-only: the CLI runs its own tools internally
        model: &str,
    ) -> Result<ChatCompletion, HarnessError> {
        let prompt = Self::flatten(&messages);
        let text = self.run_turn(prompt, model).await?;
        Ok(Self::completion_from_text(text))
    }

    /// Print-mode CLIs emit the whole reply at once, so the "stream" is the
    /// completed turn as a single content chunk. The chat loop renders it
    /// identically to a one-chunk SSE stream.
    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _tools: Vec<Tool>,
        model: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamChunk, HarnessError>> + Send>>, HarnessError>
    {
        let prompt = Self::flatten(&messages);
        let text = self.run_turn(prompt, model).await?;
        let chunk = StreamChunk {
            id: format!("cli-{}", std::process::id()),
            choices: vec![StreamDelta {
                index: 0,
                delta: Delta {
                    role: Some(Role::Assistant),
                    content: Some(text),
                    tool_calls: None,
                },
                finish_reason: Some(FinishReason::Stop),
            }],
            usage: None,
            thinking_block: None,
        };
        Ok(Box::pin(futures_util::stream::iter(vec![Ok(chunk)])))
    }

    /// Verify the CLI binary is executable. Model names are not validated
    /// up-front — the CLI checks them itself on the first turn (and an unknown
    /// model surfaces as [`HarnessError::ModelNotFound`] there).
    async fn validate_model(&self, _model: &str) -> Result<(), HarnessError> {
        let status = Command::new(&self.cmd)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|e| {
                HarnessError::Provider(format!(
                    "cli backend: cannot exec `{}`: {e} — is the CLI installed and on PATH?",
                    self.cmd
                ))
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(HarnessError::Provider(format!(
                "cli backend: `{} --version` exited {status}",
                self.cmd
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, content: &str) -> ChatMessage {
        match role {
            Role::System => ChatMessage::system(content),
            Role::User => ChatMessage::user(content),
            Role::Assistant => ChatMessage::assistant(Some(content.to_string()), None),
            Role::Tool => ChatMessage::tool_result("id", "tool", content),
        }
    }

    #[test]
    fn flatten_orders_roles_and_ends_with_assistant_cue() {
        let prompt = CliClient::flatten(&[
            msg(Role::System, "you are anna"),
            msg(Role::User, "hello"),
            msg(Role::Assistant, "hi!"),
            msg(Role::User, "how are you?"),
        ]);
        assert!(prompt.starts_with("System:\nyou are anna\n\n"));
        assert!(prompt.contains("Human:\nhello\n\n"));
        assert!(prompt.contains("Assistant:\nhi!\n\n"));
        assert!(prompt.contains("Human:\nhow are you?\n\n"));
        assert!(prompt.ends_with("Assistant:\n"));
    }

    #[test]
    fn flatten_renders_tool_call_only_assistant_as_note() {
        use super::super::types::{FunctionCall, ToolCall};
        let call = ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        };
        let prompt = CliClient::flatten(&[
            msg(Role::User, "do it"),
            ChatMessage::assistant(None, Some(vec![call])),
            ChatMessage::tool_result("c1", "read_file", "file contents"),
        ]);
        assert!(prompt.contains("Assistant:\n[invoked tools: read_file]"));
        assert!(prompt.contains("Tool result:\nfile contents"));
    }

    #[test]
    fn flatten_skips_empty_content() {
        let prompt =
            CliClient::flatten(&[ChatMessage::assistant(None, None), msg(Role::User, "x")]);
        assert!(!prompt.contains("Assistant:\n\n\nHuman"));
        assert!(prompt.contains("Human:\nx"));
    }

    // Unix-only: exercise the real subprocess path against a fake CLI script.
    #[cfg(unix)]
    mod subprocess {
        use super::*;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        /// Write an executable fake-CLI script and return its path.
        fn fake_cli(dir: &std::path::Path, body: &str) -> String {
            let path = dir.join("fake-cli");
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh\n{body}").unwrap();
            f.sync_all().unwrap();
            drop(f); // close before exec — avoids ETXTBSY
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path.to_string_lossy().into_owned()
        }

        /// Serialize the subprocess tests against each other. Each forks/execs a
        /// real fake-CLI child; under a highly parallel test runner (cargo's
        /// default on a many-core box) the concurrent fork/exec load makes spawns
        /// flake with transient timeouts (issue #334). Taking this guard runs them
        /// one at a time — removing the contention while still exercising the real
        /// subprocess path. Async mutex so the guard is held across the `.await`s;
        /// it protects no data, so poisoning is irrelevant.
        static SPAWN_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

        async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
            SPAWN_SERIAL.lock().await
        }

        #[tokio::test]
        async fn complete_parses_json_envelope() {
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                // consume stdin, then answer with the claude print-mode shape
                r#"cat > /dev/null; printf '{"result":"hello from cli","is_error":false}'"#,
            );
            let client = CliClient::new(cli);
            let done = client
                .complete(vec![msg(Role::User, "hi")], vec![], "m1")
                .await
                .unwrap();
            assert_eq!(
                done.choices[0].message.content.as_deref(),
                Some("hello from cli")
            );
            assert!(matches!(
                done.choices[0].finish_reason,
                Some(FinishReason::Stop)
            ));
        }

        #[tokio::test]
        async fn complete_falls_back_to_raw_stdout() {
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(dir.path(), r#"cat > /dev/null; printf 'plain text reply'"#);
            let client = CliClient::new(cli);
            let done = client
                .complete(vec![msg(Role::User, "hi")], vec![], "m1")
                .await
                .unwrap();
            assert_eq!(
                done.choices[0].message.content.as_deref(),
                Some("plain text reply")
            );
        }

        #[tokio::test]
        async fn is_error_envelope_becomes_provider_error() {
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                r#"cat > /dev/null; printf '{"result":"quota exceeded","is_error":true}'"#,
            );
            let client = CliClient::new(cli);
            let err = client
                .complete(vec![msg(Role::User, "hi")], vec![], "m1")
                .await
                .unwrap_err();
            assert!(matches!(err, HarnessError::Provider(ref m) if m.contains("quota exceeded")));
        }

        #[tokio::test]
        async fn nonzero_exit_surfaces_stderr() {
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(dir.path(), r#"cat > /dev/null; echo 'boom' >&2; exit 3"#);
            let client = CliClient::new(cli);
            let err = client
                .complete(vec![msg(Role::User, "hi")], vec![], "m1")
                .await
                .unwrap_err();
            assert!(matches!(err, HarnessError::Provider(ref m) if m.contains("boom")));
        }

        #[tokio::test]
        async fn unknown_model_maps_to_model_not_found() {
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                r#"cat > /dev/null; echo 'error: model not found' >&2; exit 1"#,
            );
            let client = CliClient::new(cli);
            let err = client
                .complete(vec![msg(Role::User, "hi")], vec![], "bad-model")
                .await
                .unwrap_err();
            assert!(matches!(err, HarnessError::ModelNotFound(ref m) if m == "bad-model"));
        }

        #[tokio::test]
        async fn missing_binary_is_clear_error() {
            let _serial = serial().await;
            let client = CliClient::new("/nonexistent/definitely-not-a-cli");
            let err = client
                .complete(vec![msg(Role::User, "hi")], vec![], "m1")
                .await
                .unwrap_err();
            assert!(matches!(err, HarnessError::Provider(ref m) if m.contains("cannot exec")));
        }

        #[tokio::test]
        async fn stream_is_single_full_chunk() {
            use futures_util::StreamExt;
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(
                dir.path(),
                r#"cat > /dev/null; printf '{"result":"streamed","is_error":false}'"#,
            );
            let client = CliClient::new(cli);
            let mut s = client
                .stream(vec![msg(Role::User, "hi")], vec![], "m1")
                .await
                .unwrap();
            let first = s.next().await.unwrap().unwrap();
            assert_eq!(first.choices[0].delta.content.as_deref(), Some("streamed"));
            assert!(s.next().await.is_none());
        }

        #[tokio::test]
        async fn validate_model_checks_binary() {
            let _serial = serial().await;
            let dir = tempfile::tempdir().unwrap();
            let cli = fake_cli(dir.path(), "exit 0");
            assert!(CliClient::new(cli).validate_model("any").await.is_ok());
            assert!(
                CliClient::new("/nope/cli")
                    .validate_model("any")
                    .await
                    .is_err()
            );
        }
    }
}
