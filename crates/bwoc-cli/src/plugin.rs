//! `bwoc plugin list / show / init / install / enable / disable / remove`
//! — full framework-plugin surface.
//!
//! Read-side (BWOC-5): `list`, `show` follow `docs/en/PLUGINS.en.md`
//! §"CLI Surface". Plugins live under `<workspace>/modules/plugins/<name>/`,
//! each with a `manifest.toml` (schema §"Manifest"). Discovery is
//! workspace-local — no network calls — and per-workspace opt-in is gated on
//! `workspace.toml` `[plugins.<name>]` tables (§"Loading").
//!
//! Write-side (BWOC-24): `init` scaffolds from `modules/plugin-template/`
//! with the `--kind <k>` flag substituted into `{{pluginKind}}`; `install`
//! materializes from local path / git URL / tarball URL with a SHA-256 trust
//! gate (`--no-verify` / `--allow-new-source` per §"Sources & Installation");
//! `enable`/`disable` flip the `enabled` field in `workspace.toml`
//! `[plugins.<name>]` — per-workspace scope, **not** per-agent (the
//! contract difference vs skills, see §"Loading" line 267); `remove` deletes
//! `modules/plugins/<name>/` and removes the `[plugins.<name>]` table
//! entirely from `workspace.toml` (§"Removal" line 419).
//!
//! There is no `verify` subcommand in v1 — PLUGINS.en.md §"CLI Surface"
//! line 314 calls it out as a deliberate omission (the four plugin kinds
//! diverge too much to share a uniform gate).
//!
//! Every read AND write command has a `--json` twin. Human output is
//! intentionally terse — JSON is the contract for scripts.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Top-level arguments shared by every `bwoc plugin …` subcommand.
#[derive(Debug, Clone)]
pub struct CommonArgs {
    /// Workspace root override. Resolution: `--workspace` > `BWOC_WORKSPACE`
    /// env > ancestor-walk for `.bwoc/workspace.toml` > error.
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ListArgs {
    pub common: CommonArgs,
    /// Filter to plugins enabled in `workspace.toml [plugins.<name>]`.
    pub enabled: bool,
    /// Filter to one plugin kind (`memory-backend`, `llm-backend`, `workflow`, `audit`).
    pub kind: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct ShowArgs {
    pub common: CommonArgs,
    pub name: String,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct InitArgs {
    pub common: CommonArgs,
    pub name: String,
    /// Required — one of: `memory-backend`, `llm-backend`, `workflow`, `audit`.
    /// PLUGINS.en.md §"Scaffolding from template" line 398 mandates this:
    /// "no default … forces the operator to declare intent up front."
    pub kind: String,
    /// Override `{{pluginVersion}}`. Default `0.1.0`.
    pub version: Option<String>,
    /// Override `{{pluginDescription}}`. Default a hint placeholder.
    pub description: Option<String>,
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
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct DisableArgs {
    pub common: CommonArgs,
    pub name: String,
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
// Manifest schema (mirror of PLUGINS.en.md §"Manifest", lines 144–173).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ManifestRaw {
    plugin: PluginSection,
}

#[derive(Debug, Clone, Deserialize)]
struct PluginSection {
    name: String,
    kind: String,
    version: String,
    description: String,
    compat: String,
    entry: String,
}

/// One discovered plugin — manifest contents + filesystem location.
#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    dir_name: String,
    path: PathBuf,
    manifest: ManifestRaw,
    spec_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Workspace resolution (mirror of skill.rs:106).
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

fn plugins_dir(root: &Path) -> PathBuf {
    root.join("modules/plugins")
}

/// Walk `<root>/modules/plugins/*/manifest.toml`, parse each, and return
/// them sorted by directory name. Dirs without a `manifest.toml` are skipped
/// — `bwoc check` is the authoritative validator (PLUGINS.en.md §"Verification").
fn discover(root: &Path) -> Result<Vec<DiscoveredPlugin>, String> {
    let dir = plugins_dir(root);
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
        let plugin_dir = entry.path();
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        let manifest_path = plugin_dir.join("manifest.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = parse_manifest(&manifest_path)
            .map_err(|e| format!("{}/manifest.toml: {e}", dir_name))?;
        if manifest.plugin.name != dir_name {
            return Err(format!(
                "modules/plugins/{dir_name}/manifest.toml: [plugin].name = {:?} \
                 does not match directory name {dir_name:?}",
                manifest.plugin.name
            ));
        }
        let spec = plugin_dir.join("SPEC.md");
        let spec_path = if spec.is_file() { Some(spec) } else { None };
        out.push(DiscoveredPlugin {
            dir_name,
            path: plugin_dir,
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
// `workspace.toml [plugins.<name>]` resolution (PLUGINS.en.md §"Loading").
// ---------------------------------------------------------------------------

/// Parsed entry from `workspace.toml [plugins.<name>]`. `extra` carries the
/// plugin-defined config keys (everything other than `enabled`).
#[derive(Debug, Clone)]
struct WorkspaceEntry {
    enabled: bool,
    extra: serde_json::Map<String, serde_json::Value>,
}

fn workspace_toml_path(root: &Path) -> PathBuf {
    root.join(".bwoc/workspace.toml")
}

/// Read `<root>/.bwoc/workspace.toml` and return the `[plugins]` block as a
/// map of `name -> WorkspaceEntry`. An absent `[plugins]` table is not an
/// error — it just means no plugins have been registered yet.
///
/// Per PLUGINS.en.md line 270, a missing `enabled` field is a manifest error;
/// we surface it the same way `bwoc check` will.
fn workspace_plugins(
    root: &Path,
) -> Result<std::collections::BTreeMap<String, WorkspaceEntry>, String> {
    let path = workspace_toml_path(root);
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&body).map_err(|e| format!("{}: parse: {e}", path.display()))?;
    let mut out = std::collections::BTreeMap::new();
    let Some(plugins) = value.get("plugins").and_then(|v| v.as_table()) else {
        return Ok(out);
    };
    for (name, entry) in plugins {
        let table = entry
            .as_table()
            .ok_or_else(|| format!("{}: [plugins.{name}] is not a table", path.display()))?;
        let enabled = table
            .get("enabled")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| {
                format!(
                    "{}: [plugins.{name}] is missing required 'enabled' field",
                    path.display()
                )
            })?;
        let mut extra = serde_json::Map::new();
        for (k, v) in table {
            if k == "enabled" {
                continue;
            }
            extra.insert(k.clone(), toml_to_json(v));
        }
        out.insert(name.clone(), WorkspaceEntry { enabled, extra });
    }
    Ok(out)
}

/// Set of plugin KINDS that have at least one installed-and-enabled plugin in
/// this workspace. This is the resolution substrate for the skill-on-plugin
/// dependency check (SKILLS.en.md §"Skill-on-plugin dependency", lines 194–195):
/// a kind is satisfied iff some `modules/plugins/<name>/` of that kind has an
/// `[plugins.<name>] enabled = true` entry in `.bwoc/workspace.toml`.
///
/// Fails closed: any error discovering plugins or reading `workspace.toml`
/// collapses to an empty set, so a dependency resolves as UNMET rather than
/// being silently assumed satisfied — the safe direction for a pre-flight gate.
pub fn enabled_plugin_kinds(root: &Path) -> std::collections::BTreeSet<String> {
    let installed = match discover(root) {
        Ok(v) => v,
        Err(_) => return std::collections::BTreeSet::new(),
    };
    let ws = workspace_plugins(root).unwrap_or_default();
    installed
        .into_iter()
        .filter(|p| ws.get(&p.dir_name).map(|e| e.enabled).unwrap_or(false))
        .map(|p| p.manifest.plugin.kind)
        .collect()
}

fn toml_to_json(v: &toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(a) => serde_json::Value::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            let mut m = serde_json::Map::new();
            for (k, v) in t {
                m.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(m)
        }
    }
}

// ---------------------------------------------------------------------------
// `workspace.toml [plugins.<name>]` mutation (PLUGINS.en.md §"Loading").
// ---------------------------------------------------------------------------
//
// Per-workspace scope — this is the spec difference vs skills, which flip
// `enabled` inside the consuming agent's `config.manifest.json`. Plugins are
// framework-loaded once for the whole workspace; the gate is centralized
// in `.bwoc/workspace.toml [plugins.<name>]`.

/// Load `workspace.toml` as a mutable `toml::Table`. Errors carry the path
/// so the operator can find the culprit.
fn load_workspace_toml(root: &Path) -> Result<toml::Table, String> {
    let path = workspace_toml_path(root);
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str::<toml::Table>(&body).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn save_workspace_toml(root: &Path, doc: &toml::Table) -> Result<(), String> {
    let path = workspace_toml_path(root);
    let body = toml::to_string_pretty(doc).map_err(|e| format!("serialize toml: {e}"))?;
    std::fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Set the `enabled` field on `[plugins.<name>]`. If the table does not exist
/// yet, create it (mirroring how `skill enable` adds a missing entry). Returns
/// `(entry_added, final_enabled)`. When `require_existing` is true (the
/// `disable` path), a missing entry is an error.
fn set_workspace_plugin_enabled(
    doc: &mut toml::Table,
    name: &str,
    enabled: bool,
    require_existing: bool,
) -> Result<(bool, bool), String> {
    let plugins = doc
        .entry("plugins".to_string())
        .or_insert(toml::Value::Table(toml::Table::new()));
    let plugins_table = plugins
        .as_table_mut()
        .ok_or_else(|| "workspace.toml [plugins] is not a table".to_string())?;

    if let Some(existing) = plugins_table.get_mut(name) {
        let table = existing
            .as_table_mut()
            .ok_or_else(|| format!("workspace.toml [plugins.{name}] is not a table"))?;
        table.insert("enabled".to_string(), toml::Value::Boolean(enabled));
        return Ok((false, enabled));
    }

    if require_existing {
        return Err(format!(
            "no [plugins.{name}] entry in workspace.toml \
             (run `bwoc plugin enable {name}` first)"
        ));
    }

    let mut new_entry = toml::Table::new();
    new_entry.insert("enabled".to_string(), toml::Value::Boolean(enabled));
    plugins_table.insert(name.to_string(), toml::Value::Table(new_entry));
    Ok((true, enabled))
}

/// Remove the `[plugins.<name>]` table entirely from `workspace.toml`.
/// Returns whether an entry was present before the call. Mirrors
/// PLUGINS.en.md §"Removal" line 419 — "not just `enabled = false`."
fn remove_workspace_plugin_entry(doc: &mut toml::Table, name: &str) -> bool {
    let Some(plugins) = doc.get_mut("plugins").and_then(|v| v.as_table_mut()) else {
        return false;
    };
    plugins.remove(name).is_some()
}

// ---------------------------------------------------------------------------
// JSON helpers.
// ---------------------------------------------------------------------------

fn plugin_summary_json(p: &DiscoveredPlugin, entry: Option<&WorkspaceEntry>) -> serde_json::Value {
    let mut v = serde_json::json!({
        "name": p.manifest.plugin.name,
        "kind": p.manifest.plugin.kind,
        "version": p.manifest.plugin.version,
        "description": p.manifest.plugin.description,
        "compat": p.manifest.plugin.compat,
        "entry": p.manifest.plugin.entry,
        "path": p.path.display().to_string(),
        "spec_path": p.spec_path.as_ref().map(|p| p.display().to_string()),
        "registered": entry.is_some(),
        "enabled": entry.map(|e| e.enabled),
    });
    if let Some(e) = entry {
        if !e.extra.is_empty() {
            v["config"] = serde_json::Value::Object(e.extra.clone());
        }
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
            eprintln!("bwoc plugin: serialize JSON failed: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// `bwoc plugin list` ---------------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_list(args: ListArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc plugin list: {e}");
            return 2;
        }
    };
    let plugins = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc plugin list: {e}");
            return 1;
        }
    };
    let ws_entries = match workspace_plugins(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc plugin list: {e}");
            return 1;
        }
    };

    let annotated: Vec<(DiscoveredPlugin, Option<WorkspaceEntry>)> = plugins
        .into_iter()
        .map(|p| {
            let entry = ws_entries.get(&p.manifest.plugin.name).cloned();
            (p, entry)
        })
        .collect();

    let filtered: Vec<_> = annotated
        .into_iter()
        .filter(|(p, en)| {
            if args.enabled && !matches!(en, Some(e) if e.enabled) {
                return false;
            }
            if let Some(k) = args.kind.as_deref() {
                if p.manifest.plugin.kind != k {
                    return false;
                }
            }
            true
        })
        .collect();

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "plugins": filtered
                .iter()
                .map(|(p, en)| plugin_summary_json(p, en.as_ref()))
                .collect::<Vec<_>>(),
        });
        return print_json(&value);
    }

