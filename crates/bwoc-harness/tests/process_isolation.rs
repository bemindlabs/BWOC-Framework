//! Phase 5 t5 — process-isolation integration tests.
//!
//! These drive the **real** `bwoc-harness` binary in its hidden
//! `--__turn-executor` mode (via `CARGO_BIN_EXE_bwoc-harness`) and verify every
//! binding condition that needs a live child process:
//!
//! - C9  — child env carries no API key / parent secret (scrub_env on spawn).
//! - C10 — child holds exactly stdio + the IPC fd (no leaked descriptors).
//! - C11 — the child is reaped; its PID is truly gone (no zombie).
//! - C12 — child cwd is the per-turn tempdir, yet confine_path still rejects an
//!   absolute path outside the worktree.
//! - C13 — an executor invocation without the capability token is rejected
//!   and the tool never runs.
//! - C2  — a framed request whose token mismatches the env token is rejected.
//! - C2-token / grandchild — run_command's child-of-child has the token scrubbed.
//! - C5  — un-marshallable (MCP/dynamic) tools are denied fail-closed.
//! - C7/C8 — writes (write_file, memory_write) land on disk through the child.
//! - baseline — distinct PIDs, separate tempdirs, fresh static memory, EBADF.
//!
//! C1/C3 (raw-libc pre_exec + the t6 `setrlimit` calls) and C4 (the resource-
//! containment notice) are structural and reviewed in `src/turn_executor.rs`; the
//! live per-limit assertions on the production pre_exec path live in the
//! `resource_limits` integration test.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use bwoc_harness::sandbox::ENV_SENSITIVE_PATTERNS;
use bwoc_harness::turn_executor::{
    ForgeMode, SelfTestReport, ToolInvocation, run_isolated, run_isolated_forged,
    run_isolated_selftest,
};
use std::sync::{Mutex, MutexGuard};

/// Serialize the whole suite. Each test spawns the real harness child and
/// exercises *process-wide* sandbox machinery — the one-time capability token,
/// per-turn `setrlimit`/cgroup, the IPC fd, env scrubbing, PID reaping. Run in
/// parallel (cargo's default) those contend and the suite flakes on CI: a
/// different subset fails each run. A single file-level lock makes them run one
/// at a time. Poison-tolerant so a panicking test can't cascade-fail the rest.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn harness_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc-harness"))
}

fn parse_report(content: &str) -> SelfTestReport {
    serde_json::from_str(content).expect("selftest report should be valid JSON")
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Normalize a path string for comparison when the dir may already be removed
/// (so `canonicalize` can't run) and macOS reports the `/private` real-path.
/// Strips a leading `/private` so `/private/var/…` and `/var/…` compare equal.
fn norm_cwd(p: &str) -> String {
    p.strip_prefix("/private").unwrap_or(p).to_string()
}

fn inv(tool_name: &str, args_json: &str, workdir: &Path, confine: bool) -> ToolInvocation {
    ToolInvocation {
        tool_name: tool_name.to_string(),
        args_json: args_json.to_string(),
        workdir: workdir.to_path_buf(),
        confine,
    }
}

// ── C9 — no secrets in the child env ───────────────────────────────────────
#[test]
fn c9_child_env_has_no_api_keys_or_secrets() {
    let _serial = serial();
    // Plant a provider key + a generic secret in THIS (parent) process. scrub_env
    // reads the parent env at spawn, so a leak would surface in the child report.
    // SAFETY: set before the spawn in a test that is the sole env mutator.
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "leak-canary-anthropic");
        std::env::set_var("BWOC_T5_FAKE_SECRET", "leak-canary-secret");
    }

    let work = tempfile::tempdir().unwrap();
    let out = run_isolated_selftest(&harness_bin(), work.path()).unwrap();
    let report = parse_report(&out.content);

    // The planted secrets must NOT reach the child.
    assert!(
        !report.env_keys.iter().any(|k| k == "ANTHROPIC_API_KEY"),
        "C9: ANTHROPIC_API_KEY leaked into child env"
    );
    assert!(
        !report.env_keys.iter().any(|k| k == "BWOC_T5_FAKE_SECRET"),
        "C9: parent secret leaked into child env"
    );

    // Stronger guarantee: no child env key matches a credential pattern — the
    // sole exception being the intentional capability token (which by design
    // contains "TOKEN" and is itself scrubbed before any grandchild). The OS may
    // inject its own non-secret vars (e.g. __CF_USER_TEXT_ENCODING on macOS), so
    // we assert the security property (no secrets) rather than a strict subset.
    for k in &report.env_keys {
        if k == "BWOC_TURN_EXECUTOR_TOKEN" {
            continue;
        }
        let upper = k.to_uppercase();
        let sensitive = ENV_SENSITIVE_PATTERNS.iter().any(|p| upper.contains(*p));
        assert!(
            !sensitive,
            "C9: credential-like key leaked into child env: {k}"
        );
    }

    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("BWOC_T5_FAKE_SECRET");
    }
}

