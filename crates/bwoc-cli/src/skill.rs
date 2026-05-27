//! `bwoc skill list / show / verify / init / install / enable / disable / remove`
//! — full framework-skill surface.
//!
//! Read-side (BWOC-4): `list`, `show`, `verify` follow `docs/en/SKILLS.en.md`
//! §"CLI Surface". Skills live under `<workspace>/modules/skills/<name>/`,
//! each with a `manifest.toml` (schema §"Manifest"). Discovery (§"Discovery")
//! is workspace-local — no network calls — and per-agent opt-in is gated on
//! the agent's `config.manifest.json` `skills.framework[]` array.
//!
//! Write-side (BWOC-23): `init` scaffolds from `modules/skill-template/`;
//! `install` materializes from local path / git URL / tarball URL with a
//! SHA-256 trust gate (`--no-verify` / `--allow-new-source` flags per
//! §"Sources & Installation"); `enable`/`disable` flip the `enabled` field
//! in the current agent's manifest (BWOC-20: `enabled` is required); `remove`
//! deletes `modules/skills/<name>/` and cleans `skills.framework[]` entries
//! in every consuming agent's manifest (§"Removal" line 312).
//!
//! Every read AND write command has a `--json` twin. Human output is
//! intentionally terse — JSON is the contract for scripts.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Top-level arguments shared by every `bwoc skill …` subcommand.
#[derive(Debug, Clone)]
pub struct CommonArgs {
    /// Workspace root override. Resolution: `--workspace` > `BWOC_WORKSPACE`
    /// env > ancestor-walk for `.bwoc/workspace.toml` > error.
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ListArgs {
    pub common: CommonArgs,
    /// Filter to skills `enabled = true` for the current agent.
    pub enabled: bool,
    /// Override current-agent resolution. Used with `--enabled`.
    pub agent: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct ShowArgs {
    pub common: CommonArgs,
    pub name: String,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct VerifyArgs {
    pub common: CommonArgs,
    pub name: Option<String>,
    pub all: bool,
    pub json: bool,
    /// Execute each `[gates].verify` command via `sh -c`. Off by default:
    /// gate commands come from the skill manifest, which is UNTRUSTED input,
    /// so without this flag verify performs static checks only and prints the
    /// commands it would run instead of executing them.
    pub run_gates: bool,
}

// ---------------------------------------------------------------------------
// Manifest schema (mirror of SKILLS.en.md §"Manifest", lines 45–74).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ManifestRaw {
    skill: SkillSection,
    #[serde(default)]
    contract: ContractSection,
    #[serde(default)]
    gates: GatesSection,
}

#[derive(Debug, Clone, Deserialize)]
struct SkillSection {
    name: String,
    version: String,
    description: String,
    maturity: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ContractSection {
    #[serde(default)]
    requires: Vec<String>,
    /// Plugin KINDS this skill needs enabled in the workspace (BWOC-44/45).
    /// Distinct from `requires` (skill names): resolved against the plugin-kind
    /// enum, not skill names. See SKILLS.en.md §"Skill-on-plugin dependency".
    #[serde(default)]
    requires_plugins: Vec<String>,
    #[serde(default)]
    exposes: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct GatesSection {
    #[serde(default)]
    verify: Option<String>,
}

/// One discovered skill — manifest contents + filesystem location.
#[derive(Debug, Clone)]
struct DiscoveredSkill {
    dir_name: String,
    path: PathBuf,
    manifest: ManifestRaw,
    spec_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Workspace resolution (mirror of the pattern in workspace.rs:634).
// ---------------------------------------------------------------------------

fn find_workspace_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        let p = PathBuf::from(env_path);
        if !p.as_os_str().is_empty() {
            return Some(p);
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

fn resolve_workspace(common: &CommonArgs) -> Result<PathBuf, String> {
    find_workspace_root(common.workspace.clone()).ok_or_else(|| {
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
            .to_string()
    })
}

// ---------------------------------------------------------------------------
// Discovery.
// ---------------------------------------------------------------------------

fn skills_dir(root: &Path) -> PathBuf {
    root.join("modules/skills")
}

/// Walk `<root>/modules/skills/*/manifest.toml`, parse each, and return them
/// sorted by directory name. Missing or invalid manifests are surfaced as
/// errors keyed by directory — callers decide whether to fail or skip.
fn discover(root: &Path) -> Result<Vec<DiscoveredSkill>, String> {
    let dir = skills_dir(root);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(|r| r.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let skill_dir = entry.path();
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = skill_dir.join("manifest.toml");
        if !manifest_path.is_file() {
            // Skip dirs that lack a manifest — they may be the README.md
            // top-level file or future scaffolding. `bwoc check` is the
            // authoritative validator (SKILLS.en.md §"Verification").
            continue;
        }
        let manifest = parse_manifest(&manifest_path)
            .map_err(|e| format!("{}/manifest.toml: {e}", dir_name))?;
        if manifest.skill.name != dir_name {
            return Err(format!(
                "modules/skills/{dir_name}/manifest.toml: [skill].name = {:?} \
                 does not match directory name {dir_name:?}",
                manifest.skill.name
            ));
        }
        let spec = skill_dir.join("SPEC.md");
        let spec_path = if spec.is_file() { Some(spec) } else { None };
        out.push(DiscoveredSkill {
            dir_name,
            path: skill_dir,
            manifest,
            spec_path,
        });
    }
    Ok(out)
}

fn parse_manifest(path: &Path) -> Result<ManifestRaw, String> {
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str::<ManifestRaw>(&body).map_err(|e| format!("parse: {e}"))
}

// ---------------------------------------------------------------------------
// Per-agent enabled-skills resolution (SKILLS.en.md §"Discovery").
// ---------------------------------------------------------------------------

/// Read `<agent>/config.manifest.json` and return the names of skills the
/// agent has enabled. An agent without a `skills.framework[]` block has an
/// empty enabled set — that is the same as having opted into none.
fn enabled_skill_names(agent_dir: &Path) -> Result<Vec<String>, String> {
    let manifest_path = agent_dir.join("config.manifest.json");
    let body = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("{}: parse: {e}", manifest_path.display()))?;
    let Some(entries) = value
        .get("skills")
        .and_then(|s| s.get("framework"))
        .and_then(|f| f.as_array())
    else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        let name = entry.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            format!(
                "{}: skills.framework[{idx}] is missing 'name'",
                manifest_path.display()
            )
        })?;
        // Per SKILLS.en.md §"Discovery" lines 154–156, `enabled` is required
        // and has no implicit default. Mirror that strictness here so the
        // CLI surfaces the manifest error the same way `bwoc check` will.
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| {
                format!(
                    "{}: skills.framework[{idx}] ({name}) is missing required 'enabled' field",
                    manifest_path.display()
                )
            })?;
        if enabled {
            out.push(name.to_string());
        }
    }
    Ok(out)
}

/// Resolve which agent's `config.manifest.json` to consult, per SKILLS.en.md
/// §"Current agent resolution" (lines 205–213). v1 supports `--agent`,
/// `BWOC_AGENT` env, and cwd-descent; falls through to a clear error.
fn resolve_current_agent(root: &Path, explicit: Option<&str>) -> Result<(String, PathBuf), String> {
    if let Some(id) = explicit {
        let dir = root.join("agents").join(id);
        if !dir.is_dir() {
            return Err(format!(
                "--agent {id}: no such agent in workspace ({})",
                dir.display()
            ));
        }
        return Ok((id.to_string(), dir));
    }
    if let Ok(env_id) = std::env::var("BWOC_AGENT") {
        if !env_id.is_empty() {
            let dir = root.join("agents").join(&env_id);
            if !dir.is_dir() {
                return Err(format!(
                    "BWOC_AGENT={env_id}: no such agent in workspace ({})",
                    dir.display()
                ));
            }
            return Ok((env_id, dir));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(rel) = cwd.strip_prefix(root.join("agents")) {
            if let Some(first) = rel.components().next() {
                let id = first.as_os_str().to_string_lossy().into_owned();
                let dir = root.join("agents").join(&id);
                if dir.is_dir() {
                    return Ok((id, dir));
                }
            }
        }
    }
    Err("no agent context; pass --agent <name> or run from within an agent directory".to_string())
}

// ---------------------------------------------------------------------------
// JSON helpers.
// ---------------------------------------------------------------------------

fn skill_summary_json(s: &DiscoveredSkill, enabled: Option<bool>) -> serde_json::Value {
    let mut v = serde_json::json!({
        "name": s.manifest.skill.name,
        "version": s.manifest.skill.version,
        "description": s.manifest.skill.description,
        "maturity": s.manifest.skill.maturity,
        "requires": s.manifest.contract.requires,
        "requires_plugins": s.manifest.contract.requires_plugins,
        "exposes": s.manifest.contract.exposes,
        "verify": s.manifest.gates.verify,
        "path": s.path.display().to_string(),
        "spec_path": s.spec_path.as_ref().map(|p| p.display().to_string()),
    });
    if let Some(b) = enabled {
        v["enabled"] = serde_json::Value::Bool(b);
    }
    v
}

fn print_json(value: &serde_json::Value) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc skill: serialize JSON failed: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// `bwoc skill list` ----------------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_list(args: ListArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill list: {e}");
            return 2;
        }
    };
    let skills = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill list: {e}");
            return 1;
        }
    };

    // If `--enabled` (or `--agent`) is in play we annotate each skill with
    // its enabled-on-this-agent status. Without it, `enabled` is absent —
    // a skill installed at workspace scope has no per-agent opinion yet.
    let agent_filter: Option<Vec<String>> = if args.enabled || args.agent.is_some() {
        let (_id, agent_dir) = match resolve_current_agent(&root, args.agent.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("bwoc skill list: {e}");
                return 2;
            }
        };
        match enabled_skill_names(&agent_dir) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("bwoc skill list: {e}");
                return 1;
            }
        }
    } else {
        None
    };

    let annotated: Vec<(DiscoveredSkill, Option<bool>)> = skills
        .into_iter()
        .map(|s| {
            let enabled = agent_filter
                .as_ref()
                .map(|names| names.iter().any(|n| n == &s.manifest.skill.name));
            (s, enabled)
        })
        .collect();
    let filtered: Vec<_> = annotated
        .into_iter()
        .filter(|(_, en)| !args.enabled || matches!(en, Some(true)))
        .collect();

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "skills": filtered
                .iter()
                .map(|(s, en)| skill_summary_json(s, *en))
                .collect::<Vec<_>>(),
        });
        return print_json(&value);
    }

    if filtered.is_empty() {
        println!(
            "(no framework skills installed at {})",
            skills_dir(&root).display()
        );
        return 0;
    }

    // Compact human table — name · version · maturity · description.
    // `enabled` column appears only when an agent filter is active.
    let with_enabled = filtered.iter().any(|(_, en)| en.is_some());
    let name_w = filtered
        .iter()
        .map(|(s, _)| s.manifest.skill.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let ver_w = filtered
        .iter()
        .map(|(s, _)| s.manifest.skill.version.len())
        .max()
        .unwrap_or(7)
        .max(7);

    if with_enabled {
        println!(
            "{:<name_w$}  {:<ver_w$}  {:<8}  {:<8}  DESCRIPTION",
            "NAME", "VERSION", "MATURITY", "ENABLED",
        );
    } else {
        println!(
            "{:<name_w$}  {:<ver_w$}  {:<8}  DESCRIPTION",
            "NAME", "VERSION", "MATURITY",
        );
    }
    for (s, en) in &filtered {
        if with_enabled {
            let en_str = match en {
                Some(true) => "yes",
                Some(false) => "no",
                None => "-",
            };
            println!(
                "{:<name_w$}  {:<ver_w$}  {:<8}  {:<8}  {}",
                s.manifest.skill.name,
                s.manifest.skill.version,
                s.manifest.skill.maturity,
                en_str,
                s.manifest.skill.description,
            );
        } else {
            println!(
                "{:<name_w$}  {:<ver_w$}  {:<8}  {}",
                s.manifest.skill.name,
                s.manifest.skill.version,
                s.manifest.skill.maturity,
                s.manifest.skill.description,
            );
        }
    }
    0
}

