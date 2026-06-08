//! Phase 5 t9 — cgroup v2 `pids.max` enforcement tests.
//!
//! These drive the **real** `bwoc-harness` binary in its hidden
//! `--__turn-executor` mode and verify the per-turn process cap installed in the
//! production `pre_exec` path: the parent creates a per-turn cgroup leaf with
//! `pids.max`, and the child moves itself in (`cgroup.procs` write) before its
//! fork bomb runs.
//!
//! # Honesty (condition 5 — musāvāda gate)
//!
//! The enforcement arm requires a **delegated, writable cgroup v2 subtree**. Where
//! none exists (dev box, bare-SSH login, non-delegated container — the default),
//! the arm **LOUD-skips with a `NOT COVERAGE` marker** rather than silently
//! passing. A skipped run is NOT proof of enforcement. Real coverage comes from a
//! CI job that provides delegation (a privileged container with writable cgroupfs,
//! or self-delegation on a runner) and sets `BWOC_T9_EXPECT_ENFORCE=1`, which
//! turns the skip itself into a FAILURE — so CI cannot green without the cap
//! actually firing.
//!
//! Run the enforcement bomb where delegation exists (serially, env-mutating):
//!   `BWOC_T9_RUN_BOMBS=1 cargo test -p bwoc-harness --test cgroup_pids \
//!        -- --test-threads=1 --nocapture`

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bwoc_harness::turn_executor::{ToolInvocation, run_isolated};

/// Serializes env-mutating tests in THIS binary (the bomb sets process-global
/// `BWOC_TURN_*` around a spawn). Poison ignored — a panicking test must not wedge
/// the rest.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}

fn harness_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc-harness"))
}

fn inv(tool_name: &str, args_json: &str, workdir: &Path, confine: bool) -> ToolInvocation {
    ToolInvocation {
        tool_name: tool_name.to_string(),
        args_json: args_json.to_string(),
        workdir: workdir.to_path_buf(),
        confine,
    }
}

fn expect_enforce() -> bool {
    std::env::var("BWOC_T9_EXPECT_ENFORCE").as_deref() == Ok("1")
}

fn bombs_enabled() -> bool {
    std::env::var("BWOC_T9_RUN_BOMBS").as_deref() == Ok("1") || expect_enforce()
}

// ── Always-on: the availability probe is callable + honest off-Linux ─────────
#[test]
fn availability_probe_is_honest() {
    let avail = bwoc_harness::cgroup::delegated_subtree_available();
    if !cfg!(target_os = "linux") {
        assert!(
            !avail,
            "cgroup pids.max is a Linux control; the probe must report unavailable off Linux"
        );
    }
    eprintln!("[t9] delegated_subtree_available() = {avail}");
}

// ── Enforcement: pids.max caps a fork bomb the per-UID NPROC floor would NOT ──
//
// The proof isolates the cgroup from t6's RLIMIT_NPROC: NPROC headroom is set
// HUGE (4096), so the per-UID fork guard would let all 60 subshells through. With
// `pids.max` low (20), a containment far below 60 can ONLY be the cgroup — not
// NPROC. So `created << 60` is unambiguous t9 evidence.
#[test]
fn pids_max_contains_fork_bomb_when_delegated() {
    if !bombs_enabled() {
        eprintln!(
            "[t9] enforcement arm skipped (set BWOC_T9_RUN_BOMBS=1, or BWOC_T9_EXPECT_ENFORCE=1 in \
             a delegated CI job, to run). NOT COVERAGE."
        );
        return;
    }
    if !cfg!(target_os = "linux") {
        let msg = "[t9] ENFORCEMENT ARM SKIPPED — cgroup pids.max is Linux-only. NOT COVERAGE.";
        assert!(
            !expect_enforce(),
            "{msg} but BWOC_T9_EXPECT_ENFORCE=1 demanded real enforcement on this platform"
        );
        eprintln!("{msg}");
        return;
    }

    let _g = guard();

    // Resolve the delegated base in THIS (parent) process — the same OnceLock the
    // production `roundtrip` path uses to create per-turn leaves. If no delegated
    // subtree exists, LOUD-skip (and FAIL when CI demanded enforcement).
    if !bwoc_harness::cgroup::delegated_subtree_available() {
        let msg = "[t9] ENFORCEMENT ARM SKIPPED — no delegated, writable cgroup v2 subtree with the \
                   `pids` controller (run under systemd Delegate=yes or a privileged container). \
                   NOT COVERAGE.";
        assert!(
            !expect_enforce(),
            "{msg} but BWOC_T9_EXPECT_ENFORCE=1 demanded real enforcement"
        );
        eprintln!("{msg}");
        return;
    }

    let work = tempfile::tempdir().unwrap();
    let markers = work.path().join("m");
    std::fs::create_dir_all(&markers).unwrap();
    let md = markers.to_str().unwrap();

    // HUGE NPROC headroom so the per-UID floor would pass all 60 forks; LOW
    // pids.max (20) so only the cgroup can contain them. 20 is deliberately above
    // the executor child's own baseline (its runtime threads + the run_command
    // shell) so the child can spawn the bomb, yet far below 60 so containment is
    // unambiguous. BOUNDED: 60 bg subshells, each touches a marker then sleeps 3s.
    // SAFETY: env mutation serialized by ENV_GUARD; removed before unlock.
    unsafe {
        std::env::set_var("BWOC_TURN_RLIMIT_NPROC_HEADROOM", "4096");
        std::env::set_var("BWOC_TURN_PIDS_MAX", "20");
    }
    // Count via a glob + `$#` (shell builtins, NO extra fork) so the tally itself
    // is not starved by the cap it is measuring.
    let script = format!(
        "for i in $(seq 1 60); do ( : > {md}/p.$i; sleep 3 ) & done; set -- {md}/p.*; echo $#"
    );
    let args = serde_json::json!({ "command": script }).to_string();
    // The cap counts ALL tasks in the leaf (threads included), so a tight cap can
    // starve the executor child's OWN runtime mid-turn and drop the IPC response.
    // That is fine: the ENFORCEMENT EVIDENCE is the on-disk marker count (forks
    // that the cgroup let through), not the child's reply — a fork bomb may well
    // kill the very process trying to report on it. So do NOT unwrap.
    let res = run_isolated(
        &harness_bin(),
        &inv("run_command", &args, work.path(), true),
    );
    unsafe {
        std::env::remove_var("BWOC_TURN_RLIMIT_NPROC_HEADROOM");
        std::env::remove_var("BWOC_TURN_PIDS_MAX");
    }

    let created = std::fs::read_dir(&markers).map(|d| d.count()).unwrap_or(0);
    eprintln!(
        "[t9] pids.max=20 bomb: {created}/60 subshells forked (NPROC headroom=4096 would allow all \
         60); child reply={:?}",
        res.as_ref().map(|o| o.content.trim().to_string())
    );
    // Some forks must have run (the command executed), but FAR below the 60 that
    // NPROC (headroom 4096) would have allowed — so the cgroup `pids.max` is what
    // bound it. pids.max=20 can never yield 40 markers, so `< 40` is unambiguous.
    assert!(
        created >= 1,
        "the run_command shell never forked a single subshell — the turn did not run, so this is \
         not a valid pids.max enforcement measurement"
    );
    assert!(
        created < 40,
        "cgroup pids.max did not contain the fork bomb: {created}/60 forked despite pids.max=20 \
         (NPROC headroom was 4096, so only the cgroup could bound this below 60)"
    );
}
