//! `bwoc okr <verb>` — operator-facing CLI surface for the `okr` plugin kind
//! (BWOC-48). Foundation of `BWOC-EPIC-4` (OKR Progress).
//!
//! ## What this is
//!
//! The CLI half of the contract framed in
//! `notes/2026-05-28_okr-plugin-architecture.md` (BWOC-46) and made normative by
//! the **OKR Progress Schema** in `docs/en/PLUGINS.en.md` (BWOC-47). It owns
//! **argument parsing, workspace + plugin resolution, and the JSON shapes** — it
//! does NOT author objectives or compute progress itself. The objectives /
//! key-results live in operator-authored TOML inside the plugin, and the
//! `track` / `report` verbs are implemented by an `okr`-kind plugin
//! (the `workspace-okrs` reference plugin, BWOC-49, in flight). This CLI
//! discovers an installed `okr` plugin by name and invokes its `[plugin].entry`;
//! when the named plugin is absent the verbs **stub-error gracefully** (exit `4`)
//! rather than panicking.
//!
//! ## Verb table
//!
//! | Verb                                            | Needs plugin | Notes                                                       |
//! |---|---|---|
//! | `list`                                          | no           | Enumerate installed `okr`-kind plugins (enabled + disabled). |
//! | `show <plugin>`                                 | yes          | Print the plugin `SPEC.md` + an objectives summary. Degrades. |
//! | `track <plugin> --key-result <id> --current <v>`| yes          | Record a key-result's `current` value. **Local-file write — no confirm gate** (the operator's own TOML, fully reversible; BWOC-46 §3). |
//! | `report <plugin>`                               | yes          | Emit the OKR Progress Schema JSON for every key result.     |
//!
//! ## Why `track` has no confirmation gate
//!
//! Unlike `bwoc jira` / `bwoc gcloud` write verbs (which mutate an external
//! system of record), `okr track` writes the operator's own `key_results.toml`
//! — a local, git-tracked file the operator can `git diff` and revert. Per the
//! BWOC-46 design note (decision 3) a confirmation gate here would be ceremony
//! without risk, so there is none.
//!
//! ## Named-plugin invocation, not enabled-gated
//!
//! `show` / `track` / `report` each name a specific plugin, mirroring
//! `bwoc audit run --plugin <name>` (which runs regardless of the workspace
//! `enabled` flag). The gate is therefore **installed**, not **enabled** — an
//! explicitly named plugin runs if it is present on disk with `kind = "okr"`.
//! `bwoc okr list` surfaces the `enabled` flag separately for visibility.
//!
//! ## Exit codes — normative
//!
//! - `0` — success.
//! - `1` — local I/O error (e.g. JSON serialization).
//! - `2` — operator/usage error (no workspace).
//! - `4` — the named `okr` plugin is not installed in this workspace
//!   (remediation message names it).
//! - `255` — plugin runtime error (spawn failure or non-JSON output).
//!
//! Passing `--json` makes the exit code redundant: the structured envelope
//! carries `ok`/`error` fields with the same signal.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Exit codes + plugin kind + env var (single source of truth).
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_PLUGIN: i32 = 4;
const EXIT_PLUGIN_ERROR: i32 = 255;

const PLUGIN_KIND: &str = "okr";

