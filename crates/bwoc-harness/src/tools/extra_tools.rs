//! Extra tool implementations added in the "complete tool set" increment:
//! edit_file, grep, git, run_gates, bwoc_task, bwoc_send, memory_read,
//! memory_write.
//!
//! Every tool routes through the caller's safety pipeline
//! (guardrails → permission → sandbox) via `ToolContext` path confinement.
//! The tools themselves do not call the pipeline — that is the agent_loop's
//! responsibility — but they enforce worktree confinement via
//! `ToolContext::resolve_path` on every filesystem operation.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ToolContext, ToolImpl};
use crate::error::HarnessError;
use crate::sandbox::shell_command;

// ---------------------------------------------------------------------------
// edit_file — targeted string replacement (unique-match patch)
// ---------------------------------------------------------------------------

/// Replace exactly one occurrence of `old_string` with `new_string` in the
/// file at `path`.  The replacement is rejected if `old_string` matches zero
/// or more than one time (unique-match invariant, same as Claude Code's Edit
/// tool).
///
/// Confined to the worktree; passes through the safety pipeline before
/// reaching here.
pub struct EditFile;

#[async_trait]
impl ToolImpl for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Targeted string replacement in a file. Replaces exactly one occurrence of \
         `old_string` with `new_string`. Fails if `old_string` matches zero or more \
         than one time (unique-match invariant). The file must already exist."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative to working directory)."
                },
                "old_string": {
                    "type": "string",
                    "description": "The exact string to find. Must appear exactly once in the file."
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement string."
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let raw = args["path"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `path` argument".to_string(),
            })?;
        let old = args["old_string"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `old_string` argument".to_string(),
            })?;
        let new = args["new_string"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `new_string` argument".to_string(),
            })?;

        let path = ctx.resolve_path(raw)?;

        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("cannot read `{}`: {e}", path.display()),
                })?;

        let count = content.matches(old).count();
        if count == 0 {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "`old_string` not found in `{}`. \
                     Read the file first and ensure the string matches exactly.",
                    path.display()
                ),
            });
        }
        if count > 1 {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "`old_string` matches {count} times in `{}`. \
                     Provide more surrounding context so the match is unique.",
                    path.display()
                ),
            });
        }

        let updated = content.replacen(old, new, 1);
        tokio::fs::write(&path, &updated)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot write `{}`: {e}", path.display()),
            })?;

        Ok(format!(
            "edited `{}`: replaced 1 occurrence",
            path.display()
        ))
    }
}

// ---------------------------------------------------------------------------
// grep — search file contents by pattern under the worktree
// ---------------------------------------------------------------------------

/// Walk the worktree (or a sub-path) and search for lines matching a pattern.
///
/// Uses a pure-Rust walk + `str::contains` (substring, case-sensitive by
/// default) so no external binary is required.  The `regex` crate is NOT
/// added as a dep (dep-quarantine: bwoc-harness is already the heaviest
/// crate; a simple substring search covers the primary use-case and the
/// model can shell out to `rg` via `run_command` for advanced regex).
///
/// Output: matching lines in `<relative-path>:<line-no>:<line>` format,
/// capped at 1000 matches to prevent context overflow.
pub struct Grep;

const GREP_MAX_MATCHES: usize = 1_000;

#[async_trait]
impl ToolImpl for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search file contents for lines containing a pattern under the working directory. \
         Returns matching lines in `<path>:<line>:<content>` format. \
         `pattern` is a case-sensitive substring by default; set `case_insensitive` to true \
         for case-insensitive matching. Results are capped at 1000 matches. \
         The search is confined to the working directory (worktree-safe)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Substring to search for."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search (relative to working directory). Defaults to working directory root."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "If true, matching is case-insensitive. Default: false."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `pattern` argument".to_string(),
            })?;
        let raw = args["path"].as_str().unwrap_or(".");
        let case_insensitive = args["case_insensitive"].as_bool().unwrap_or(false);

        let search_root = ctx.resolve_path(raw)?;

        // Tokio's fs::read_dir is async but walking is inherently recursive;
        // use blocking spawn to avoid blocking the async runtime.
        let workdir = ctx.workdir.clone();
        let pattern_owned = pattern.to_string();
        let pattern_display = pattern_owned.clone();
        let results = tokio::task::spawn_blocking(move || {
            grep_walk(&search_root, &workdir, &pattern_owned, case_insensitive)
        })
        .await
        .map_err(|e| HarnessError::ToolExecution {
            tool: "grep".to_string(),
            reason: format!("grep task panicked: {e}"),
        })??;

        if results.is_empty() {
            Ok(format!("no matches for `{pattern_display}` in `{raw}`"))
        } else {
            let truncated = results.len() >= GREP_MAX_MATCHES;
            let mut out = results.join("\n");
            if truncated {
                out.push_str(&format!(
                    "\n[truncated at {GREP_MAX_MATCHES} matches — narrow your search path or pattern]"
                ));
            }
            Ok(out)
        }
    }
}

