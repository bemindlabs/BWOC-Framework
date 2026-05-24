//! Core tool implementations: read_file, write_file, list_dir, run_command.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ToolContext, ToolImpl};
use crate::error::HarnessError;
use crate::sandbox::shell_command;

// ---------------------------------------------------------------------------
// read_file
// ---------------------------------------------------------------------------

pub struct ReadFile;

#[async_trait]
impl ToolImpl for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read the contents of a file at the given path. The path is relative to the working directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read (relative to working directory)."
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let raw = args["path"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `path` argument".to_string(),
            })?;

        let path = ctx.resolve_path(raw)?;

        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot read `{}`: {e}", path.display()),
            })
    }
}

// ---------------------------------------------------------------------------
// write_file
// ---------------------------------------------------------------------------

pub struct WriteFile;

#[async_trait]
impl ToolImpl for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write content to a file, creating parent directories as needed. Overwrites existing content."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to write (relative to working directory)."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let raw = args["path"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `path` argument".to_string(),
            })?;

        let content = args["content"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `content` argument".to_string(),
            })?;

        let path = ctx.resolve_path(raw)?;

        // Create parent directories if they don't exist.
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("cannot create parent dirs for `{}`: {e}", path.display()),
                })?;
        }

        tokio::fs::write(&path, content)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot write `{}`: {e}", path.display()),
            })?;

        Ok(format!(
            "wrote {} bytes to `{}`",
            content.len(),
            path.display()
        ))
    }
}

// ---------------------------------------------------------------------------
// list_dir
// ---------------------------------------------------------------------------

pub struct ListDir;

#[async_trait]
impl ToolImpl for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List the entries in a directory. Returns a newline-separated list of names (with a trailing `/` for directories)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path to list (relative to working directory). Defaults to working directory root if omitted."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let raw = args["path"].as_str().unwrap_or(".");
        let path = ctx.resolve_path(raw)?;

        let mut entries =
            tokio::fs::read_dir(&path)
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("cannot read dir `{}`: {e}", path.display()),
                })?;

        let mut names = Vec::new();
        while let Some(entry) =
            entries
                .next_entry()
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("error reading dir entry: {e}"),
                })?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let suffix = if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                "/"
            } else {
                ""
            };
            names.push(format!("{name}{suffix}"));
        }

        names.sort();
        Ok(names.join("\n"))
    }
}

// ---------------------------------------------------------------------------
// run_command
// ---------------------------------------------------------------------------

/// Minimal path confinement for `run_command`:
/// - Rejects commands that look like they could escape the worktree or bypass
///   git safety hooks.  Full allow/deny + arg scan is P2.
/// - The working directory for the child process is always the worktree root.
pub struct RunCommand;

/// Commands that are blocked in P1 (P2 adds a full allow/deny list).
const BLOCKED_COMMANDS: &[&str] = &[
    "rm",
    "sudo",
    "su",
    "curl",
    "wget",
    "git push --force",
    "git push -f",
];

#[async_trait]
impl ToolImpl for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self) -> &'static str {
        "Run a shell command inside the working directory. The command runs as a subprocess with the working directory set to the agent worktree. Some dangerous commands are blocked."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to run (runs via sh -c)."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let cmd = args["command"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `command` argument".to_string(),
            })?;

        // P1 minimal block list.
        for blocked in BLOCKED_COMMANDS {
            if cmd.contains(blocked) {
                return Err(HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("command blocked by P1 policy (contains `{blocked}`): `{cmd}`"),
                });
            }
        }

        let output = shell_command(cmd)
            .current_dir(&ctx.workdir)
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn command: {e}"),
            })?;

        let mut result = String::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stdout.is_empty() {
            result.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            result.push_str(&format!("\n[exit code: {code}]"));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    fn ctx_for_tmp(dir: &TempDir) -> ToolContext {
        ToolContext::new(dir.path().to_path_buf())
    }

    #[test]
    fn read_file_roundtrip() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for_tmp(&tmp);

            // Write via WriteFile then read back.
            let writer = WriteFile;
            let args = json!({ "path": "hello.txt", "content": "สวัสดี" });
            let msg = writer.execute(args, &ctx).await.unwrap();
            assert!(msg.contains("hello.txt"));

            let reader = ReadFile;
            let args = json!({ "path": "hello.txt" });
            let content = reader.execute(args, &ctx).await.unwrap();
            assert_eq!(content, "สวัสดี");
        });
    }

    #[test]
    fn read_file_escape_rejected() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for_tmp(&tmp);
            let reader = ReadFile;
            let args = json!({ "path": "../../etc/passwd" });
            let err = reader.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    #[test]
    fn list_dir_returns_entries() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for_tmp(&tmp);

            // Create a file and a dir.
            tokio::fs::write(tmp.path().join("file.txt"), "x")
                .await
                .unwrap();
            tokio::fs::create_dir(tmp.path().join("subdir"))
                .await
                .unwrap();

            let lister = ListDir;
            let args = json!({});
            let out = lister.execute(args, &ctx).await.unwrap();
            assert!(out.contains("file.txt"));
            assert!(out.contains("subdir/"));
        });
    }

    #[test]
    fn run_command_blocked() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for_tmp(&tmp);
            let runner = RunCommand;
            let args = json!({ "command": "rm -rf ." });
            let err = runner.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::ToolExecution { .. }));
        });
    }

    #[test]
    fn run_command_echo() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for_tmp(&tmp);
            let runner = RunCommand;
            let args = json!({ "command": "echo hello" });
            let out = runner.execute(args, &ctx).await.unwrap();
            assert!(out.contains("hello"));
        });
    }

    #[test]
    fn write_creates_parent_dirs() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for_tmp(&tmp);
            let writer = WriteFile;
            let args = json!({ "path": "deep/nested/dir/file.txt", "content": "data" });
            writer.execute(args, &ctx).await.unwrap();
            let path: PathBuf = [tmp.path().to_str().unwrap(), "deep/nested/dir/file.txt"]
                .iter()
                .collect();
            assert!(path.exists());
        });
    }
}