    if filtered.is_empty() {
        println!(
            "(no framework plugins installed at {})",
            plugins_dir(&root).display()
        );
        return 0;
    }

    // Compact human table — name · kind · version · enabled · description.
    let name_w = filtered
        .iter()
        .map(|(p, _)| p.manifest.plugin.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let kind_w = filtered
        .iter()
        .map(|(p, _)| p.manifest.plugin.kind.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let ver_w = filtered
        .iter()
        .map(|(p, _)| p.manifest.plugin.version.len())
        .max()
        .unwrap_or(7)
        .max(7);

    println!(
        "{:<name_w$}  {:<kind_w$}  {:<ver_w$}  {:<8}  DESCRIPTION",
        "NAME", "KIND", "VERSION", "ENABLED",
    );
    for (p, en) in &filtered {
        let en_str = match en {
            Some(e) if e.enabled => "yes",
            Some(_) => "no",
            None => "-",
        };
        println!(
            "{:<name_w$}  {:<kind_w$}  {:<ver_w$}  {:<8}  {}",
            p.manifest.plugin.name,
            p.manifest.plugin.kind,
            p.manifest.plugin.version,
            en_str,
            p.manifest.plugin.description,
        );
    }
    0
}

// ---------------------------------------------------------------------------
// `bwoc plugin show <name>` --------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_show(args: ShowArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc plugin show: {e}");
            return 2;
        }
    };
    let plugins = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc plugin show: {e}");
            return 1;
        }
    };
    let Some(p) = plugins.into_iter().find(|p| p.dir_name == args.name) else {
        eprintln!(
            "bwoc plugin show: '{}' not installed in {}",
            args.name,
            plugins_dir(&root).display()
        );
        return 2;
    };
    let ws_entries = match workspace_plugins(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc plugin show: {e}");
            return 1;
        }
    };
    let entry = ws_entries.get(&p.manifest.plugin.name);

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "plugin": plugin_summary_json(&p, entry),
        });
        return print_json(&value);
    }

    println!("Plugin        {}", p.manifest.plugin.name);
    println!("Kind          {}", p.manifest.plugin.kind);
    println!("Version       {}", p.manifest.plugin.version);
    println!("Compat        {}", p.manifest.plugin.compat);
    println!("Entry         {}", p.manifest.plugin.entry);
    println!("Description   {}", p.manifest.plugin.description);
    println!("Path          {}", p.path.display());
    if let Some(sp) = &p.spec_path {
        println!("Spec          {}", sp.display());
    } else {
        println!("Spec          (missing — SPEC.md not present)");
    }
    match entry {
        Some(e) => {
            println!(
                "Registered    yes  (workspace.toml [plugins.{}])",
                p.manifest.plugin.name
            );
            println!("Enabled       {}", if e.enabled { "yes" } else { "no" });
            if e.extra.is_empty() {
                println!("Config        (none)");
            } else {
                println!("Config");
                for (k, v) in &e.extra {
                    println!("  {k} = {}", v);
                }
            }
        }
        None => {
            println!("Registered    no   (not present in workspace.toml [plugins])");
            println!("Enabled       -");
            println!("Config        (none)");
        }
    }
    0
}

