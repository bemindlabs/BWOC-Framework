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
    provider::{AnthropicClient, ChatMessage, CliClient, OllamaClient, ProviderClient},
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

    /// Drive an interactive, multi-turn chat session over stdin/stdout using the
    /// `bwoc_core::chat_proto` JSON-line protocol (the `bwoc chat --tui`
    /// frontend spawns the harness this way).  Mutually exclusive with the
    /// task/resume/lead paths.
    #[arg(long, conflicts_with_all = ["task", "resume", "lead"])]
    chat: bool,

    /// Headless / served session (issue #301): the same multi-turn
    /// `chat_proto` loop as `--chat`, but driven by a machine frontend (e.g. the
    /// `bwoc-agent --serve` daemon) instead of a human. Because no one is present
    /// to answer permission prompts, `ask`-mode tools auto-approve — the turn
    /// never blocks — while layer-1 guardrails, policy `deny` rules, and the
    /// worktree sandbox still confine it. This keeps one resident process warm
    /// across messages instead of cold-starting per message. Must not be combined
    /// with `--unrestricted` (which would lift the confining sandbox).
    #[arg(long, conflicts_with_all = ["task", "resume", "lead", "chat", "unrestricted"])]
    headless: bool,

    /// Evaluation mode: run a single eval fixture directory (containing
    /// `fixture.toml` + optional `seed/` / `expected/`) against `--backend` and
    /// print the scored [`EvalResult`]. Exit 0 if it passed (or was skipped),
    /// 1 if it failed. Mutually exclusive with task/resume/lead/chat.
    #[arg(long, conflicts_with_all = ["task", "resume", "lead", "chat", "headless"])]
    eval: Option<PathBuf>,

    /// Emit machine-readable JSON instead of a human report. Eval-mode only —
    /// `--chat` already speaks a JSONL event stream, so clap requires `--eval`.
    #[arg(long, requires = "eval")]
    json: bool,

    /// Route `ask`-mode tools to the human-in-the-loop approval console when
    /// there is no TTY (e.g. a fleet agent spawned by the macOS control center).
    /// The harness writes each pending request to `<workdir>/.bwoc/approvals/`
    /// and blocks for the operator's verdict; a timeout falls back to the same
    /// fail-safe as without the flag (an approval can only turn a would-be deny
    /// into an operator-approved allow, never weaken a deny).
    #[arg(long = "approval-channel")]
    approval_channel: bool,

    /// Path to the Saṅgha `tasks.jsonl` (required with `--lead`).
    #[arg(long, requires = "lead")]
    tasks: Option<PathBuf>,

    /// Path to a team's shared chat log (`chat.jsonl`) for `--chat` broadcast
    /// (HV3-3a). When set, teammate messages are injected into context before
    /// each turn and this agent's replies are appended. Unset = solo session.
    #[arg(long, requires = "chat")]
    team_chat: Option<PathBuf>,

    /// Agent id the lead claims tasks as (lead mode).
    #[arg(long, default_value = "agent-lead")]
    agent: String,

    /// Peer-review agent for the lead's review gate (HV3-3c). When set (and
    /// different from `--agent`), each successful worker's diff is routed to
    /// this agent before completion; a rejection re-queues the task. Unset =
    /// no review gate. (`bwoc chat`/team tooling resolves this from the team's
    /// `reviewer` field.)
    #[arg(long, requires = "lead")]
    reviewer: Option<String>,

    /// Max tasks to process this lead invocation; `0` = drain all.
    #[arg(long, default_value_t = 0)]
    max_tasks: usize,

    /// Worker concurrency for lead mode: up to this many workers run in
    /// parallel (`1` = one at a time).
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

    /// Connect to a remote MCP server over Streamable HTTP and register its
    /// tools. Value is the endpoint URL, e.g. `--mcp-http https://host/mcp`.
    /// Must be https (except http://localhost). Repeatable. A bearer token is
    /// read from `~/.bwoc/secrets.toml` `[mcp] <label>_token`, where `<label>`
    /// is the URL host with non-alphanumerics replaced by `_` (so `example.com`
    /// → `example_com`, a valid bare TOML key). Tools are exposed as
    /// `mcp__<label>__<tool>`.
    #[arg(long = "mcp-http")]
    mcp_http: Vec<String>,

    /// Working directory (worktree root).  Relative tool paths resolve against
    /// it, and — unless `--unrestricted` — file operations are confined to it.
    /// Defaults to the current directory.
    #[arg(long, short = 'd', default_value = ".")]
    workdir: PathBuf,

    /// Lift the workdir path-traversal sandbox: file tools may read/write/edit
    /// any absolute path on the machine (relative paths still resolve against
    /// `--workdir`).  The permission policy becomes the only gate — use only
    /// with an `ask`/operator-reviewed policy (e.g. the interactive `--chat`).
    #[arg(long)]
    unrestricted: bool,

    /// Model identifier (must be pulled and available at the endpoint).
    #[arg(long, short = 'm', default_value = "gemma4")]
    model: String,

    /// OpenAI-compatible endpoint base URL.
    #[arg(long, short = 'e', default_value = bwoc_harness::provider::client::DEFAULT_ENDPOINT)]
    endpoint: String,

    /// Provider backend: `ollama` / `openai-compatible` (OpenAI-compatible HTTP),
    /// `openrouter` (OpenAI-compatible aggregator, key from `OPENROUTER_API_KEY`
    /// / `~/.bwoc/secrets.toml`), `litellm` (self-hosted OpenAI-compatible proxy;
    /// base from `--endpoint` or `LITELLM_API_BASE` env, else the LiteLLM default
    /// port; **optional** key from `LITELLM_API_KEY` / `~/.bwoc/secrets.toml`),
    /// `claude` / `anthropic` (Anthropic Messages API, key from
    /// `ANTHROPIC_API_KEY`), or `cli` (local subscription-authenticated vendor
    /// CLI via `--cli-cmd`; **no API key**, chat-only). Selects which provider
    /// client renders the model; the chat/agent loops are backend-neutral.
    #[arg(long, default_value = "ollama")]
    backend: String,

    /// Vendor CLI executable for `--backend cli` (e.g. `claude`, `codex`).
    /// Invoked per turn as `<cli-cmd> -p --model <model> --output-format json`
    /// with the conversation on stdin. Ignored by other backends.
    #[arg(long, default_value = bwoc_harness::provider::cli::DEFAULT_CLI_CMD)]
    cli_cmd: String,

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

