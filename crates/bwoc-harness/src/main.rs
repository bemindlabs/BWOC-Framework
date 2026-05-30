//! `bwoc-harness` binary entry point.
//!
//! Parses CLI args, loads the system prompt from `AGENTS.md` / `CLAUDE.md`
//! in the working directory (if present), validates the model, and runs the
//! agentic loop.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;

use bwoc_harness::{
    agent_loop::{LoopConfig, VettedMode, run_loop},
    error::HarnessResult,
    policy::{HarnessPolicy, Policy},
    provider::{ChatMessage, OllamaClient, ProviderClient},
    tools::{ToolContext, registry::default_registry},
};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// BWOC self-hosted agentic harness.
///
/// Runs an OpenAI-compatible agentic loop against a local model endpoint
/// (default: Ollama at http://localhost:11434/v1).
///
/// P1 — dev-only, no safety guardrails.  Do not use on untrusted tasks.
#[derive(Parser, Debug)]
#[command(name = "bwoc-harness", version, about, long_about = None)]
struct Args {
    /// Initial task / prompt for the agent.  Required for a new run; ignored
    /// (and may be omitted) when `--resume` is given.
    #[arg(long, short = 't')]
    task: Option<String>,

    /// Resume a previously-checkpointed run by id.  Reloads its history,
    /// counters, and active model and continues against the existing worktree
    /// (no replay).  Mutually exclusive with a fresh `--task`.
    #[arg(long, conflicts_with = "task")]
    resume: Option<String>,

    /// Run as a Saṅgha lead (HV2-1): drain claimable tasks from `--tasks`,
    /// spawning a `bwoc-harness` worker subprocess per task in its own git
    /// worktree off `--workdir`.  Mutually exclusive with `--task`/`--resume`.
    #[arg(long, conflicts_with_all = ["task", "resume"])]
    lead: bool,

    /// Path to the Saṅgha `tasks.jsonl` (required with `--lead`).
    #[arg(long, requires = "lead")]
    tasks: Option<PathBuf>,

    /// Agent id the lead claims tasks as (lead mode).
    #[arg(long, default_value = "agent-lead")]
    agent: String,

    /// Max tasks to process this lead invocation; `0` = drain all.
    #[arg(long, default_value_t = 0)]
    max_tasks: usize,

    /// Worker concurrency for lead mode (collection is currently sequential).
    #[arg(long, default_value_t = 1)]
    concurrency: usize,

    /// Per-run hard token budget (prompt + completion). The run aborts with
    /// `BudgetExceeded` once cumulative usage crosses it.  Unset = no limit.
    #[arg(long)]
    token_budget: Option<u64>,

    /// Per-run hard cost budget (e.g. USD).  Only enforced together with
    /// `--cost-per-1m`.  Unset = no limit.
    #[arg(long)]
    cost_limit: Option<f64>,

    /// Price per 1,000,000 tokens, used to derive cost for `--cost-limit`.
    #[arg(long)]
    cost_per_1m: Option<f64>,

    /// Launch an external MCP tool server and register its tools (HV2-5).
    /// Value is the server command line, e.g. `--mcp "my-mcp-server --flag"`.
    /// Repeatable.  Tools are exposed as `mcp__<server>__<tool>`.
    #[arg(long)]
    mcp: Vec<String>,

    /// Working directory (worktree root).  All file operations are confined
    /// to this directory.  Defaults to the current directory.
    #[arg(long, short = 'd', default_value = ".")]
    workdir: PathBuf,

    /// Model identifier (must be pulled and available at the endpoint).
    #[arg(long, short = 'm', default_value = "gemma4")]
    model: String,

    /// OpenAI-compatible endpoint base URL.
    #[arg(long, short = 'e', default_value = "http://localhost:11434/v1")]
    endpoint: String,

    /// Maximum number of agentic turns before giving up.
    #[arg(long, default_value_t = 20)]
    max_iterations: u32,

    /// Use SSE streaming mode (token deltas).  Default is blocking mode.
    #[arg(long)]
    stream: bool,

    /// Skip model validation at startup (useful for testing with mock endpoints).
    #[arg(long)]
    skip_model_check: bool,

