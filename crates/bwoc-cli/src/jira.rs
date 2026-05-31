//! `bwoc jira <verb>` — the CLI surface for the `jira` plugin kind (BWOC-42).
//!
//! ## What this is
//!
//! The operator-facing command surface for bidirectional scrum ↔ Jira sync. It
//! is the `bwoc jira` half of the contract framed in
//! `notes/2026-05-27_jira-plugin-architecture.md` and the
//! [Jira Issue Mapping Schema](../../docs/en/PLUGINS.en.md#jira-issue-mapping-schema):
//! this module owns **argument parsing, the sync ledger I/O, the auth gate, the
//! write-confirmation gate, and the JSON shapes** — it does NOT speak HTTP. The
//! live REST v3 calls (JQL, transitions, sprint assignment) belong to the
//! `jira`-kind plugin's adapter, exactly as `bwoc audit` dispatches to
//! `audit`-kind plugins. This CLI discovers the enabled `jira` plugin and
//! invokes its `[plugin].entry`; if none is installed, the live verbs
//! **stub-error gracefully** (exit `4`) rather than failing hard.
//!
//! ## Verbs
//!
//! | Verb | Needs auth | Needs plugin | Notes |
//! |---|---|---|---|
//! | `status` | no | no | Reads `.scrum/jira-sync.json` + reports auth presence. Offline. |
//! | `link <story> <issue>` | no | no | Records a story ↔ issue mapping in the ledger. Offline. |
//! | `unlink <story>` | no | no | Removes a mapping (idempotent). Offline. |
//! | `query <jql>` | yes | yes | Project-scoped JQL read, delegated to the plugin. |
//! | `transition <issue> <status>` | yes | yes | **Write** — gated behind confirmation. |
//! | `sync [--dry-run]` | yes | yes | `--dry-run` previews the plan (read-only); the bare apply is a **gated write**. |
//!
//! ## Auth model — credentials NEVER touch the CLI's printed surface
//!
//! Credentials resolve from three environment variables (per the BWOC-40 note
//! §2): `BWOC_JIRA_EMAIL`, `BWOC_JIRA_TOKEN`, `BWOC_JIRA_BASE_URL` — or, when an
//! env var is unset, from the `[jira]` table of the gitignored, `0600`
//! `.bwoc/secrets.toml` (auth.toml resolution option 2; env wins). The CLI only
//! ever **verifies the token is present** — it never reads the token value into
//! a field, never logs it, and never serializes it. `bwoc jira status` reports
//! `token_present: <bool>`, never the secret. The token reaches the plugin
//! subprocess purely by inherited environment, so the value provably never
//! flows through this module's own state. `.scrum/jira-sync.json` is sync
//! **state**, not secrets — the ledger schema below carries no token field.
//!
//! ## Exit codes — normative
//!
//! - `0` — success.
//! - `1` — local I/O or ledger-state error (could not read/write the ledger).
//! - `2` — operator/usage error: no workspace context, **missing Jira
//!   credentials**, a malformed issue key, or a gated write requested with
//!   `--json` but without `--yes`.
//! - `3` — `sync` detected a true per-field conflict requiring operator
//!   resolution; nothing was auto-resolved (BWOC-40 note §4).
//! - `4` — no enabled `jira`-kind plugin, so the live REST path is unavailable.
//! - `255` — plugin runtime error (spawn failure or non-JSON output from the
//!   plugin entry).
//!
//! Passing `--json` makes the exit code redundant: the structured envelope
//! carries `error`/`summary` fields with the same signal.

use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Exit codes + env var names (single source of truth for the module).
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_CONFLICT: i32 = 3;
const EXIT_NO_PLUGIN: i32 = 4;
const EXIT_PLUGIN_ERROR: i32 = 255;

const ENV_EMAIL: &str = "BWOC_JIRA_EMAIL";
const ENV_TOKEN: &str = "BWOC_JIRA_TOKEN";
const ENV_BASE_URL: &str = "BWOC_JIRA_BASE_URL";

const LEDGER_REL: &str = ".scrum/jira-sync.json";