fn main() {
    // Phase 5 t5 — process isolation. When re-exec'd as the hidden turn-executor
    // we MUST run the one-shot child path *before* building any async runtime, so
    // the inherited-fd snapshot stays clean (no tokio epoll/eventfd). The child
    // verifies its capability token, runs exactly one tool, and exits.
    #[cfg(unix)]
    {
        if bwoc_harness::turn_executor::is_executor_invocation() {
            std::process::exit(bwoc_harness::turn_executor::run_executor_blocking());
        }
    }

    // Phase 5 t7a / C4 — this is the PARENT (it holds provider API keys + the
    // SessionTrust latch in RAM). Harden it against same-uid ptrace /
    // process_vm_readv from a turn-executor child BEFORE any secret is loaded or
    // any child is spawned. Fail-closed if the kernel cannot protect it.
    harden_parent_against_ptrace();

    // Phase 5 t9 / condition 2 — the prod hard-guarantee switch. Default OFF
    // (best-effort); when on, refuse to start without an enforceable cgroup cap.
    assert_cgroup_enforcement_if_required();

    // Normal harness path: build the multi-thread runtime and run the loop.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    rt.block_on(async {
        if let Err(e) = run().await {
            eprintln!("bwoc-harness error: {e}");
            std::process::exit(1);
        }
    });
}

/// Phase 5 t7a / C4 — protect the parent's RAM (provider keys, trust latch)
/// from a same-uid attacker (the turn-executor child), closing CRIT-1.
///
/// 1. `prctl(PR_SET_DUMPABLE, 0)` — a non-dumpable process can only be ptraced /
///    have `/proc/<pid>/mem` read by root (`CAP_SYS_PTRACE`); a same-uid caller
///    gets `EPERM`. This is the control that actually blocks the RAM-read.
/// 2. Verify `kernel.yama.ptrace_scope >= 1` and **fail-closed** if it reads `0`
///    (yama further restricts ptrace to descendants — defence in depth).
///
/// macOS protects task ports via taskgated/SIP; this is a Linux control and the
/// redteam ptrace arm LOUD-skips off-Linux.
#[cfg(target_os = "linux")]
fn harden_parent_against_ptrace() {
    // SAFETY: prctl(PR_SET_DUMPABLE) only mutates this process's own flag.
    let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if rc != 0 {
        eprintln!(
            "[bwoc-harness:t7a] FATAL: prctl(PR_SET_DUMPABLE,0) failed: {} — cannot protect \
             parent memory from same-uid ptrace. Refusing to start (fail-closed). [C4]",
            std::io::Error::last_os_error()
        );
        std::process::exit(70);
    }
    match std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope") {
        Ok(s) => {
            if s.trim().parse::<i32>().unwrap_or(-1) == 0 {
                eprintln!(
                    "[bwoc-harness:t7a] FATAL: kernel.yama.ptrace_scope=0 — same-uid ptrace is \
                     permitted. A turn-executor could read the parent's API keys from RAM. \
                     Refusing to start (fail-closed). Fix: `sudo sysctl kernel.yama.ptrace_scope=1`. \
                     [C4]"
                );
                std::process::exit(70);
            }
        }
        Err(_) => {
            // yama LSM absent (file missing). PR_SET_DUMPABLE(0) already blocks
            // same-uid ptrace on its own, so warn LOUDLY instead of refusing —
            // requiring yama would needlessly break otherwise-safe kernels.
            eprintln!(
                "[bwoc-harness:t7a] WARNING: kernel.yama.ptrace_scope unreadable (yama not \
                 enabled); relying on PR_SET_DUMPABLE(0) alone to block same-uid ptrace. [C4]"
            );
        }
    }
}

/// Non-Linux: C4's mechanism (PR_SET_DUMPABLE + yama) does not apply; macOS uses
/// taskgated/SIP. No-op so the call site stays platform-neutral.
#[cfg(not(target_os = "linux"))]
fn harden_parent_against_ptrace() {}

/// Phase 5 t9 / condition 2 — the prod hard-guarantee switch for the per-turn
/// process cap. When `BWOC_REQUIRE_CGROUP_PIDS` is truthy, refuse to start unless
/// a delegated writable cgroup v2 subtree with the `pids` controller is present,
/// so the per-turn `pids.max` cap (t9) is actually enforceable instead of silently
/// degrading to the best-effort per-UID `RLIMIT_NPROC` floor.
///
/// Default OFF: dev boxes, bare-SSH logins, and non-delegated containers keep the
/// best-effort floor and start normally. Setting the flag is how a production
/// deployment demands the hard cap. The file-tracked deployment prerequisite (a
/// systemd unit drop-in with `Delegate=yes`) is **t14**; this switch is the
/// runtime assertion that the prerequisite was met. Probing here also runs the
/// (idempotent) cgroup delegation dance eagerly, so the first turn pays no setup.
fn assert_cgroup_enforcement_if_required() {
    let required = matches!(
        std::env::var("BWOC_REQUIRE_CGROUP_PIDS").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    );
    if !required {
        return;
    }
    if bwoc_harness::cgroup::delegated_subtree_available() {
        eprintln!(
            "[bwoc-harness:t9] BWOC_REQUIRE_CGROUP_PIDS satisfied: a delegated cgroup v2 pids \
             subtree is present; per-turn pids.max will be enforced. [Phase 5 t9 / condition 2]"
        );
        return;
    }
    eprintln!(
        "[bwoc-harness:t9] FATAL: BWOC_REQUIRE_CGROUP_PIDS is set but no delegated writable cgroup \
         v2 subtree with the `pids` controller is available — the per-turn pids.max cap cannot be \
         enforced and the harness would silently fall back to best-effort RLIMIT_NPROC. Refusing to \
         start (fail-closed). Fix: run under a systemd unit with `Delegate=yes` (t14 deployment \
         prereq) or unset BWOC_REQUIRE_CGROUP_PIDS to accept the RLIMIT_NPROC floor. \
         [Phase 5 t9 / condition 2]"
    );
    std::process::exit(70);
}