    /// How to handle a model that is absent from the vetted-models allowlist.
    ///
    /// `off` — skip the check silently.
    /// `warn` — emit a warning but proceed (default).
    /// `enforce` — refuse to run an unvetted primary model.
    #[arg(long, default_value = "warn")]
    vetted_mode: String,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("bwoc-harness error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> HarnessResult<()> {
    let args = Args::parse();

    // Resolve working directory to an absolute path.
    let workdir = args.workdir.canonicalize().unwrap_or_else(|_| {
        // If the path doesn't exist yet, leave as-is and let the first tool
        // call surface the error.
        args.workdir.clone()
    });

    println!("bwoc-harness P1 starting");
    println!("  workdir  : {}", workdir.display());
    println!("  model    : {}", args.model);
    println!("  endpoint : {}", args.endpoint);
    println!("  stream   : {}", args.stream);

    // ── Saṅgha lead mode (HV2-1) ──────────────────────────────────────────
    // Drains tasks and spawns worker subprocesses; the parent never runs task
    // code or calls a provider — each worker does, as its own sandboxed process.
    if args.lead {
        return run_lead_mode(&args, &workdir).await;
    }

    // ── Provider ──────────────────────────────────────────────────────────
    let provider: Arc<dyn ProviderClient> = Arc::new(OllamaClient::new(args.endpoint.clone()));

    // ── Auto model selection (primaryModel: "auto") ───────────────────────
    // When the agent's manifest declares `primaryModel: "auto"`, `bwoc run`
    // passes the literal sentinel through as --model. Resolve it now against
    // the live provider using the manifest's `autoModels` pool, and harvest the
    // by-products (fallback chain, probed context limits) so the LoopConfig
    // fields below get populated from real provider data rather than left empty.
    let mut resolved_model = args.model.clone();
    let mut auto_fallbacks: Vec<String> = Vec::new();
    let mut auto_context_limits: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    if let Some(run_id) = args.resume.as_deref() {
        // On --resume we must NOT re-resolve: with no `--task` the resolver
        // would reclassify the work as Light and could swap the run onto a
        // smaller/cheaper model mid-history. Reuse the model the run was
        // checkpointed with (the loop also overrides `active_model` from the
        // checkpoint, but config.model still feeds the vetted-model gate).
        if args.model == bwoc_harness::model_select::AUTO_SENTINEL {
            resolved_model = bwoc_harness::checkpoint::CheckpointConfig::resume(run_id)
                .ok()
                .and_then(|c| c.resume)
                .map(|s| s.active_model)
                .unwrap_or(resolved_model);
        }
    } else if args.model == bwoc_harness::model_select::AUTO_SENTINEL {
        let candidates =
            bwoc_core::manifest::Manifest::load_from_path(&workdir.join("config.manifest.json"))
                .ok()
                .and_then(|m| m.auto_models)
                .unwrap_or_default();
        let task_for_class = args.task.as_deref().unwrap_or("");
        print!(
            "  resolving auto model from {} candidate(s)... ",
            candidates.len()
        );
        let sel = bwoc_harness::model_select::resolve_auto(
            provider.as_ref(),
            &candidates,
            task_for_class,
        )
        .await?;
        println!("→ {}", sel.chosen);
        resolved_model = sel.chosen;
        auto_fallbacks = sel.remaining;
        auto_context_limits = sel.context_limits;
    }

    // Validate model exists before running (spike: wrong tag → 404). Skipped on
    // resume: the model was validated in the original run and is reloaded from
    // the checkpoint, not re-supplied here.
    if !args.skip_model_check && args.resume.is_none() {
        print!("  checking model availability... ");
        provider.validate_model(&resolved_model).await?;
        println!("ok");
    }

    // ── System prompt ─────────────────────────────────────────────────────
    let system_prompt = load_system_prompt(&workdir).await;
    if system_prompt.is_empty() {
        println!("  system prompt: (none — AGENTS.md / CLAUDE.md not found in workdir)");
    } else {
        println!("  system prompt: loaded ({} chars)", system_prompt.len());
    }

    // ── Tool registry ─────────────────────────────────────────────────────
    let mut registry = default_registry();
    // ── MCP tool servers (HV2-5) ──────────────────────────────────────────
    // Each --mcp launches an external MCP server and registers its tools.
    // Failures are warned, not fatal — the run proceeds with the built-in set.
    for spec in &args.mcp {
        let parts: Vec<String> = spec.split_whitespace().map(String::from).collect();
        let Some((program, prog_args)) = parts.split_first() else {
            continue;
        };
        let label = std::path::Path::new(program)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(program);
        match bwoc_harness::mcp::McpClient::connect_stdio(program, prog_args).await {
            Ok(client) => match client.register_tools(&mut registry, label).await {
                Ok(n) => println!("  mcp      : {n} tool(s) from `{program}`"),
                Err(e) => {
                    eprintln!("[bwoc-harness] warning: MCP `tools/list` from `{program}`: {e}")
                }
            },
            Err(e) => eprintln!("[bwoc-harness] warning: MCP connect `{program}`: {e}"),
        }
    }
    let registry = Arc::new(registry);

    // ── Context ───────────────────────────────────────────────────────────
    let ctx = ToolContext::new(&workdir);

    // ── Permission policy ─────────────────────────────────────────────────
    // Load from .bwoc/harness-policy.toml relative to the workdir.
    // Falls back to a fail-safe deny-all policy if the file is absent.
    let policy: Policy = HarnessPolicy::load(&workdir)
        .unwrap_or_else(|e| {
            eprintln!(
                "[bwoc-harness] warning: could not load harness-policy.toml: {e}. \
                 Using fail-safe deny-all policy."
            );
            bwoc_harness::policy::HarnessPolicy::default()
        })
        .into();

    // Detect TTY: if stderr is a terminal, the operator can respond to `ask` prompts.
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());

