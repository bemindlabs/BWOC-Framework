//! Phase 5 t7a — adversarial sandbox-escape gate proof.
//!
//! Built ONLY under `--features test-redteam`. This is the load-bearing proof
//! for the t7a claim: *the turn-executor cannot read/mutate the harness via the
//! filesystem, ptrace/proc-mem, or a planted build artifact.*
//!
//! Two tests:
//!
//! 1. [`redteam_executor_cannot_escape_the_fs_jail`] — spawns the hostile child
//!    (`sandbox_redteam` bin) inside the SAME FS jail the real executor uses
//!    (`jail::jail_command` + `JailSpec::executor`), with this test process
//!    standing in for the parent (made non-dumpable per C4). It asserts every
//!    escape the child attempts is blocked: read `~/.ssh` canary, write outside
//!    the worktree, overwrite the checkpoint canary (M2), overwrite its own
//!    binary (M3), ptrace/process_vm_readv the parent (CRIT-1 / C4), read the
//!    parent's `/proc/<pid>/environ` (C4). A planted `build.rs` is just
//!    worktree code under the same jail, so the read/write arms ARE its
//!    confinement proof (CRIT-2).
//!
//! 2. [`c7_parent_git_does_not_run_planted_worktree_code`] — plants a malicious
//!    `core.fsmonitor` (+ hook) in a worktree, then runs the PRODUCTION
//!    `DiffSummary::from_worktree` (the C7-hardened + jailed parent git) and
//!    asserts the planted code did NOT execute.
//!
//! **Honest scoping (mirrors t6's Linux-only memory cap):** the read / ptrace /
//! `/proc` guarantees are the **Linux** Landlock + C4 controls. On macOS the
//! jail is *write confinement only* (`JailStatus::WriteConfineOnly`), so those
//! arms LOUD-skip — never silent-pass. If the jail cannot be enforced at all
//! (`JailStatus::Unavailable`), the whole proof LOUD-skips.

#![cfg(all(unix, feature = "test-redteam"))]

use std::path::PathBuf;
use std::process::Command;

use bwoc_harness::jail::{self, JailSpec, JailStatus};

fn redteam_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sandbox_redteam"))
}

fn arm_blocked(report: &serde_json::Value, arm: &str) {
    let v = report
        .get(arm)
        .unwrap_or_else(|| panic!("t7a: redteam report missing arm `{arm}`: {report}"));
    assert_eq!(
        v.get("blocked").and_then(|b| b.as_bool()),
        Some(true),
        "t7a: escape `{arm}` was NOT blocked: {v}"
    );
}

/// Like [`arm_blocked`] but tolerates an arm that LOUD-marks itself `skip` on a
/// platform/arch where the control does not apply (e.g. the i386/x32 probes off
/// x86_64). A skipped arm is reported, never silently treated as a pass.
#[cfg(target_os = "linux")]
fn arm_blocked_or_skip(report: &serde_json::Value, arm: &str) {
    let v = report
        .get(arm)
        .unwrap_or_else(|| panic!("t11: redteam report missing arm `{arm}`: {report}"));
    if v.get("skip").and_then(|b| b.as_bool()) == Some(true) {
        eprintln!("[t11 redteam] LOUD SKIP arm `{arm}` (control N/A here): {v}");
        return;
    }
    assert_eq!(
        v.get("blocked").and_then(|b| b.as_bool()),
        Some(true),
        "t11: escape `{arm}` was NOT blocked: {v}"
    );
}

/// The inverse of [`arm_blocked_or_skip`]: assert a deliberately-ALLOWED
/// precision control was permitted (or LOUD-skips off-platform). Here
/// `allowed == true` is the desired outcome — it proves the egress filter is
/// PRECISE (e.g. local `socketpair` survives) and not a blanket socket ban that
/// would sever the executor's tokio runtime.
#[cfg(target_os = "linux")]
fn arm_allowed_or_skip(report: &serde_json::Value, arm: &str) {
    let v = report
        .get(arm)
        .unwrap_or_else(|| panic!("t11: redteam report missing arm `{arm}`: {report}"));
    if v.get("skip").and_then(|b| b.as_bool()) == Some(true) {
        eprintln!("[t11 redteam] LOUD SKIP arm `{arm}` (control N/A here): {v}");
        return;
    }
    assert_eq!(
        v.get("allowed").and_then(|b| b.as_bool()),
        Some(true),
        "t11: precision control `{arm}` should be ALLOWED (the filter must not blanket-ban): {v}"
    );
}