// ---------------------------------------------------------------------------
// `bwoc skill show <name>` ---------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_show(args: ShowArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill show: {e}");
            return 2;
        }
    };
    let skills = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill show: {e}");
            return 1;
        }
    };
    let Some(s) = skills.into_iter().find(|s| s.dir_name == args.name) else {
        eprintln!(
            "bwoc skill show: '{}' not installed in {}",
            args.name,
            skills_dir(&root).display()
        );
        return 2;
    };

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "skill": skill_summary_json(&s, None),
        });
        return print_json(&value);
    }

    println!("Skill         {}", s.manifest.skill.name);
    println!("Version       {}", s.manifest.skill.version);
    println!("Maturity      {}", s.manifest.skill.maturity);
    println!("Description   {}", s.manifest.skill.description);
    println!("Path          {}", s.path.display());
    if let Some(sp) = &s.spec_path {
        println!("Spec          {}", sp.display());
    } else {
        println!("Spec          (missing — SPEC.md not present)");
    }
    println!(
        "Requires      {}",
        if s.manifest.contract.requires.is_empty() {
            "(none)".to_string()
        } else {
            s.manifest.contract.requires.join(", ")
        }
    );
    println!(
        "Req. plugins  {}",
        if s.manifest.contract.requires_plugins.is_empty() {
            "(none)".to_string()
        } else {
            s.manifest.contract.requires_plugins.join(", ")
        }
    );
    println!("Exposes       {}", s.manifest.contract.exposes.join(", "));
    println!(
        "Verify gate   {}",
        s.manifest.gates.verify.as_deref().unwrap_or("(none)"),
    );
    0
}

// ---------------------------------------------------------------------------
// `bwoc skill verify <name>` / `--all` ---------------------------------------
// ---------------------------------------------------------------------------

/// Sentinel env var: set on every spawned `[gates].verify` shell so a
/// manifest whose verify command recursively re-enters `bwoc skill verify`
/// fails fast instead of forking unbounded.
const VERIFY_INFLIGHT_ENV: &str = "BWOC_SKILL_VERIFY_INFLIGHT";

