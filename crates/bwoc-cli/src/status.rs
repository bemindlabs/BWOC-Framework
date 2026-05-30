//! `bwoc status [name]` — per-agent health + identity snapshot.
//!
//! Phase 2 ṭhiti starter. Without a name, prints a per-agent table
//! (id · role · backend · model · health). With a name, prints full
//! detail for that one agent — manifest fields, dir contents, health
//! probes, dates.
//!
//! Read-only — no process supervision (that needs Phase 2 control
//! socket). Health probes mirror `bwoc doctor`'s per-agent checks:
//! does the dir exist? AGENTS.md present? all 4 backend symlinks?
//!
//! Workspace resolution: explicit > BWOC_WORKSPACE > ancestor walk
//! > exit 2 (same chain as list / info / validate).

use std::path::{Path, PathBuf};

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::{AgentEntry, AgentsRegistry};

pub struct StatusArgs {
    pub name: Option<String>,
    pub workspace: Option<PathBuf>,
    pub json: bool,
    /// Print the full detail block for every agent (loops `print_one`).
    /// Without this, no-name + non-JSON gives the compact table view;
    /// with `--all`, you get the same output as iterating `status <each>`.
    /// Mutually exclusive with `name` (clap-enforced) and `--banner`.
    pub all: bool,
    /// Replay the agent's startup liveness banner from manifest data.
    /// Requires `name`. Mutually exclusive with `--all` (clap-enforced).
    pub banner: bool,
    /// Resolved language tag (from `--lang` / BWOC_LANG / $LANG / "en").
    /// Populated by main before calling `run()`.
    pub lang: String,
}

const BACKEND_SYMLINKS: &[&str] = &["CLAUDE.md", "AGY.md", "CODEX.md", "KIMI.md", "OLLAMA.md"];

/// Build the liveness banner string from a manifest + locale bundle.
/// Mirrors `bwoc-agent::liveness_banner` exactly — same keys, same field order.
fn banner_string(m: &Manifest, lang: &str) -> String {
    let bundle = crate::i18n::bundle_for(lang);
    let mut lines = Vec::with_capacity(6);
    lines.push(crate::i18n::t_with(
        &bundle,
        "status-banner-alive",
        &[("agent_id", m.agent_id.as_str())],
    ));
    lines.push(crate::i18n::t_with(
        &bundle,
        "status-banner-role",
        &[("role", m.agent_role.as_str())],
    ));
    lines.push(crate::i18n::t_with(
        &bundle,
        "status-banner-model",
        &[("model", m.primary_model.as_str())],
    ));
    if let Some(ref fb) = m.fallback_model {
        lines.push(crate::i18n::t_with(
            &bundle,
            "status-banner-fallback",
            &[("fallback", fb.as_str())],
        ));
    }
    lines.push(crate::i18n::t_with(
        &bundle,
        "status-banner-memory",
        &[("memory_path", m.memory_path.as_str())],
    ));
    lines.push(crate::i18n::t_with(
        &bundle,
        "status-banner-version",
        &[("version", m.version.as_str())],
    ));
    lines.join("\n")
}

pub fn run(args: StatusArgs) -> i32 {
    let Some(root) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc status: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };

    let registry = match AgentsRegistry::load(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc status: failed to read agents.toml: {e}");
            return 1;
        }
    };

    // --banner branch — manifest-driven liveness replay, no daemon needed.
    // clap enforces: requires name, conflicts_with all.
    if args.banner {
        let name = args.name.as_deref().unwrap_or(""); // clap `requires` guarantees Some
        let lookup_id = if name.starts_with("agent-") {
            name.to_string()
        } else {
            format!("agent-{name}")
        };
        let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
            eprintln!(
                "bwoc status: no agent named '{name}' in workspace {}. Try `bwoc list`.",
                root.display()
            );
            return 2;
        };
        let manifest_path = root.join(&entry.path).join("config.manifest.json");
        let manifest = match Manifest::load_from_path(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("bwoc status --banner: failed to load manifest: {e}");
                return 1;
            }
        };
        let banner = banner_string(&manifest, &args.lang);
        if args.json {
            match serde_json::to_string_pretty(&serde_json::json!({ "banner": banner })) {
                Ok(s) => println!("{s}"),
                Err(e) => {
                    eprintln!("bwoc status --banner --json: serialize error: {e}");
                    return 1;
                }
            }
        } else {
            println!("{banner}");
        }
        return 0;
    }

    // JSON branch — single shape for both "all" and "one" cases.
    if args.json {
        return emit_json(&root, &registry, args.name.as_deref());
    }

    if args.all {
        // Full detail block per agent. Returns 2 if any fails the
        // health probe (matches `print_one`'s exit semantics).
        let mut worst = 0;
        for entry in &registry.agents {
            let code = print_one(&root, &registry, &entry.id);
            if code > worst {
                worst = code;
            }
        }
        return worst;
    }

    match args.name {
        Some(name) => print_one(&root, &registry, &name),
        None => print_all(&root, &registry),
    }
}

