//! `bwoc migrate` — bring a workspace's on-disk artifacts up to the current schema.
//!
//! # Why this command exists
//!
//! 3.0 reads everything 2.x wrote (see [`bwoc_core::schema`] for the seam), so
//! nothing breaks on upgrade day. But that tolerance ends at 4.0, and an
//! operator needs one command that moves a fleet forward rather than a
//! checklist of hand edits across six file formats.
//!
//! # Splice, never reserialize
//!
//! The load-bearing decision. Two of the natural round-trips are **lossy**:
//!
//! - [`bwoc_core::manifest::Manifest`] models no catch-all, so a save drops any
//!   key it does not know — `skills.framework[]`, written raw by `bwoc skill
//!   enable`, is the live example.
//! - [`bwoc_core::workspace::Workspace`] models only `[workspace]` and
//!   `[defaults]`, so a save drops `[plugins.*]`.
//!
//! A migration that silently deleted an operator's plugin config while
//! "upgrading" it would be worse than no migration at all. So every writer here
//! **parses only to decide**, then edits the file's text: the marker is spliced
//! in as a line, the spec version is replaced in place. Comments, key order and
//! unmodeled keys survive byte-for-byte. Every write re-parses before it lands,
//! and refuses if the result would not read back.
//!
//! # Backups live under `.bwoc/`
//!
//! Not beside the original. The harness's control-plane gate
//! (`bwoc-harness`'s `is_control_plane`) protects any path with a `.bwoc`
//! component, plus the exact name `config.manifest.json` — so a backup written
//! as `<agent>/config.manifest.json.bak` would sit *outside* the control plane,
//! and an untrusted turn could plant a poisoned "backup" for an operator to
//! restore. Under `.bwoc/migrate-backup/<timestamp>/` it inherits the existing
//! protection with no change to the gate.

use std::path::{Path, PathBuf};

use bwoc_core::schema::SchemaVersion;
use bwoc_core::workspace::AgentsRegistry;

use crate::util::utc_now_iso8601;

/// The spec version an agent's `config.manifest.json` and `AGENTS.md` declare
/// once migrated. Tracks the framework specification, not the schema integer —
/// see `VERSION.md` §Specification.
const SPEC_VERSION_CURRENT: &str = "3.0";
/// What 2.x agents declare.
const SPEC_VERSION_LEGACY: &str = "2.0";

pub struct MigrateArgs {
    /// Workspace root or agent directory. Defaults to the current directory.
    /// Mutually exclusive with `--all`.
    pub path: Option<PathBuf>,
    /// Migrate the workspace plus every agent registered in it.
    pub all: bool,
    /// Workspace root (only meaningful with `--all`).
    pub workspace: Option<PathBuf>,
    /// Report what would change; write nothing.
    pub dry_run: bool,
    /// Machine-readable report. Requires `--yes`.
    pub json: bool,
    /// Proceed without the confirmation prompt.
    pub yes: bool,
    /// Skip the backup copy. The operator is asserting they have their own.
    pub no_backup: bool,
}

/// What happened to one artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Action {
    /// Rewritten to the current schema.
    Migrated,
    /// Would be rewritten (`--dry-run`).
    WouldMigrate,
    /// Already current — nothing to do. Re-running is a no-op.
    AlreadyCurrent,
    /// Not present in this workspace/agent. Not a problem.
    Absent,
    /// Declares a schema newer than this build understands.
    Ahead,
    /// Unreadable, unparseable, or unwritable.
    Failed,
}

