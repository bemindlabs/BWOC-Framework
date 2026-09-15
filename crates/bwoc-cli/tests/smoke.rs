//! End-to-end integration smoke test — the most-traveled CLI path.
//!
//! Closes gap-analysis item 4 (`notes/2026-05-22_gap-analysis-remediation.md`).
//! Inline unit tests (62 of them across `src/`) verify types and pure
//! functions; this test verifies *behavior*: the actual binary process
//! produces the right side-effects on disk for the golden-path flow.
//!
//! Scope: ONE test function. The plan deliberately constrains this to the
//! single most user-traveled chain — `init → new → list`. Adding daemon /
//! spawn / stop tests requires test doubles for the backend CLI
//! subprocess, which is out of scope for this pass. Yoniso Manasikāra —
//! verify behavior, not just types; one test proving the most-used path
//! is the right amount (Mattaññutā).
//!
//! Skipped on Windows because the template embedded via `include_dir!`
//! contains `*.md` symlinks (`CLAUDE.md` → `AGENTS.md`) that the Windows
//! filesystem can't reproduce without admin perms. The `bwoc new`
//! Windows path is already tested by the matrix CI's unit-test pass.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

/// Path to the binary under test, set by Cargo at compile time.
fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc"))
}

#[test]
fn end_to_end_init_new_list() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    // 1. `bwoc init <ws>` — creates .bwoc/workspace.toml + agents.toml + agents/.
    let status = Command::new(bin())
        .args(["init"])
        .arg(ws)
        .status()
        .expect("spawn bwoc init");
    assert!(status.success(), "bwoc init failed: {status}");
    assert!(
        ws.join(".bwoc/workspace.toml").is_file(),
        "workspace.toml missing"
    );
    assert!(
        ws.join(".bwoc/agents.toml").is_file(),
        "agents.toml missing"
    );
    assert!(ws.join("agents").is_dir(), "agents/ dir missing");

    // 2. `bwoc new alpha ...` — all required fields supplied (non-TTY).
    //    Reads the embedded template — no --template flag needed.
    let status = Command::new(bin())
        .arg("new")
        .arg("alpha")
        .args(["--target"])
        .arg(ws.join("agents/agent-alpha"))
        .args([
            "--backend",
            "claude",
            "--role",
            "smoke test",
            "--primary-model",
            "claude-opus-4-7",
            "--lint-cmd",
            "true",
            "--format-cmd",
            "true",
            "--test-cmd",
            "true",
            "--build-cmd",
            "true",
        ])
        // Force non-TTY by closing stdin so the prompt path is never reached.
        .stdin(std::process::Stdio::null())
        .status()
        .expect("spawn bwoc new");
    assert!(status.success(), "bwoc new failed: {status}");
    assert!(
        ws.join("agents/agent-alpha/AGENTS.md").is_file(),
        "AGENTS.md missing in agent dir"
    );
    assert!(
        ws.join("agents/agent-alpha/config.manifest.json").is_file(),
        "config.manifest.json missing"
    );

    // 3. agents.toml should now contain alpha.
    let registry = std::fs::read_to_string(ws.join(".bwoc/agents.toml")).expect("read agents.toml");
    assert!(
        registry.contains("agent-alpha"),
        "alpha not in agents.toml:\n{registry}"
    );
    assert!(
        registry.contains("backend = \"claude\""),
        "backend not recorded:\n{registry}"
    );

    // 4. `bwoc list` — should run cleanly + mention alpha.
    let output = Command::new(bin())
        .args(["list"])
        .args(["--workspace"])
        .arg(ws)
        .output()
        .expect("spawn bwoc list");
    assert!(
        output.status.success(),
        "bwoc list failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("agent-alpha"),
        "list output missing agent-alpha:\n{stdout}"
    );
}

