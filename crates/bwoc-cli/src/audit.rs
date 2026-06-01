//! `bwoc audit run [--plugin <name>] [--json]` — dispatch audit-kind plugins
//! and emit a canonical report.
//!
//! ## What this does
//!
//! 1. Discovers `modules/plugins/<name>/manifest.toml` with `[plugin].kind = "audit"`.
//! 2. Selects plugins to run:
//!    - default: every audit plugin **enabled** in `.bwoc/workspace.toml [plugins.<name>]`.
//!    - `--plugin <name>`: that one plugin (regardless of `enabled`); errors if it is not an audit plugin.
//! 3. For each selected plugin, dispatches its `[plugin].entry` per PLUGINS.en.md
//!    §"Hook contract — success, failure, partial state" (lines 203-212):
//!      - if `<plugin_dir>/<entry>` is an executable file, runs it directly;
//!      - otherwise resolves `entry` against `PATH`.
//!
//!    CWD = plugin dir (so the entry can find sibling `criteria.toml`).
//!    Env vars `BWOC_WORKSPACE`, `BWOC_PLUGIN_DIR`, `BWOC_AUDIT_OPERATION` carry
//!    the invocation context. Stdin receives
//!    `{"operation":"audit_run","workspace":"<abs>"}` (plugins MAY ignore stdin;
//!    the env vars are the canonical channel).
//! 4. Parses stdout as JSON — accepts either `[ <finding>, ... ]` or
//!    `{"findings":[ ... ]}`. Validates each finding against the normative BWOC-11
//!    schema (closed enums for `severity`/`status`/`evidence.kind`, the
//!    `remedy` ↔ `status = pass` rule, the `evidence.value`-empty ↔
//!    `evidence.kind = none` rule). Any violation rejects the plugin's run as
//!    a **plugin bug** (PLUGINS.en.md line 59), exits 255.
//! 5. Emits the canonical report — either a per-plugin human-readable table
//!    plus an aggregate summary, OR the `--json` envelope:
//!    `{ workspace, runs: [{ plugin, version, started_at, finished_at, findings: [...] }, ...], summary }`
//!    Findings serialize in the order the plugin emits them — per BWOC-11 line 84,
//!    that order is itself **criterion-declaration order**, owned by the plugin.
//!
//! ## Exit-code convention — normative
//!
//! Pinned in `PLUGINS.en.md` §"Exit codes — `bwoc audit run`" and its TH
//! parity. The four codes:
//!
//! - `0` — no `fail` findings across selected plugins (or no plugins selected).
//! - `1..=254` — number of `fail` findings, clamped to 254.
//! - `255` — framework/plugin runtime error (spawn failure, non-JSON output,
//!   schema violation, manifest parse error). `summary.framework_error` is
//!   `true` in the `--json` envelope.
//! - `2` — operator/usage error (no workspace context, or `--plugin <name>`
//!   did not resolve to an audit-kind plugin).
//!
//! The exit code can be ignored entirely by passing `--json`; the
//! `summary.fail_count` and `summary.framework_error` fields carry the same
//! signal in structured form.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Args.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CommonArgs {
    /// Workspace root override. Resolution: `--workspace` > `BWOC_WORKSPACE`
    /// env > ancestor-walk for `.bwoc/workspace.toml` > error.
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RunArgs {
    pub common: CommonArgs,
    /// Scope to one audit plugin (overrides the default "all enabled" set).
    pub plugin: Option<String>,
    pub json: bool,
}

// ---------------------------------------------------------------------------
// Manifest schema (mirror of PLUGINS.en.md §"Manifest", lines 144-173).
// Kept local rather than re-using plugin.rs's private types — keeps the audit
// surface decoupled from the read/write plugin surface.
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
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    compat: String,
    entry: String,
}

#[derive(Debug, Clone)]
struct DiscoveredAudit {
    path: PathBuf,
    manifest: ManifestRaw,
}

// ---------------------------------------------------------------------------
// Workspace resolution (mirror of plugin.rs:143 — ancestor walk for
// .bwoc/workspace.toml unless overridden).
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

fn plugins_dir(root: &Path) -> PathBuf {
    root.join("modules/plugins")
}

fn parse_manifest(path: &Path) -> Result<ManifestRaw, String> {
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str::<ManifestRaw>(&body).map_err(|e| format!("parse: {e}"))
}

