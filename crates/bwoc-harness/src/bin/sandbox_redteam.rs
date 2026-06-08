//! Phase 5 t7a + t11 — hostile turn-executor child (red-team adversary).
//!
//! Built ONLY under `--features test-redteam`. `tests/sandbox_escape.rs` spawns
//! this binary **inside the same jail the real turn-executor uses** (FS Landlock
//! for t7a + the seccomp egress filter for t11, both via `jail::jail_command`),
//! hands it the canary paths + the stand-in parent's pid via `REDTEAM_CONFIG`,
//! and asserts every escape below is contained. Run *unjailed/unfiltered*, every
//! one would succeed — that contrast is the proof.
//!
//! # Two failure modes, two harnesses (t11 fork-per-arm)
//!
//! - **t7a FS arms** fail with an *errno* (Landlock `EACCES`). They run
//!   **in-process** (`arm`): a denied filesystem op just returns `Err`.
//! - **t11 egress / ptrace arms** are killed by `SECCOMP_RET_KILL_PROCESS`
//!   (`SIGSYS`). They MUST run in a **forked child** (`kill_arm`) so the SIGSYS
//!   takes only that child and the reporter (this process) survives to print the
//!   verdict. The parent `waitpid`s and reads the child's exit/term status:
//!   * killed by `SIGSYS`            ⇒ blocked (seccomp contained it),
//!   * exited non-zero (errno)       ⇒ blocked (the syscall returned an error),
//!   * exited **zero** (escape ran)  ⇒ NOT blocked — the containment FAILED.
//!
//! Each arm reports `{"blocked": bool, "detail": "..."}`; some add `"skip": true`
//! on a platform/arch where the control does not apply (the gate then LOUD-skips
//! that arm rather than false-passing).

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

    // ───────────────────────── t7a — FS arms (in-process, errno) ────────────
    // 1. Read a secret OUTSIDE the worktree (~/.ssh canary). Also the build.rs
    //    RCE confinement proof (CRIT-2): worktree code reading a host secret.
    out.insert(
        "read_ssh_canary".into(),
        arm(|| {
            std::fs::read(&ssh_canary)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
    );
    // 2. Write OUTSIDE the worktree (also build.rs payload-drop proof).
    out.insert(
        "write_outside_worktree".into(),
        arm(|| std::fs::write(&outside_write, b"pwned").map_err(|e| e.to_string())),
    );
    // 3. Overwrite the SessionTrust checkpoint canary (M2).
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
    // 5. Read the parent's environment via /proc (C4 — keys live there).
    out.insert(
        "read_parent_environ".into(),
        arm(|| {
            std::fs::read(format!("/proc/{parent_pid}/environ"))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
    );

    // ───────────────── t11 / C4 — ptrace + egress arms (fork-per-arm) ───────
    // ptrace/process_vm_readv the parent's RAM (CRIT-1): blocked by seccomp KILL
    // (t11) and/or PR_SET_DUMPABLE(0) → EPERM (C4).
    out.insert("ptrace_parent".into(), ptrace_parent_arm(parent_pid));

    // Control A — the child cannot ACQUIRE a network fd.
    out.insert("net_socket".into(), net_socket_arm());
    out.insert("net_socketpair".into(), net_socketpair_arm());
    out.insert("net_abstract_connect".into(), net_abstract_connect_arm());
    out.insert("pidfd_getfd_steal".into(), pidfd_getfd_arm());

    // Control B — the child HOLDS no network fd (no egress target to send/splice).
    out.insert("holds_no_network_fd".into(), holds_no_network_fd_arm());
    out.insert("splice_to_existing_fd".into(), splice_to_existing_fd_arm());

    // Control D — the arch-guard is tight (non-native + x32 renumber killed).
    out.insert("arch_i386_int80".into(), arch_i386_int80_arm());
    out.insert("arch_x32_renumber".into(), arch_x32_renumber_arm());

    let report = serde_json::Value::Object(out);
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(
        serde_json::to_string(&report)
            .unwrap_or_else(|_| "{}".into())
            .as_bytes(),
    );
    let _ = stdout.flush();
}

/// In-process arm — `blocked == true` ⇔ it returned `Err` (the op failed).
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

// ===========================================================================
// Linux — the fork-per-arm SIGSYS harness + the t11 egress/ptrace/arch arms
// ===========================================================================

/// Run a KILL-prone escape in a **forked child** so a `SIGSYS` from
/// `SECCOMP_RET_KILL_PROCESS` does not take down this reporter. `f` returns the
/// child exit code: **0 = the escape ran to completion (NOT blocked)**, non-zero
/// = the syscall returned an error (blocked-by-errno). A child killed by SIGSYS
/// is blocked-by-seccomp. `detail` records exactly which.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn kill_arm<F: FnOnce() -> i32>(f: F) -> serde_json::Value {
    // SAFETY: this binary is single-threaded (no async runtime); fork is safe
    // and the child does only async-signal-safe libc work before _exit.
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        let code = f();
        unsafe { libc::_exit(code) };
    }
    if pid < 0 {
        return serde_json::json!({ "blocked": false, "detail": "fork() failed" });
    }
    let mut status: libc::c_int = 0;
    // SAFETY: waitpid only writes `status`.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    if libc::WIFSIGNALED(status) {
        let sig = libc::WTERMSIG(status);
        return serde_json::json!({
            "blocked": sig == libc::SIGSYS,
            "detail": format!("child killed by signal {sig} (SIGSYS={})", libc::SIGSYS)
        });
    }
    if libc::WIFEXITED(status) {
        let code = libc::WEXITSTATUS(status);
        return serde_json::json!({
            "blocked": code != 0,
            "detail": if code == 0 { "escape SUCCEEDED — child exited 0".into() }
                      else { format!("escape failed by errno — child exited {code}") }
        });
    }
    serde_json::json!({ "blocked": false, "detail": "child neither signaled nor exited cleanly" })
}

