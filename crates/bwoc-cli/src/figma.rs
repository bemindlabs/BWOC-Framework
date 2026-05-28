//! `bwoc figma <verb>` — operator-facing CLI surface for the `figma` plugin kind
//! (BWOC-63). Foundation of `BWOC-EPIC-7` (Figma), the final epic of the original
//! roadmap.
//!
//! ## What this is
//!
//! The CLI half of the contract framed in
//! `notes/2026-05-28_figma-plugin-architecture.md` (BWOC-61) and made normative by
//! the **Figma Asset Mapping Schema** in `docs/en/PLUGINS.en.md` (BWOC-62). It owns
//! **argument parsing, workspace + plugin resolution, the token-presence gate, the
//! export-dir resolution, and the JSON shapes** — it does NOT speak to Figma
//! directly. The live REST calls (fetch node metadata, render an image, read
//! component styles) belong to a `figma`-kind plugin — the `figma-rest` reference
//! plugin, BWOC-64, in flight. This CLI discovers an installed `figma` plugin by
//! name and invokes its `[plugin].entry`; when the named plugin is absent the
//! verbs **stub-error gracefully** (exit `4`) rather than panicking.
//!
//! ## Verb table — read-mostly
//!
//! | Verb                                          | Needs token | Needs plugin | Notes                                                          |
//! |---|---|---|---|
//! | `fetch <plugin> --file <key> [--node <id>]`   | yes         | yes          | Frame/node metadata in the Asset Mapping Schema shape.         |
//! | `export <plugin> --file <key> --node <id> [--format png\|svg]` | yes | yes | Render a node image into the content-addressable cache under `figma/exports/`; returns `exported_path`. |
//! | `tokens <plugin> --file <key> [--node <id>]`  | yes         | yes          | Extract design tokens (`{ name: value }`) tied to the node.    |
//! | `status <plugin>`                             | no          | degrades     | Auth state (token present? which scope?) + rate-limit headroom. Never the token value. |
//!
//! ## Auth model — operator personal access token, never echoed
//!
//! Figma's REST API authenticates with a personal access token (PAT) via the
//! `X-Figma-Token` header. The token resolves from the `BWOC_FIGMA_TOKEN`
//! environment variable. **This CLI only checks the token's *presence*** — it
//! never reads, logs, serializes, or forwards the value. The plugin (which
//! inherits this process's environment) reads `BWOC_FIGMA_TOKEN` itself and owns
//! the outbound header. The read verbs (`fetch` / `export` / `tokens`) require the
//! token and exit `2` when it is absent; `status` reports presence without
//! requiring it (that is the point of `status`).
//!
//! ## Read-mostly — no write-back to Figma
//!
//! Every verb either reads Figma (`fetch` / `tokens` / `status`) or writes
//! **locally** (`export` drops a content-addressable image under `figma/exports/`).
//! Nothing writes back to Figma, so — unlike `bwoc jira` / `bwoc gcloud` — there
//! are no operator-confirm gates here: there is no remote write to gate.
//!
//! ## Content-addressable export
//!
//! `export` writes under the plugin's configured export dir (`[plugins.<name>]
//! export_dir` in `workspace.toml`, default `figma/exports`). The CLI resolves
//! that dir and passes it to the plugin so both agree on one location; the plugin
//! owns the content-addressable naming (`SHA-256(file_key + node_id + version +
//! format)` per the design note) and the cache hit/miss decision. The CLI
//! validates the returned `exported_path` is a workspace-relative path under the
//! export dir with no `..` traversal before relaying it.
//!
//! ## Named-plugin invocation, not enabled-gated
//!
//! Each verb names a specific plugin, mirroring `bwoc audit run --plugin <name>` /
//! `bwoc okr <verb> <plugin>`. The gate is therefore **installed**, not
//! **enabled** — an explicitly named plugin runs if it is present on disk with
//! `kind = "figma"`.
//!
//! ## Exit codes — normative
//!
//! - `0` — success.
//! - `1` — local I/O error (e.g. JSON serialization).
//! - `2` — operator/usage error (no workspace, missing `BWOC_FIGMA_TOKEN` for a
//!   read verb, malformed file key / node id). Bad `--format` is a clap parse
//!   error, which also exits `2`.
//! - `4` — the named `figma` plugin is not installed in this workspace
//!   (remediation message names it).
//! - `255` — plugin runtime error (spawn failure, non-JSON output, or an
//!   `exported_path` that escapes the export dir).
//!
//! Passing `--json` makes the exit code redundant: the structured envelope
//! carries `ok`/`error` fields with the same signal.

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Exit codes + plugin kind + env var + defaults (single source of truth).
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_PLUGIN: i32 = 4;
const EXIT_PLUGIN_ERROR: i32 = 255;

