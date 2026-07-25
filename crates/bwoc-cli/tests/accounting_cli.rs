//! End-to-end smoke for the `bwoc accounting` gated CLI.
//!
//! The real `accounting-api` plugin hits an external HTTP API, so these tests
//! install a **stub** workflow plugin (name `accounting-api`, kind `workflow`)
//! that echoes a canned JSON envelope — this exercises the CLI's discovery →
//! gate → invoke → render wiring without any network. The gate-refusal cases
//! don't even reach the stub: they assert the choke point fires first.
//!
//! Skipped on Windows (shell stub entry).

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc"))
}

/// Install a stub `workflow/accounting-api` plugin whose entry echoes a canned
/// envelope keyed by the operation it's handed on stdin. Proves the CLI spawns
/// the plugin and renders its output — no live API involved.
fn install_stub_plugin(workspace: &Path) {
    let dir = workspace.join("modules/plugins/accounting-api");
    std::fs::create_dir_all(&dir).expect("mkdir stub plugin dir");

    std::fs::write(
        dir.join("manifest.toml"),
        "[plugin]\nname = \"accounting-api\"\nkind = \"workflow\"\n\
         version = \"0.0.0\"\ndescription = \"stub\"\nentry = \"stub.sh\"\n",
    )
    .expect("write stub manifest");

    // Reads the JSON request on stdin; branches on .operation via a grep so we
    // don't need jq in CI. Emits the same field shape the real plugin would.
    let stub = r#"#!/usr/bin/env bash
set -euo pipefail
req="$(cat)"
op="${BWOC_ACCOUNTING_OPERATION:-}"
case "$op" in
  report)         printf '%s\n' '{"ok":true,"plugin":"accounting-api","operation":"report","report":"pnl","data":{"net":42}}' ;;
  bill-create)    printf '%s\n' '{"ok":true,"plugin":"accounting-api","operation":"bill-create","document_id":"PI-999","number":"PI-999","type":"bill"}' ;;
  bill-update)    printf '%s\n' '{"ok":true,"plugin":"accounting-api","operation":"bill-update","document_id":"PI-999","number":"PI-999","status":"finalized"}' ;;
  expense-create) printf '%s\n' '{"ok":true,"plugin":"accounting-api","operation":"expense-create","expense_id":"EX-1","number":"EX-1"}' ;;
  sales-open-invoices|quick-sales-list|stock-low|stock-movements|gl-journals-list) \
                  printf '%s\n' "{\"ok\":true,\"plugin\":\"accounting-api\",\"operation\":\"$op\",\"data\":[]}" ;;
  quick-sales-show|stock-balance|gl-journal-show) \
                  printf '%s\n' "{\"ok\":true,\"plugin\":\"accounting-api\",\"operation\":\"$op\",\"data\":{}}" ;;
  quick-sales-create|quick-sales-convert|stock-receipt|stock-adjust|gl-journal-create) \
                  printf '%s\n' "{\"ok\":true,\"plugin\":\"accounting-api\",\"operation\":\"$op\",\"id\":\"NEW-1\",\"number\":\"NEW-1\"}" ;;
  *)              printf '%s\n' "stub: unknown op '$op' (req=$req)" >&2; exit 2 ;;
esac
"#;
    let entry = dir.join("stub.sh");
    std::fs::write(&entry, stub).expect("write stub entry");
    let mut perms = std::fs::metadata(&entry).expect("stat stub").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&entry, perms).expect("chmod stub");
}

fn write_workspace(ws: &Path, body: &str) {
    std::fs::create_dir_all(ws.join(".bwoc")).expect("mkdir .bwoc");
    std::fs::write(ws.join(".bwoc/workspace.toml"), body).expect("write workspace.toml");
}

const ENABLED_READONLY: &str = "[workspace]\nname = \"acct\"\nversion = \"0.1.0\"\n\n\
     [plugins.accounting-api]\nenabled = true\n";