    // ── Vetted mode ───────────────────────────────────────────────────────
    let vetted_mode: VettedMode = args.vetted_mode.parse().unwrap_or_else(|e: String| {
        eprintln!("[bwoc-harness] warning: {e}; defaulting to warn");
        VettedMode::Warn
    });

    // ── Durable run (HV2-2) ───────────────────────────────────────────────
    // Either resume a checkpointed run or start a fresh one.  The harness
    // binary always checkpoints; `LoopConfig::checkpoint = None` is reserved
    // for embedders/tests.
    let (checkpoint, initial_messages) = match &args.resume {
        Some(run_id) => {
            let cfg =
                bwoc_harness::checkpoint::CheckpointConfig::resume(run_id).unwrap_or_else(|e| {
                    eprintln!("[bwoc-harness] error: cannot resume run `{run_id}`: {e}");
                    std::process::exit(1);
                });
            let prior_turns = cfg.resume.as_ref().map(|s| s.turns).unwrap_or(0);
            println!("  resuming : {run_id} ({prior_turns} prior turn(s))");
            // Resumed history seeds the loop; no fresh task message.
            (Some(cfg), Vec::new())
        }
        None => {
            let task = args.task.clone().unwrap_or_else(|| {
                eprintln!("[bwoc-harness] error: --task is required (or use --resume <run-id>)");
                std::process::exit(1);
            });
            let run_id = bwoc_harness::checkpoint::new_run_id();
            println!("  run id   : {run_id}");
            (
                Some(bwoc_harness::checkpoint::CheckpointConfig::new(run_id)),
                vec![ChatMessage::user(&task)],
            )
        }
    };

    // ── Loop config ───────────────────────────────────────────────────────
    let config = LoopConfig {
        model: resolved_model.clone(),
        // For an auto-resolved run these carry the remaining available
        // candidates (preference order) + their probed context limits; for a
        // concrete model they stay empty, preserving prior behaviour.
        fallback_models: auto_fallbacks.clone(),
        vetted_models: Vec::new(),
        vetted_mode,
        max_iterations: args.max_iterations,
        stream: args.stream,
        policy,
        is_tty,
        context_limit: 0, // no compaction by default; operator sets via config
        model_context_limits: auto_context_limits,
        token_pressure_models: auto_fallbacks,
        checkpoint,
        budget: bwoc_harness::budget::BudgetConfig {
            max_tokens: args.token_budget,
            max_cost: args.cost_limit,
            cost_per_1m_tokens: args.cost_per_1m,
        },
    };

