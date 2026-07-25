//! `bwoc accounting` — the gated CLI front for the `workflow/accounting-api`
//! plugin (Bemind Accounting Open API).
//!
//! Split by side-effect, same shape as `gcloud.rs` / `gws.rs`:
//!
//! | Verb                       | Plugin op        | Tier                                    |
//! |----------------------------|------------------|-----------------------------------------|
//! | `report <name>`            | `report`         | READ (T0) — free                        |
//! | `bill create`              | `bill-create`    | WRITE — financial, GL-posting           |
//! | `bill update <id>`         | `bill-update`    | WRITE — financial, GL-posting           |
//! | `expense create`           | `expense-create` | WRITE — financial, GL-posting           |
//!
//! Financial writes are the framework's highest-consequence class: each posts a
//! durable document **and** an auto double-entry GL entry on an external system
//! of record. So they carry the IAM-grade double gate (EPIC-12 shape):
//!   1. a standing `[plugins.accounting-api] writes_enabled = true` opt-in in
//!      `.bwoc/workspace.toml` (refuse-by-default when absent), and
//!   2. a per-write operator confirmation echoing the resolved target — or
//!      `--yes` to ack up front (required in `--json` mode).
//!
//! The plugin itself holds **no** gate — it executes when invoked. This CLI is
//! the single choke point, so a write can never reach the API un-gated.
//!
//! Exit codes mirror the other plugin CLIs (0/1/2/4/255):
//!   - `0` — success
//!   - `1` — reserved for a pure local I/O error (parity with gcloud/gws; not currently emitted here)
//!   - `2` — operator/usage error (bad args; gated write without opt-in/ack; unreadable/malformed config)
//!   - `4` — plugin not installed or disabled
//!   - `255` — plugin discovery or runtime error (malformed manifest, spawn failure, non-JSON output)
//!
//! `--json` makes the exit code redundant: the envelope carries `ok`/`error`.

use clap::{Args, Subcommand};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Exit codes + plugin name + env (single source of truth).
// ---------------------------------------------------------------------------

const EXIT_OK: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_PLUGIN: i32 = 4;
const EXIT_PLUGIN_ERROR: i32 = 255;

const PLUGIN_ACCOUNTING: &str = "accounting-api";
const PLUGIN_KIND: &str = "workflow";

/// Report names the API exposes under `/reports/<name>` (v2.3.2). Kept in sync
/// with the plugin's `$REPORTS` — this is the local pre-check so we never spawn
/// the plugin for an obviously-unknown report; the plugin re-validates.
const REPORTS: &[&str] = &[
    "pnl",
    "balance-sheet",
    "cashflow",
    "trial-balance",
    "vat",
    "wht",
    "ap-aging",
    "ar-aging",
    "expenses",
    "sales-by-channel",
    "mrr",
    "product-margin",
    "asset-register",
];

/// Purchase-document types the `bill create` verb accepts (POST /purchase-docs).
const BILL_TYPES: &[&str] = &["bill", "purchase_order", "goods_receipt"];

// ---------------------------------------------------------------------------
// CLI surface — own arg structs so parsing is unit-testable against
// `AccountingCommand` directly (see `tests`).
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum AccountingCommand {
    /// Read a financial report — GET /reports/<name>. READ (T0), free.
    Report(ReportArgs),
    /// Purchase-document (bill) operations. WRITE — financial, GL-posting.
    #[command(subcommand)]
    Bill(BillCommand),
    /// Expense operations. WRITE — financial, GL-posting.
    #[command(subcommand)]
    Expense(ExpenseCommand),
}

#[derive(Subcommand, Debug)]
pub enum BillCommand {
    /// Create a draft purchase document — POST /purchase-docs {type}. WRITE.
    Create(BillCreateArgs),
    /// Fill/finalize a purchase document — PATCH /purchase-docs/{id}. WRITE.
    Update(BillUpdateArgs),
}