// ---------------------------------------------------------------------------
// CLI surface. Defined here (not in main.rs) so argument parsing is unit-testable
// against `JiraCommand` directly — see the `tests` module.
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum JiraCommand {
    /// Reconcile the scrum backlog with Jira. `--dry-run` prints the per-field
    /// resolution plan (read-only); the bare apply is a write gated behind
    /// `--yes` (or an interactive prompt). Needs auth + an enabled jira plugin.
    Sync(SyncArgs),
    /// Run a project-scoped JQL read through the enabled jira plugin.
    Query(QueryArgs),
    /// Transition a Jira issue to a target status. A WRITE — gated behind
    /// `--yes` (or an interactive prompt). Needs auth + an enabled jira plugin.
    Transition(TransitionArgs),
    /// Record a scrum-story ↔ Jira-issue mapping in `.scrum/jira-sync.json`.
    /// Offline: writes the ledger only; projection fields fill on the next sync.
    Link(LinkArgs),
    /// Remove a scrum-story mapping from `.scrum/jira-sync.json` (idempotent).
    Unlink(UnlinkArgs),
    /// Show the sync ledger summary + Jira auth presence (never the token).
    /// Offline: reads `.scrum/jira-sync.json`; no network, no plugin needed.
    Status(StatusArgs),
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Print the resolution plan without writing anything (read-only preview).
    #[arg(long = "dry-run")]
    dry_run: bool,
    /// Acknowledge the gated write up front (required to apply in `--json` mode).
    #[arg(long)]
    yes: bool,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct QueryArgs {
    /// The JQL to run. The plugin constrains it to the configured project(s).
    jql: String,
    /// Page size for the bounded read (Atlassian `maxResults`).
    #[arg(long = "max-results", default_value_t = 50)]
    max_results: u32,
    /// Pagination offset (Atlassian `startAt`).
    #[arg(long = "start-at", default_value_t = 0)]
    start_at: u32,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable table.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct TransitionArgs {
    /// The Jira issue key to transition (e.g. `BWOC-123`).
    issue: String,
    /// The target workflow status (e.g. `In Progress`).
    status: String,
    /// Acknowledge the write up front (required in `--json` mode).
    #[arg(long)]
    yes: bool,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct LinkArgs {
    /// The scrum story id (the local key — any non-empty token, e.g. `BWOC-42`).
    story: String,
    /// The Jira issue key to map it to (e.g. `BWOC-123`).
    issue: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct UnlinkArgs {
    /// The scrum story id whose mapping to remove.
    story: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

/// Dispatch a parsed `JiraCommand`. Returns the process exit code.
pub fn run(cmd: JiraCommand) -> i32 {
    match cmd {
        JiraCommand::Sync(a) => run_sync(a),
        JiraCommand::Query(a) => run_query(a),
        JiraCommand::Transition(a) => run_transition(a),
        JiraCommand::Link(a) => run_link(a),
        JiraCommand::Unlink(a) => run_unlink(a),
        JiraCommand::Status(a) => run_status(a),
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution (mirror of audit.rs / plugin.rs — ancestor walk for
// .bwoc/workspace.toml unless overridden). Kept local to keep the surface
// decoupled from the read/write plugin module.
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
// Auth gate — env-only, presence-verified, token value NEVER captured.
// ---------------------------------------------------------------------------

/// Non-secret auth context. The token is verified present but deliberately NOT
/// stored — the plugin subprocess reads `BWOC_JIRA_TOKEN` from inherited env.
#[derive(Debug, Clone, PartialEq)]
struct JiraAuth {
    email: String,
    base_url: String,
}

/// Resolve auth from a getenv source. Returns the missing var names (in a stable
/// order) when any of the three are absent or empty. The token value is checked
/// for presence and immediately discarded — it is never returned.
fn resolve_auth(getenv: &dyn Fn(&str) -> Option<String>) -> Result<JiraAuth, Vec<&'static str>> {
    let nonempty = |k: &str| getenv(k).filter(|s| !s.is_empty());
    let email = nonempty(ENV_EMAIL);
    let base_url = nonempty(ENV_BASE_URL);
    let token_present = nonempty(ENV_TOKEN).is_some();

    let mut missing = Vec::new();
    if email.is_none() {
        missing.push(ENV_EMAIL);
    }
    if token_present {
        // present — nothing to report and nothing to store.
    } else {
        missing.push(ENV_TOKEN);
    }
    if base_url.is_none() {
        missing.push(ENV_BASE_URL);
    }
    if !missing.is_empty() {
        return Err(missing);
    }
    Ok(JiraAuth {
        email: email.expect("checked above"),
        base_url: base_url.expect("checked above"),
    })
}

/// Auth presence report for `bwoc jira status`. Carries `token_present` only —
/// never the token value (Adinnādāna at the output boundary).
#[derive(Debug, Clone, Serialize, PartialEq)]
struct AuthStatus {
    configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_url: Option<String>,
    token_present: bool,
    missing: Vec<&'static str>,
}

fn auth_status(getenv: &dyn Fn(&str) -> Option<String>) -> AuthStatus {
    let nonempty = |k: &str| getenv(k).filter(|s| !s.is_empty());
    let email = nonempty(ENV_EMAIL);
    let base_url = nonempty(ENV_BASE_URL);
    let token_present = nonempty(ENV_TOKEN).is_some();
    let mut missing = Vec::new();
    if email.is_none() {
        missing.push(ENV_EMAIL);
    }
    if !token_present {
        missing.push(ENV_TOKEN);
    }
    if base_url.is_none() {
        missing.push(ENV_BASE_URL);
    }
    AuthStatus {
        configured: missing.is_empty(),
        email,
        base_url,
        token_present,
        missing,
    }
}

fn real_getenv(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Resolution option 2 (auth.toml §`[jira.auth.secrets_file]`): load the
/// `[jira]` table from `<workspace>/.bwoc/secrets.toml` into the `BWOC_JIRA_*`
/// keyspace, so credentials resolve from the gitignored secrets file when the
/// env vars are unset. Returns an empty map when the file is absent, unparseable,
/// or (on unix) group/world-accessible — the last is refused with a stderr
/// warning, mirroring the gws/figma secret-permission guards. The token value is
/// handed only to the plugin's inherited env; it is never logged or serialized.
fn secrets_file_env(workspace: &Path) -> BTreeMap<&'static str, String> {
    let mut out = BTreeMap::new();
    let path = workspace.join(".bwoc/secrets.toml");
    let body = match std::fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => return out, // absent → nothing to resolve
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.permissions().mode() & 0o077 != 0 {
                eprintln!(
                    "bwoc jira: ignoring {} — it is group/world-accessible; `chmod 600` it.",
                    path.display()
                );
                return out;
            }
        }
    }
    let value: toml::Value = match toml::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc jira: ignoring {} — parse error: {e}", path.display());
            return out;
        }
    };
    let Some(jira) = value.get("jira").and_then(|v| v.as_table()) else {
        return out;
    };
    let mut take = |field: &str, key: &'static str| {
        if let Some(s) = jira.get(field).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                out.insert(key, s.to_string());
            }
        }
    };
    take("email", ENV_EMAIL);
    take("token", ENV_TOKEN);
    take("base_url", ENV_BASE_URL);
    out
}

