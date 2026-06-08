//! Phase 5 t7a — process-level filesystem jail for the turn-executor (and the
//! parent's post-turn `git` on a child-touched worktree).
//!
//! This is the **process/FS-confinement** half of t7a (the SPLIT ruling: the
//! egress/seccomp/netns half is t7b, ticket t11). It confines a *whole process*
//! — not a single tool call — so that even code the executor never meant to run
//! (a `run_command` grandchild, a planted build artifact) cannot read or mutate
//! anything outside an explicit allowlist.
//!
//! # What it claims (and does not)
//!
//! On **Linux** (the production host + the gate-proof platform) a Landlock LSM
//! domain is installed on the executor process in its `pre_exec`:
//!   - **read+write+exec** on `{worktree, per-turn tempdir}` (the `rw` set),
//!   - **read+exec only** on the binary + a *minimal* system allowlist
//!     (loader, libc, `/usr` `/bin` `/lib*` `/etc` …) — NOT a blanket
//!     `/usr/bin` write or `/proc` read,
//!   - everything else (—`$HOME`, `~/.ssh`, the checkpoint dir, `/proc/<other>`)
//!     **denied**, and `no_new_privs` set (a Landlock prerequisite).
//!
//! This is what lets t7a claim the executor *cannot read/mutate the harness via
//! the filesystem*. It does **not** claim mount-namespace isolation and does
//! **not** claim egress containment (network / ssh-agent / abstract sockets —
//! that is t7b/t11). Do not describe it as "no shared writable mount".
//!
//! On **macOS** (a dev box, never production) the strict deny-default read jail
//! is too fragile to run a dynamically-linked binary (`sandbox-exec` denies the
//! mach/sysctl machinery basic exec needs), so the macOS arm is **write
//! confinement only** — exactly mirroring t6's honest "memory capping is
//! Linux-only" degrade. macOS does NOT block reads of secrets; the read /
//! ptrace / `/proc` guarantees are Linux-only. The factory degrades gracefully
//! and the redteam suite LOUD-skips the arms it cannot prove on macOS.
//!
//! # Async-signal-safety (Linux)
//!
//! The Landlock *ruleset* (which opens path fds and allocates) is built in the
//! **parent** before fork. Only the final `landlock_restrict_self` syscall +
//! `prctl(PR_SET_NO_NEW_PRIVS)` run post-fork, in `pre_exec`, via raw libc — no
//! allocation, no locks. This is mandatory: the parent is a multi-threaded tokio
//! runtime, so a post-fork heap allocation could deadlock on a copied malloc
//! lock. (Contrast the *grandchild* `run_command` Landlock in `sandbox.rs`,
//! which is forked from the child's single-threaded runtime.)

use std::path::{Path, PathBuf};

/// Where a jailed process may read/write/exec. Paths that do not exist are
/// skipped when the ruleset is built (so a missing `/lib32` is not an error).
#[derive(Debug, Clone, Default)]
pub struct JailSpec {
    /// Full read+write+exec subtrees (the worktree, the per-turn tempdir, …).
    pub rw: Vec<PathBuf>,
    /// Read+exec-only subtrees/files (the binary, loader, libc, system dirs).
    pub ro_exec: Vec<PathBuf>,
}

/// The outcome of trying to jail a process — surfaced so callers/tests can
/// LOUD-skip rather than silently pass when the jail is not enforceable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailStatus {
    /// Linux: a Landlock domain was installed (full read+write+exec jail).
    Enforced,
    /// macOS: write-confinement only (reads NOT jailed — see module docs).
    WriteConfineOnly,
    /// The platform/kernel cannot enforce a jail; the process runs unjailed.
    /// A LOUD warning was emitted. Callers must treat this as "not proven".
    Unavailable,
}

/// Minimal read+exec system allowlist (Linux). Deliberately NOT `/usr/bin`-only
/// and NOT `/proc`: just the directories a dynamically-linked binary needs to
/// load (loader, libc, system config like `ld.so.cache`) plus the standard
/// read-only tool/library trees. Nonexistent entries are skipped at build time.
#[cfg(target_os = "linux")]
const SYSTEM_RO: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/lib32",
    "/libx32",
    "/etc",
    "/opt",
    "/usr/local",
];

/// Read-only char devices a child may legitimately open (entropy + zero source).
#[cfg(target_os = "linux")]
const DEV_RO: &[&str] = &["/dev/urandom", "/dev/random", "/dev/zero"];

/// Char devices that need write access (`/dev/null` sink — also the
/// `core.hooksPath=/dev/null` target used by the C7 git hardening).
#[cfg(any(target_os = "linux", target_os = "macos"))]
const DEV_RW: &[&str] = &["/dev/null", "/dev/dtracehelper"];

impl JailSpec {
    /// The jail for the turn-executor itself: rw on the real worktree + the
    /// per-turn tempdir; read+exec on the binary being re-exec'd + the system
    /// allowlist. The binary is read+exec **only** (never rw) so `current_exe`
    /// cannot be overwritten from inside the jail (closes M3 / C8).
    pub fn executor(worktree: &Path, tempdir: &Path, program: &Path) -> Self {
        let mut rw = vec![worktree.to_path_buf(), tempdir.to_path_buf()];
        let mut ro_exec = vec![program.to_path_buf()];
        Self::push_system(&mut rw, &mut ro_exec);
        JailSpec { rw, ro_exec }
    }

