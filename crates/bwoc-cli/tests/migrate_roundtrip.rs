//! End-to-end contract for `bwoc migrate`: a 2.x workspace becomes a 3.x
//! workspace, and **nothing else changes**.
//!
//! The unit tests in `src/migrate.rs` prove each splice in isolation. This one
//! proves the property that actually matters to an operator upgrading a fleet:
//! the command edits six formats across a workspace and its agents while
//! comments, key order, and every key BWOC does not model survive byte-for-byte.
//! That is not a nicety — `Manifest::save_to_path` and `Workspace::save` both
//! drop unmodeled keys, so a migration that reserialized would silently delete
//! an operator's `skills.framework[]` and `[plugins.*]` while claiming to
//! upgrade them.
//!
//! Also pins the two behaviours a fleet upgrade depends on: re-running is a
//! no-op, and an artifact from a *newer* BWOC is reported rather than
//! rewritten.
//!
//! Unix-only for the same reason as `smoke.rs` — the fixture is written by the
//! test itself, but the assertions compare exact file text and Windows line
//! endings would make that a false failure.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc"))
}

const WORKSPACE_TOML: &str = "\
# operator's banner — keep me
[workspace]
name = \"demo\"
version = \"0.1.0\"
created = \"2026-05-22T06:00:00Z\"

[plugins.jira-cloud-rest]
enabled = true
";

const AGENTS_TOML: &str = "\
[[agent]]
id = \"agent-demo\"
path = \"agents/agent-demo\"
backend = \"claude\"
incarnated = \"2026-05-22T06:00:00Z\"
status = \"active\"
";

const ROUTES_TOML: &str = "\
[[route]]
agent = \"agent-peer\"
workspace = \"/tmp/peer\"
";

const MANIFEST_JSON: &str = "\
{
  \"agentId\": \"agent-demo\",
  \"version\": \"2.0\",
  \"skills\": { \"framework\": [\"yoniso-check\"] },
  \"trust\": { \"schemaVersion\": 1 }
}
";

const AGENTS_MD: &str = "\
# AGENTS.md

| | |
|---|---|
| **Version** | 2.0 |
| **Date** | 2026-05-22 |
";

const POLICY_TOML: &str = "\
# operator notes: run_command stays denied
default_mode = \"ask\"

[tools]
run_command = \"deny\"  # never relax this
";

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

/// A workspace exactly as BWOC 2.x left it: not one `schema_version` anywhere.
fn seed_v2_workspace(ws: &Path) {
    write(&ws.join(".bwoc/workspace.toml"), WORKSPACE_TOML);
    write(&ws.join(".bwoc/agents.toml"), AGENTS_TOML);
    write(&ws.join(".bwoc/interconnect/routes.toml"), ROUTES_TOML);

    let agent = ws.join("agents/agent-demo");
    write(&agent.join("config.manifest.json"), MANIFEST_JSON);
    write(&agent.join("AGENTS.md"), AGENTS_MD);
    write(&agent.join(".bwoc/harness-policy.toml"), POLICY_TOML);
}

