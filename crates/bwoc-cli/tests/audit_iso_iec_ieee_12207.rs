//! End-to-end smoke for the `audit-iso-iec-ieee-12207` runtime.
//!
//! Verifies `bwoc audit run --plugin audit-iso-iec-ieee-12207 --json` against a
//! tempdir workspace declaring attestations — the same shared attestation
//! runtime as the ISO 9001 / 27001 / 29148 lanes, exercised for the ISO/IEC/IEEE
//! 12207 software-life-cycle lane.
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
        .join("../../modules/plugins/audit-iso-iec-ieee-12207")
        .canonicalize()
        .expect("canonicalize audit-iso-iec-ieee-12207 plugin source");
    let dst = workspace.join("modules/plugins/audit-iso-iec-ieee-12207");
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
name = "12207-smoke"
version = "0.1.0"

[plugins.audit-iso-iec-ieee-12207]
enabled = true

[[plugins.audit-iso-iec-ieee-12207.attestations]]
criterion_id  = "12207-configuration-management"
statement     = "All source, docs, and build artefacts are in git with tagged baselines; changes land only via reviewed, CI-gated PRs."
signer        = "Eng Lead: Somchai T."
signed_at     = "2026-07-25"
valid_through = "2027-07-25"

[[plugins.audit-iso-iec-ieee-12207.attestations]]
criterion_id = "12207-project-planning"
statement    = "Each increment has a plan (scope, schedule, owners) agreed before build."
signed_at    = "2026-07-20"
signer       = "PM: Ratana S."
"#;

#[test]
fn audit_12207_emits_attestation_and_fail_findings() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    std::fs::create_dir_all(ws.join(".bwoc")).expect("mkdir .bwoc");
    std::fs::write(ws.join(".bwoc/workspace.toml"), WORKSPACE_TOML).expect("write workspace.toml");
    install_plugin(ws);

    let output = Command::new(bin())
        .args([
            "audit",
            "run",
            "--plugin",
            "audit-iso-iec-ieee-12207",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc audit run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // criteria.toml declares 9 criteria; 2 have attestations (pass), 7 do not
    // (fail). Exit code = fail count = 7.
    assert!(
        !stderr.contains("framework error"),
        "framework error in stderr:\n{stderr}\n\nstdout:\n{stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(7),
        "expected exit 7 (one per missing-attestation fail); stdout=\n{stdout}\nstderr=\n{stderr}"
    );

    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("parse --json envelope");
    let summary = &envelope["summary"];
    assert_eq!(summary["pass_count"], 2);
    assert_eq!(summary["fail_count"], 7);
    assert_eq!(summary["framework_error"], false);

    let runs = envelope["runs"].as_array().expect("runs is array");
    assert_eq!(runs[0]["plugin"], "audit-iso-iec-ieee-12207");
    let findings = runs[0]["findings"].as_array().expect("findings is array");
    assert_eq!(findings.len(), 9, "expected 9 findings (one per criterion)");

    let cm = findings
        .iter()
        .find(|f| f["criterion_id"] == "12207-configuration-management")
        .expect("cm finding present");
    assert_eq!(cm["status"], "pass");
    assert_eq!(cm["evidence"]["kind"], "attestation");
    assert_eq!(cm["evidence"]["valid_through"], "2027-07-25");

    let vv = findings
        .iter()
        .find(|f| f["criterion_id"] == "12207-verification-validation")
        .expect("v&v finding present");
    assert_eq!(vv["status"], "fail");
    assert_eq!(vv["evidence"]["value"], ".bwoc/workspace.toml");
    let remedy = vv["remedy"].as_str().expect("fail must carry remedy");
    assert!(
        remedy.contains("12207-verification-validation"),
        "remedy names the criterion: {remedy}"
    );
}

#[test]
fn audit_12207_fails_all_when_no_attestations_present() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    std::fs::create_dir_all(ws.join(".bwoc")).expect("mkdir .bwoc");
    std::fs::write(
        ws.join(".bwoc/workspace.toml"),
        "[workspace]\nname = \"empty\"\nversion = \"0.1.0\"\n\n\
         [plugins.audit-iso-iec-ieee-12207]\nenabled = true\n",
    )
    .expect("write workspace.toml");
    install_plugin(ws);

    let output = Command::new(bin())
        .args([
            "audit",
            "run",
            "--plugin",
            "audit-iso-iec-ieee-12207",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc audit run");

    assert_eq!(
        output.status.code(),
        Some(9),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse --json envelope");
    assert_eq!(envelope["summary"]["fail_count"], 9);
    assert_eq!(envelope["summary"]["pass_count"], 0);
}