/// Credential resolver used by the live verbs + `status`: a non-empty env var
/// wins; otherwise fall back to the `.bwoc/secrets.toml` `[jira]` table. Mirrors
/// the precedence documented in auth.toml (env first — nothing touches disk).
fn getenv_with_secrets(workspace: &Path) -> impl Fn(&str) -> Option<String> {
    let secrets = secrets_file_env(workspace);
    move |k: &str| {
        real_getenv(k)
            .filter(|s| !s.is_empty())
            .or_else(|| secrets.get(k).cloned())
    }
}

// ---------------------------------------------------------------------------
// Sync ledger — `.scrum/jira-sync.json`. State, not secrets (no token field).
// Mapping entries conform to the normative Jira Issue Mapping Schema
// (PLUGINS.en.md §"Jira Issue Mapping Schema"): `issue_key` + `project` are the
// stable pair; the rest are mutable projections, omitted (never `null`) when the
// issue has no value. A freshly `link`-ed entry is "pending" — it carries only
// `issue_key` + `project`; the projection fields fill on the first `sync`.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MappingEntry {
    issue_key: String,
    project: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    assignee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    story_points: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    parent_epic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    sprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    last_synced: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Ledger {
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    project: Option<String>,
    /// Keyed by scrum story id (e.g. `BWOC-40`).
    #[serde(default)]
    issues: BTreeMap<String, MappingEntry>,
}

impl Default for Ledger {
    fn default() -> Self {
        Ledger {
            version: 1,
            site: None,
            project: None,
            issues: BTreeMap::new(),
        }
    }
}

fn ledger_path(root: &Path) -> PathBuf {
    root.join(LEDGER_REL)
}

/// `Ok(None)` when the ledger file does not exist yet (a fresh workspace).
fn load_ledger(root: &Path) -> Result<Option<Ledger>, String> {
    let path = ledger_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let ledger = serde_json::from_str::<Ledger>(&body)
        .map_err(|e| format!("parse {}: {e}", path.display()))?;
    Ok(Some(ledger))
}

fn save_ledger(root: &Path, ledger: &Ledger) -> Result<(), String> {
    let path = ledger_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let body =
        serde_json::to_string_pretty(ledger).map_err(|e| format!("serialize ledger: {e}"))?;
    std::fs::write(&path, format!("{body}\n")).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Split a Jira issue key into `(project, number)`. Jira keys are
/// `<PROJECT>-<NUMBER>` with a numeric suffix; the project prefix carries no `-`.
fn parse_issue_key(key: &str) -> Result<(String, String), String> {
    let invalid = || {
        format!("invalid Jira issue key '{key}' — expected '<PROJECT>-<NUMBER>' (e.g. BWOC-123)")
    };
    let (project, number) = key.split_once('-').ok_or_else(invalid)?;
    if project.is_empty() || number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid());
    }
    Ok((project.to_string(), number.to_string()))
}

// ---------------------------------------------------------------------------
// jira-kind plugin discovery + invocation (the live REST path delegates here).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ManifestRaw {
    plugin: PluginSection,
}

#[derive(Debug, Clone, Deserialize)]
struct PluginSection {
    name: String,
    kind: String,
    entry: String,
}

#[derive(Debug, Clone, PartialEq)]
struct JiraPlugin {
    name: String,
    dir: PathBuf,
    entry: String,
}

fn plugins_dir(root: &Path) -> PathBuf {
    root.join("modules/plugins")
}

/// Read `.bwoc/workspace.toml [plugins.<name>] enabled` flags.
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

/// Walk `modules/plugins/*/manifest.toml`, keep `kind = "jira"`.
fn discover_jira_plugins(root: &Path) -> Result<Vec<JiraPlugin>, String> {
    let dir = plugins_dir(root);
    let mut found = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(format!("read {}: {e}", dir.display())),
    };
    for entry in entries.flatten() {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }
        let manifest = plugin_dir.join("manifest.toml");
        if !manifest.is_file() {
            continue;
        }
        let body = std::fs::read_to_string(&manifest)
            .map_err(|e| format!("read {}: {e}", manifest.display()))?;
        let parsed: ManifestRaw =
            toml::from_str(&body).map_err(|e| format!("parse {}: {e}", manifest.display()))?;
        if parsed.plugin.kind == "jira" {
            found.push(JiraPlugin {
                name: parsed.plugin.name,
                dir: plugin_dir,
                entry: parsed.plugin.entry,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// First `jira`-kind plugin that is enabled in `workspace.toml`.
fn find_enabled_jira_plugin(root: &Path) -> Result<Option<JiraPlugin>, String> {
    let all = discover_jira_plugins(root)?;
    let enabled = workspace_enabled_set(root)?;
    Ok(all
        .into_iter()
        .find(|p| matches!(enabled.get(&p.name), Some(true))))
}

fn resolve_entry_program(plugin_dir: &Path, entry: &str) -> OsString {
    let candidate = plugin_dir.join(entry);
    if candidate.is_file() {
        candidate.into_os_string()
    } else {
        OsString::from(entry)
    }
}

// --- Request payloads handed to the plugin over stdin (one per live verb). ---

fn query_request(
    workspace: &Path,
    plugin_dir: &Path,
    jql: &str,
    start_at: u32,
    max_results: u32,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "query",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "jql": jql,
        "start_at": start_at,
        "max_results": max_results,
    })
}

fn transition_request(
    workspace: &Path,
    plugin_dir: &Path,
    issue: &str,
    to_status: &str,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "transition",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "issue": issue,
        "to_status": to_status,
    })
}

fn sync_request(workspace: &Path, plugin_dir: &Path, dry_run: bool) -> serde_json::Value {
    serde_json::json!({
        "operation": "sync",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "dry_run": dry_run,
    })
}

