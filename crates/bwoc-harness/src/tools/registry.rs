//! Tool registry — maps tool names to implementations and builds the schema
//! list passed to the model.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::ToolImpl;
use crate::error::HarnessError;
use crate::provider::Tool;

/// Registry of available tools.
///
/// Holds `Arc<dyn ToolImpl>` so the registry is cheaply cloneable.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ToolImpl>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool implementation.
    pub fn register(&mut self, tool: impl ToolImpl + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    /// Return the OpenAI-compat tool schema list for the chat completion request.
    pub fn tool_schemas(&self) -> Vec<Tool> {
        self.tools
            .values()
            .map(|t| Tool::function(t.name(), t.description(), t.parameters_schema()))
            .collect()
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn ToolImpl>> {
        self.tools.get(name).cloned()
    }

    /// The names of every registered tool. Used by the turn-executor to decide
    /// which tools are marshallable across the process boundary (Phase 5 t5).
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the default registry with the full BWOC tool set.
///
/// P1 tools: `read_file`, `write_file`, `list_dir`, `run_command`
/// Complete tool set: `edit_file`, `grep`, `git`, `run_gates`, `bwoc_task`,
/// `bwoc_send`, `bwoc_run`, `memory_read`, `memory_write`
pub fn default_registry() -> ToolRegistry {
    use super::extra_tools::{
        BwocRun, BwocSend, BwocTask, EditFile, Git, Grep, MemoryRead, MemoryWrite, RunGates,
    };
    use super::impls::{ListDir, ReadFile, RunCommand, WriteFile};
    let mut reg = ToolRegistry::new();
    // P1 core tools
    reg.register(ReadFile);
    reg.register(WriteFile);
    reg.register(ListDir);
    reg.register(RunCommand);
    // Complete tool set
    reg.register(EditFile);
    reg.register(Grep);
    reg.register(Git);
    reg.register(RunGates);
    reg.register(BwocTask);
    reg.register(BwocSend);
    reg.register(BwocRun);
    reg.register(MemoryRead);
    reg.register(MemoryWrite);
    // Computer-use (headless browser) — opt-in via the `browser` feature; the
    // browser launches lazily on first use. Registration only exposes the tool;
    // the policy pipeline gates it (`computer` is `Gated` → refused on untrusted
    // turns, ask-by-default, autoprocess-refused). Default builds pull zero
    // browser deps and do not register it.
    #[cfg(feature = "browser")]
    {
        use super::browser::LazyBrowserExecutor;
        use super::computer::ComputerTool;
        reg.register(ComputerTool::new(LazyBrowserExecutor::new("about:blank")));
    }
    reg
}

/// Dispatch a single tool call to the registry.
///
/// Parses the JSON arguments string, executes the tool, and returns the string
/// result.  On failure, returns the error message as the tool result so the
/// model can react (e.g., retry with different args) rather than crashing.
pub async fn dispatch(
    registry: &ToolRegistry,
    tool_name: &str,
    arguments_json: &str,
    ctx: &super::ToolContext,
) -> String {
    dispatch_rich(registry, tool_name, arguments_json, ctx)
        .await
        .content
}

/// Like [`dispatch`] but returns the full [`ToolOutput`] (text + any images),
/// so a visual tool's screenshot survives to the `tool_result`. Errors are
/// returned as text-only output (no images), same as [`dispatch`].
pub async fn dispatch_rich(
    registry: &ToolRegistry,
    tool_name: &str,
    arguments_json: &str,
    ctx: &super::ToolContext,
) -> super::ToolOutput {
    let tool = match registry.get(tool_name) {
        Some(t) => t,
        None => {
            return super::ToolOutput::text(format!(
                "error: unknown tool `{tool_name}`. Available: {}",
                registry
                    .tools
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    };

    let args: Value = match serde_json::from_str(arguments_json) {
        Ok(v) => v,
        Err(e) => {
            return super::ToolOutput::text(format!(
                "error: failed to parse arguments for `{tool_name}`: {e}. Arguments were: {arguments_json}"
            ));
        }
    };

    match tool.execute_rich(args, ctx).await {
        Ok(mut output) => {
            // Clamp once, here — the single seam every tool (and every MCP tool)
            // passes through, so none can flood the context. Images are untouched
            // (bounded elsewhere); only the text budget applies (#478).
            output.content = super::clamp_tool_output(output.content);
            output
        }
        Err(HarnessError::PathEscape(p)) => super::ToolOutput::text(format!(
            "error: path `{p}` is outside the allowed working directory"
        )),
        Err(e) => super::ToolOutput::text(format!("error: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_ctx(dir: &TempDir) -> super::super::ToolContext {
        super::super::ToolContext::new(dir.path().to_path_buf())
    }

    #[test]
    fn registry_has_full_tool_set() {
        let reg = default_registry();
        let schemas = reg.tool_schemas();
        let names: Vec<&str> = schemas.iter().map(|t| t.function.name.as_str()).collect();
        // P1 core tools
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"list_dir"));
        assert!(names.contains(&"run_command"));
        // Complete tool set
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"grep"));
        assert!(names.contains(&"git"));
        assert!(names.contains(&"run_gates"));
        assert!(names.contains(&"bwoc_task"));
        assert!(names.contains(&"bwoc_send"));
        assert!(names.contains(&"memory_read"));
        assert!(names.contains(&"memory_write"));
    }

    #[tokio::test]
    async fn dispatch_unknown_tool() {
        let reg = default_registry();
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let result = dispatch(&reg, "nonexistent_tool", "{}", &ctx).await;
        assert!(result.contains("unknown tool"));
    }

    #[tokio::test]
    async fn dispatch_bad_json_args() {
        let reg = default_registry();
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let result = dispatch(&reg, "read_file", "not-json", &ctx).await;
        assert!(result.contains("parse arguments"));
    }

    #[tokio::test]
    async fn dispatch_path_escape_returns_error_string() {
        let reg = default_registry();
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(&tmp);
        let result = dispatch(&reg, "read_file", r#"{"path": "../../etc/passwd"}"#, &ctx).await;
        assert!(result.contains("outside the allowed working directory"));
    }

    #[tokio::test]
    async fn dispatch_unconfined_reads_outside_workdir() {
        // `--unrestricted` end-to-end at the dispatch layer: an unconfined ctx
        // reads an absolute path outside the workdir that a confined ctx rejects.
        let reg = default_registry();
        let workdir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let probe = outside.path().join("probe.txt");
        std::fs::write(&probe, "secret-outside").unwrap();
        let args = format!(r#"{{"path": {:?}}}"#, probe.to_str().unwrap());

        // Confined: rejected.
        let confined = super::super::ToolContext::new(workdir.path().to_path_buf());
        let rej = dispatch(&reg, "read_file", &args, &confined).await;
        assert!(rej.contains("outside the allowed working directory"));

        // Unconfined: the file's contents come back.
        let unconfined = super::super::ToolContext::unconfined(workdir.path().to_path_buf());
        let ok = dispatch(&reg, "read_file", &args, &unconfined).await;
        assert_eq!(ok, "secret-outside");
    }
}
