//! `bwoc run <agent> --task "<prompt>"` — headless single-task agent invocation.
//!
//! Dispatches one task to a named agent **non-interactively** and reports a
//! structured result.  Closes the gap that prevents orchestrators and CI from
//! dispatching a task and getting a result back (issue #5).
//!
//! ## Backend dispatch
//!
//! | Backend     | Command                                                  | Headless? |
//! |-------------|----------------------------------------------------------|-----------|
//! | `claude`    | `claude -p "<task>"`                                     | Real       |
//! | `copilot`   | `copilot -p "<task>" --no-ask-user`                      | Real       |
//! | `ollama`    | `bwoc-harness --workdir <dir> --task "<task>" --model <model>` | Real |
//! | `codex`     | `RunError::HeadlessUnsupported`                          | Deferred  |
//! | `agy`       | `RunError::HeadlessUnsupported`                          | Deferred  |
//! | `kimi`      | `RunError::HeadlessUnsupported`                          | Deferred  |
//!
//! `codex`, `agy`, and `kimi` each have interactive-only or OAuth-gated CLIs
//! at the time of writing; no confirmed non-interactive exec flag exists, so
//! we surface `HeadlessUnsupported` rather than fabricate an argument.
//!
//! ## Test seam
//!
//! `CommandRunner` is a trait over the `std::process::Command`-build-and-execute
//! path.  Unit tests inject `MockCommandRunner` to verify backend dispatch and
//! argument construction without forking real CLIs.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;

use crate::spawn::Backend;

// ── Public args ──────────────────────────────────────────────────────────────

pub struct RunArgs {
    /// Agent name ("foo" or "agent-foo").
    pub agent: String,
    /// The task prompt to deliver to the agent.
    pub task: String,
    /// If true, emit JSON result to stdout instead of human text + status footer.
    pub json: bool,
    /// Optional hard kill timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Workspace override — resolution follows the standard chain when `None`.
    pub workspace: Option<PathBuf>,
    /// Working directory for the run. `None` (default) jails the agent to its own
    /// directory — the safe, blast-radius-minimal default. `Some(p)` runs from
    /// `p` (relative resolved against the workspace root) so a task can touch
    /// shared workspace files (`projects/`, `wiki/`); the resolved path must be an
    /// existing directory **inside** the workspace root — escaping is refused.
    pub workdir: Option<PathBuf>,
}

// ── Result types ─────────────────────────────────────────────────────────────

/// Structured result of one headless agent run.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub agent: String,
    pub backend: String,
    pub task: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    /// Combined stdout + stderr captured from the child process.
    pub output: String,
}

impl RunResult {
    /// Serialize to the canonical `--json` shape.
    pub fn to_json(&self) -> String {
        // Build JSON manually — keeps bwoc-cli dep-lean (no extra serde derives needed).
        let agent = json_escape(&self.agent);
        let backend = json_escape(&self.backend);
        let task = json_escape(&self.task);
        let output = json_escape(&self.output);
        format!(
            "{{\n  \"agent\": \"{agent}\",\n  \"backend\": \"{backend}\",\
            \n  \"task\": \"{task}\",\n  \"exit_code\": {},\
            \n  \"duration_ms\": {},\n  \"output\": \"{output}\"\n}}",
            self.exit_code, self.duration_ms
        )
    }
}