// ── C10 — exactly stdio + IPC fd ────────────────────────────────────────────
#[test]
fn c10_child_fds_are_stdio_plus_ipc_only() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let out = run_isolated_selftest(&harness_bin(), work.path()).unwrap();
    let report = parse_report(&out.content);

    let mut fds = report.fds.clone();
    fds.sort_unstable();
    assert_eq!(
        fds,
        vec![0, 1, 2, report.ipc_fd],
        "C10: child must hold exactly stdio + the IPC fd, got {:?}",
        report.fds
    );
    // Baseline EBADF: a high fd is genuinely closed (not merely one probed number).
    assert!(
        !report.fds.contains(&100),
        "baseline: high fd 100 must be closed in the child"
    );
}

// ── C11 — child reaped, PID gone ────────────────────────────────────────────
#[test]
fn c11_child_is_reaped_no_zombie() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let out = run_isolated_selftest(&harness_bin(), work.path()).unwrap();

    // roundtrip() already wait()'d the child. kill(pid, 0) must now fail with
    // ESRCH — a zombie would still answer signal 0 (errno EPERM/0), so ESRCH
    // proves the PID is fully gone, not merely exited-unreaped.
    let rc = unsafe { libc::kill(out.child_pid as libc::pid_t, 0) };
    let err = std::io::Error::last_os_error();
    assert_eq!(
        rc, -1,
        "C11: pid {} still exists (not reaped)",
        out.child_pid
    );
    assert_eq!(
        err.raw_os_error(),
        Some(libc::ESRCH),
        "C11: expected ESRCH for a reaped pid, got {err}"
    );
}

// ── C12 — cwd is the per-turn tempdir; confine still rejects escapes ─────────
#[test]
fn c12_cwd_is_tempdir_and_confine_holds() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();

    // Part A: the child's process cwd is the per-turn tempdir handed by the
    // parent — NOT the worktree.
    let out = run_isolated_selftest(&harness_bin(), work.path()).unwrap();
    let report = parse_report(&out.content);
    assert_eq!(
        norm_cwd(&report.cwd),
        norm_cwd(&out.cwd.to_string_lossy()),
        "C12: child cwd must equal the per-turn tempdir"
    );
    assert_ne!(
        norm_cwd(&report.cwd),
        norm_cwd(&canon(work.path()).to_string_lossy()),
        "C12: child cwd must NOT be the worktree"
    );

    // Part B: confine_path is still enforced inside the child. An absolute write
    // to a path OUTSIDE the worktree is rejected and the file is not created.
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("escape.txt");
    let args = serde_json::json!({ "path": target.to_str().unwrap(), "content": "x" }).to_string();
    let out = run_isolated(&harness_bin(), &inv("write_file", &args, work.path(), true)).unwrap();
    assert!(
        out.content
            .contains("outside the allowed working directory"),
        "C12: confined write outside worktree must be rejected, got: {}",
        out.content
    );
    assert!(
        !target.exists(),
        "C12: the out-of-worktree file must NOT exist"
    );
}

// ── C13 — forged executor without the token is rejected ─────────────────────
#[test]
fn c13_forged_executor_no_env_token_rejected() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let target = work.path().join("should_not_exist.txt");
    let args = serde_json::json!({ "path": target.to_str().unwrap(), "content": "x" }).to_string();

    let res = run_isolated_forged(
        &harness_bin(),
        &inv("write_file", &args, work.path(), true),
        ForgeMode::NoEnvToken,
    );

    assert!(
        res.is_err(),
        "C13: a tokenless executor invocation must fail (child refuses)"
    );
    assert!(
        !target.exists(),
        "C13: the tool must NOT have executed without the capability token"
    );
}