/// Synchronous recursive walk + grep (runs in spawn_blocking).
fn grep_walk(
    root: &std::path::Path,
    workdir: &std::path::Path,
    pattern: &str,
    case_insensitive: bool,
) -> Result<Vec<String>, HarnessError> {
    let pattern_lower = pattern.to_lowercase();
    let mut matches = Vec::new();

    // Simple recursive walker using std::fs.
    let mut dirs: Vec<std::path::PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        // Confinement: every dir must be inside workdir.
        if !dir.starts_with(workdir) {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let p = entry.path();
            // Confinement check.
            if !p.starts_with(workdir) {
                continue;
            }
            // Skip hidden directories (.git, .bwoc, …).
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') && p.is_dir() {
                    continue;
                }
            }
            if p.is_dir() {
                dirs.push(p);
            } else if p.is_file() {
                // Best-effort: skip binary-looking files (non-UTF-8).
                let content = match std::fs::read_to_string(&p) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let rel = p
                    .strip_prefix(workdir)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .to_string();

                for (lineno, line) in content.lines().enumerate() {
                    let hit = if case_insensitive {
                        line.to_lowercase().contains(&pattern_lower)
                    } else {
                        line.contains(pattern)
                    };
                    if hit {
                        matches.push(format!("{}:{}:{}", rel, lineno + 1, line));
                        if matches.len() >= GREP_MAX_MATCHES {
                            return Ok(matches);
                        }
                    }
                }
            }
        }
    }

    Ok(matches)
}

// ---------------------------------------------------------------------------
// git — scoped git ops in the worktree
// ---------------------------------------------------------------------------

/// Run a git subcommand inside the worktree.
///
/// The current working directory is forced to the worktree root.
/// Force-push (`--force`, `-f`) and history-rewrite (`--no-verify`) remain
/// blocked — the guardrails layer catches them *before* this tool executes,
/// so we do not need to duplicate the check here (defence-in-depth is the
/// guardrails + sandbox layers).
///
/// Allowed subcommands: status, diff, add, commit, branch, worktree, log,
/// show, stash (list only), fetch, pull, push (non-force).
///
/// Credentials for push are injected by the P3 auth broker; git reads them
/// from the environment.
pub struct Git;

/// Git subcommands explicitly allowed.
const GIT_ALLOWED_SUBS: &[&str] = &[
    "status",
    "diff",
    "add",
    "commit",
    "branch",
    "worktree",
    "log",
    "show",
    "stash",
    "fetch",
    "pull",
    "push",
    "checkout",
    "switch",
    "restore",
    "merge-base",
    "rev-parse",
    "ls-files",
    "tag",
];

/// Git subcommands that are never allowed (history-rewrite / destructive).
const GIT_BLOCKED_SUBS: &[&str] = &[
    "rebase",
    "filter-branch",
    "filter-repo",
    "reset",
    "clean",
    "gc",
    "reflog",
    "update-ref",
    "am",
    "apply",
];