// ===========================================================================
// Write-side surface (BWOC-24) ==============================================
// ===========================================================================

// ---------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------

fn template_dir(root: &Path) -> PathBuf {
    root.join("modules/plugin-template")
}

fn installed_sources_path(root: &Path) -> PathBuf {
    root.join(".bwoc/installed-sources.toml")
}

/// Closed enum from PLUGINS.en.md §"Plugin Kinds" + the EPIC-2 `audit` kind
/// (BWOC-10). Future kinds extend this list without restructuring the
/// template — see `modules/plugin-template/SPEC.md` line 62.
const VALID_KINDS: &[&str] = &["memory-backend", "llm-backend", "workflow", "audit"];

fn validate_plugin_kind(kind: &str) -> Result<(), String> {
    if VALID_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(format!(
            "'{kind}' is not a valid plugin kind. Expected one of: {}",
            VALID_KINDS.join(", ")
        ))
    }
}

/// kebab-case validator. Allows `[a-z0-9]+(-[a-z0-9]+)*`. Rejects empty,
/// path separators, leading/trailing dashes, double dashes, uppercase.
fn validate_plugin_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("plugin name is empty".to_string());
    }
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(format!(
            "'{name}' is not a valid plugin name (no path separators)"
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

/// Substitute the four documented placeholders. Unknown `{{...}}` markers
/// are left in place — the operator is the editor of last resort.
fn substitute_placeholders(
    body: &str,
    name: &str,
    kind: &str,
    version: &str,
    description: &str,
) -> String {
    body.replace("{{pluginName}}", name)
        .replace("{{pluginKind}}", kind)
        .replace("{{pluginVersion}}", version)
        .replace("{{pluginDescription}}", description)
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
            } else if ft.is_symlink() {
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

/// Three source kinds per PLUGINS.en.md §"Sources & Installation" line 335.
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
/// (returns empty). The format is a top-level table keyed by source_key —
/// shared with SKILLS, plugin entries use `kind = "plugin"`.
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
        entry
            .as_table()
            .ok_or_else(|| format!("{}: entry '{key}' is not a table", path.display()))?;
        out.push(InstalledSource {
            source_key: key.clone(),
        });
    }
    Ok(out)
}

/// Append (or replace) one entry. Other entries (including `kind = "skill"`
/// rows) are preserved untouched — the registry is shared across surfaces.
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
    entry.insert("kind".into(), toml::Value::String("plugin".to_string()));
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

/// Remove one `kind = "plugin"` entry from `.bwoc/installed-sources.toml`.
/// Missing file or missing key is a no-op (success). Skill entries with the
/// same name (unlikely but possible) are left intact.
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
            let kind = t.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            (n == name && kind == "plugin").then(|| k.clone())
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
/// success; Err with the command + stderr on failure or spawn error.
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
// `bwoc plugin init <name> --kind <kind>` ------------------------------------
// ---------------------------------------------------------------------------

pub fn run_init(args: InitArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc plugin init: {e}");
            return 2;
        }
    };
    if let Err(e) = validate_plugin_name(&args.name) {
        eprintln!("bwoc plugin init: {e}");
        return 2;
    }
    if let Err(e) = validate_plugin_kind(&args.kind) {
        eprintln!("bwoc plugin init: {e}");
        return 2;
    }
    let template = template_dir(&root);
    if !template.is_dir() {
        eprintln!(
            "bwoc plugin init: template missing at {}. \
             Run a workspace with `modules/plugin-template/`.",
            template.display()
        );
        return 2;
    }
    let target = plugins_dir(&root).join(&args.name);
    if target.exists() {
        eprintln!(
            "bwoc plugin init: target already exists: {}",
            target.display()
        );
        return 2;
    }

    let version = args.version.as_deref().unwrap_or("0.1.0").to_string();
    let description = args
        .description
        .as_deref()
        .unwrap_or("Describe what this plugin does (one sentence).")
        .to_string();

    // Materialize: copy then rewrite each file's text in place.
    let written = match copy_dir_recursive(&template, &target) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc plugin init: {e}");
            let _ = std::fs::remove_dir_all(&target);
            return 1;
        }
    };
    let mut substituted: Vec<String> = Vec::with_capacity(written.len());
    for file in &written {
        if let Ok(text) = std::fs::read_to_string(file) {
            let out =
                substitute_placeholders(&text, &args.name, &args.kind, &version, &description);
            if let Err(e) = std::fs::write(file, out) {
                eprintln!("bwoc plugin init: write {}: {e}", file.display());
                let _ = std::fs::remove_dir_all(&target);
                return 1;
            }
            substituted.push(file.display().to_string());
        }
    }

    // Drop an `.authored-in-place` marker so `bwoc check`'s orphan-installation
    // gate (PLUGINS.en.md §"Verification" line 442) treats this as authored,
    // not installed-from-source.
    let marker = target.join(".authored-in-place");
    if let Err(e) = std::fs::write(&marker, "") {
        eprintln!(
            "bwoc plugin init: warning — could not write {}: {e}",
            marker.display()
        );
    }

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "name": args.name,
            "kind": args.kind,
            "target": target.display().to_string(),
            "files_written": substituted,
            "placeholders": {
                "pluginName": args.name,
                "pluginKind": args.kind,
                "pluginVersion": version,
                "pluginDescription": description,
            },
            "authored_in_place": true,
        });
        return print_json(&value);
    }

    println!("Initialized framework plugin '{}'", args.name);
    println!("  Target:      {}", target.display());
    println!("  Kind:        {}", args.kind);
    println!("  Version:     {version}");
    println!("  Files:       {} written", substituted.len());
    println!();
    println!("Next: edit SPEC.md, then");
    println!(
        "      `bwoc plugin enable {}` to register the plugin in workspace.toml.",
        args.name
    );
    0
}