// ---------------------------------------------------------------------------
// CLI surface — defined here so arg parsing is unit-testable against
// `OkrCommand` directly (see `tests` module).
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum OkrCommand {
    /// List installed `okr`-kind plugins (enabled + disabled).
    List(ListArgs),
    /// Print a plugin's `SPEC.md` plus an objectives summary.
    Show(ShowArgs),
    /// Record a key-result's current value (writes the operator's local TOML).
    Track(TrackArgs),
    /// Emit the OKR Progress Schema JSON for every key result.
    Report(ReportArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Show only plugins enabled in `workspace.toml [plugins.<name>]`.
    #[arg(long)]
    enabled: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct TrackArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// The key-result id to update (must exist in the plugin's `key_results.toml`).
    #[arg(long = "key-result")]
    key_result: String,
    /// The new `current` value. Over-attainment (`current > target`) is preserved.
    #[arg(long)]
    current: f64,
    /// Optional evidence reference (a file path, command, attestation, …) the
    /// plugin records alongside the value. Reuses the audit evidence model.
    #[arg(long)]
    evidence: Option<String>,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the OKR Progress Schema JSON instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

/// Dispatch a parsed `OkrCommand`. Returns the process exit code.
pub fn run(cmd: OkrCommand) -> i32 {
    match cmd {
        OkrCommand::List(a) => run_list(a),
        OkrCommand::Show(a) => run_show(a),
        OkrCommand::Track(a) => run_track(a),
        OkrCommand::Report(a) => run_report(a),
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution — same shape as gcloud.rs / jira.rs / audit.rs.
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

fn resolve_workspace(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    find_workspace_root(explicit).ok_or_else(|| {
        "no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
         Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
            .to_string()
    })
}

// ---------------------------------------------------------------------------
// Plugin discovery — finds `okr`-kind plugins by name + kind. Checks both the
// flat layout (`modules/plugins/<name>/`) and the kind-namespaced layout
// (`modules/plugins/okr/<name>/`) so the CLI works regardless of which layout
// BWOC-49 ships with (mirrors gcloud.rs).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct ManifestRaw {
    plugin: PluginSection,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PluginSection {
    name: String,
    kind: String,
    version: String,
    description: String,
    entry: String,
}

#[derive(Debug, Clone, PartialEq)]
struct OkrPlugin {
    name: String,
    dir: PathBuf,
    entry: String,
    version: String,
    description: String,
}

/// Read `.bwoc/workspace.toml [plugins.<name>] enabled` flags. Missing table or
/// missing `enabled` defaults to `false` — for `list` visibility we never reject
/// here; `bwoc check` owns strict validation of the `enabled` field.
fn workspace_enabled_set(root: &Path) -> Result<BTreeMap<String, bool>, String> {
    let path = root.join(".bwoc/workspace.toml");
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&body).map_err(|e| format!("{}: parse: {e}", path.display()))?;
    let mut out = BTreeMap::new();
    let Some(plugins) = value.get("plugins").and_then(|v| v.as_table()) else {
        return Ok(out);
    };
    for (name, entry) in plugins {
        let enabled = entry
            .as_table()
            .and_then(|t| t.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        out.insert(name.clone(), enabled);
    }
    Ok(out)
}

/// Try the two known plugin layouts in order — flat, then `okr/`-namespaced.
fn candidate_plugin_dirs(root: &Path, name: &str) -> [PathBuf; 2] {
    [
        root.join("modules/plugins").join(name),
        root.join("modules/plugins/okr").join(name),
    ]
}

fn parse_plugin(manifest: &Path, plugin_dir: &Path) -> Result<OkrPlugin, String> {
    let body = std::fs::read_to_string(manifest)
        .map_err(|e| format!("read {}: {e}", manifest.display()))?;
    let parsed: ManifestRaw =
        toml::from_str(&body).map_err(|e| format!("parse {}: {e}", manifest.display()))?;
    Ok(OkrPlugin {
        name: parsed.plugin.name,
        dir: plugin_dir.to_path_buf(),
        entry: parsed.plugin.entry,
        version: parsed.plugin.version,
        description: parsed.plugin.description,
    })
}

/// Find an `okr`-kind plugin by name across both layouts. Returns `None` when no
/// manifest matches; returns `Err` on parse failure or a kind mismatch (the
/// plugin *exists* but is malformed/misconfigured — surface, don't degrade).
fn discover_plugin(root: &Path, name: &str) -> Result<Option<OkrPlugin>, String> {
    for plugin_dir in candidate_plugin_dirs(root, name) {
        let manifest = plugin_dir.join("manifest.toml");
        if !manifest.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("read {}: {e}", manifest.display()))?;
        let parsed: ManifestRaw =
            toml::from_str(&body).map_err(|e| format!("parse {}: {e}", manifest.display()))?;
        if parsed.plugin.name != name {
            // Wrong manifest at this path — keep looking.
            continue;
        }
        if parsed.plugin.kind != PLUGIN_KIND {
            // Right name, wrong kind. Surface — this is a misconfiguration.
            return Err(format!(
                "{}: [plugin].kind = {:?}, expected {:?}",
                manifest.display(),
                parsed.plugin.kind,
                PLUGIN_KIND
            ));
        }
        return Ok(Some(OkrPlugin {
            name: parsed.plugin.name,
            dir: plugin_dir,
            entry: parsed.plugin.entry,
            version: parsed.plugin.version,
            description: parsed.plugin.description,
        }));
    }
    Ok(None)
}

/// Enumerate every installed `okr`-kind plugin across both layouts, sorted by
/// name. A directory whose manifest fails to parse is surfaced as `Err` (mirror
/// of `audit::discover_audit`); a directory with a non-`okr` kind is skipped.
fn discover_all(root: &Path) -> Result<Vec<OkrPlugin>, String> {
    let mut found: BTreeMap<String, OkrPlugin> = BTreeMap::new();
    // Flat layout first, then the namespaced one. Flat takes precedence on a
    // name clash (it is the canonical install location).
    for base in [
        root.join("modules/plugins"),
        root.join("modules/plugins/okr"),
    ] {
        if !base.is_dir() {
            continue;
        }
        let mut entries: Vec<_> = std::fs::read_dir(&base)
            .map_err(|e| format!("read {}: {e}", base.display()))?
            .filter_map(|r| r.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let plugin_dir = entry.path();
            let manifest = plugin_dir.join("manifest.toml");
            if !manifest.is_file() {
                continue;
            }
            let plugin = parse_plugin(&manifest, &plugin_dir)?;
            // Re-read kind: parse_plugin keeps only the fields we surface, so
            // probe kind directly from the parsed manifest body.
            let kind = std::fs::read_to_string(&manifest)
                .ok()
                .and_then(|b| toml::from_str::<ManifestRaw>(&b).ok())
                .map(|m| m.plugin.kind)
                .unwrap_or_default();
            if kind != PLUGIN_KIND {
                continue;
            }
            found.entry(plugin.name.clone()).or_insert(plugin);
        }
    }
    Ok(found.into_values().collect())
}

/// Resolve a named plugin for a verb invocation. The gate is **installed**, not
/// **enabled** — naming a plugin explicitly is the intent (mirrors
/// `bwoc audit run --plugin`). Maps the absence path to a clean exit `4`.
fn require_plugin(root: &Path, name: &str, verb: &str, json: bool) -> Result<OkrPlugin, i32> {
    match discover_plugin(root, name) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => {
            let msg = no_plugin_message(name);
            if json {
                emit_error_json(verb, "no_plugin", &msg);
            } else {
                eprintln!("bwoc okr {verb}: {msg}");
            }
            Err(EXIT_NO_PLUGIN)
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "discovery_error", &e);
            } else {
                eprintln!("bwoc okr {verb}: {e}");
            }
            Err(EXIT_PLUGIN_ERROR)
        }
    }
}

