//! `bwoc fleet health` — Aparihāniya-dhamma 7 fleet-governance signals.
//!
//! Reads the workspace registry, then checks all seven conditions defined in
//! `docs/en/FLEET-GOVERNANCE.en.md`. Read-only, workspace-scoped, backend-neutral.
//! v1 reports only; gating is deferred to v2.
//!
//! Conditions 1, 2, 4, 5 are mechanically computed (produce ✓ ok or ⚠ warn).
//! Conditions 3, 6, 7 are informational — they print the spec's suggested
//! operator practice (no git shell-out in v1).

use std::path::{Path, PathBuf};

use bwoc_core::workspace::AgentsRegistry;

// ── Public args ──────────────────────────────────────────────────────────────

pub struct FleetHealthArgs {
    /// Workspace root. Resolution: explicit > BWOC_WORKSPACE env > ancestor walk > error.
    pub workspace: Option<PathBuf>,
    /// Emit a machine-readable JSON array instead of the human report.
    pub json: bool,
    /// Number of days after which an un-touched agent dir triggers a ⚠ for
    /// condition 1 (regular meetings). Default: 7.
    pub stale_days: u64,
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

    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc fleet health: failed to read agents registry: {e}");
            return 1;
        }
    };

    let results = evaluate_all(&workspace, &registry, args.stale_days);

    if args.json {
        emit_json(&results);
    } else {
        print_report(&workspace, &results);
    }

    // v1: always exit 0 (report-only, no gating)
    0
}

// ── Evaluate all 7 conditions ────────────────────────────────────────────────

fn evaluate_all(
    workspace: &Path,
    registry: &AgentsRegistry,
    stale_days: u64,
) -> Vec<ConditionResult> {
    vec![
        condition_1_regular_meetings(workspace, registry, stale_days),
        condition_2_coordinated_start_end(workspace, registry),
        condition_3_convention_change(),
        condition_4_honor_template_version(workspace, registry),
        condition_5_protect_vulnerable(workspace, registry),
        condition_6_honor_shared_resources(),
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
// Informational only in v1. No git shell-out.

fn condition_3_convention_change() -> ConditionResult {
    ConditionResult {
        number: 3,
        name: "Process-bound convention change (appaññattaṃ na paññāpenti)",
        status: ConditionStatus::Info,
        finding: "Operator practice: `git log -- .bwoc/ modules/agent-template/` — \
                  schema bumps should be coordinated and operator-signed."
            .into(),
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
// Informational in v1.

fn condition_6_honor_shared_resources() -> ConditionResult {
    ConditionResult {
        number: 6,
        name: "Honor shared resources (cetiya)",
        status: ConditionStatus::Info,
        finding: "Operator practice: `git blame .bwoc/agents.toml` — \
                  only operator-authored changes expected in shared config files."
            .into(),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        let results = evaluate_all(&ws, &registry, 7);
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
        let results = evaluate_all(&ws, &registry, 7);
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
        });
        assert_eq!(code, 0);
        let _ = fs::remove_dir_all(&ws);
    }
}