#[test]
fn redteam_executor_cannot_escape_the_fs_jail() {
    // Canaries OUTSIDE the worktree — the "host" the executor must not reach.
    let worktree = tempfile::tempdir().unwrap();
    let tempdir = tempfile::tempdir().unwrap(); // per-turn cwd stand-in (rw)
    let outside = tempfile::tempdir().unwrap();

    let ssh_canary = outside.path().join("id_rsa");
    std::fs::write(&ssh_canary, "PRIVATE-KEY-CANARY").unwrap();
    let checkpoint_canary = outside.path().join("checkpoint.json");
    std::fs::write(&checkpoint_canary, "{\"untrusted_seen\":true}").unwrap();
    let outside_write = outside.path().join("escape.txt");

    // C4: make THIS process (the stand-in parent) non-dumpable, exactly as
    // `main.rs` does, so the child's ptrace/process_vm_readv gets EPERM.
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_DUMPABLE only mutates this process's own flag.
        let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
        assert_eq!(rc, 0, "test setup: PR_SET_DUMPABLE(0) must succeed");
        let scope = std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok());
        eprintln!("[t7a redteam] yama ptrace_scope = {scope:?} (C4 fail-closes on 0 in main.rs)");
    }

    let config = serde_json::json!({
        "ssh_canary": ssh_canary.to_str().unwrap(),
        "outside_write": outside_write.to_str().unwrap(),
        "checkpoint_canary": checkpoint_canary.to_str().unwrap(),
        "parent_pid": std::process::id(),
    })
    .to_string();

    // Spawn the hostile child inside the SAME executor FS jail.
    let spec = JailSpec::executor(worktree.path(), tempdir.path(), &redteam_bin());
    let mut cmd = Command::new(redteam_bin());
    cmd.current_dir(tempdir.path())
        .env("REDTEAM_CONFIG", &config);
    let status = jail::jail_command(&mut cmd, &spec);

    if status == JailStatus::Unavailable {
        eprintln!(
            "[t7a redteam] LOUD SKIP: FS jail unavailable on this platform/kernel — \
             cannot prove escape containment (NOT a pass)."
        );
        return;
    }

    let output = cmd.output().expect("spawn redteam child");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "redteam child emitted no JSON report.\nstdout={:?}\nstderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    });
    eprintln!("[t7a redteam] jail={status:?} report={report}");

    // Enforced on every jailed platform (write confinement covers these).
    arm_blocked(&report, "write_outside_worktree");
    arm_blocked(&report, "overwrite_checkpoint"); // M2
    assert!(
        !outside_write.exists(),
        "t7a: write-outside escape created the file on disk"
    );
    assert_eq!(
        std::fs::read_to_string(&checkpoint_canary).unwrap(),
        "{\"untrusted_seen\":true}",
        "t7a: checkpoint canary was mutated"
    );

    // Read / exec / ptrace / proc confinement = the Linux Landlock + C4
    // guarantee. macOS is write-confinement only → LOUD-skip those arms.
    if status == JailStatus::Enforced {
        arm_blocked(&report, "read_ssh_canary"); // CRIT FS read (+ build.rs RCE confinement)
        arm_blocked(&report, "overwrite_self_binary"); // M3
        arm_blocked(&report, "ptrace_parent"); // CRIT-1 / C4
        arm_blocked(&report, "read_parent_environ"); // C4
    } else {
        eprintln!(
            "[t7a redteam] LOUD SKIP (macOS write-confine): read/ptrace/proc arms are \
             Linux-only guarantees; not asserted here."
        );
    }
}

