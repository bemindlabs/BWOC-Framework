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
//! dyld shared-cache / mach / sysctl machinery basic exec needs), so the macOS
//! arm is **write confinement** plus a *narrowed* read residual: a selective
//! secret read-denylist (#329, Option D — [`sbpl_secret_read_block`]) blocks a
//! curated set of high-value secrets (`~/.ssh`, `~/.aws`, `~/.config/{gcloud,gh}`,
//! the BWOC home holding agent keys + checkpoints) while leaving ordinary reads
//! (and the loader's) intact. This is **not** full read-confinement parity —
//! an *unlisted* secret is still readable, and the ptrace / `/proc` guarantees
//! stay Linux-only, mirroring t6's honest "memory capping is Linux-only"
//! degrade. The factory degrades gracefully and the redteam suite LOUD-skips
//! the arms it cannot prove on macOS.
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
    /// macOS: write-confinement + a selective secret read-denylist (#329); the
    /// full read jail stays Linux-only (see module docs).
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
// macOS — sandbox-exec write confinement + selective secret read-deny (#329);
// full read jail is Linux-only (see module docs)
// ===========================================================================

/// Escape a path for embedding inside an SBPL double-quoted string literal.
#[cfg(target_os = "macos")]
pub(crate) fn sbpl_escape(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// macOS secret-read escape-hatch env var — the read-confinement analogue of the
/// network `BWOC_SANDBOX_ALLOW_NET` seam.
///
/// The selective secret read-deny arm (#329, Option D) is **on by default** and
/// fail-closed: honored only when set to exactly `"1"`. It exists solely as a
/// test/operator seam for the rare case a legitimate subprocess must read one of
/// the denied dirs (mirroring the Linux posture of running such a tool in the
/// parent). Default-absent ⇒ secret reads denied.
///
/// **Scope (honest).** A *narrowed residual*, not macOS read-confinement parity
/// with Linux Landlock: a curated **denylist** of known high-value secret paths —
/// an unlisted secret is still readable. A full deny-default read arm is
/// deliberately avoided (it breaks the dyld shared-cache reads the loader needs);
/// see THREAT-MODEL Residuals and issue #329.
#[cfg(target_os = "macos")]
pub const BWOC_SANDBOX_ALLOW_SECRET_READ_ENV: &str = "BWOC_SANDBOX_ALLOW_SECRET_READ";

/// Whether the macOS secret-read escape-hatch is engaged. Fail-closed: any value
/// other than exactly `"1"` keeps secret reads denied.
#[cfg(target_os = "macos")]
pub(crate) fn sbpl_allow_secret_read() -> bool {
    std::env::var(BWOC_SANDBOX_ALLOW_SECRET_READ_ENV).as_deref() == Ok("1")
}

/// The curated set of high-value secret paths denied to the turn-executor on
/// macOS (#329, Option D). Resolves `$HOME`-relative credential dirs plus the
/// BWOC home that holds agent keys + SessionTrust checkpoints (`$BWOC_HOME` else
/// `$HOME/.bwoc`, mirroring [`crate::checkpoint`]'s root resolution).
#[cfg(target_os = "macos")]
pub(crate) fn secret_read_deny_paths() -> Vec<PathBuf> {
    secret_read_deny_paths_from(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("BWOC_HOME").map(PathBuf::from),
    )
}

/// Pure core of [`secret_read_deny_paths`]: builds the candidate set from the
/// given `home`/`bwoc_home`, canonicalizes each, drops the ones that do not
/// exist (denying a nonexistent path protects nothing, and canonicalization
/// would fail), and dedupes. No env reads — unit-testable with temp dirs.
#[cfg(target_os = "macos")]
pub(crate) fn secret_read_deny_paths_from(
    home: Option<PathBuf>,
    bwoc_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = home {
        candidates.push(home.join(".ssh"));
        candidates.push(home.join(".aws"));
        candidates.push(home.join(".config").join("gcloud"));
        candidates.push(home.join(".config").join("gh"));
        // Agent keys + SessionTrust checkpoints live under the BWOC home.
        candidates.push(home.join(".bwoc"));
    }
    if let Some(bwoc_home) = bwoc_home {
        candidates.push(bwoc_home);
    }

    let mut out: Vec<PathBuf> = Vec::new();
    for c in candidates {
        if let Ok(canon) = std::fs::canonicalize(&c) {
            if !out.contains(&canon) {
                out.push(canon);
            }
        }
    }
    out
}

/// Render the SBPL secret-read-deny block shared by the turn-executor jail
/// ([`macos_write_confine_profile`]) and the tool sandbox
/// (`sandbox::build_sbpl_profile`) so the two macOS read surfaces cannot drift
/// (#329).
///
/// Emits one `(deny file-read* (subpath …))` per canonical `secret_paths` entry,
/// then a `(allow file-read* (subpath …))` per `reallow_paths` entry so the
/// confinement roots stay readable even if a secret dir lexically contains one
/// (SBPL is last-match-wins). Returns a comment line — no deny, no re-allow —
/// when the escape-hatch is engaged or the secret set is empty. The block is a
/// `file-read*` arm only; it never touches the caller's `file-write*` rules.
#[cfg(target_os = "macos")]
pub(crate) fn sbpl_secret_read_block(
    secret_paths: &[PathBuf],
    allow_secret_read: bool,
    reallow_paths: &[PathBuf],
) -> String {
    if allow_secret_read {
        return "; secret reads allowed via BWOC_SANDBOX_ALLOW_SECRET_READ escape-hatch"
            .to_string();
    }
    if secret_paths.is_empty() {
        return "; no secret paths resolved on this host".to_string();
    }
    let mut lines = String::new();
    for p in secret_paths {
        lines.push_str(&format!(
            "(deny file-read* (subpath \"{}\"))\n",
            sbpl_escape(p)
        ));
    }
    for r in reallow_paths {
        let canon = std::fs::canonicalize(r).unwrap_or_else(|_| r.clone());
        lines.push_str(&format!(
            "(allow file-read* (subpath \"{}\"))\n",
            sbpl_escape(&canon)
        ));
    }
    // Trim the trailing newline; the caller controls line joins.
    lines.pop();
    lines
}

/// Build a `sandbox-exec` SBPL profile that allows everything by default,
/// **denies writes** outside the `rw` subtrees (canonicalized — macOS `/tmp`
/// and `/var` are symlinks, so an un-canonicalized subpath would never match),
/// and applies the selective secret read-deny arm (#329 — a *narrowed* residual,
/// not a full read jail; see module docs + THREAT-MODEL Residuals).
#[cfg(target_os = "macos")]
pub fn macos_write_confine_profile(spec: &JailSpec) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n");

    // Secret read-deny arm (#329). Re-allow the executor's own rw subtrees below
    // the denies so an overlapping secret dir can never block a confinement root.
    let secret_block = sbpl_secret_read_block(
        &secret_read_deny_paths(),
        sbpl_allow_secret_read(),
        &spec.rw,
    );
    p.push_str(&secret_block);
    p.push('\n');

    p.push_str("(deny file-write*)\n");
    let mut allow = String::from("(allow file-write*");
    for path in &spec.rw {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        allow.push_str(&format!(" (subpath \"{}\")", sbpl_escape(&canon)));
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
// t11 — post-fork fd hygiene (control B: the child holds no network fd)
// ===========================================================================

/// Close every inherited descriptor above `keep_max` and re-point any
/// socket-backed stdio (0/1/2) at `/dev/null`. This is the **no-fd invariant**
/// (t11 control B): a forked-then-exec'd child must hold no network fd it did
/// not open itself. Shared by `turn_executor::roundtrip` (`keep_max =
/// EXECUTOR_FD`, preserving the IPC socket) and [`jail_command`] (`keep_max =
/// 2`, stdio only) so the two paths cannot drift.
///
/// Post-fork / async-signal-safe: raw libc only (`close_range`/`fstat`/`open`/
/// `dup2`), no allocation, no locks. MUST be called in a `pre_exec` closure.
// `as u32` on the mode bits is a no-op on Linux (mode_t = u32) but load-bearing
// on macOS (mode_t = u16); keep it for cross-platform correctness.
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)]
pub fn harden_child_fds(keep_max: i32) -> std::io::Result<()> {
    // 1. Close the whole fd table above {0..=keep_max} in one syscall. The old
    //    `4..1024` loop left any inherited fd >= 1024 OPEN; `close_range` makes
    //    the no-fd invariant total (covers a leaked high network fd).
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        // SAFETY: close_range only closes descriptors in [first, last]; with
        // first = keep_max+1 it never touches {0..=keep_max}.
        if unsafe { libc::close_range((keep_max + 1) as libc::c_uint, libc::c_uint::MAX, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        // No close_range (macOS): best-effort bounded close loop, matching the
        // pre-t11 behaviour. macOS is not the seccomp egress target.
        let mut fd = keep_max + 1;
        while fd < 1024 {
            // SAFETY: close on a possibly-unopen fd is a harmless EBADF no-op.
            unsafe { libc::close(fd) };
            fd += 1;
        }
    }
    // 2. stdio socket audit — a network-backed 0/1/2 would be a held egress fd
    //    that survives close_range. Re-point any such socket at /dev/null.
    let mut i = 0;
    while i <= 2 {
        // SAFETY: fstat only reads descriptor metadata into our own `st`.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(i, &mut st) } == 0
            && (st.st_mode as u32 & libc::S_IFMT as u32) == libc::S_IFSOCK as u32
        {
            // SAFETY: open/dup2/close act on our own descriptors only.
            let nul = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDWR) };
            if nul >= 0 {
                unsafe { libc::dup2(nul, i) };
                if nul > 2 {
                    unsafe { libc::close(nul) };
                }
            }
        }
        i += 1;
    }
    Ok(())
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
        let ruleset = build_ruleset(spec);
        let status = match ruleset {
            Some(_) => JailStatus::Enforced,
            None => {
                eprintln!(
                    "[bwoc-harness:jail] WARNING: Landlock unavailable; command runs without the \
                     FS jail (FS confinement not enforced). [Phase 5 t7a]"
                );
                JailStatus::Unavailable
            }
        };
        // t11 — compile the seccomp egress filter (parent-side allocation) and
        // install it post-fork INDEPENDENTLY of Landlock: egress containment must
        // hold even if the FS jail is unavailable. Best-effort here (gated on a
        // non-destructive `available()` probe) so an FS-only caller (the C7 git
        // path) does not break on a seccomp-less kernel; the t11 gate checks the
        // same probe and LOUD-skips rather than false-pass. Production
        // (`turn_executor::roundtrip`) is STRICTLY fail-closed instead.
        let seccomp_bpf = if crate::seccomp::available() {
            crate::seccomp::build_filter()
        } else {
            None
        };
        // SAFETY: only async-signal-safe work runs post-fork; `ruleset`/`bpf`
        // are moved in to keep them alive until restrict_self / install.
        unsafe {
            cmd.pre_exec(move || {
                // 1. Landlock FS jail (if available) — must precede the fd close.
                if let Some(ref owned) = ruleset {
                    // SAFETY (covered by the enclosing `unsafe` on `pre_exec`):
                    // `owned` is a live ruleset fd built in the parent.
                    restrict_current_thread(owned.as_raw_fd())?;
                }
                // 2. t11 control B — no-fd invariant (stdio preserved).
                harden_child_fds(2)?;
                // 3. t11 — seccomp egress filter LAST (after fd hygiene). The
                //    shared installer is the SAME fn the production path calls.
                //    Fail-closed: an install error aborts the spawn.
                if let Some(ref bpf) = seccomp_bpf {
                    crate::seccomp::install_in_child(bpf)?;
                }
                Ok(())
            });
        }
        status
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

    // ── macOS secret read-deny (#329, Option D) — shared with sandbox.rs ───────

    /// The shared block renders one deny-read per secret path plus a re-allow per
    /// confinement root. Pure — explicit inputs, no env.
    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_secret_read_block_renders_denies_and_reallows() {
        let secret = tempfile::tempdir().unwrap();
        let rw = tempfile::tempdir().unwrap();
        let secret_canon = std::fs::canonicalize(secret.path()).unwrap();
        let rw_canon = std::fs::canonicalize(rw.path()).unwrap();

        let block = sbpl_secret_read_block(
            std::slice::from_ref(&secret_canon),
            false,
            std::slice::from_ref(&rw_canon),
        );
        assert!(
            block.contains(&format!(
                "(deny file-read* (subpath \"{}\"))",
                secret_canon.to_string_lossy()
            )),
            "block must deny-read the secret path; got:\n{block}"
        );
        assert!(
            block.contains(&format!(
                "(allow file-read* (subpath \"{}\"))",
                rw_canon.to_string_lossy()
            )),
            "block must re-allow the confinement root; got:\n{block}"
        );
    }

    /// Escape-hatch engaged, or an empty secret set, both collapse to a bare
    /// comment — no deny, no orphaned re-allow. Pure.
    #[cfg(target_os = "macos")]
    #[test]
    fn sbpl_secret_read_block_escape_hatch_and_empty_are_comments() {
        let secret = tempfile::tempdir().unwrap();
        let secret_canon = std::fs::canonicalize(secret.path()).unwrap();
        let rw = vec![secret_canon.clone()];

        let hatched = sbpl_secret_read_block(std::slice::from_ref(&secret_canon), true, &rw);
        assert!(
            !hatched.contains("(deny file-read*") && hatched.starts_with(';'),
            "escape-hatch must render a comment only; got:\n{hatched}"
        );
        let empty = sbpl_secret_read_block(&[], false, &rw);
        assert!(
            !empty.contains("file-read*") && empty.starts_with(';'),
            "empty secret set must render a comment only; got:\n{empty}"
        );
    }

    /// `secret_read_deny_paths_from` canonicalizes existing dirs, skips missing
    /// ones, and dedupes an overlapping BWOC_HOME. Pure — explicit args, no env.
    #[cfg(target_os = "macos")]
    #[test]
    fn secret_read_deny_paths_from_canonicalize_skip_dedup() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
        std::fs::create_dir_all(home.path().join(".bwoc")).unwrap();

        let resolved = secret_read_deny_paths_from(
            Some(home.path().to_path_buf()),
            Some(home.path().join(".bwoc")), // dedupe vs $HOME/.bwoc
        );
        let ssh = std::fs::canonicalize(home.path().join(".ssh")).unwrap();
        let bwoc = std::fs::canonicalize(home.path().join(".bwoc")).unwrap();
        assert!(resolved.contains(&ssh));
        assert!(resolved.contains(&bwoc));
        assert_eq!(
            resolved.len(),
            2,
            "missing dirs skipped + BWOC_HOME deduped"
        );
    }

    /// Host-independent structural invariant: whenever the executor profile emits
    /// a secret read-deny arm, it sits ABOVE the file-write rules (they are
    /// distinct effect families, but the ordering documents intent and guards a
    /// future regression that might merge the arms).
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_executor_profile_secret_arm_precedes_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = JailSpec::executor(tmp.path(), tmp.path(), Path::new("/bin/echo"));
        let profile = macos_write_confine_profile(&spec);
        if let Some(read_at) = profile.find("(deny file-read*") {
            let write_at = profile.find("(deny file-write*)").unwrap();
            assert!(
                read_at < write_at,
                "secret read-deny must precede the file-write rules; got:\n{profile}"
            );
        }
    }
}
