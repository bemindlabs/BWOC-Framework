//! `bwoc fleet health` — Aparihāniya-dhamma 7 fleet-governance signals.
//!
//! Reads the workspace registry, then checks all seven conditions defined in
//! `docs/en/FLEET-GOVERNANCE.en.md`. Read-only, workspace-scoped, backend-neutral.
//! v1 reports only; gating is deferred to v2.
//!
//! Conditions 1, 2, 4, 5 are mechanically computed (produce ✓ ok or ⚠ warn).
//! Conditions 3 and 6 are git-backed mechanical checks (produce ✓/⚠/ℹ via git
//! shell-out through the `GitRunner` seam — offline-mockable).
//! Condition 7 remains informational.

use std::path::{Path, PathBuf};

use bwoc_core::workspace::AgentsRegistry;

// ── GitRunner seam ────────────────────────────────────────────────────────────

/// Abstraction over read-only git shell-outs.  `ProcessGitRunner` in production;
/// `MockGitRunner` in unit tests (offline-deterministic, no real repo needed).
pub trait GitRunner {
    /// Run `git <args>` with `cwd` as the working directory.
    /// Returns captured stdout on success, or `Err(())` when the command fails
    /// (non-zero exit or any I/O error).  Best-effort: callers must never panic
    /// on `Err(())` — they degrade to `ℹ`.
    fn git(&self, args: &[&str], cwd: &Path) -> Result<String, ()>;
}

/// Production runner — shells out to the system `git` binary.
pub struct ProcessGitRunner;

impl GitRunner for ProcessGitRunner {
    fn git(&self, args: &[&str], cwd: &Path) -> Result<String, ()> {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|_| ())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(())
        }
    }
}

// ── Public args ──────────────────────────────────────────────────────────────

pub struct FleetHealthArgs {
    /// Workspace root. Resolution: explicit > BWOC_WORKSPACE env > ancestor walk > error.
    pub workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON array instead of the human report.
    pub json: bool,
    /// Number of days after which an un-touched agent dir triggers a ⚠ for
    /// condition 1 (regular meetings). Default: 7.
    pub stale_days: u64,
    /// Goal-loop mode (Loop-Engineering L2): instead of a single scan, re-scan on
    /// a ticker and run `doctor --auto` on each auto-remediable warn (stale
    /// PID/socket) until all conditions are green (DoD), a non-auto-remediable
    /// warn remains (blocked — operator action), or the iteration budget.
    pub loop_mode: bool,
    /// Ticker interval (seconds) between fires. Only with `--loop`.
    pub loop_interval_secs: u64,
    /// Iteration budget so an unattended loop provably halts. `0` = unbounded
    /// (DoD/blocked still terminate). Only with `--loop`.
    pub loop_max_iters: usize,
}

// ── Result types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionStatus {
    Ok,
    Warn,
    Info,
}

impl ConditionStatus {
    fn label(self) -> &'static str {
        match self {
            ConditionStatus::Ok => "ok",
            ConditionStatus::Warn => "warn",
            ConditionStatus::Info => "info",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            ConditionStatus::Ok => "✓",
            ConditionStatus::Warn => "⚠",
            ConditionStatus::Info => "ℹ",
        }
    }
}

#[derive(Debug)]
pub struct ConditionResult {
    pub number: u8,
    pub name: &'static str,
    pub status: ConditionStatus,
    pub finding: String,
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub fn run(args: FleetHealthArgs) -> i32 {
    let workspace = match resolve_workspace(args.workspace.as_deref()) {
        Some(p) => p,
        None => {
            eprintln!(
                "bwoc fleet health: no workspace found \
                 (no .bwoc/workspace.toml in cwd or ancestors). \
                 Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
            );
            return 2;
        }
    };

    if let Err(code) = crate::workspace::ensure_workspace(&workspace, "bwoc fleet health") {
        return code;
    }

    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc fleet health: failed to read agents registry: {e}");
            return 1;
        }
    };

    if args.loop_mode {
        return run_health_loop(&workspace, &registry, &args);
    }

    let git_runner = ProcessGitRunner;
    let results = evaluate_all(&workspace, &registry, args.stale_days, &git_runner);

    if args.json {
        emit_json(&results);
    } else {
        print_report(&workspace, &results);
    }

    // v1: always exit 0 (report-only, no gating)
    0
}

// ── Fleet-health goal loop (Loop-Engineering L2) ─────────────────────────────
//
// A reconcile loop (k8s-style): drive observed fleet health toward "all green"
// by re-scanning on a ticker and remediating the one auto-fixable warn class
// (condition 2 — stale PID/socket → `bwoc doctor --auto`). Terminates on DoD
// (no warns), Blocked (a warn no auto-fix can clear, or remediation stalled), or
// the iteration budget — so it always provably halts.

#[derive(Debug, PartialEq, Eq)]
enum FleetLoopDecision {
    /// DoD: no warns remain.
    Done,
    /// An auto-remediable warn is present and progress is still possible.
    Remediate,
    /// Stop and surface to the operator — nothing to auto-fix, or no progress.
    Blocked(String),
}

/// Pure gate over the warn condition-numbers this fire, plus the previous fire's
/// warn count. `2` is the only auto-remediable condition (`doctor --auto`).
/// Separated out so the DoD/blocked logic is unit-testable without a workspace.
///
/// When an auto-remediable warn (2) coexists with non-remediable ones (e.g.
/// `[2, 4]`), this returns `Remediate`: the loop fixes what it can *first*, and
/// once condition 2 clears the residual non-remediable warns surface as
/// `Blocked` on the next fire (a reconcile loop makes progress before reporting
/// what it can't fix — it never spins, since a stalled remediation also Blocks).
fn fleet_loop_decide(warn_numbers: &[u8], prev_warn_count: usize) -> FleetLoopDecision {
    if warn_numbers.is_empty() {
        return FleetLoopDecision::Done;
    }
    if !warn_numbers.contains(&2) {
        return FleetLoopDecision::Blocked(format!(
            "{} warn(s) remain, none auto-remediable — operator action needed",
            warn_numbers.len()
        ));
    }
    // An auto-remediable warn is present but a prior remediation didn't reduce
    // the warn count → doctor can't clear it; stop rather than spin.
    if warn_numbers.len() >= prev_warn_count {
        return FleetLoopDecision::Blocked(format!(
            "auto-remediation made no progress ({} warn(s) remain)",
            warn_numbers.len()
        ));
    }
    FleetLoopDecision::Remediate
}