#[derive(Subcommand, Debug)]
pub enum ExpenseCommand {
    /// Record an expense — POST /expenses {payload}. WRITE.
    Create(ExpenseCreateArgs),
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Report name (e.g. `pnl`, `balance-sheet`, `vat`). See `--help` list.
    pub report: String,
    /// Optional query params as a single JSON object, e.g.
    /// `--params '{"from":"2026-01-01","to":"2026-03-31"}'`.
    #[arg(long)]
    pub params: Option<String>,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    /// Emit the structured JSON envelope instead of a human summary.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct BillCreateArgs {
    /// Document type — one of `bill`, `purchase_order`, `goods_receipt`.
    #[arg(long = "type", default_value = "bill")]
    pub doc_type: String,
    /// Acknowledge the write up front (required in `--json` mode; still fenced
    /// by the workspace `writes_enabled` opt-in).
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct BillUpdateArgs {
    /// Document id to fill/finalize (e.g. `PI-123`).
    pub document_id: String,
    /// The document body as a JSON object (date, supplier, items, vat, …).
    #[arg(long)]
    pub payload: String,
    /// Acknowledge the write up front (required in `--json` mode).
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ExpenseCreateArgs {
    /// The expense body as a JSON object (date, description, amount, …).
    #[arg(long)]
    pub payload: String,
    /// Acknowledge the write up front (required in `--json` mode).
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(cmd: AccountingCommand) -> i32 {
    match cmd {
        AccountingCommand::Report(a) => run_report(a),
        AccountingCommand::Bill(BillCommand::Create(a)) => run_bill_create(a),
        AccountingCommand::Bill(BillCommand::Update(a)) => run_bill_update(a),
        AccountingCommand::Expense(ExpenseCommand::Create(a)) => run_expense_create(a),
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution — same shape as gcloud.rs / jira.rs.
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
// Plugin discovery + the writes_enabled opt-in.
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
struct AccountingPlugin {
    name: String,
    dir: PathBuf,
    entry: String,
}

fn candidate_plugin_dirs(root: &Path, name: &str) -> [PathBuf; 2] {
    [
        root.join("modules/plugins").join(name),
        root.join("modules/plugins/workflow").join(name),
    ]
}

fn discover_plugin(root: &Path, name: &str) -> Result<Option<AccountingPlugin>, String> {
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
            continue;
        }
        if parsed.plugin.kind != PLUGIN_KIND {
            return Err(format!(
                "{}: [plugin].kind = {:?}, expected {:?}",
                manifest.display(),
                parsed.plugin.kind,
                PLUGIN_KIND
            ));
        }
        return Ok(Some(AccountingPlugin {
            name: parsed.plugin.name,
            dir: plugin_dir,
            entry: parsed.plugin.entry,
        }));
    }
    Ok(None)
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

fn find_enabled_plugin(root: &Path, name: &str) -> Result<Option<AccountingPlugin>, String> {
    let Some(plugin) = discover_plugin(root, name)? else {
        return Ok(None);
    };
    let enabled = workspace_enabled_set(root)?;
    if matches!(enabled.get(name), Some(true)) {
        Ok(Some(plugin))
    } else {
        Ok(None)
    }
}

/// Read `.bwoc/workspace.toml [plugins.accounting-api] writes_enabled` — the
/// standing opt-in gating every financial write. Absent/non-true ⇒ refuse.
fn writes_enabled(root: &Path) -> Result<bool, String> {
    let path = root.join(".bwoc/workspace.toml");
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&body).map_err(|e| format!("{}: parse: {e}", path.display()))?;
    Ok(value
        .get("plugins")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(PLUGIN_ACCOUNTING))
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("writes_enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

fn require_plugin(root: &Path, label: &str, json: bool) -> Result<AccountingPlugin, i32> {
    match find_enabled_plugin(root, PLUGIN_ACCOUNTING) {
        Ok(Some(p)) => Ok(p),
        Ok(None) => {
            let msg = format!(
                "plugin '{PLUGIN_ACCOUNTING}' is not installed+enabled — add it under \
                 modules/plugins/ and set [plugins.{PLUGIN_ACCOUNTING}] enabled = true \
                 in .bwoc/workspace.toml"
            );
            if json {
                emit_error_json(label, "no_plugin", &msg);
            } else {
                eprintln!("bwoc accounting {label}: {msg}");
            }
            Err(EXIT_NO_PLUGIN)
        }
        Err(e) => {
            // A plugin that exists but is malformed (bad manifest / unreadable
            // workspace.toml) is a discovery failure, not a plain local I/O
            // error — classify it exactly as gcloud/gws do (255).
            if json {
                emit_error_json(label, "discovery_error", &e);
            } else {
                eprintln!("bwoc accounting {label}: {e}");
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

fn invoke_plugin(
    plugin: &AccountingPlugin,
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
        .env("BWOC_ACCOUNTING_OPERATION", operation)
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
// Gate + JSON helpers.
// ---------------------------------------------------------------------------

fn json_write_blocked(json: bool, yes: bool) -> bool {
    json && !yes
}

fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn print_json(value: &serde_json::Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(_) => false,
    }
}

fn emit_error_json(verb: &str, code: &str, message: &str) {
    print_json(&serde_json::json!({
        "ok": false,
        "verb": verb,
        "error": code,
        "message": message,
    }));
}

/// The shared standing-opt-in gate for every financial write. Returns `Ok(())`
/// when the write may proceed, or `Err(exit_code)` after emitting the reason.
/// `target` is the human echo used in the confirmation prompt.
fn financial_write_gate(
    root: &Path,
    label: &str,
    json: bool,
    yes: bool,
    target: &str,
) -> Result<(), i32> {
    // 1) Standing opt-in — refuse by default.
    match writes_enabled(root) {
        Ok(true) => {}
        Ok(false) => {
            let msg = format!(
                "financial writes are disabled — set [plugins.{PLUGIN_ACCOUNTING}] \
                 writes_enabled = true in .bwoc/workspace.toml to allow {label} (this \
                 posts a document + an auto GL entry on the live books)"
            );
            if json {
                emit_error_json(label, "writes_disabled", &msg);
            } else {
                eprintln!("bwoc accounting {label}: {msg}");
            }
            return Err(EXIT_USAGE);
        }
        Err(e) => {
            // Unreadable/malformed workspace.toml → config error, matching
            // gcloud's `iam_writes_enabled` classification (`config_error`, exit 2).
            if json {
                emit_error_json(label, "config_error", &e);
            } else {
                eprintln!("bwoc accounting {label}: {e}");
            }
            return Err(EXIT_USAGE);
        }
    }

    // 2) Per-write confirmation (or --yes ack).
    if !yes {
        if json_write_blocked(json, yes) {
            eprintln!(
                "bwoc accounting {label}: --json requires --yes (a financial write needs explicit ack)"
            );
            return Err(EXIT_USAGE);
        }
        let prompt =
            format!("{label}: {target}. This posts to the live books + an auto GL entry. Proceed?");
        if !confirm(&prompt) {
            eprintln!("bwoc accounting {label}: aborted (no write performed)");
            return Err(EXIT_USAGE);
        }
    }
    Ok(())
}

/// Parse a `--params`/`--payload` string as a JSON **object**. Rejects arrays,
/// scalars, and malformed JSON locally so the plugin never sees junk.
fn parse_json_object(raw: &str, field: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("--{field} is not valid JSON: {e}"))?;
    if !value.is_object() {
        return Err(format!(
            "--{field} must be a JSON object (got {})",
            kind_of(&value)
        ));
    }
    Ok(value)
}

fn kind_of(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Verb: report (READ, free).
// ---------------------------------------------------------------------------

fn run_report(args: ReportArgs) -> i32 {
    let label = "report";
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc accounting {label}: {e}");
            return EXIT_USAGE;
        }
    };
    if !REPORTS.contains(&args.report.as_str()) {
        let msg = format!(
            "unknown report '{}' — expected one of: {}",
            args.report,
            REPORTS.join(", ")
        );
        if args.json {
            emit_error_json(label, "bad_report", &msg);
        } else {
            eprintln!("bwoc accounting {label}: {msg}");
        }
        return EXIT_USAGE;
    }
    let params = match args.params.as_deref() {
        None => serde_json::json!({}),
        Some(raw) => match parse_json_object(raw, "params") {
            Ok(v) => v,
            Err(e) => {
                if args.json {
                    emit_error_json(label, "bad_params", &e);
                } else {
                    eprintln!("bwoc accounting {label}: {e}");
                }
                return EXIT_USAGE;
            }
        },
    };

    let plugin = match require_plugin(&root, label, args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = serde_json::json!({
        "operation": "report",
        "report": args.report,
        "params": params,
    });
    dispatch(&plugin, &root, &request, label, args.json, |value| {
        let report = value
            .get("report")
            .and_then(|v| v.as_str())
            .unwrap_or(&args.report);
        println!("bwoc accounting {label}: {report} ✓ (data in --json)");
    })
}

// ---------------------------------------------------------------------------
// Verb: bill create (WRITE).
// ---------------------------------------------------------------------------

fn run_bill_create(args: BillCreateArgs) -> i32 {
    let label = "bill create";
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc accounting {label}: {e}");
            return EXIT_USAGE;
        }
    };
    if !BILL_TYPES.contains(&args.doc_type.as_str()) {
        let msg = format!(
            "invalid --type '{}' — expected one of: {}",
            args.doc_type,
            BILL_TYPES.join(", ")
        );
        if args.json {
            emit_error_json(label, "bad_type", &msg);
        } else {
            eprintln!("bwoc accounting {label}: {msg}");
        }
        return EXIT_USAGE;
    }

    let target = format!("create a new '{}' draft document", args.doc_type);
    if let Err(code) = financial_write_gate(&root, label, args.json, args.yes, &target) {
        return code;
    }

    let plugin = match require_plugin(&root, label, args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = serde_json::json!({
        "operation": "bill-create",
        "type": args.doc_type,
    });
    dispatch(&plugin, &root, &request, label, args.json, |value| {
        let id = value
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let number = value.get("number").and_then(|v| v.as_str()).unwrap_or("");
        println!(
            "{}",
            format!("bwoc accounting {label}: created {id} {number}").trim_end()
        );
    })
}

// ---------------------------------------------------------------------------
// Verb: bill update (WRITE).
// ---------------------------------------------------------------------------

fn run_bill_update(args: BillUpdateArgs) -> i32 {
    let label = "bill update";
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc accounting {label}: {e}");
            return EXIT_USAGE;
        }
    };
    if args.document_id.trim().is_empty() {
        let msg = "document_id must be non-empty".to_string();
        if args.json {
            emit_error_json(label, "bad_id", &msg);
        } else {
            eprintln!("bwoc accounting {label}: {msg}");
        }
        return EXIT_USAGE;
    }
    let payload = match parse_json_object(&args.payload, "payload") {
        Ok(v) => v,
        Err(e) => {
            if args.json {
                emit_error_json(label, "bad_payload", &e);
            } else {
                eprintln!("bwoc accounting {label}: {e}");
            }
            return EXIT_USAGE;
        }
    };

    let target = format!("fill/finalize purchase document '{}'", args.document_id);
    if let Err(code) = financial_write_gate(&root, label, args.json, args.yes, &target) {
        return code;
    }

    let plugin = match require_plugin(&root, label, args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = serde_json::json!({
        "operation": "bill-update",
        "document_id": args.document_id,
        "payload": payload,
    });
    dispatch(&plugin, &root, &request, label, args.json, |value| {
        let id = value
            .get("document_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&args.document_id);
        let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("?");
        println!("bwoc accounting {label}: {id} → status {status}");
    })
}

// ---------------------------------------------------------------------------
// Verb: expense create (WRITE).
// ---------------------------------------------------------------------------

fn run_expense_create(args: ExpenseCreateArgs) -> i32 {
    let label = "expense create";
    let root = match resolve_workspace(args.workspace) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc accounting {label}: {e}");
            return EXIT_USAGE;
        }
    };
    let payload = match parse_json_object(&args.payload, "payload") {
        Ok(v) => v,
        Err(e) => {
            if args.json {
                emit_error_json(label, "bad_payload", &e);
            } else {
                eprintln!("bwoc accounting {label}: {e}");
            }
            return EXIT_USAGE;
        }
    };

    let target = "record a new expense".to_string();
    if let Err(code) = financial_write_gate(&root, label, args.json, args.yes, &target) {
        return code;
    }

    let plugin = match require_plugin(&root, label, args.json) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let request = serde_json::json!({
        "operation": "expense-create",
        "payload": payload,
    });
    dispatch(&plugin, &root, &request, label, args.json, |value| {
        let id = value
            .get("expense_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let number = value.get("number").and_then(|v| v.as_str()).unwrap_or("");
        println!(
            "{}",
            format!("bwoc accounting {label}: recorded {id} {number}").trim_end()
        );
    })
}

// ---------------------------------------------------------------------------
// Shared dispatch — invoke the plugin, then render JSON or a human summary.
// ---------------------------------------------------------------------------

fn dispatch(
    plugin: &AccountingPlugin,
    root: &Path,
    request: &serde_json::Value,
    label: &str,
    json: bool,
    human: impl FnOnce(&serde_json::Value),
) -> i32 {
    match invoke_plugin(plugin, root, request) {
        Ok(value) => {
            if json {
                return if print_json(&value) {
                    EXIT_OK
                } else {
                    EXIT_PLUGIN_ERROR
                };
            }
            human(&value);
            EXIT_OK
        }
        Err(e) => {
            if json {
                emit_error_json(label, "plugin_error", &e);
            } else {
                eprintln!("bwoc accounting {label}: {e}");
            }
            EXIT_PLUGIN_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_match_plugin_list() {
        // Local pre-check must not drift from the plugin's $REPORTS. Assert the
        // set the CLI knows — the plugin is the authority, this catches a typo.
        assert!(REPORTS.contains(&"pnl"));
        assert!(REPORTS.contains(&"vat"));
        assert!(REPORTS.contains(&"asset-register"));
        assert_eq!(REPORTS.len(), 13);
    }

    #[test]
    fn bill_types_are_the_three_api_kinds() {
        assert_eq!(BILL_TYPES, &["bill", "purchase_order", "goods_receipt"]);
    }

    #[test]
    fn json_write_blocked_only_without_yes() {
        assert!(json_write_blocked(true, false));
        assert!(!json_write_blocked(true, true));
        assert!(!json_write_blocked(false, false));
    }

    #[test]
    fn parse_json_object_rejects_non_objects() {
        assert!(parse_json_object("{\"a\":1}", "payload").is_ok());
        assert!(parse_json_object("[1,2]", "payload").is_err());
        assert!(parse_json_object("42", "payload").is_err());
        assert!(parse_json_object("nope", "payload").is_err());
    }
}