/// Walk `modules/plugins/*/manifest.toml`, keep only `kind = "audit"`.
fn discover_audit(root: &Path) -> Result<Vec<DiscoveredAudit>, String> {
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

    let mut out = Vec::new();
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
        if manifest.plugin.kind != "audit" {
            continue;
        }
        out.push(DiscoveredAudit {
            path: plugin_dir,
            manifest,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// workspace.toml [plugins.<name>] resolution — just enabled-set discovery.
// ---------------------------------------------------------------------------

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
        out.insert(name.clone(), enabled);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Finding schema — normative per BWOC-11 (PLUGINS.en.md lines 57-127).
// ---------------------------------------------------------------------------

const SEVERITIES: &[&str] = &["info", "low", "medium", "high", "critical"];
const STATUSES: &[&str] = &["pass", "fail", "not_applicable", "not_implemented"];
// BWOC-27 grew the enum from {file, content, command, none} to add `attestation`
// + `sample`. The closed set is owned by `check` (single source of truth shared
// with manifest validation) so the two validators can't drift apart.
use crate::check::EVIDENCE_KINDS;

/// Kind-specific and orthogonal sub-fields on `evidence` (BWOC-27). All optional
/// at the struct level; `parse_finding` enforces the per-kind required-ness rule
/// before constructing this. Unset fields are dropped from the canonical JSON
/// output so a kind=`file` finding does not carry empty `signer` noise.
#[derive(Debug, Clone, Default)]
struct EvidenceExtras {
    // attestation kind
    signer: Option<String>,
    signed_at: Option<String>,
    // sample kind
    sampled_count: Option<i64>,
    sampled_of: Option<i64>,
    window: Option<String>,
    // orthogonal to kind (any kind may carry these)
    as_of: Option<String>,
    valid_through: Option<String>,
}

/// One validated finding. `severity`, `status`, `evidence.kind` are stored as
/// the closed-enum strings; we re-emit them verbatim so canonical JSON output
/// matches the plugin's own spelling.
#[derive(Debug, Clone)]
struct Finding {
    criterion_id: String,
    severity: String,
    status: String,
    evidence_kind: String,
    evidence_value: String,
    evidence_extras: EvidenceExtras,
    remedy: Option<String>,
}

impl Finding {
    fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "criterion_id".into(),
            serde_json::Value::String(self.criterion_id.clone()),
        );
        obj.insert(
            "severity".into(),
            serde_json::Value::String(self.severity.clone()),
        );
        obj.insert(
            "status".into(),
            serde_json::Value::String(self.status.clone()),
        );
        let mut ev = serde_json::Map::new();
        ev.insert(
            "kind".into(),
            serde_json::Value::String(self.evidence_kind.clone()),
        );
        ev.insert(
            "value".into(),
            serde_json::Value::String(self.evidence_value.clone()),
        );
        let x = &self.evidence_extras;
        if let Some(s) = &x.signer {
            ev.insert("signer".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(s) = &x.signed_at {
            ev.insert("signed_at".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(n) = x.sampled_count {
            ev.insert("sampled_count".into(), serde_json::Value::from(n));
        }
        if let Some(n) = x.sampled_of {
            ev.insert("sampled_of".into(), serde_json::Value::from(n));
        }
        if let Some(s) = &x.window {
            ev.insert("window".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(s) = &x.as_of {
            ev.insert("as_of".into(), serde_json::Value::String(s.clone()));
        }
        if let Some(s) = &x.valid_through {
            ev.insert("valid_through".into(), serde_json::Value::String(s.clone()));
        }
        obj.insert("evidence".into(), serde_json::Value::Object(ev));
        if let Some(r) = &self.remedy {
            obj.insert("remedy".into(), serde_json::Value::String(r.clone()));
        }
        serde_json::Value::Object(obj)
    }
}

/// Parse one finding object. Returns Err with a precise diagnostic on any
/// schema violation — every closed-enum check, the remedy ↔ status rule, and
/// the evidence-kind ↔ evidence-value rule are validated here.
fn parse_finding(v: &serde_json::Value, index: usize) -> Result<Finding, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| format!("findings[{index}] is not an object"))?;

    let criterion_id = obj
        .get("criterion_id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("findings[{index}]: missing 'criterion_id' (string)"))?
        .to_string();
    if criterion_id.is_empty() {
        return Err(format!("findings[{index}]: 'criterion_id' is empty"));
    }
    if !is_kebab_case(&criterion_id) {
        return Err(format!(
            "findings[{index}]: 'criterion_id' = {criterion_id:?} is not kebab-case \
             ([a-z0-9-], no leading/trailing/double dashes)"
        ));
    }

    let severity = obj
        .get("severity")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("findings[{index}]: missing 'severity'"))?
        .to_string();
    if !SEVERITIES.contains(&severity.as_str()) {
        return Err(format!(
            "findings[{index}]: 'severity' = {severity:?} not in closed enum {:?}",
            SEVERITIES
        ));
    }

    let status = obj
        .get("status")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("findings[{index}]: missing 'status'"))?
        .to_string();
    if !STATUSES.contains(&status.as_str()) {
        return Err(format!(
            "findings[{index}]: 'status' = {status:?} not in closed enum {:?}",
            STATUSES
        ));
    }

    let evidence = obj
        .get("evidence")
        .and_then(|x| x.as_object())
        .ok_or_else(|| format!("findings[{index}]: missing 'evidence' object {{ kind, value }}"))?;
    let evidence_kind = evidence
        .get("kind")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("findings[{index}]: missing 'evidence.kind'"))?
        .to_string();
    if !EVIDENCE_KINDS.contains(&evidence_kind.as_str()) {
        return Err(format!(
            "findings[{index}]: 'evidence.kind' = {evidence_kind:?} not in closed enum {:?}",
            EVIDENCE_KINDS
        ));
    }
    let evidence_value = evidence
        .get("value")
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("findings[{index}]: missing 'evidence.value' (string)"))?
        .to_string();

    // Schema rule (PLUGINS.en.md line 78): evidence.kind = "none" iff
    // evidence.value is empty. Closed enum — no other combo permitted.
    if evidence_kind == "none" {
        if !evidence_value.is_empty() {
            return Err(format!(
                "findings[{index}]: evidence.kind = 'none' must have empty value, \
                 got {evidence_value:?}"
            ));
        }
    } else if evidence_value.is_empty() {
        return Err(format!(
            "findings[{index}]: evidence.kind = {evidence_kind:?} requires a non-empty value"
        ));
    }

    // evidence.kind = "none" is forbidden with status = pass / fail
    // (PLUGINS.en.md line 78: "MUST NOT appear with status = pass or fail").
    if evidence_kind == "none" && (status == "pass" || status == "fail") {
        return Err(format!(
            "findings[{index}]: evidence.kind = 'none' is forbidden with status = {status:?}"
        ));
    }

    // BWOC-27 sub-fields. Extract whatever is present; per-kind required-field
    // validation runs after extraction.
    let evidence_extras = parse_evidence_extras(evidence, &evidence_kind, index)?;

    let remedy = obj
        .get("remedy")
        .map(|x| {
            x.as_str()
                .ok_or_else(|| format!("findings[{index}]: 'remedy' must be a string"))
                .map(|s| s.to_string())
        })
        .transpose()?;

    // remedy ↔ status rule (PLUGINS.en.md line 69): required when status is
    // fail / not_applicable / not_implemented; forbidden when status = pass.
    match (status.as_str(), remedy.as_deref()) {
        ("pass", Some(_)) => {
            return Err(format!(
                "findings[{index}]: status = 'pass' must not carry 'remedy' (PLUGINS.en.md line 69)"
            ));
        }
        ("pass", None) => {}
        (_, None) => {
            return Err(format!(
                "findings[{index}]: status = {status:?} requires 'remedy' (PLUGINS.en.md line 69)"
            ));
        }
        (_, Some("")) => {
            return Err(format!(
                "findings[{index}]: 'remedy' is empty (status = {status:?} requires actionable text)"
            ));
        }
        _ => {}
    }

    Ok(Finding {
        criterion_id,
        severity,
        status,
        evidence_kind,
        evidence_value,
        evidence_extras,
        remedy,
    })
}

/// Extract and validate BWOC-27 evidence sub-fields. Per-kind required-ness:
///
/// - `attestation` → `signer` (non-empty string) + `signed_at` (non-empty string).
/// - `sample` → `sampled_count` (integer ≥ 0) + `sampled_of` (integer ≥ count).
///   `window` is optional. Strict ISO 8601 date parsing for `signed_at` /
///   `as_of` / `valid_through` is deferred to `bwoc check` (BWOC-29) — here
///   the dispatcher only enforces shape and required-ness.
///
/// Sub-fields present on a kind that doesn't claim them (e.g. `signer` on a
/// `file` finding) are passed through unmodified; flagging them as semantic
/// noise is `bwoc check`'s job, not the runtime dispatcher's.
fn parse_evidence_extras(
    evidence: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
    index: usize,
) -> Result<EvidenceExtras, String> {
    fn opt_string(
        evidence: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        index: usize,
    ) -> Result<Option<String>, String> {
        match evidence.get(field) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .ok_or_else(|| format!("findings[{index}]: 'evidence.{field}' must be a string"))
                .map(|s| Some(s.to_string())),
        }
    }
    fn opt_i64(
        evidence: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        index: usize,
    ) -> Result<Option<i64>, String> {
        match evidence.get(field) {
            None => Ok(None),
            Some(v) => v
                .as_i64()
                .ok_or_else(|| format!("findings[{index}]: 'evidence.{field}' must be an integer"))
                .map(Some),
        }
    }

    let signer = opt_string(evidence, "signer", index)?;
    let signed_at = opt_string(evidence, "signed_at", index)?;
    let sampled_count = opt_i64(evidence, "sampled_count", index)?;
    let sampled_of = opt_i64(evidence, "sampled_of", index)?;
    let window = opt_string(evidence, "window", index)?;
    let as_of = opt_string(evidence, "as_of", index)?;
    let valid_through = opt_string(evidence, "valid_through", index)?;

    if kind == "attestation" {
        // PLUGINS.en.md §Evidence kinds — attestation row: signer + signed_at
        // are required. Empty string fails the same as missing — the operator
        // attestation has to actually carry an identity and a date.
        match signer.as_deref() {
            Some(s) if !s.is_empty() => {}
            Some(_) => {
                return Err(format!(
                    "findings[{index}]: evidence.kind = 'attestation' requires non-empty 'signer'"
                ));
            }
            None => {
                return Err(format!(
                    "findings[{index}]: evidence.kind = 'attestation' requires 'signer'"
                ));
            }
        }
        match signed_at.as_deref() {
            Some(s) if !s.is_empty() => {}
            Some(_) => {
                return Err(format!(
                    "findings[{index}]: evidence.kind = 'attestation' requires non-empty \
                     'signed_at'"
                ));
            }
            None => {
                return Err(format!(
                    "findings[{index}]: evidence.kind = 'attestation' requires 'signed_at'"
                ));
            }
        }
    } else if kind == "sample" {
        // PLUGINS.en.md §Evidence kinds — sample row: sampled_count + sampled_of
        // are required. `window` stays optional.
        let count = sampled_count.ok_or_else(|| {
            format!(
                "findings[{index}]: evidence.kind = 'sample' requires 'sampled_count' (integer)"
            )
        })?;
        let of = sampled_of.ok_or_else(|| {
            format!("findings[{index}]: evidence.kind = 'sample' requires 'sampled_of' (integer)")
        })?;
        if count < 0 {
            return Err(format!(
                "findings[{index}]: 'sampled_count' must be ≥ 0, got {count}"
            ));
        }
        if of < count {
            return Err(format!(
                "findings[{index}]: 'sampled_of' ({of}) must be ≥ 'sampled_count' ({count})"
            ));
        }
    }

    Ok(EvidenceExtras {
        signer,
        signed_at,
        sampled_count,
        sampled_of,
        window,
        as_of,
        valid_through,
    })
}

fn is_kebab_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }
    let mut prev_dash = false;
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
        if !ok {
            return false;
        }
        if b == b'-' && prev_dash {
            return false;
        }
        prev_dash = b == b'-';
    }
    true
}