/// A deprecated alias must be the canonical command plus one stderr line:
/// identical stdout and exit code, and `BWOC_NO_DEPRECATION_WARNINGS=1` silences it.
#[test]
fn deprecated_alias_warns_on_stderr_only() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();
    let ok = Command::new(bin()).args(["init"]).arg(ws).output();
    assert!(ok.is_ok_and(|o| o.status.success()), "init failed");

    let run = |args: &[&str], silence: bool| {
        let mut cmd = Command::new(bin());
        cmd.args(args)
            .arg("--workspace")
            .arg(ws)
            .env_remove("BWOC_NO_DEPRECATION_WARNINGS")
            .env("BWOC_NO_WHATSNEW", "1")
            .env("BWOC_NO_UPDATE_CHECK", "1");
        if silence {
            cmd.env("BWOC_NO_DEPRECATION_WARNINGS", "1");
        }
        cmd.output().expect("spawn bwoc")
    };

    let canonical = run(&["doc", "list", "notes"], false);
    let old = run(&["notes", "list"], false);
    assert_eq!(old.status.code(), canonical.status.code());
    assert_eq!(
        old.stdout, canonical.stdout,
        "stdout must be byte-identical"
    );
    let stderr = String::from_utf8_lossy(&old.stderr);
    assert!(
        stderr.contains("bwoc notes list: deprecated — use `bwoc doc list notes` (removal in 4.0)"),
        "missing deprecation line on stderr: {stderr}"
    );
    assert!(!String::from_utf8_lossy(&old.stdout).contains("deprecated"));
    assert!(!String::from_utf8_lossy(&canonical.stderr).contains("deprecated"));

    let silenced = run(&["notes", "list"], true);
    assert_eq!(silenced.stdout, canonical.stdout);
    assert!(!String::from_utf8_lossy(&silenced.stderr).contains("deprecated"));
}

#[test]
fn end_to_end_set_updates_backend_and_model() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let ws = tmp.path();

    // init + new beta (backend=claude, primaryModel=claude-opus-4-7).
    let ok = Command::new(bin()).args(["init"]).arg(ws).status();
    assert!(ok.is_ok_and(|s| s.success()), "init failed");
    let status = Command::new(bin())
        .arg("new")
        .arg("beta")
        .args(["--target"])
        .arg(ws.join("agents/agent-beta"))
        .args([
            "--backend",
            "claude",
            "--role",
            "set test",
            "--primary-model",
            "claude-opus-4-7",
            "--lint-cmd",
            "true",
            "--format-cmd",
            "true",
            "--test-cmd",
            "true",
            "--build-cmd",
            "true",
        ])
        .stdin(std::process::Stdio::null())
        .status()
        .expect("spawn bwoc new");
    assert!(status.success(), "bwoc new failed: {status}");

    // `bwoc set beta --backend ollama --primary-model qwen2.5 --fallback-model llama3.2`.
    let status = Command::new(bin())
        .args(["set", "beta"])
        .args(["--workspace"])
        .arg(ws)
        .args([
            "--backend",
            "ollama",
            "--primary-model",
            "qwen2.5",
            "--fallback-model",
            "llama3.2",
        ])
        .status()
        .expect("spawn bwoc set");
    assert!(status.success(), "bwoc set failed: {status}");

    // registry backend flipped to ollama.
    let registry = std::fs::read_to_string(ws.join(".bwoc/agents.toml")).expect("read agents.toml");
    assert!(
        registry.contains("backend = \"ollama\""),
        "backend not updated in registry:\n{registry}"
    );

    // manifest primaryModel + fallbackModel updated.
    let manifest = std::fs::read_to_string(ws.join("agents/agent-beta/config.manifest.json"))
        .expect("read manifest");
    assert!(
        manifest.contains("\"qwen2.5\""),
        "primaryModel not updated:\n{manifest}"
    );
    assert!(
        manifest.contains("\"llama3.2\""),
        "fallbackModel not updated:\n{manifest}"
    );
}