    /// The jail for the parent's post-turn `git` on a child-touched worktree
    /// (C7): rw on the worktree + the git common dir (the linked-worktree gitdir
    /// lives outside the worktree); read+exec on the system allowlist. Combined
    /// with `core.hooksPath=/dev/null` and config overrides at the call site, a
    /// worktree that planted a hook / `core.fsmonitor` / `diff.external` cannot
    /// run code as the unjailed parent.
    pub fn for_git(worktree: &Path, git_common_dir: Option<&Path>) -> Self {
        let mut rw = vec![worktree.to_path_buf()];
        if let Some(gd) = git_common_dir {
            rw.push(gd.to_path_buf());
        }
        let mut ro_exec = Vec::new();
        Self::push_system(&mut rw, &mut ro_exec);
        JailSpec { rw, ro_exec }
    }

    /// Append the platform system allowlist (`rw` gets the writable devices,
    /// `ro_exec` gets the read-only system trees + entropy devices).
    fn push_system(rw: &mut Vec<PathBuf>, ro_exec: &mut Vec<PathBuf>) {
        #[cfg(target_os = "linux")]
        {
            for d in SYSTEM_RO {
                ro_exec.push(PathBuf::from(d));
            }
            for d in DEV_RO {
                ro_exec.push(PathBuf::from(d));
            }
            for d in DEV_RW {
                rw.push(PathBuf::from(d));
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (rw, ro_exec);
        }
    }
}

// ===========================================================================
// Linux — Landlock
// ===========================================================================

#[cfg(target_os = "linux")]
mod linux_impl {
    use super::JailSpec;
    use landlock::{
        ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
        path_beneath_rules,
    };
    use std::os::fd::OwnedFd;

    /// Build the Landlock ruleset for `spec` in the **parent** and return its
    /// fd. `None` means Landlock is unavailable on this kernel (best-effort
    /// downgraded to nothing) — the caller must LOUD-skip, never silent-pass.
    ///
    /// The returned `OwnedFd` is moved into the child's `pre_exec`, where only
    /// [`restrict_current_thread`] runs (async-signal-safe).
    pub fn build_ruleset(spec: &JailSpec) -> Option<OwnedFd> {
        // ABI::V1 covers read/write/exec — all t7a needs. BestEffort downgrades
        // (rather than erroring) on older kernels; on a Landlock-less kernel the
        // created ruleset carries no fd, which is our "unavailable" signal.
        let abi = ABI::V1;
        let ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::BestEffort)
            .handle_access(AccessFs::from_all(abi))
            .ok()?
            .create()
            .ok()?
            .add_rules(path_beneath_rules(&spec.rw, AccessFs::from_all(abi)))
            .ok()?
            .add_rules(path_beneath_rules(&spec.ro_exec, AccessFs::from_read(abi)))
            .ok()?;
        // `None` ⇔ Landlock unavailable (best-effort produced no ruleset fd).
        Option::<OwnedFd>::from(ruleset)
    }