/// `true` ⇔ this fd refers to a socket (`S_IFSOCK`). async-signal-safe.
// `as u32` is a no-op on Linux (mode_t = u32) but needed where mode_t is narrower.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
#[allow(clippy::unnecessary_cast)]
fn fd_is_socket(fd: i32) -> bool {
    // SAFETY: fstat only reads descriptor metadata into our own `st`.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::fstat(fd, &mut st) };
    r == 0 && (st.st_mode as u32 & libc::S_IFMT as u32) == libc::S_IFSOCK as u32
}

#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn ptrace_parent_arm(parent_pid: i32) -> serde_json::Value {
    kill_arm(move || {
        let mut buf = [0u8; 16];
        let local = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        let remote = libc::iovec {
            iov_base: 0x1000 as *mut libc::c_void,
            iov_len: buf.len(),
        };
        // SAFETY: process_vm_readv only reads into our own `buf`; on the expected
        // permission failure (or seccomp KILL) it touches nothing.
        let ret = unsafe { libc::process_vm_readv(parent_pid, &local, 1, &remote, 1, 0) };
        if ret >= 0 { 0 } else { 1 }
    })
}

#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn net_socket_arm() -> serde_json::Value {
    kill_arm(|| {
        // SAFETY: socket() either is killed (SIGSYS) or returns an fd we close.
        let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        if fd >= 0 {
            unsafe { libc::close(fd) };
            0
        } else {
            1
        }
    })
}