fn run(ws: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(bin())
        .args(args)
        .current_dir(ws)
        .output()
        .expect("spawn bwoc migrate");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_v2_workspace_migrates_without_losing_anything() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();
    seed_v2_workspace(ws);

    // ── Dry run writes nothing ───────────────────────────────────────────────
    let (code, stdout) = run(ws, &["migrate", "--all", "--dry-run"]);
    assert_eq!(code, 0, "dry run should succeed:\n{stdout}");
    assert_eq!(
        read(ws.join(".bwoc/workspace.toml")),
        WORKSPACE_TOML,
        "--dry-run must not write"
    );

    // ── Apply ────────────────────────────────────────────────────────────────
    let (code, stdout) = run(ws, &["migrate", "--all", "--yes", "--json"]);
    assert_eq!(code, 0, "migrate should succeed:\n{stdout}");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("json report");
    assert_eq!(report["summary"]["migrated"], 6, "{stdout}");
    assert_eq!(report["summary"]["failed"], 0, "{stdout}");
    assert_eq!(report["schema"]["current"], 3);

    // ── Every artifact now declares v3 ───────────────────────────────────────
    for rel in [
        ".bwoc/workspace.toml",
        ".bwoc/agents.toml",
        ".bwoc/interconnect/routes.toml",
        "agents/agent-demo/.bwoc/harness-policy.toml",
    ] {
        let text = read(ws.join(rel));
        let value: toml::Value = toml::from_str(&text).unwrap_or_else(|e| {
            panic!("{rel} no longer parses after migration: {e}\n{text}");
        });
        assert_eq!(
            value
                .get("schema_version")
                .and_then(toml::Value::as_integer),
            Some(3),
            "{rel} missing the marker:\n{text}"
        );
    }

    // ── …and nothing else moved ──────────────────────────────────────────────
    let ws_toml = read(ws.join(".bwoc/workspace.toml"));
    assert!(
        ws_toml.contains("# operator's banner — keep me"),
        "comment lost:\n{ws_toml}"
    );
    assert!(
        ws_toml.contains("[plugins.jira-cloud-rest]") && ws_toml.contains("enabled = true"),
        "an unmodeled table was dropped — this is the reserialization bug:\n{ws_toml}"
    );

    let policy = read(ws.join("agents/agent-demo/.bwoc/harness-policy.toml"));
    assert!(policy.contains("# operator notes: run_command stays denied"));
    assert!(
        policy.contains("run_command = \"deny\"  # never relax this"),
        "trailing comment lost — a policy file is human-authored:\n{policy}"
    );

    let manifest: serde_json::Value =
        serde_json::from_str(&read(ws.join("agents/agent-demo/config.manifest.json"))).unwrap();
    assert_eq!(manifest["version"], "3.0", "spec version not bumped");
    assert_eq!(
        manifest["skills"]["framework"][0], "yoniso-check",
        "an unmodeled manifest block was dropped"
    );
    assert_eq!(
        manifest["trust"]["schemaVersion"], 1,
        "the trust sub-spec version is orthogonal and must not be renumbered"
    );

    let agents_md = read(ws.join("agents/agent-demo/AGENTS.md"));
    assert!(agents_md.contains("| **Version** | 3.0 |"), "{agents_md}");
    assert!(
        agents_md.contains("| **Date** | 2026-05-22 |"),
        "the rest of the header table must survive:\n{agents_md}"
    );

    // ── Originals are kept, inside the control plane ─────────────────────────
    let backup_root = ws.join(".bwoc/migrate-backup");
    assert!(backup_root.is_dir(), "no backup directory");
    let stamp = std::fs::read_dir(&backup_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        read(stamp.join(".bwoc/workspace.toml")),
        WORKSPACE_TOML,
        "backup must be the untouched original"
    );

    // ── The workspace still works ────────────────────────────────────────────
    let (code, stdout) = run(ws, &["workspace", "validate"]);
    assert_eq!(code, 0, "migrated workspace no longer validates:\n{stdout}");

    // ── Re-running changes nothing ───────────────────────────────────────────
    let before = read(ws.join(".bwoc/workspace.toml"));
    let (code, stdout) = run(ws, &["migrate", "--all", "--yes", "--json"]);
    assert_eq!(code, 0, "{stdout}");
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        report["summary"]["migrated"], 0,
        "second run must be a no-op"
    );
    assert_eq!(report["summary"]["already_current"], 6);
    assert_eq!(
        read(ws.join(".bwoc/workspace.toml")),
        before,
        "second run rewrote a current file"
    );
}

#[test]
fn an_artifact_from_a_newer_bwoc_is_reported_not_rewritten() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();
    seed_v2_workspace(ws);

    let policy = ws.join("agents/agent-demo/.bwoc/harness-policy.toml");
    let from_the_future = format!("schema_version = 99\n{POLICY_TOML}");
    std::fs::write(&policy, &from_the_future).unwrap();

    let (code, stdout) = run(ws, &["migrate", "--all", "--yes", "--json"]);
    assert_eq!(
        code, 3,
        "a future artifact must exit 3 — the operator's next move is to upgrade bwoc, \
         not to fix the file:\n{stdout}"
    );
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["summary"]["ahead"], 1, "{stdout}");
    assert_eq!(
        std::fs::read_to_string(&policy).unwrap(),
        from_the_future,
        "a file this build does not understand must be left exactly as found"
    );
}

#[test]
fn json_without_yes_is_refused() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();
    seed_v2_workspace(ws);

    let (code, _) = run(ws, &["migrate", "--all", "--json"]);
    assert_eq!(
        code, 2,
        "--json writes without prompting, so it requires --yes"
    );
    assert_eq!(
        read(ws.join(".bwoc/workspace.toml")),
        WORKSPACE_TOML,
        "a refused invocation must not have written"
    );
}