#[derive(Debug, serde::Serialize)]
struct TargetReport {
    path: String,
    from: String,
    to: String,
    action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Artifacts rooted at a workspace directory.
const WORKSPACE_ARTIFACTS: &[&str] = &[
    ".bwoc/workspace.toml",
    ".bwoc/agents.toml",
    ".bwoc/interconnect/routes.toml",
    ".bwoc/harness-policy.toml",
    ".bwoc/peers.toml",
];

/// Artifacts rooted at an agent directory.
const AGENT_ARTIFACTS: &[&str] = &[
    "config.manifest.json",
    "AGENTS.md",
    ".bwoc/harness-policy.toml",
    ".bwoc/peers.toml",
];

pub fn run(args: MigrateArgs) -> i32 {
    if args.json && !args.yes {
        eprintln!("bwoc migrate: --json requires --yes (it writes without prompting)");
        return 2;
    }

    let roots = match resolve_roots(&args) {
        Ok(r) => r,
        Err(code) => return code,
    };

    let mut reports: Vec<TargetReport> = Vec::new();
    let backup_stamp = utc_now_iso8601().replace(':', "-");

    for (root, artifacts) in &roots {
        for rel in *artifacts {
            reports.push(migrate_artifact(
                root,
                rel,
                args.dry_run,
                args.no_backup,
                &backup_stamp,
            ));
        }
    }

    let migrated = count(&reports, Action::Migrated);
    let would = count(&reports, Action::WouldMigrate);
    let current = count(&reports, Action::AlreadyCurrent);
    let failed = count(&reports, Action::Failed);
    let ahead = count(&reports, Action::Ahead);

    if args.json {
        let value = serde_json::json!({
            "targets": reports,
            "summary": {
                "migrated": migrated,
                "would_migrate": would,
                "already_current": current,
                "failed": failed,
                "ahead": ahead,
                "dry_run": args.dry_run,
            },
            "schema": {
                "current": SchemaVersion::CURRENT.0,
                "oldest_supported": SchemaVersion::LEGACY.0,
                "spec": SPEC_VERSION_CURRENT,
            },
        });
        println!(
            "{}",
            serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        print_report(&reports, &args, migrated, would, current, failed, ahead);
    }

    // An artifact from a newer BWOC outranks a write failure: the operator's
    // next action is "upgrade bwoc", not "fix this file".
    if ahead > 0 {
        return 3;
    }
    if failed > 0 {
        return 1;
    }
    0
}

/// Resolve what to migrate: an explicit path, or a whole fleet under `--all`.
fn resolve_roots(args: &MigrateArgs) -> Result<Vec<(PathBuf, &'static [&'static str])>, i32> {
    if args.all {
        let root = match resolve_workspace(args.workspace.as_deref()) {
            Some(r) => r,
            None => {
                eprintln!(
                    "bwoc migrate --all: no workspace found. Pass --workspace, set \
                     BWOC_WORKSPACE, or run from a workspace directory."
                );
                return Err(2);
            }
        };
        let registry = match AgentsRegistry::load(&root) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("bwoc migrate --all: failed to read agents.toml: {e}");
                return Err(1);
            }
        };
        let mut roots: Vec<(PathBuf, &'static [&'static str])> =
            vec![(root.clone(), WORKSPACE_ARTIFACTS)];
        for entry in &registry.agents {
            roots.push((entry.dir(&root), AGENT_ARTIFACTS));
        }
        return Ok(roots);
    }

    let target = args
        .path
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if !target.is_dir() {
        eprintln!("bwoc migrate: not a directory: {}", target.display());
        return Err(2);
    }

    // Detect what the operator pointed at rather than making them say. A
    // workspace root and an agent directory are told apart by the artifact each
    // is required to have.
    if target.join(".bwoc/workspace.toml").is_file() {
        Ok(vec![(target, WORKSPACE_ARTIFACTS)])
    } else if target.join("config.manifest.json").is_file() {
        Ok(vec![(target, AGENT_ARTIFACTS)])
    } else {
        eprintln!(
            "bwoc migrate: {} is neither a workspace (.bwoc/workspace.toml) nor an \
             agent (config.manifest.json). Pass --all to migrate a whole fleet.",
            target.display()
        );
        Err(2)
    }
}

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

fn count(reports: &[TargetReport], action: Action) -> usize {
    reports.iter().filter(|r| r.action == action).count()
}

// ── One artifact ─────────────────────────────────────────────────────────────