/// C7 — the parent running git on a child-touched worktree must not execute code
/// the worktree planted (hooks / `core.fsmonitor` / `diff.external`). Drives the
/// real production path: `DiffSummary::from_worktree`.
#[test]
fn c7_parent_git_does_not_run_planted_worktree_code() {
    use std::os::unix::fs::PermissionsExt;

    if Command::new("git")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("[t7a C7] LOUD SKIP: git unavailable.");
        return;
    }

    let worktree = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let marker = outside.path().join("HOOK_FIRED");

    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .arg("-C")
            .arg(worktree.path())
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "test setup: git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t.t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(worktree.path().join("f.txt"), "a\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "seed"]);

    // Plant the vector: a core.fsmonitor program that writes a marker OUTSIDE the
    // worktree. `git diff`/`status` refresh the index, which queries fsmonitor.
    let evil = worktree.path().join(".git").join("evil.sh");
    std::fs::write(
        &evil,
        format!("#!/bin/sh\necho pwned > {}\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&evil, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(&["config", "core.fsmonitor", evil.to_str().unwrap()]);

    // A pending change so the production diff/ls-files refresh the index.
    std::fs::write(worktree.path().join("f.txt"), "a\nb\n").unwrap();

    // Positive control (informational, version-dependent): an UNHARDENED git may
    // run the planted vector. We do not assert on it — the hardened assertion
    // below is what gates — but we log whether the vector reproduced.
    let _ = Command::new("git")
        .arg("-C")
        .arg(worktree.path())
        .arg("status")
        .output();
    eprintln!(
        "[t7a C7] positive control: marker present after RAW git status = {}",
        marker.exists()
    );
    let _ = std::fs::remove_file(&marker);

    // PRODUCTION path: DiffSummary::from_worktree runs the C7-hardened + jailed
    // parent git (core.hooksPath=/dev/null, core.fsmonitor=false, … overrides +
    // Landlock/sandbox-exec). The planted code must NOT execute.
    let _diff = bwoc_harness::result::DiffSummary::from_worktree(worktree.path());
    assert!(
        !marker.exists(),
        "C7: the hardened/jailed parent git executed planted core.fsmonitor code"
    );
}

// ===========================================================================
// t11 — egress containment proof (A ∧ B ∧ D). Closure theorem (yudi/nezha):
//   egress contained ⟺ child can't acquire a network fd (A) ∧ holds none (B)
//                       ∧ the arch-guard is tight (D).
// Each leg is proven by a red-team arm, never assumed. The whole proof
// LOUD-skips (never false-passes) when kernel seccomp is unavailable.
// ===========================================================================

#[cfg(target_os = "linux")]
fn raise_nofile(target: u64) {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: get/setrlimit operate on this process's own NOFILE limit.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) } == 0 {
        rl.rlim_cur = target.min(rl.rlim_max);
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &rl) };
    }
}