// ── Error types ───────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
    )]
    NoWorkspace,
    #[error("failed to read agents.toml: {0}")]
    RegistryLoad(String),
    #[error("no agent named '{0}' in workspace. Try `bwoc list`.")]
    AgentNotFound(String),
    #[error("agent '{agent}' has unknown backend '{backend}' — edit .bwoc/agents.toml")]
    UnknownBackend { agent: String, backend: String },
    #[error("agent manifest not found at {0}: {1}")]
    ManifestMissing(PathBuf, String),
    #[error(
        "backend '{backend}' has no confirmed non-interactive exec flag; \
         headless invocation is not yet supported for this backend"
    )]
    HeadlessUnsupported { backend: String },
    #[error(
        "bwoc-harness binary not found; install it \
         (`cargo install --path crates/bwoc-harness`) or add it to PATH"
    )]
    HarnessNotFound,
    #[error(
        "backend `openai-compatible` requires a `\"baseUrl\"` field in config.manifest.json; \
         none found in {0}"
    )]
    MissingBaseUrl(PathBuf),
    #[error("failed to start agent process: {0}")]
    Io(#[from] io::Error),
    #[error("task timed out after {secs}s")]
    Timeout { secs: u64 },
    #[error(
        "--workdir {0} is not a directory inside the workspace \
         (relative paths resolve against the workspace root; escaping it is refused)"
    )]
    BadWorkdir(PathBuf),
}

// ── CommandRunner seam ────────────────────────────────────────────────────────

/// Result of launching and waiting for a child process.
pub struct CommandOutcome {
    pub exit_code: i32,
    pub output: String,
}

/// Abstraction over the launch-and-capture path.  `std::process::Command` in
/// production; `MockCommandRunner` in unit tests.
pub trait CommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        timeout_secs: Option<u64>,
    ) -> Result<CommandOutcome, RunError>;
}

// ── Real runner (production) ──────────────────────────────────────────────────

pub struct ProcessCommandRunner;

impl CommandRunner for ProcessCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        timeout_secs: Option<u64>,
    ) -> Result<CommandOutcome, RunError> {
        use std::process::{Command, Stdio};

        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    // Remap to a friendlier Io error; callers that care about
                    // harness-not-found check upstream before reaching here.
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("'{program}' not found on PATH: {e}"),
                    )
                } else {
                    e
                }
            })?;

        let child_id = child.id();

        // Spawn a watcher thread if --timeout is set.
        let timeout_reached = if let Some(secs) = timeout_secs {
            let deadline = Instant::now() + Duration::from_secs(secs);
            // We poll rather than SIGKILL immediately to give the process a
            // moment to flush buffers.  Poll granularity: 100 ms.
            let mut killed = false;
            loop {
                if Instant::now() >= deadline {
                    // Kill the process group so forked children also die.
                    kill_child(child_id);
                    let _ = child.wait(); // reap the zombie
                    killed = true;
                    break;
                }
                // Try a non-blocking wait.
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                    Err(_) => break,
                }
            }
            killed
        } else {
            false
        };

        if timeout_reached {
            return Err(RunError::Timeout {
                secs: timeout_secs.unwrap_or(0),
            });
        }

        let output = child.wait_with_output()?;
        let exit_code = output.status.code().unwrap_or(1);
        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok(CommandOutcome {
            exit_code,
            output: combined,
        })
    }
}

// ── Platform kill helper ──────────────────────────────────────────────────────