const PLUGIN_KIND: &str = "figma";
const ENV_TOKEN: &str = "BWOC_FIGMA_TOKEN";
const DEFAULT_EXPORT_DIR: &str = "figma/exports";

// ---------------------------------------------------------------------------
// CLI surface — defined here so arg parsing is unit-testable against
// `FigmaCommand` directly (see `tests` module).
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum FigmaCommand {
    /// Fetch frame/node metadata in the Figma Asset Mapping Schema shape.
    Fetch(FetchArgs),
    /// Render a node image into the content-addressable cache under the export dir.
    Export(ExportArgs),
    /// Extract design tokens tied to a file or node.
    Tokens(TokensArgs),
    /// Report auth state (token presence + scope) and rate-limit headroom.
    Status(StatusArgs),
}

/// Image export formats Figma's REST API renders. `png` is the default.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[clap(rename_all = "lowercase")]
pub enum ExportFormat {
    Png,
    Svg,
}

impl ExportFormat {
    fn as_str(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Svg => "svg",
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Args, Debug)]
pub struct FetchArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// The Figma file key (from the file URL).
    #[arg(long = "file")]
    file: String,
    /// A specific node id (frame / component / …). Omitted = whole-file metadata.
    #[arg(long = "node")]
    node: Option<String>,
    /// Workspace root. Resolution: --workspace > BWOC_WORKSPACE env > ancestor walk > cwd.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// The Figma file key (from the file URL).
    #[arg(long = "file")]
    file: String,
    /// The node id to export. Required — you export a specific node, not a file.
    #[arg(long = "node")]
    node: String,
    /// Image format to render. `png` (default) or `svg`.
    #[arg(long, value_enum, default_value_t = ExportFormat::Png)]
    format: ExportFormat,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct TokensArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// The Figma file key (from the file URL).
    #[arg(long = "file")]
    file: String,
    /// A specific node id to scope token extraction to. Omitted = whole file.
    #[arg(long = "node")]
    node: Option<String>,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Plugin name (directory under `modules/plugins/`).
    plugin: String,
    /// Workspace root.
    #[arg(long = "workspace")]
    workspace: Option<PathBuf>,
    /// Emit the structured envelope instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