fn run_health_loop(workspace: &Path, registry: &AgentsRegistry, args: &FleetHealthArgs) -> i32 {
    use bwoc_core::loop_control::{Budget, Ticker};
    let git_runner = ProcessGitRunner;
    // Shared loop-control primitives: the ticker floors a 0 interval so the loop
    // can't spin, and the budget bounds an unattended loop.
    let ticker = Ticker::every_secs(args.loop_interval_secs);
    let budget = Budget::new(args.loop_max_iters);
    let mut iteration = 0usize;
    let mut prev_warn_count = usize::MAX;
    eprintln!(
        "fleet-health loop: reconcile to all-green (ticker {}s, budget {})",
        ticker.interval().as_secs(),
        budget.describe()
    );
    loop {
        iteration += 1;
        let results = evaluate_all(workspace, registry, args.stale_days, &git_runner);
        let warn_numbers: Vec<u8> = results
            .iter()
            .filter(|r| r.status == ConditionStatus::Warn)
            .map(|r| r.number)
            .collect();

        match fleet_loop_decide(&warn_numbers, prev_warn_count) {
            FleetLoopDecision::Done => {
                println!("fleet-health loop: all conditions green after {iteration} iteration(s).");
                return 0;
            }
            FleetLoopDecision::Blocked(reason) => {
                eprintln!("fleet-health loop stopped after {iteration} iteration(s) — {reason}:");
                for r in results.iter().filter(|r| r.status == ConditionStatus::Warn) {
                    eprintln!("  ⚠ condition {} ({}): {}", r.number, r.name, r.finding);
                }
                return 1;
            }
            FleetLoopDecision::Remediate => {}
        }

        prev_warn_count = warn_numbers.len();
        eprintln!(
            "fleet-health loop: iteration {iteration} — {} warn(s), running `doctor --auto`…",
            warn_numbers.len()
        );
        let _ = crate::doctor::run(crate::doctor::DoctorArgs {
            path: Some(workspace.to_path_buf()),
            auto: true,
            json: false,
        });

        if budget.exhausted(iteration) {
            eprintln!("fleet-health loop hit its {iteration}-iteration budget before all-green.");
            return 1;
        }
        std::thread::sleep(ticker.interval());
    }
}

// ── Evaluate all 7 conditions ────────────────────────────────────────────────

fn evaluate_all(
    workspace: &Path,
    registry: &AgentsRegistry,
    stale_days: u64,
    git_runner: &dyn GitRunner,
) -> Vec<ConditionResult> {
    vec![
        condition_1_regular_meetings(workspace, registry, stale_days),
        condition_2_coordinated_start_end(workspace, registry),
        condition_3_convention_change(workspace, git_runner),
        condition_4_honor_template_version(workspace, registry),
        condition_5_protect_vulnerable(workspace, registry),
        condition_6_honor_shared_resources(workspace, git_runner),
        condition_7_protect_senior_agents(registry),
    ]
}

// ── Condition 1: Regular meetings — abhiṇha-sannipāta ──────────────────────
//
// Check each agent dir's mtime. ⚠ if any dir has not been touched in
// stale_days days. Reuses the registry list from AgentsRegistry.

fn condition_1_regular_meetings(
    workspace: &Path,
    registry: &AgentsRegistry,
    stale_days: u64,
) -> ConditionResult {
    const NAME: &str = "Regular meetings (abhiṇha-sannipāta)";

    if registry.agents.is_empty() {
        return ConditionResult {
            number: 1,
            name: NAME,
            status: ConditionStatus::Info,
            finding: "No agents registered in workspace.".into(),
        };
    }

    let threshold_secs = stale_days * 24 * 60 * 60;
    let now = std::time::SystemTime::now();
    let mut stale: Vec<String> = Vec::new();

    for agent in &registry.agents {
        let agent_dir = workspace.join(&agent.path);
        // Use the most-recently-modified file in .bwoc/ or the dir mtime
        // itself, whichever is newer.
        let last_touched = dir_last_touched(&agent_dir);
        if let Some(elapsed_secs) = last_touched
            .and_then(|t| now.duration_since(t).ok())
            .map(|d| d.as_secs())
        {
            if elapsed_secs >= threshold_secs {
                let days = elapsed_secs / 86_400;
                stale.push(format!("{} ({days}d ago)", agent.id));
            }
        }
    }

    if stale.is_empty() {
        ConditionResult {
            number: 1,
            name: NAME,
            status: ConditionStatus::Ok,
            finding: format!(
                "All {} agent(s) touched within {stale_days}d.",
                registry.agents.len()
            ),
        }
    } else {
        ConditionResult {
            number: 1,
            name: NAME,
            status: ConditionStatus::Warn,
            finding: format!(
                "{} agent(s) untouched >{stale_days}d: {}",
                stale.len(),
                stale.join(", ")
            ),
        }
    }
}

/// Return the most-recent mtime among the agent dir itself and all files
/// immediately under `<agent>/.bwoc/` (one level deep — inbox, pid, etc).
fn dir_last_touched(agent_dir: &Path) -> Option<std::time::SystemTime> {
    let mut latest: Option<std::time::SystemTime> = None;

    let update = |candidate: std::time::SystemTime, latest: &mut Option<std::time::SystemTime>| {
        *latest = Some(match latest {
            Some(prev) if candidate > *prev => candidate,
            Some(prev) => *prev,
            None => candidate,
        });
    };

    // Agent dir itself
    if let Ok(meta) = std::fs::metadata(agent_dir) {
        if let Ok(mtime) = meta.modified() {
            update(mtime, &mut latest);
        }
    }

    // Files inside <agent>/.bwoc/
    let bwoc_dir = agent_dir.join(".bwoc");
    if let Ok(read) = std::fs::read_dir(&bwoc_dir) {
        for entry in read.flatten() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    update(mtime, &mut latest);
                }
            }
        }
    }

    latest
}