/// NEGATIVE / precision control — `socketpair(AF_UNIX)` is DELIBERATELY allowed
/// (the production executor's tokio runtime needs it for its SIGCHLD self-pipe;
/// see `seccomp::net_deny` docs). Unlike `socket(AF_UNIX)` (killed), a socketpair
/// yields a *connected local pair* that can reach nothing new, so it is not an
/// egress vector. This arm proves the filter is PRECISE — it must NOT blanket-ban
/// sockets, which would sever the runtime; `allowed == true` is the desired
/// outcome. Runs in-process: an unexpected KILL here crashes the reporter and
/// surfaces as "no JSON report" — a loud failure, never a silent pass.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn net_socketpair_arm() -> serde_json::Value {
    let mut sv = [0i32; 2];
    // SAFETY: socketpair writes two fds into `sv` on success; both closed below.
    let r = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, sv.as_mut_ptr()) };
    let allowed = r == 0;
    if allowed {
        unsafe {
            libc::close(sv[0]);
            libc::close(sv[1]);
        }
    }
    serde_json::json!({
        "allowed": allowed,
        "detail": format!("socketpair(AF_UNIX) allowed={allowed} (local-only; filter-precision control)")
    })
}

/// Abstract-namespace AF_UNIX connect — the reach an FS jail cannot fence (the
/// socket has no path). seccomp's `socket` deny kills it at creation.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn net_abstract_connect_arm() -> serde_json::Value {
    kill_arm(|| {
        // SAFETY: socket() is the denied syscall; if it survives we close the fd.
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return 1; // could not even create the socket → blocked
        }
        let r = unsafe {
            let mut addr: libc::sockaddr_un = std::mem::zeroed();
            addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
            // Abstract socket: sun_path[0] == 0, then a name.
            let name = b"bwoc-redteam-abstract";
            for (i, b) in name.iter().enumerate() {
                addr.sun_path[1 + i] = *b as libc::c_char;
            }
            let len =
                (std::mem::size_of::<libc::sa_family_t>() + 1 + name.len()) as libc::socklen_t;
            libc::connect(fd, &addr as *const _ as *const libc::sockaddr, len)
        };
        unsafe { libc::close(fd) };
        if r == 0 { 0 } else { 1 }
    })
}

/// `pidfd_getfd` is the fd-THEFT primitive: it duplicates a live fd (incl. a
/// socket) out of a sibling — an acquisition path no FS jail sees. The syscall
/// is in the deny set, so it is killed before it can steal anything.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn pidfd_getfd_arm() -> serde_json::Value {
    kill_arm(|| {
        // SAFETY: raw syscall; killed by seccomp before it runs. Args are inert.
        let r = unsafe { libc::syscall(libc::SYS_pidfd_getfd, 0, 0, 0) };
        if r >= 0 { 0 } else { 1 }
    })
}

/// Control B — scan our own descriptor table; a held socket fd is an egress
/// target. With `harden_child_fds` (close_range + stdio audit) there is none.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn holds_no_network_fd_arm() -> serde_json::Value {
    let mut sockets = Vec::new();
    let mut fd = 0;
    while fd < 4096 {
        if fd_is_socket(fd) {
            sockets.push(fd);
        }
        fd += 1;
    }
    serde_json::json!({
        "blocked": sockets.is_empty(),
        "detail": format!("network (S_IFSOCK) fds held by the child: {sockets:?}")
    })
}

/// Control B — even a data-mover (`splice`/`sendto`) has no egress target: there
/// is no held network fd to send to, and acquiring one is killed.
#[cfg(all(unix, feature = "test-redteam", target_os = "linux"))]
fn splice_to_existing_fd_arm() -> serde_json::Value {
    kill_arm(|| {
        let mut target = -1;
        let mut fd = 0;
        while fd < 4096 {
            if fd_is_socket(fd) {
                target = fd;
                break;
            }
            fd += 1;
        }
        if target < 0 {
            return 1; // no network fd to splice/send through → blocked (B holds)
        }
        let buf = b"x";
        // SAFETY: sendto on the found socket; a denied syscall is SIGSYS-killed.
        let r = unsafe {
            libc::sendto(
                target,
                buf.as_ptr() as *const libc::c_void,
                1,
                0,
                std::ptr::null(),
                0,
            )
        };
        if r >= 0 { 0 } else { 1 }
    })
}