// ---------------------------------------------------------------------------
// `bwoc plugin install <source>` ---------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_install(args: InstallArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc plugin install: {e}");
            return 2;
        }
    };
    let kind = match detect_source_kind(&args.source) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("bwoc plugin install: {e}");
            return 2;
        }
    };

    // First-install gate: check installed-sources.toml for prior records of
    // this exact source string. We key by SHA-256(url) so even local paths
    // get a stable key.
    let key = source_key(&args.source);
    let prior = match load_installed_sources(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc plugin install: {e}");
            return 1;
        }
    };
    let already_known = prior.iter().any(|s| s.source_key == key);
    if !already_known && !args.allow_new_source {
        eprintln!(
            "bwoc plugin install: '{}' has not been installed in this workspace before. \
             Pass --allow-new-source to acknowledge you have inspected this source.",
            args.source
        );
        return 2;
    }

    let stage = match stage_source(&kind, &args.source) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bwoc plugin install: {e}");
            return 1;
        }
    };

    // Trust gate.
    let mut checksum_outcome: Option<String> = None;
    if !args.no_verify {
        match verify_checksum(&kind, &stage.staged_dir, &stage.archive_path) {
            Ok(s) => checksum_outcome = Some(s),
            Err(e) => {
                eprintln!("bwoc plugin install: trust-gate failed: {e}");
                let _ = std::fs::remove_dir_all(&stage.staged_dir);
                return 1;
            }
        }
    } else {
        eprintln!(
            "bwoc plugin install: warning — --no-verify skips SHA-256 verification of {}",
            args.source
        );
    }

    let manifest_path = stage.staged_dir.join("manifest.toml");
    if !manifest_path.is_file() {
        eprintln!(
            "bwoc plugin install: source missing manifest.toml at staged root; \
             cannot resolve plugin name or kind"
        );
        let _ = std::fs::remove_dir_all(&stage.staged_dir);
        return 1;
    }
    let staged_manifest = match parse_manifest(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc plugin install: parse staged manifest: {e}");
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
    };
    let plugin_name = staged_manifest.plugin.name.clone();
    let plugin_kind = staged_manifest.plugin.kind.clone();
    if let Err(e) = validate_plugin_name(&plugin_name) {
        eprintln!("bwoc plugin install: manifest plugin name: {e}");
        let _ = std::fs::remove_dir_all(&stage.staged_dir);
        return 2;
    }
    // Per PLUGINS.en.md §"init vs install" line 406-409, kind is read from
    // the source manifest and never overridden. We still validate it against
    // the closed enum so a malformed source surfaces before we materialize.
    if let Err(e) = validate_plugin_kind(&plugin_kind) {
        eprintln!("bwoc plugin install: manifest plugin kind: {e}");
        let _ = std::fs::remove_dir_all(&stage.staged_dir);
        return 2;
    }

    let target = plugins_dir(&root).join(&plugin_name);
    if target.exists() {
        if !args.upgrade {
            eprintln!(
                "bwoc plugin install: '{plugin_name}' already installed at {}; \
                 pass --upgrade to replace",
                target.display()
            );
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 2;
        }
        if let Err(e) = std::fs::remove_dir_all(&target) {
            eprintln!(
                "bwoc plugin install: --upgrade: failed to remove {}: {e}",
                target.display()
            );
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
    }

    if let Some(parent) = target.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("bwoc plugin install: create {}: {e}", parent.display());
            let _ = std::fs::remove_dir_all(&stage.staged_dir);
            return 1;
        }
    }
    if let Err(e) = std::fs::rename(&stage.staged_dir, &target) {
        if let Err(e2) = copy_dir_recursive(&stage.staged_dir, &target) {
            eprintln!(
                "bwoc plugin install: install {} -> {}: {e} (copy fallback also failed: {e2})",
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
            eprintln!("bwoc plugin install: hash {}: {e}", target.display());
            return 1;
        }
    };

    let acknowledged_by = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let target_rel = format!("modules/plugins/{plugin_name}");
    if let Err(e) = record_installed_source(
        &root,
        &key,
        &args.source,
        &plugin_name,
        &target_rel,
        &installed_hash,
        &acknowledged_by,
    ) {
        eprintln!("bwoc plugin install: warning — could not record source: {e}");
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
            "name": plugin_name,
            "kind": plugin_kind,
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

    println!("Installed framework plugin '{plugin_name}'");
    println!("  Source:      {}", args.source);
    println!("  Kind:        {plugin_kind}");
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
    println!(
        "Plugin is dormant. Run `bwoc plugin enable {plugin_name}` to register it in workspace.toml."
    );
    0
}

struct StagedSource {
    staged_dir: PathBuf,
    /// For tarball installs only — the downloaded archive byte path, kept
    /// alongside the staged dir so verify_checksum can read it.
    archive_path: Option<PathBuf>,
}

fn stage_source(kind: &SourceKind, raw: &str) -> Result<StagedSource, String> {
    let stem = sha256_string(raw);
    let stage_root = std::env::temp_dir().join(format!("bwoc-plugin-install-{}", &stem[..16]));
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
// `bwoc plugin enable / disable <name>` --------------------------------------
// ---------------------------------------------------------------------------

pub fn run_enable(args: EnableArgs) -> i32 {
    run_enable_disable(args.common, args.name, args.json, true)
}

pub fn run_disable(args: DisableArgs) -> i32 {
    run_enable_disable(args.common, args.name, args.json, false)
}

fn run_enable_disable(common: CommonArgs, name: String, json: bool, enable: bool) -> i32 {
    let verb = if enable { "enable" } else { "disable" };
    let root = match resolve_workspace(&common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc plugin {verb}: {e}");
            return 2;
        }
    };
    // Plugin must be discoverable for `enable`; `disable` tolerates a missing
    // dir on disk (the operator may be cleaning up after a manual remove).
    let plugins = match discover(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc plugin {verb}: {e}");
            return 1;
        }
    };
    let discovered = plugins.iter().find(|p| p.dir_name == name);
    if enable && discovered.is_none() {
        eprintln!(
            "bwoc plugin enable: '{name}' is not installed under {}",
            plugins_dir(&root).display()
        );
        return 2;
    }

    let mut doc = match load_workspace_toml(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc plugin {verb}: {e}");
            return 1;
        }
    };
    let (added, final_enabled) =
        match set_workspace_plugin_enabled(&mut doc, &name, enable, !enable) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("bwoc plugin {verb}: {e}");
                return 2;
            }
        };
    if let Err(e) = save_workspace_toml(&root, &doc) {
        eprintln!("bwoc plugin {verb}: {e}");
        return 1;
    }

    if json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "plugin": name,
            "enabled": final_enabled,
            "entry_added": added,
        });
        return print_json(&value);
    }

    if added {
        println!("Added [plugins.{name}] entry to workspace.toml (enabled={final_enabled})");
    } else {
        println!("Set enabled={final_enabled} on [plugins.{name}] in workspace.toml");
    }
    0
}