fn emit_json(root: &Path, registry: &AgentsRegistry, name: Option<&str>) -> i32 {
    let agents: Vec<serde_json::Value> = registry
        .agents
        .iter()
        .filter(|a| {
            name.is_none_or(|n| {
                let lookup = if n.starts_with("agent-") {
                    n.to_string()
                } else {
                    format!("agent-{n}")
                };
                a.id == lookup
            })
        })
        .map(|a| {
            let health = probe(root, a);
            let (health_str, health_detail) = match &health {
                Health::Ok => ("ok", None),
                Health::Warn(m) => ("warn", Some(m.as_str())),
                Health::Fail(m) => ("fail", Some(m.as_str())),
            };
            let primary_model = read_primary_model(root, a);
            // Surface persona scope + resource counts (mirror of the
            // human-readable detail). Lets JSON consumers branch on
            // whether `--scope` was set without re-parsing the manifest.
            let agent_path = root.join(&a.path);
            let manifest = Manifest::load_from_path(&agent_path.join("config.manifest.json")).ok();
            let running = crate::livecheck::running_pid(root, a).is_some();
            let uptime_seconds = if running {
                crate::livecheck::query_uptime(root, a)
            } else {
                None
            };
            let scope = manifest.as_ref().and_then(|m| m.scope_description.clone());
            let out_of_scope = manifest.as_ref().and_then(|m| m.out_of_scope.clone());
            serde_json::json!({
                "id": a.id,
                "path": a.path,
                "backend": a.backend,
                "status": a.status,
                "incarnated": a.incarnated,
                "primary_model": primary_model,
                "scope": scope,
                "out_of_scope": out_of_scope,
                "running": running,
                "uptime_seconds": uptime_seconds,
                "health": health_str,
                "health_detail": health_detail,
                "resources": {
                    "mindsets": crate::livecheck::count_user_md_files(&agent_path.join("mindsets")),
                    "skills": crate::livecheck::count_user_md_files(&agent_path.join("skills")),
                    "memories": crate::livecheck::count_user_md_files(&agent_path.join("memories")),
                },
            })
        })
        .collect();

    if name.is_some() && agents.is_empty() {
        eprintln!(
            "bwoc status: no agent named '{}' in workspace {}",
            name.unwrap_or(""),
            root.display()
        );
        return 2;
    }

    let value = serde_json::json!({
        "workspace": root.display().to_string(),
        "agents": agents,
    });
    match serde_json::to_string_pretty(&value) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc status: failed to serialize JSON: {e}");
            1
        }
    }
}

fn print_all(root: &Path, registry: &AgentsRegistry) -> i32 {
    println!();
    println!("Workspace: {}", root.display());
    println!();
    if registry.agents.is_empty() {
        println!("(no agents registered — `bwoc new <name>` to incarnate the first)");
        println!();
        return 0;
    }

    println!(
        "{:<24} {:<8} {:<10} {:<9} {:<24} PATH",
        "ID", "HEALTH", "BACKEND", "UPTIME", "MODEL"
    );
    println!(
        "{:<24} {:<8} {:<10} {:<9} {:<24} {}",
        "─".repeat(24),
        "─".repeat(8),
        "─".repeat(10),
        "─".repeat(9),
        "─".repeat(24),
        "─".repeat(20),
    );
    let mut unhealthy = 0u32;
    for a in &registry.agents {
        let health = probe(root, a);
        let model = read_primary_model(root, a).unwrap_or_else(|| "—".to_string());
        let mark = match health {
            Health::Ok => "✓",
            Health::Warn(_) => "⚠",
            Health::Fail(_) => "✗",
        };
        if !matches!(health, Health::Ok) {
            unhealthy += 1;
        }
        // Daemon liveness + uptime — same data path as `bwoc list` and
        // the dashboard, keeping the 3 surfaces aligned.
        let (live_mark, uptime) = match crate::livecheck::running_pid(root, a) {
            Some(_) => match crate::livecheck::query_uptime(root, a) {
                Some(secs) => ("●", crate::livecheck::format_uptime(secs)),
                None => ("●", "?".to_string()),
            },
            None => ("○", "—".to_string()),
        };
        println!(
            "{live_mark} {:<22} {:<8} {:<10} {:<9} {:<24} {}",
            a.id, mark, a.backend, uptime, model, a.path
        );
    }
    println!();
    if unhealthy > 0 {
        println!(
            "{unhealthy} agent(s) need attention. Run `bwoc status <name>` for details, or `bwoc doctor` to scan + auto-fix safe issues."
        );
    } else {
        println!("All agents healthy. Run `bwoc status <name>` for per-agent detail.");
    }
    println!();
    0
}