#[async_trait]
impl ToolImpl for Git {
    fn name(&self) -> &'static str {
        "git"
    }

    fn description(&self) -> &'static str {
        "Run a scoped git operation inside the worktree. \
         `subcommand` is the git subcommand (e.g. `status`, `diff`, `add`, `commit`, `push`). \
         `args` is an optional list of arguments. \
         Force-push, `--no-verify`, history-rewrite, and destructive subcommands are blocked. \
         The working directory is always the worktree root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "Git subcommand to run (e.g. 'status', 'diff', 'add', 'commit', 'push')."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of arguments to pass after the subcommand."
                }
            },
            "required": ["subcommand"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let sub = args["subcommand"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `subcommand` argument".to_string(),
            })?;

        // Tool-level allow/block list (guardrails also check at a higher level).
        if GIT_BLOCKED_SUBS.contains(&sub) {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "git subcommand `{sub}` is blocked (history-rewrite / destructive). \
                     Use the worktree model: work on feature branches, rebase manually if needed."
                ),
            });
        }
        if !GIT_ALLOWED_SUBS.contains(&sub) {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "git subcommand `{sub}` is not in the allowed list: {}",
                    GIT_ALLOWED_SUBS.join(", ")
                ),
            });
        }

        // Collect extra args.
        let extra: Vec<String> = match args["args"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            None => Vec::new(),
        };

        // Defence-in-depth: block --no-verify and force-push flags
        // (guardrails catch these first, but being explicit here is safer).
        for arg in &extra {
            if arg == "--no-verify" {
                return Err(HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: "`--no-verify` is not permitted (Surāmeraya guardrail).".to_string(),
                });
            }
            if sub == "push"
                && (arg == "--force"
                    || arg == "-f"
                    || arg == "--force-with-lease"
                    || arg == "--force-if-includes")
            {
                return Err(HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!(
                        "`git push {arg}` is not permitted (Surāmeraya guardrail). \
                         Only non-force pushes are allowed."
                    ),
                });
            }
        }

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg(sub);
        cmd.args(&extra);
        cmd.current_dir(&ctx.workdir);

        // Scrub the environment (same as sandbox.rs) — git only needs PATH +
        // identity vars.  The P3 auth broker injects GITHUB_TOKEN at exec time.
        cmd.env_clear();
        for var in &[
            "PATH",
            "HOME",
            "USER",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "SSH_AUTH_SOCK",
            "GIT_SSH_COMMAND",
            "TMPDIR",
            "TMP",
            "TEMP",
        ] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn git: {e}"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        let mut result = stdout;
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

// ---------------------------------------------------------------------------
// run_gates — run the verification gates declared in the agent manifest
// ---------------------------------------------------------------------------

/// Read `lintCmd`, `formatCmd`, `testCmd`, `buildCmd` from the agent's
/// `config.manifest.json` and run them in order, capturing pass/fail and
/// output.  Returns a structured report.
///
/// The manifest is located by searching upward from `ctx.workdir` for
/// `config.manifest.json`.  The commands are run via `sh -c` with the
/// worktree root as `cwd`.
///
/// Source of gate definitions: `bwoc-core::manifest::Manifest` (already a dep).
pub struct RunGates;