pub fn run_verify(args: VerifyArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill verify: {e}");
            return 2;
        }
    };

    // Recursion guard — see VERIFY_INFLIGHT_ENV. A manifest whose verify
    // command shells out to `bwoc skill verify` would otherwise recurse
    // forever; surface the manifest bug to the operator instead.
    if std::env::var_os(VERIFY_INFLIGHT_ENV).is_some() {
        let names = args
            .name
            .clone()
            .map(|n| format!(" for '{n}'"))
            .unwrap_or_default();
        eprintln!(
            "bwoc skill verify{names}: refusing to recurse — \
             a parent `bwoc skill verify` is already running. \
             Check the skill's [gates].verify command; it must not re-invoke `bwoc skill verify`."
        );
        return 3;
    }

    let skills = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill verify: {e}");
            return 1;
        }
    };

    let targets: Vec<DiscoveredSkill> = if args.all {
        skills
    } else {
        let Some(name) = args.name.as_deref() else {
            // clap's ArgGroup guarantees one of (name, --all); defensive only.
            eprintln!("bwoc skill verify: pass <name> or --all");
            return 2;
        };
        let Some(s) = skills.into_iter().find(|s| s.dir_name == name) else {
            eprintln!(
                "bwoc skill verify: '{name}' not installed in {}",
                skills_dir(&root).display()
            );
            return 2;
        };
        vec![s]
    };

    // BWOC-45: skill-on-plugin dependency resolution substrate. The set of
    // plugin kinds with an enabled plugin in this workspace — resolved once,
    // reused for every target. `bwoc skill verify` runs the SAME check agent
    // spawn does (SKILLS.en.md §"Skill-on-plugin dependency", line 195) so the
    // gap surfaces in pre-flight, not at runtime.
    let available_kinds = crate::plugin::enabled_plugin_kinds(&root);

    let mut results: Vec<serde_json::Value> = Vec::with_capacity(targets.len());
    let mut overall_ok = true;
    // Tracks whether any skill declared a gate that was printed but not run, so
    // the human report can surface the trust-boundary footer once at the end.
    let mut any_gate_not_run = false;

    for s in &targets {
        let started = std::time::Instant::now();
        // `executed` distinguishes "gate ran" from "gate declared but not run"
        // (the safe default) and "no gate at all" (`command_present == false`).
        let (exit_code, ok, command_present, executed) = match &s.manifest.gates.verify {
            None => {
                // Per SKILLS.en.md line 74, [gates].verify is optional.
                // No gate → no claim of passing, no claim of failing. Report
                // as "skipped" via a null exit_code in JSON, and "ok = true"
                // so --all does not fail solely because gates were declared
                // absent.
                (None, true, false, false)
            }
            Some(_cmd) if !args.run_gates => {
                // SECURITY (BWOC-37): [gates].verify is arbitrary shell pulled
                // from an untrusted manifest. The default path NEVER executes
                // it — static checks only (the manifest parsed and declares a
                // gate). The command is printed below so the operator can audit
                // it before opting in via --run-gates.
                any_gate_not_run = true;
                (None, true, true, false)
            }
            Some(cmd) => {
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .current_dir(&root)
                    .env(VERIFY_INFLIGHT_ENV, "1")
                    .status();
                match status {
                    Ok(st) => {
                        let code = st.code().unwrap_or(-1);
                        (Some(code), code == 0, true, true)
                    }
                    Err(e) => {
                        eprintln!(
                            "bwoc skill verify: '{}': spawn failed: {e}",
                            s.manifest.skill.name
                        );
                        (Some(-1), false, true, true)
                    }
                }
            }
        };
        if !ok {
            overall_ok = false;
        }
        // BWOC-45: resolve [contract].requires_plugins against the enabled
        // plugin kinds. A required kind with no enabled plugin is unmet — the
        // agent would be half-wired at spawn — so the skill fails verify.
        let missing_kinds: Vec<String> = s
            .manifest
            .contract
            .requires_plugins
            .iter()
            .filter(|k| !available_kinds.contains(k.as_str()))
            .cloned()
            .collect();
        let deps_ok = missing_kinds.is_empty();
        if !deps_ok {
            overall_ok = false;
        }
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if args.json {
            results.push(serde_json::json!({
                "skill": s.manifest.skill.name,
                "verify_command": s.manifest.gates.verify,
                "exit_code": exit_code,
                "ok": ok && deps_ok,
                "executed": executed,
                "skipped": !command_present,
                "duration_ms": elapsed_ms,
                "requires_plugins": s.manifest.contract.requires_plugins,
                "missing_plugin_kinds": missing_kinds,
                "deps_ok": deps_ok,
            }));
        } else if !command_present {
            println!("- {}  (skipped — no [gates].verify)", s.manifest.skill.name);
        } else if !executed {
            println!(
                "- {}  (gate not run — pass --run-gates)",
                s.manifest.skill.name
            );
            if let Some(cmd) = &s.manifest.gates.verify {
                println!("      would run (sh -c): {cmd}");
            }
        } else {
            let tag = if ok { "OK" } else { "FAIL" };
            println!("{tag:<5} {}  ({elapsed_ms} ms)", s.manifest.skill.name);
        }
        // BWOC-45: surface an unmet plugin dependency on its own line so it is
        // visible even when the gate itself was skipped or not run.
        if !args.json && !missing_kinds.is_empty() {
            println!(
                "      ✗ unmet plugin dependency — no enabled plugin of kind(s): {}",
                missing_kinds.join(", ")
            );
        }
    }

    if !args.json && any_gate_not_run {
        println!();
        println!(
            "Gates were NOT executed. [gates].verify commands come from the skill\n\
             manifest — UNTRUSTED input — so they are printed, not run. Review the\n\
             commands above, then re-run with --run-gates to execute them (sh -c)."
        );
    }

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "ok": overall_ok,
            "gates_executed": args.run_gates,
            "results": results,
        });
        return print_json(&value).max(if overall_ok { 0 } else { 1 });
    }

    if overall_ok { 0 } else { 1 }
}

// ===========================================================================
// Write-side surface (BWOC-23) ==============================================
// ===========================================================================