/// The load-bearing t11 proof. Spawns the hostile child inside the REAL seccomp
/// egress filter (installed by `jail::jail_command`, the same installer the
/// production `turn_executor::roundtrip` calls) and asserts every acquire / send
/// / steal / arch-renumber arm is contained.
#[cfg(target_os = "linux")]
#[test]
fn redteam_executor_egress_is_contained() {
    use bwoc_harness::seccomp;

    if !seccomp::available() {
        eprintln!(
            "[t11 redteam] LOUD SKIP: kernel seccomp (kill_process action) unavailable — \
             cannot prove egress containment (this is NOT a pass)."
        );
        return;
    }

    let worktree = tempfile::tempdir().unwrap();
    let tempdir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let ssh_canary = outside.path().join("id_rsa");
    std::fs::write(&ssh_canary, "K").unwrap();
    let checkpoint_canary = outside.path().join("c.json");
    std::fs::write(&checkpoint_canary, "{}").unwrap();
    let outside_write = outside.path().join("e.txt");

    // Control-B leak regression (condition #8 — "new fd >= 1024 auto-re-reds").
    // Leak an INHERITABLE network fd at a HIGH number into the child. The
    // executor's `close_range(EXECUTOR_FD+1, ~0)` (vs the old `4..1024` loop)
    // must close it; the child's `holds_no_network_fd` arm scans past 1024 and
    // must find nothing.
    raise_nofile(8192);
    // SAFETY: raw socket()/dup2() on our own descriptors; closed below.
    let leaked = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    let high_fd = 2000;
    let mut leaked_high = -1;
    if leaked >= 0 && unsafe { libc::dup2(leaked, high_fd) } == high_fd {
        leaked_high = high_fd; // dup2 clears CLOEXEC → inheritable
    }

    let config = serde_json::json!({
        "ssh_canary": ssh_canary.to_str().unwrap(),
        "outside_write": outside_write.to_str().unwrap(),
        "checkpoint_canary": checkpoint_canary.to_str().unwrap(),
        "parent_pid": std::process::id(),
    })
    .to_string();

    let spec = JailSpec::executor(worktree.path(), tempdir.path(), &redteam_bin());
    let mut cmd = Command::new(redteam_bin());
    cmd.current_dir(tempdir.path())
        .env("REDTEAM_CONFIG", &config);
    let _status = jail::jail_command(&mut cmd, &spec);
    let output = cmd.output().expect("spawn redteam child");

    // SAFETY: close our own leaked descriptors.
    if leaked >= 0 {
        unsafe { libc::close(leaked) };
    }
    if leaked_high >= 0 {
        unsafe { libc::close(leaked_high) };
    }

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "t11: redteam child emitted no JSON report.\nstdout={:?}\nstderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )
    });
    eprintln!("[t11 redteam] leaked_high_fd={leaked_high} report={report}");

    // ── A — cannot ACQUIRE a network fd ──────────────────────────────────────
    arm_blocked_or_skip(&report, "net_socket");
    arm_blocked_or_skip(&report, "net_abstract_connect");
    arm_blocked_or_skip(&report, "pidfd_getfd_steal");
    // Precision (negative) control: a LOCAL socketpair is deliberately allowed —
    // proves the filter is not a blanket socket ban that would break tokio.
    arm_allowed_or_skip(&report, "net_socketpair");
    // ── B — HOLDS no network fd (incl. the leaked >= 1024 socket) ─────────────
    arm_blocked_or_skip(&report, "holds_no_network_fd");
    arm_blocked_or_skip(&report, "splice_to_existing_fd");
    // ── ptrace (CRIT-1) — seccomp KILL and/or C4 EPERM ───────────────────────
    arm_blocked_or_skip(&report, "ptrace_parent");
    // ── D — the arch-guard is tight (x86_64 probes; LOUD-skip elsewhere) ──────
    arm_blocked_or_skip(&report, "arch_i386_int80");
    arm_blocked_or_skip(&report, "arch_x32_renumber");
}

/// t11 condition #5 — every Landlock-WRITABLE path the executor grants must be a
/// LOCAL filesystem. A "local write" onto NFS/CIFS/SMB/9p/FUSE is a network
/// egress in disguise (the FS jail would happily allow it). Standing regression:
/// adding a networked path to the executor `rw` set re-reds this on CI.
// `f_type as i64` is a no-op on 64-bit Linux but normalizes the field width
// across libc targets where `__fsword_t` differs; keep it for the comparison.
#[cfg(target_os = "linux")]
#[allow(clippy::unnecessary_cast)]
#[test]
fn landlock_writable_paths_are_local_fs() {
    let worktree = tempfile::tempdir().unwrap();
    let tempdir = tempfile::tempdir().unwrap();
    let spec = JailSpec::executor(worktree.path(), tempdir.path(), &redteam_bin());

    // Networked / fuse fs magics that must NOT back a "local" writable path.
    const NFS: i64 = 0x6969;
    const SMB: i64 = 0x517B;
    const CIFS: i64 = 0xFF53_4D42u32 as i64;
    const SMB2: i64 = 0xFE53_4D42u32 as i64;
    const NINEP: i64 = 0x0102_1997;
    const FUSE: i64 = 0x6573_5546;
    let networked = [NFS, SMB, CIFS, SMB2, NINEP, FUSE];

    let mut checked = 0;
    for p in &spec.rw {
        if !p.exists() {
            continue;
        }
        let cpath = std::ffi::CString::new(p.to_string_lossy().as_bytes()).unwrap();
        let mut sfs: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: statfs reads fs metadata for `cpath` into our own `sfs`.
        assert_eq!(
            unsafe { libc::statfs(cpath.as_ptr(), &mut sfs) },
            0,
            "t11: statfs({p:?}) failed"
        );
        let ty = sfs.f_type as i64;
        assert!(
            !networked.contains(&ty),
            "t11 cond #5: Landlock-writable path {p:?} is on a NETWORKED fs \
             (f_type={ty:#x}) — a 'local write' there can egress"
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "t11: no writable paths were checked (spec empty?)"
    );
}