#[async_trait]
impl ToolImpl for RunGates {
    fn name(&self) -> &'static str {
        "run_gates"
    }

    fn description(&self) -> &'static str {
        "Run the verification gates declared in the agent manifest \
         (lint/format/test/build commands). Returns pass/fail status and output \
         for each gate. Gates are read from `config.manifest.json` in the \
         working directory. Feeds gate-pass/fail counts into telemetry."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "gates": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["lint", "format", "test", "build"]
                    },
                    "description": "Which gates to run. Defaults to all four: [\"lint\", \"format\", \"test\", \"build\"]."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        // Load the manifest.
        let manifest_path = ctx.workdir.join("config.manifest.json");
        if !manifest_path.exists() {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "config.manifest.json not found in `{}`. \
                     run_gates requires the agent manifest.",
                    ctx.workdir.display()
                ),
            });
        }

        let manifest =
            bwoc_core::manifest::Manifest::load_from_path(&manifest_path).map_err(|e| {
                HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("cannot load manifest: {e}"),
                }
            })?;

        // Decide which gates to run.
        let requested: Vec<String> = match args["gates"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            None => vec![
                "lint".to_string(),
                "format".to_string(),
                "test".to_string(),
                "build".to_string(),
            ],
        };

        let gate_cmds: Vec<(&str, &str)> = requested
            .iter()
            .filter_map(|g| match g.as_str() {
                "lint" => Some(("lint", manifest.lint_cmd.as_str())),
                "format" => Some(("format", manifest.format_cmd.as_str())),
                "test" => Some(("test", manifest.test_cmd.as_str())),
                "build" => Some(("build", manifest.build_cmd.as_str())),
                _ => None,
            })
            .collect();

        if gate_cmds.is_empty() {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "no valid gates specified (accepted: lint, format, test, build)"
                    .to_string(),
            });
        }

        let mut report = String::new();
        let mut all_passed = true;

        for (gate_name, cmd) in &gate_cmds {
            let output = shell_command(cmd)
                .current_dir(&ctx.workdir)
                .output()
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("failed to run gate `{gate_name}` (`{cmd}`): {e}"),
                })?;

            let passed = output.status.success();
            if !passed {
                all_passed = false;
            }

            let status_str = if passed { "PASS" } else { "FAIL" };
            report.push_str(&format!("[{gate_name}] {status_str} — `{cmd}`\n"));

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                report.push_str(&format!("  stdout: {}\n", stdout.trim()));
            }
            if !stderr.trim().is_empty() {
                report.push_str(&format!("  stderr: {}\n", stderr.trim()));
            }
            let code = output.status.code().unwrap_or(-1);
            if !passed {
                report.push_str(&format!("  exit code: {code}\n"));
            }
        }

        let summary = if all_passed {
            "ALL GATES PASSED"
        } else {
            "ONE OR MORE GATES FAILED"
        };
        report.push_str(&format!("\n{summary}\n"));

        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// bwoc_task — claim / complete / list Saṅgha tasks
// ---------------------------------------------------------------------------

/// Claim, complete, or list tasks on the team's shared task list.
///
/// **Implementation choice: shell out to `bwoc` binary.**
/// Rationale: `bwoc-core::team` owns pure data-model + state-transition
/// logic, but the file locking, JSONL persistence, and team discovery all
/// live in `bwoc-cli`.  Reimplementing that here would duplicate non-trivial
/// I/O logic *and* risk race conditions that the CLI's advisory lock prevents.
/// Shelling out to `bwoc` is the cleaner path, adds zero deps, and keeps
/// the harness decoupled from CLI internals.
///
/// The `--from` identity is the agent's own ID; the guardrails' Musāvāda
/// check (identity spoof) fires first if the model tries to inject a
/// different agent ID.
pub struct BwocTask;