use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct InitArgs {
    pub common: CommonArgs,
    pub name: String,
    /// Override `{{skillVersion}}`. Default `0.1.0`.
    pub version: Option<String>,
    /// Override `{{skillDescription}}`. Default a hint placeholder.
    pub description: Option<String>,
    /// Override `{{skillOperation}}`. Default `<name>_op` (snake-cased).
    pub operation: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct InstallArgs {
    pub common: CommonArgs,
    /// Source argument: local path, git URL (`*.git[#ref]`), or tarball URL (`*.tar.gz` / `*.tgz`).
    pub source: String,
    /// Skip the SHA-256 trust gate. Emits a stderr warning.
    pub no_verify: bool,
    /// Required the first time a source URL is installed in this workspace.
    pub allow_new_source: bool,
    /// Replace an existing install in place (retains the registry record).
    pub upgrade: bool,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct EnableArgs {
    pub common: CommonArgs,
    pub name: String,
    /// Override the current-agent resolution.
    pub agent: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct DisableArgs {
    pub common: CommonArgs,
    pub name: String,
    pub agent: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct RemoveArgs {
    pub common: CommonArgs,
    pub name: String,
    /// Skip the confirmation prompt. Required with `--json`.
    pub yes: bool,
    /// Also drop the entry from `.bwoc/installed-sources.toml`.
    pub forget_source: bool,
    pub json: bool,
}

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

fn template_dir(root: &Path) -> PathBuf {
    root.join("modules/skill-template")
}

fn installed_sources_path(root: &Path) -> PathBuf {
    root.join(".bwoc/installed-sources.toml")
}

/// kebab-case validator. Allows `[a-z0-9]+(-[a-z0-9]+)*`. Rejects empty,
/// path separators, leading/trailing dashes, double dashes, uppercase.
fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("skill name is empty".to_string());
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(format!(
            "'{name}' is not a valid skill name (no path separators)"
        ));
    }
    let mut prev_dash = true;
    for c in name.chars() {
        let valid = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if !valid {
            return Err(format!(
                "'{name}' is not kebab-case (only [a-z0-9-], single dashes)"
            ));
        }
        if c == '-' && prev_dash {
            return Err(format!(
                "'{name}' is not kebab-case (no leading or consecutive dashes)"
            ));
        }
        prev_dash = c == '-';
    }
    if prev_dash {
        return Err(format!("'{name}' is not kebab-case (no trailing dash)"));
    }
    Ok(())
}

/// Best-effort kebab→snake for the default `{{skillOperation}}`.
fn default_operation(name: &str) -> String {
    let snake = name.replace('-', "_");
    format!("{snake}_op")
}

/// Substitute the four documented placeholders. Unknown `{{...}}` markers
/// are left in place — the operator is the editor of last resort.
fn substitute_placeholders(
    body: &str,
    name: &str,
    version: &str,
    description: &str,
    operation: &str,
) -> String {
    body.replace("{{skillName}}", name)
        .replace("{{skillVersion}}", version)
        .replace("{{skillDescription}}", description)
        .replace("{{skillOperation}}", operation)
}

/// Recursively copy `src` into `dst`. Both must be directories; `dst` must
/// not exist. Symlinks are skipped (the template ships no symlinks); only
/// regular files and directories are reproduced.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<Vec<PathBuf>, String> {
    if !src.is_dir() {
        return Err(format!("source is not a directory: {}", src.display()));
    }
    if dst.exists() {
        return Err(format!("destination already exists: {}", dst.display()));
    }
    let mut written = Vec::new();
    std::fs::create_dir_all(dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        for entry in std::fs::read_dir(&s).map_err(|e| format!("read {}: {e}", s.display()))? {
            let entry = entry.map_err(|e| format!("read entry in {}: {e}", s.display()))?;
            let sp = entry.path();
            let dp = d.join(entry.file_name());
            let ft = entry
                .file_type()
                .map_err(|e| format!("stat {}: {e}", sp.display()))?;
            if ft.is_dir() {
                std::fs::create_dir_all(&dp)
                    .map_err(|e| format!("create {}: {e}", dp.display()))?;
                stack.push((sp, dp));
            } else if ft.is_file() {
                std::fs::copy(&sp, &dp)
                    .map_err(|e| format!("copy {} -> {}: {e}", sp.display(), dp.display()))?;
                written.push(dp);
            }
            // Skip symlinks deliberately — neither template nor installable
            // skill should embed them. Surface as an error if we ever see one
            // so it does not silently vanish from the materialized tree.
            else if ft.is_symlink() {
                return Err(format!(
                    "symlink encountered (unsupported): {}",
                    sp.display()
                ));
            }
        }
    }
    Ok(written)
}

/// Deterministic SHA-256 of a directory tree — sorted-path walk over regular
/// files; header `<rel-path>\0<size>\0` then file bytes then `\n`.
fn sha256_tree(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(cur) = stack.pop() {
        for entry in std::fs::read_dir(&cur).map_err(|e| format!("read {}: {e}", cur.display()))? {
            let entry = entry.map_err(|e| format!("read entry in {}: {e}", cur.display()))?;
            let path = entry.path();
            let ft = entry
                .file_type()
                .map_err(|e| format!("stat {}: {e}", path.display()))?;
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut hasher = Sha256::new();
    for f in &files {
        let rel = f
            .strip_prefix(root)
            .map_err(|_| format!("strip prefix: {}", f.display()))?;
        let bytes = std::fs::read(f).map_err(|e| format!("read {}: {e}", f.display()))?;
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes.len().to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(&bytes);
        hasher.update(b"\n");
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

fn sha256_string(s: &str) -> String {
    sha256_bytes(s.as_bytes())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Three source kinds per SKILLS.en.md §"Sources & Installation" line 220.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceKind {
    LocalPath(PathBuf),
    /// (url-without-fragment, optional ref)
    GitUrl(String, Option<String>),
    TarballUrl(String),
}

fn detect_source_kind(src: &str) -> Result<SourceKind, String> {
    if src.starts_with("./") || src.starts_with("../") || src.starts_with('/') {
        return Ok(SourceKind::LocalPath(PathBuf::from(src)));
    }
    let lower = src.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("git://")
    {
        // Tarball detection precedes git detection — a URL that ends in
        // `.tar.gz`/`.tgz` is unambiguously an archive even when served from
        // a git host.
        let (path_part, _fragment) = src.split_once('#').unwrap_or((src, ""));
        let path_lower = path_part.to_ascii_lowercase();
        if path_lower.ends_with(".tar.gz") || path_lower.ends_with(".tgz") {
            return Ok(SourceKind::TarballUrl(src.to_string()));
        }
        if path_lower.ends_with(".git") {
            let (url, frag) = src.split_once('#').unwrap_or((src, ""));
            let r = if frag.is_empty() {
                None
            } else {
                // SECURITY (BWOC-39): the ref is passed to `git clone --branch
                // <ref>`. A ref beginning with '-' (e.g. `--upload-pack=evil`)
                // would be parsed by git as a flag, not a ref — argument
                // injection. Reject it before it reaches the git invocation.
                if frag.starts_with('-') {
                    return Err(format!(
                        "invalid git ref '{frag}': a ref must not begin with '-' \
                         (it would be parsed as a git flag)"
                    ));
                }
                Some(frag.to_string())
            };
            return Ok(SourceKind::GitUrl(url.to_string(), r));
        }
    }
    Err(format!(
        "unrecognized source '{src}' — expected local path (./, ../, /), \
         git URL (*.git[#ref]), or tarball URL (*.tar.gz / *.tgz)"
    ))
}

/// Existing installed-sources.toml entry. v1 only uses `source_key` for the
/// "have we seen this source before?" check; full row is preserved in TOML.
#[derive(Debug, Clone)]
struct InstalledSource {
    source_key: String,
}

fn source_key(url: &str) -> String {
    sha256_string(url)
}

/// Parse `.bwoc/installed-sources.toml` into a flat list. Missing file is OK
/// (returns empty). The format is a top-level table keyed by source_key
/// per SKILLS.en.md §"`.bwoc/installed-sources.toml` schema".
fn load_installed_sources(root: &Path) -> Result<Vec<InstalledSource>, String> {
    let path = installed_sources_path(root);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let table = value
        .as_table()
        .ok_or_else(|| format!("{}: top-level is not a table", path.display()))?;
    let mut out = Vec::new();
    for (key, entry) in table {
        // Validate the entry is a table — surfaces hand-edit corruption before
        // it silently degrades the "is this source new?" check downstream.
        entry
            .as_table()
            .ok_or_else(|| format!("{}: entry '{key}' is not a table", path.display()))?;
        out.push(InstalledSource {
            source_key: key.clone(),
        });
    }
    Ok(out)
}

/// Append (or replace) one entry. Other entries are preserved.
fn record_installed_source(
    root: &Path,
    key: &str,
    url: &str,
    name: &str,
    target_rel: &str,
    installed_hash: &str,
    acknowledged_by: &str,
) -> Result<(), String> {
    let path = installed_sources_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let mut doc: toml::Table = if path.is_file() {
        let body =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        toml::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))?
    } else {
        toml::Table::new()
    };
    let now = current_utc_iso8601();
    let mut entry = toml::Table::new();
    entry.insert("url".into(), toml::Value::String(url.to_string()));
    entry.insert("kind".into(), toml::Value::String("skill".to_string()));
    entry.insert("name".into(), toml::Value::String(name.to_string()));
    entry.insert("target".into(), toml::Value::String(target_rel.to_string()));
    entry.insert("installed_at".into(), toml::Value::String(now.clone()));
    entry.insert(
        "installed_hash".into(),
        toml::Value::String(installed_hash.to_string()),
    );
    entry.insert("last_verified".into(), toml::Value::String(now));
    entry.insert(
        "acknowledged_by".into(),
        toml::Value::String(acknowledged_by.to_string()),
    );
    doc.insert(key.to_string(), toml::Value::Table(entry));
    let body = toml::to_string_pretty(&doc).map_err(|e| format!("serialize toml: {e}"))?;
    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

/// Remove one source_key entry from `.bwoc/installed-sources.toml`.
/// Missing file or missing key is a no-op (success).
fn forget_installed_source(root: &Path, name: &str) -> Result<bool, String> {
    let path = installed_sources_path(root);
    if !path.is_file() {
        return Ok(false);
    }
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut doc: toml::Table =
        toml::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let keys: Vec<String> = doc
        .iter()
        .filter_map(|(k, v)| {
            let t = v.as_table()?;
            let n = t.get("name")?.as_str()?;
            let kind = t.get("kind").and_then(|v| v.as_str()).unwrap_or("skill");
            (n == name && kind == "skill").then(|| k.clone())
        })
        .collect();
    let removed = !keys.is_empty();
    for k in keys {
        doc.remove(&k);
    }
    let body = toml::to_string_pretty(&doc).map_err(|e| format!("serialize toml: {e}"))?;
    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(removed)
}

fn current_utc_iso8601() -> String {
    // Std-only ISO 8601 (UTC seconds precision). No chrono dependency for
    // a single timestamp field — staying scope-disciplined.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days algorithm (Howard Hinnant). Std doesn't expose UTC
    // breakdown, so we do it by hand. Good through year 4000.
    let days = (secs / 86_400) as i64;
    let sod = (secs % 86_400) as u32;
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let hh = sod / 3600;
    let mm = (sod % 3600) / 60;
    let ss = sod % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, m, d, hh, mm, ss
    )
}

/// Run an external command in `cwd`, capturing stderr. Returns Ok(stdout) on
/// success; Err with the command + stderr on failure or spawn error. The
/// caller frames the user-facing error.
fn run_capture(cwd: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("spawn {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} {} failed (exit {}): {}",
            args.join(" "),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// Agent-manifest mutation helpers.
// ---------------------------------------------------------------------------

/// Load `<agent>/config.manifest.json` as a mutable JSON Value. Errors carry
/// the path so the operator can find the culprit.
fn load_agent_manifest(agent_dir: &Path) -> Result<serde_json::Value, String> {
    let path = agent_dir.join("config.manifest.json");
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn save_agent_manifest(agent_dir: &Path, value: &serde_json::Value) -> Result<(), String> {
    let path = agent_dir.join("config.manifest.json");
    let mut body =
        serde_json::to_string_pretty(value).map_err(|e| format!("serialize JSON: {e}"))?;
    body.push('\n');
    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Set the `enabled` field on one entry in `skills.framework[]`, adding the
/// entry if it does not yet exist. Returns the resolved entry name + final
/// enabled value + whether the entry was newly added.
fn set_skill_enabled_in_manifest(
    manifest: &mut serde_json::Value,
    name: &str,
    version_constraint: &str,
    enabled: bool,
    require_existing: bool,
) -> Result<(bool, bool), String> {
    let obj = manifest
        .as_object_mut()
        .ok_or_else(|| "manifest is not a JSON object".to_string())?;
    let skills = obj
        .entry("skills".to_string())
        .or_insert(serde_json::json!({}));
    let skills_obj = skills
        .as_object_mut()
        .ok_or_else(|| "manifest.skills is not an object".to_string())?;
    let framework = skills_obj
        .entry("framework".to_string())
        .or_insert(serde_json::json!([]));
    let arr = framework
        .as_array_mut()
        .ok_or_else(|| "manifest.skills.framework is not an array".to_string())?;
    for entry in arr.iter_mut() {
        let entry_name = entry
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if entry_name.as_deref() == Some(name) {
            entry
                .as_object_mut()
                .unwrap()
                .insert("enabled".to_string(), serde_json::Value::Bool(enabled));
            return Ok((false, enabled));
        }
    }
    if require_existing {
        return Err(format!(
            "no skills.framework[] entry for '{name}' (run `bwoc skill enable {name}` first)"
        ));
    }
    arr.push(serde_json::json!({
        "name": name,
        "version": version_constraint,
        "enabled": enabled,
    }));
    Ok((true, enabled))
}

/// Drop every `skills.framework[]` entry whose `name == <name>`. Returns the
/// number of removed entries (0 if the agent never referenced the skill).
fn remove_skill_from_manifest(manifest: &mut serde_json::Value, name: &str) -> usize {
    let Some(arr) = manifest
        .get_mut("skills")
        .and_then(|s| s.get_mut("framework"))
        .and_then(|f| f.as_array_mut())
    else {
        return 0;
    };
    let before = arr.len();
    arr.retain(|entry| entry.get("name").and_then(|v| v.as_str()) != Some(name));
    before - arr.len()
}

/// Walk `<workspace>/agents/*/config.manifest.json`, return the ids of agents
/// whose `skills.framework[]` references the named skill.
fn agents_consuming(root: &Path, skill_name: &str) -> Result<Vec<String>, String> {
    let agents_dir = root.join("agents");
    if !agents_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(&agents_dir).map_err(|e| format!("read {}: {e}", agents_dir.display()))?
    {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = entry.path().join("config.manifest.json");
        if !manifest_path.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
        let v: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => continue, // hand-edited manifest; skip rather than block remove
        };
        let consumes = v
            .get("skills")
            .and_then(|s| s.get("framework"))
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|e| e.get("name").and_then(|n| n.as_str()) == Some(skill_name))
            })
            .unwrap_or(false);
        if consumes {
            out.push(id);
        }
    }
    out.sort();
    Ok(out)
}

// ---------------------------------------------------------------------------
// `bwoc skill init <name>` ---------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_init(args: InitArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill init: {e}");
            return 2;
        }
    };
    if let Err(e) = validate_skill_name(&args.name) {
        eprintln!("bwoc skill init: {e}");
        return 2;
    }
    let template = template_dir(&root);
    if !template.is_dir() {
        eprintln!(
            "bwoc skill init: template missing at {}. \
             Run a workspace with `modules/skill-template/` (jisoo's BWOC-22).",
            template.display()
        );
        return 2;
    }
    let target = skills_dir(&root).join(&args.name);
    if target.exists() {
        eprintln!(
            "bwoc skill init: target already exists: {}",
            target.display()
        );
        return 2;
    }

    let version = args.version.as_deref().unwrap_or("0.1.0").to_string();
    let description = args
        .description
        .as_deref()
        .unwrap_or("Describe what this skill does (one sentence).")
        .to_string();
    let operation = args
        .operation
        .clone()
        .unwrap_or_else(|| default_operation(&args.name));

    // Materialize: copy then rewrite each file's text in place.
    let written = match copy_dir_recursive(&template, &target) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill init: {e}");
            // Roll back partial copy — caller asked for atomic-ish init.
            let _ = std::fs::remove_dir_all(&target);
            return 1;
        }
    };
    let mut substituted: Vec<String> = Vec::with_capacity(written.len());
    for file in &written {
        if let Ok(text) = std::fs::read_to_string(file) {
            let out =
                substitute_placeholders(&text, &args.name, &version, &description, &operation);
            if let Err(e) = std::fs::write(file, out) {
                eprintln!("bwoc skill init: write {}: {e}", file.display());
                let _ = std::fs::remove_dir_all(&target);
                return 1;
            }
            substituted.push(file.display().to_string());
        }
    }

    // Drop an `.authored-in-place` marker so `bwoc check`'s orphan-installation
    // gate (SKILLS.en.md §"Verification" line 352) treats this as authored,
    // not installed-from-source.
    let marker = target.join(".authored-in-place");
    if let Err(e) = std::fs::write(&marker, "") {
        eprintln!(
            "bwoc skill init: warning — could not write {}: {e}",
            marker.display()
        );
    }

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "name": args.name,
            "target": target.display().to_string(),
            "files_written": substituted,
            "placeholders": {
                "skillName": args.name,
                "skillVersion": version,
                "skillDescription": description,
                "skillOperation": operation,
            },
            "authored_in_place": true,
        });
        return print_json(&value);
    }

    println!("Initialized framework skill '{}'", args.name);
    println!("  Target:      {}", target.display());
    println!("  Version:     {version}");
    println!("  Operation:   {operation}");
    println!("  Files:       {} written", substituted.len());
    println!();
    println!("Next: edit SPEC.md, bump maturity honestly, then");
    println!(
        "      `bwoc skill enable {}` on the consuming agent.",
        args.name
    );
    0
}