/// Dispatch a parsed `FigmaCommand`. Returns the process exit code.
pub fn run(cmd: FigmaCommand) -> i32 {
    match cmd {
        FigmaCommand::Fetch(a) => run_fetch(a),
        FigmaCommand::Export(a) => run_export(a),
        FigmaCommand::Tokens(a) => run_tokens(a),
        FigmaCommand::Status(a) => run_status(a),
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution — same shape as okr.rs / gcloud.rs / jira.rs / audit.rs.
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
// Token shape — PRESENCE ONLY. The value is never read, stored, or serialized
// (Sīla — Adinnādāna). The plugin reads `BWOC_FIGMA_TOKEN` from the inherited
// environment itself; this CLI only answers "is it set?".
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum TokenSource {
    Env,
    None,
}

impl TokenSource {
    fn as_str(self) -> &'static str {
        match self {
            TokenSource::Env => "env",
            TokenSource::None => "none",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct TokenShape {
    /// Whether `BWOC_FIGMA_TOKEN` is set to a non-empty value.
    present: bool,
    /// Which source resolved the token (only `env` is supported today).
    source: TokenSource,
}

fn probe_token_shape(getenv: &dyn Fn(&str) -> Option<String>) -> TokenShape {
    let present = getenv(ENV_TOKEN).map(|v| !v.is_empty()).unwrap_or(false);
    TokenShape {
        present,
        source: if present {
            TokenSource::Env
        } else {
            TokenSource::None
        },
    }
}

fn real_getenv(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Gate a read verb on token presence. Returns `Err(exit_code)` (after emitting
/// the message) when the token is absent.
fn require_token(verb: &str, json: bool) -> Result<(), i32> {
    if probe_token_shape(&real_getenv).present {
        return Ok(());
    }
    let msg = format!(
        "{ENV_TOKEN} is not set. Export a Figma personal access token \
         (https://www.figma.com/developers/api#access-tokens) into {ENV_TOKEN} \
         before running read verbs. `bwoc figma status {{plugin}}` reports token \
         presence without requiring it."
    );
    if json {
        emit_error_json(verb, "no_token", &msg);
    } else {
        eprintln!("bwoc figma {verb}: {msg}");
    }
    Err(EXIT_USAGE)
}

// ---------------------------------------------------------------------------
// Input validation — cheap local pre-checks so we never spawn the plugin for
// obvious junk. The plugin re-validates against the live API.
// ---------------------------------------------------------------------------

/// Figma file keys are URL-safe alphanumeric tokens. 1–128 chars; letters,
/// digits, `-`, `_`.
fn is_valid_file_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Figma node ids look like `12:345` (and variants with `-`/`;`/`_`). Reject
/// empty, over-long, or whitespace/control-bearing values.
fn is_valid_node_id(node: &str) -> bool {
    !node.is_empty()
        && node.len() <= 64
        && node
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b';' | b'_'))
}

// ---------------------------------------------------------------------------
// Export-dir resolution + returned-path safety.
// ---------------------------------------------------------------------------

/// Resolve the export dir from `[plugins.<name>] export_dir` in `workspace.toml`,
/// defaulting to `figma/exports`. Best-effort: any read/parse failure falls back
/// to the default (the plugin applies the same default, so they stay aligned).
fn resolve_export_dir(root: &Path, plugin_name: &str) -> String {
    let read = || -> Option<String> {
        let body = std::fs::read_to_string(root.join(".bwoc/workspace.toml")).ok()?;
        let value: toml::Value = toml::from_str(&body).ok()?;
        value
            .get("plugins")?
            .get(plugin_name)?
            .get("export_dir")?
            .as_str()
            .map(|s| s.trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
    };
    read().unwrap_or_else(|| DEFAULT_EXPORT_DIR.to_string())
}

/// A plugin's returned `exported_path` MUST be a workspace-relative path under the
/// export dir with no `..` traversal — otherwise a misbehaving plugin could point
/// the operator at an arbitrary file. Validated before we relay it.
fn validate_exported_path(export_dir: &str, exported: &str) -> Result<(), String> {
    if exported.is_empty() {
        return Err("plugin returned an empty exported_path".to_string());
    }
    if Path::new(exported).is_absolute() || exported.contains('\\') {
        return Err(format!(
            "exported_path '{exported}' is not a forward-slash workspace-relative path"
        ));
    }
    if exported.split('/').any(|c| c == "..") {
        return Err(format!(
            "exported_path '{exported}' contains a '..' traversal component"
        ));
    }
    let prefix = format!("{}/", export_dir.trim_end_matches('/'));
    if !exported.starts_with(&prefix) {
        return Err(format!(
            "exported_path '{exported}' is not under the export dir '{export_dir}/'"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plugin discovery — finds `figma`-kind plugins by name + kind. Checks both the
// flat layout (`modules/plugins/<name>/`) and the kind-namespaced layout
// (`modules/plugins/figma/<name>/`) so the CLI works regardless of which layout
// BWOC-64 ships with (mirrors okr.rs / gcloud.rs).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct ManifestRaw {
    plugin: PluginSection,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PluginSection {
    name: String,
    kind: String,
    entry: String,
}

#[derive(Debug, Clone, PartialEq)]
struct FigmaPlugin {
    name: String,
    dir: PathBuf,
    entry: String,
}

/// Try the two known plugin layouts in order — flat, then `figma/`-namespaced.
fn candidate_plugin_dirs(root: &Path, name: &str) -> [PathBuf; 2] {
    [
        root.join("modules/plugins").join(name),
        root.join("modules/plugins/figma").join(name),
    ]
}

/// Find a `figma`-kind plugin by name across both layouts. Returns `None` when no
/// manifest matches; returns `Err` on parse failure or a kind mismatch (the
/// plugin *exists* but is malformed/misconfigured — surface, don't degrade).
fn discover_plugin(root: &Path, name: &str) -> Result<Option<FigmaPlugin>, String> {
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
        return Ok(Some(FigmaPlugin {
            name: parsed.plugin.name,
            dir: plugin_dir,
            entry: parsed.plugin.entry,
        }));
    }
    Ok(None)
}

/// Resolve a named plugin for a verb invocation. The gate is **installed**, not
/// **enabled** — naming a plugin explicitly is the intent (mirrors
/// `bwoc audit run --plugin`). Maps the absence path to a clean exit `4`.
fn require_plugin(root: &Path, name: &str, verb: &str, json: bool) -> Result<FigmaPlugin, i32> {
    match discover_plugin(root, name) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => {
            let msg = no_plugin_message(name);
            if json {
                emit_error_json(verb, "no_plugin", &msg);
            } else {
                eprintln!("bwoc figma {verb}: {msg}");
            }
            Err(EXIT_NO_PLUGIN)
        }
        Err(e) => {
            if json {
                emit_error_json(verb, "discovery_error", &e);
            } else {
                eprintln!("bwoc figma {verb}: {e}");
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
// Plugin invocation — same shape as okr.rs::invoke_plugin / gcloud.rs / audit.rs.
// The token is NEVER placed in the request; the plugin inherits BWOC_FIGMA_TOKEN
// from this process's environment and reads it itself.
// ---------------------------------------------------------------------------

fn invoke_plugin(
    plugin: &FigmaPlugin,
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
        .env("BWOC_FIGMA_OPERATION", operation)
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
// Request payloads handed to the plugin over stdin (one per verb). NONE carry
// the token value.
// ---------------------------------------------------------------------------

fn fetch_request(
    workspace: &Path,
    plugin_dir: &Path,
    file_key: &str,
    node_id: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "fetch",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "file_key": file_key,
        "node_id": node_id,
    })
}

fn export_request(
    workspace: &Path,
    plugin_dir: &Path,
    file_key: &str,
    node_id: &str,
    format: ExportFormat,
    export_dir: &str,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "export",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "file_key": file_key,
        "node_id": node_id,
        "format": format.as_str(),
        "export_dir": export_dir,
    })
}

fn tokens_request(
    workspace: &Path,
    plugin_dir: &Path,
    file_key: &str,
    node_id: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "operation": "tokens",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
        "file_key": file_key,
        "node_id": node_id,
    })
}

fn status_request(workspace: &Path, plugin_dir: &Path) -> serde_json::Value {
    serde_json::json!({
        "operation": "status",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin_dir.display().to_string(),
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
            eprintln!("bwoc figma: serialize JSON: {e}");
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
        "no installed '{plugin_name}' plugin (figma kind) in this workspace. \
         The live Figma REST path is provided by a `figma`-kind plugin such as \
         `figma-rest` (see the EPIC-7 design note). Install it (BWOC-64) with \
         `bwoc plugin install <source>` then `bwoc plugin enable {plugin_name}`."
    )
}

/// Pull the asset entries out of whatever envelope the plugin emits: a bare JSON
/// array of schema entries, `{ "entries": [...] }`, or a single entry object.
fn entries_of(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(arr) = value.as_array() {
        return arr.clone();
    }
    if let Some(arr) = value.get("entries").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    if value.is_object() {
        return vec![value.clone()];
    }
    Vec::new()
}

fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("?")
}

// ---------------------------------------------------------------------------
// Verb implementations.
// ---------------------------------------------------------------------------

fn run_fetch(args: FetchArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc figma fetch: {e}");
            return EXIT_USAGE;
        }
    };
    if let Err(code) = validate_file_and_node(&args.file, args.node.as_deref(), "fetch", args.json)
    {
        return code;
    }
    if let Err(code) = require_token("fetch", args.json) {
        return code;
    }
    let plugin = match require_plugin(&root, &args.plugin, "fetch", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = fetch_request(&root, &plugin.dir, &args.file, args.node.as_deref());
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                // Emit the Asset Mapping Schema output as the plugin produced it
                // — the schema is the contract.
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let entries = entries_of(&value);
                println!(
                    "bwoc figma fetch: {} — {} node(s)",
                    args.file,
                    entries.len()
                );
                for e in &entries {
                    println!(
                        "  {} [{}] — {} (last_modified={})",
                        str_field(e, "node_id"),
                        str_field(e, "type"),
                        str_field(e, "name"),
                        str_field(e, "last_modified"),
                    );
                }
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("fetch", "plugin_error", &e);
            } else {
                eprintln!("bwoc figma fetch: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

fn run_export(args: ExportArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc figma export: {e}");
            return EXIT_USAGE;
        }
    };
    if let Err(code) = validate_file_and_node(&args.file, Some(&args.node), "export", args.json) {
        return code;
    }
    if let Err(code) = require_token("export", args.json) {
        return code;
    }
    let plugin = match require_plugin(&root, &args.plugin, "export", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let export_dir = resolve_export_dir(&root, &plugin.name);
    let request = export_request(
        &root,
        &plugin.dir,
        &args.file,
        &args.node,
        args.format,
        &export_dir,
    );
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            // Validate the content-addressable artifact lives under the export dir
            // before we relay it (a misbehaving plugin must not point elsewhere).
            let exported = value
                .get("exported_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Err(e) = validate_exported_path(&export_dir, exported) {
                if args.json {
                    emit_error_json("export", "bad_exported_path", &e);
                } else {
                    eprintln!("bwoc figma export: {e}");
                }
                return EXIT_PLUGIN_ERROR;
            }
            if args.json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let cached = value
                    .get("cached")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                println!(
                    "bwoc figma export: {} → {} ({}, cached={cached})",
                    args.node, exported, args.format,
                );
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("export", "plugin_error", &e);
            } else {
                eprintln!("bwoc figma export: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

fn run_tokens(args: TokensArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc figma tokens: {e}");
            return EXIT_USAGE;
        }
    };
    if let Err(code) = validate_file_and_node(&args.file, args.node.as_deref(), "tokens", args.json)
    {
        return code;
    }
    if let Err(code) = require_token("tokens", args.json) {
        return code;
    }
    let plugin = match require_plugin(&root, &args.plugin, "tokens", args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = tokens_request(&root, &plugin.dir, &args.file, args.node.as_deref());
    match invoke_plugin(&plugin, &root, &request) {
        Ok(value) => {
            if args.json {
                if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                }
            } else {
                let tokens = design_tokens_of(&value);
                println!(
                    "bwoc figma tokens: {} — {} token(s)",
                    args.file,
                    tokens.len()
                );
                for (name, val) in &tokens {
                    println!("  {name}: {val}");
                }
                EXIT_OK
            }
        }
        Err(e) => {
            if args.json {
                emit_error_json("tokens", "plugin_error", &e);
            } else {
                eprintln!("bwoc figma tokens: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

/// `bwoc figma status <plugin>` — auth + reachability view. **Degrades when the
/// plugin is missing:** reports the offline token shape alone and notes the
/// plugin is absent. Always exits `0` unless the workspace can't be resolved —
/// the absence of a plugin or token is a reportable condition, not an error
/// (reporting token presence is the whole point of `status`).
fn run_status(args: StatusArgs) -> i32 {
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc figma status: {e}");
            return EXIT_USAGE;
        }
    };
    let token = probe_token_shape(&real_getenv);

    let (live, state) = match discover_plugin(&root, &args.plugin) {
        Ok(Some(p)) => {
            let req = status_request(&root, &p.dir);
            match invoke_plugin(&p, &root, &req) {
                Ok(v) => (Some(v), "ok"),
                Err(_) => (None, "plugin_error"),
            }
        }
        Ok(None) => (None, "not_installed"),
        Err(_) => (None, "discovery_error"),
    };

    if args.json {
        let value = serde_json::json!({
            "ok": true,
            "workspace": root.display().to_string(),
            "plugin": args.plugin,
            "token": token,
            "figma": { "state": state, "live": live },
        });
        return if print_json(&value) {
            EXIT_OK
        } else {
            EXIT_LOCAL_ERROR
        };
    }

    println!("bwoc figma status — workspace: {}", root.display());
    println!(
        "  token: present={}, source={}",
        token.present,
        token.source.as_str()
    );
    match (state, live.as_ref()) {
        ("ok", Some(v)) => {
            let scope = str_field(v, "scope");
            let remaining = v
                .get("rate_limit_remaining")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "?".to_string());
            let limit = v
                .get("rate_limit_limit")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "?".to_string());
            println!(
                "  plugin '{}': ok (scope={scope}, rate_limit={remaining}/{limit})",
                args.plugin
            );
        }
        ("not_installed", _) => {
            println!("  plugin '{}': not installed", args.plugin)
        }
        (other, _) => println!("  plugin '{}': {other}", args.plugin),
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------
// Verb-shared validation + extraction.
// ---------------------------------------------------------------------------

fn validate_file_and_node(
    file: &str,
    node: Option<&str>,
    verb: &str,
    json: bool,
) -> Result<(), i32> {
    if !is_valid_file_key(file) {
        let msg = format!(
            "invalid file key '{file}' — expected 1–128 chars of letters, digits, \
             '-' or '_' (the key from the Figma file URL)"
        );
        if json {
            emit_error_json(verb, "bad_file_key", &msg);
        } else {
            eprintln!("bwoc figma {verb}: {msg}");
        }
        return Err(EXIT_USAGE);
    }
    if let Some(n) = node {
        if !is_valid_node_id(n) {
            let msg = format!(
                "invalid node id '{n}' — expected a Figma node id such as '12:345' \
                 (alphanumerics and ':' '-' ';' '_', up to 64 chars)"
            );
            if json {
                emit_error_json(verb, "bad_node_id", &msg);
            } else {
                eprintln!("bwoc figma {verb}: {msg}");
            }
            return Err(EXIT_USAGE);
        }
    }
    Ok(())
}

/// Extract the `design_tokens` object from a plugin response — from the top-level
/// or, failing that, from the first asset entry that carries one — as sorted
/// `(name, value)` pairs for stable human output.
fn design_tokens_of(value: &serde_json::Value) -> Vec<(String, String)> {
    let map = match value.get("design_tokens").and_then(|v| v.as_object()) {
        Some(m) => m.clone(),
        None => entries_of(value)
            .into_iter()
            .find_map(|e| e.get("design_tokens").and_then(|v| v.as_object()).cloned())
            .unwrap_or_default(),
    };
    let mut pairs: Vec<(String, String)> = map
        .into_iter()
        .map(|(k, v)| {
            let val = v
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string());
            (k, val)
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

// ===========================================================================
// Tests — arg parsing, validation, token shape (never leaks), export-dir
// resolution, exported-path safety, discovery (both layouts), no-plugin stub
// path, request payload shapes (no token), token/entry extraction.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::collections::HashMap;

    #[derive(Parser, Debug)]
    struct TestCli {
        #[command(subcommand)]
        cmd: FigmaCommand,
    }

    fn parse(args: &[&str]) -> Result<FigmaCommand, clap::Error> {
        let mut full = vec!["bwoc-figma-test"];
        full.extend_from_slice(args);
        TestCli::try_parse_from(full).map(|c| c.cmd)
    }

    fn getenv_from(map: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |k: &str| map.get(k).map(|v| v.to_string())
    }

    // --- arg parsing -------------------------------------------------------

    #[test]
    fn parses_fetch_with_node() {
        match parse(&[
            "fetch",
            "figma-rest",
            "--file",
            "AbC123",
            "--node",
            "12:345",
            "--json",
        ])
        .unwrap()
        {
            FigmaCommand::Fetch(a) => {
                assert_eq!(a.plugin, "figma-rest");
                assert_eq!(a.file, "AbC123");
                assert_eq!(a.node.as_deref(), Some("12:345"));
                assert!(a.json);
            }
            other => panic!("expected Fetch, got {other:?}"),
        }
    }

    #[test]
    fn parses_fetch_without_node() {
        match parse(&["fetch", "figma-rest", "--file", "AbC123"]).unwrap() {
            FigmaCommand::Fetch(a) => assert!(a.node.is_none()),
            other => panic!("expected Fetch, got {other:?}"),
        }
    }

    #[test]
    fn parses_export_defaults_to_png() {
        match parse(&[
            "export",
            "figma-rest",
            "--file",
            "AbC123",
            "--node",
            "12:345",
        ])
        .unwrap()
        {
            FigmaCommand::Export(a) => {
                assert_eq!(a.format, ExportFormat::Png);
                assert_eq!(a.node, "12:345");
            }
            other => panic!("expected Export, got {other:?}"),
        }
    }

    #[test]
    fn parses_export_svg() {
        match parse(&[
            "export",
            "figma-rest",
            "--file",
            "AbC123",
            "--node",
            "12:345",
            "--format",
            "svg",
        ])
        .unwrap()
        {
            FigmaCommand::Export(a) => assert_eq!(a.format, ExportFormat::Svg),
            other => panic!("expected Export, got {other:?}"),
        }
    }

    #[test]
    fn export_rejects_unknown_format() {
        assert!(
            parse(&[
                "export",
                "figma-rest",
                "--file",
                "AbC123",
                "--node",
                "12:345",
                "--format",
                "gif",
            ])
            .is_err()
        );
    }

    #[test]
    fn export_requires_file_and_node() {
        assert!(parse(&["export", "figma-rest", "--file", "AbC123"]).is_err());
        assert!(parse(&["export", "figma-rest", "--node", "12:345"]).is_err());
    }

    #[test]
    fn parses_tokens_and_status() {
        match parse(&["tokens", "figma-rest", "--file", "AbC123", "--json"]).unwrap() {
            FigmaCommand::Tokens(a) => assert!(a.json),
            other => panic!("expected Tokens, got {other:?}"),
        }
        match parse(&["status", "figma-rest"]).unwrap() {
            FigmaCommand::Status(a) => assert_eq!(a.plugin, "figma-rest"),
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn verbs_require_plugin_name_and_file() {
        assert!(parse(&["fetch"]).is_err());
        assert!(parse(&["fetch", "figma-rest"]).is_err()); // missing --file
        assert!(parse(&["tokens"]).is_err());
        assert!(parse(&["status"]).is_err());
    }

    #[test]
    fn rejects_unknown_subcommand() {
        assert!(parse(&["frobnicate"]).is_err());
    }

    // --- format ------------------------------------------------------------

    #[test]
    fn export_format_as_str() {
        assert_eq!(ExportFormat::Png.as_str(), "png");
        assert_eq!(ExportFormat::Svg.as_str(), "svg");
        assert_eq!(ExportFormat::Png.to_string(), "png");
    }

    // --- input validation --------------------------------------------------

    #[test]
    fn accepts_valid_file_keys() {
        assert!(is_valid_file_key("AbC123dEf456"));
        assert!(is_valid_file_key("a"));
        assert!(is_valid_file_key("file-key_123"));
    }

    #[test]
    fn rejects_invalid_file_keys() {
        assert!(!is_valid_file_key(""));
        assert!(!is_valid_file_key("has space"));
        assert!(!is_valid_file_key("slash/no"));
        assert!(!is_valid_file_key(&"a".repeat(129)));
    }

    #[test]
    fn accepts_valid_node_ids() {
        assert!(is_valid_node_id("12:345"));
        assert!(is_valid_node_id("0:1"));
        assert!(is_valid_node_id("I12:34;56-7"));
    }

    #[test]
    fn rejects_invalid_node_ids() {
        assert!(!is_valid_node_id(""));
        assert!(!is_valid_node_id("12 345"));
        assert!(!is_valid_node_id("node/slash"));
        assert!(!is_valid_node_id(&"1".repeat(65)));
    }

    // --- token shape (presence only, never leaks the value) ----------------

    #[test]
    fn token_present_when_env_set_nonempty() {
        let env = getenv_from(HashMap::from([(ENV_TOKEN, "figd_super-secret-value")]));
        let shape = probe_token_shape(&env);
        assert!(shape.present);
        assert_eq!(shape.source, TokenSource::Env);
    }

    #[test]
    fn token_absent_when_env_empty_or_missing() {
        let empty = getenv_from(HashMap::from([(ENV_TOKEN, "")]));
        assert!(!probe_token_shape(&empty).present);
        let missing = getenv_from(HashMap::new());
        let shape = probe_token_shape(&missing);
        assert!(!shape.present);
        assert_eq!(shape.source, TokenSource::None);
    }

    #[test]
    fn token_shape_never_serializes_the_value() {
        let env = getenv_from(HashMap::from([(ENV_TOKEN, "figd_super-secret-value")]));
        let shape = probe_token_shape(&env);
        let json = serde_json::to_string(&shape).unwrap();
        assert!(
            !json.contains("super-secret"),
            "token shape leaked value: {json}"
        );
        assert!(!json.contains("figd_"), "token shape leaked value: {json}");
        assert!(json.contains("\"present\":true"), "{json}");
        assert!(json.contains("\"source\":\"env\""), "{json}");
    }

    // --- export-dir resolution ---------------------------------------------

    fn write_workspace(root: &Path, workspace_toml: &str) {
        std::fs::create_dir_all(root.join(".bwoc")).unwrap();
        std::fs::write(root.join(".bwoc/workspace.toml"), workspace_toml).unwrap();
    }

    #[test]
    fn export_dir_defaults_when_unset() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(dir.path(), "[plugins.figma-rest]\nenabled = true\n");
        assert_eq!(
            resolve_export_dir(dir.path(), "figma-rest"),
            "figma/exports"
        );
        // No workspace.toml at all also falls back.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_export_dir(empty.path(), "figma-rest"),
            "figma/exports"
        );
    }

    #[test]
    fn export_dir_reads_override() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(
            dir.path(),
            "[plugins.figma-rest]\nenabled = true\nexport_dir = \"assets/figma\"\n",
        );
        assert_eq!(resolve_export_dir(dir.path(), "figma-rest"), "assets/figma");
    }

    #[test]
    fn export_dir_strips_trailing_slash() {
        let dir = tempfile::tempdir().unwrap();
        write_workspace(
            dir.path(),
            "[plugins.figma-rest]\nexport_dir = \"assets/figma/\"\n",
        );
        assert_eq!(resolve_export_dir(dir.path(), "figma-rest"), "assets/figma");
    }

    // --- exported-path safety ----------------------------------------------

    #[test]
    fn accepts_path_under_export_dir() {
        assert!(validate_exported_path("figma/exports", "figma/exports/9f86d081.png").is_ok());
    }

    #[test]
    fn rejects_unsafe_exported_paths() {
        assert!(validate_exported_path("figma/exports", "").is_err());
        assert!(validate_exported_path("figma/exports", "/etc/passwd").is_err());
        assert!(validate_exported_path("figma/exports", "figma/exports/../../etc/passwd").is_err());
        assert!(validate_exported_path("figma/exports", "other/dir/x.png").is_err());
        assert!(validate_exported_path("figma/exports", "figma\\exports\\x.png").is_err());
    }

    // --- plugin discovery (both layouts) -----------------------------------

    fn write_plugin_at(root: &Path, layout: &str, name: &str, kind: &str) {
        let dir = root.join("modules/plugins").join(layout).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "[plugin]\nname = \"{name}\"\nkind = \"{kind}\"\nversion = \"0.1.0\"\n\
                 description = \"figma adapter\"\ncompat = \">=2.10.0\"\nentry = \"figma.sh\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn no_plugins_dir_discovers_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover_plugin(dir.path(), "figma-rest").unwrap().is_none());
    }

    #[test]
    fn discovers_flat_layout() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", "figma-rest", "figma");
        let p = discover_plugin(dir.path(), "figma-rest").unwrap().unwrap();
        assert_eq!(p.name, "figma-rest");
        assert_eq!(p.entry, "figma.sh");
    }

    #[test]
    fn discovers_figma_namespaced_layout() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "figma", "figma-rest", "figma");
        let p = discover_plugin(dir.path(), "figma-rest").unwrap().unwrap();
        assert_eq!(p.name, "figma-rest");
    }

    #[test]
    fn discovery_rejects_wrong_kind() {
        let dir = tempfile::tempdir().unwrap();
        write_plugin_at(dir.path(), "", "figma-rest", "audit");
        let err = discover_plugin(dir.path(), "figma-rest").unwrap_err();
        assert!(err.contains("expected"), "{err}");
        assert!(err.contains("figma"), "{err}");
    }

    #[test]
    fn no_plugin_message_names_install_command() {
        let m = no_plugin_message("figma-rest");
        assert!(m.contains("figma-rest"));
        assert!(m.contains("bwoc plugin install"));
        assert!(m.contains("bwoc plugin enable"));
    }

    // --- request payload shapes (what the CLI hands the plugin) ------------

    #[test]
    fn fetch_request_shape_and_no_token() {
        let v = fetch_request(Path::new("/ws"), Path::new("/p"), "AbC123", Some("12:345"));
        assert_eq!(v["operation"], "fetch");
        assert_eq!(v["workspace"], "/ws");
        assert_eq!(v["plugin_dir"], "/p");
        assert_eq!(v["file_key"], "AbC123");
        assert_eq!(v["node_id"], "12:345");
        // The token MUST NEVER be forwarded — the plugin reads the env itself.
        let serialized = v.to_string();
        assert!(
            !serialized.contains(ENV_TOKEN),
            "request leaked token env: {serialized}"
        );
        assert!(
            !serialized.to_lowercase().contains("token"),
            "request mentions token: {serialized}"
        );
    }

    #[test]
    fn fetch_request_null_node_when_absent() {
        let v = fetch_request(Path::new("/ws"), Path::new("/p"), "AbC123", None);
        assert!(v["node_id"].is_null());
    }

    #[test]
    fn export_request_carries_format_and_export_dir() {
        let v = export_request(
            Path::new("/ws"),
            Path::new("/p"),
            "AbC123",
            "12:345",
            ExportFormat::Svg,
            "figma/exports",
        );
        assert_eq!(v["operation"], "export");
        assert_eq!(v["format"], "svg");
        assert_eq!(v["export_dir"], "figma/exports");
        assert_eq!(v["node_id"], "12:345");
        let serialized = v.to_string();
        assert!(
            !serialized.contains(ENV_TOKEN),
            "request leaked token env: {serialized}"
        );
    }

    #[test]
    fn tokens_and_status_request_shapes() {
        let t = tokens_request(Path::new("/ws"), Path::new("/p"), "AbC123", None);
        assert_eq!(t["operation"], "tokens");
        assert!(t["node_id"].is_null());
        let s = status_request(Path::new("/ws"), Path::new("/p"));
        assert_eq!(s["operation"], "status");
        assert_eq!(s["workspace"], "/ws");
    }

    // --- response extraction helpers ---------------------------------------

    #[test]
    fn entries_of_handles_array_envelope_and_single() {
        assert_eq!(
            entries_of(&serde_json::json!([{ "node_id": "1:2" }])).len(),
            1
        );
        assert_eq!(
            entries_of(
                &serde_json::json!({ "entries": [{ "node_id": "1:2" }, { "node_id": "3:4" }] })
            )
            .len(),
            2
        );
        // A single bare entry object counts as one entry.
        assert_eq!(
            entries_of(&serde_json::json!({ "node_id": "1:2", "name": "X" })).len(),
            1
        );
    }

    #[test]
    fn design_tokens_top_level_sorted() {
        let v = serde_json::json!({
            "design_tokens": { "radius/sm": "4px", "color/primary": "#2D7FF9" }
        });
        let pairs = design_tokens_of(&v);
        assert_eq!(pairs.len(), 2);
        // Sorted by name.
        assert_eq!(pairs[0].0, "color/primary");
        assert_eq!(pairs[0].1, "#2D7FF9");
        assert_eq!(pairs[1].0, "radius/sm");
    }

    #[test]
    fn design_tokens_from_first_entry() {
        let v = serde_json::json!({
            "entries": [
                { "node_id": "1:2", "design_tokens": { "color/bg": "#fff" } }
            ]
        });
        let pairs = design_tokens_of(&v);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "color/bg");
    }

    #[test]
    fn design_tokens_empty_when_absent() {
        let v = serde_json::json!({ "node_id": "1:2" });
        assert!(design_tokens_of(&v).is_empty());
    }
}