/// Accept either bare `[ <finding>, ... ]` or `{"findings":[ ... ]}`.
fn parse_findings_stdout(raw: &str) -> Result<Vec<Finding>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("plugin emitted no stdout (expected JSON findings)".to_string());
    }
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("stdout is not valid JSON: {e}"))?;
    let arr = if let Some(a) = value.as_array() {
        a.clone()
    } else if let Some(obj) = value.as_object() {
        obj.get("findings")
            .and_then(|x| x.as_array())
            .ok_or_else(|| {
                "stdout object missing 'findings': [...] (or emit a bare array)".to_string()
            })?
            .clone()
    } else {
        return Err(
            "stdout JSON must be an array of findings or {\"findings\": [...]}".to_string(),
        );
    };

    let mut out = Vec::with_capacity(arr.len());
    for (i, v) in arr.iter().enumerate() {
        out.push(parse_finding(v, i)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Plugin invocation.
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable_file(p: &Path) -> bool {
    p.is_file()
}

/// Resolve `entry` to a runnable program. PLUGINS.en.md §"Manifest" line 172
/// allows two forms — a binary on `PATH` (preferred), or a sibling Rust crate
/// name. For BWOC-12 we additionally accept an executable file living
/// alongside `manifest.toml` (lets a shell script or compiled helper ship
/// inside the plugin dir without polluting `PATH`). Resolution order:
///   1. `<plugin_dir>/<entry>` if it is an executable regular file → run that.
///   2. Otherwise treat `entry` as a PATH-resolved program name.
fn resolve_entry_program(plugin_dir: &Path, entry: &str) -> String {
    let sibling = plugin_dir.join(entry);
    if is_executable_file(&sibling) {
        sibling.to_string_lossy().into_owned()
    } else {
        entry.to_string()
    }
}

#[derive(Debug)]
struct InvokeOutcome {
    started_at: String,
    finished_at: String,
    findings: Vec<Finding>,
    stderr: String,
    duration_ms: u64,
}

/// Environment handed to a spawned audit plugin: the **scrubbed** (allowlist-
/// only) operator environment — never the raw inherited env, so a third-party
/// plugin can't read `GITHUB_TOKEN` / `AWS_*` — plus the three `BWOC_*` context
/// vars the plugin protocol defines. Apply as `env_clear().envs(...)`.
fn plugin_child_env(workspace: &Path, plugin_dir: &Path) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = bwoc_core::env_scrub::scrub_env().into_iter().collect();
    env.push((
        "BWOC_WORKSPACE".to_string(),
        workspace.display().to_string(),
    ));
    env.push((
        "BWOC_PLUGIN_DIR".to_string(),
        plugin_dir.display().to_string(),
    ));
    env.push(("BWOC_AUDIT_OPERATION".to_string(), "audit_run".to_string()));
    env
}

fn invoke_plugin(plugin: &DiscoveredAudit, workspace: &Path) -> Result<InvokeOutcome, String> {
    // BWOC-36: guard against path-traversal RCE before resolving/spawning the
    // entry. A malicious manifest could otherwise escape the plugin dir.
    crate::util::validate_plugin_entry(&plugin.manifest.plugin.entry)?;
    let program = resolve_entry_program(&plugin.path, &plugin.manifest.plugin.entry);
    let started_at = current_utc_iso8601();
    let start = Instant::now();

    // An audit plugin is third-party code (installed from a git/tarball URL) —
    // the one spawn whose explicit job is to run code we don't yet trust. Start
    // it from a scrubbed environment so it cannot read the operator's
    // GITHUB_TOKEN / AWS_* / NPM_TOKEN, etc.; `env_clear` first, then re-add only
    // the scrubbed allowlist + the three BWOC_* context vars it needs.
    let mut child = Command::new(&program)
        .current_dir(&plugin.path)
        .env_clear()
        .envs(plugin_child_env(workspace, &plugin.path))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn '{program}': {e}"))?;

    // Stdin: the same context as the env vars, in the operation-envelope shape
    // suggested by PLUGINS.en.md §"Per-phase examples" line 233. Plugins MAY
    // ignore stdin and read the env vars instead — both channels carry the
    // same data, env-var-first lets `/bin/echo`-style wrappers work.
    let payload = serde_json::json!({
        "operation": "audit_run",
        "workspace": workspace.display().to_string(),
        "plugin_dir": plugin.path.display().to_string(),
    });
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "{}", payload);
    }
    drop(child.stdin.take());

    let output = child
        .wait_with_output()
        .map_err(|e| format!("wait '{program}': {e}"))?;
    let finished_at = current_utc_iso8601();
    let duration_ms = start.elapsed().as_millis() as u64;
    let stderr_text = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        return Err(format!(
            "'{program}' exited {} (stderr: {})",
            output.status.code().unwrap_or(-1),
            stderr_text.trim()
        ));
    }

    let stdout_text = String::from_utf8_lossy(&output.stdout).into_owned();
    let findings = parse_findings_stdout(&stdout_text)?;

    Ok(InvokeOutcome {
        started_at,
        finished_at,
        findings,
        stderr: stderr_text,
        duration_ms,
    })
}