// ---------------------------------------------------------------------------
// `bwoc skill install <source>` ----------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_install(args: InstallArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill install: {e}");
            return 2;
        }
    };
    let kind = match detect_source_kind(&args.source) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("bwoc skill install: {e}");
            return 2;
        }
    };

    // First-install gate: check installed-sources.toml for prior records of
    // this exact source string. The spec keys by SHA-256(url) so we compute
    // that even when the source is local — local paths still get a key.
    let key = source_key(&args.source);
    let prior = match load_installed_sources(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill install: {e}");
            return 1;
        }
    };
    let already_known = prior.iter().any(|s| s.source_key == key);
    if !already_known && !args.allow_new_source {
        eprintln!(
            "bwoc skill install: '{}' has not been installed in this workspace before. \
             Pass --allow-new-source to acknowledge you have inspected this source.",
            args.source
        );
        return 2;
    }

    // Stage into a tempdir under /tmp, validate, then move into place.
    let stage = match stage_source(&kind, &args.source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bwoc skill install: {e}");
            return 1;
        }
    };

    // Trust gate.
    let mut checksum_outcome: Option<String> = None;
    if !args.no_verify {
        match verify_checksum(&kind, &stage.staged_dir, &stage.archive_path) {
            Ok(s) => checksum_outcome = Some(s),
            Err(e) => {
                eprintln!("bwoc skill install: trust-gate failed: {e}");
                let _ = std::fs::remove_dir_all(&stage.staged_dir);
                return 1;
            }
        }
    } else {
        eprintln!(
            "bwoc skill install: warning — --no-verify skips SHA-256 verification of {}",
            args.source
        );
    }

    // Read staged manifest to discover the skill name.
    let manifest_path = stage.staged_dir.join("manifest.toml");
    if !manifest_path.is_file() {
        eprintln!(
            "bwoc skill install: source missing manifest.toml at staged root; \
             cannot resolve skill name"
        );
        let _ = std::fs::remove_dir_all(&stage.staged_dir);
        return 1;
    }
    let staged_manifest = match parse_manifest(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc skill install: parse staged manifest: {e}");
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
    };
    let skill_name = staged_manifest.skill.name.clone();
    if let Err(e) = validate_skill_name(&skill_name) {
        eprintln!("bwoc skill install: manifest skill name: {e}");
        let _ = std::fs::remove_dir_all(&stage.staged_dir);
        return 2;
    }

    let target = skills_dir(&root).join(&skill_name);
    if target.exists() {
        if !args.upgrade {
            eprintln!(
                "bwoc skill install: '{skill_name}' already installed at {}; \
                 pass --upgrade to replace",
                target.display()
            );
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 2;
        }
        if let Err(e) = std::fs::remove_dir_all(&target) {
            eprintln!(
                "bwoc skill install: --upgrade: failed to remove {}: {e}",
                target.display()
            );
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
    }

    if let Some(parent) = target.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("bwoc skill install: create {}: {e}", parent.display());
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
    }
    if let Err(e) = std::fs::rename(&stage.staged_dir, &target) {
        // Cross-device or some other rename failure — fall back to copy + remove.
        if let Err(e2) = copy_dir_recursive(&stage.staged_dir, &target) {
            eprintln!(
                "bwoc skill install: install {} -> {}: {e} (copy fallback also failed: {e2})",
                stage.staged_dir.display(),
                target.display()
            );
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
        let _ = std::fs::remove_dir_all(&stage.staged_dir);
    }

    let installed_hash = match sha256_tree(&target) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("bwoc skill install: hash {}: {e}", target.display());
            return 1;
        }
    };

    let acknowledged_by = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let target_rel = format!("modules/skills/{skill_name}");
    if let Err(e) = record_installed_source(
        &root,
        &key,
        &args.source,
        &skill_name,
        &target_rel,
        &installed_hash,
        &acknowledged_by,
    ) {
        eprintln!("bwoc skill install: warning — could not record source: {e}");
    }

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "source": args.source,
            "source_kind": match &kind {
                SourceKind::LocalPath(p) => format!("local-path:{}", p.display()),
                SourceKind::GitUrl(u, r) => match r {
                    Some(r) => format!("git:{u}#{r}"),
                    None => format!("git:{u}"),
                },
                SourceKind::TarballUrl(u) => format!("tarball:{u}"),
            },
            "name": skill_name,
            "target": target.display().to_string(),
            "installed_hash": installed_hash,
            "trust_gate": match (args.no_verify, checksum_outcome.as_deref()) {
                (true, _) => "skipped",
                (false, Some(_)) => "verified",
                (false, None) => "n/a",
            },
            "newly_registered": !already_known,
            "upgrade": args.upgrade,
        });
        return print_json(&value);
    }

    println!("Installed framework skill '{skill_name}'");
    println!("  Source:      {}", args.source);
    println!("  Target:      {}", target.display());
    println!("  Tree hash:   {}", installed_hash);
    println!(
        "  Trust gate:  {}",
        if args.no_verify {
            "SKIPPED (--no-verify)".to_string()
        } else if let Some(s) = checksum_outcome {
            format!("verified ({s})")
        } else {
            "n/a (no sidecar)".to_string()
        }
    );
    println!();
    println!("Skill is dormant. Run `bwoc skill enable {skill_name}` on the consuming agent.");
    0
}