/// Control D — an i386 `int 0x80` enters via the ia32 compat path, so
/// `seccomp_data.arch == AUDIT_ARCH_I386 != X86_64`. The seccompiler arch
/// prologue must KILL it. x86_64-only probe.
#[cfg(all(
    unix,
    feature = "test-redteam",
    target_os = "linux",
    target_arch = "x86_64"
))]
fn arch_i386_int80_arm() -> serde_json::Value {
    kill_arm(|| {
        // SAFETY: `int 0x80` invokes the i386 syscall ABI (getpid=20). The arch
        // guard kills it before it runs; if it somehow runs, getpid is harmless.
        unsafe {
            core::arch::asm!(
                "int 0x80",
                in("eax") 20i32,
                lateout("eax") _,
                lateout("ecx") _,
                lateout("edx") _,
                options(nostack),
            );
        }
        0 // reached only if the i386 syscall was NOT killed → arch-guard FAILED
    })
}

#[cfg(all(
    unix,
    feature = "test-redteam",
    target_os = "linux",
    not(target_arch = "x86_64")
))]
fn arch_i386_int80_arm() -> serde_json::Value {
    serde_json::json!({ "blocked": false, "skip": true, "detail": "i386 int 0x80 probe is x86_64-only" })
}

/// Control D — a denied syscall renumbered with `__X32_SYSCALL_BIT` is a
/// different number; the x32-renumber deny entries must KILL it. x86_64-only.
#[cfg(all(
    unix,
    feature = "test-redteam",
    target_os = "linux",
    target_arch = "x86_64"
))]
fn arch_x32_renumber_arm() -> serde_json::Value {
    kill_arm(|| {
        const X32_SYSCALL_BIT: libc::c_long = 0x4000_0000;
        let nr = (libc::SYS_socket as libc::c_long) | X32_SYSCALL_BIT;
        // SAFETY: raw syscall with the x32-renumbered socket nr; killed by the
        // x32 deny entry before it runs.
        let r = unsafe { libc::syscall(nr, libc::AF_INET, libc::SOCK_STREAM, 0) };
        if r >= 0 { 0 } else { 1 }
    })
}

#[cfg(all(
    unix,
    feature = "test-redteam",
    target_os = "linux",
    not(target_arch = "x86_64")
))]
fn arch_x32_renumber_arm() -> serde_json::Value {
    serde_json::json!({ "blocked": false, "skip": true, "detail": "x32 renumber probe is x86_64-only" })
}

// ── non-Linux unix (macOS): the t11 egress controls are Linux-only. ─────────
#[cfg(all(unix, feature = "test-redteam", not(target_os = "linux")))]
mod non_linux_stubs {
    fn skip(what: &str) -> serde_json::Value {
        serde_json::json!({ "blocked": false, "skip": true, "detail": format!("{what} is Linux-only (t11 seccomp)") })
    }
    pub fn ptrace_parent_arm(_p: i32) -> serde_json::Value {
        skip("ptrace/process_vm_readv")
    }
    pub fn net_socket_arm() -> serde_json::Value {
        skip("socket")
    }
    pub fn net_socketpair_arm() -> serde_json::Value {
        skip("socketpair")
    }
    pub fn net_abstract_connect_arm() -> serde_json::Value {
        skip("abstract connect")
    }
    pub fn pidfd_getfd_arm() -> serde_json::Value {
        skip("pidfd_getfd")
    }
    pub fn holds_no_network_fd_arm() -> serde_json::Value {
        skip("fd snapshot")
    }
    pub fn splice_to_existing_fd_arm() -> serde_json::Value {
        skip("splice/sendto")
    }
    pub fn arch_i386_int80_arm() -> serde_json::Value {
        skip("i386 int 0x80")
    }
    pub fn arch_x32_renumber_arm() -> serde_json::Value {
        skip("x32 renumber")
    }
}
#[cfg(all(unix, feature = "test-redteam", not(target_os = "linux")))]
use non_linux_stubs::*;

#[cfg(not(all(unix, feature = "test-redteam")))]
fn main() {}