fn resolve_entry_program(plugin_dir: &Path, entry: &str) -> OsString {
    let candidate = plugin_dir.join(entry);
    if candidate.is_file() {
        candidate.into_os_string()
    } else {
        OsString::from(entry)
    }
}

// ---------------------------------------------------------------------------
// Plugin invocation — same shape as gcloud.rs::invoke_plugin / audit.rs.
// ---------------------------------------------------------------------------

fn invoke_plugin(
    plugin: &OkrPlugin,
    workspace: &Path,
    request: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    // BWOC-36: guard against path-traversal RCE before spawning the entry.
    crate::util::validate_plugin_entry(&plugin.entry)?;
    let program = resolve_entry_program(&plugin.dir, &plugin.entry);
    let operation = request
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut child = Command::new(&program)
        .current_dir(&plugin.dir)
        .env("BWOC_WORKSPACE", workspace)
        .env("BWOC_PLUGIN_DIR", &plugin.dir)
        .env("BWOC_OKR_OPERATION", operation)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn '{}': {e}", program.to_string_lossy()))?;

    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "{request}");
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .map_err(|e| format!("wait '{}': {e}", program.to_string_lossy()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "plugin '{}' exited {} (stderr: {})",
            plugin.name,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| format!("plugin '{}' did not emit valid JSON: {e}", plugin.name))
}