/// Spawn the plugin entry, feed it the request payload on stdin, parse its
/// stdout as JSON. The token reaches the child via inherited env — never
/// re-handled here. Mirrors `audit::invoke_plugin`.
fn invoke_jira_plugin(
    plugin: &JiraPlugin,
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

    let mut cmd = Command::new(&program);
    cmd.current_dir(&plugin.dir)
        .env("BWOC_WORKSPACE", workspace)
        .env("BWOC_PLUGIN_DIR", &plugin.dir)
        .env("BWOC_JIRA_OPERATION", operation)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // The plugin reads BWOC_JIRA_{EMAIL,TOKEN,BASE_URL} from its env. When they
    // come from .bwoc/secrets.toml (not the process env), inject them here so the
    // child sees them — a non-empty inherited env var still wins (never overridden).
    for (key, val) in secrets_file_env(workspace) {
        if real_getenv(key).filter(|s| !s.is_empty()).is_none() {
            cmd.env(key, val);
        }
    }

    let mut child = cmd
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
// Shared helpers.
// ---------------------------------------------------------------------------

fn print_json(value: &serde_json::Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(e) => {
            eprintln!("bwoc jira: serialize JSON: {e}");
            false
        }
    }
}

/// A gated write requested in `--json` mode cannot prompt — it requires `--yes`.
fn json_write_blocked(json: bool, yes: bool) -> bool {
    json && !yes
}

/// Interactive y/N confirmation on stderr. EOF / anything but yes → false.
fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Structured error envelope for `--json` mode — every error has a JSON twin.
fn emit_error_json(verb: &str, code: &str, message: &str) {
    let value = serde_json::json!({
        "ok": false,
        "verb": verb,
        "error": code,
        "message": message,
    });
    print_json(&value);
}

/// Resolve auth for a live verb. On miss, print the precise remediation (naming
/// the missing vars, never any value) and the JSON twin, returning the exit code.
fn require_auth(verb: &str, json: bool, workspace: &Path) -> Result<JiraAuth, i32> {
    match resolve_auth(&getenv_with_secrets(workspace)) {
        Ok(auth) => Ok(auth),
        Err(missing) => {
            let list = missing.join(", ");
            let msg = format!(
                "missing Jira credentials: {list}. Set {ENV_EMAIL}, {ENV_TOKEN}, and \
                 {ENV_BASE_URL} in the environment or the [jira] table of \
                 .bwoc/secrets.toml (never commit the token)."
            );
            if json {
                emit_error_json(verb, "auth_missing", &msg);
            } else {
                eprintln!("bwoc jira {verb}: {msg}");
            }
            Err(EXIT_USAGE)
        }
    }
}

/// Find the enabled jira plugin for a live verb, or stub-error gracefully.
fn require_plugin(root: &Path, verb: &str, json: bool) -> Result<JiraPlugin, i32> {
    match find_enabled_jira_plugin(root) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => {
            let msg = "no enabled 'jira'-kind plugin in this workspace. The live Jira REST \
                       path is provided by a jira-kind plugin (see PLUGINS.en.md §jira). \
                       Install one with `bwoc plugin install <source>` then `bwoc plugin \
                       enable <name>`. `bwoc jira status`, `link`, and `unlink` work offline."
                .to_string();
            if json {
                emit_error_json(verb, "no_jira_plugin", &msg);
            } else {
                eprintln!("bwoc jira {verb}: {msg}");
            }
            Err(EXIT_NO_PLUGIN)
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "discovery_error", &e);
            } else {
                eprintln!("bwoc jira {verb}: {e}");
            }
            Err(EXIT_PLUGIN_ERROR)
        }
    }
}

// ---------------------------------------------------------------------------
// Verb implementations.
// ---------------------------------------------------------------------------