fn print_one(root: &Path, registry: &AgentsRegistry, name: &str) -> i32 {
    let lookup_id = if name.starts_with("agent-") {
        name.to_string()
    } else {
        format!("agent-{name}")
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc status: no agent named '{name}' in workspace {}. Try `bwoc list` to see what's registered.",
            root.display()
        );
        return 2;
    };

    let agent_path = root.join(&entry.path);
    let health = probe(root, entry);

    let runtime = match crate::livecheck::running_pid(root, entry) {
        Some(pid) => match crate::livecheck::query_uptime(root, entry) {
            Some(secs) => format!(
                "● running (pid {pid}, uptime {})",
                crate::livecheck::format_uptime(secs)
            ),
            None => format!("● running (pid {pid})"),
        },
        None => "○ not running".to_string(),
    };

    println!();
    println!("Agent: {}", entry.id);
    println!("={}", "=".repeat(entry.id.len() + 5));
    println!("  path:        {}", entry.path);
    println!("  backend:     {}", entry.backend);
    println!("  status:      {}", entry.status);
    println!("  runtime:     {runtime}");
    println!("  incarnated:  {}", entry.incarnated);
    println!();
    match Manifest::load_from_path(&agent_path.join("config.manifest.json")) {
        Ok(m) => {
            println!("  Manifest:");
            println!("    role:           {}", m.agent_role);
            println!("    primaryModel:   {}", m.primary_model);
            if let Some(fb) = &m.fallback_model {
                println!("    fallbackModel:  {fb}");
            }
            println!("    memoryPath:     {}", m.memory_path);
            println!("    lintCmd:        {}", m.lint_cmd);
            println!("    formatCmd:      {}", m.format_cmd);
            println!("    testCmd:        {}", m.test_cmd);
            println!("    buildCmd:       {}", m.build_cmd);
            println!("    version:        {}", m.version);
            // Persona scope (when user supplied `--scope`/`--out-of-scope`
            // at `bwoc new` time; otherwise omitted — the {{placeholder}}
            // fallback isn't useful here).
            if let Some(scope) = &m.scope_description {
                println!("    scope:          {scope}");
            }
            if let Some(out) = &m.out_of_scope {
                println!("    outOfScope:     {out}");
            }
        }
        Err(e) => println!("  Manifest:    (failed to read: {e})"),
    }

    // Resource counts — mindsets / skills / memories. Mirror of the
    // dashboard detail pane. Only printed when at least one is non-zero
    // (avoid noise on fresh-from-template agents).
    let resources = [
        (
            "mindsets",
            crate::livecheck::count_user_md_files(&agent_path.join("mindsets")),
        ),
        (
            "skills",
            crate::livecheck::count_user_md_files(&agent_path.join("skills")),
        ),
        (
            "memories",
            crate::livecheck::count_user_md_files(&agent_path.join("memories")),
        ),
    ];
    if resources.iter().any(|(_, n)| *n > 0) {
        println!();
        println!("  Resources:");
        for (label, n) in &resources {
            println!("    {label:<14}  {n}");
        }
    }

    println!();
    println!("  Health:");
    match &health {
        Health::Ok => println!("    ✓ all probes passed"),
        Health::Warn(msg) => println!("    ⚠ {msg}"),
        Health::Fail(msg) => println!("    ✗ {msg}"),
    }
    println!();
    if matches!(health, Health::Fail(_)) {
        return 2;
    }
    0
}

// --- helpers ---------------------------------------------------------------

#[derive(Debug)]
enum Health {
    Ok,
    Warn(String),
    Fail(String),
}

