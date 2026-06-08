//! Phase 5 t7a — hostile turn-executor child (red-team adversary).
//!
//! Built ONLY under `--features test-redteam`. `tests/sandbox_escape.rs` spawns
//! this binary **inside the same FS jail the real turn-executor uses**, hands it
//! the canary paths + the stand-in parent's pid via `REDTEAM_CONFIG`, and then
//! asserts every escape below is blocked. Run *unjailed*, every one of these
//! would succeed — that contrast is the proof: this binary is the attacker; the
//! jail (Landlock on Linux) + `PR_SET_DUMPABLE(0)` on the parent (C4) are what
//! must defeat it.
//!
//! A planted `build.rs` is exactly "worktree-resident code executing under the
//! C1 jail" (builds run in the in-child executor, which is jailed) — so the
//! `read_ssh_canary` + `write_outside_worktree` arms below ARE the build.rs
//! confinement proof (CRIT-2); no separate cargo build is needed.
//!
//! Each arm reports `{"blocked": bool, "detail": "..."}` where `blocked == true`
//! means the escape attempt FAILED (the desired outcome).

#[cfg(all(unix, feature = "test-redteam"))]
fn main() {
    use std::io::Write;

    let raw = std::env::var("REDTEAM_CONFIG").expect("REDTEAM_CONFIG must be set");
    let cfg: serde_json::Value = serde_json::from_str(&raw).expect("REDTEAM_CONFIG must be JSON");
    let s = |k: &str| {
        cfg.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let ssh_canary = s("ssh_canary");
    let outside_write = s("outside_write");
    let checkpoint_canary = s("checkpoint_canary");
    let parent_pid: i32 = cfg.get("parent_pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

    let mut out = serde_json::Map::new();

    // 1. Read a secret OUTSIDE the worktree (~/.ssh private-key canary). Also the
    //    build.rs-RCE confinement proof (CRIT-2): worktree code reading a host
    //    secret under the jail.
    out.insert(
        "read_ssh_canary".into(),
        arm(|| {
            std::fs::read(&ssh_canary)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
    );

    // 2. Write OUTSIDE the worktree. Also the build.rs-RCE confinement proof:
    //    worktree code dropping a payload outside the jail.
    out.insert(
        "write_outside_worktree".into(),
        arm(|| std::fs::write(&outside_write, b"pwned").map_err(|e| e.to_string())),
    );

    // 3. Overwrite the SessionTrust checkpoint canary (M2: the latch is written
    //    by the parent only; the child must not be able to forge it).
    out.insert(
        "overwrite_checkpoint".into(),
        arm(|| std::fs::write(&checkpoint_canary, b"{}").map_err(|e| e.to_string())),
    );

    // 4. Overwrite our own binary (M3: current_exe must be read+exec-only).
    out.insert(
        "overwrite_self_binary".into(),
        arm(|| {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            std::fs::OpenOptions::new()
                .write(true)
                .open(&exe)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
    );

    // 5. ptrace / process_vm_readv the parent's RAM (CRIT-1) — Linux/C4.
    out.insert("ptrace_parent".into(), ptrace_arm(parent_pid));

    // 6. Read the parent's environment via /proc (C4 — provider keys live there
    //    in the real parent; a non-dumpable parent makes this EACCES/EPERM, and
    //    Landlock independently denies /proc).
    out.insert(
        "read_parent_environ".into(),
        arm(|| {
            std::fs::read(format!("/proc/{parent_pid}/environ"))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
    );

    let report = serde_json::Value::Object(out);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(
        serde_json::to_string(&report)
            .unwrap_or_else(|_| "{}".into())
            .as_bytes(),
    );
    let _ = stdout.flush();
}

/// Run one escape attempt. `blocked == true` ⇔ it returned `Err` (failed).
#[cfg(all(unix, feature = "test-redteam"))]
fn arm<F: FnOnce() -> Result<(), String>>(f: F) -> serde_json::Value {
    match f() {
        Ok(()) => serde_json::json!({
            "blocked": false,
            "detail": "escape SUCCEEDED — jail did NOT contain it"
        }),
        Err(e) => serde_json::json!({ "blocked": true, "detail": e }),
    }
}

/// The ptrace/process_vm_readv arm (CRIT-1). Linux-only: a non-dumpable parent
/// (C4) yields `EPERM` because `ptrace_may_access` is checked before the address.
#[cfg(all(unix, feature = "test-redteam"))]
fn ptrace_arm(parent_pid: i32) -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        let mut buf = [0u8; 16];
        let local = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        // A plausible remote address; permission is checked first, so a
        // non-dumpable parent returns EPERM regardless of address validity.
        let remote = libc::iovec {
            iov_base: 0x1000 as *mut libc::c_void,
            iov_len: buf.len(),
        };
        // SAFETY: process_vm_readv only reads into our own `buf`; on a permission
        // failure (the expected outcome) it touches nothing.
        let ret = unsafe { libc::process_vm_readv(parent_pid, &local, 1, &remote, 1, 0) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            let blocked = err.raw_os_error() == Some(libc::EPERM);
            return serde_json::json!({
                "blocked": blocked,
                "detail": format!("process_vm_readv → {err}")
            });
        }
        serde_json::json!({
            "blocked": false,
            "detail": format!("process_vm_readv READ {ret} bytes from parent — CRIT-1 OPEN")
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = parent_pid;
        serde_json::json!({
            "blocked": false,
            "skip": true,
            "detail": "ptrace/process_vm_readv arm is Linux-only (C4)"
        })
    }
}

#[cfg(not(all(unix, feature = "test-redteam")))]
fn main() {}