async fn run() -> HarnessResult<()> {
    let args = Args::parse();

    // Resolve working directory to an absolute path.
    let workdir = args.workdir.canonicalize().unwrap_or_else(|_| {
        // If the path doesn't exist yet, leave as-is and let the first tool
        // call surface the error.
        args.workdir.clone()
    });

    // ── Interactive chat session (PR1 of the chat TUI) ────────────────────
    // stdout must carry ONLY the JSON-line event stream, so this path runs
    // before the human-readable banner below and emits nothing to stdout itself.
    if args.chat {
        return run_chat_mode(&args, &workdir, false).await;
    }

    // ── Headless / served session (#301) ──────────────────────────────────
    // Same multi-turn loop as `--chat`, but driven by a machine frontend with
    // no human to answer prompts (see `run_chat_mode`'s `headless` arg).
    if args.headless {
        return run_chat_mode(&args, &workdir, true).await;
    }

    // ── Eval mode: run one fixture, score it, exit. Runs before the banner so
    // `--json` keeps stdout to a single clean JSON object.
    if let Some(fixture_dir) = args.eval.clone() {
        return run_eval_mode(&args, &fixture_dir, &workdir).await;
    }

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
    // Pick up an optional `reasoningEffort` from the agent's manifest and send
    // it as `reasoning_effort` on every completion (OpenAI-compat effort
    // control). Absent manifest / field ≡ provider default.
    let (reasoning_effort, max_tokens, prompt_cache, thinking) =
        match bwoc_core::manifest::Manifest::load_from_path(&workdir.join("config.manifest.json")) {
            Ok(m) => (
                m.reasoning_effort,
                m.max_tokens,
                m.prompt_cache.unwrap_or(true),
                m.thinking.unwrap_or(false),
            ),
            // A malformed manifest is worth surfacing — silently dropping
            // `reasoningEffort` / `maxTokens` here would make a config typo hard to
            // debug. A missing file is normal (no manifest ⇒ defaults), so stay quiet.
            Err(bwoc_core::manifest::ManifestError::Json(e)) => {
                eprintln!(
                    "[bwoc-harness] warning: config.manifest.json parse error: {e}; \
                 ignoring reasoningEffort / maxTokens / promptCache / thinking"
                );
                (None, None, true, false)
            }
            Err(_) => (None, None, true, false),
        };
    if let Some(ref e) = reasoning_effort {
        println!("  effort   : {e}");
    }
    if let Some(n) = max_tokens {
        println!("  maxTokens: {n}");
    }
    ensure_backend_credentials(&args.backend)?;
    let provider: Arc<dyn ProviderClient> = build_provider(
        &args.backend,
        &args.endpoint,
        &args.cli_cmd,
        reasoning_effort,
        max_tokens,
        prompt_cache,
        thinking,
    );

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
    let mut system_prompt = load_system_prompt(&workdir).await;
    if system_prompt.is_empty() {
        println!("  system prompt: (none — AGENTS.md / CLAUDE.md not found in workdir)");
    } else {
        println!("  system prompt: loaded ({} chars)", system_prompt.len());
    }

    // ── Tier 1 memory recall ──────────────────────────────────────────────
    // Load the MEMORY.md index into the system prompt so the agent starts the
    // session aware of its own curated memory (SRS FR-7.16), honoring the
    // manifest's `memoryPath`.
    let memory_dir = memory_dir_for(&workdir);
    if let Some(block) = tier1_recall_block(&memory_dir).await {
        println!(
            "  memory   : Tier 1 MEMORY.md recalled ({} chars)",
            block.len()
        );
        system_prompt.push_str(&block);
    }

    // ── Tier 2 deep memory (HV3-1) ────────────────────────────────────────
    // When the manifest configures `deepMemoryCmd`: wake-up output joins the
    // system prompt, a read-only `memory_search` tool is registered below,
    // and the run's checkpoint is mined at the end. All best-effort.
    let deep_memory = bwoc_harness::deep_memory::DeepMemoryCmd::from_workdir(&workdir);
    if let Some(dm) = &deep_memory {
        if let Some(prior) = dm.wake_up().await {
            println!(
                "  memory   : Tier 2 wake-up injected ({} chars)",
                prior.len()
            );
            system_prompt.push_str(&bwoc_harness::deep_memory::wake_up_block(&prior));
        } else {
            println!("  memory   : Tier 2 configured (no prior context)");
        }
    }

    // ── Tool registry ─────────────────────────────────────────────────────
    let mut registry = default_registry();
    if let Some(dm) = &deep_memory {
        registry.register(bwoc_harness::deep_memory::MemorySearch::new(dm.clone()));
    }
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
    // Remote MCP servers over Streamable HTTP. The server label (and secrets
    // token key prefix) is the URL host. Same fail-soft posture as --mcp.
    for url in &args.mcp_http {
        let host = url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split(['/', ':']).next())
            .filter(|h| !h.is_empty())
            .unwrap_or(url.as_str());
        // Sanitize the host into a label usable as both a tool-name prefix
        // segment and a bare TOML secrets key: `example.com` → `example_com`.
        let label: String = host
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let token = bwoc_harness::mcp::token_from_secrets(&label);
        match bwoc_harness::mcp::McpClient::connect_http(url, token).await {
            Ok(client) => match client.register_tools(&mut registry, &label).await {
                Ok(n) => println!("  mcp      : {n} tool(s) from `{url}`"),
                Err(e) => {
                    eprintln!("[bwoc-harness] warning: MCP `tools/list` from `{url}`: {e}")
                }
            },
            Err(e) => eprintln!("[bwoc-harness] warning: MCP connect `{url}`: {e}"),
        }
    }
    let registry = Arc::new(registry);

    // ── Context ───────────────────────────────────────────────────────────
    let ctx = if args.unrestricted {
        ToolContext::unconfined(&workdir)
    } else {
        ToolContext::new(&workdir)
    }
    .with_memory_dir(memory_dir.clone());

    // ── Permission policy ─────────────────────────────────────────────────
    // Load from .bwoc/harness-policy.toml relative to the workdir.
    // Falls back to a fail-safe deny-all policy if the file is absent.
    let mut policy: Policy = HarnessPolicy::load(&workdir)
        .unwrap_or_else(|e| {
            eprintln!(
                "[bwoc-harness] warning: could not load harness-policy.toml: {e}. \
                 Using fail-safe deny-all policy."
            );
            bwoc_harness::policy::HarnessPolicy::default()
        })
        .into();

    // Human-in-the-loop approval console (opt-in). Only meaningful without a TTY
    // (with one, `ask` prompts on stdin as before); attaching it is harmless
    // otherwise since the channel is consulted only on the non-TTY `ask` path.
    if args.approval_channel {
        policy.agent_id = args.agent.clone();
        policy.approval = Some(std::sync::Arc::new(
            bwoc_harness::policy::FileApprovalChannel::new(workdir.join(".bwoc").join("approvals")),
        ));
    }

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
                // The interactive `--task` flag is the local operator invoking
                // the CLI directly — the only Trusted ingress (Phase 5 t2). This
                // keeps the batch entrypoint able to drive effectful tools while
                // connector/queue ingress (which flows through `ingest()` /
                // `user()`) stays Untrusted and capability-gated.
                vec![ChatMessage::operator(&task)],
            )
        }
    };

    // The run's checkpoint file doubles as the mining artifact: it carries the
    // full history, so `mine` turns this session into tomorrow's memory.
    let mine_artifact = checkpoint.as_ref().map(|c| c.path());

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

    // ── Tier 2 mine (HV3-1) ───────────────────────────────────────────────
    // Session end: persist this run into deep memory. Best-effort — memory
    // trouble never changes the run's outcome. Two shapes:
    //   - success: the checkpoint dir was already cleaned up (finished runs
    //     don't linger), so distil the run into a small transcript
    //     (`.bwoc/last-run.md`, overwritten per run) and mine that — the
    //     task → outcome pair is the memory-worthy distillate anyway;
    //   - failure: the checkpoint survives for `--resume` — mine it as-is
    //     (failed runs are exactly what's worth remembering).
    if let Some(dm) = &deep_memory {
        match (&outcome, &mine_artifact) {
            (Ok(res), _) => {
                let transcript = format!(
                    "## Task\n\n{}\n\n## Outcome ({} turn(s))\n\n{}\n",
                    args.task.as_deref().unwrap_or("(resumed run)"),
                    res.turns,
                    res.final_response
                );
                let bwoc_dir = workdir.join(".bwoc");
                let path = bwoc_dir.join("last-run.md");
                let _ = std::fs::create_dir_all(&bwoc_dir);
                if std::fs::write(&path, transcript).is_ok() {
                    dm.mine(&path, "run").await;
                }
            }
            (Err(_), Some(artifact)) => dm.mine(artifact, "run").await,
            (Err(_), None) => {}
        }
    }

    // ── Worker result envelope (HV3-3b) ───────────────────────────────────
    // Leave a structured outcome in the worktree for a Saṅgha lead to collect
    // (it can't read this process's return value). Written on success AND
    // failure, before the abort propagates below; best-effort like the mine.
    {
        use bwoc_harness::result::{DiffSummary, WorkerResult};
        let task = args
            .task
            .clone()
            .unwrap_or_else(|| "(resumed run)".to_string());
        let diff = DiffSummary::from_worktree(&workdir);
        let envelope = match &outcome {
            Ok(res) => WorkerResult::completed(task, res, diff),
            Err(e) => WorkerResult::aborted(task, args.model.clone(), diff, &e.to_string()),
        };
        if let Err(e) = envelope.write(&workdir) {
            eprintln!("[bwoc-harness] warning: could not write worker result: {e}");
        }
    }

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
    use bwoc_harness::review::SubprocessReviewer;
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
            // Propagate the scalar budget/vetted flags the operator set on the
            // lead so a worker enforces the same limits (contrast the non-lead
            // path, which feeds these straight into BudgetConfig/VettedMode).
            // `token_budget`/`cost_limit`/`cost_per_1m` are Option — forwarded
            // only when set. `vetted_mode` always has a clap default ("warn"),
            // so it is always forwarded.
            token_budget: args.token_budget,
            cost_limit: args.cost_limit,
            cost_per_1m: args.cost_per_1m,
            vetted_mode: Some(args.vetted_mode.clone()),
        },
        capacity: args.concurrency,
        max_tasks: args.max_tasks,
        // Peer-review gate (HV3-3c): operator-supplied reviewer agent. Filtered
        // through the same placeholder rule so an unset/blank flag = no gate.
        reviewer: args
            .reviewer
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };
    let reviewer = std::sync::Arc::new(SubprocessReviewer::new()?);

    println!(
        "  mode     : Saṅgha lead (agent={}, tasks={}{})",
        cfg.agent_id,
        tasks_path.display(),
        cfg.reviewer
            .as_deref()
            .map(|r| format!(", reviewer={r}"))
            .unwrap_or_default()
    );
    println!("─────────────────────────────────────────────");

    let summary = run_lead(&source, runner, reviewer, &cfg).await?;

    println!("─────────────────────────────────────────────");
    println!(
        "lead done: {} claimed, {} completed, {} rejected, {} failed.",
        summary.claimed, summary.completed, summary.rejected, summary.failed
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive chat mode (PR1 of the chat TUI)
// ---------------------------------------------------------------------------

/// Set up the provider, tools, policy, and system prompt — mirroring the batch
/// `run()` path — then hand off to the [`chat_session`] driver, which speaks the
/// `bwoc_core::chat_proto` JSON-line protocol over stdin/stdout.
///
/// No setup output goes to stdout: the chat client reads that stream as JSON
/// events. Status/warnings (model resolution, policy load) go to stderr.
/// Build the provider client for `backend`. OpenAI-compatible backends
/// (`ollama` / `openai-compatible`) hit `endpoint` directly; `openrouter` is the
/// same client with bearer auth + attribution headers, substituting OpenRouter's
/// base URL when `--endpoint` is unset; `litellm` is the same client pointed at
/// a self-hosted proxy — base from `LITELLM_API_BASE` (env) or the LiteLLM
/// default port when `--endpoint` is unset, with **optional** bearer auth
/// (attached only when a `LITELLM_API_KEY` resolves); Anthropic backends (`claude` /
/// `anthropic`) use the Messages API (key from `ANTHROPIC_API_KEY`),
/// substituting the Anthropic default endpoint when the caller left the
/// OpenAI/Ollama default in place — i.e. a `claude` agent with no manifest
/// `baseUrl`. `reasoning_effort` only applies to the OpenAI-compatible path.
/// Eval mode: run one fixture directory against the configured backend, score
/// it, print the [`EvalResult`], and exit 0 (pass or skip) / 1 (fail). A failed
/// fixture is a normal eval *outcome*, not a harness error, so it sets the exit
/// code directly rather than returning `Err` (which would read as a crash).
async fn run_eval_mode(
    args: &Args,
    fixture_dir: &std::path::Path,
    workdir: &std::path::Path,
) -> HarnessResult<()> {
    use bwoc_harness::eval::{Fixture, run_fixture};
    use bwoc_harness::policy::permission::Mode;
    use std::io::Write as _;

    ensure_backend_credentials(&args.backend)?;

    let fixture_toml = fixture_dir.join("fixture.toml");
    let raw = std::fs::read_to_string(&fixture_toml)?; // Io is #[from] → exit 1
    let fixture = Fixture::from_toml(&raw).map_err(|e| {
        bwoc_harness::error::HarnessError::Other(format!("parse {}: {e}", fixture_toml.display()))
    })?;

    // An eval work dir usually has no manifest (a fresh temp dir), so absence is
    // normal and silent — but a *present-but-malformed* one is worth surfacing
    // (mirrors the main run path), not silently dropped.
    let manifest_path = workdir.join("config.manifest.json");
    let (reasoning_effort, max_tokens, prompt_cache, thinking) = if manifest_path.exists() {
        match bwoc_core::manifest::Manifest::load_from_path(&manifest_path) {
            Ok(m) => (
                m.reasoning_effort,
                m.max_tokens,
                m.prompt_cache.unwrap_or(true),
                m.thinking.unwrap_or(false),
            ),
            Err(e) => {
                eprintln!(
                    "[bwoc-harness] warning: ignoring malformed {}: {e}",
                    manifest_path.display()
                );
                (None, None, true, false)
            }
        }
    } else {
        (None, None, true, false)
    };
    let provider = build_provider(
        &args.backend,
        &args.endpoint,
        &args.cli_cmd,
        reasoning_effort,
        max_tokens,
        prompt_cache,
        thinking,
    );

    let config = LoopConfig {
        model: args.model.clone(),
        fallback_models: Vec::new(),
        vetted_models: Vec::new(),
        vetted_mode: VettedMode::Warn,
        max_iterations: args.max_iterations,
        stream: false,
        // Eval runs in an isolated, seeded work dir — a controlled benchmark, not
        // untrusted input — so tools are allowed: a tool-requiring fixture must be
        // able to write files / run gates to be scorable at all.
        policy: Policy {
            default_mode: Mode::Allow,
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
            ..Default::default()
        },
        is_tty: false,
        context_limit: 0,
        model_context_limits: std::collections::HashMap::new(),
        token_pressure_models: Vec::new(),
        checkpoint: None,
        budget: bwoc_harness::budget::BudgetConfig::default(),
    };

    let result = run_fixture(
        &fixture,
        fixture_dir,
        workdir,
        provider,
        config,
        &args.backend,
    )
    .await?;

    if args.json {
        // A serialization failure is a real fault — surface it and exit non-zero
        // rather than emitting a misleading empty `{}` with a success status.
        match serde_json::to_string_pretty(&result) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("[bwoc-harness] eval: failed to serialize result: {e}");
                let _ = std::io::stdout().flush();
                std::process::exit(1);
            }
        }
    } else {
        let status = if result.skipped {
            "SKIP"
        } else if result.passed {
            "PASS"
        } else {
            "FAIL"
        };
        let passed = result.checks.iter().filter(|c| c.passed).count();
        println!(
            "[{status}] {} — score {:.2} ({passed}/{} checks), {} turn(s)",
            result.fixture_id,
            result.score,
            result.checks.len(),
            result.turns
        );
        if let Some(reason) = &result.skip_reason {
            println!("  skipped: {reason}");
        }
        for c in &result.checks {
            let mark = if c.passed { "✓" } else { "✗" };
            println!("  {mark} {} — {}", c.check, c.detail);
        }
    }

    // Flush before the explicit exit (stdout is block-buffered when piped).
    let _ = std::io::stdout().flush();
    // Skip = structural non-score, treated as success (the suite drops it).
    std::process::exit(if result.passed || result.skipped {
        0
    } else {
        1
    });
}