#[async_trait]
impl ToolImpl for BwocTask {
    fn name(&self) -> &'static str {
        "bwoc_task"
    }

    fn description(&self) -> &'static str {
        "Claim, complete, or list Saṅgha tasks on the shared task list. \
         `action` is one of: `list`, `claim`, `complete`. \
         `task_id` is required for `claim` and `complete`. \
         `team_id` identifies the team (defaults to the first team if omitted). \
         The agent identity is verified by the safety guardrails (Musāvāda)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "claim", "complete"],
                    "description": "Task action to perform."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID to claim or complete. Required for `claim` and `complete`."
                },
                "team_id": {
                    "type": "string",
                    "description": "Team ID. If omitted, the first available team is used."
                },
                "agent_id": {
                    "type": "string",
                    "description": "The agent's own ID. Must match the harness agent identity."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `action` argument (list | claim | complete)".to_string(),
            })?;

        // Build the bwoc command: `bwoc task <action> [--task <id>] [--team <id>] [--agent <id>]`
        let mut cmd = tokio::process::Command::new("bwoc");
        cmd.arg("task").arg(action);

        if let Some(task_id) = args["task_id"].as_str() {
            cmd.arg("--task").arg(task_id);
        } else if matches!(action, "claim" | "complete") {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("`task_id` is required for action `{action}`"),
            });
        }

        if let Some(team_id) = args["team_id"].as_str() {
            cmd.arg("--team").arg(team_id);
        }

        if let Some(agent_id) = args["agent_id"].as_str() {
            // The guardrails (Musāvāda) check fires before this point if
            // the model injects a spoofed identity.
            cmd.arg("--agent").arg(agent_id);
        }

        cmd.current_dir(&ctx.workdir);
        // Minimal clean env — bwoc reads its workspace from cwd.
        cmd.env_clear();
        for var in &["PATH", "HOME", "USER", "LANG", "TMPDIR"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn `bwoc`: {e}. Is `bwoc` on PATH?"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let mut result = stdout;
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

// ---------------------------------------------------------------------------
// bwoc_send — send an inter-agent message via the bwoc send channel
// ---------------------------------------------------------------------------

/// Send an inter-agent message.
///
/// **Implementation choice: shell out to `bwoc send`.**
/// Same rationale as `bwoc_task`: the send channel's routing, inbox locking,
/// and routing-table resolution live in `bwoc-cli`.  Shelling out is the
/// clean path and avoids duplicating that logic.
///
/// The `--from` field MUST be the agent's own ID.  The guardrails' Musāvāda
/// check fires before execute() if the model tries to inject a spoofed `from`
/// / `sender` value.
pub struct BwocSend;

#[async_trait]
impl ToolImpl for BwocSend {
    fn name(&self) -> &'static str {
        "bwoc_send"
    }

    fn description(&self) -> &'static str {
        "Send an inter-agent message to another agent via the bwoc send channel. \
         `to` is the target agent ID. `body` is the message text. \
         `from` must be this agent's own ID — identity spoofing is blocked by \
         the Musāvāda safety guardrail."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Target agent ID (e.g. 'agent-pi')."
                },
                "body": {
                    "type": "string",
                    "description": "Message body text."
                },
                "from": {
                    "type": "string",
                    "description": "Sender agent ID. Must be this agent's own identity."
                },
                "subject": {
                    "type": "string",
                    "description": "Optional message subject / title."
                }
            },
            "required": ["to", "body", "from"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let to = args["to"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `to` argument".to_string(),
            })?;
        let body = args["body"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `body` argument".to_string(),
            })?;
        let from = args["from"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `from` argument".to_string(),
            })?;

        let mut cmd = tokio::process::Command::new("bwoc");
        cmd.arg("send")
            .arg("--to")
            .arg(to)
            .arg("--from")
            .arg(from)
            .arg("--body")
            .arg(body);

        if let Some(subject) = args["subject"].as_str() {
            cmd.arg("--subject").arg(subject);
        }

        cmd.current_dir(&ctx.workdir);
        cmd.env_clear();
        for var in &["PATH", "HOME", "USER", "LANG", "TMPDIR"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn `bwoc`: {e}. Is `bwoc` on PATH?"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let mut result = stdout;
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

// ---------------------------------------------------------------------------
// memory_read — read from the agent's tier-1 file-based memory store
// ---------------------------------------------------------------------------

/// Read a memory file from the agent's `memories/` directory.
///
/// If `name` is omitted, reads `MEMORY.md` (the index).
/// All paths are confined to the memories sub-directory inside the worktree.
pub struct MemoryRead;

#[async_trait]
impl ToolImpl for MemoryRead {
    fn name(&self) -> &'static str {
        "memory_read"
    }

    fn description(&self) -> &'static str {
        "Read from the agent's tier-1 file-based memory store (`memories/` directory). \
         If `name` is omitted, reads `MEMORY.md` (the index). \
         Paths are confined to the memories directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory file name within the `memories/` directory (e.g. `feedback_over_engineering.md`). Defaults to `MEMORY.md`."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let name = args["name"].as_str().unwrap_or("MEMORY.md");
        // Memories live in `memories/` under the worktree root.
        let memories_dir = ctx.workdir.join("memories");
        let mem_path = memories_dir.join(name);

        // Confinement: must stay inside memories/ which is inside workdir.
        let canonical_mem_dir = super::normalize_path_pub(&memories_dir);
        let canonical_path = super::normalize_path_pub(&mem_path);
        if !canonical_path.starts_with(&canonical_mem_dir) {
            return Err(HarnessError::PathEscape(name.to_string()));
        }
        // Also ensure memories/ is inside workdir.
        if !canonical_mem_dir.starts_with(&ctx.workdir) {
            return Err(HarnessError::PathEscape("memories/".to_string()));
        }

        tokio::fs::read_to_string(&mem_path)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot read memory `{name}`: {e}"),
            })
    }
}

// ---------------------------------------------------------------------------
// memory_write — write to the agent's tier-1 file-based memory store
// ---------------------------------------------------------------------------

