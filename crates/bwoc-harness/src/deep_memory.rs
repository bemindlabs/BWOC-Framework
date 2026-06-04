//! Tier 2 deep-memory integration (HV3-1) — the harness side of the
//! `bwoc-core::deep_memory` contract.
//!
//! When the agent's manifest configures `deepMemoryCmd`, the harness:
//!
//! 1. runs `<cmd> wake-up` at session start and appends the output to the
//!    system prompt (a "Prior context" block) — both the batch loop and
//!    `--chat`;
//! 2. registers a `memory_search` tool (`<cmd> search "<q>"`) so the model
//!    can recall past decisions mid-run — read-only, and like every tool it
//!    flows through the guardrails → permission pipeline;
//! 3. runs `<cmd> mine <artifact> --mode <run|chat>` at session end so the
//!    session's own history becomes tomorrow's memory.
//!
//! All of it is **best-effort and non-fatal**: an absent/placeholder
//! `deepMemoryCmd` means none of this happens (Tier 1 alone keeps working),
//! and a failing tool invocation degrades to a warning. Every subprocess call
//! is bounded by a timeout so a hung memory backend can never stall a run.
//! Async throughout (`tokio::process`) to match the harness runtime — the
//! sync `bwoc-core::deep_memory::ShellDeepMemory` stays the CLI's client.

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::HarnessError;
use crate::tools::{ToolContext, ToolImpl};

/// Bound on `wake-up` (session start must stay snappy).
const WAKE_UP_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound on `search` (a tool call inside a turn).
const SEARCH_TIMEOUT: Duration = Duration::from_secs(15);
/// Bound on `mine` (session end; embedding a session can take a moment).
const MINE_TIMEOUT: Duration = Duration::from_secs(60);

/// A resolved, non-placeholder `deepMemoryCmd` from the agent's manifest.
#[derive(Debug, Clone)]
pub struct DeepMemoryCmd {
    cmd: String,
}

impl DeepMemoryCmd {
    /// Load `deepMemoryCmd` from `<workdir>/config.manifest.json`. `None` when
    /// the manifest is absent/unreadable, the field is unset/empty, or it is
    /// the `bwoc new` placeholder — Tier 2 is strictly opt-in.
    pub fn from_workdir(workdir: &Path) -> Option<Self> {
        let m =
            bwoc_core::manifest::Manifest::load_from_path(&workdir.join("config.manifest.json"))
                .ok()?;
        Self::from_cmd(m.deep_memory_cmd.as_deref())
    }

    /// Same filtering rules as `bwoc-core::deep_memory::from_manifest_cmd`.
    pub fn from_cmd(cmd: Option<&str>) -> Option<Self> {
        match cmd {
            None => None,
            Some(s)
                if s.trim().is_empty()
                    || s.trim() == bwoc_core::deep_memory::UNCONFIGURED_PLACEHOLDER =>
            {
                None
            }
            Some(s) => Some(Self { cmd: s.to_string() }),
        }
    }

    /// Split the configured command into `(program, leading-args)` —
    /// `deepMemoryCmd` may be `"bwoc-deep-memory --db x"` as well as a bare
    /// binary name (same convention as the core contract).
    fn argv(&self) -> Option<(String, Vec<String>)> {
        let mut parts = self.cmd.split_whitespace();
        let program = parts.next()?.to_string();
        Some((program, parts.map(str::to_string).collect()))
    }

    /// Run `<cmd> <sub-args…>` with a timeout; `Ok(stdout)` on exit 0.
    async fn invoke(&self, sub_args: &[&str], timeout: Duration) -> Result<String, String> {
        let Some((program, mut args)) = self.argv() else {
            return Err("deepMemoryCmd is empty".to_string());
        };
        args.extend(sub_args.iter().map(|s| s.to_string()));

        let fut = tokio::process::Command::new(&program).args(&args).output();
        let output = tokio::time::timeout(timeout, fut)
            .await
            .map_err(|_| format!("`{program}` timed out after {}s", timeout.as_secs()))?
            .map_err(|e| format!("failed to spawn `{program}`: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "`{program}` exited {}: {}",
                output.status.code().unwrap_or(-1),
                stderr.trim()
            ))
        }
    }

    /// `wake-up`: prior context for session start. `None` on any failure or
    /// empty output — callers inject nothing rather than noise.
    pub async fn wake_up(&self) -> Option<String> {
        match self.invoke(&["wake-up"], WAKE_UP_TIMEOUT).await {
            Ok(out) if !out.trim().is_empty() => Some(out.trim().to_string()),
            Ok(_) => None,
            Err(e) => {
                eprintln!("[bwoc-harness] warning: deep-memory wake-up: {e}");
                None
            }
        }
    }

    /// `search "<query>"`: semantic recall mid-run (powers `memory_search`).
    pub async fn search(&self, query: &str) -> Result<String, String> {
        self.invoke(&["search", query], SEARCH_TIMEOUT).await
    }

    /// `mine <artifact> --mode <mode>`: persist this session's history at
    /// session end. Best-effort — a warning, never an error, so memory
    /// failures can't fail an otherwise-successful run.
    pub async fn mine(&self, artifact: &Path, mode: &str) {
        if !artifact.exists() {
            return;
        }
        let artifact_str = artifact.to_string_lossy().into_owned();
        match self
            .invoke(&["mine", &artifact_str, "--mode", mode], MINE_TIMEOUT)
            .await
        {
            Ok(report) => {
                let line = report.trim();
                if !line.is_empty() {
                    eprintln!("[bwoc-harness] deep-memory: {line}");
                }
            }
            Err(e) => eprintln!("[bwoc-harness] warning: deep-memory mine: {e}"),
        }
    }
}