fn migrate_artifact(
    root: &Path,
    rel: &str,
    dry_run: bool,
    no_backup: bool,
    stamp: &str,
) -> TargetReport {
    let path = root.join(rel);
    let display = path.display().to_string();

    let mut report = TargetReport {
        path: display.clone(),
        from: String::new(),
        to: String::new(),
        action: Action::Absent,
        backup: None,
        error: None,
    };

    if !path.is_file() {
        return report;
    }

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            report.action = Action::Failed;
            report.error = Some(format!("cannot read: {e}"));
            return report;
        }
    };

    let outcome = if rel.ends_with(".json") {
        plan_spec_version(&text, SpecFormat::Json)
    } else if rel.ends_with(".md") {
        plan_spec_version(&text, SpecFormat::Markdown)
    } else {
        plan_toml_marker(&text)
    };

    let (from, to, rewritten) = match outcome {
        Ok(Plan::AlreadyCurrent { at }) => {
            report.from = at.clone();
            report.to = at;
            report.action = Action::AlreadyCurrent;
            return report;
        }
        Ok(Plan::Ahead { at }) => {
            report.from = at;
            report.to = SchemaVersion::CURRENT.0.to_string();
            report.action = Action::Ahead;
            report.error = Some(
                "declares a schema newer than this bwoc understands — upgrade bwoc".to_string(),
            );
            return report;
        }
        Ok(Plan::Rewrite { from, to, text }) => (from, to, text),
        Err(e) => {
            report.action = Action::Failed;
            report.error = Some(e);
            return report;
        }
    };

    report.from = from;
    report.to = to;

    if dry_run {
        report.action = Action::WouldMigrate;
        return report;
    }

    if !no_backup {
        match write_backup(root, rel, &text, stamp) {
            Ok(p) => report.backup = Some(p.display().to_string()),
            Err(e) => {
                report.action = Action::Failed;
                report.error = Some(format!("cannot write backup (nothing changed): {e}"));
                return report;
            }
        }
    }

    match std::fs::write(&path, &rewritten) {
        Ok(()) => report.action = Action::Migrated,
        Err(e) => {
            report.action = Action::Failed;
            report.error = Some(format!("cannot write: {e}"));
        }
    }
    report
}

/// The original, kept under the control plane so the harness gate covers it.
///
/// The relative path is preserved inside the backup directory rather than
/// flattened, so restoring is a plain recursive copy back over the root and an
/// operator can see at a glance which file each copy came from.
fn write_backup(root: &Path, rel: &str, text: &str, stamp: &str) -> std::io::Result<PathBuf> {
    let dest = root.join(".bwoc/migrate-backup").join(stamp).join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, text)?;
    Ok(dest)
}

// ── Planning: parse to decide, splice to write ───────────────────────────────

enum Plan {
    AlreadyCurrent {
        at: String,
    },
    Ahead {
        at: String,
    },
    Rewrite {
        from: String,
        to: String,
        text: String,
    },
}

/// Add `schema_version = N` to a TOML file that lacks it.
fn plan_toml_marker(text: &str) -> Result<Plan, String> {
    let parsed: toml::Value =
        toml::from_str(text).map_err(|e| format!("cannot parse TOML: {e}"))?;
    let found = bwoc_core::schema::marker_from_toml(&parsed);

    // The presence test runs on the *parsed* value, not a substring, so a
    // `schema_version` nested inside some `[plugins.foo]` table does not count
    // as the file's own marker.
    let has_marker = parsed.get("schema_version").is_some();
    if has_marker {
        if found.is_future() {
            return Ok(Plan::Ahead {
                at: found.0.to_string(),
            });
        }
        if found.is_current() {
            return Ok(Plan::AlreadyCurrent {
                at: found.0.to_string(),
            });
        }
    }

    let spliced = splice_toml_marker(text, SchemaVersion::CURRENT);
    verify_toml(&spliced)?;
    Ok(Plan::Rewrite {
        from: found.0.to_string(),
        to: SchemaVersion::CURRENT.0.to_string(),
        text: spliced,
    })
}