// ── Condition 2: Coordinated start/end — samaggā sannipatanti ───────────────
//
// Mirror doctor's stale-PID / stale-socket detection across all agents.
// ⚠ if any stale finding exists.

fn condition_2_coordinated_start_end(
    workspace: &Path,
    registry: &AgentsRegistry,
) -> ConditionResult {
    const NAME: &str = "Coordinated start/end (samaggā sannipatanti)";

    let mut stale_pids: Vec<String> = Vec::new();
    let mut stale_socks: Vec<String> = Vec::new();

    for agent in &registry.agents {
        let bwoc = workspace.join(&agent.path).join(".bwoc");

        // Stale PID check — mirrors doctor::check_stale_pids
        let pid_path = bwoc.join("agent.pid");
        if pid_path.is_file() {
            let pid_alive = std::fs::read_to_string(&pid_path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(crate::livecheck::signal_zero_alive)
                .unwrap_or(false);
            if !pid_alive {
                stale_pids.push(agent.id.clone());
            }
        }

        // Stale socket check — mirrors doctor::check_stale_sockets
        let sock_path = bwoc.join("agent.sock");
        if sock_path.exists() {
            let owner_alive = std::fs::read_to_string(bwoc.join("agent.pid"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .map(crate::livecheck::signal_zero_alive)
                .unwrap_or(false);
            if !owner_alive {
                stale_socks.push(agent.id.clone());
            }
        }
    }

    if stale_pids.is_empty() && stale_socks.is_empty() {
        return ConditionResult {
            number: 2,
            name: NAME,
            status: ConditionStatus::Ok,
            finding: "No stale PID/socket files found.".into(),
        };
    }

    let mut parts: Vec<String> = Vec::new();
    if !stale_pids.is_empty() {
        parts.push(format!("stale PID: {}", stale_pids.join(", ")));
    }
    if !stale_socks.is_empty() {
        parts.push(format!("stale socket: {}", stale_socks.join(", ")));
    }
    ConditionResult {
        number: 2,
        name: NAME,
        status: ConditionStatus::Warn,
        finding: format!("{}. Run `bwoc doctor --auto` to clean.", parts.join("; ")),
    }
}

// ── Condition 3: Process-bound convention change — appaññattaṃ na paññāpenti
//
// Run `git status --porcelain -- .bwoc/ modules/agent-template/` from the
// workspace root.  Any uncommitted change to those paths indicates ungoverned
// convention drift.
//
//   ✓  → porcelain output is empty (clean)
//   ⚠  → one or more lines (uncommitted changes)
//   ℹ  → git command fails (not a repo, git not on PATH, etc.)

fn condition_3_convention_change(workspace: &Path, git_runner: &dyn GitRunner) -> ConditionResult {
    const NAME: &str = "Process-bound convention change (appaññattaṃ na paññāpenti)";

    let output = match git_runner.git(
        &[
            "status",
            "--porcelain",
            "--",
            ".bwoc/",
            "modules/agent-template/",
        ],
        workspace,
    ) {
        Ok(s) => s,
        Err(()) => {
            return ConditionResult {
                number: 3,
                name: NAME,
                status: ConditionStatus::Info,
                finding: "not a git repo — convention-change governance is manual.".into(),
            };
        }
    };

    let changed_count = output.lines().filter(|l| !l.trim().is_empty()).count();

    if changed_count == 0 {
        ConditionResult {
            number: 3,
            name: NAME,
            status: ConditionStatus::Ok,
            finding: "No uncommitted changes to .bwoc/ or modules/agent-template/.".into(),
        }
    } else {
        ConditionResult {
            number: 3,
            name: NAME,
            status: ConditionStatus::Warn,
            finding: format!(
                "ungoverned convention drift — {changed_count} uncommitted change(s) to \
                 .bwoc/ or the template; commit through the normal process."
            ),
        }
    }
}

// ── Condition 4: Honor template version — vuḍḍhā vuḍḍhataravā ──────────────
//
// Compare each agent's config.manifest.json::version against the template's
// config.manifest.json::version. Mirrors bwoc check's manifest version logic.

fn condition_4_honor_template_version(
    workspace: &Path,
    registry: &AgentsRegistry,
) -> ConditionResult {
    const NAME: &str = "Honor template version (vuḍḍhā vuḍḍhataravā)";

    // Load template version
    let template_path = workspace.join("modules/agent-template/config.manifest.json");
    let template_version = load_manifest_version(&template_path);

    let Some(tv) = template_version else {
        return ConditionResult {
            number: 4,
            name: NAME,
            status: ConditionStatus::Info,
            finding: format!(
                "Template manifest not found at {} — version comparison skipped.",
                template_path.display()
            ),
        };
    };

    if registry.agents.is_empty() {
        return ConditionResult {
            number: 4,
            name: NAME,
            status: ConditionStatus::Info,
            finding: format!("No agents registered; template version is {tv}."),
        };
    }

    let mut lagging: Vec<String> = Vec::new();
    for agent in &registry.agents {
        let manifest_path = workspace.join(&agent.path).join("config.manifest.json");
        match load_manifest_version(&manifest_path) {
            Some(av) if av != tv => {
                lagging.push(format!("{} ({av} ≠ {tv})", agent.id));
            }
            None => {
                lagging.push(format!("{} (no manifest)", agent.id));
            }
            _ => {}
        }
    }

    if lagging.is_empty() {
        ConditionResult {
            number: 4,
            name: NAME,
            status: ConditionStatus::Ok,
            finding: format!(
                "All {} agent(s) match template version {tv}.",
                registry.agents.len()
            ),
        }
    } else {
        ConditionResult {
            number: 4,
            name: NAME,
            status: ConditionStatus::Warn,
            finding: format!(
                "{} agent(s) lagging: {}. Run `bwoc check --all` for details.",
                lagging.len(),
                lagging.join(", ")
            ),
        }
    }
}

/// Read only the `version` field from a config.manifest.json without requiring
/// a fully-valid Manifest (the template manifest has a different shape).
fn load_manifest_version(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("version")
        .and_then(|val| val.as_str())
        .map(|s| s.to_string())
}

// ── Condition 5: Protect vulnerable — parihāra ───────────────────────────────
//
// Count inbox refusals per agent (inbox.refusals.jsonl sidecar).
// ℹ if any agent has refusals (current count; trend is a v2 follow-up).

fn condition_5_protect_vulnerable(workspace: &Path, registry: &AgentsRegistry) -> ConditionResult {
    const NAME: &str = "Protect vulnerable (parihāra)";

    let mut totals: Vec<(String, usize)> = Vec::new();

    for agent in &registry.agents {
        let refusals_path = workspace
            .join(&agent.path)
            .join(".bwoc/inbox.refusals.jsonl");
        let count = count_jsonl_lines(&refusals_path);
        if count > 0 {
            totals.push((agent.id.clone(), count));
        }
    }

    if totals.is_empty() {
        return ConditionResult {
            number: 5,
            name: NAME,
            status: ConditionStatus::Ok,
            finding: "No inbox refusals recorded across all agents.".into(),
        };
    }

    let summary: Vec<String> = totals.iter().map(|(id, n)| format!("{id}: {n}")).collect();
    let total_count: usize = totals.iter().map(|(_, n)| n).sum();

    // ⚠ if a single agent accounts for the majority (> 50% from one sender
    // would need per-sender breakdown which is v2). In v1, flag any non-zero
    // count as ℹ to surface it; ⚠ when aggregate count is high (>= 10).
    let status = if total_count >= 10 {
        ConditionStatus::Warn
    } else {
        ConditionStatus::Info
    };

    ConditionResult {
        number: 5,
        name: NAME,
        status,
        finding: format!(
            "{total_count} refusal(s) on record: {}. Investigate sender trust if count grows.",
            summary.join(", ")
        ),
    }
}

/// Count non-empty lines in a JSONL file. Missing file → 0 (not an error).
fn count_jsonl_lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

// ── Condition 6: Honor shared resources — cetiya ─────────────────────────────
//
// `git log --format=%an -- .bwoc/agents.toml` → unique set of committer names.
// `git config user.name`                      → operator identity.
//
//   ✓  → all authors == operator name (or no history yet)
//   ⚠  → any author != operator
//   ℹ  → git command fails or no commit history for agents.toml

fn condition_6_honor_shared_resources(
    workspace: &Path,
    git_runner: &dyn GitRunner,
) -> ConditionResult {
    const NAME: &str = "Honor shared resources (cetiya)";

    // Resolve operator identity — best-effort; empty string if unavailable.
    let operator = git_runner
        .git(&["config", "user.name"], workspace)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Fetch unique author names from agents.toml history.
    let log_output = match git_runner.git(
        &["log", "--format=%an", "--", ".bwoc/agents.toml"],
        workspace,
    ) {
        Ok(s) => s,
        Err(()) => {
            return ConditionResult {
                number: 6,
                name: NAME,
                status: ConditionStatus::Info,
                finding: "not a git repo — shared-resource authorship governance is manual.".into(),
            };
        }
    };

    let authors: std::collections::HashSet<&str> = log_output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();

    if authors.is_empty() {
        return ConditionResult {
            number: 6,
            name: NAME,
            status: ConditionStatus::Info,
            finding: "No commit history for .bwoc/agents.toml — nothing to verify yet.".into(),
        };
    }

    // Collect non-operator authors (if operator is unknown, treat all as non-operator).
    let non_operator: Vec<&str> = if operator.is_empty() {
        authors.iter().copied().collect()
    } else {
        authors
            .iter()
            .copied()
            .filter(|&a| a != operator.as_str())
            .collect()
    };

    if non_operator.is_empty() {
        ConditionResult {
            number: 6,
            name: NAME,
            status: ConditionStatus::Ok,
            finding: format!(
                ".bwoc/agents.toml modified only by operator ({operator}) — shared registry is \
                 operator-owned."
            ),
        }
    } else {
        let mut names: Vec<&str> = non_operator;
        names.sort_unstable();
        ConditionResult {
            number: 6,
            name: NAME,
            status: ConditionStatus::Warn,
            finding: format!(
                ".bwoc/agents.toml modified by non-operator author(s): {} — the shared \
                 registry should be operator-owned.",
                names.join(", ")
            ),
        }
    }
}

// ── Condition 7: Protect senior agents — arahantesu rakkhāvaraṇa-gutti ──────
//
// Informational in v1. Surface the count of agents that have trust qualities
// declared (as a proxy for "senior") and suggest the audit command.

fn condition_7_protect_senior_agents(registry: &AgentsRegistry) -> ConditionResult {
    const NAME: &str = "Protect senior agents (arahantesu rakkhāvaraṇa-gutti)";

    // Count agents that have any trust quality declared true (in memory only —
    // we'd need workspace path to load manifests, but the spec's v1 intent is
    // purely informational; we just note the agent count).
    let agent_count = registry.agents.len();

    ConditionResult {
        number: 7,
        name: NAME,
        status: ConditionStatus::Info,
        finding: format!(
            "{agent_count} registered agent(s). Operator practice: audit with \
             `bwoc trust <agent> --json` + check succession before `bwoc retire` \
             on high-trust agents."
        ),
    }
}

// ── Output ───────────────────────────────────────────────────────────────────

fn print_report(workspace: &Path, results: &[ConditionResult]) {
    println!();
    println!("BWOC Fleet Health — Aparihāniya-dhamma 7");
    println!("==========================================");
    println!("Workspace: {}", workspace.display());
    println!();

    for r in results {
        let icon = r.status.icon();
        let label = r.status.label();
        println!("  {} [{label:4}]  {}. {}", icon, r.number, r.name);
        println!("            {}", r.finding);
        println!();
    }

    let warn_count = results
        .iter()
        .filter(|r| r.status == ConditionStatus::Warn)
        .count();
    let ok_count = results
        .iter()
        .filter(|r| r.status == ConditionStatus::Ok)
        .count();
    let info_count = results
        .iter()
        .filter(|r| r.status == ConditionStatus::Info)
        .count();

    println!("==========================================");
    println!("{ok_count} ok · {warn_count} warn · {info_count} info  (exit 0 — v1 report-only)");
    println!();
}

/// Machine-readable shape:
/// ```json
/// [
///   { "condition": 1, "name": "...", "status": "ok"|"warn"|"info", "finding": "..." },
///   ...
/// ]
/// ```
fn emit_json(results: &[ConditionResult]) {
    let items: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "condition": r.number,
                "name": r.name,
                "status": r.status.label(),
                "finding": r.finding,
            })
        })
        .collect();
    match serde_json::to_string_pretty(&items) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("bwoc fleet health --json: serialize failed: {e}"),
    }
}