fn build_provider(
    backend: &str,
    endpoint: &str,
    cli_cmd: &str,
    reasoning_effort: Option<String>,
    max_tokens: Option<u32>,
    prompt_cache: bool,
    thinking: bool,
) -> Arc<dyn ProviderClient> {
    use bwoc_harness::provider::client as oai;
    match backend {
        // Local subscription-authenticated vendor CLI (#277): one subprocess
        // per turn, no key, chat-only (the CLI runs its own tools internally).
        // AMBIENT backend (t30): the vendor CLI executes its OWN tools outside
        // the harness, so the Phase 5 capability gate / FS jail / egress filter
        // do NOT reach them — the `#271` "Untrusted turn is read-only" guarantee
        // is structurally unenforceable here. Warn loudly so an interactive
        // operator makes an informed choice (the gateway auto-process path
        // refuses this backend outright; see `bwoc-agent` autoprocess).
        "cli" => {
            eprintln!(
                "[bwoc-harness] ⚠ SECURITY: `--backend cli` is an AMBIENT backend — the vendor \
                 CLI `{cli_cmd}` runs its own tools with full ambient authority. Harness \
                 tool-confinement (capability gate, FS jail, egress filter) does NOT apply, so \
                 the Untrusted-turn read-only guarantee (#271) is NOT enforced. Use an HTTP \
                 backend for any session that processes untrusted input."
            );
            Arc::new(CliClient::new(cli_cmd))
        }
        "claude" | "anthropic" => {
            let base = if endpoint == oai::DEFAULT_ENDPOINT {
                bwoc_harness::provider::anthropic::ANTHROPIC_DEFAULT_ENDPOINT
            } else {
                endpoint
            };
            Arc::new(
                AnthropicClient::new(base)
                    .with_reasoning_effort(reasoning_effort)
                    .with_max_tokens(max_tokens)
                    .with_prompt_cache(prompt_cache)
                    .with_thinking(thinking),
            )
        }
        "openrouter" => {
            // OpenRouter is OpenAI-compatible, so it reuses OllamaClient — the
            // only additions are bearer auth (key from OPENROUTER_API_KEY /
            // secrets.toml) and the optional attribution headers. Swap the
            // Ollama-localhost default for OpenRouter's base when the caller
            // left `--endpoint` unset (mirrors the Anthropic default swap above).
            let base = if endpoint == oai::DEFAULT_ENDPOINT {
                oai::OPENROUTER_DEFAULT_ENDPOINT
            } else {
                endpoint
            };
            Arc::new(
                OllamaClient::new(base)
                    .with_api_key(Some(oai::resolve_openrouter_api_key()))
                    .with_headers(oai::openrouter_headers())
                    .with_reasoning_effort(reasoning_effort)
                    .with_max_tokens(max_tokens),
            )
        }
        "litellm" => {
            // LiteLLM is a self-hosted, OpenAI-compatible proxy, so it reuses
            // OllamaClient. Unlike OpenRouter it has no canonical URL: when
            // `--endpoint` is left at the default, resolve the base from
            // `LITELLM_API_BASE` (env) or the LiteLLM default port — never a
            // hardcoded infra host. The key is OPTIONAL (a local proxy may be
            // keyless), so attach bearer auth only when one actually resolves.
            let base = if endpoint == oai::DEFAULT_ENDPOINT {
                oai::resolve_litellm_endpoint()
            } else {
                endpoint.to_string()
            };
            let key = oai::resolve_litellm_api_key();
            let client = OllamaClient::new(&base)
                .with_reasoning_effort(reasoning_effort)
                .with_max_tokens(max_tokens);
            let client = if key.trim().is_empty() {
                client
            } else {
                client.with_api_key(Some(key))
            };
            Arc::new(client)
        }
        _ => Arc::new(
            OllamaClient::new(endpoint)
                .with_reasoning_effort(reasoning_effort)
                .with_max_tokens(max_tokens),
        ),
    }
}