/// Best-effort SIGKILL / TerminateProcess for the process group.
#[cfg(unix)]
fn kill_child(pid: u32) {
    unsafe {
        // Negative pid = process group.
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        // Fall back to single-process kill if pgid send fails.
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_child(pid: u32) {
    // On Windows, TerminateProcess is the equivalent. We use `taskkill /F /T`
    // which terminates the process tree. Best-effort; ignore errors.
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output();
}

// ── Core dispatch ─────────────────────────────────────────────────────────────

/// Build the (program, args) pair for a headless agent run.
///
/// Returns `Err(RunError::HeadlessUnsupported)` for backends with no confirmed
/// non-interactive flag.  Returns `Err(RunError::HarnessNotFound)` when the
/// Ollama harness cannot be located.
/// `config_dir` is always the agent's own directory — the manifest
/// (`config.manifest.json`) is the agent's identity and never moves. `work_dir`
/// is where the agent *runs* (its process cwd + the harness `--workdir` FS-jail
/// root); it equals `config_dir` unless the caller widened it via `--workdir`.
pub fn build_command(
    backend: Backend,
    config_dir: &Path,
    work_dir: &Path,
    task: &str,
    primary_model: &str,
) -> Result<(String, Vec<String>), RunError> {
    match backend {
        Backend::Claude => {
            // `claude -p "<task>"` — Claude Code's print/headless mode.
            let mut args = vec!["-p".to_string()];
            // Pass through the manifest's reasoningEffort as `--effort <level>`
            // (Claude Opus 4.8 effort control: low|medium|high|xhigh|max).
            // Absent manifest / field leaves Claude on its default effort.
            let manifest_path = config_dir.join("config.manifest.json");
            if let Ok(m) = Manifest::load_from_path(&manifest_path) {
                if let Some(effort) = m.reasoning_effort {
                    args.push("--effort".to_string());
                    args.push(effort);
                }
            }
            args.push(task.to_string());
            Ok(("claude".to_string(), args))
        }
        Backend::Ollama => {
            let harness = Backend::harness_binary().ok_or(RunError::HarnessNotFound)?;
            let mut args = vec![
                "--workdir".to_string(),
                work_dir.to_string_lossy().into_owned(),
                "--task".to_string(),
                task.to_string(),
                "--model".to_string(),
                primary_model.to_string(),
            ];
            // Honour baseUrl from config.manifest.json when present.
            let manifest_path = config_dir.join("config.manifest.json");
            if let Ok(m) = Manifest::load_from_path(&manifest_path) {
                if let Some(url) = m.base_url {
                    args.push("--endpoint".to_string());
                    args.push(url);
                }
            }
            Ok((harness.to_string_lossy().into_owned(), args))
        }
        Backend::OpenAiCompatible => {
            let harness = Backend::harness_binary().ok_or(RunError::HarnessNotFound)?;
            // baseUrl is required for openai-compatible.
            let manifest_path = config_dir.join("config.manifest.json");
            let base_url = Manifest::load_from_path(&manifest_path)
                .ok()
                .and_then(|m| m.base_url)
                .ok_or(RunError::MissingBaseUrl(manifest_path))?;
            let args = vec![
                "--workdir".to_string(),
                work_dir.to_string_lossy().into_owned(),
                "--task".to_string(),
                task.to_string(),
                "--model".to_string(),
                primary_model.to_string(),
                "--endpoint".to_string(),
                base_url,
            ];
            Ok((harness.to_string_lossy().into_owned(), args))
        }
        Backend::OpenRouter => {
            let harness = Backend::harness_binary().ok_or(RunError::HarnessNotFound)?;
            // `--backend openrouter` is required so the harness attaches bearer
            // auth; without it requests hit the unauthenticated OpenAI-compat
            // path and 401. baseUrl is optional (harness defaults to
            // https://openrouter.ai/api/v1).
            let mut args = vec![
                "--workdir".to_string(),
                work_dir.to_string_lossy().into_owned(),
                "--task".to_string(),
                task.to_string(),
                "--model".to_string(),
                primary_model.to_string(),
                "--backend".to_string(),
                "openrouter".to_string(),
            ];
            let manifest_path = config_dir.join("config.manifest.json");
            if let Ok(m) = Manifest::load_from_path(&manifest_path) {
                if let Some(url) = m.base_url {
                    args.push("--endpoint".to_string());
                    args.push(url);
                }
            }
            Ok((harness.to_string_lossy().into_owned(), args))
        }
        Backend::LiteLlm => {
            let harness = Backend::harness_binary().ok_or(RunError::HarnessNotFound)?;
            // `--backend litellm` is required so the harness resolves the LiteLLM
            // base (`--endpoint` / `LITELLM_API_BASE`) and attaches the optional
            // bearer key. baseUrl is optional (harness falls back to
            // `LITELLM_API_BASE` env or the LiteLLM default port).
            let mut args = vec![
                "--workdir".to_string(),
                work_dir.to_string_lossy().into_owned(),
                "--task".to_string(),
                task.to_string(),
                "--model".to_string(),
                primary_model.to_string(),
                "--backend".to_string(),
                "litellm".to_string(),
            ];
            let manifest_path = config_dir.join("config.manifest.json");
            if let Ok(m) = Manifest::load_from_path(&manifest_path) {
                // Skip a blank/whitespace baseUrl so we never forward an empty
                // `--endpoint` that would defeat the harness's env/default base.
                if let Some(url) = m.base_url.filter(|u| !u.trim().is_empty()) {
                    args.push("--endpoint".to_string());
                    args.push(url.trim().to_string());
                }
            }
            Ok((harness.to_string_lossy().into_owned(), args))
        }
        Backend::Copilot => {
            // `copilot -p "<task>" --no-ask-user` — Copilot CLI's programmatic
            // mode. `--no-ask-user` is required headless (no TTY to answer
            // approval prompts); tool calls that would ask are refused rather
            // than hanging the run. The permissive `--allow-all-tools` is
            // deliberately NOT passed — GitHub's own guidance scopes it to
            // sandboxed/container environments (matches our fail-safe posture).
            Ok((
                "copilot".to_string(),
                vec![
                    "-p".to_string(),
                    task.to_string(),
                    "--no-ask-user".to_string(),
                ],
            ))
        }
        Backend::Codex => Err(RunError::HeadlessUnsupported {
            backend: "codex".to_string(),
        }),
        Backend::Antigravity => Err(RunError::HeadlessUnsupported {
            backend: "agy".to_string(),
        }),
        Backend::Kimi => Err(RunError::HeadlessUnsupported {
            backend: "kimi".to_string(),
        }),
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Entry point called from `main.rs` — returns process exit code.
pub fn run(args: RunArgs) -> i32 {
    run_with(args, &ProcessCommandRunner)
}

/// Testable entry point accepting a `CommandRunner` impl.
pub fn run_with(args: RunArgs, runner: &dyn CommandRunner) -> i32 {
    match execute(args, runner) {
        Ok((result, json)) => {
            if json {
                println!("{}", result.to_json());
            } else {
                // Print agent output, then a one-line status footer.
                print!("{}", result.output);
                println!(
                    "\n--- bwoc run: agent={} backend={} exit={} ({} ms) ---",
                    result.agent, result.backend, result.exit_code, result.duration_ms
                );
            }
            if result.exit_code == 0 { 0 } else { 1 }
        }
        Err(RunError::HeadlessUnsupported { backend }) => {
            eprintln!("bwoc run: {}", RunError::HeadlessUnsupported { backend });
            3
        }
        Err(RunError::Timeout { secs }) => {
            eprintln!("bwoc run: {}", RunError::Timeout { secs });
            4
        }
        Err(e) => {
            eprintln!("bwoc run: {e}");
            2
        }
    }
}

/// Core logic — separated so tests can call it directly.
pub fn execute(args: RunArgs, runner: &dyn CommandRunner) -> Result<(RunResult, bool), RunError> {
    let workspace = resolve_workspace(args.workspace).ok_or(RunError::NoWorkspace)?;

    // Validate that the resolved path is actually a workspace root.
    if !workspace.join(".bwoc/workspace.toml").is_file() {
        return Err(RunError::NoWorkspace);
    }

    let registry =
        AgentsRegistry::load(&workspace).map_err(|e| RunError::RegistryLoad(e.to_string()))?;

    // Resolve agent by id or bare name.
    let lookup_id = normalize_agent_id(&args.agent);
    let entry = registry
        .agents
        .iter()
        .find(|a| a.id == lookup_id)
        .ok_or_else(|| RunError::AgentNotFound(args.agent.clone()))?;

    let backend = parse_backend(&entry.backend).ok_or_else(|| RunError::UnknownBackend {
        agent: entry.id.clone(),
        backend: entry.backend.clone(),
    })?;

    let agent_dir = workspace.join(&entry.path);

    // The run cwd: agent_dir by default (jailed), or the caller's --workdir
    // (resolved + validated to stay inside the workspace root).
    let work_dir = resolve_workdir(&workspace, args.workdir.as_deref(), &agent_dir)?;

    // Load manifest for primaryModel (needed by ollama dispatch). The manifest is
    // the agent's identity — always read from agent_dir, never from work_dir.
    let manifest_path = agent_dir.join("config.manifest.json");
    let manifest = Manifest::load_from_path(&manifest_path)
        .map_err(|e| RunError::ManifestMissing(manifest_path.clone(), e.to_string()))?;

    let (program, cmd_args) = build_command(
        backend,
        &agent_dir,
        &work_dir,
        &args.task,
        &manifest.primary_model,
    )?;

    let arg_refs: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();

    let start = Instant::now();
    let outcome = runner.run(&program, &arg_refs, &work_dir, args.timeout_secs)?;
    let duration_ms = start.elapsed().as_millis() as u64;

    let result = RunResult {
        agent: entry.id.clone(),
        backend: entry.backend.clone(),
        task: args.task,
        exit_code: outcome.exit_code,
        duration_ms,
        output: outcome.output,
    };
    Ok((result, args.json))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn normalize_agent_id(name: &str) -> String {
    if name.starts_with("agent-") {
        name.to_string()
    } else {
        format!("agent-{name}")
    }
}

fn parse_backend(s: &str) -> Option<Backend> {
    match s {
        "claude" => Some(Backend::Claude),
        "agy" => Some(Backend::Antigravity),
        "codex" => Some(Backend::Codex),
        "kimi" => Some(Backend::Kimi),
        "copilot" => Some(Backend::Copilot),
        "ollama" => Some(Backend::Ollama),
        "openai-compatible" => Some(Backend::OpenAiCompatible),
        "openrouter" => Some(Backend::OpenRouter),
        "litellm" => Some(Backend::LiteLlm),
        _ => None,
    }
}

fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return Some(PathBuf::from(env_path));
        }
    }
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Resolve the `--workdir` override to the run cwd, or fall back to the agent's
/// own directory (the jailed default). A relative override resolves against the
/// workspace root; the result must be an existing directory that stays **inside**
/// the workspace root — both sides are canonicalized so a `..` traversal can't
/// escape the workspace (Sīla — the blast radius stays bounded to the workspace).
fn resolve_workdir(
    workspace: &Path,
    workdir: Option<&Path>,
    agent_dir: &Path,
) -> Result<PathBuf, RunError> {
    let Some(w) = workdir else {
        return Ok(agent_dir.to_path_buf());
    };
    let candidate = if w.is_absolute() {
        w.to_path_buf()
    } else {
        workspace.join(w)
    };
    let canonical = candidate
        .canonicalize()
        .ok()
        .filter(|p| p.is_dir())
        .ok_or_else(|| RunError::BadWorkdir(w.to_path_buf()))?;
    let ws_canonical = workspace
        .canonicalize()
        .map_err(|_| RunError::BadWorkdir(w.to_path_buf()))?;
    if !canonical.starts_with(&ws_canonical) {
        return Err(RunError::BadWorkdir(w.to_path_buf()));
    }
    Ok(canonical)
}

/// Minimal JSON string escaping for the hand-rolled serializer.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // ── Mock runner ───────────────────────────────────────────────────────────

    /// Captures the invocation for assertion; returns a canned outcome.
    struct MockCommandRunner {
        pub exit_code: i32,
        pub output: String,
        // Captured on first call.
        pub captured_program: std::cell::RefCell<Option<String>>,
        pub captured_args: std::cell::RefCell<Option<Vec<String>>>,
        pub captured_cwd: std::cell::RefCell<Option<PathBuf>>,
    }

    impl MockCommandRunner {
        fn ok(output: &str) -> Self {
            Self {
                exit_code: 0,
                output: output.to_string(),
                captured_program: std::cell::RefCell::new(None),
                captured_args: std::cell::RefCell::new(None),
                captured_cwd: std::cell::RefCell::new(None),
            }
        }
        fn fail(code: i32) -> Self {
            Self {
                exit_code: code,
                output: String::new(),
                captured_program: std::cell::RefCell::new(None),
                captured_args: std::cell::RefCell::new(None),
                captured_cwd: std::cell::RefCell::new(None),
            }
        }
    }

    impl CommandRunner for MockCommandRunner {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            cwd: &Path,
            _timeout_secs: Option<u64>,
        ) -> Result<CommandOutcome, RunError> {
            *self.captured_program.borrow_mut() = Some(program.to_string());
            *self.captured_args.borrow_mut() = Some(args.iter().map(|s| s.to_string()).collect());
            *self.captured_cwd.borrow_mut() = Some(cwd.to_path_buf());
            Ok(CommandOutcome {
                exit_code: self.exit_code,
                output: self.output.clone(),
            })
        }
    }

    // ── Workspace + registry fixtures ─────────────────────────────────────────

    /// Minimal BWOC workspace in a tempdir with one agent registered.
    fn make_workspace(backend: &str, primary_model: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().to_path_buf();

        // .bwoc/workspace.toml
        fs::create_dir_all(root.join(".bwoc")).unwrap();
        fs::write(
            root.join(".bwoc/workspace.toml"),
            "[workspace]\nname = 'test'\nversion = '0.1'\ncreated = '2026-01-01'\n",
        )
        .unwrap();

        // agents/ directory with an agent stub
        let agent_dir = root.join("agents/agent-test");
        fs::create_dir_all(&agent_dir).unwrap();
        // AGENTS.md presence (required by bwoc spawn, not strictly by run — but good practice)
        fs::write(agent_dir.join("AGENTS.md"), "# stub\n").unwrap();

        // Minimal config.manifest.json
        let manifest_json = format!(
            r#"{{
  "name": "test",
  "agentId": "agent-test",
  "agentRole": "tester",
  "primaryModel": "{primary_model}",
  "memoryPath": "memories/",
  "lintCmd": "echo lint",
  "formatCmd": "echo fmt",
  "testCmd": "echo test",
  "buildCmd": "echo build",
  "version": "1.0"
}}"#
        );
        fs::write(agent_dir.join("config.manifest.json"), manifest_json).unwrap();

        // .bwoc/agents.toml — single-quoted TOML literals (CI-safe on Windows)
        let toml = format!(
            "[[agent]]\nid = 'agent-test'\npath = 'agents/agent-test'\n\
             backend = '{backend}'\nincarnated = '2026-01-01'\nstatus = 'active'\n"
        );
        fs::write(root.join(".bwoc/agents.toml"), toml).unwrap();

        (dir, root.clone(), agent_dir)
    }

    // ── build_command tests ───────────────────────────────────────────────────

    #[test]
    fn claude_dispatch_uses_print_flag() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (program, args) = build_command(
            Backend::Claude,
            tmp.path(),
            tmp.path(),
            "hello world",
            "claude-opus-4-7",
        )
        .unwrap();
        assert_eq!(program, "claude");
        assert_eq!(args, ["-p", "hello world"]);
    }

    #[test]
    fn claude_dispatch_passes_reasoning_effort_when_set() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("config.manifest.json"),
            r#"{
                "name": "x", "agentId": "agent-x", "agentRole": "r",
                "primaryModel": "claude-opus-4-8", "reasoningEffort": "max",
                "memoryPath": "memories/", "lintCmd": "true", "formatCmd": "true",
                "testCmd": "true", "buildCmd": "true", "version": "2.0"
            }"#,
        )
        .unwrap();
        let (program, args) = build_command(
            Backend::Claude,
            tmp.path(),
            tmp.path(),
            "do it",
            "claude-opus-4-8",
        )
        .unwrap();
        assert_eq!(program, "claude");
        // Effort flag sits between -p and the positional task.
        assert_eq!(args, ["-p", "--effort", "max", "do it"]);
    }

    #[test]
    fn codex_returns_headless_unsupported() {
        let tmp = tempfile::TempDir::new().unwrap();
        let err =
            build_command(Backend::Codex, tmp.path(), tmp.path(), "task", "gpt-5").unwrap_err();
        assert!(matches!(err, RunError::HeadlessUnsupported { ref backend } if backend == "codex"));
    }

    #[test]
    fn antigravity_returns_headless_unsupported() {
        let tmp = tempfile::TempDir::new().unwrap();
        let err = build_command(
            Backend::Antigravity,
            tmp.path(),
            tmp.path(),
            "task",
            "gemini-3.5-flash-medium",
        )
        .unwrap_err();
        assert!(matches!(err, RunError::HeadlessUnsupported { ref backend } if backend == "agy"));
    }

    #[test]
    fn kimi_returns_headless_unsupported() {
        let tmp = tempfile::TempDir::new().unwrap();
        let err =
            build_command(Backend::Kimi, tmp.path(), tmp.path(), "task", "kimi-k2").unwrap_err();
        assert!(matches!(err, RunError::HeadlessUnsupported { ref backend } if backend == "kimi"));
    }

    // ── execute() with mock runner ────────────────────────────────────────────

    #[test]
    fn claude_run_passes_task_as_print_arg() {
        let (dir, root, _agent_dir) = make_workspace("claude", "claude-opus-4-7");
        let _keep = dir; // keep tempdir alive

        let runner = MockCommandRunner::ok("agent output here");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "write a hello world".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: None,
        };
        let (result, _json) = execute(args, &runner).unwrap();

        let prog = runner.captured_program.borrow().clone().unwrap();
        let captured_args = runner.captured_args.borrow().clone().unwrap();

        assert_eq!(prog, "claude");
        assert_eq!(captured_args, ["-p", "write a hello world"]);
        assert_eq!(result.output, "agent output here");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.backend, "claude");
    }

    #[test]
    fn default_workdir_jails_to_agent_dir() {
        let (dir, root, agent_dir) = make_workspace("claude", "claude-opus-4-8");
        let _keep = dir;
        let runner = MockCommandRunner::ok("out");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "t".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: None,
        };
        execute(args, &runner).unwrap();
        let cwd = runner.captured_cwd.borrow().clone().unwrap();
        assert_eq!(
            cwd.canonicalize().unwrap(),
            agent_dir.canonicalize().unwrap()
        );
    }

    #[test]
    fn workdir_dot_widens_to_workspace_root() {
        let (dir, root, _agent_dir) = make_workspace("claude", "claude-opus-4-8");
        let _keep = dir;
        let runner = MockCommandRunner::ok("out");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "t".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root.clone()),
            workdir: Some(PathBuf::from(".")),
        };
        execute(args, &runner).unwrap();
        let cwd = runner.captured_cwd.borrow().clone().unwrap();
        // `.` resolves against the workspace root, so the run cwd IS the root.
        assert_eq!(cwd.canonicalize().unwrap(), root.canonicalize().unwrap());
    }

    #[test]
    fn harness_workdir_arg_follows_the_run_cwd() {
        // For harness backends the `--workdir` arg is the FS-jail root — it must
        // track the widened cwd, not stay pinned to agent_dir.
        let (dir, root, _agent_dir) = make_workspace("ollama", "qwen2.5-coder");
        let _keep = dir;
        // The harness binary may be absent in CI; only assert when dispatch built.
        let runner = MockCommandRunner::ok("out");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "t".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root.clone()),
            workdir: Some(PathBuf::from(".")),
        };
        match execute(args, &runner) {
            Ok(_) => {
                let cargs = runner.captured_args.borrow().clone().unwrap();
                let i = cargs.iter().position(|a| a == "--workdir").unwrap();
                assert_eq!(
                    PathBuf::from(&cargs[i + 1]).canonicalize().unwrap(),
                    root.canonicalize().unwrap()
                );
            }
            Err(RunError::HarnessNotFound) => {} // harness not installed in this env
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    #[test]
    fn workdir_escaping_workspace_is_refused() {
        let (dir, root, _agent_dir) = make_workspace("claude", "claude-opus-4-8");
        let _keep = dir;
        let runner = MockCommandRunner::ok("out");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "t".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: Some(PathBuf::from("..")), // parent of the workspace root
        };
        let err = execute(args, &runner).unwrap_err();
        assert!(matches!(err, RunError::BadWorkdir(_)));
    }

    #[test]
    fn workdir_nonexistent_is_refused() {
        let (dir, root, _agent_dir) = make_workspace("claude", "claude-opus-4-8");
        let _keep = dir;
        let runner = MockCommandRunner::ok("out");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "t".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: Some(PathBuf::from("no/such/dir")),
        };
        let err = execute(args, &runner).unwrap_err();
        assert!(matches!(err, RunError::BadWorkdir(_)));
    }

    #[test]
    fn non_zero_exit_code_preserved() {
        let (dir, root, _) = make_workspace("claude", "claude-sonnet-4-6");
        let _keep = dir;

        let runner = MockCommandRunner::fail(42);
        let args = RunArgs {
            agent: "agent-test".to_string(),
            task: "do something".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: None,
        };
        let (result, _) = execute(args, &runner).unwrap();
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn agent_not_found_returns_error() {
        let (dir, root, _) = make_workspace("claude", "claude-opus-4-7");
        let _keep = dir;

        let runner = MockCommandRunner::ok("");
        let args = RunArgs {
            agent: "no-such-agent".to_string(),
            task: "irrelevant".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: None,
        };
        let err = execute(args, &runner).unwrap_err();
        assert!(matches!(err, RunError::AgentNotFound(_)));
    }

    #[test]
    fn headless_unsupported_propagated_from_execute() {
        let (dir, root, _) = make_workspace("codex", "gpt-5");
        let _keep = dir;

        let runner = MockCommandRunner::ok("irrelevant");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "do task".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: None,
        };
        let err = execute(args, &runner).unwrap_err();
        assert!(matches!(err, RunError::HeadlessUnsupported { .. }));
    }

    #[test]
    fn json_output_shape() {
        let result = RunResult {
            agent: "agent-test".to_string(),
            backend: "claude".to_string(),
            task: "hello".to_string(),
            exit_code: 0,
            duration_ms: 1234,
            output: "some output".to_string(),
        };
        let json = result.to_json();
        // Must contain every required key.
        assert!(json.contains(r#""agent":"#));
        assert!(json.contains(r#""backend":"#));
        assert!(json.contains(r#""task":"#));
        assert!(json.contains(r#""exit_code":"#));
        assert!(json.contains(r#""duration_ms":"#));
        assert!(json.contains(r#""output":"#));
        // Values spot check.
        assert!(json.contains("agent-test"));
        assert!(json.contains("1234"));
    }

    #[test]
    fn json_escapes_special_chars() {
        let result = RunResult {
            agent: r#"ag"ent"#.to_string(),
            backend: "claude".to_string(),
            task: "line1\nline2".to_string(),
            exit_code: 0,
            duration_ms: 0,
            output: "has\\backslash".to_string(),
        };
        let json = result.to_json();
        assert!(json.contains(r#"ag\"ent"#));
        assert!(json.contains(r#"line1\nline2"#));
        assert!(json.contains(r#"has\\backslash"#));
    }

    #[test]
    fn agent_id_normalization() {
        assert_eq!(normalize_agent_id("oracle"), "agent-oracle");
        assert_eq!(normalize_agent_id("agent-oracle"), "agent-oracle");
    }

    #[test]
    fn no_workspace_error_without_workspace_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        // No .bwoc/workspace.toml

        let runner = MockCommandRunner::ok("");
        let args = RunArgs {
            agent: "test".to_string(),
            task: "do it".to_string(),
            json: false,
            timeout_secs: None,
            workspace: Some(root),
            workdir: None,
        };
        let err = execute(args, &runner).unwrap_err();
        assert!(matches!(err, RunError::NoWorkspace));
    }
}