struct StagedSource {
    staged_dir: PathBuf,
    /// For tarball installs only — the downloaded archive byte path, kept
    /// alongside the staged dir so verify_checksum can read it.
    archive_path: Option<PathBuf>,
}

fn stage_source(kind: &SourceKind, raw: &str) -> Result<StagedSource, String> {
    // All stages land under a per-invocation tempdir to avoid colliding with
    // any concurrent install. We never share staging space across installs.
    let stem = sha256_string(raw);
    let stage_root = std::env::temp_dir().join(format!("bwoc-skill-install-{}", &stem[..16]));
    let _ = std::fs::remove_dir_all(&stage_root);
    std::fs::create_dir_all(&stage_root)
        .map_err(|e| format!("create {}: {e}", stage_root.display()))?;
    let staged_dir = stage_root.join("staged");
    match kind {
        SourceKind::LocalPath(p) => {
            let abs = if p.is_absolute() {
                p.clone()
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("cwd: {e}"))?
                    .join(p)
            };
            copy_dir_recursive(&abs, &staged_dir)?;
            Ok(StagedSource {
                staged_dir,
                archive_path: None,
            })
        }
        SourceKind::GitUrl(url, r) => {
            let mut args: Vec<&str> = vec!["clone", "--depth", "1"];
            if let Some(rf) = r.as_deref() {
                args.push("--branch");
                args.push(rf);
            }
            args.push(url.as_str());
            let staged_str = staged_dir.to_string_lossy().into_owned();
            args.push(&staged_str);
            run_capture(&stage_root, "git", &args).map_err(|e| format!("git clone {url}: {e}"))?;
            // Drop .git — installed skill is a flat snapshot.
            let _ = std::fs::remove_dir_all(staged_dir.join(".git"));
            Ok(StagedSource {
                staged_dir,
                archive_path: None,
            })
        }
        SourceKind::TarballUrl(url) => {
            let archive = stage_root.join("source.tar.gz");
            let archive_str = archive.to_string_lossy().into_owned();
            run_capture(
                &stage_root,
                "curl",
                &["-fsSL", "-o", &archive_str, url.as_str()],
            )
            .map_err(|e| format!("curl {url}: {e}"))?;
            std::fs::create_dir_all(&staged_dir)
                .map_err(|e| format!("create {}: {e}", staged_dir.display()))?;
            // SECURITY (BWOC-38): validate every member BEFORE extracting so a
            // crafted archive cannot escape `staged_dir` via `..` or an
            // absolute path. List first, reject on any unsafe member.
            let listing = run_capture(&stage_root, "tar", &["-tzf", &archive_str])
                .map_err(|e| format!("tar -tzf: {e}"))?;
            crate::util::assert_safe_tar_listing(&listing)?;
            let extract_str = staged_dir.to_string_lossy().into_owned();
            // --strip-components=1 collapses the conventional top-level
            // `<name>-<version>/` directory inside archives.
            run_capture(
                &stage_root,
                "tar",
                &[
                    "-xzf",
                    &archive_str,
                    "-C",
                    &extract_str,
                    "--strip-components=1",
                ],
            )
            .map_err(|e| format!("tar -xzf: {e}"))?;
            Ok(StagedSource {
                staged_dir,
                archive_path: Some(archive),
            })
        }
    }
}

fn verify_checksum(
    kind: &SourceKind,
    staged_dir: &Path,
    archive_path: &Option<PathBuf>,
) -> Result<String, String> {
    match kind {
        SourceKind::LocalPath(p) => {
            // Spec: if a sibling `<dir>.sha256` exists, verify; otherwise
            // local paths are operator-trusted by convention. Return Ok with
            // a "n/a" descriptor so the caller surfaces an honest report.
            let sidecar = sibling_sha256(p);
            if !sidecar.is_file() {
                return Ok("local-path: no sidecar".to_string());
            }
            let expected = read_expected_digest(&sidecar)?;
            let actual = sha256_tree(staged_dir)?;
            if expected != actual {
                return Err(format!(
                    "local-path checksum mismatch (expected {expected}, got {actual})"
                ));
            }
            Ok(format!("local-path sha256 ok ({})", &expected[..16]))
        }
        SourceKind::TarballUrl(url) => {
            let archive = archive_path
                .as_ref()
                .ok_or_else(|| "tarball staged without archive_path".to_string())?;
            let sidecar = format!("{url}.sha256");
            let staged_root = staged_dir
                .parent()
                .ok_or_else(|| "no parent for staged dir".to_string())?;
            let sidecar_path = staged_root.join("source.sha256");
            let sidecar_str = sidecar_path.to_string_lossy().into_owned();
            run_capture(
                staged_root,
                "curl",
                &["-fsSL", "-o", &sidecar_str, sidecar.as_str()],
            )
            .map_err(|e| format!("fetch checksum {sidecar}: {e}"))?;
            let expected = read_expected_digest(&sidecar_path)?;
            let bytes =
                std::fs::read(archive).map_err(|e| format!("read {}: {e}", archive.display()))?;
            let actual = sha256_bytes(&bytes);
            if expected != actual {
                return Err(format!(
                    "tarball checksum mismatch (expected {expected}, got {actual})"
                ));
            }
            Ok(format!("tarball sha256 ok ({})", &expected[..16]))
        }
        SourceKind::GitUrl(url, _r) => {
            // Per spec the operator publishes a manifest of tree-shas keyed
            // by ref at <url>.replace(".git", ".sha256"). v1: fetch the file
            // and compare against the post-clone tree hash. Format is one
            // `<ref> <sha256>` per line.
            let sidecar_url = url.replace(".git", ".sha256");
            let staged_root = staged_dir
                .parent()
                .ok_or_else(|| "no parent for staged dir".to_string())?;
            let sidecar_path = staged_root.join("source.sha256");
            let sidecar_str = sidecar_path.to_string_lossy().into_owned();
            if run_capture(
                staged_root,
                "curl",
                &["-fsSL", "-o", &sidecar_str, sidecar_url.as_str()],
            )
            .is_err()
            {
                // BWOC-38: a missing sidecar is NOT a pass — silently returning
                // "ok" here would bypass the SHA-256 gate for git sources.
                // Refuse so the operator chooses explicitly: publish a sidecar,
                // or pass --no-verify to install unverified.
                return Err(format!(
                    "no SHA-256 sidecar published at {sidecar_url} — publish a \
                     `.sha256`, or pass --no-verify to install this git source unverified"
                ));
            }
            let expected = read_expected_digest(&sidecar_path)?;
            let actual = sha256_tree(staged_dir)?;
            if expected != actual {
                return Err(format!(
                    "git tree checksum mismatch (expected {expected}, got {actual})"
                ));
            }
            Ok(format!("git sha256 ok ({})", &expected[..16]))
        }
    }
}

fn sibling_sha256(dir: &Path) -> PathBuf {
    let parent = dir.parent().unwrap_or(Path::new("."));
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{name}.sha256"))
}

fn read_expected_digest(path: &Path) -> Result<String, String> {
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    // Accept either a bare hex string or the `<sha>  <name>` shasum format.
    // Multi-line files (git ref manifests): take the first 64-char hex token.
    for line in body.lines() {
        for tok in line.split_whitespace() {
            if tok.len() == 64 && tok.chars().all(|c| c.is_ascii_hexdigit()) {
                return Ok(tok.to_ascii_lowercase());
            }
        }
    }
    Err(format!("{}: no SHA-256 digest found", path.display()))
}

