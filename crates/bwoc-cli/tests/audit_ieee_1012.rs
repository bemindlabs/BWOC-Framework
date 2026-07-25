//! End-to-end smoke for the `audit-ieee-1012` runtime.
//!
//! Verifies `bwoc audit run --plugin audit-ieee-1012 --json` against a tempdir
//! workspace declaring attestations — the shared attestation runtime exercised
//! for the IEEE 1012 verification-and-validation lane (the first IEEE-standalone
//! standard in the audit kind).
//!
//! Skipped on Windows for the same reason as `audit_iso_9001.rs`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc"))
}

fn install_plugin(workspace: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/plugins/audit-ieee-1012")
        .canonicalize()
        .expect("canonicalize audit-ieee-1012 plugin source");
    let dst = workspace.join("modules/plugins/audit-ieee-1012");
    std::fs::create_dir_all(&dst).expect("mkdir plugin dst");
    for entry in std::fs::read_dir(&src).expect("read plugin src dir") {
        let entry = entry.expect("read dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        std::fs::copy(&from, &to).expect("copy plugin file");
        let perms = std::fs::metadata(&from).expect("stat src").permissions();
        std::fs::set_permissions(&to, perms).expect("chmod dst");
    }
}

const WORKSPACE_TOML: &str = r#"[workspace]
name = "1012-smoke"
version = "0.1.0"

[plugins.audit-ieee-1012]
enabled = true

[[plugins.audit-ieee-1012.attestations]]
criterion_id  = "1012-test-vv"
statement     = "Every requirement maps to at least one automated test; CI runs the suite on each PR and blocks merge on failure."
signer        = "QA Lead: Naruemon K."
signed_at     = "2026-07-25"
valid_through = "2027-07-25"

[[plugins.audit-ieee-1012.attestations]]
criterion_id = "1012-vv-planning"
statement    = "An SVVP documents V&V scope, activities, and integrity-level scaling; reviewed each release."
signed_at    = "2026-07-20"
signer       = "V&V Lead: Anon P."
"#;

#[test]
fn audit_1012_emits_attestation_and_fail_findings() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    std::fs::create_dir_all(ws.join(".bwoc")).expect("mkdir .bwoc");
    std::fs::write(ws.join(".bwoc/workspace.toml"), WORKSPACE_TOML).expect("write workspace.toml");
    install_plugin(ws);

    let output = Command::new(bin())
        .args(["audit", "run", "--plugin", "audit-ieee-1012", "--json"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc audit run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // criteria.toml declares 8 criteria; 2 have attestations (pass), 6 do not
    // (fail). Exit code = fail count = 6.
    assert!(
        !stderr.contains("framework error"),
        "framework error in stderr:\n{stderr}\n\nstdout:\n{stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(6),
        "expected exit 6 (one per missing-attestation fail); stdout=\n{stdout}\nstderr=\n{stderr}"
    );

    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("parse --json envelope");
    let summary = &envelope["summary"];
    assert_eq!(summary["pass_count"], 2);
    assert_eq!(summary["fail_count"], 6);
    assert_eq!(summary["framework_error"], false);

    let runs = envelope["runs"].as_array().expect("runs is array");
    assert_eq!(runs[0]["plugin"], "audit-ieee-1012");
    let findings = runs[0]["findings"].as_array().expect("findings is array");
    assert_eq!(findings.len(), 8, "expected 8 findings (one per criterion)");

    let test_vv = findings
        .iter()
        .find(|f| f["criterion_id"] == "1012-test-vv")
        .expect("test-vv finding present");
    assert_eq!(test_vv["status"], "pass");
    assert_eq!(test_vv["evidence"]["kind"], "attestation");
    assert_eq!(test_vv["evidence"]["valid_through"], "2027-07-25");

    let integ = findings
        .iter()
        .find(|f| f["criterion_id"] == "1012-integrity-levels")
        .expect("integrity-levels finding present");
    assert_eq!(integ["status"], "fail");
    let remedy = integ["remedy"].as_str().expect("fail must carry remedy");
    assert!(
        remedy.contains("1012-integrity-levels"),
        "remedy names the criterion: {remedy}"
    );
}

#[test]
fn audit_1012_fails_all_when_no_attestations_present() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    std::fs::create_dir_all(ws.join(".bwoc")).expect("mkdir .bwoc");
    std::fs::write(
        ws.join(".bwoc/workspace.toml"),
        "[workspace]\nname = \"empty\"\nversion = \"0.1.0\"\n\n\
         [plugins.audit-ieee-1012]\nenabled = true\n",
    )
    .expect("write workspace.toml");
    install_plugin(ws);

    let output = Command::new(bin())
        .args(["audit", "run", "--plugin", "audit-ieee-1012", "--json"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc audit run");

    assert_eq!(
        output.status.code(),
        Some(8),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse --json envelope");
    assert_eq!(envelope["summary"]["fail_count"], 8);
    assert_eq!(envelope["summary"]["pass_count"], 0);
}