/// Insert the marker above the first line that is neither blank nor a comment.
///
/// That line is either the file's first key or its first `[table]` header, so
/// the marker always lands where TOML requires a top-level scalar — above every
/// table — while staying below whatever banner comment the file opens with.
fn splice_toml_marker(text: &str, version: SchemaVersion) -> String {
    let marker = format!(
        "# Schema revision of this file — written by `bwoc migrate`.\nschema_version = {}\n",
        version.0
    );
    let mut out = String::with_capacity(text.len() + marker.len() + 1);
    let mut inserted = false;
    for line in text.lines() {
        let t = line.trim_start();
        if !inserted && !t.is_empty() && !t.starts_with('#') {
            out.push_str(&marker);
            out.push('\n');
            inserted = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !inserted {
        // Comments and blank lines only (or an empty file) — append.
        out.push_str(&marker);
    }
    out
}

fn verify_toml(text: &str) -> Result<(), String> {
    toml::from_str::<toml::Value>(text)
        .map(|_| ())
        .map_err(|e| format!("refusing to write: result would not parse as TOML: {e}"))
}

enum SpecFormat {
    /// `config.manifest.json` — the top-level `"version"` key.
    Json,
    /// `AGENTS.md` — the `| **Version** | X.Y |` table row.
    Markdown,
}

/// Move an agent's declared specification version from 2.0 to 3.0.
fn plan_spec_version(text: &str, format: SpecFormat) -> Result<Plan, String> {
    match format {
        SpecFormat::Json => {
            let value: serde_json::Value =
                serde_json::from_str(text).map_err(|e| format!("cannot parse JSON: {e}"))?;
            let current = value
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "no top-level \"version\" string".to_string())?;
            if current == SPEC_VERSION_CURRENT {
                return Ok(Plan::AlreadyCurrent {
                    at: current.to_string(),
                });
            }
            if current != SPEC_VERSION_LEGACY {
                return Err(format!(
                    "declares specification version {current}, which this bwoc does not know how \
                     to migrate (expected {SPEC_VERSION_LEGACY})"
                ));
            }
            let needle = format!("\"version\": \"{SPEC_VERSION_LEGACY}\"");
            let replacement = format!("\"version\": \"{SPEC_VERSION_CURRENT}\"");
            let spliced = replace_first(text, &needle, &replacement).ok_or_else(|| {
                format!("could not locate `{needle}` verbatim — refusing to guess at the edit")
            })?;
            verify_spec_json(text, &spliced)?;
            Ok(Plan::Rewrite {
                from: current.to_string(),
                to: SPEC_VERSION_CURRENT.to_string(),
                text: spliced,
            })
        }
        SpecFormat::Markdown => {
            let current = agents_md_version(text)
                .ok_or_else(|| "no `| **Version** | X.Y |` row".to_string())?;
            if current == SPEC_VERSION_CURRENT {
                return Ok(Plan::AlreadyCurrent { at: current });
            }
            if current != SPEC_VERSION_LEGACY {
                return Err(format!(
                    "declares specification version {current}, which this bwoc does not know how \
                     to migrate (expected {SPEC_VERSION_LEGACY})"
                ));
            }
            let spliced = replace_agents_md_version(text, SPEC_VERSION_CURRENT)
                .ok_or_else(|| "could not rewrite the version row".to_string())?;
            Ok(Plan::Rewrite {
                from: current,
                to: SPEC_VERSION_CURRENT.to_string(),
                text: spliced,
            })
        }
    }
}

/// The spliced JSON must differ from the original in exactly the top-level
/// `version` and nothing else — the guard against a same-shaped `"version":
/// "2.0"` further down the file being the one that got replaced.
fn verify_spec_json(original: &str, spliced: &str) -> Result<(), String> {
    let mut before: serde_json::Value = serde_json::from_str(original)
        .map_err(|e| format!("refusing to write: original no longer parses: {e}"))?;
    let after: serde_json::Value = serde_json::from_str(spliced)
        .map_err(|e| format!("refusing to write: result would not parse as JSON: {e}"))?;
    if let Some(v) = before.get_mut("version") {
        *v = serde_json::Value::String(SPEC_VERSION_CURRENT.to_string());
    }
    if before == after {
        Ok(())
    } else {
        Err("refusing to write: the edit changed more than the top-level version".to_string())
    }
}

fn replace_first(text: &str, needle: &str, replacement: &str) -> Option<String> {
    let at = text.find(needle)?;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..at]);
    out.push_str(replacement);
    out.push_str(&text[at + needle.len()..]);
    Some(out)
}

/// Read the `| **Version** | X.Y |` row out of an `AGENTS.md` header table.
fn agents_md_version(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let t = line.trim();
        if !t.starts_with('|') || !t.contains("**Version**") {
            return None;
        }
        t.split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty() && !c.contains("**Version**"))
            .map(str::to_string)
            .next()
    })
}