/// Format wake-up output as the system-prompt block both paths append.
pub fn wake_up_block(text: &str) -> String {
    format!("\n\n## Prior context (Tier 2 memory)\n\n{text}\n")
}

// ---------------------------------------------------------------------------
// memory_search — Tier 2 semantic recall as a tool
// ---------------------------------------------------------------------------

/// Search the agent's Tier 2 deep-memory store. Registered only when the
/// manifest configures `deepMemoryCmd`; read-only (the backend's `search`
/// verb), so chat's default policy allows it alongside `memory_read`.
pub struct MemorySearch {
    dm: DeepMemoryCmd,
}

impl MemorySearch {
    pub fn new(dm: DeepMemoryCmd) -> Self {
        Self { dm }
    }
}

#[async_trait]
impl ToolImpl for MemorySearch {
    fn name(&self) -> &'static str {
        "memory_search"
    }

    fn description(&self) -> &'static str {
        "Semantic search over the agent's Tier 2 deep-memory store (past \
         decisions, prior sessions). Use when the task references earlier \
         work or you need context that predates this session. Read-only."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to recall — phrased as the decision/topic you are looking for."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<String, HarnessError> {
        let query = args["query"].as_str().unwrap_or_default();
        if query.trim().is_empty() {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "`query` is required".to_string(),
            });
        }
        match self.dm.search(query).await {
            Ok(out) if out.trim().is_empty() => Ok("(no matching memories)".to_string()),
            Ok(out) => Ok(out),
            Err(e) => Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: e,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_cmd_filters_placeholder_and_empty() {
        assert!(DeepMemoryCmd::from_cmd(None).is_none());
        assert!(DeepMemoryCmd::from_cmd(Some("")).is_none());
        assert!(DeepMemoryCmd::from_cmd(Some("   ")).is_none());
        assert!(
            DeepMemoryCmd::from_cmd(Some(bwoc_core::deep_memory::UNCONFIGURED_PLACEHOLDER))
                .is_none()
        );
        assert!(DeepMemoryCmd::from_cmd(Some("bwoc-deep-memory --db x")).is_some());
    }

    #[test]
    fn argv_splits_program_and_leading_args() {
        let dm = DeepMemoryCmd::from_cmd(Some("bwoc-deep-memory --db /tmp/m.db")).unwrap();
        let (prog, args) = dm.argv().unwrap();
        assert_eq!(prog, "bwoc-deep-memory");
        assert_eq!(args, vec!["--db", "/tmp/m.db"]);
    }

    #[test]
    fn wake_up_block_shapes_the_header() {
        let b = wake_up_block("remembered thing");
        assert!(b.contains("## Prior context (Tier 2 memory)"));
        assert!(b.contains("remembered thing"));
    }

    // Exec-path tests use `sh -c` and are Unix-only; the windows CI leg covers
    // compile via --all-targets and the logic above is platform-free.
    #[cfg(unix)]
    #[tokio::test]
    async fn wake_up_returns_trimmed_stdout() {
        let dm = DeepMemoryCmd::from_cmd(Some("echo prior-context")).unwrap();
        // `echo wake-up`'s output contains the sub-command itself — fine: we
        // only assert plumbing, not the reference backend's semantics.
        let out = dm.wake_up().await.expect("echo always succeeds");
        assert!(out.contains("prior-context"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failing_cmd_is_non_fatal_for_wake_up_and_err_for_search() {
        let dm = DeepMemoryCmd::from_cmd(Some("false")).unwrap();
        assert!(dm.wake_up().await.is_none());
        assert!(dm.search("anything").await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_search_tool_roundtrip() {
        let dm = DeepMemoryCmd::from_cmd(Some("echo")).unwrap();
        let tool = MemorySearch::new(dm);
        let ctx = ToolContext::new(std::env::temp_dir());
        let out = tool
            .execute(json!({"query": "tls decision"}), &ctx)
            .await
            .unwrap();
        assert!(out.contains("tls decision"));

        let err = tool.execute(json!({}), &ctx).await;
        assert!(err.is_err(), "missing query must error");
    }
}