// ---------------------------------------------------------------------------
// Request payloads handed to the plugin over stdin (one per verb).
// ---------------------------------------------------------------------------

fn report_request(workspace: &Path, plugin_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "operation": "report",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
    })
}

fn track_request(
    workspace: &Path,
    plugin_dir: &Path,
    key_result_id: &str,
    current: f64,
    evidence: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "track",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "key_result_id": key_result_id,
        "current": current,
        "evidence": evidence,
    })
}

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

fn print_json(value: &serde_json::Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(e) => {
            eprintln!("bwoc okr: serialize JSON: {e}");
            false
        }
    }
}

fn emit_error_json(verb: &str, code: &str, message: &str) {
    let value = serde_json::json!({
        "ok": false,
        "verb": verb,
        "error": code,
        "message": message,
    });
    print_json(&value);
}

/// Stub-error envelope for the missing-plugin path. Names the exact plugin and
/// the install hint the operator needs.
fn no_plugin_message(plugin_name: &str) -> String {
    format!(
        "no installed '{plugin_name}' plugin (okr kind) in this workspace. \
         The OKR data + verbs are provided by an `okr`-kind plugin such as \
         `workspace-okrs` (see the EPIC-4 design note). Install it (BWOC-49) with \
         `bwoc plugin install <source>` then `bwoc plugin enable {plugin_name}`."
    )
}

/// Per-objective rollup derived from the OKR Progress Schema entries the plugin
/// emits. `mean_attainment` averages `current / target` over the objective's
/// key results, skipping any with `target == 0` (a boolean/never-set KR); `None`
/// when no key result yields a defined ratio.
#[derive(Debug, Clone, Serialize, PartialEq)]
struct ObjectiveSummary {
    objective_id: String,
    key_results: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    mean_attainment: Option<f64>,
}

/// Group OKR Progress Schema entries by `objective_id`, in first-seen order, and
/// roll each objective's key results up to a summary. Pure — unit-tested.
fn summarize_objectives(entries: &[serde_json::Value]) -> Vec<ObjectiveSummary> {
    // Preserve plugin-emit order of objectives by tracking first appearance.
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut ratio_sum: BTreeMap<String, f64> = BTreeMap::new();
    let mut ratio_n: BTreeMap<String, usize> = BTreeMap::new();

    for e in entries {
        let Some(obj) = e.get("objective_id").and_then(|v| v.as_str()) else {
            continue;
        };
        if !counts.contains_key(obj) {
            order.push(obj.to_string());
        }
        *counts.entry(obj.to_string()).or_insert(0) += 1;

        let target = e.get("target").and_then(|v| v.as_f64());
        let current = e.get("current").and_then(|v| v.as_f64());
        if let (Some(t), Some(c)) = (target, current) {
            if t != 0.0 {
                *ratio_sum.entry(obj.to_string()).or_insert(0.0) += c / t;
                *ratio_n.entry(obj.to_string()).or_insert(0) += 1;
            }
        }
    }

    order
        .into_iter()
        .map(|obj| {
            let n = *ratio_n.get(&obj).unwrap_or(&0);
            let mean = if n > 0 {
                Some(ratio_sum.get(&obj).copied().unwrap_or(0.0) / n as f64)
            } else {
                None
            };
            ObjectiveSummary {
                key_results: *counts.get(&obj).unwrap_or(&0),
                objective_id: obj,
                mean_attainment: mean,
            }
        })
        .collect()
}