// ---------------------------------------------------------------------------
// Mini stdlib-only ISO 8601 (copied from plugin.rs:933 — kept private here
// to keep the audit module self-contained).
// ---------------------------------------------------------------------------

fn current_utc_iso8601() -> String {
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

// ---------------------------------------------------------------------------
// Report rendering.
// ---------------------------------------------------------------------------

fn print_json(value: &serde_json::Value) -> Result<(), String> {
    let s = serde_json::to_string_pretty(value).map_err(|e| format!("serialize JSON: {e}"))?;
    println!("{s}");
    Ok(())
}

fn render_human(workspace: &Path, runs: &[(String, String, InvokeOutcome)], summary: &Summary) {
    println!("Audit run — workspace: {}", workspace.display());
    println!();
    if runs.is_empty() {
        println!("(no audit plugins selected)");
        return;
    }
    for (plugin_name, plugin_version, outcome) in runs {
        println!(
            "═══ {} (v{}) — {} finding(s), {} ms",
            plugin_name,
            plugin_version,
            outcome.findings.len(),
            outcome.duration_ms,
        );
        if outcome.findings.is_empty() {
            println!("  (no findings)");
            println!();
            continue;
        }
        let crit_w = outcome
            .findings
            .iter()
            .map(|f| f.criterion_id.len())
            .max()
            .unwrap_or(12)
            .max(12);
        let sev_w = outcome
            .findings
            .iter()
            .map(|f| f.severity.len())
            .max()
            .unwrap_or(8)
            .max(8);
        let st_w = outcome
            .findings
            .iter()
            .map(|f| f.status.len())
            .max()
            .unwrap_or(15)
            .max(15);
        println!(
            "  {:<crit_w$}  {:<sev_w$}  {:<st_w$}  EVIDENCE",
            "CRITERION", "SEVERITY", "STATUS",
        );
        for f in &outcome.findings {
            let ev = if f.evidence_kind == "none" {
                "(none)".to_string()
            } else {
                format!("{}: {}", f.evidence_kind, f.evidence_value)
            };
            println!(
                "  {:<crit_w$}  {:<sev_w$}  {:<st_w$}  {}",
                f.criterion_id, f.severity, f.status, ev,
            );
            if let Some(r) = &f.remedy {
                println!("    ↳ {}", r);
            }
        }
        if !outcome.stderr.trim().is_empty() {
            println!("  (stderr from plugin):");
            for line in outcome.stderr.lines().take(8) {
                println!("    {line}");
            }
        }
        println!();
    }
    println!(
        "Summary — pass={}, fail={}, not_applicable={}, not_implemented={} (across {} plugin(s))",
        summary.pass_count,
        summary.fail_count,
        summary.not_applicable_count,
        summary.not_implemented_count,
        summary.plugin_count,
    );
}

#[derive(Debug, Default, Clone)]
struct Summary {
    plugin_count: usize,
    pass_count: usize,
    fail_count: usize,
    not_applicable_count: usize,
    not_implemented_count: usize,
}

impl Summary {
    fn add(&mut self, findings: &[Finding]) {
        for f in findings {
            match f.status.as_str() {
                "pass" => self.pass_count += 1,
                "fail" => self.fail_count += 1,
                "not_applicable" => self.not_applicable_count += 1,
                "not_implemented" => self.not_implemented_count += 1,
                _ => unreachable!("parse_finding gates the closed enum"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `bwoc audit run` — the public entry point.
// ---------------------------------------------------------------------------

/// Exit-code constants — see module doc for the normative convention.
const EXIT_FRAMEWORK_ERROR: i32 = 255;
const EXIT_FAIL_COUNT_MAX: i32 = 254;

/// Map the post-run state to a process exit code per the module-doc
/// convention. Framework errors win over fail counts — if any plugin failed
/// to produce a valid report, the run did not complete cleanly and we
/// surface `255` even when other plugins reported zero fails. Otherwise
/// the count of `fail` findings is returned, clamped to `254` so it never
/// collides with the framework-error code.
fn compute_exit_code(fail_count: usize, framework_error: bool) -> i32 {
    if framework_error {
        return EXIT_FRAMEWORK_ERROR;
    }
    (fail_count as i32).min(EXIT_FAIL_COUNT_MAX)
}

pub fn run(args: RunArgs) -> i32 {
    let root = match resolve_workspace(&args.common) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc audit run: {e}");
            return 2;
        }
    };

    let all_audit = match discover_audit(&root) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc audit run: {e}");
            return EXIT_FRAMEWORK_ERROR;
        }
    };

    let enabled_set = match workspace_enabled_set(&root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("bwoc audit run: {e}");
            return EXIT_FRAMEWORK_ERROR;
        }
    };

    // Select plugins.
    let selected: Vec<DiscoveredAudit> = if let Some(name) = args.plugin.as_deref() {
        let Some(p) = all_audit.iter().find(|p| p.manifest.plugin.name == name) else {
            // Check if a non-audit plugin matches — gives a sharper error.
            let plugin_dir = plugins_dir(&root).join(name);
            if plugin_dir.is_dir() {
                eprintln!(
                    "bwoc audit run: '{name}' is installed but its [plugin].kind != \"audit\""
                );
            } else {
                eprintln!(
                    "bwoc audit run: no audit plugin named '{name}' under {}",
                    plugins_dir(&root).display()
                );
            }
            return 2;
        };
        vec![p.clone()]
    } else {
        all_audit
            .iter()
            .filter(|p| matches!(enabled_set.get(&p.manifest.plugin.name), Some(true)))
            .cloned()
            .collect()
    };

    if selected.is_empty() {
        if args.json {
            let value = serde_json::json!({
                "workspace": root.display().to_string(),
                "runs": [],
                "summary": {
                    "plugin_count": 0,
                    "pass_count": 0,
                    "fail_count": 0,
                    "not_applicable_count": 0,
                    "not_implemented_count": 0,
                    "framework_error": false,
                },
            });
            if let Err(e) = print_json(&value) {
                eprintln!("bwoc audit run: {e}");
                return EXIT_FRAMEWORK_ERROR;
            }
            return 0;
        }
        if args.plugin.is_some() {
            // Unreachable — handled above. Defensive only.
            return 2;
        }
        println!(
            "Audit run — workspace: {}\n(no audit plugins enabled in workspace.toml [plugins])",
            root.display()
        );
        return 0;
    }

    let mut runs: Vec<(String, String, InvokeOutcome)> = Vec::with_capacity(selected.len());
    let mut summary = Summary {
        plugin_count: selected.len(),
        ..Summary::default()
    };
    let mut framework_error = false;
    let mut error_messages: Vec<String> = Vec::new();

    for plugin in &selected {
        match invoke_plugin(plugin, &root) {
            Ok(outcome) => {
                summary.add(&outcome.findings);
                runs.push((
                    plugin.manifest.plugin.name.clone(),
                    plugin.manifest.plugin.version.clone(),
                    outcome,
                ));
            }
            Err(e) => {
                framework_error = true;
                let msg = format!("'{}': {e}", plugin.manifest.plugin.name);
                eprintln!("bwoc audit run: {msg}");
                error_messages.push(msg);
            }
        }
    }

    if args.json {
        let mut runs_json = Vec::with_capacity(runs.len());
        for (name, version, outcome) in &runs {
            runs_json.push(serde_json::json!({
                "plugin": name,
                "version": version,
                "started_at": outcome.started_at,
                "finished_at": outcome.finished_at,
                "duration_ms": outcome.duration_ms,
                "findings": outcome.findings.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
                "stderr": outcome.stderr,
            }));
        }
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "runs": runs_json,
            "summary": {
                "plugin_count": summary.plugin_count,
                "pass_count": summary.pass_count,
                "fail_count": summary.fail_count,
                "not_applicable_count": summary.not_applicable_count,
                "not_implemented_count": summary.not_implemented_count,
                "framework_error": framework_error,
                "errors": error_messages,
            },
        });
        if let Err(e) = print_json(&value) {
            eprintln!("bwoc audit run: {e}");
            return EXIT_FRAMEWORK_ERROR;
        }
    } else {
        render_human(&root, &runs, &summary);
        if framework_error {
            eprintln!();
            eprintln!(
                "bwoc audit run: {} plugin(s) failed to produce a valid report; see stderr above",
                error_messages.len()
            );
        }
    }

    compute_exit_code(summary.fail_count, framework_error)
}