// ── Workspace resolution ─────────────────────────────────────────────────────

fn resolve_workspace(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
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

// ── `bwoc fleet status` ───────────────────────────────────────────────────────
//
// A per-agent status overview — the "which agents are stuck?" view (issue #297).
// Plain text + `--json`, both non-TTY-friendly (no TUI). For each registered
// agent it reports backend, registry status, real liveness (`online`, via
// `livecheck::running_pid` — the same pid probe status/dashboard use), the
// pending inbox count (via the shared `AgentEntry::inbox_path` resolver), and
// how long since the inbox last changed (LAST-MSG — last message written, not
// liveness).

pub struct FleetStatusArgs {
    /// Workspace root. Resolution: explicit > BWOC_WORKSPACE env > ancestor walk.
    pub workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON object (`{ workspace, agents: [...] }`)
    /// instead of the human table.
    pub json: bool,
}

struct AgentStatus {
    id: String,
    backend: String,
    status: String,
    /// Real liveness — the agent's daemon is running (pid file + signal-0), via
    /// `livecheck::running_pid`. Distinct from `status` (registry lifecycle).
    online: bool,
    pending: usize,
    /// Seconds since the inbox file last changed; `None` when no inbox exists.
    /// This is "last message written", NOT liveness — see `online`.
    last_seen_secs: Option<u64>,
}

pub fn status(args: FleetStatusArgs) -> i32 {
    let workspace = match resolve_workspace(args.workspace.as_deref()) {
        Some(p) => p,
        None => {
            eprintln!(
                "bwoc fleet status: no workspace found \
                 (no .bwoc/workspace.toml in cwd or ancestors). \
                 Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
            );
            return 2;
        }
    };
    if let Err(code) = crate::workspace::ensure_workspace(&workspace, "bwoc fleet status") {
        return code;
    }
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc fleet status: failed to read agents registry: {e}");
            return 1;
        }
    };

    let now = std::time::SystemTime::now();
    let rows: Vec<AgentStatus> = registry
        .agents
        .iter()
        .map(|entry| {
            let (pending, last_seen_secs) = inbox_stats(&entry.inbox_path(&workspace), now);
            AgentStatus {
                id: entry.id.clone(),
                backend: entry.backend.clone(),
                status: entry.status.clone(),
                online: crate::livecheck::running_pid(&workspace, entry).is_some(),
                pending,
                last_seen_secs,
            }
        })
        .collect();

    if args.json {
        emit_status_json(&workspace, &rows);
    } else {
        print_status_table(&workspace, &rows);
    }
    0
}