    /// Restrict the **calling thread** to the ruleset `fd`. Async-signal-safe:
    /// only `prctl` + the raw `landlock_restrict_self` syscall, no allocation.
    /// MUST be called post-fork in `pre_exec`, before `execve`.
    ///
    /// # Safety
    /// `fd` must be a live Landlock ruleset fd created by [`build_ruleset`].
    pub unsafe fn restrict_current_thread(fd: i32) -> std::io::Result<()> {
        // no_new_privs is a Landlock prerequisite for an unprivileged process.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::syscall(libc::SYS_landlock_restrict_self, fd, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub use linux_impl::{build_ruleset, restrict_current_thread};

// ===========================================================================
// macOS — sandbox-exec write confinement (read jail NOT supported; see docs)
// ===========================================================================

/// Build a `sandbox-exec` SBPL profile that allows everything by default but
/// **denies writes** outside the `rw` subtrees (canonicalized — macOS `/tmp`
/// and `/var` are symlinks, so an un-canonicalized subpath would never match).
/// Reads are NOT confined on macOS (see module docs).
#[cfg(target_os = "macos")]
pub fn macos_write_confine_profile(spec: &JailSpec) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n(deny file-write*)\n");
    let mut allow = String::from("(allow file-write*");
    for path in &spec.rw {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let s = canon
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        allow.push_str(&format!(" (subpath \"{s}\")"));
    }
    for dev in DEV_RW {
        allow.push_str(&format!(" (literal \"{dev}\")"));
    }
    // Common write sinks a child may open even under write confinement.
    allow.push_str(" (literal \"/dev/zero\") (literal \"/dev/tty\")");
    allow.push_str(")\n");
    p.push_str(&allow);
    p
}

/// Locate `sandbox-exec` (ships at `/usr/bin/sandbox-exec` on every supported
/// macOS; PATH fallback for the unusual case).
#[cfg(target_os = "macos")]
pub fn which_sandbox_exec() -> Option<PathBuf> {
    let fixed = Path::new("/usr/bin/sandbox-exec");
    if fixed.exists() {
        return Some(fixed.to_path_buf());
    }
    std::env::var_os("PATH")
        .as_deref()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default()
        .split(':')
        .map(|dir| PathBuf::from(dir).join("sandbox-exec"))
        .find(|p| p.exists())
}

// ===========================================================================
// Standalone command jailing (C7 git, redteam spawns)
// ===========================================================================

/// Jail a **standalone** `std::process::Command` (one that needs no custom
/// `pre_exec` of its own — i.e. not the turn-executor, whose fd/rlimit
/// `pre_exec` is wired in `turn_executor::roundtrip`).
///
/// - **Linux:** installs a `pre_exec` that calls `landlock_restrict_self` on a
///   parent-built ruleset. Returns [`JailStatus::Enforced`], or
///   [`JailStatus::Unavailable`] (with a LOUD warning) on a Landlock-less kernel.
/// - **macOS:** rewrites the command to `sandbox-exec -p <profile> <prog> <args>`
///   (write confinement) and returns [`JailStatus::WriteConfineOnly`].
/// - **other unix:** no jail available → [`JailStatus::Unavailable`] (LOUD).
#[cfg(unix)]
pub fn jail_command(cmd: &mut std::process::Command, spec: &JailSpec) -> JailStatus {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        match build_ruleset(spec) {
            Some(owned) => {
                // SAFETY: only async-signal-safe syscalls run post-fork; `owned`
                // is moved in to keep the ruleset fd alive until restrict_self.
                unsafe {
                    cmd.pre_exec(move || {
                        // SAFETY: `owned` is a live ruleset fd; restrict the
                        // calling (post-fork) thread before execve.
                        unsafe { restrict_current_thread(owned.as_raw_fd())? };
                        Ok(())
                    });
                }
                JailStatus::Enforced
            }
            None => {
                eprintln!(
                    "[bwoc-harness:jail] WARNING: Landlock unavailable; command runs UNJAILED \
                     (FS confinement not enforced). [Phase 5 t7a]"
                );
                JailStatus::Unavailable
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let Some(sb) = which_sandbox_exec() else {
            eprintln!(
                "[bwoc-harness:jail] WARNING: sandbox-exec not found; command runs UNJAILED. \
                 [Phase 5 t7a]"
            );
            return JailStatus::Unavailable;
        };
        let profile = macos_write_confine_profile(spec);
        let std_cmd = cmd;
        // Capture original program + args, then rebuild as sandbox-exec wrapper,
        // preserving cwd + env (the rebuild discards them otherwise).
        let program = std_cmd.get_program().to_os_string();
        let args: Vec<std::ffi::OsString> = std_cmd.get_args().map(|a| a.to_os_string()).collect();
        let cwd = std_cmd.get_current_dir().map(|p| p.to_path_buf());
        let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = std_cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
            .collect();
        *std_cmd = std::process::Command::new(&sb);
        std_cmd.arg("-p").arg(&profile).arg(&program).args(&args);
        if let Some(d) = cwd {
            std_cmd.current_dir(d);
        }
        for (k, v) in envs {
            match v {
                Some(v) => std_cmd.env(k, v),
                None => std_cmd.env_remove(k),
            };
        }
        JailStatus::WriteConfineOnly
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (cmd, spec);
        eprintln!("[bwoc-harness:jail] WARNING: no FS jail on this platform. [Phase 5 t7a]");
        JailStatus::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_spec_binary_is_ro_not_rw() {
        let wt = Path::new("/tmp/wt");
        let td = Path::new("/tmp/td");
        let bin = Path::new("/usr/local/bin/bwoc-harness");
        let spec = JailSpec::executor(wt, td, bin);
        assert!(spec.rw.iter().any(|p| p == wt), "worktree must be rw");
        assert!(spec.rw.iter().any(|p| p == td), "tempdir must be rw");
        // C8/M3: the binary is read+exec only — never writable.
        assert!(
            spec.ro_exec.iter().any(|p| p == bin),
            "binary must be in ro_exec"
        );
        assert!(
            !spec.rw.iter().any(|p| p == bin),
            "binary must NOT be writable (current_exe overwrite guard)"
        );
    }

    #[test]
    fn git_spec_includes_common_dir() {
        let wt = Path::new("/tmp/wt");
        let gd = Path::new("/tmp/main/.git/worktrees/wt");
        let spec = JailSpec::for_git(wt, Some(gd));
        assert!(spec.rw.iter().any(|p| p == wt));
        assert!(
            spec.rw.iter().any(|p| p == gd),
            "linked-worktree gitdir must be rw so git can refresh its index"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_profile_denies_writes_and_allows_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = JailSpec::executor(tmp.path(), tmp.path(), Path::new("/bin/echo"));
        let profile = macos_write_confine_profile(&spec);
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains("(allow file-write*"));
        let canon = std::fs::canonicalize(tmp.path()).unwrap();
        assert!(
            profile.contains(canon.to_string_lossy().as_ref()),
            "profile must reference the canonical worktree path"
        );
    }
}