/// Per-agent health probe mirroring doctor's per-agent checks but
/// returning a single summarised verdict.
fn probe(root: &Path, a: &AgentEntry) -> Health {
    let p = root.join(&a.path);
    if !p.is_dir() {
        return Health::Fail(format!("directory missing: {}", p.display()));
    }
    if !p.join("AGENTS.md").is_file() {
        return Health::Fail(format!("missing AGENTS.md in {}", p.display()));
    }
    let missing: Vec<&str> = BACKEND_SYMLINKS
        .iter()
        .copied()
        .filter(|link| !p.join(link).exists())
        .collect();
    if !missing.is_empty() {
        return Health::Warn(format!(
            "missing backend symlinks: {} (rerun `bwoc doctor --auto`)",
            missing.join(", ")
        ));
    }
    if !p.join("config.manifest.json").is_file() {
        return Health::Warn("config.manifest.json missing".to_string());
    }
    Health::Ok
}

// Liveness helpers (signal_zero_alive, running_pid, query_uptime,
// format_uptime) moved to `crate::livecheck` — used by 5+ modules now.

fn read_primary_model(root: &Path, a: &AgentEntry) -> Option<String> {
    let manifest = root.join(&a.path).join("config.manifest.json");
    Manifest::load_from_path(&manifest)
        .ok()
        .map(|m| m.primary_model)
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

#[cfg(test)]
mod tests {
    use super::*;
    use bwoc_core::workspace::AgentEntry;
    use std::fs;

    fn sample_manifest() -> Manifest {
        Manifest {
            name: "demo".into(),
            agent_id: "agent-demo".into(),
            agent_role: "demo role".into(),
            primary_model: "model-x".into(),
            fallback_model: Some("model-y".into()),
            auto_models: None,
            reasoning_effort: None,
            memory_path: "memories/".into(),
            sessions_path: None,
            deep_memory_cmd: None,
            lint_cmd: "true".into(),
            format_cmd: "true".into(),
            test_cmd: "true".into(),
            build_cmd: "true".into(),
            worktree_base: None,
            scope_description: None,
            out_of_scope: None,
            backend: None,
            base_url: None,
            trust: None,
            version: "2.0".into(),
        }
    }

    #[test]
    fn banner_string_en_contains_required_fields() {
        let b = banner_string(&sample_manifest(), "en");
        assert!(b.contains("I am alive: agent-demo"), "got: {b:?}");
        assert!(b.contains("demo role"), "got: {b:?}");
        assert!(b.contains("model-x"), "got: {b:?}");
        assert!(b.contains("model-y"), "got: {b:?}");
        assert!(b.contains("memories/"), "got: {b:?}");
        assert!(b.contains("2.0"), "got: {b:?}");
    }

    #[test]
    fn banner_string_th_alive_line() {
        let b = banner_string(&sample_manifest(), "th");
        assert!(b.contains("ฉันยังมีชีวิตอยู่: agent-demo"), "got: {b:?}");
    }

    #[test]
    fn banner_string_omits_fallback_when_none() {
        let mut m = sample_manifest();
        m.fallback_model = None;
        let b = banner_string(&m, "en");
        assert!(!b.contains("fallback:"), "got: {b:?}");
        assert!(b.contains("I am alive:"), "got: {b:?}");
    }

    #[test]
    fn probe_ok_for_complete_agent() {
        let root = std::env::temp_dir().join(format!("bwoc-status-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let agent = root.join("agents/agent-alpha");
        fs::create_dir_all(&agent).unwrap();
        fs::write(agent.join("AGENTS.md"), "stub").unwrap();
        for link in BACKEND_SYMLINKS {
            fs::write(agent.join(link), "stub").unwrap();
        }
        fs::write(agent.join("config.manifest.json"), "{}").unwrap();
        let entry = AgentEntry {
            id: "agent-alpha".into(),
            path: "agents/agent-alpha".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22".into(),
            status: "active".into(),
        };
        assert!(matches!(probe(&root, &entry), Health::Ok));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn probe_fail_for_missing_dir() {
        let root = std::env::temp_dir().join(format!("bwoc-status-miss-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let entry = AgentEntry {
            id: "agent-alpha".into(),
            path: "agents/agent-alpha".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22".into(),
            status: "active".into(),
        };
        assert!(matches!(probe(&root, &entry), Health::Fail(_)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn probe_warn_for_missing_symlinks() {
        let root = std::env::temp_dir().join(format!("bwoc-status-warn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let agent = root.join("agents/agent-alpha");
        fs::create_dir_all(&agent).unwrap();
        fs::write(agent.join("AGENTS.md"), "stub").unwrap();
        // Note: NO symlinks created
        let entry = AgentEntry {
            id: "agent-alpha".into(),
            path: "agents/agent-alpha".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22".into(),
            status: "active".into(),
        };
        assert!(matches!(probe(&root, &entry), Health::Warn(_)));
        let _ = fs::remove_dir_all(&root);
    }
}