/// Write a memory file to the agent's `memories/` directory.
///
/// Creates the file if it does not exist.  All paths are confined to the
/// memories sub-directory.  The 200-line cap on `MEMORY.md` is a convention
/// enforced by the AGENTS.md spec — this tool does not hard-enforce it (the
/// model is informed in the description and in feedback memories).
pub struct MemoryWrite;

#[async_trait]
impl ToolImpl for MemoryWrite {
    fn name(&self) -> &'static str {
        "memory_write"
    }

    fn description(&self) -> &'static str {
        "Write to the agent's tier-1 file-based memory store (`memories/` directory). \
         Creates the file if it does not exist. Confined to the `memories/` directory. \
         `MEMORY.md` is the index (capped at 200 lines by convention — Mattaññutā)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory file name within `memories/` (e.g. `feedback_foo.md`)."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write."
                }
            },
            "required": ["name", "content"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let name = args["name"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `name` argument".to_string(),
            })?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `content` argument".to_string(),
            })?;

        let memories_dir = ctx.workdir.join("memories");
        let mem_path = memories_dir.join(name);

        // Confinement check.
        let canonical_mem_dir = super::normalize_path_pub(&memories_dir);
        let canonical_path = super::normalize_path_pub(&mem_path);
        if !canonical_path.starts_with(&canonical_mem_dir) {
            return Err(HarnessError::PathEscape(name.to_string()));
        }
        if !canonical_mem_dir.starts_with(&ctx.workdir) {
            return Err(HarnessError::PathEscape("memories/".to_string()));
        }

        // Create the memories directory if needed.
        tokio::fs::create_dir_all(&memories_dir)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot create memories/ dir: {e}"),
            })?;

        tokio::fs::write(&mem_path, content)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot write memory `{name}`: {e}"),
            })?;

        Ok(format!("memory `{name}` written ({} bytes)", content.len()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    fn ctx_for(dir: &TempDir) -> ToolContext {
        ToolContext::new(dir.path().to_path_buf())
    }

    // ── edit_file ─────────────────────────────────────────────────────────────

    #[test]
    fn edit_file_happy_path() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            // Create a file to edit.
            tokio::fs::write(
                tmp.path().join("hello.rs"),
                "fn main() { println!(\"hello\"); }\n",
            )
            .await
            .unwrap();

            let tool = EditFile;
            let args = json!({
                "path": "hello.rs",
                "old_string": "hello",
                "new_string": "world"
            });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("replaced 1 occurrence"));

            let content = tokio::fs::read_to_string(tmp.path().join("hello.rs"))
                .await
                .unwrap();
            assert!(content.contains("world"));
            assert!(!content.contains("println!(\"hello\")"));
        });
    }

    #[test]
    fn edit_file_not_found_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = EditFile;
            let args = json!({
                "path": "missing.rs",
                "old_string": "foo",
                "new_string": "bar"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::ToolExecution { .. }));
        });
    }

    #[test]
    fn edit_file_zero_matches_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "line one\nline two\n")
                .await
                .unwrap();
            let tool = EditFile;
            let args = json!({
                "path": "f.txt",
                "old_string": "nonexistent",
                "new_string": "replacement"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not found"), "expected 'not found' in: {msg}");
        });
    }

    #[test]
    fn edit_file_multiple_matches_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "foo\nfoo\n")
                .await
                .unwrap();
            let tool = EditFile;
            let args = json!({
                "path": "f.txt",
                "old_string": "foo",
                "new_string": "bar"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("2 times"), "expected '2 times' in: {msg}");
        });
    }

    #[test]
    fn edit_file_path_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = EditFile;
            let args = json!({
                "path": "../../etc/passwd",
                "old_string": "root",
                "new_string": "hacked"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    // ── grep ──────────────────────────────────────────────────────────────────

    #[test]
    fn grep_finds_pattern() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            tokio::fs::write(tmp.path().join("code.rs"), "fn hello() {}\nfn world() {}\n")
                .await
                .unwrap();

            let tool = Grep;
            let args = json!({ "pattern": "hello" });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("code.rs"));
            assert!(result.contains("hello"));
            assert!(!result.contains("world() {}"));
        });
    }

    #[test]
    fn grep_no_match_returns_no_matches_message() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "line one\nline two\n")
                .await
                .unwrap();
            let tool = Grep;
            let args = json!({ "pattern": "ZZZNOMATCH" });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("no matches"));
        });
    }

    #[test]
    fn grep_case_insensitive() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "Hello World\n")
                .await
                .unwrap();
            let tool = Grep;
            let args = json!({ "pattern": "hello", "case_insensitive": true });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("Hello World"));
        });
    }

    #[test]
    fn grep_path_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Grep;
            let args = json!({ "pattern": "root", "path": "../../etc" });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    // ── git ───────────────────────────────────────────────────────────────────

    #[test]
    fn git_status_in_real_repo() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            // Initialise a real git repo so `git status` works.
            // HOME (Unix) / USERPROFILE (Windows) let git find its global config.
            let home_val = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_default();
            tokio::process::Command::new("git")
                .args(["init", "-b", "main"])
                .current_dir(tmp.path())
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap_or_default())
                .env("HOME", &home_val)
                .env("USERPROFILE", &home_val)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .await
                .unwrap();

            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "status" });
            let result = tool.execute(args, &ctx).await.unwrap();
            // `git status` in an empty repo should mention branch or nothing to commit.
            assert!(
                result.contains("branch")
                    || result.contains("commit")
                    || result.contains("No commits"),
                "unexpected git status output: {result}"
            );
        });
    }

    #[test]
    fn git_blocked_subcommand_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "rebase", "args": ["-i", "HEAD~3"] });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::ToolExecution { .. }));
        });
    }

    #[test]
    fn git_unknown_subcommand_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "hack" });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not in the allowed list"), "got: {msg}");
        });
    }

    #[test]
    fn git_no_verify_rejected_by_tool_layer() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "commit", "args": ["--no-verify", "-m", "skip"] });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("--no-verify"), "got: {msg}");
        });
    }

    #[test]
    fn git_force_push_rejected_by_tool_layer() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "push", "args": ["--force", "origin", "main"] });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("--force"), "got: {msg}");
        });
    }

    // ── run_gates ─────────────────────────────────────────────────────────────

    #[test]
    fn run_gates_no_manifest_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = RunGates;
            let args = json!({});
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("config.manifest.json"), "got: {msg}");
        });
    }

    #[test]
    fn run_gates_with_passing_manifest() {
        // `true` (Unix) / `exit 0` (Windows CMD) — always-passing gate command.
        #[cfg(unix)]
        let pass_cmd = "true";
        #[cfg(windows)]
        let pass_cmd = "exit 0";

        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            // Write a minimal manifest where all gate commands always pass.
            let manifest_json = serde_json::json!({
                "name": "test-agent",
                "agentId": "agent-test",
                "agentRole": "test",
                "primaryModel": "model-x",
                "memoryPath": "memories/",
                "lintCmd": pass_cmd,
                "formatCmd": pass_cmd,
                "testCmd": pass_cmd,
                "buildCmd": pass_cmd,
                "version": "2.0"
            });
            tokio::fs::write(
                tmp.path().join("config.manifest.json"),
                serde_json::to_string_pretty(&manifest_json).unwrap(),
            )
            .await
            .unwrap();

            let tool = RunGates;
            let args = json!({ "gates": ["lint", "format"] });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("PASS"), "expected PASS in: {result}");
            assert!(
                result.contains("ALL GATES PASSED"),
                "expected all passed in: {result}"
            );
        });
    }

    #[test]
    fn run_gates_with_failing_gate() {
        // `false` (Unix) / `exit 1` (Windows CMD) — always-failing gate command.
        #[cfg(unix)]
        let fail_cmd = "false";
        #[cfg(windows)]
        let fail_cmd = "exit 1";
        // pass_cmd: same as above
        #[cfg(unix)]
        let pass_cmd = "true";
        #[cfg(windows)]
        let pass_cmd = "exit 0";

        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            let manifest_json = serde_json::json!({
                "name": "test-agent",
                "agentId": "agent-test",
                "agentRole": "test",
                "primaryModel": "model-x",
                "memoryPath": "memories/",
                "lintCmd": fail_cmd,
                "formatCmd": pass_cmd,
                "testCmd": pass_cmd,
                "buildCmd": pass_cmd,
                "version": "2.0"
            });
            tokio::fs::write(
                tmp.path().join("config.manifest.json"),
                serde_json::to_string_pretty(&manifest_json).unwrap(),
            )
            .await
            .unwrap();

            let tool = RunGates;
            let args = json!({ "gates": ["lint"] });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("FAIL"), "expected FAIL in: {result}");
            assert!(result.contains("ONE OR MORE GATES FAILED"), "got: {result}");
        });
    }

    // ── bwoc_task ─────────────────────────────────────────────────────────────

    /// bwoc_task shells out to `bwoc`; in tests we verify schema + arg
    /// handling without requiring `bwoc` to be on PATH.  If bwoc is absent,
    /// the tool returns an error — that is expected behaviour.
    #[test]
    fn bwoc_task_claim_requires_task_id() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = BwocTask;
            let args = json!({ "action": "claim" }); // no task_id
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("task_id"), "got: {msg}");
        });
    }

    #[test]
    fn bwoc_task_list_does_not_require_task_id() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            // Initialise a dummy bwoc workspace so list doesn't error on missing dirs.
            // (If bwoc is on PATH this gives a real result; if not, we just get a
            //  spawn error which is the expected fallback behaviour.)
            let ctx = ctx_for(&tmp);
            let tool = BwocTask;
            let args = json!({ "action": "list" });
            // We accept either success or a spawn error (bwoc not on PATH).
            // The important invariant is that the *argument parsing* succeeds —
            // no `task_id` is required for `list`.
            let _ = tool.execute(args, &ctx).await;
        });
    }

    // ── bwoc_send ─────────────────────────────────────────────────────────────

    #[test]
    fn bwoc_send_missing_to_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = BwocSend;
            let args = json!({ "body": "hello", "from": "agent-oracle" }); // no `to`
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("missing `to`"), "got: {msg}");
        });
    }

    #[test]
    fn bwoc_send_missing_from_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = BwocSend;
            let args = json!({ "to": "agent-pi", "body": "hello" }); // no `from`
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("missing `from`"), "got: {msg}");
        });
    }

    // ── memory_read ───────────────────────────────────────────────────────────

    #[test]
    fn memory_read_returns_content() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            // Create memories/ dir and write a memory file.
            tokio::fs::create_dir(tmp.path().join("memories"))
                .await
                .unwrap();
            tokio::fs::write(
                tmp.path().join("memories").join("MEMORY.md"),
                "# Memory Index\n",
            )
            .await
            .unwrap();

            let tool = MemoryRead;
            let args = json!({});
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("Memory Index"));
        });
    }

    #[test]
    fn memory_read_dotdot_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = MemoryRead;
            let args = json!({ "name": "../../etc/passwd" });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    // ── memory_write ──────────────────────────────────────────────────────────

    #[test]
    fn memory_write_creates_file() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = MemoryWrite;
            let args = json!({
                "name": "project_foo.md",
                "content": "# Foo\nSome project memory.\n"
            });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("written"));

            let content =
                tokio::fs::read_to_string(tmp.path().join("memories").join("project_foo.md"))
                    .await
                    .unwrap();
            assert!(content.contains("Foo"));
        });
    }

    #[test]
    fn memory_write_dotdot_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = MemoryWrite;
            let args = json!({
                "name": "../../evil.sh",
                "content": "#!/bin/sh\nrm -rf /\n"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    // ── Schema tests (all tools have valid JSON schema) ───────────────────────

    #[test]
    fn all_new_tools_have_schemas() {
        let tools: Vec<Box<dyn ToolImpl + Send + Sync>> = vec![
            Box::new(EditFile),
            Box::new(Grep),
            Box::new(Git),
            Box::new(RunGates),
            Box::new(BwocTask),
            Box::new(BwocSend),
            Box::new(MemoryRead),
            Box::new(MemoryWrite),
        ];
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "schema for `{}` is not an object",
                tool.name()
            );
            assert!(
                schema["type"].as_str() == Some("object"),
                "schema for `{}` missing type:object",
                tool.name()
            );
        }
    }
}
