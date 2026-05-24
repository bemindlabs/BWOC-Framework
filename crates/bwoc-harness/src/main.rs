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
    self_improve,
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
    /// Initial task / prompt for the agent.
    #[arg(long, short = 't')]
    task: String,

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

    // ── Provider ──────────────────────────────────────────────────────────
    let provider: Arc<dyn ProviderClient> = Arc::new(OllamaClient::new(args.endpoint.clone()));

    // Validate model exists before running (spike: wrong tag → 404).
    if !args.skip_model_check {
        print!("  checking model availability... ");
        provider.validate_model(&args.model).await?;
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
    let registry = Arc::new(default_registry());

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

    // ── Loop config ───────────────────────────────────────────────────────
    let config = LoopConfig {
        model: args.model.clone(),
        fallback_models: Vec::new(),
        vetted_models: Vec::new(),
        vetted_mode,
        max_iterations: args.max_iterations,
        stream: args.stream,
        policy,
        is_tty,
        context_limit: 0, // no compaction by default; operator sets via config
        model_context_limits: std::collections::HashMap::new(),
        token_pressure_models: Vec::new(),
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
    println!("\ntask: {}", args.task);
    println!("─────────────────────────────────────────────");

    let result = run_loop(
        provider,
        registry,
        ctx,
        config,
        system_prompt,
        vec![ChatMessage::user(&args.task)],
        &mut telemetry,
    )
    .await?;

    // Persist session metrics (best-effort; non-fatal if it fails).
    let metrics_path = args.workdir.join("session-metrics.jsonl");
    if let Err(e) = telemetry.finish(&metrics_path) {
        eprintln!("[bwoc-harness] warning: could not write session metrics: {e}");
    }

    // ── Paññā-3 retrospective analysis ────────────────────────────────────
    // Read-only analysis over recent session history.  Runs after the metrics
    // record is written so the current session is included.  Any error is
    // silently absorbed (best-effort — must not fail the run).
    let retro = self_improve::analyse(&metrics_path);
    if retro.retro_recommended {
        eprintln!("[bwoc-harness] [panna-3] retrospective recommended:");
        for reason in &retro.reasons {
            eprintln!("[bwoc-harness] [panna-3]   - {reason}");
        }

        // Write a retrospective stub (best-effort; non-fatal).
        let date = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // Simple YYYY-MM-DD from seconds (UTC).
            let days = secs / 86400;
            let mut year = 1970u64;
            let mut remaining = days;
            loop {
                let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
                let dy = if leap { 366 } else { 365 };
                if remaining < dy {
                    break;
                }
                remaining -= dy;
                year += 1;
            }
            let months = [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let mut month = 1u64;
            for (i, &dim) in months.iter().enumerate() {
                let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
                let d = if i == 1 && leap { 29 } else { dim };
                if remaining < d {
                    month = i as u64 + 1;
                    break;
                }
                remaining -= d;
            }
            let day = remaining + 1;
            format!("{year:04}-{month:02}-{day:02}")
        };

        let retro_dir = workdir.join("retrospectives");
        let stub_path = retro_dir.join(format!("{date}_auto.md"));
        let stub_content = self_improve::build_retro_stub(&retro, "bwoc-harness", &date);

        // Create the retrospectives directory if needed (best-effort).
        let _ = std::fs::create_dir_all(&retro_dir);
        match std::fs::write(&stub_path, &stub_content) {
            Ok(()) => {
                eprintln!(
                    "[bwoc-harness] [panna-3] retrospective stub written: {}",
                    stub_path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "[bwoc-harness] [panna-3] warning: could not write retro stub \
                     ({}): {e}",
                    stub_path.display()
                );
            }
        }
    }

    // Propagate retro findings into the LoopResult for callers / tests.
    let result = bwoc_harness::agent_loop::LoopResult {
        retro_recommended: retro.retro_recommended,
        retro_reasons: retro.reasons,
        ..result
    };

    println!("─────────────────────────────────────────────");
    println!("done in {} turn(s).\n", result.turns);
    println!("{}", result.final_response);

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