    // ── Telemetry ─────────────────────────────────────────────────────────
    let session_id = format!(
        "sess-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let mut telemetry = bwoc_harness::telemetry::Telemetry::new(session_id, "bwoc-harness");

    // ── Run ───────────────────────────────────────────────────────────────
    println!(
        "\ntask: {}",
        args.task.as_deref().unwrap_or("(resumed run)")
    );
    println!("─────────────────────────────────────────────");

    let outcome = run_loop(
        provider,
        registry,
        ctx,
        config,
        system_prompt,
        initial_messages,
        &mut telemetry,
    )
    .await;

    // Record the run for the §8b retrospective regardless of how it ended.
    // One attempted task always; one completed only on success — an aborted
    // run (budget / max-iterations / models-exhausted) must surface as a
    // sub-100% completion rate, not be skipped.  Those are exactly the runs
    // §8b is meant to learn from.
    telemetry.agent.tasks_attempted += 1;
    if outcome.is_ok() {
        telemetry.agent.tasks_completed += 1;
    }

    // Persist session metrics (best-effort; non-fatal if it fails).
    let metrics_path = args.workdir.join("session-metrics.jsonl");
    if let Err(e) = telemetry.finish(&metrics_path) {
        eprintln!("[bwoc-harness] warning: could not write session metrics: {e}");
    }

    // ── Run-end retrospective (HV2-3) ─────────────────────────────────────
    // Surface any §8b self-improvement triggers.  Runs on success AND failure.
    // Observe-don't-drive: printed, never applied.
    let retro = bwoc_harness::retrospective::Retrospective::analyze(
        &telemetry.build_record(),
        &bwoc_harness::retrospective::RetroThresholds::default(),
    );
    eprint!("{}", retro.render());

    // Propagate an aborted run as an error — after the retrospective has been
    // recorded and printed.
    let result = outcome?;

    println!("─────────────────────────────────────────────");
    println!("done in {} turn(s).\n", result.turns);
    println!("{}", result.final_response);

    Ok(())
}

// ---------------------------------------------------------------------------
// Saṅgha lead mode (HV2-1)
// ---------------------------------------------------------------------------

/// Run the lead loop: drain `--tasks` and spawn a worker subprocess per task.
async fn run_lead_mode(args: &Args, workdir: &std::path::Path) -> HarnessResult<()> {
    use bwoc_harness::lead::{JsonlTaskSource, LeadConfig, run_lead};
    use bwoc_harness::worker::{SubprocessRunner, WorkerConfig};

    let tasks_path = args.tasks.as_ref().ok_or_else(|| {
        bwoc_harness::error::HarnessError::Other("--lead requires --tasks <path>".to_string())
    })?;

    let source = JsonlTaskSource::new(tasks_path);
    let runner = std::sync::Arc::new(SubprocessRunner::new()?);
    let cfg = LeadConfig {
        agent_id: args.agent.clone(),
        repo_root: workdir.to_path_buf(),
        worktree_base: workdir.join(".bwoc").join("worktrees"),
        worker: WorkerConfig {
            model: args.model.clone(),
            endpoint: args.endpoint.clone(),
            skip_model_check: args.skip_model_check,
        },
        capacity: args.concurrency,
        max_tasks: args.max_tasks,
    };

    println!(
        "  mode     : Saṅgha lead (agent={}, tasks={})",
        cfg.agent_id,
        tasks_path.display()
    );
    println!("─────────────────────────────────────────────");

    let summary = run_lead(&source, runner, &cfg).await?;

    println!("─────────────────────────────────────────────");
    println!(
        "lead done: {} claimed, {} completed, {} failed.",
        summary.claimed, summary.completed, summary.failed
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// System prompt loading
// ---------------------------------------------------------------------------

/// Load the system prompt from `AGENTS.md` (preferred) or `CLAUDE.md` in the
/// working directory.  Returns an empty string if neither is found.
async fn load_system_prompt(workdir: &std::path::Path) -> String {
    for filename in &["AGENTS.md", "CLAUDE.md"] {
        let path = workdir.join(filename);
        if let Ok(content) = tokio::fs::read_to_string(&path).await {
            return content;
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn load_system_prompt_agents_md() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("AGENTS.md"), "You are an agent.")
            .await
            .unwrap();
        let prompt = load_system_prompt(tmp.path()).await;
        assert_eq!(prompt, "You are an agent.");
    }

    #[tokio::test]
    async fn load_system_prompt_claude_md_fallback() {
        let tmp = TempDir::new().unwrap();
        // No AGENTS.md — falls back to CLAUDE.md.
        tokio::fs::write(tmp.path().join("CLAUDE.md"), "Claude system prompt.")
            .await
            .unwrap();
        let prompt = load_system_prompt(tmp.path()).await;
        assert_eq!(prompt, "Claude system prompt.");
    }

    #[tokio::test]
    async fn load_system_prompt_missing_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let prompt = load_system_prompt(tmp.path()).await;
        assert!(prompt.is_empty());
    }
}