// ---------------------------------------------------------------------------
// `bwoc skill enable / disable <name>` --------------------------------------
// ---------------------------------------------------------------------------

pub fn run_enable(args: EnableArgs) -> i32 {
    run_enable_disable(args.common, args.name, args.agent, args.json, true)
}

pub fn run_disable(args: DisableArgs) -> i32 {
    run_enable_disable(args.common, args.name, args.agent, args.json, false)
}

fn run_enable_disable(
    common: CommonArgs,
    name: String,
    agent: Option<String>,
    json: bool,
    enable: bool,
) -> i32 {
    let verb = if enable { "enable" } else { "disable" };
    let root = match resolve_workspace(&common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill {verb}: {e}");
            return 2;
        }
    };
    // Skill must be discoverable for `enable`; `disable` tolerates a missing
    // skill on disk (the operator may be cleaning up after a manual remove).
    let skills = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill {verb}: {e}");
            return 1;
        }
    };
    let discovered = skills.iter().find(|s| s.dir_name == name);
    if enable && discovered.is_none() {
        eprintln!(
            "bwoc skill enable: '{name}' is not installed under {}",
            skills_dir(&root).display()
        );
        return 2;
    }
    let version_constraint = discovered
        .map(|s| format!(">={}", s.manifest.skill.version))
        .unwrap_or_else(|| ">=0.0.0".to_string());

    let (agent_id, agent_dir) = match resolve_current_agent(&root, agent.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill {verb}: {e}");
            return 2;
        }
    };
    let mut manifest = match load_agent_manifest(&agent_dir) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill {verb}: {e}");
            return 1;
        }
    };
    let (added, final_enabled) = match set_skill_enabled_in_manifest(
        &mut manifest,
        &name,
        &version_constraint,
        enable,
        !enable, // disable requires the entry to already exist
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill {verb}: {e}");
            return 2;
        }
    };
    if let Err(e) = save_agent_manifest(&agent_dir, &manifest) {
        eprintln!("bwoc skill {verb}: {e}");
        return 1;
    }

    if json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "agent": agent_id,
            "skill": name,
            "enabled": final_enabled,
            "entry_added": added,
            "version_constraint": version_constraint,
        });
        return print_json(&value);
    }

    if added {
        println!(
            "Added skills.framework[] entry for '{name}' on '{agent_id}' (enabled={final_enabled})"
        );
    } else {
        println!("Set enabled={final_enabled} on '{name}' for '{agent_id}'");
    }
    0
}

// ---------------------------------------------------------------------------
// `bwoc skill remove <name>` -------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_remove(args: RemoveArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc skill remove: {e}");
            return 2;
        }
    };
    let target = skills_dir(&root).join(&args.name);
    let dir_exists = target.is_dir();

    let consumers = match agents_consuming(&root, &args.name) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc skill remove: {e}");
            return 1;
        }
    };

    if !dir_exists && consumers.is_empty() {
        // Idempotent: nothing to do, exit 0 (per SKILLS.en.md §"Removal" line 314).
        if args.json {
            let value = serde_json::json!({
                "workspace": root.display().to_string(),
                "skill": args.name,
                "removed_dir": false,
                "cleaned_consumers": [],
                "forgot_source": false,
                "note": "not installed",
            });
            return print_json(&value);
        }
        println!("bwoc skill remove: '{}' not installed", args.name);
        return 0;
    }

    if !args.yes {
        if args.json {
            eprintln!(
                "bwoc skill remove: --json requires --yes (destructive op needs explicit ack)"
            );
            return 2;
        }
        eprintln!(
            "bwoc skill remove: refusing to delete without --yes. \
             Would delete:\n  - {} (dir)\nand clean skills.framework[] in: {}",
            target.display(),
            if consumers.is_empty() {
                "(none)".to_string()
            } else {
                consumers.join(", ")
            }
        );
        return 2;
    }

    // Clean consumer manifests first — if dir-delete fails, we have not yet
    // left agents pointing at a half-deleted skill. This is the inverse of
    // install, which writes manifest last after materializing the dir.
    let mut cleaned: Vec<serde_json::Value> = Vec::with_capacity(consumers.len());
    for id in &consumers {
        let agent_dir = root.join("agents").join(id);
        let mut manifest = match load_agent_manifest(&agent_dir) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("bwoc skill remove: {id}: {e}");
                return 1;
            }
        };
        let removed = remove_skill_from_manifest(&mut manifest, &args.name);
        if removed > 0 {
            if let Err(e) = save_agent_manifest(&agent_dir, &manifest) {
                eprintln!("bwoc skill remove: {id}: {e}");
                return 1;
            }
        }
        cleaned.push(serde_json::json!({ "agent": id, "entries_removed": removed }));
    }

    let mut removed_dir = false;
    if dir_exists {
        if let Err(e) = std::fs::remove_dir_all(&target) {
            eprintln!("bwoc skill remove: remove {}: {e}", target.display());
            return 1;
        }
        removed_dir = true;
    }

    let mut forgot = false;
    if args.forget_source {
        match forget_installed_source(&root, &args.name) {
            Ok(b) => forgot = b,
            Err(e) => {
                eprintln!("bwoc skill remove: --forget-source: {e}");
                return 1;
            }
        }
    }

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "skill": args.name,
            "removed_dir": removed_dir,
            "cleaned_consumers": cleaned,
            "forgot_source": forgot,
        });
        return print_json(&value);
    }

    println!("Removed framework skill '{}'", args.name);
    if removed_dir {
        println!("  Deleted:     {}", target.display());
    }
    for c in &consumers {
        println!("  Cleaned:     agents/{c}/config.manifest.json");
    }
    if forgot {
        println!("  Forgotten:   .bwoc/installed-sources.toml entry");
    }
    0
}