// ---------------------------------------------------------------------------
// `bwoc plugin remove <name>` ------------------------------------------------
// ---------------------------------------------------------------------------

pub fn run_remove(args: RemoveArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc plugin remove: {e}");
            return 2;
        }
    };
    let target = plugins_dir(&root).join(&args.name);
    let dir_exists = target.is_dir();

    // Inspect workspace.toml for an existing [plugins.<name>] table — we
    // need this for both the confirmation banner and the actual cleanup.
    let ws_entries = match workspace_plugins(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc plugin remove: {e}");
            return 1;
        }
    };
    let registered = ws_entries.contains_key(&args.name);

    if !dir_exists && !registered {
        // Idempotent: nothing to do, exit 0 (PLUGINS.en.md §"Removal" line 421).
        if args.json {
            let value = serde_json::json!({
                "workspace": root.display().to_string(),
                "plugin": args.name,
                "removed_dir": false,
                "removed_registration": false,
                "forgot_source": false,
                "note": "not installed",
            });
            return print_json(&value);
        }
        println!("bwoc plugin remove: '{}' not installed", args.name);
        return 0;
    }

    if !args.yes {
        if args.json {
            eprintln!(
                "bwoc plugin remove: --json requires --yes (destructive op needs explicit ack)"
            );
            return 2;
        }
        eprintln!(
            "bwoc plugin remove: refusing to delete without --yes. \
             Would delete:\n  - {} (dir)\n  - workspace.toml [plugins.{}] entry: {}",
            target.display(),
            args.name,
            if registered { "yes" } else { "no" },
        );
        return 2;
    }

    // Clean workspace.toml first — if dir-delete fails, we have not yet left
    // the framework pointing at a half-deleted plugin. This is the inverse of
    // install, which writes registry last after materializing the dir.
    let mut removed_registration = false;
    if registered {
        let mut doc = match load_workspace_toml(&root) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("bwoc plugin remove: {e}");
                return 1;
            }
        };
        if remove_workspace_plugin_entry(&mut doc, &args.name) {
            if let Err(e) = save_workspace_toml(&root, &doc) {
                eprintln!("bwoc plugin remove: {e}");
                return 1;
            }
            removed_registration = true;
        }
    }

    let mut removed_dir = false;
    if dir_exists {
        if let Err(e) = std::fs::remove_dir_all(&target) {
            eprintln!("bwoc plugin remove: remove {}: {e}", target.display());
            return 1;
        }
        removed_dir = true;
    }

    let mut forgot = false;
    if args.forget_source {
        match forget_installed_source(&root, &args.name) {
            Ok(b) => forgot = b,
            Err(e) => {
                eprintln!("bwoc plugin remove: --forget-source: {e}");
                return 1;
            }
        }
    }

    if args.json {
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "plugin": args.name,
            "removed_dir": removed_dir,
            "removed_registration": removed_registration,
            "forgot_source": forgot,
        });
        return print_json(&value);
    }

    println!("Removed framework plugin '{}'", args.name);
    if removed_dir {
        println!("  Deleted:     {}", target.display());
    }
    if removed_registration {
        println!("  Cleaned:     workspace.toml [plugins.{}]", args.name);
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
            "name={{pluginName}} kind={{pluginKind}} v={{pluginVersion}} d={{pluginDescription}}";
        let out = substitute_placeholders(body, "alpha", "memory-backend", "0.2.0", "desc");
        assert_eq!(out, "name=alpha kind=memory-backend v=0.2.0 d=desc");
    }

    #[test]
    fn substitute_leaves_unknown_placeholders_alone() {
        let body = "{{pluginName}} {{unknown}}";
        let out = substitute_placeholders(body, "alpha", "workflow", "0.1.0", "d");
        assert_eq!(out, "alpha {{unknown}}");
    }

    #[test]
    fn validate_name_accepts_kebab() {
        assert!(validate_plugin_name("memory-tier2-noop").is_ok());
        assert!(validate_plugin_name("a").is_ok());
        assert!(validate_plugin_name("a1-b2").is_ok());
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
                validate_plugin_name(bad).is_err(),
                "expected '{bad}' to fail"
            );
        }
    }

    #[test]
    fn validate_kind_accepts_all_four() {
        for k in ["memory-backend", "llm-backend", "workflow", "audit"] {
            assert!(validate_plugin_kind(k).is_ok(), "expected '{k}' to pass");
        }
    }

    #[test]
    fn validate_kind_rejects_unknown() {
        for k in ["", "compliance", "policy", "Memory-Backend", "audit "] {
            assert!(validate_plugin_kind(k).is_err(), "expected '{k}' to fail");
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
        let k = detect_source_kind("https://github.com/org/plugin.git#v0.1.0").unwrap();
        match k {
            SourceKind::GitUrl(u, Some(r)) => {
                assert_eq!(u, "https://github.com/org/plugin.git");
                assert_eq!(r, "v0.1.0");
            }
            _ => panic!("expected GitUrl with ref"),
        }
    }

    #[test]
    fn detect_git_url_without_ref() {
        let k = detect_source_kind("https://github.com/org/plugin.git").unwrap();
        match k {
            SourceKind::GitUrl(u, None) => {
                assert_eq!(u, "https://github.com/org/plugin.git")
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

    // BWOC-39: argument-injection defense for git refs.
    #[test]
    fn detect_rejects_dash_leading_git_ref() {
        let err =
            detect_source_kind("https://github.com/org/plugin.git#--upload-pack=evil").unwrap_err();
        assert!(err.contains("must not begin with '-'"), "{err}");
        assert!(detect_source_kind("https://github.com/org/plugin.git#-x").is_err());
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
    fn workspace_plugin_enable_adds_entry() {
        let mut doc: toml::Table = toml::from_str("[workspace]\nname = \"t\"\n").unwrap();
        let (added, en) = set_workspace_plugin_enabled(&mut doc, "p", true, false).unwrap();
        assert!(added);
        assert!(en);
        // Re-read the value through the table API to confirm shape.
        let v = doc
            .get("plugins")
            .and_then(|v| v.get("p"))
            .and_then(|v| v.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap();
        assert!(v);
    }

    #[test]
    fn workspace_plugin_disable_requires_existing() {
        let mut doc: toml::Table = toml::from_str("[workspace]\nname = \"t\"\n").unwrap();
        let err = set_workspace_plugin_enabled(&mut doc, "p", false, true).unwrap_err();
        assert!(err.contains("no [plugins.p] entry"));
    }

    #[test]
    fn workspace_plugin_flip_existing() {
        let body = "[plugins.p]\nenabled = true\n";
        let mut doc: toml::Table = toml::from_str(body).unwrap();
        let (added, en) = set_workspace_plugin_enabled(&mut doc, "p", false, true).unwrap();
        assert!(!added);
        assert!(!en);
        let v = doc
            .get("plugins")
            .and_then(|v| v.get("p"))
            .and_then(|v| v.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap();
        assert!(!v);
    }

    #[test]
    fn workspace_plugin_remove_drops_entry() {
        let body = "[plugins.p]\nenabled = true\n[plugins.q]\nenabled = false\n";
        let mut doc: toml::Table = toml::from_str(body).unwrap();
        assert!(remove_workspace_plugin_entry(&mut doc, "p"));
        let plugins = doc.get("plugins").and_then(|v| v.as_table()).unwrap();
        assert!(!plugins.contains_key("p"));
        assert!(plugins.contains_key("q"));
        // Idempotent — second remove returns false.
        assert!(!remove_workspace_plugin_entry(&mut doc, "p"));
    }

    #[test]
    fn workspace_plugin_remove_preserves_other_tables() {
        let body = "[workspace]\nname = \"t\"\n[plugins.p]\nenabled = true\n";
        let mut doc: toml::Table = toml::from_str(body).unwrap();
        assert!(remove_workspace_plugin_entry(&mut doc, "p"));
        // [workspace] is untouched.
        assert_eq!(
            doc.get("workspace")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str()),
            Some("t")
        );
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
        let tmp = std::env::temp_dir().join("bwoc-plugin-test-digest");
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
}