/// Fail fast with an actionable message when a backend needs credentials that
/// live outside the (generic) OpenAI-compatible client and would otherwise only
/// surface as a bare `HTTP 401` at the first request. Mirrors
/// [`AnthropicClient::require_key`]'s contract, but for `openrouter` — whose key
/// the harness resolves, not the shared `OllamaClient`. A no-op for every other
/// backend (vendor CLIs and plain Ollama need no key here).
fn ensure_backend_credentials(backend: &str) -> HarnessResult<()> {
    use bwoc_harness::provider::client as oai;
    if backend == "openrouter" && oai::resolve_openrouter_api_key().trim().is_empty() {
        return Err(bwoc_harness::error::HarnessError::Provider(format!(
            "no OpenRouter API key — set `{env}` (e.g. `export {env}=sk-or-...`) or add an \
             `[openrouter] api_key = \"sk-or-...\"` entry to ~/.bwoc/secrets.toml (chmod 600)",
            env = oai::OPENROUTER_API_KEY_ENV
        )));
    }
    Ok(())
}

/// Drives the interactive `--chat` loop, or — when `headless` is true — the
/// served `--headless` variant (#301): identical wiring (provider, system
/// prompt, deep memory, tools), but the session starts in auto-approve mode so a
/// machine frontend's turn never blocks on a permission prompt. Both paths reuse
/// the same `chat_session` driver and `.bwoc/chat-session.json` persistence.
async fn run_chat_mode(
    args: &Args,
    workdir: &std::path::Path,
    headless: bool,
) -> HarnessResult<()> {
    use bwoc_harness::chat_session::{self, ChatConfig};

    // Provider — same reasoning-effort / max-tokens wiring as run().
    let (reasoning_effort, max_tokens, prompt_cache, thinking) =
        match bwoc_core::manifest::Manifest::load_from_path(&workdir.join("config.manifest.json")) {
            Ok(m) => (
                m.reasoning_effort,
                m.max_tokens,
                m.prompt_cache.unwrap_or(true),
                m.thinking.unwrap_or(false),
            ),
            Err(bwoc_core::manifest::ManifestError::Json(e)) => {
                eprintln!(
                    "[bwoc-harness] warning: config.manifest.json parse error: {e}; \
                 ignoring reasoningEffort / maxTokens / promptCache / thinking"
                );
                (None, None, true, false)
            }
            Err(_) => (None, None, true, false),
        };
    ensure_backend_credentials(&args.backend)?;
    let provider: Arc<dyn ProviderClient> = build_provider(
        &args.backend,
        &args.endpoint,
        &args.cli_cmd,
        reasoning_effort,
        max_tokens,
        prompt_cache,
        thinking,
    );

    // Resolve the `auto` model sentinel the same way the batch path does (see
    // the `AUTO_SENTINEL` branch in `run()`), BEFORE validation and before the
    // session is built. Without this, chat validates and then loops on the
    // literal string "auto" — which no provider serves — so an agent configured
    // `primaryModel: "auto"` cannot chat (#347). Chat is interactive, so there is
    // no `--task` to classify: the resolver picks its default from the pool.
    let resolved_model = if args.model == bwoc_harness::model_select::AUTO_SENTINEL {
        // Progress goes to STDERR: in chat/headless mode stdout carries the
        // `chat_proto` JSON-line protocol, so a stray human line there would
        // corrupt the event stream the frontend parses.
        let candidates = match bwoc_core::manifest::Manifest::load_from_path(
            &workdir.join("config.manifest.json"),
        ) {
            Ok(m) => m.auto_models.unwrap_or_default(),
            Err(bwoc_core::manifest::ManifestError::Json(e)) => {
                eprintln!(
                    "[bwoc-harness] warning: config.manifest.json parse error: {e}; \
                     no auto-model candidates"
                );
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        eprintln!(
            "[bwoc-harness] resolving auto model from {} candidate(s)...",
            candidates.len()
        );
        let sel =
            bwoc_harness::model_select::resolve_auto(provider.as_ref(), &candidates, "").await?;
        eprintln!("[bwoc-harness] auto model → {}", sel.chosen);
        sel.chosen
    } else {
        args.model.clone()
    };

    if !args.skip_model_check {
        provider.validate_model(&resolved_model).await?;
    }

    // System prompt (AGENTS.md / CLAUDE.md), same as run().
    let mut system_prompt = load_system_prompt(workdir).await;

    // Tier 1 memory recall — same as run(): MEMORY.md index into the system
    // prompt, honoring the manifest's `memoryPath`.
    let memory_dir = memory_dir_for(workdir);
    if let Some(block) = tier1_recall_block(&memory_dir).await {
        system_prompt.push_str(&block);
    }

    // Tier 2 deep memory (HV3-1) — same wiring as run(): wake-up into the
    // system prompt, memory_search tool, mine on session end.
    let deep_memory = bwoc_harness::deep_memory::DeepMemoryCmd::from_workdir(workdir);
    if let Some(dm) = &deep_memory {
        if let Some(prior) = dm.wake_up().await {
            system_prompt.push_str(&bwoc_harness::deep_memory::wake_up_block(&prior));
        }
    }

    // Tool registry + context, same as run() (no MCP in the chat v1 driver).
    let mut registry = default_registry();
    if let Some(dm) = &deep_memory {
        registry.register(bwoc_harness::deep_memory::MemorySearch::new(dm.clone()));
    }
    let registry = Arc::new(registry);
    let ctx = if args.unrestricted {
        ToolContext::unconfined(workdir)
    } else {
        ToolContext::new(workdir)
    }
    .with_memory_dir(memory_dir.clone());

    // Permission policy. A `.bwoc/harness-policy.toml` wins; otherwise — unlike
    // the batch path's fail-safe deny — chat defaults to **ask** (reads free,
    // writes/edits/run prompt the frontend's Allow/Deny), because an interactive
    // client is always present to answer. This is what makes a file-editing chat
    // usable out of the box without a hand-written policy.
    let policy: Policy = if workdir.join(".bwoc").join("harness-policy.toml").is_file() {
        HarnessPolicy::load(workdir)
            .unwrap_or_else(|e| {
                eprintln!(
                    "[bwoc-harness] warning: could not load harness-policy.toml: {e}. \
                     Using ask-by-default chat policy."
                );
                HarnessPolicy::default()
            })
            .into()
    } else {
        chat_default_policy()
    };

    // Agent id from the manifest when present, else the --agent fallback.
    let agent =
        bwoc_core::manifest::Manifest::load_from_path(&workdir.join("config.manifest.json"))
            .map(|m| m.agent_id)
            .unwrap_or_else(|_| args.agent.clone());

    let config = ChatConfig {
        agent,
        model: resolved_model.clone(),
        backend: args.backend.clone(),
        system_prompt,
        policy,
        max_turn_iterations: args.max_iterations,
        max_context_tokens: bwoc_harness::chat_session::DEFAULT_MAX_CONTEXT_TOKENS,
        // Team chat broadcast (HV3-3a): `--team-chat <path>` opts this session
        // into a team's shared `chat.jsonl`. The host (`bwoc chat --team`)
        // resolves the workspace-relative path; unset = solo session.
        team_chat_log: args.team_chat.clone(),
        // #301: served mode auto-approves `ask` tools (no human to prompt);
        // guardrails + deny rules + sandbox still confine the session.
        headless,
    };

    let outcome = chat_session::run(provider, registry, ctx, config).await;

    // Tier 2 mine (HV3-1): the persisted conversation becomes memory. The
    // chat driver saves `.bwoc/chat-session.json` after each turn, so this
    // captures the whole session regardless of how it ended.
    if let Some(dm) = &deep_memory {
        let mode = if headless { "served" } else { "chat" };
        dm.mine(&workdir.join(".bwoc").join("chat-session.json"), mode)
            .await;
    }

    outcome
}

/// The fallback permission policy for `--chat` when the workdir has no
/// `.bwoc/harness-policy.toml`: read-only tools run freely; everything else
/// (write/edit/run/git/…) is `ask`, surfaced to the frontend as an Allow/Deny
/// prompt. Chat always has an interactive client to answer, so `ask` is a safe
/// default — and the one that makes file editing work without setup.
fn chat_default_policy() -> Policy {
    use bwoc_harness::policy::permission::Mode;
    let read_only = [
        "read_file",
        "list_dir",
        "grep",
        "memory_read",
        "memory_search",
    ];
    let tools = read_only
        .iter()
        .map(|t| (t.to_string(), Mode::Allow))
        .collect();
    Policy {
        default_mode: Mode::Ask,
        tools,
        patterns: Vec::new(),
        ..Default::default()
    }
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

/// Resolve the agent's tier-1 memory directory from the manifest's `memoryPath`
/// (default `memories/`), relative to the worktree. Honors a configured override
/// instead of hardcoding `memories/`. A missing/malformed manifest ⇒ the default.
///
/// The `memoryPath` is confined to the worktree: an absolute path or one with a
/// `..` component is rejected (falls back to `memories/`) so a crafted manifest
/// can't point recall/tools at an out-of-tree file — recall injects the index
/// into the system prompt, so an escape would be an exfiltration vector.
fn memory_dir_for(workdir: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, Path};
    let rel = bwoc_core::manifest::Manifest::load_from_path(&workdir.join("config.manifest.json"))
        .ok()
        .map(|m| m.memory_path)
        .filter(|p| !p.trim().is_empty())
        .filter(|p| {
            // Accept only a pure relative path of Normal segments. This rejects
            // absolute paths (`Prefix`/`RootDir`), `..` (`ParentDir`), AND a
            // leading `/` — cross-platform: on Windows `/etc` isn't `is_absolute()`
            // but its leading `RootDir` still escapes, so a Normal-only check is
            // the portable guard.
            Path::new(p)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        })
        .unwrap_or_else(|| "memories".to_string());
    workdir.join(rel)
}

/// Tier-1 boot recall: load the `MEMORY.md` index from `memory_dir` and render a
/// system-prompt block, or `None` when there's no index. The Tier-1 counterpart
/// to Tier-2 wake-up — closes the gap where an agent started each session blind
/// to its own curated memory (SRS FR-7.16). Injects the *index*; individual
/// memory files stay behind the `memory_read` tool (Mattaññutā — don't bloat the
/// prompt).
async fn tier1_recall_block(memory_dir: &std::path::Path) -> Option<String> {
    let idx = tokio::fs::read_to_string(memory_dir.join("MEMORY.md"))
        .await
        .ok()?;
    let idx = idx.trim();
    if idx.is_empty() {
        return None;
    }
    Some(format!(
        "\n\n# Your saved memory — Tier 1 index (MEMORY.md)\n\n\
         Consult this before acting. Each entry is a past claim: verify any file, \
         function, or flag it names against the current code before relying on it \
         (Yoniso Manasikāra); trust the code over memory on conflict, then update \
         the memory.\n\n{idx}\n"
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_backend_credentials_is_noop_for_non_openrouter() {
        // Vendor CLIs and plain Ollama need no key at this gate — never errors
        // regardless of the ambient OPENROUTER_API_KEY env. (The openrouter
        // error path is env/home-dependent, so it's covered by the live smoke
        // test rather than a race-prone unit test that mutates process env.)
        for backend in ["ollama", "openai-compatible", "claude", "anthropic"] {
            assert!(ensure_backend_credentials(backend).is_ok());
        }
    }

    #[tokio::test]
    async fn tier1_recall_reads_memory_md_index() {
        let tmp = TempDir::new().unwrap();
        let mem = tmp.path().join("memories");
        tokio::fs::create_dir_all(&mem).await.unwrap();
        // No index yet → no block.
        assert!(tier1_recall_block(&mem).await.is_none());
        // Index present → block carries its content + the verify reminder.
        tokio::fs::write(mem.join("MEMORY.md"), "- [thing](thing.md) — hook")
            .await
            .unwrap();
        let block = tier1_recall_block(&mem).await.unwrap();
        assert!(block.contains("Tier 1 index"));
        assert!(block.contains("thing.md"));
        assert!(block.contains("Yoniso"));
    }

    #[tokio::test]
    async fn memory_dir_for_honors_manifest_path_else_defaults() {
        let tmp = TempDir::new().unwrap();
        // No manifest → default `memories/`.
        assert_eq!(memory_dir_for(tmp.path()), tmp.path().join("memories"));
        // Manifest override → honored.
        tokio::fs::write(
            tmp.path().join("config.manifest.json"),
            r#"{"agentId":"agent-x","name":"x","agentRole":"r","primaryModel":"m",
                "memoryPath":"brain/","lintCmd":"true","formatCmd":"true",
                "testCmd":"true","buildCmd":"true","version":"2.0"}"#,
        )
        .await
        .unwrap();
        assert_eq!(memory_dir_for(tmp.path()), tmp.path().join("brain/"));
    }

    #[tokio::test]
    async fn memory_dir_for_rejects_escaping_memory_path() {
        // A crafted `memoryPath` (absolute or `..`) must not point recall/tools
        // outside the worktree — it falls back to the default `memories/`.
        for bad in ["../../etc", "/etc", "a/../../b"] {
            let tmp = TempDir::new().unwrap();
            tokio::fs::write(
                tmp.path().join("config.manifest.json"),
                format!(
                    r#"{{"agentId":"agent-x","name":"x","agentRole":"r","primaryModel":"m",
                        "memoryPath":"{bad}","lintCmd":"true","formatCmd":"true",
                        "testCmd":"true","buildCmd":"true","version":"2.0"}}"#
                ),
            )
            .await
            .unwrap();
            assert_eq!(
                memory_dir_for(tmp.path()),
                tmp.path().join("memories"),
                "escaping memoryPath `{bad}` must fall back to memories/"
            );
        }
    }

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