// ── C2 — request token must match the env token ─────────────────────────────
#[test]
fn c2_bad_request_token_rejected() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let target = work.path().join("should_not_exist.txt");
    let args = serde_json::json!({ "path": target.to_str().unwrap(), "content": "x" }).to_string();

    let res = run_isolated_forged(
        &harness_bin(),
        &inv("write_file", &args, work.path(), true),
        ForgeMode::BadRequestToken,
    );

    assert!(
        res.is_err(),
        "C2: a request whose token mismatches the env token must be rejected"
    );
    assert!(
        !target.exists(),
        "C2: the tool must NOT have executed on a token mismatch"
    );
}

// ── C2-token / grandchild — run_command's child-of-child has no token ────────
#[test]
fn c2_token_scrubbed_before_grandchild() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    // The grandchild (sh, spawned by run_command via run_sandboxed) must NOT see
    // BWOC_TURN_EXECUTOR_TOKEN — scrub_env strips it. An unset var expands empty.
    let args = serde_json::json!({
        "command": "echo \"TOK=[${BWOC_TURN_EXECUTOR_TOKEN}]\""
    })
    .to_string();
    let out = run_isolated(
        &harness_bin(),
        &inv("run_command", &args, work.path(), true),
    )
    .unwrap();
    assert!(
        out.content.contains("TOK=[]"),
        "C2-token: grandchild must not inherit the capability token, got: {}",
        out.content
    );
}

// ── C5 — un-marshallable tool denied fail-closed ────────────────────────────
#[test]
fn c5_unmarshallable_tool_denied_fail_closed() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let out = run_isolated(
        &harness_bin(),
        &inv("mcp__server__do_thing", "{}", work.path(), true),
    )
    .unwrap();
    assert!(
        out.content.contains("fail-closed"),
        "C5: an MCP/dynamic tool must be denied fail-closed in the child, got: {}",
        out.content
    );
}

// ── C7/C8 — writes land on disk through the child ───────────────────────────
#[test]
fn write_file_executes_in_child_and_lands_on_disk() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let target = work.path().join("created_by_child.txt");
    let args = serde_json::json!({ "path": "created_by_child.txt", "content": "hello-from-child" })
        .to_string();
    let out = run_isolated(&harness_bin(), &inv("write_file", &args, work.path(), true)).unwrap();
    assert!(
        target.exists(),
        "C7: write_file in the child must create the file on disk: {}",
        out.content
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "hello-from-child"
    );
}

#[test]
fn c8_memory_write_lands_on_disk_for_rehydration() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let args = serde_json::json!({ "name": "note.md", "content": "remembered" }).to_string();
    let out = run_isolated(
        &harness_bin(),
        &inv("memory_write", &args, work.path(), true),
    )
    .unwrap();
    let mem = work.path().join("memories").join("note.md");
    // The parent holds no cache; the value is re-read straight from disk.
    assert!(
        mem.exists(),
        "C8: memory_write in the child must persist to memories/ on disk: {}",
        out.content
    );
    assert_eq!(std::fs::read_to_string(&mem).unwrap(), "remembered");
}

// ── baseline — distinct PIDs + separate tempdirs ────────────────────────────
#[test]
fn baseline_distinct_pids_and_separate_tempdirs() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let a = run_isolated_selftest(&harness_bin(), work.path()).unwrap();
    let b = run_isolated_selftest(&harness_bin(), work.path()).unwrap();
    assert_ne!(
        a.child_pid, b.child_pid,
        "baseline: each turn is a distinct PID"
    );
    assert_ne!(
        canon(&a.cwd),
        canon(&b.cwd),
        "baseline: each turn gets a separate tempdir"
    );
}

// ── baseline — fresh static memory per child ────────────────────────────────
#[test]
fn baseline_fresh_static_memory_each_child() {
    let _serial = serial();
    let work = tempfile::tempdir().unwrap();
    let a = parse_report(
        &run_isolated_selftest(&harness_bin(), work.path())
            .unwrap()
            .content,
    );
    let b = parse_report(
        &run_isolated_selftest(&harness_bin(), work.path())
            .unwrap()
            .content,
    );
    // A fresh process always reads the initial 0 from the process-static probe.
    assert_eq!(
        a.static_probe, 0,
        "baseline: first child must see fresh static memory"
    );
    assert_eq!(
        b.static_probe, 0,
        "baseline: second child must ALSO see fresh static memory"
    );
}