// ===========================================================================
// Unit tests ================================================================
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// BWOC-C6: an audit plugin must never inherit the operator's environment.
    /// Every var handed to the child, apart from the `BWOC_*` context vars, must
    /// come from the scrub allowlist — so a raw-inherited env (the original bug)
    /// would surface `GITHUB_TOKEN` / `CI` / `GITHUB_ACTIONS`, none of which are
    /// allowlisted, and fail here regardless of the runner's ambient env.
    #[test]
    fn plugin_child_env_is_scrubbed_not_inherited() {
        let env = plugin_child_env(Path::new("/ws"), Path::new("/ws/modules/plugins/x"));
        let has = |k: &str| env.iter().any(|(key, _)| key == k);

        // The three context vars the plugin protocol expects are present...
        assert!(has("BWOC_WORKSPACE"));
        assert!(has("BWOC_PLUGIN_DIR"));
        assert!(has("BWOC_AUDIT_OPERATION"));

        // ...and every other var is allowlisted — never the raw inherited env.
        for (k, _) in &env {
            if k.starts_with("BWOC_") {
                continue;
            }
            assert!(
                bwoc_core::env_scrub::ENV_ALLOWLIST.contains(&k.as_str()),
                "audit plugin would inherit non-allowlisted var `{k}` — env scrub bypassed"
            );
        }
    }
    use serde_json::json;

    fn ev(kind: &str, value: &str) -> serde_json::Value {
        json!({ "kind": kind, "value": value })
    }

    // BWOC-36 — the path-traversal guard applied before `invoke_plugin` spawns
    // the entry. Uses the shared validator that `invoke_plugin` calls.
    use crate::util::validate_plugin_entry;

    #[test]
    fn entry_guard_accepts_bare_name() {
        assert!(validate_plugin_entry("audit.sh").is_ok());
    }

    #[test]
    fn entry_guard_accepts_contained_relative() {
        assert!(validate_plugin_entry("bin/audit.sh").is_ok());
    }

    #[test]
    fn entry_guard_rejects_parent_escape() {
        let e = validate_plugin_entry("../escape").unwrap_err();
        assert!(e.contains("../escape"), "error must name the entry: {e}");
        assert!(e.contains(".."), "error must name the rule: {e}");
    }

    #[test]
    fn entry_guard_rejects_absolute() {
        let e = validate_plugin_entry("/tmp/evil").unwrap_err();
        assert!(e.contains("/tmp/evil"), "error must name the entry: {e}");
        assert!(e.contains("absolute"), "error must name the rule: {e}");
    }

    #[test]
    fn entry_guard_rejects_parent_anywhere() {
        // `..` is rejected even mid-path, where it does not visibly escape.
        assert!(validate_plugin_entry("nested/../evil").is_err());
    }

    #[test]
    fn finding_pass_accepts_omitted_remedy() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "high",
            "status": "pass",
            "evidence": ev("file", "docs/X.md"),
        });
        let f = parse_finding(&v, 0).unwrap();
        assert_eq!(f.status, "pass");
        assert!(f.remedy.is_none());
    }

    #[test]
    fn finding_pass_rejects_remedy() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "high",
            "status": "pass",
            "evidence": ev("file", "docs/X.md"),
            "remedy": "nothing",
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("status = 'pass' must not carry 'remedy'"), "{e}");
    }

    #[test]
    fn finding_fail_requires_remedy() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "medium",
            "status": "fail",
            "evidence": ev("file", "docs/X.md"),
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("requires 'remedy'"), "{e}");
    }

    #[test]
    fn finding_not_implemented_requires_remedy_and_accepts_none_evidence() {
        let v = json!({
            "criterion_id": "iso-9001-x",
            "severity": "medium",
            "status": "not_implemented",
            "evidence": ev("none", ""),
            "remedy": "Runtime deferred to BWOC-EPIC-3.",
        });
        let f = parse_finding(&v, 0).unwrap();
        assert_eq!(f.status, "not_implemented");
        assert_eq!(f.evidence_kind, "none");
    }

    #[test]
    fn finding_rejects_unknown_severity() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "blocker",
            "status": "pass",
            "evidence": ev("file", "X.md"),
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'severity' = \"blocker\""), "{e}");
    }

    #[test]
    fn finding_rejects_unknown_status() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "low",
            "status": "warning",
            "evidence": ev("file", "X.md"),
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'status' = \"warning\""), "{e}");
    }

    #[test]
    fn finding_rejects_unknown_evidence_kind() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "low",
            "status": "pass",
            "evidence": ev("url", "https://x"),
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'evidence.kind' = \"url\""), "{e}");
    }

    #[test]
    fn finding_rejects_none_evidence_with_pass_status() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "low",
            "status": "pass",
            "evidence": ev("none", ""),
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(
            e.contains("evidence.kind = 'none' is forbidden with status = \"pass\""),
            "{e}"
        );
    }

    #[test]
    fn finding_rejects_none_evidence_with_nonempty_value() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "low",
            "status": "not_applicable",
            "evidence": ev("none", "something"),
            "remedy": "n/a",
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(
            e.contains("evidence.kind = 'none' must have empty value"),
            "{e}"
        );
    }

    #[test]
    fn finding_rejects_nonnone_evidence_with_empty_value() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "low",
            "status": "pass",
            "evidence": ev("file", ""),
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("requires a non-empty value"), "{e}");
    }

    #[test]
    fn finding_rejects_non_kebab_criterion_id() {
        for bad in ["X-Y", "x_y", "-leading", "trailing-", "x--y", ""] {
            let v = json!({
                "criterion_id": bad,
                "severity": "low",
                "status": "pass",
                "evidence": ev("file", "X.md"),
            });
            assert!(parse_finding(&v, 0).is_err(), "expected {bad:?} to fail");
        }
    }

    #[test]
    fn finding_rejects_empty_remedy_string() {
        let v = json!({
            "criterion_id": "x-y",
            "severity": "low",
            "status": "fail",
            "evidence": ev("file", "X.md"),
            "remedy": "",
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'remedy' is empty"), "{e}");
    }

    #[test]
    fn parse_findings_accepts_bare_array() {
        let raw = r#"[
            {"criterion_id":"a-1","severity":"low","status":"pass","evidence":{"kind":"file","value":"X"}}
        ]"#;
        let v = parse_findings_stdout(raw).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].criterion_id, "a-1");
    }

    #[test]
    fn parse_findings_accepts_envelope_object() {
        let raw = r#"{ "findings": [
            {"criterion_id":"a-1","severity":"low","status":"pass","evidence":{"kind":"file","value":"X"}}
        ]}"#;
        let v = parse_findings_stdout(raw).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn parse_findings_rejects_empty_stdout() {
        let e = parse_findings_stdout("").unwrap_err();
        assert!(e.contains("no stdout"), "{e}");
    }

    #[test]
    fn parse_findings_rejects_non_json() {
        let e = parse_findings_stdout("not json").unwrap_err();
        assert!(e.contains("not valid JSON"), "{e}");
    }

    #[test]
    fn parse_findings_rejects_envelope_without_findings_key() {
        let e = parse_findings_stdout(r#"{"something":1}"#).unwrap_err();
        assert!(e.contains("missing 'findings'"), "{e}");
    }

    #[test]
    fn summary_counts_each_status() {
        let mk = |status: &str, has_remedy: bool| Finding {
            criterion_id: "c-1".to_string(),
            severity: "low".to_string(),
            status: status.to_string(),
            evidence_kind: if status == "not_applicable" || status == "not_implemented" {
                "none".to_string()
            } else {
                "file".to_string()
            },
            evidence_value: if status == "not_applicable" || status == "not_implemented" {
                String::new()
            } else {
                "X".to_string()
            },
            evidence_extras: EvidenceExtras::default(),
            remedy: if has_remedy {
                Some("r".to_string())
            } else {
                None
            },
        };
        let fs = vec![
            mk("pass", false),
            mk("pass", false),
            mk("fail", true),
            mk("not_applicable", true),
            mk("not_implemented", true),
            mk("not_implemented", true),
        ];
        let mut s = Summary {
            plugin_count: 1,
            ..Summary::default()
        };
        s.add(&fs);
        assert_eq!(s.pass_count, 2);
        assert_eq!(s.fail_count, 1);
        assert_eq!(s.not_applicable_count, 1);
        assert_eq!(s.not_implemented_count, 2);
    }

    #[test]
    fn iso8601_shape() {
        let s = current_utc_iso8601();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn to_json_round_trips_pass_finding() {
        let v = json!({
            "criterion_id": "a-1",
            "severity": "medium",
            "status": "pass",
            "evidence": {"kind":"file","value":"X.md"},
        });
        let f = parse_finding(&v, 0).unwrap();
        let out = f.to_json();
        assert_eq!(out["criterion_id"], "a-1");
        assert_eq!(out["severity"], "medium");
        assert_eq!(out["status"], "pass");
        assert_eq!(out["evidence"]["kind"], "file");
        assert_eq!(out["evidence"]["value"], "X.md");
        assert!(out.as_object().unwrap().get("remedy").is_none());
    }

    #[test]
    fn to_json_includes_remedy_when_present() {
        let v = json!({
            "criterion_id": "a-1",
            "severity": "medium",
            "status": "fail",
            "evidence": {"kind":"file","value":"X.md"},
            "remedy": "fix it",
        });
        let f = parse_finding(&v, 0).unwrap();
        let out = f.to_json();
        assert_eq!(out["remedy"], "fix it");
    }

    // -----------------------------------------------------------------------
    // Exit-code convention — pins PLUGINS.en.md §"Exit codes — `bwoc audit run`".
    // -----------------------------------------------------------------------

    #[test]
    fn exit_code_all_pass_returns_zero() {
        assert_eq!(compute_exit_code(0, false), 0);
    }

    #[test]
    fn exit_code_one_fail_returns_one() {
        assert_eq!(compute_exit_code(1, false), 1);
    }

    #[test]
    fn exit_code_two_fail_returns_two() {
        assert_eq!(compute_exit_code(2, false), 2);
    }

    #[test]
    fn exit_code_framework_error_returns_255() {
        // framework error wins regardless of fail count — even zero fails.
        assert_eq!(compute_exit_code(0, true), 255);
        assert_eq!(compute_exit_code(7, true), 255);
    }

    #[test]
    fn exit_code_clamps_at_254() {
        // 255 is reserved for framework error; fail counts can never collide.
        assert_eq!(compute_exit_code(254, false), 254);
        assert_eq!(compute_exit_code(255, false), 254);
        assert_eq!(compute_exit_code(10_000, false), 254);
    }

    // -----------------------------------------------------------------------
    // BWOC-27 evidence kinds — attestation + sample + orthogonal time-bounded
    // fields. The dispatcher enforces enum + required sub-fields; per-criterion
    // expected_evidence_kind validation lives in `bwoc check` (BWOC-29).
    // -----------------------------------------------------------------------

    #[test]
    fn finding_attestation_happy_path() {
        let v = json!({
            "criterion_id": "9001-management-review",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "attestation",
                "value": "Management review held 2026-04-15.",
                "signer": "Quality Manager: Tonkla K.",
                "signed_at": "2026-04-15",
                "valid_through": "2027-04-15",
            },
        });
        let f = parse_finding(&v, 0).unwrap();
        assert_eq!(f.evidence_kind, "attestation");
        assert_eq!(
            f.evidence_extras.signer.as_deref(),
            Some("Quality Manager: Tonkla K.")
        );
        assert_eq!(f.evidence_extras.signed_at.as_deref(), Some("2026-04-15"));
        assert_eq!(
            f.evidence_extras.valid_through.as_deref(),
            Some("2027-04-15")
        );

        let out = f.to_json();
        assert_eq!(out["evidence"]["kind"], "attestation");
        assert_eq!(out["evidence"]["signer"], "Quality Manager: Tonkla K.");
        assert_eq!(out["evidence"]["signed_at"], "2026-04-15");
        assert_eq!(out["evidence"]["valid_through"], "2027-04-15");
    }

    #[test]
    fn finding_attestation_requires_signer() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "attestation",
                "value": "statement text",
                "signed_at": "2026-04-15",
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("requires 'signer'"), "{e}");
    }

    #[test]
    fn finding_attestation_requires_signed_at() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "attestation",
                "value": "statement",
                "signer": "X",
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("requires 'signed_at'"), "{e}");
    }

    #[test]
    fn finding_attestation_rejects_empty_signer() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "attestation",
                "value": "statement",
                "signer": "",
                "signed_at": "2026-04-15",
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("non-empty 'signer'"), "{e}");
    }

    #[test]
    fn finding_sample_happy_path() {
        let v = json!({
            "criterion_id": "20000-1-incident-management",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "sample",
                "value": "49 of 50 incidents resolved within SLA",
                "sampled_count": 49,
                "sampled_of": 50,
                "window": "2026-Q1",
            },
        });
        let f = parse_finding(&v, 0).unwrap();
        assert_eq!(f.evidence_kind, "sample");
        assert_eq!(f.evidence_extras.sampled_count, Some(49));
        assert_eq!(f.evidence_extras.sampled_of, Some(50));
        assert_eq!(f.evidence_extras.window.as_deref(), Some("2026-Q1"));

        let out = f.to_json();
        assert_eq!(out["evidence"]["sampled_count"], 49);
        assert_eq!(out["evidence"]["sampled_of"], 50);
        assert_eq!(out["evidence"]["window"], "2026-Q1");
    }

    #[test]
    fn finding_sample_requires_sampled_count() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "sample",
                "value": "summary",
                "sampled_of": 50,
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'sampled_count'"), "{e}");
    }

    #[test]
    fn finding_sample_requires_sampled_of() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "sample",
                "value": "summary",
                "sampled_count": 49,
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'sampled_of'"), "{e}");
    }

    #[test]
    fn finding_sample_rejects_count_greater_than_of() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "sample",
                "value": "summary",
                "sampled_count": 60,
                "sampled_of": 50,
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("must be ≥ 'sampled_count'"), "{e}");
    }

    #[test]
    fn finding_sample_rejects_negative_count() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "sample",
                "value": "summary",
                "sampled_count": -1,
                "sampled_of": 50,
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("must be ≥ 0"), "{e}");
    }

    #[test]
    fn finding_attestation_drops_unset_fields_in_json() {
        // Optional `valid_through` omitted — to_json must NOT emit the key.
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "attestation",
                "value": "statement",
                "signer": "X",
                "signed_at": "2026-04-15",
            },
        });
        let f = parse_finding(&v, 0).unwrap();
        let out = f.to_json();
        let ev = out["evidence"].as_object().unwrap();
        assert!(!ev.contains_key("valid_through"));
        assert!(!ev.contains_key("as_of"));
        assert!(!ev.contains_key("window"));
        assert!(!ev.contains_key("sampled_count"));
    }

    #[test]
    fn finding_file_finding_drops_attestation_subfields_in_json() {
        // Sub-fields not declared for `file` are still passed through if the
        // plugin sends them (dispatcher does not strip), but `to_json` only
        // emits what's set in `EvidenceExtras` — and `signer` IS set here
        // because we extract regardless of kind.
        let v = json!({
            "criterion_id": "a-b",
            "severity": "low",
            "status": "pass",
            "evidence": {
                "kind": "file",
                "value": "X.md",
            },
        });
        let f = parse_finding(&v, 0).unwrap();
        let out = f.to_json();
        let ev = out["evidence"].as_object().unwrap();
        assert_eq!(ev["kind"], "file");
        assert_eq!(ev["value"], "X.md");
        assert!(!ev.contains_key("signer"));
        assert!(!ev.contains_key("signed_at"));
    }

    #[test]
    fn finding_attestation_string_sub_field_must_be_string() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "attestation",
                "value": "statement",
                "signer": 42,
                "signed_at": "2026-04-15",
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(e.contains("'evidence.signer' must be a string"), "{e}");
    }

    #[test]
    fn finding_sample_integer_sub_field_must_be_integer() {
        let v = json!({
            "criterion_id": "a-b",
            "severity": "high",
            "status": "pass",
            "evidence": {
                "kind": "sample",
                "value": "summary",
                "sampled_count": "49",
                "sampled_of": 50,
            },
        });
        let e = parse_finding(&v, 0).unwrap_err();
        assert!(
            e.contains("'evidence.sampled_count' must be an integer"),
            "{e}"
        );
    }
}