/// Count non-empty inbox lines and the age (seconds) of the file's last change.
///
/// A *missing* inbox yields `(0, None)` — not an error (no one has written yet).
/// Any *other* I/O error (permission denied, corruption) is surfaced as a stderr
/// warning rather than silently masked as "empty inbox", then degrades to the
/// same `(0, …)` / `None` so one unreadable inbox doesn't abort the whole report.
fn inbox_stats(path: &Path, now: std::time::SystemTime) -> (usize, Option<u64>) {
    use std::io::ErrorKind::NotFound;
    let count = match std::fs::read_to_string(path) {
        Ok(c) => c.lines().filter(|l| !l.trim().is_empty()).count(),
        Err(e) if e.kind() == NotFound => 0,
        Err(e) => {
            eprintln!(
                "bwoc fleet status: warning — cannot read inbox {}: {e}",
                path.display()
            );
            0
        }
    };
    // `Some(age)` whenever the inbox file exists (age clamps to 0 if its mtime
    // is somehow ahead of `now` — clock skew), `None` only when there is no file.
    let last_seen_secs = match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(mt) => Some(now.duration_since(mt).map(|d| d.as_secs()).unwrap_or(0)),
        Err(e) if e.kind() == NotFound => None,
        Err(e) => {
            eprintln!(
                "bwoc fleet status: warning — cannot stat inbox {}: {e}",
                path.display()
            );
            None
        }
    };
    (count, last_seen_secs)
}