/// Pull the report entries out of whatever envelope the plugin emits: either a
/// bare JSON array of schema entries, or `{ "entries": [...] }`.
fn entries_of(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = value.as_array() {
        return arr.clone();
    }
    if let Some(arr) = value.get("entries").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Verb implementations.
// ---------------------------------------------------------------------------

fn run_list(args: ListArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc okr list: {e}");
            return EXIT_USAGE;
        }
    };

    let plugins = match discover_all(&root) {
        Ok(p) => p,
        Err(e) => {
            if args.json {
                emit_error_json("list", "discovery_error", &e);
            } else {
                eprintln!("bwoc okr list: {e}");
            }
            return EXIT_PLUGIN_ERROR;
        }
    };
    let enabled = match workspace_enabled_set(&root) {
        Ok(m) => m,
        Err(e) => {
            if args.json {
                emit_error_json("list", "workspace_error", &e);
            } else {
                eprintln!("bwoc okr list: {e}");
            }
            return EXIT_LOCAL_ERROR;
        }
    };

    let rows: Vec<(OkrPlugin, bool)> = plugins
        .into_iter()
        .map(|p| {
            let on = matches!(enabled.get(&p.name), Some(true));
            (p, on)
        })
        .filter(|(_, on)| !args.enabled || *on)
        .collect();

    if args.json {
        let arr: Vec<serde_json::Value> = rows
            .iter()
            .map(|(p, on)| {
                serde_json::json!({
                    "name": p.name,
                    "kind": PLUGIN_KIND,
                    "enabled": on,
                    "version": p.version,
                    "description": p.description,
                })
            })
            .collect();
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "plugins": arr,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }

    println!("bwoc okr list: {} okr plugin(s)", rows.len());
    for (p, on) in &rows {
        let state = if *on { "enabled" } else { "disabled" };
        println!(
            "  {} v{} [{}] — {}",
            p.name, p.version, state, p.description
        );
    }
    EXIT_OK
}

fn run_show(args: ShowArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc okr show: {e}");
            return EXIT_USAGE;
        }
    };
    let plugin = match require_plugin(&root, &args.plugin, "show", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };

    // SPEC.md is required by the plugin directory layout, but `show` is an
    // orientation command — read best-effort so a missing/locked SPEC degrades
    // to a note rather than failing the command.
    let spec = std::fs::read_to_string(plugin.dir.join("SPEC.md")).ok();

    // Objectives summary is derived from the normative `report` output. `show`
    // degrades (like `bwoc gcloud status`): if the plugin can't report yet, we
    // still print the SPEC and note that progress data is unavailable.
    let request = report_request(&root, &plugin.dir);
    let (objectives, objectives_error) = match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => (summarize_objectives(&entries_of(&value)), None),
        Err(e) => (Vec::new(), Some(e)),
    };

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "plugin": plugin.name,
            "version": plugin.version,
            "description": plugin.description,
            "spec": spec,
            "objectives": objectives,
            "objectives_available": objectives_error.is_none(),
            "objectives_error": objectives_error,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }

    println!("bwoc okr show: {} v{}", plugin.name, plugin.version);
    println!("  {}", plugin.description);
    println!("\n--- SPEC.md ---");
    match &spec {
        Some(s) => println!("{}", s.trim_end()),
        None => println!("(SPEC.md not found in {})", plugin.dir.display()),
    }
    println!("\n--- Objectives ---");
    if let Some(e) = &objectives_error {
        println!("  (progress data unavailable: {e})");
    } else if objectives.is_empty() {
        println!("  (no objectives reported)");
    } else {
        for o in &objectives {
            match o.mean_attainment {
                Some(m) => println!(
                    "  {} — {} key result(s), mean attainment {:.0}%",
                    o.objective_id,
                    o.key_results,
                    m * 100.0
                ),
                None => println!(
                    "  {} — {} key result(s), attainment n/a",
                    o.objective_id, o.key_results
                ),
            }
        }
    }
    EXIT_OK
}