const ENABLED_WRITES: &str = "[workspace]\nname = \"acct\"\nversion = \"0.1.0\"\n\n\
     [plugins.accounting-api]\nenabled = true\nwrites_enabled = true\n";

// --- Gate refusals (never reach the stub) ----------------------------------

#[test]
fn write_refused_without_standing_opt_in() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_READONLY); // enabled, but writes_enabled absent
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args(["accounting", "bill", "create", "--type", "bill", "--json"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(
        out.status.code(),
        Some(2),
        "financial write must refuse without writes_enabled"
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"], "writes_disabled");
}

#[test]
fn write_json_mode_requires_yes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_WRITES);
    install_stub_plugin(ws);

    // opt-in present, but --json without --yes must block (no silent write).
    let out = Command::new(bin())
        .args([
            "accounting",
            "expense",
            "create",
            "--payload",
            "{\"amount\":10}",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(out.status.code(), Some(2), "--json write needs --yes");
    // This path prints the reason to stderr (no envelope), matching gcloud.
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--json requires --yes"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn report_rejects_unknown_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_READONLY);
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args(["accounting", "report", "not-a-report", "--json"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(out.status.code(), Some(2));
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["error"], "bad_report");
}

#[test]
fn bad_payload_is_rejected_locally() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_WRITES);
    install_stub_plugin(ws);

    // A JSON array is not an object → local reject with --yes present, before spawn.
    let out = Command::new(bin())
        .args([
            "accounting",
            "expense",
            "create",
            "--payload",
            "[1,2,3]",
            "--yes",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(out.status.code(), Some(2));
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["error"], "bad_payload");
}

// --- Happy paths (reach the stub) ------------------------------------------

#[test]
fn report_read_needs_no_write_opt_in() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_READONLY); // reads are free — no writes_enabled
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args(["accounting", "report", "pnl", "--json"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["report"], "pnl");
}

#[test]
fn write_proceeds_with_opt_in_and_yes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_WRITES);
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args([
            "accounting",
            "bill",
            "create",
            "--type",
            "bill",
            "--yes",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["document_id"], "PI-999");
}

// --- Sales / stock / cashbook domains --------------------------------------

#[test]
fn stock_low_read_is_free() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_READONLY); // reads need no writes_enabled
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args(["accounting", "stock", "low", "--json"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["operation"], "stock-low");
}

#[test]
fn stock_receipt_refused_without_opt_in() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_READONLY); // enabled, but writes_enabled absent
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args([
            "accounting",
            "stock",
            "receipt",
            "--payload",
            "{\"qty\":10}",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(
        out.status.code(),
        Some(2),
        "a stock write is financial — must refuse without writes_enabled"
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["error"], "writes_disabled");
}

#[test]
fn stock_receipt_proceeds_with_opt_in_and_yes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_WRITES);
    install_stub_plugin(ws);

    let out = Command::new(bin())
        .args([
            "accounting",
            "stock",
            "receipt",
            "--payload",
            "{\"qty\":10}",
            "--yes",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");

    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["id"], "NEW-1");
}

#[test]
fn quick_convert_is_gated_and_validates_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = tmp.path();
    write_workspace(ws, ENABLED_WRITES);
    install_stub_plugin(ws);

    // A bad id is rejected locally, before any spawn/gate.
    let bad = Command::new(bin())
        .args([
            "accounting",
            "sales",
            "quick",
            "convert",
            "-bad",
            "--yes",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");
    // clap treats a leading-hyphen positional as a flag; either way it's a usage error (2).
    assert_eq!(bad.status.code(), Some(2));

    // A good id + opt-in + --yes proceeds through the gate to the stub.
    let ok = Command::new(bin())
        .args([
            "accounting",
            "sales",
            "quick",
            "convert",
            "QS-1",
            "--yes",
            "--json",
        ])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn");
    assert_eq!(
        ok.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let env: serde_json::Value = serde_json::from_slice(&ok.stdout).expect("json envelope");
    assert_eq!(env["operation"], "quick-sales-convert");
}