// ===========================================================================
// Unit tests ================================================================
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_replaces_all_four_placeholders() {
        let body =
            "name={{skillName}} v={{skillVersion}} d={{skillDescription}} op={{skillOperation}}";
        let out = substitute_placeholders(body, "alpha", "0.2.0", "desc", "do_x");
        assert_eq!(out, "name=alpha v=0.2.0 d=desc op=do_x");
    }

    #[test]
    fn substitute_leaves_unknown_placeholders_alone() {
        let body = "{{skillName}} {{unknown}}";
        let out = substitute_placeholders(body, "alpha", "0.1.0", "d", "o");
        assert_eq!(out, "alpha {{unknown}}");
    }

    #[test]
    fn validate_name_accepts_kebab() {
        assert!(validate_skill_name("worktree-discipline").is_ok());
        assert!(validate_skill_name("a").is_ok());
        assert!(validate_skill_name("a1-b2").is_ok());
    }

    #[test]
    fn validate_name_rejects_bad() {
        for bad in [
            "",
            "Cap",
            "trailing-",
            "-leading",
            "double--dash",
            "with/slash",
            "with space",
            ".",
            "..",
        ] {
            assert!(
                validate_skill_name(bad).is_err(),
                "expected '{bad}' to fail"
            );
        }
    }

    #[test]
    fn detect_local_path_kinds() {
        assert!(matches!(
            detect_source_kind("./foo").unwrap(),
            SourceKind::LocalPath(_)
        ));
        assert!(matches!(
            detect_source_kind("../foo").unwrap(),
            SourceKind::LocalPath(_)
        ));
        assert!(matches!(
            detect_source_kind("/abs/path").unwrap(),
            SourceKind::LocalPath(_)
        ));
    }

    #[test]
    fn detect_git_url_with_ref() {
        let k = detect_source_kind("https://github.com/org/skill.git#v0.1.0").unwrap();
        match k {
            SourceKind::GitUrl(u, Some(r)) => {
                assert_eq!(u, "https://github.com/org/skill.git");
                assert_eq!(r, "v0.1.0");
            }
            _ => panic!("expected GitUrl with ref"),
        }
    }

    #[test]
    fn detect_git_url_without_ref() {
        let k = detect_source_kind("https://github.com/org/skill.git").unwrap();
        match k {
            SourceKind::GitUrl(u, None) => {
                assert_eq!(u, "https://github.com/org/skill.git")
            }
            _ => panic!("expected GitUrl without ref"),
        }
    }

    #[test]
    fn detect_tarball_url() {
        let k = detect_source_kind("https://example.com/x.tar.gz").unwrap();
        assert!(matches!(k, SourceKind::TarballUrl(_)));
        let k = detect_source_kind("https://example.com/x.tgz").unwrap();
        assert!(matches!(k, SourceKind::TarballUrl(_)));
    }

    #[test]
    fn detect_rejects_unknown() {
        assert!(detect_source_kind("nonsense").is_err());
        assert!(detect_source_kind("https://example.com/file.zip").is_err());
    }

    #[test]
    fn iso8601_format_shape() {
        let s = current_utc_iso8601();
        assert_eq!(s.len(), 20, "{s}");
        assert!(s.ends_with('Z'));
        assert_eq!(s.chars().nth(4), Some('-'));
        assert_eq!(s.chars().nth(7), Some('-'));
        assert_eq!(s.chars().nth(10), Some('T'));
    }

    #[test]
    fn manifest_mutation_enable_adds_entry() {
        let mut m = serde_json::json!({});
        let (added, _) =
            set_skill_enabled_in_manifest(&mut m, "wd", ">=0.1.0", true, false).unwrap();
        assert!(added);
        assert_eq!(m["skills"]["framework"][0]["name"], "wd");
        assert_eq!(m["skills"]["framework"][0]["enabled"], true);
    }

    #[test]
    fn manifest_mutation_disable_requires_existing() {
        let mut m = serde_json::json!({});
        let err = set_skill_enabled_in_manifest(&mut m, "wd", ">=0.1.0", false, true).unwrap_err();
        assert!(err.contains("no skills.framework[] entry"));
    }

    #[test]
    fn manifest_mutation_flip_existing() {
        let mut m = serde_json::json!({
            "skills": { "framework": [
                { "name": "wd", "version": ">=0.1.0", "enabled": true }
            ]}
        });
        let (added, _) =
            set_skill_enabled_in_manifest(&mut m, "wd", ">=0.1.0", false, true).unwrap();
        assert!(!added);
        assert_eq!(m["skills"]["framework"][0]["enabled"], false);
    }

    #[test]
    fn manifest_remove_drops_entry() {
        let mut m = serde_json::json!({
            "skills": { "framework": [
                { "name": "wd", "enabled": true },
                { "name": "other", "enabled": false }
            ]}
        });
        let n = remove_skill_from_manifest(&mut m, "wd");
        assert_eq!(n, 1);
        assert_eq!(m["skills"]["framework"].as_array().unwrap().len(), 1);
        assert_eq!(m["skills"]["framework"][0]["name"], "other");
    }

    #[test]
    fn sha256_deterministic() {
        let a = sha256_string("hello");
        let b = sha256_string("hello");
        let c = sha256_string("hello!");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn read_expected_digest_accepts_bare_and_shasum_format() {
        let tmp = std::env::temp_dir().join("bwoc-skill-test-digest");
        let _ = std::fs::create_dir_all(&tmp);
        let p = tmp.join("d.sha256");
        std::fs::write(
            &p,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  some-file\n",
        )
        .unwrap();
        let got = read_expected_digest(&p).unwrap();
        assert_eq!(
            got,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
        let _ = std::fs::remove_file(&p);
    }

    // BWOC-39: argument-injection defense for git refs.
    #[test]
    fn detect_rejects_dash_leading_git_ref() {
        let err =
            detect_source_kind("https://github.com/org/skill.git#--upload-pack=evil").unwrap_err();
        assert!(err.contains("must not begin with '-'"), "{err}");
        // A bare '-' ref is rejected too.
        assert!(detect_source_kind("https://github.com/org/skill.git#-x").is_err());
    }

    #[test]
    fn detect_accepts_normal_git_ref() {
        // Regression guard: ordinary refs still parse after the BWOC-39 guard.
        let k = detect_source_kind("https://github.com/org/skill.git#v1.2.3").unwrap();
        assert!(matches!(k, SourceKind::GitUrl(_, Some(r)) if r == "v1.2.3"));
    }

    // BWOC-37: `[gates].verify` comes from an untrusted manifest. Default path
    // must NOT execute it; --run-gates opts in.
    fn write_probe_skill_with_gate(root: &Path, gate: &str) {
        let dir = root.join("modules/skills/probe");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = format!(
            "[skill]\nname = \"probe\"\nversion = \"0.1.0\"\n\
             description = \"probe\"\nmaturity = \"stable\"\n\n\
             [gates]\nverify = \"{gate}\"\n"
        );
        std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    }

    fn verify_args(root: &Path, run_gates: bool) -> VerifyArgs {
        VerifyArgs {
            common: CommonArgs {
                workspace: Some(root.to_path_buf()),
            },
            name: Some("probe".to_string()),
            all: false,
            run_gates,
            json: false,
        }
    }

    #[test]
    fn verify_does_not_exec_gate_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Gate writes a sentinel iff it runs. Command runs with cwd == root.
        write_probe_skill_with_gate(root, "touch GATE_RAN");

        let code = run_verify(verify_args(root, false));
        assert_eq!(
            code, 0,
            "default verify should succeed without running gate"
        );
        assert!(
            !root.join("GATE_RAN").exists(),
            "gate executed despite --run-gates being off"
        );
    }

    #[test]
    fn verify_execs_gate_with_run_gates_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_probe_skill_with_gate(root, "touch GATE_RAN");

        let code = run_verify(verify_args(root, true));
        assert_eq!(code, 0, "gate `touch` exits 0");
        assert!(
            root.join("GATE_RAN").exists(),
            "gate did not execute with --run-gates"
        );
    }

    #[test]
    fn verify_run_gates_propagates_gate_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_probe_skill_with_gate(root, "exit 7");
        // With the flag, a failing gate makes verify non-zero.
        assert_ne!(run_verify(verify_args(root, true)), 0);
        // Without the flag, the failing gate never runs → success.
        assert_eq!(run_verify(verify_args(root, false)), 0);
    }

    // ---- BWOC-45: skill-on-plugin dependency resolution --------------------

    /// Write a skill that declares `requires_plugins = ["jira"]` and no gate,
    /// so verify's result is driven purely by dependency resolution.
    fn write_jira_dep_skill(root: &Path) {
        let dir = root.join("modules/skills/scrum-via-jira");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            "[skill]\nname = \"scrum-via-jira\"\nversion = \"0.1.0\"\n\
             description = \"scrum via a jira-kind plugin\"\nmaturity = \"L1\"\n\n\
             [contract]\nrequires = []\nrequires_plugins = [\"jira\"]\n\
             exposes = [\"propose-sprint\"]\n",
        )
        .unwrap();
    }

    /// Install a jira-kind plugin AND enable it in `workspace.toml`, so the
    /// `requires_plugins = ["jira"]` dependency resolves.
    fn install_and_enable_jira_plugin(root: &Path) {
        let dir = root.join("modules/plugins/jira-cloud-rest");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            "[plugin]\nname = \"jira-cloud-rest\"\nkind = \"jira\"\nversion = \"0.1.0\"\n\
             description = \"jira adapter\"\ncompat = \">=2.7.0\"\nentry = \"jira.sh\"\n",
        )
        .unwrap();
        let bwoc = root.join(".bwoc");
        std::fs::create_dir_all(&bwoc).unwrap();
        std::fs::write(
            bwoc.join("workspace.toml"),
            "[plugins.jira-cloud-rest]\nenabled = true\n",
        )
        .unwrap();
    }

    fn jira_dep_verify_args(root: &Path) -> VerifyArgs {
        VerifyArgs {
            common: CommonArgs {
                workspace: Some(root.to_path_buf()),
            },
            name: Some("scrum-via-jira".to_string()),
            all: false,
            run_gates: false,
            json: false,
        }
    }

    #[test]
    fn verify_resolves_satisfied_plugin_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_jira_dep_skill(root);
        install_and_enable_jira_plugin(root);
        assert_eq!(
            run_verify(jira_dep_verify_args(root)),
            0,
            "an enabled jira-kind plugin should satisfy requires_plugins = [\"jira\"]"
        );
    }

    #[test]
    fn verify_fails_on_missing_plugin_dependency() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_jira_dep_skill(root);
        // No jira-kind plugin installed/enabled → the dependency is unmet.
        assert_ne!(
            run_verify(jira_dep_verify_args(root)),
            0,
            "an unmet plugin dependency must fail verify"
        );
    }

    #[test]
    fn verify_fails_when_required_plugin_installed_but_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_jira_dep_skill(root);
        install_and_enable_jira_plugin(root);
        // Flip the plugin off — installed is not enough; spawn needs ENABLED.
        std::fs::write(
            root.join(".bwoc/workspace.toml"),
            "[plugins.jira-cloud-rest]\nenabled = false\n",
        )
        .unwrap();
        assert_ne!(
            run_verify(jira_dep_verify_args(root)),
            0,
            "a disabled plugin must not satisfy the dependency"
        );
    }
}
