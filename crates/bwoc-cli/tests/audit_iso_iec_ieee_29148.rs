//! End-to-end smoke for the `audit-iso-iec-ieee-29148` runtime.
//!
//! Verifies the round-trip `bwoc audit run --plugin audit-iso-iec-ieee-29148
//! --json` against a tempdir workspace that declares an attestation block —
//! the same shared attestation runtime as the ISO 9001 / ISO/IEC 27001 lanes,
//! exercised here for the ISO/IEC/IEEE 29148 Requirements-Engineering lane.
//!
//! Skipped on Windows for the same reason as `audit_iso_9001.rs`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc"))
}

/// Copy the plugin dir verbatim from the worktree into the tempdir. Preserves
/// `audit.sh` executable bit (the dispatcher only runs entries with `+x`).
fn install_plugin(workspace: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../modules/plugins/audit-iso-iec-ieee-29148")
        .canonicalize()
        .expect("canonicalize audit-iso-iec-ieee-29148 plugin source");
    let dst = workspace.join("modules/plugins/audit-iso-iec-ieee-29148");
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

/// workspace.toml enabling the plugin + declaring two attestations (one with
/// `valid_through`, one without). The remaining five of the seven criteria have
/// no attestation — they emit `status=fail` with the workspace.toml remedy.
const WORKSPACE_TOML: &str = r#"[workspace]
name = "29148-smoke"
version = "0.1.0"

[plugins.audit-iso-iec-ieee-29148]
enabled = true

[[plugins.audit-iso-iec-ieee-29148.attestations]]
criterion_id  = "29148-traceability"
statement     = "A requirements traceability matrix links every StRS need to a SyRS/SRS requirement to a test case; reviewed each release."
signer        = "RE Lead: Anong P."
signed_at     = "2026-07-24"
valid_through = "2027-07-24"

[[plugins.audit-iso-iec-ieee-29148.attestations]]
criterion_id = "29148-requirement-characteristics"
statement    = "Requirements review 2026-07-20 confirmed each requirement is necessary, unambiguous, singular, feasible, and verifiable."
signer       = "RE Lead: Anong P."
signed_at    = "2026-07-20"
"#;

#[test]
fn audit_29148_emits_attestation_and_fail_findings() {
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
            "audit-iso-iec-ieee-29148",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc audit run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // criteria.toml declares 7 criteria; 2 have attestations (pass), 5 do not
    // (fail). Exit code = fail count = 5.
    assert!(
        !stderr.contains("framework error"),
        "framework error in stderr — dispatcher rejected attestation shape:\n{stderr}\n\nstdout:\n{stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(5),
        "expected exit code 5 (one per missing-attestation fail); stdout=\n{stdout}\nstderr=\n{stderr}"
    );

    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("parse --json envelope");

    let summary = &envelope["summary"];
    assert_eq!(summary["pass_count"], 2);
    assert_eq!(summary["fail_count"], 5);
    assert_eq!(summary["framework_error"], false);

    let runs = envelope["runs"].as_array().expect("runs is array");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["plugin"], "audit-iso-iec-ieee-29148");

    let findings = runs[0]["findings"].as_array().expect("findings is array");
    assert_eq!(findings.len(), 7, "expected 7 findings (one per criterion)");

    // The traceability attestation carries valid_through.
    let tr = findings
        .iter()
        .find(|f| f["criterion_id"] == "29148-traceability")
        .expect("traceability finding present");
    assert_eq!(tr["status"], "pass");
    assert_eq!(tr["evidence"]["kind"], "attestation");
    assert_eq!(tr["evidence"]["signer"], "RE Lead: Anong P.");
    assert_eq!(tr["evidence"]["valid_through"], "2027-07-24");
    assert!(tr.get("remedy").is_none(), "pass must not carry remedy");

    // The characteristics attestation has no valid_through — key must be dropped.
    let rc = findings
        .iter()
        .find(|f| f["criterion_id"] == "29148-requirement-characteristics")
        .expect("requirement-characteristics finding present");
    assert_eq!(rc["status"], "pass");
    assert!(
        rc["evidence"]
            .as_object()
            .unwrap()
            .get("valid_through")
            .is_none(),
        "valid_through was omitted in workspace.toml — must not surface in envelope"
    );

    // A criterion without an attestation must fail and point at workspace.toml.
    let v = findings
        .iter()
        .find(|f| f["criterion_id"] == "29148-verifiability")
        .expect("verifiability finding present");
    assert_eq!(v["status"], "fail");
    assert_eq!(v["evidence"]["kind"], "file");
    assert_eq!(v["evidence"]["value"], ".bwoc/workspace.toml");
    let remedy = v["remedy"].as_str().expect("fail must carry remedy");
    assert!(
        remedy.contains("[[plugins.audit-iso-iec-ieee-29148.attestations]]"),
        "remedy does not name the workspace.toml block: {remedy}"
    );
    assert!(
        remedy.contains("29148-verifiability"),
        "remedy does not name the criterion: {remedy}"
    );
}

#[test]
fn audit_29148_fails_all_when_no_attestations_present() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    std::fs::create_dir_all(ws.join(".bwoc")).expect("mkdir .bwoc");
    std::fs::write(
        ws.join(".bwoc/workspace.toml"),
        "[workspace]\nname = \"empty\"\nversion = \"0.1.0\"\n\n\
         [plugins.audit-iso-iec-ieee-29148]\nenabled = true\n",
    )
    .expect("write workspace.toml");
    install_plugin(ws);

    let output = Command::new(bin())
        .args([
            "audit",
            "run",
            "--plugin",
            "audit-iso-iec-ieee-29148",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc audit run");

    assert_eq!(
        output.status.code(),
        Some(7),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse --json envelope");
    assert_eq!(envelope["summary"]["fail_count"], 7);
    assert_eq!(envelope["summary"]["pass_count"], 0);
    assert_eq!(envelope["summary"]["framework_error"], false);
}