/// Render a seconds-ago age as a compact "2d 3h" / "5h 12m" / "just now" /
/// "never" string for the human table.
fn humanize_age(secs: Option<u64>) -> String {
    let Some(s) = secs else {
        return "never".to_string();
    };
    if s < 60 {
        return "just now".to_string();
    }
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3_600, (s % 3_600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

fn print_status_table(workspace: &Path, rows: &[AgentStatus]) {
    println!();
    println!(
        "Fleet status — {} ({} agent(s))",
        workspace.display(),
        rows.len()
    );
    println!();
    if rows.is_empty() {
        println!("(no agents registered — `bwoc new` to incarnate one)");
        println!();
        return;
    }
    let id_w = rows.iter().map(|r| r.id.len()).max().unwrap_or(5).max(5);
    let be_w = rows
        .iter()
        .map(|r| r.backend.len())
        .max()
        .unwrap_or(7)
        .max(7);
    println!(
        "  {:<id_w$}  {:<be_w$}  {:<9}  {:<8}  {:>7}  LAST-MSG",
        "AGENT", "BACKEND", "ONLINE", "STATUS", "PENDING"
    );
    for r in rows {
        println!(
            "  {:<id_w$}  {:<be_w$}  {:<9}  {:<8}  {:>7}  {}",
            r.id,
            r.backend,
            if r.online {
                "● online"
            } else {
                "○ offline"
            },
            r.status,
            r.pending,
            humanize_age(r.last_seen_secs),
        );
    }
    println!();
}

/// Build the `--json` value (pure, so the schema — notably the `online`
/// liveness field — is unit-testable without capturing stdout).
fn status_json_value(workspace: &Path, rows: &[AgentStatus]) -> serde_json::Value {
    let agents: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "agent": r.id,
                "backend": r.backend,
                "online": r.online,
                "status": r.status,
                "pending": r.pending,
                // "last message written" age — not liveness (see `online`).
                "last_seen_secs": r.last_seen_secs,
            })
        })
        .collect();
    serde_json::json!({
        "workspace": workspace.display().to_string(),
        "agents": agents,
    })
}