fn run_track(args: TrackArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc okr track: {e}");
            return EXIT_USAGE;
        }
    };
    let plugin = match require_plugin(&root, &args.plugin, "track", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    // No confirmation gate: this writes the operator's own local key_results.toml
    // (fully reversible), not an external system of record (BWOC-46 §3).
    let request = track_request(
        &root,
        &plugin.dir,
        &args.key_result,
        args.current,
        args.evidence.as_deref(),
    );
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let kr = value
                    .get("key_result_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&args.key_result);
                let current = value
                    .get("current")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(args.current);
                let target = value.get("target").and_then(|v| v.as_f64());
                let unit = value.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                let confidence = value
                    .get("confidence")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                match target {
                    Some(t) => println!(
                        "bwoc okr track: {kr} current={current} (target={t}, unit={unit}, confidence={confidence})"
                    ),
                    None => println!(
                        "bwoc okr track: {kr} current={current} (unit={unit}, confidence={confidence})"
                    ),
                }
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("track", "plugin_error", &e);
            } else {
                eprintln!("bwoc okr track: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

fn run_report(args: ReportArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc okr report: {e}");
            return EXIT_USAGE;
        }
    };
    let plugin = match require_plugin(&root, &args.plugin, "report", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = report_request(&root, &plugin.dir);
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                // Emit the OKR Progress Schema as the plugin produced it (bare
                // array, or its envelope) — the schema is the contract.
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let entries = entries_of(&value);
                println!(
                    "bwoc okr report: {} — {} key result(s)",
                    plugin.name,
                    entries.len()
                );
                for e in &entries {
                    let obj = e
                        .get("objective_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let kr = e
                        .get("key_result_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let current = e.get("current").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let target = e.get("target").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let unit = e.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                    let confidence = e.get("confidence").and_then(|v| v.as_str()).unwrap_or("?");
                    let as_of = e.get("as_of").and_then(|v| v.as_str()).unwrap_or("never");
                    println!(
                        "  [{obj}] {kr}: {current}/{target} {unit} confidence={confidence} as_of={as_of}"
                    );
                }
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("report", "plugin_error", &e);
            } else {
                eprintln!("bwoc okr report: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

// ===========================================================================
// Tests — arg parsing, discovery (both layouts), enabled-set reading, no-plugin
// stub path, request payload shapes, objectives rollup, entry-envelope parsing.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        cmd: OkrCommand,
    }

    fn parse(args: &[&str]) -> Result<OkrCommand, clap::Error> {
        let mut full = vec!["bwoc-okr-test"];
        full.extend_from_slice(args);
        TestCli::try_parse_from(full).map(|c| c.cmd)
    }

    // --- arg parsing -------------------------------------------------------

    #[test]
    fn parses_list() {
        match parse(&["list", "--enabled", "--json"]).unwrap() {
            OkrCommand::List(a) => {
                assert!(a.enabled);
                assert!(a.json);
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn parses_show() {
        match parse(&["show", "workspace-okrs", "--json"]).unwrap() {
            OkrCommand::Show(a) => {
                assert_eq!(a.plugin, "workspace-okrs");
                assert!(a.json);
            }
            other => panic!("expected Show, got {other:?}"),
        }
    }

    #[test]
    fn parses_track_full() {
        match parse(&[
            "track",
            "workspace-okrs",
            "--key-result",
            "O1-KR1",
            "--current",
            "0.8",
            "--evidence",
            "docs/en/PLUGINS.en.md",
            "--json",
        ])
        .unwrap()
        {
            OkrCommand::Track(a) => {
                assert_eq!(a.plugin, "workspace-okrs");
                assert_eq!(a.key_result, "O1-KR1");
                assert!((a.current - 0.8).abs() < f64::EPSILON);
                assert_eq!(a.evidence.as_deref(), Some("docs/en/PLUGINS.en.md"));
                assert!(a.json);
            }
            other => panic!("expected Track, got {other:?}"),
        }
    }

    #[test]
    fn track_requires_key_result_and_current() {
        assert!(parse(&["track", "workspace-okrs"]).is_err());
        assert!(parse(&["track", "workspace-okrs", "--key-result", "O1-KR1"]).is_err());
        assert!(parse(&["track", "workspace-okrs", "--current", "1"]).is_err());
    }

    #[test]
    fn track_rejects_non_numeric_current() {
        assert!(parse(&["track", "p", "--key-result", "k", "--current", "lots"]).is_err());
    }

    #[test]
    fn parses_report() {
        match parse(&["report", "workspace-okrs"]).unwrap() {
            OkrCommand::Report(a) => {
                assert_eq!(a.plugin, "workspace-okrs");
                assert!(!a.json);
            }
            other => panic!("expected Report, got {other:?}"),
        }
    }

    #[test]
    fn show_and_report_require_plugin_name() {
        assert!(parse(&["show"]).is_err());
        assert!(parse(&["report"]).is_err());
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(parse(&["frobnicate"]).is_err());
    }

    // --- plugin discovery (both layouts) -----------------------------------

    fn write_workspace(root: &Path, workspace_toml: &str) {
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(root.join(".bwoc/workspace.toml"), workspace_toml).unwrap();
    }

    fn write_plugin_at(root: &Path, layout: &str, name: &str, kind: &str) {
        let dir = root.join("modules/plugins").join(layout).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "[plugin]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
                 description = \"track the things\"\ncompat = \">=2.5.0\"\nentry = \"okr.sh\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn no_plugins_dir_discovers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            discover_plugin(dir.path(), "workspace-okrs")
                .unwrap()
                .is_none()
        );
        assert!(discover_all(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn discovers_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", "workspace-okrs", "okr");
        let p = discover_plugin(dir.path(), "workspace-okrs")
            .unwrap()
            .unwrap();
        assert_eq!(p.name, "workspace-okrs");
        assert_eq!(p.entry, "okr.sh");
        assert_eq!(p.version, "0.1.0");
    }

    #[test]
    fn discovers_okr_namespaced_layout() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "okr", "workspace-okrs", "okr");
        let p = discover_plugin(dir.path(), "workspace-okrs")
            .unwrap()
            .unwrap();
        assert_eq!(p.name, "workspace-okrs");
    }

    #[test]
    fn discovery_rejects_wrong_kind() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", "workspace-okrs", "audit");
        let err = discover_plugin(dir.path(), "workspace-okrs").unwrap_err();
        assert!(err.contains("expected"), "{err}");
        assert!(err.contains("okr"), "{err}");
    }

    #[test]
    fn discover_all_lists_only_okr_kind_sorted() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", "zeta-okrs", "okr");
        write_plugin_at(dir.path(), "", "alpha-okrs", "okr");
        write_plugin_at(dir.path(), "", "some-audit", "audit");
        let all = discover_all(dir.path()).unwrap();
        let names: Vec<_> = all.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha-okrs", "zeta-okrs"]);
    }

    #[test]
    fn discover_all_dedupes_flat_over_namespaced() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", "workspace-okrs", "okr");
        write_plugin_at(dir.path(), "okr", "workspace-okrs", "okr");
        let all = discover_all(dir.path()).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "workspace-okrs");
    }

    #[test]
    fn enabled_set_reads_flags() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(
            dir.path(),
            "[plugins.workspace-okrs]\nenabled = true\n[plugins.other]\nenabled = false\n",
        );
        let set = workspace_enabled_set(dir.path()).unwrap();
        assert_eq!(set.get("workspace-okrs"), Some(&true));
        assert_eq!(set.get("other"), Some(&false));
    }

    #[test]
    fn no_plugin_message_names_install_command() {
        let m = no_plugin_message("workspace-okrs");
        assert!(m.contains("workspace-okrs"));
        assert!(m.contains("bwoc plugin install"));
        assert!(m.contains("bwoc plugin enable"));
    }

    // --- request payload shapes (what the CLI hands the plugin) ------------

    #[test]
    fn report_request_shape() {
        let v = report_request(Path::new("/ws"), Path::new("/p"));
        assert_eq!(v["operation"], "report");
        assert_eq!(v["workspace"], "/ws");
        assert_eq!(v["plugin_dir"], "/p");
    }

    #[test]
    fn track_request_carries_evidence_when_set() {
        let v = track_request(
            Path::new("/ws"),
            Path::new("/p"),
            "O1-KR1",
            0.8,
            Some("docs/en/PLUGINS.en.md"),
        );
        assert_eq!(v["operation"], "track");
        assert_eq!(v["key_result_id"], "O1-KR1");
        assert_eq!(v["current"], 0.8);
        assert_eq!(v["evidence"], "docs/en/PLUGINS.en.md");
    }

    #[test]
    fn track_request_null_evidence_when_absent() {
        let v = track_request(Path::new("/ws"), Path::new("/p"), "O1-KR1", 1.0, None);
        assert!(v["evidence"].is_null());
        assert_eq!(v["current"], 1.0);
    }

    // --- objectives rollup -------------------------------------------------

    #[test]
    fn summarize_groups_and_means_attainment() {
        let entries = vec![
            serde_json::json!({ "objective_id": "O1", "key_result_id": "O1-KR1", "target": 2.0, "current": 1.0 }),
            serde_json::json!({ "objective_id": "O1", "key_result_id": "O1-KR2", "target": 4.0, "current": 4.0 }),
            serde_json::json!({ "objective_id": "O2", "key_result_id": "O2-KR1", "target": 10.0, "current": 5.0 }),
        ];
        let s = summarize_objectives(&entries);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].objective_id, "O1");
        assert_eq!(s[0].key_results, 2);
        // (0.5 + 1.0) / 2 = 0.75
        assert!((s[0].mean_attainment.unwrap() - 0.75).abs() < 1e-9);
        assert_eq!(s[1].objective_id, "O2");
        assert!((s[1].mean_attainment.unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn summarize_skips_zero_target_for_mean() {
        let entries = vec![
            serde_json::json!({ "objective_id": "O1", "key_result_id": "O1-KR1", "target": 0.0, "current": 1.0 }),
        ];
        let s = summarize_objectives(&entries);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].key_results, 1);
        assert_eq!(s[0].mean_attainment, None);
    }

    #[test]
    fn summarize_preserves_first_seen_order() {
        let entries = vec![
            serde_json::json!({ "objective_id": "O2", "key_result_id": "O2-KR1", "target": 1.0, "current": 1.0 }),
            serde_json::json!({ "objective_id": "O1", "key_result_id": "O1-KR1", "target": 1.0, "current": 1.0 }),
        ];
        let s = summarize_objectives(&entries);
        assert_eq!(s[0].objective_id, "O2");
        assert_eq!(s[1].objective_id, "O1");
    }

    #[test]
    fn objective_summary_omits_none_mean_in_json() {
        let s = ObjectiveSummary {
            objective_id: "O1".to_string(),
            key_results: 1,
            mean_attainment: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("mean_attainment"), "{json}");
    }

    // --- entry-envelope parsing -------------------------------------------

    #[test]
    fn entries_of_bare_array() {
        let v = serde_json::json!([{ "key_result_id": "O1-KR1" }]);
        assert_eq!(entries_of(&v).len(), 1);
    }

    #[test]
    fn entries_of_wrapped_envelope() {
        let v = serde_json::json!({ "entries": [{ "key_result_id": "O1-KR1" }, { "key_result_id": "O1-KR2" }] });
        assert_eq!(entries_of(&v).len(), 2);
    }

    #[test]
    fn entries_of_unrecognized_is_empty() {
        let v = serde_json::json!({ "nope": true });
        assert!(entries_of(&v).is_empty());
    }
}