fn replace_agents_md_version(text: &str, to: &str) -> Option<String> {
    let mut done = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        if !done && t.starts_with('|') && t.contains("**Version**") {
            // Rebuild the row rather than string-replacing "2.0", which could
            // collide with a version elsewhere on the line.
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push_str(indent);
            out.push_str(&format!("| **Version** | {to} |"));
            done = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    done.then_some(out)
}

// ── Human report ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn print_report(
    reports: &[TargetReport],
    args: &MigrateArgs,
    migrated: usize,
    would: usize,
    current: usize,
    failed: usize,
    ahead: usize,
) {
    println!();
    if args.dry_run {
        println!("bwoc migrate — dry run (nothing written)");
    } else {
        println!("bwoc migrate — schema v{}", SchemaVersion::CURRENT.0);
    }
    println!("==============================");

    for r in reports {
        match r.action {
            Action::Absent => continue,
            Action::AlreadyCurrent => println!("  ok      {} (v{})", r.path, r.to),
            Action::Migrated => println!("  MIGRATED {} v{} → v{}", r.path, r.from, r.to),
            Action::WouldMigrate => println!("  would   {} v{} → v{}", r.path, r.from, r.to),
            Action::Ahead => println!(
                "  AHEAD   {} declares v{} — this bwoc understands v{}",
                r.path,
                r.from,
                SchemaVersion::CURRENT.0
            ),
            Action::Failed => println!(
                "  FAILED  {} — {}",
                r.path,
                r.error.as_deref().unwrap_or("unknown error")
            ),
        }
    }

    println!("==============================");
    if args.dry_run {
        println!(
            "{would} would migrate, {current} already current, {failed} failed, {ahead} ahead"
        );
        if would > 0 {
            println!("Re-run without --dry-run to apply.");
        }
    } else {
        println!("{migrated} migrated, {current} already current, {failed} failed, {ahead} ahead");
        if migrated > 0 && !args.no_backup {
            println!("Originals kept under <root>/.bwoc/migrate-backup/");
        }
    }
    if ahead > 0 {
        println!("Upgrade bwoc — an artifact here was written by a newer build.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TOML marker ──────────────────────────────────────────────────────────

    #[test]
    fn marker_lands_above_the_first_table() {
        let src = "# banner comment\n\n[[route]]\nagent = 'a'\n";
        let out = splice_toml_marker(src, SchemaVersion::CURRENT);
        assert!(out.starts_with("# banner comment"), "{out}");
        assert!(out.find("schema_version").unwrap() < out.find("[[route]]").unwrap());
        // And the result is still readable.
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(
            bwoc_core::schema::marker_from_toml(&v),
            SchemaVersion::CURRENT
        );
    }

    #[test]
    fn marker_preserves_comments_and_unmodeled_keys() {
        let src = "\
# operator's notes — keep me
default_mode = \"allow\"

[tools]
read_file = \"allow\"  # trailing comment

[plugins.something-we-do-not-model]
enabled = true
";
        let out = splice_toml_marker(src, SchemaVersion::CURRENT);
        assert!(out.contains("# operator's notes — keep me"));
        assert!(out.contains("# trailing comment"));
        assert!(out.contains("[plugins.something-we-do-not-model]"));
        assert!(toml::from_str::<toml::Value>(&out).is_ok());
    }

    #[test]
    fn already_current_is_a_noop() {
        let src = "schema_version = 3\ndefault_mode = \"allow\"\n";
        assert!(matches!(
            plan_toml_marker(src).unwrap(),
            Plan::AlreadyCurrent { .. }
        ));
    }

    #[test]
    fn a_future_marker_is_reported_not_rewritten() {
        let src = "schema_version = 99\ndefault_mode = \"allow\"\n";
        assert!(matches!(plan_toml_marker(src).unwrap(), Plan::Ahead { .. }));
    }

    #[test]
    fn a_nested_marker_does_not_count_as_the_files_own() {
        let src = "[plugins.foo]\nschema_version = 3\n";
        let Plan::Rewrite { text, .. } = plan_toml_marker(src).unwrap() else {
            panic!("a nested key must not satisfy the file's own marker");
        };
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            bwoc_core::schema::marker_from_toml(&v),
            SchemaVersion::CURRENT
        );
    }

    #[test]
    fn splicing_is_idempotent_through_the_planner() {
        let src = "default_mode = \"deny\"\n";
        let Plan::Rewrite { text, .. } = plan_toml_marker(src).unwrap() else {
            panic!("first pass should rewrite")
        };
        assert!(matches!(
            plan_toml_marker(&text).unwrap(),
            Plan::AlreadyCurrent { .. }
        ));
    }

    #[test]
    fn unparseable_toml_fails_rather_than_mangles() {
        assert!(plan_toml_marker("not valid toml [[[").is_err());
    }

    #[test]
    fn comment_only_file_still_gets_a_marker() {
        let out = splice_toml_marker("# nothing but a comment\n", SchemaVersion::CURRENT);
        let v: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(
            bwoc_core::schema::marker_from_toml(&v),
            SchemaVersion::CURRENT
        );
    }

    // ── Manifest spec version ────────────────────────────────────────────────

    #[test]
    fn manifest_version_moves_and_nothing_else_does() {
        let src = "{\n  \"agentId\": \"agent-a\",\n  \"version\": \"2.0\",\n  \"skills\": { \"framework\": [\"x\"] }\n}\n";
        let Plan::Rewrite { text, from, to } = plan_spec_version(src, SpecFormat::Json).unwrap()
        else {
            panic!("expected a rewrite")
        };
        assert_eq!((from.as_str(), to.as_str()), ("2.0", "3.0"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], "3.0");
        // The unmodeled block survives — this is the whole reason for splicing.
        assert_eq!(v["skills"]["framework"][0], "x");
    }

    #[test]
    fn manifest_already_at_3_is_a_noop() {
        let src = "{ \"version\": \"3.0\" }";
        assert!(matches!(
            plan_spec_version(src, SpecFormat::Json).unwrap(),
            Plan::AlreadyCurrent { .. }
        ));
    }

    #[test]
    fn an_unknown_manifest_version_is_refused() {
        let src = "{ \"version\": \"1.4\" }";
        assert!(plan_spec_version(src, SpecFormat::Json).is_err());
    }

    #[test]
    fn a_nested_version_2_0_is_not_the_one_replaced() {
        // The top-level key comes first, so the first-match replace hits it —
        // and `verify_spec_json` proves nothing else moved.
        let src = "{\n  \"version\": \"2.0\",\n  \"plugin\": { \"version\": \"2.0\" }\n}\n";
        let Plan::Rewrite { text, .. } = plan_spec_version(src, SpecFormat::Json).unwrap() else {
            panic!("expected a rewrite")
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["version"], "3.0");
        assert_eq!(
            v["plugin"]["version"], "2.0",
            "nested version must not move"
        );
    }

    // ── AGENTS.md spec version ───────────────────────────────────────────────

    #[test]
    fn agents_md_version_row_is_read_and_rewritten() {
        let src =
            "# AGENTS.md\n\n| | |\n|---|---|\n| **Version** | 2.0 |\n| **Date** | 2026-05-22 |\n";
        assert_eq!(agents_md_version(src).as_deref(), Some("2.0"));
        let Plan::Rewrite { text, .. } = plan_spec_version(src, SpecFormat::Markdown).unwrap()
        else {
            panic!("expected a rewrite")
        };
        assert!(text.contains("| **Version** | 3.0 |"), "{text}");
        assert!(text.contains("| **Date** | 2026-05-22 |"), "rest survives");
        assert_eq!(agents_md_version(&text).as_deref(), Some("3.0"));
    }

    #[test]
    fn agents_md_without_a_version_row_is_an_error() {
        assert!(plan_spec_version("# AGENTS.md\n\nno table here\n", SpecFormat::Markdown).is_err());
    }

    // ── Backups ──────────────────────────────────────────────────────────────

    #[test]
    fn backups_live_inside_the_control_plane() {
        let root = std::env::temp_dir().join(format!("bwoc-migrate-bk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dest =
            write_backup(&root, ".bwoc/interconnect/routes.toml", "old", "2026-01-01").unwrap();

        let rel = dest.strip_prefix(&root).unwrap();
        assert!(
            rel.components().any(|c| c.as_os_str() == ".bwoc"),
            "a backup outside .bwoc/ escapes the harness control-plane gate: {}",
            rel.display()
        );
        // The original layout is preserved, so a restore is a recursive copy.
        assert!(
            dest.ends_with(".bwoc/interconnect/routes.toml"),
            "{}",
            dest.display()
        );
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
        let _ = std::fs::remove_dir_all(&root);
    }
}