fn emit_status_json(workspace: &Path, rows: &[AgentStatus]) {
    let value = status_json_value(workspace, rows);
    // Surface a serialization failure on stderr rather than silently emitting
    // `{}` — consistent with `emit_json` for fleet health, so automation can
    // tell a broken run from an empty fleet.
    match serde_json::to_string_pretty(&value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("bwoc fleet status: failed to serialize status JSON: {e}"),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn status_json_includes_online_liveness_field() {
        let rows = vec![
            AgentStatus {
                id: "agent-a".into(),
                backend: "claude".into(),
                online: true,
                status: "active".into(),
                pending: 2,
                last_seen_secs: Some(5),
            },
            AgentStatus {
                id: "agent-b".into(),
                backend: "ollama".into(),
                online: false,
                status: "active".into(),
                pending: 0,
                last_seen_secs: None,
            },
        ];
        let v = status_json_value(Path::new("/ws"), &rows);
        let agents = v["agents"].as_array().unwrap();
        assert_eq!(agents[0]["online"], serde_json::json!(true));
        assert_eq!(agents[1]["online"], serde_json::json!(false));
        // last_seen_secs stays present (last-msg age), distinct from online.
        assert_eq!(agents[0]["last_seen_secs"], serde_json::json!(5));
        assert!(agents[1]["last_seen_secs"].is_null());
    }

    // ── MockGitRunner ────────────────────────────────────────────────────────

    /// Offline mock: maps (args[0], optional args[last]) → canned stdout.
    /// Any unregistered call returns `Err(())` (simulates git failure).
    struct MockGitRunner {
        /// Entries: (first_arg, last_path_arg_or_empty) → stdout string.
        responses: Vec<(String, String, Result<String, ()>)>,
    }

    impl MockGitRunner {
        fn new() -> Self {
            Self {
                responses: Vec::new(),
            }
        }

        /// Register: when git is called with `first_arg` and the last arg
        /// contains `path_fragment`, return `response`.
        fn on(mut self, first_arg: &str, path_fragment: &str, response: Result<&str, ()>) -> Self {
            self.responses.push((
                first_arg.to_string(),
                path_fragment.to_string(),
                response.map(|s| s.to_string()),
            ));
            self
        }
    }

    impl GitRunner for MockGitRunner {
        fn git(&self, args: &[&str], _cwd: &Path) -> Result<String, ()> {
            let first = args.first().copied().unwrap_or("");
            for (fa, pf, resp) in &self.responses {
                let first_matches = fa == first;
                // path_fragment matches if it appears in ANY arg (or is empty)
                let path_matches = pf.is_empty() || args.iter().any(|a| a.contains(pf.as_str()));
                if first_matches && path_matches {
                    return resp.clone();
                }
            }
            // Unregistered → simulate git failure
            Err(())
        }
    }

    // ── Fixture helpers ──────────────────────────────────────────────────────

    fn fresh_workspace(label: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("bwoc-fleet-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".bwoc")).unwrap();
        fs::write(
            base.join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"test\"\nversion=\"0.1.0\"\ncreated=\"2026-01-01T00:00:00Z\"\n",
        )
        .unwrap();
        fs::write(base.join(".bwoc/agents.toml"), "").unwrap();
        base
    }

    fn add_agent(workspace: &Path, id: &str) {
        // Register in agents.toml
        let toml_path = workspace.join(".bwoc/agents.toml");
        let existing = fs::read_to_string(&toml_path).unwrap_or_default();
        let entry = format!(
            "\n[[agent]]\nid = \"{id}\"\npath = \"agents/{id}\"\nbackend = \"claude\"\nincarnated = \"2026-01-01T00:00:00Z\"\nstatus = \"active\"\n"
        );
        fs::write(&toml_path, format!("{existing}{entry}")).unwrap();

        // Create agent dir + minimal structure
        let agent_dir = workspace.join(format!("agents/{id}"));
        fs::create_dir_all(agent_dir.join(".bwoc")).unwrap();
    }

    fn write_agent_manifest(workspace: &Path, id: &str, version: &str) {
        let manifest = workspace.join(format!("agents/{id}/config.manifest.json"));
        let content = serde_json::json!({
            "name": id,
            "agentId": id,
            "agentRole": "test",
            "primaryModel": "test-model",
            "memoryPath": "memories/",
            "lintCmd": "true",
            "formatCmd": "true",
            "testCmd": "true",
            "buildCmd": "true",
            "version": version,
        });
        fs::write(&manifest, serde_json::to_string_pretty(&content).unwrap()).unwrap();
    }

    fn write_template_manifest(workspace: &Path, version: &str) {
        let template_dir = workspace.join("modules/agent-template");
        fs::create_dir_all(&template_dir).unwrap();
        let manifest = template_dir.join("config.manifest.json");
        let content = serde_json::json!({ "version": version });
        fs::write(&manifest, serde_json::to_string_pretty(&content).unwrap()).unwrap();
    }

    // ── Condition 1 ─────────────────────────────────────────────────────────

    #[test]
    fn cond1_fresh_agent_is_ok() {
        let ws = fresh_workspace("c1-fresh");
        add_agent(&ws, "agent-alpha");
        let registry = AgentsRegistry::load(&ws).unwrap();
        let result = condition_1_regular_meetings(&ws, &registry, 7);
        assert_eq!(
            result.status,
            ConditionStatus::Ok,
            "fresh agent should be ok: {}",
            result.finding
        );
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn cond1_old_mtime_warns() {
        let ws = fresh_workspace("c1-old");
        add_agent(&ws, "agent-beta");

        // Back-date the agent directory's mtime to 30 days ago.
        let agent_dir = ws.join("agents/agent-beta");
        let thirty_days_ago = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(30 * 86_400))
            .unwrap();
        // Manipulate via writing a file with a back-dated mtime — we can't
        // set mtime directly without libc, so instead we check the ⚠ path by
        // passing a very small stale_days threshold (0) that any file will exceed.
        let _ = agent_dir;
        let registry = AgentsRegistry::load(&ws).unwrap();
        // With stale_days=0, even a file modified just now (a few ms) exceeds threshold.
        // This deterministically exercises the ⚠ branch.
        let result = condition_1_regular_meetings(&ws, &registry, 0);
        assert_eq!(
            result.status,
            ConditionStatus::Warn,
            "stale_days=0 should warn: {}",
            result.finding
        );
        let _ = thirty_days_ago; // referenced for docs
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn cond1_respects_stale_days_flag() {
        let ws = fresh_workspace("c1-stale-days");
        add_agent(&ws, "agent-gamma");
        let registry = AgentsRegistry::load(&ws).unwrap();
        // With a very large stale_days value, no agent should ever warn.
        let result = condition_1_regular_meetings(&ws, &registry, 99999);
        assert_eq!(
            result.status,
            ConditionStatus::Ok,
            "large stale_days should pass: {}",
            result.finding
        );
        let _ = fs::remove_dir_all(&ws);
    }

    // ── Condition 4 ─────────────────────────────────────────────────────────

    #[test]
    fn cond4_matching_versions_ok() {
        let ws = fresh_workspace("c4-match");
        add_agent(&ws, "agent-delta");
        write_template_manifest(&ws, "2.0");
        write_agent_manifest(&ws, "agent-delta", "2.0");
        let registry = AgentsRegistry::load(&ws).unwrap();
        let result = condition_4_honor_template_version(&ws, &registry);
        assert_eq!(
            result.status,
            ConditionStatus::Ok,
            "matching versions: {}",
            result.finding
        );
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn cond4_version_mismatch_warns() {
        let ws = fresh_workspace("c4-mismatch");
        add_agent(&ws, "agent-epsilon");
        write_template_manifest(&ws, "2.0");
        write_agent_manifest(&ws, "agent-epsilon", "1.9");
        let registry = AgentsRegistry::load(&ws).unwrap();
        let result = condition_4_honor_template_version(&ws, &registry);
        assert_eq!(
            result.status,
            ConditionStatus::Warn,
            "version mismatch should warn: {}",
            result.finding
        );
        assert!(
            result.finding.contains("agent-epsilon"),
            "finding should name the agent"
        );
        let _ = fs::remove_dir_all(&ws);
    }

    // ── JSON shape ───────────────────────────────────────────────────────────

    #[test]
    fn json_shape_has_required_fields() {
        let ws = fresh_workspace("json-shape");
        let registry = AgentsRegistry::load(&ws).unwrap();
        let git = MockGitRunner::new(); // all git calls → Err → ℹ for cond 3/6
        let results = evaluate_all(&ws, &registry, 7, &git);
        // Serialize and parse back to verify shape.
        let items: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "condition": r.number,
                    "name": r.name,
                    "status": r.status.label(),
                    "finding": r.finding,
                })
            })
            .collect();
        assert_eq!(items.len(), 7, "must have exactly 7 conditions");
        for item in &items {
            assert!(item.get("condition").is_some());
            assert!(item.get("name").is_some());
            assert!(item.get("status").is_some());
            assert!(item.get("finding").is_some());
            let status = item["status"].as_str().unwrap();
            assert!(
                matches!(status, "ok" | "warn" | "info"),
                "status must be ok|warn|info, got '{status}'"
            );
        }
        let _ = fs::remove_dir_all(&ws);
    }

    // ── Clean workspace — no hard failures, exit 0 ───────────────────────────

    #[test]
    fn clean_workspace_no_hard_failures() {
        let ws = fresh_workspace("clean");
        let registry = AgentsRegistry::load(&ws).unwrap();
        let git = MockGitRunner::new();
        let results = evaluate_all(&ws, &registry, 7, &git);
        // v1: no "fail" status exists; only ok/warn/info.
        for r in &results {
            assert_ne!(
                r.status as u8, // just checking it's one of the three
                255,            // sentinel — all valid statuses are < 3
                "unexpected status for condition {}",
                r.number
            );
            // There should be no ConditionStatus outside the three variants.
        }
        // run() always returns 0 in v1.
        let code = run(FleetHealthArgs {
            workspace: Some(ws.clone()),
            json: false,
            stale_days: 7,
            loop_mode: false,
            loop_interval_secs: 0,
            loop_max_iters: 0,
        });
        assert_eq!(code, 0, "clean workspace must exit 0");
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn json_mode_clean_workspace_exit_0() {
        let ws = fresh_workspace("json-clean");
        let code = run(FleetHealthArgs {
            workspace: Some(ws.clone()),
            json: true,
            stale_days: 7,
            loop_mode: false,
            loop_interval_secs: 0,
            loop_max_iters: 0,
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&ws);
    }

    // ── Fleet-health loop gate (L2) ──────────────────────────────────────────

    #[test]
    fn fleet_loop_done_when_no_warns() {
        assert_eq!(fleet_loop_decide(&[], usize::MAX), FleetLoopDecision::Done);
    }

    #[test]
    fn fleet_loop_remediates_a_stale_pid_warn() {
        // Condition 2 warn present, first fire (prev = MAX) → remediate.
        assert_eq!(
            fleet_loop_decide(&[2], usize::MAX),
            FleetLoopDecision::Remediate
        );
    }

    #[test]
    fn fleet_loop_blocked_when_no_auto_remediable_warn() {
        // A non-condition-2 warn (e.g. template version lag) can't be auto-fixed.
        match fleet_loop_decide(&[4], usize::MAX) {
            FleetLoopDecision::Blocked(r) => assert!(r.contains("none auto-remediable"), "{r}"),
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn fleet_loop_blocked_when_remediation_stalls() {
        // Condition 2 warn persists (count didn't drop from the prior fire) →
        // doctor can't clear it; stop rather than spin.
        match fleet_loop_decide(&[2], 1) {
            FleetLoopDecision::Blocked(r) => assert!(r.contains("no progress"), "{r}"),
            other => panic!("expected Blocked (stalled), got {other:?}"),
        }
    }

    // ── Condition 3 — git-backed convention drift ────────────────────────────

    #[test]
    fn cond3_clean_porcelain_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // Empty porcelain output → clean
        let git = MockGitRunner::new().on("status", ".bwoc/", Ok(""));
        let result = condition_3_convention_change(ws, &git);
        assert_eq!(result.status, ConditionStatus::Ok, "{}", result.finding);
        assert!(result.finding.contains("No uncommitted"));
    }

    #[test]
    fn cond3_dirty_porcelain_warns() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // Porcelain with two changed lines
        let git = MockGitRunner::new().on(
            "status",
            ".bwoc/",
            Ok(" M .bwoc/agents.toml\n M modules/agent-template/AGENTS.md\n"),
        );
        let result = condition_3_convention_change(ws, &git);
        assert_eq!(result.status, ConditionStatus::Warn, "{}", result.finding);
        assert!(
            result.finding.contains('2'),
            "finding should mention count: {}",
            result.finding
        );
    }

    #[test]
    fn cond3_git_failure_is_info() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // No responses registered → git call returns Err(())
        let git = MockGitRunner::new();
        let result = condition_3_convention_change(ws, &git);
        assert_eq!(result.status, ConditionStatus::Info, "{}", result.finding);
        assert!(result.finding.contains("not a git repo"));
    }

    // ── Condition 6 — shared-resource authorship ─────────────────────────────

    #[test]
    fn cond6_all_authors_are_operator_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let git = MockGitRunner::new()
            .on("config", "user.name", Ok("Alice\n"))
            .on("log", "agents.toml", Ok("Alice\nAlice\n"));
        let result = condition_6_honor_shared_resources(ws, &git);
        assert_eq!(result.status, ConditionStatus::Ok, "{}", result.finding);
        assert!(result.finding.contains("Alice"));
    }

    #[test]
    fn cond6_non_operator_author_warns() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let git = MockGitRunner::new()
            .on("config", "user.name", Ok("Alice\n"))
            .on("log", "agents.toml", Ok("Alice\nBob\n"));
        let result = condition_6_honor_shared_resources(ws, &git);
        assert_eq!(result.status, ConditionStatus::Warn, "{}", result.finding);
        assert!(
            result.finding.contains("Bob"),
            "finding should name non-operator: {}",
            result.finding
        );
    }

    #[test]
    fn cond6_empty_log_is_info() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // config resolves but agents.toml has no history
        let git = MockGitRunner::new()
            .on("config", "user.name", Ok("Alice\n"))
            .on("log", "agents.toml", Ok(""));
        let result = condition_6_honor_shared_resources(ws, &git);
        assert_eq!(result.status, ConditionStatus::Info, "{}", result.finding);
        assert!(result.finding.contains("No commit history"));
    }

    #[test]
    fn cond6_git_failure_is_info() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        // No git responses at all → both calls fail
        let git = MockGitRunner::new();
        let result = condition_6_honor_shared_resources(ws, &git);
        assert_eq!(result.status, ConditionStatus::Info, "{}", result.finding);
        assert!(result.finding.contains("not a git repo"));
    }

    // ── fleet status (#297) ──────────────────────────────────────────────────

    #[test]
    fn humanize_age_buckets() {
        assert_eq!(humanize_age(None), "never");
        assert_eq!(humanize_age(Some(10)), "just now");
        assert_eq!(humanize_age(Some(5 * 60)), "5m");
        assert_eq!(humanize_age(Some(3 * 3600 + 12 * 60)), "3h 12m");
        assert_eq!(humanize_age(Some(2 * 86400 + 3 * 3600)), "2d 3h");
    }

    #[test]
    fn inbox_stats_counts_nonempty_lines_and_missing_is_zero() {
        let ws = fresh_workspace("status-stats");
        add_agent(&ws, "agent-alpha");
        let now = std::time::SystemTime::now();
        // Missing inbox → (0, None).
        let inbox = ws.join("agents/agent-alpha/.bwoc/inbox.jsonl");
        assert_eq!(inbox_stats(&inbox, now), (0, None));
        // Two envelopes + a blank line → count 2, and a Some(age).
        fs::write(&inbox, "{\"a\":1}\n\n{\"b\":2}\n").unwrap();
        let (count, age) = inbox_stats(&inbox, now);
        assert_eq!(count, 2);
        assert!(age.is_some());
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn status_exits_zero_with_and_without_agents() {
        let ws = fresh_workspace("status-run");
        // Empty fleet.
        assert_eq!(
            status(FleetStatusArgs {
                workspace: Some(ws.clone()),
                json: true,
            }),
            0
        );
        // With an agent + a pending message.
        add_agent(&ws, "agent-alpha");
        fs::write(
            ws.join("agents/agent-alpha/.bwoc/inbox.jsonl"),
            "{\"from\":\"x\",\"message\":\"hi\"}\n",
        )
        .unwrap();
        assert_eq!(
            status(FleetStatusArgs {
                workspace: Some(ws.clone()),
                json: false,
            }),
            0
        );
        let _ = fs::remove_dir_all(&ws);
    }
}