fn run_status(args: StatusArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc jira status: {e}");
            return EXIT_USAGE;
        }
    };

    let ledger = match load_ledger(&root) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bwoc jira status: {e}");
            return EXIT_LOCAL_ERROR;
        }
    };
    let auth = auth_status(&getenv_with_secrets(&root));

    if args.json {
        let issues: Vec<serde_json::Value> = ledger
            .as_ref()
            .map(|l| {
                l.issues
                    .iter()
                    .map(|(story, e)| {
                        serde_json::json!({
                            "story": story,
                            "issue_key": e.issue_key,
                            "project": e.project,
                            "status": e.status,
                            "last_synced": e.last_synced,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "ledger": {
                "present": ledger.is_some(),
                "path": LEDGER_REL,
                "version": ledger.as_ref().map(|l| l.version),
                "site": ledger.as_ref().and_then(|l| l.site.clone()),
                "project": ledger.as_ref().and_then(|l| l.project.clone()),
                "issue_count": issues.len(),
                "issues": issues,
            },
            "auth": auth,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }

    println!("Jira sync status — workspace: {}", root.display());
    match &ledger {
        None => println!("  ledger: (none yet — {LEDGER_REL} not created)"),
        Some(l) => {
            println!(
                "  ledger: {} — v{}, project {}, {} mapping(s)",
                LEDGER_REL,
                l.version,
                l.project.as_deref().unwrap_or("(unset)"),
                l.issues.len()
            );
            for (story, e) in &l.issues {
                println!(
                    "    {story} → {} [{}]{}",
                    e.issue_key,
                    e.status.as_deref().unwrap_or("pending"),
                    if e.last_synced.is_none() {
                        " (not yet synced)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    if auth.configured {
        println!(
            "  auth: configured (email {}, base_url {}, token present)",
            auth.email.as_deref().unwrap_or("?"),
            auth.base_url.as_deref().unwrap_or("?"),
        );
    } else {
        println!(
            "  auth: NOT configured — missing {}",
            auth.missing.join(", ")
        );
    }
    EXIT_OK
}

fn run_link(args: LinkArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc jira link: {e}");
            return EXIT_USAGE;
        }
    };
    if args.story.is_empty() {
        eprintln!("bwoc jira link: story id must not be empty");
        return EXIT_USAGE;
    }
    let (project, _number) = match parse_issue_key(&args.issue) {
        Ok(v) => v,
        Err(e) => {
            if args.json {
                emit_error_json("link", "bad_issue_key", &e);
            } else {
                eprintln!("bwoc jira link: {e}");
            }
            return EXIT_USAGE;
        }
    };

    let mut ledger = match load_ledger(&root) {
        Ok(l) => l.unwrap_or_default(),
        Err(e) => {
            eprintln!("bwoc jira link: {e}");
            return EXIT_LOCAL_ERROR;
        }
    };

    if let Some(existing) = ledger.issues.get(&args.story) {
        if existing.issue_key != args.issue {
            let msg = format!(
                "story '{}' is already linked to '{}'. Run `bwoc jira unlink {}` first.",
                args.story, existing.issue_key, args.story
            );
            if args.json {
                emit_error_json("link", "already_linked", &msg);
            } else {
                eprintln!("bwoc jira link: {msg}");
            }
            return EXIT_USAGE;
        }
        // Idempotent: already linked to the same issue.
    } else {
        ledger.issues.insert(
            args.story.clone(),
            MappingEntry {
                issue_key: args.issue.clone(),
                project: project.clone(),
                summary: None,
                status: None,
                assignee: None,
                story_points: None,
                parent_epic: None,
                sprint: None,
                last_synced: None,
            },
        );
        if ledger.project.is_none() {
            ledger.project = Some(project.clone());
        }
        if let Err(e) = save_ledger(&root, &ledger) {
            eprintln!("bwoc jira link: {e}");
            return EXIT_LOCAL_ERROR;
        }
    }

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "linked": {
                "story": args.story,
                "issue_key": args.issue,
                "project": project,
                "pending_sync": true,
            },
            "ledger_path": LEDGER_REL,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }
    println!(
        "bwoc jira link: {} → {} (project {}) — recorded; projection fields fill on next sync",
        args.story, args.issue, project
    );
    EXIT_OK
}

fn run_unlink(args: UnlinkArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc jira unlink: {e}");
            return EXIT_USAGE;
        }
    };
    let mut ledger = match load_ledger(&root) {
        Ok(l) => l.unwrap_or_default(),
        Err(e) => {
            eprintln!("bwoc jira unlink: {e}");
            return EXIT_LOCAL_ERROR;
        }
    };
    let removed = ledger.issues.remove(&args.story);
    if removed.is_some() {
        if let Err(e) = save_ledger(&root, &ledger) {
            eprintln!("bwoc jira unlink: {e}");
            return EXIT_LOCAL_ERROR;
        }
    }

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "removed": removed.is_some(),
            "unlinked": removed.as_ref().map(|e| serde_json::json!({
                "story": args.story,
                "issue_key": e.issue_key,
            })),
            "ledger_path": LEDGER_REL,
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }
    match removed {
        Some(e) => println!("bwoc jira unlink: removed {} → {}", args.story, e.issue_key),
        None => println!("bwoc jira unlink: '{}' was not linked (no-op)", args.story),
    }
    EXIT_OK
}

fn run_query(args: QueryArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc jira query: {e}");
            return EXIT_USAGE;
        }
    };
    if args.jql.trim().is_empty() {
        let msg = "JQL must not be empty".to_string();
        if args.json {
            emit_error_json("query", "bad_args", &msg);
        } else {
            eprintln!("bwoc jira query: {msg}");
        }
        return EXIT_USAGE;
    }
    let _auth = match require_auth("query", args.json, &root) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let plugin = match require_plugin(&root, "query", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let request = query_request(
        &root,
        &plugin.dir,
        &args.jql,
        args.start_at,
        args.max_results,
    );
    match invoke_jira_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let total = value.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                println!("bwoc jira query: {total} issue(s) for `{}`", args.jql);
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("query", "plugin_error", &e);
            } else {
                eprintln!("bwoc jira query: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

fn run_transition(args: TransitionArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc jira transition: {e}");
            return EXIT_USAGE;
        }
    };
    if let Err(e) = parse_issue_key(&args.issue) {
        if args.json {
            emit_error_json("transition", "bad_issue_key", &e);
        } else {
            eprintln!("bwoc jira transition: {e}");
        }
        return EXIT_USAGE;
    }
    let _auth = match require_auth("transition", args.json, &root) {
        Ok(a) => a,
        Err(code) => return code,
    };

    // Write gate.
    if !args.yes {
        if json_write_blocked(args.json, args.yes) {
            eprintln!("bwoc jira transition: --json requires --yes (a write needs explicit ack)");
            return EXIT_USAGE;
        }
        let prompt = format!("Transition {} → '{}'?", args.issue, args.status);
        if !confirm(&prompt) {
            eprintln!("bwoc jira transition: aborted (no write performed)");
            return EXIT_USAGE;
        }
    }

    let plugin = match require_plugin(&root, "transition", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = transition_request(&root, &plugin.dir, &args.issue, &args.status);
    match invoke_jira_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                println!(
                    "bwoc jira transition: {} → '{}' applied",
                    args.issue, args.status
                );
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("transition", "plugin_error", &e);
            } else {
                eprintln!("bwoc jira transition: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

fn run_sync(args: SyncArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc jira sync: {e}");
            return EXIT_USAGE;
        }
    };
    let _auth = match require_auth("sync", args.json, &root) {
        Ok(a) => a,
        Err(code) => return code,
    };

    // The apply path is a gated write; --dry-run is the read-only preview.
    if !args.dry_run && !args.yes {
        if args.json {
            eprintln!(
                "bwoc jira sync: --json requires --yes or --dry-run (apply is a gated write)"
            );
            return EXIT_USAGE;
        }
        if !confirm("Apply the Jira sync plan (writes to the external tracker)?") {
            eprintln!(
                "bwoc jira sync: aborted (no write performed). Re-run with --dry-run to preview."
            );
            return EXIT_USAGE;
        }
    }

    let plugin = match require_plugin(&root, "sync", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = sync_request(&root, &plugin.dir, args.dry_run);
    match invoke_jira_plugin(&plugin, &root, &request) {
        Ok(value) => {
            let conflicts = value
                .get("summary")
                .and_then(|s| s.get("conflict"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let emitted = if args.json {
                print_json(&value)
            } else {
                let s = value.get("summary");
                let get = |k: &str| {
                    s.and_then(|s| s.get(k))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                };
                println!(
                    "bwoc jira sync ({}): push {}, pull {}, no-op {}, conflict {}",
                    if args.dry_run { "dry-run" } else { "applied" },
                    get("push"),
                    get("pull"),
                    get("noop"),
                    conflicts
                );
                true
            };
            if !emitted {
                return EXIT_PLUGIN_ERROR;
            }
            if conflicts > 0 {
                EXIT_CONFLICT
            } else {
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("sync", "plugin_error", &e);
            } else {
                eprintln!("bwoc jira sync: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

// ===========================================================================
// Tests — arg parsing, JSON shapes, the auth-missing path, ledger round-trip,
// and the no-plugin stub path. NO live Jira; NO process-env mutation (auth is
// resolved through an injectable getenv).
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::collections::HashMap;

    /// Test-only `Parser` wrapper so we can exercise `JiraCommand` arg parsing.
    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        cmd: JiraCommand,
    }

    fn parse(args: &[&str]) -> Result<JiraCommand, clap::Error> {
        let mut full = vec!["bwoc-jira-test"];
        full.extend_from_slice(args);
        TestCli::try_parse_from(full).map(|c| c.cmd)
    }

    fn getenv_from(map: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| map.get(k).map(|v| v.to_string())
    }

    // --- .bwoc/secrets.toml resolution (auth.toml option 2) ----------------

    fn write_secrets(dir: &Path, body: &str) -> PathBuf {
        let secrets_dir = dir.join(".bwoc");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        let path = secrets_dir.join("secrets.toml");
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }

    #[test]
    fn secrets_file_reads_jira_table() {
        let dir = tempfile::tempdir().unwrap();
        write_secrets(
            dir.path(),
            "[jira]\nemail = \"a@b.com\"\ntoken = \"t0ken\"\nbase_url = \"https://x.atlassian.net\"\n",
        );
        let env = secrets_file_env(dir.path());
        assert_eq!(env.get(ENV_EMAIL).map(String::as_str), Some("a@b.com"));
        assert_eq!(env.get(ENV_TOKEN).map(String::as_str), Some("t0ken"));
        assert_eq!(
            env.get(ENV_BASE_URL).map(String::as_str),
            Some("https://x.atlassian.net")
        );
        // Resolves cleanly through the live-verb auth gate.
        let auth = resolve_auth(&getenv_with_secrets(dir.path())).unwrap();
        assert_eq!(auth.email, "a@b.com");
        assert_eq!(auth.base_url, "https://x.atlassian.net");
    }

    #[test]
    fn secrets_file_absent_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(secrets_file_env(dir.path()).is_empty());
    }

    #[test]
    fn secrets_file_ignores_empty_values() {
        let dir = tempfile::tempdir().unwrap();
        write_secrets(dir.path(), "[jira]\nemail = \"\"\ntoken = \"t\"\n");
        let env = secrets_file_env(dir.path());
        assert!(!env.contains_key(ENV_EMAIL)); // empty string is not a value
        assert_eq!(env.get(ENV_TOKEN).map(String::as_str), Some("t"));
    }

    #[cfg(unix)]
    #[test]
    fn secrets_file_refuses_group_or_world_accessible() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = write_secrets(dir.path(), "[jira]\ntoken = \"t\"\n");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            secrets_file_env(dir.path()).is_empty(),
            "a group/world-accessible secrets file must be refused"
        );
    }

    // --- arg parsing -------------------------------------------------------

    #[test]
    fn parses_query_with_positional_jql() {
        let cmd = parse(&["query", "project = BWOC AND status = 'In Progress'"]).unwrap();
        match cmd {
            JiraCommand::Query(a) => {
                assert_eq!(a.jql, "project = BWOC AND status = 'In Progress'");
                assert_eq!(a.max_results, 50); // default
                assert_eq!(a.start_at, 0);
                assert!(!a.json);
            }
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[test]
    fn parses_query_pagination_flags() {
        let cmd = parse(&[
            "query",
            "x",
            "--max-results",
            "10",
            "--start-at",
            "20",
            "--json",
        ])
        .unwrap();
        match cmd {
            JiraCommand::Query(a) => {
                assert_eq!(a.max_results, 10);
                assert_eq!(a.start_at, 20);
                assert!(a.json);
            }
            other => panic!("expected Query, got {other:?}"),
        }
    }

    #[test]
    fn parses_transition_two_positionals() {
        let cmd = parse(&["transition", "BWOC-123", "In Progress", "--yes"]).unwrap();
        match cmd {
            JiraCommand::Transition(a) => {
                assert_eq!(a.issue, "BWOC-123");
                assert_eq!(a.status, "In Progress");
                assert!(a.yes);
            }
            other => panic!("expected Transition, got {other:?}"),
        }
    }

    #[test]
    fn parses_link_and_unlink() {
        match parse(&["link", "BWOC-40", "BWOC-123"]).unwrap() {
            JiraCommand::Link(a) => {
                assert_eq!(a.story, "BWOC-40");
                assert_eq!(a.issue, "BWOC-123");
            }
            other => panic!("expected Link, got {other:?}"),
        }
        match parse(&["unlink", "BWOC-40", "--json"]).unwrap() {
            JiraCommand::Unlink(a) => {
                assert_eq!(a.story, "BWOC-40");
                assert!(a.json);
            }
            other => panic!("expected Unlink, got {other:?}"),
        }
    }

    #[test]
    fn parses_sync_dry_run() {
        match parse(&["sync", "--dry-run", "--json"]).unwrap() {
            JiraCommand::Sync(a) => {
                assert!(a.dry_run);
                assert!(a.json);
                assert!(!a.yes);
            }
            other => panic!("expected Sync, got {other:?}"),
        }
    }

    #[test]
    fn query_requires_jql_positional() {
        assert!(parse(&["query"]).is_err());
    }

    #[test]
    fn transition_requires_both_positionals() {
        assert!(parse(&["transition", "BWOC-123"]).is_err());
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(parse(&["frobnicate"]).is_err());
    }

    // --- auth gate (the critical auth-missing path; no env mutation) -------

    #[test]
    fn auth_ok_when_all_three_present() {
        let env = getenv_from(HashMap::from([
            (ENV_EMAIL, "op@example.com"),
            (ENV_TOKEN, "fake-token-not-real"),
            (ENV_BASE_URL, "https://example.atlassian.net"),
        ]));
        let auth = resolve_auth(&env).expect("all present");
        assert_eq!(auth.email, "op@example.com");
        assert_eq!(auth.base_url, "https://example.atlassian.net");
    }

    #[test]
    fn auth_missing_token_reports_only_token() {
        let env = getenv_from(HashMap::from([
            (ENV_EMAIL, "op@example.com"),
            (ENV_BASE_URL, "https://example.atlassian.net"),
        ]));
        let missing = resolve_auth(&env).unwrap_err();
        assert_eq!(missing, vec![ENV_TOKEN]);
    }

    #[test]
    fn auth_missing_all_reports_all_in_stable_order() {
        let env = getenv_from(HashMap::new());
        let missing = resolve_auth(&env).unwrap_err();
        assert_eq!(missing, vec![ENV_EMAIL, ENV_TOKEN, ENV_BASE_URL]);
    }

    #[test]
    fn auth_empty_string_counts_as_missing() {
        let env = getenv_from(HashMap::from([
            (ENV_EMAIL, ""),
            (ENV_TOKEN, "fake-token-not-real"),
            (ENV_BASE_URL, "https://example.atlassian.net"),
        ]));
        let missing = resolve_auth(&env).unwrap_err();
        assert_eq!(missing, vec![ENV_EMAIL]);
    }

    #[test]
    fn auth_struct_never_holds_token() {
        // JiraAuth has exactly two fields, neither named/typed as a token. This
        // test documents the Adinnādāna invariant: the value is discarded.
        let env = getenv_from(HashMap::from([
            (ENV_EMAIL, "op@example.com"),
            (ENV_TOKEN, "super-secret-do-not-store"),
            (ENV_BASE_URL, "https://example.atlassian.net"),
        ]));
        let auth = resolve_auth(&env).unwrap();
        let dbg = format!("{auth:?}");
        assert!(
            !dbg.contains("super-secret-do-not-store"),
            "token value must never appear in JiraAuth: {dbg}"
        );
    }

    // --- auth_status JSON shape (token presence, never the value) ----------

    #[test]
    fn auth_status_configured_omits_token_value() {
        let env = getenv_from(HashMap::from([
            (ENV_EMAIL, "op@example.com"),
            (ENV_TOKEN, "super-secret-do-not-store"),
            (ENV_BASE_URL, "https://example.atlassian.net"),
        ]));
        let st = auth_status(&env);
        assert!(st.configured);
        assert!(st.token_present);
        assert!(st.missing.is_empty());
        let json = serde_json::to_string(&st).unwrap();
        assert!(json.contains("\"token_present\":true"));
        assert!(
            !json.contains("super-secret-do-not-store"),
            "token value leaked into status JSON: {json}"
        );
    }

    #[test]
    fn auth_status_unconfigured_lists_missing() {
        let env = getenv_from(HashMap::new());
        let st = auth_status(&env);
        assert!(!st.configured);
        assert!(!st.token_present);
        assert_eq!(st.missing, vec![ENV_EMAIL, ENV_TOKEN, ENV_BASE_URL]);
    }

    // --- issue key parsing -------------------------------------------------

    #[test]
    fn parses_valid_issue_key() {
        assert_eq!(
            parse_issue_key("BWOC-123").unwrap(),
            ("BWOC".to_string(), "123".to_string())
        );
    }

    #[test]
    fn rejects_issue_key_without_dash() {
        assert!(parse_issue_key("BWOC123").is_err());
    }

    #[test]
    fn rejects_issue_key_non_numeric_suffix() {
        // a scrum epic id like BWOC-EPIC-1 is NOT a valid Jira issue key.
        assert!(parse_issue_key("BWOC-EPIC-1").is_err());
    }

    #[test]
    fn rejects_issue_key_empty_parts() {
        assert!(parse_issue_key("-123").is_err());
        assert!(parse_issue_key("BWOC-").is_err());
    }

    // --- ledger round-trip + schema shape ----------------------------------

    #[test]
    fn pending_entry_omits_optional_fields_never_null() {
        let entry = MappingEntry {
            issue_key: "BWOC-123".to_string(),
            project: "BWOC".to_string(),
            summary: None,
            status: None,
            assignee: None,
            story_points: None,
            parent_epic: None,
            sprint: None,
            last_synced: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        // required key pair present...
        assert!(json.contains("\"issue_key\":\"BWOC-123\""));
        assert!(json.contains("\"project\":\"BWOC\""));
        // ...optional fields omitted, never serialized as null.
        assert!(
            !json.contains("null"),
            "optional fields must be omitted, not null: {json}"
        );
        assert!(!json.contains("summary"));
        assert!(!json.contains("last_synced"));
    }

    #[test]
    fn synced_entry_carries_projection_fields() {
        let entry = MappingEntry {
            issue_key: "BWOC-123".to_string(),
            project: "BWOC".to_string(),
            summary: Some("Declare jira plugin kind".to_string()),
            status: Some("In Progress".to_string()),
            assignee: Some("agent-jisoo@bwoc.local".to_string()),
            story_points: Some(5.0),
            parent_epic: Some("BWOC-100".to_string()),
            sprint: Some("Sprint 6".to_string()),
            last_synced: Some("2026-05-27T10:00:00Z".to_string()),
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["status"], "In Progress");
        assert_eq!(value["last_synced"], "2026-05-27T10:00:00Z");
        // round-trips identically.
        let back: MappingEntry = serde_json::from_value(value).unwrap();
        assert_eq!(back, entry);
    }

    #[test]
    fn ledger_round_trips() {
        let mut ledger = Ledger {
            version: 1,
            site: Some("https://example.atlassian.net".to_string()),
            project: Some("BWOC".to_string()),
            issues: BTreeMap::new(),
        };
        ledger.issues.insert(
            "BWOC-40".to_string(),
            MappingEntry {
                issue_key: "BWOC-123".to_string(),
                project: "BWOC".to_string(),
                summary: None,
                status: None,
                assignee: None,
                story_points: None,
                parent_epic: None,
                sprint: None,
                last_synced: None,
            },
        );
        let json = serde_json::to_string_pretty(&ledger).unwrap();
        let back: Ledger = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ledger);
        // the ledger NEVER carries a token field.
        assert!(
            !json.to_lowercase().contains("token"),
            "ledger must not carry a token: {json}"
        );
    }

    #[test]
    fn load_ledger_absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_ledger(dir.path()).unwrap(), None);
    }

    #[test]
    fn save_then_load_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = Ledger::default();
        ledger.issues.insert(
            "BWOC-40".to_string(),
            MappingEntry {
                issue_key: "BWOC-123".to_string(),
                project: "BWOC".to_string(),
                summary: None,
                status: None,
                assignee: None,
                story_points: None,
                parent_epic: None,
                sprint: None,
                last_synced: None,
            },
        );
        save_ledger(dir.path(), &ledger).unwrap();
        assert!(dir.path().join(LEDGER_REL).is_file());
        let back = load_ledger(dir.path()).unwrap().unwrap();
        assert_eq!(back, ledger);
    }

    // --- request payload shapes (what the CLI hands the plugin) ------------

    #[test]
    fn query_request_shape() {
        let v = query_request(
            Path::new("/ws"),
            Path::new("/ws/modules/plugins/jira"),
            "project = BWOC",
            0,
            50,
        );
        assert_eq!(v["operation"], "query");
        assert_eq!(v["jql"], "project = BWOC");
        assert_eq!(v["start_at"], 0);
        assert_eq!(v["max_results"], 50);
        assert_eq!(v["workspace"], "/ws");
    }

    #[test]
    fn transition_request_shape() {
        let v = transition_request(Path::new("/ws"), Path::new("/p"), "BWOC-123", "Done");
        assert_eq!(v["operation"], "transition");
        assert_eq!(v["issue"], "BWOC-123");
        assert_eq!(v["to_status"], "Done");
    }

    #[test]
    fn sync_request_shape() {
        let v = sync_request(Path::new("/ws"), Path::new("/p"), true);
        assert_eq!(v["operation"], "sync");
        assert_eq!(v["dry_run"], true);
    }

    // --- write gate --------------------------------------------------------

    #[test]
    fn json_write_blocked_without_yes() {
        assert!(json_write_blocked(true, false));
        assert!(!json_write_blocked(true, true));
        assert!(!json_write_blocked(false, false));
    }

    // --- jira plugin discovery / stub-error path ---------------------------

    fn write_workspace(root: &Path, workspace_toml: &str) {
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(root.join(".bwoc/workspace.toml"), workspace_toml).unwrap();
    }

    fn write_plugin(root: &Path, name: &str, kind: &str) {
        let dir = root.join("modules/plugins").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "[plugin]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
                 description = \"x\"\ncompat = \">=2.5.0\"\nentry = \"jira.sh\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn no_plugins_dir_discovers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_jira_plugins(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn discovers_only_jira_kind() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(dir.path(), "jira-cloud-rest", "jira");
        write_plugin(dir.path(), "some-audit", "audit");
        let found = discover_jira_plugins(dir.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "jira-cloud-rest");
        assert_eq!(found[0].entry, "jira.sh");
    }

    #[test]
    fn enabled_jira_plugin_requires_enabled_flag() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin(dir.path(), "jira-cloud-rest", "jira");
        // present but disabled → stub path (None).
        write_workspace(dir.path(), "[plugins.jira-cloud-rest]\nenabled = false\n");
        assert!(find_enabled_jira_plugin(dir.path()).unwrap().is_none());
        // enabled → discovered.
        write_workspace(dir.path(), "[plugins.jira-cloud-rest]\nenabled = true\n");
        let p = find_enabled_jira_plugin(dir.path()).unwrap().unwrap();
        assert_eq!(p.name, "jira-cloud-rest");
    }

    #[test]
    fn no_jira_plugin_when_none_installed() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path(), "");
        assert!(find_enabled_jira_plugin(dir.path()).unwrap().is_none());
    }
}
