//! Phase 5 t9 — true per-turn process cap via cgroup v2 `pids.max` (best-effort).
//!
//! t6 gave every turn-executor child a per-UID, RELATIVE `RLIMIT_NPROC` fork
//! guard. That is **partial**: it is per-UID (not per-turn) and usage-relative,
//! so it bounds a runaway fork but does not give a turn its own absolute pid
//! ceiling. t9 closes that gap **where a delegated cgroup v2 subtree exists**: the
//! parent creates a per-turn cgroup leaf, writes its `pids.max`, and the child
//! moves itself into the leaf post-fork (in `pre_exec`) so the cap binds its whole
//! future process tree.
//!
//! # Posture — best-effort, with a split fail behaviour (Ruling 5)
//!
//! Whether a usable cgroup exists depends on deployment. Under systemd
//! `Delegate=yes` (the t14 deployment prereq) or a privileged container with a
//! writable `cgroupfs`, it does; on a dev box, bare SSH login, or a non-delegated
//! container, it does not. So the **default** is best-effort:
//!
//!   - **Parent setup fails / no delegated subtree** → **fail-OPEN**: a LOUD
//!     no-op, the turn keeps the t6 `RLIMIT_NPROC` floor, and the spawn is **never
//!     blocked**. ([`TurnCgroup::create`] returns `None`.) This is the common,
//!     non-delegated case — its residual is 🟠, documented in THREAT-MODEL.
//!   - **Child self-join fails** (the `write` of `"0"` to `cgroup.procs` in the
//!     child's `pre_exec`) → **fail-CLOSED**: abort the spawn. By then the parent
//!     has already committed the leaf and set `pids.max`; a silent failure here
//!     would let the child run **outside** the cage while the parent believes it
//!     is capped. The child half lives in `turn_executor::roundtrip`'s `pre_exec`,
//!     not here, because only there is it async-signal-safe to issue the raw
//!     `write` (this module's parent-side code allocates freely).
//!
//! The hard-guarantee switch (`BWOC_REQUIRE_CGROUP_PIDS`, checked at startup in
//! `main`) flips the default: when set, the harness refuses to start unless a
//! delegated subtree is present, so prod can demand the cap rather than degrade.
//!
//! # The delegation dance (cgroup v2, unprivileged)
//!
//! cgroup v2 forbids a non-root cgroup from enabling a controller in its
//! `cgroup.subtree_control` while it directly holds processes ("no internal
//! process" rule). To create per-turn leaves with `pids.max` we therefore, once:
//!   1. read our own cgroup from `/proc/self/cgroup` (`0::<path>`),
//!   2. move ourselves into a `_bwoc.supervisor` leaf (write our TGID to its
//!      `cgroup.procs`) so our cgroup holds no direct processes,
//!   3. enable the controller for children: `echo +pids > cgroup.subtree_control`,
//!   4. probe that a leaf + `pids.max` write + `rmdir` actually succeeds.
//!
//! All four are skipped/short-circuited when systemd already delegated a subtree
//! with `pids` enabled. Any failure ⇒ best-effort no-op (fail-open).
//!
//! Nothing here touches `bwoc-core` (dep-quarantine): std + libc only.

/// Whether a delegated, writable cgroup v2 subtree with the `pids` controller is
/// available — i.e. whether t9's `pids.max` cap can actually be enforced. Drives
/// the `BWOC_REQUIRE_CGROUP_PIDS` startup hard-guarantee switch (condition 2).
/// Always `false` off Linux (cgroups are a Linux facility).
#[cfg(target_os = "linux")]
pub fn delegated_subtree_available() -> bool {
    linux_impl::base().is_some()
}

/// Off-Linux stub: cgroup `pids.max` enforcement is a Linux-only control.
#[cfg(not(target_os = "linux"))]
pub fn delegated_subtree_available() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub use linux_impl::TurnCgroup;

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::fs;
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The unified cgroup v2 mount point (the only layout t9 supports; a v1/hybrid
    /// host simply reports "unavailable" and falls back to the NPROC floor).
    const CGROUP_MOUNT: &str = "/sys/fs/cgroup";

    /// The leaf we move ourselves into so our own cgroup can enable
    /// `subtree_control` (the "no internal process" rule — see module docs).
    const SUPERVISOR_LEAF: &str = "_bwoc.supervisor";

    /// Per-turn pid ceiling. Tunable via `BWOC_TURN_PIDS_MAX` (read in the PARENT),
    /// clamped to `[floor, ceiling]`; out-of-range / malformed / unset → default.
    /// An absolute per-turn cap — unlike the per-UID, relative `RLIMIT_NPROC`.
    fn pids_max_value() -> u64 {
        env_or_default("BWOC_TURN_PIDS_MAX", 512, 4, 65_536)
    }

    fn env_or_default(var: &str, default: u64, floor: u64, ceiling: u64) -> u64 {
        match std::env::var(var) {
            Ok(raw) => match raw.trim().parse::<u64>() {
                Ok(v) if (floor..=ceiling).contains(&v) => v,
                _ => {
                    eprintln!(
                        "[bwoc-harness:t9] {var}={raw:?} out of [{floor},{ceiling}] or malformed; \
                         using default {default}"
                    );
                    default
                }
            },
            Err(_) => default,
        }
    }

    /// The delegated parent cgroup under which per-turn leaves are created, with
    /// the `pids` controller enabled in its `subtree_control`. The delegation
    /// dance runs once (idempotent + process-global), memoised here.
    #[derive(Debug)]
    pub struct Base {
        dir: PathBuf,
    }

    static BASE: OnceLock<Option<Base>> = OnceLock::new();

    /// Resolve (once) the delegated base, running the delegation dance. `None`
    /// ⇒ no usable subtree ⇒ best-effort: t9 is a no-op and the t6 `RLIMIT_NPROC`
    /// floor stands. LOUD either way — never a silent decision.
    pub fn base() -> Option<&'static Base> {
        BASE.get_or_init(|| match detect_and_delegate() {
            Ok(b) => {
                eprintln!(
                    "[bwoc-harness:t9] cgroup v2 pids.max enforcement ENABLED \
                     (delegated subtree: {}, default pids.max={}). [Phase 5 t9]",
                    b.dir.display(),
                    pids_max_value()
                );
                Some(b)
            }
            Err(e) => {
                eprintln!(
                    "[bwoc-harness:t9] cgroup v2 pids.max enforcement UNAVAILABLE — \
                     falling back to the best-effort per-UID RLIMIT_NPROC floor: {e}. \
                     [Phase 5 t9]"
                );
                None
            }
        })
        .as_ref()
    }

    fn detect_and_delegate() -> io::Result<Base> {
        // 1. unified cgroup v2 hierarchy present? (its root carries
        //    `cgroup.controllers`; v1/hybrid hosts do not).
        if !Path::new(CGROUP_MOUNT).join("cgroup.controllers").exists() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no cgroup v2 unified hierarchy at /sys/fs/cgroup",
            ));
        }
        // 2. our own cgroup directory.
        let our_dir = our_cgroup_dir()?;
        // 3. ensure the `pids` controller is enabled for our children (the dance).
        ensure_pids_in_subtree_control(&our_dir)?;
        // 4. prove a leaf + pids.max write + rmdir actually works here.
        probe_leaf(&our_dir)?;
        Ok(Base { dir: our_dir })
    }

    /// Map `/proc/self/cgroup`'s unified entry (`0::<path>`) to an absolute dir.
    fn our_cgroup_dir() -> io::Result<PathBuf> {
        let raw = fs::read_to_string("/proc/self/cgroup")?;
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("0::") {
                let rel = rest.trim().trim_start_matches('/');
                return Ok(Path::new(CGROUP_MOUNT).join(rel));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "/proc/self/cgroup has no unified (0::) entry",
        ))
    }

    /// True iff `path` (a cgroup interface file) lists the whitespace-separated
    /// `needle` token. Missing/unreadable file ⇒ false.
    fn file_lists(path: &Path, needle: &str) -> bool {
        fs::read_to_string(path)
            .map(|s| s.split_whitespace().any(|t| t == needle))
            .unwrap_or(false)
    }

    fn write_str(path: &Path, val: &str) -> io::Result<()> {
        let mut f = fs::OpenOptions::new().write(true).open(path)?;
        f.write_all(val.as_bytes())
    }

    /// Enable the `pids` controller in `our_dir`'s `subtree_control`, performing
    /// the delegation dance if our cgroup still holds processes directly.
    fn ensure_pids_in_subtree_control(our_dir: &Path) -> io::Result<()> {
        let subtree = our_dir.join("cgroup.subtree_control");
        if file_lists(&subtree, "pids") {
            return Ok(()); // already delegated (e.g. systemd Delegate=yes).
        }
        if !file_lists(&our_dir.join("cgroup.controllers"), "pids") {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the `pids` controller is not available in our cgroup.controllers",
            ));
        }
        // "no internal process" rule: a cgroup must hold no direct processes to
        // enable a controller for its children. Move ourselves into a supervisor
        // leaf first. We ALWAYS move rather than special-casing `== CGROUP_MOUNT`:
        // under a cgroup namespace our own cgroup reads as `0::/` (so its dir is
        // CGROUP_MOUNT) yet is NOT the real root and is NOT exempt from the rule —
        // a privileged container is exactly this case. At the genuine real root the
        // move is unnecessary but harmless (and on a bare host without write
        // permission it simply fails → fail-open, the same outcome as before).
        move_self_to_supervisor_leaf(our_dir)?;
        write_str(&subtree, "+pids")
    }

    fn move_self_to_supervisor_leaf(our_dir: &Path) -> io::Result<()> {
        let leaf = our_dir.join(SUPERVISOR_LEAF);
        mkdir_idempotent(&leaf)?;
        // Writing our TGID moves the whole (multi-threaded) process in one shot.
        write_str(&leaf.join("cgroup.procs"), &std::process::id().to_string())
    }

    fn probe_leaf(our_dir: &Path) -> io::Result<()> {
        let probe = our_dir.join("_bwoc.probe");
        mkdir_idempotent(&probe)?;
        let r = write_str(&probe.join("pids.max"), "max");
        let _ = fs::remove_dir(&probe);
        r
    }

    fn mkdir_idempotent(dir: &Path) -> io::Result<()> {
        match fs::create_dir(dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Monotonic per-turn leaf discriminator (process-local; no wall clock).
    static TURN_SEQ: AtomicU64 = AtomicU64::new(0);

    /// A per-turn cgroup v2 leaf with `pids.max` set, holding an `O_WRONLY` handle
    /// on its `cgroup.procs` for the child's post-fork self-join. RAII: `Drop`
    /// kills any survivors then `rmdir`s the leaf, so reap / timeout / error paths
    /// all clean up (Ruling 5).
    pub struct TurnCgroup {
        dir: PathBuf,
        procs: OwnedFd,
    }

    impl TurnCgroup {
        /// Create the per-turn leaf in the delegated base, set `pids.max`, and open
        /// its `cgroup.procs`. `None` when no delegated subtree exists OR setup
        /// fails — **fail-OPEN**: a LOUD no-op that keeps the `RLIMIT_NPROC` floor
        /// and never blocks the spawn (Ruling 5, parent half).
        pub fn create() -> Option<Self> {
            let base = base()?;
            match Self::try_create(&base.dir) {
                Ok(g) => Some(g),
                Err(e) => {
                    eprintln!(
                        "[bwoc-harness:t9] WARNING: per-turn cgroup setup failed ({e}); this turn \
                         falls back to the RLIMIT_NPROC floor (fail-open, spawn NOT blocked). \
                         [Phase 5 t9]"
                    );
                    None
                }
            }
        }

        fn try_create(base_dir: &Path) -> io::Result<Self> {
            let seq = TURN_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = base_dir.join(format!("turn-{}-{seq}", std::process::id()));
            fs::create_dir(&dir)?;
            // pids.max must be in force BEFORE the child enters, so set it first.
            if let Err(e) = write_str(&dir.join("pids.max"), &pids_max_value().to_string()) {
                let _ = fs::remove_dir(&dir);
                return Err(e);
            }
            let file = match fs::OpenOptions::new()
                .write(true)
                .open(dir.join("cgroup.procs"))
            {
                Ok(f) => f,
                Err(e) => {
                    let _ = fs::remove_dir(&dir);
                    return Err(e);
                }
            };
            // File → OwnedFd via the always-stable raw-fd round-trip.
            let procs = unsafe { OwnedFd::from_raw_fd(file.into_raw_fd()) };
            Ok(Self { dir, procs })
        }

        /// The raw fd the child writes `"0"` into (post-fork, in `pre_exec`) to
        /// move itself into this cgroup. Captured as a plain `RawFd` (Copy) into
        /// the closure; the owning [`TurnCgroup`] outlives the spawn so the fd
        /// stays valid. The child write is fail-CLOSED (see module docs).
        pub fn procs_fd(&self) -> RawFd {
            self.procs.as_raw_fd()
        }
    }

    impl Drop for TurnCgroup {
        fn drop(&mut self) {
            // Ruling 5 cleanup — runs on reap, timeout, AND error (RAII). Kill any
            // lingering members (`cgroup.kill`, kernel ≥ 5.14) so the leaf is empty,
            // then rmdir. Best-effort + LOUD: a leaked empty leaf is inert.
            let _ = fs::write(self.dir.join("cgroup.kill"), b"1");
            if let Err(e) = fs::remove_dir(&self.dir) {
                eprintln!(
                    "[bwoc-harness:t9] WARNING: could not rmdir per-turn cgroup {}: {e} \
                     (leaked an inert empty leaf). [Phase 5 t9]",
                    self.dir.display()
                );
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pids_max_default_and_clamp() {
            // Default when unset.
            unsafe { std::env::remove_var("BWOC_TURN_PIDS_MAX") };
            assert_eq!(pids_max_value(), 512);
            // Out-of-range falls back to the default; in-range is honoured.
            unsafe { std::env::set_var("BWOC_TURN_PIDS_MAX", "0") };
            assert_eq!(pids_max_value(), 512);
            unsafe { std::env::set_var("BWOC_TURN_PIDS_MAX", "32") };
            assert_eq!(pids_max_value(), 32);
            unsafe { std::env::set_var("BWOC_TURN_PIDS_MAX", "garbage") };
            assert_eq!(pids_max_value(), 512);
            unsafe { std::env::remove_var("BWOC_TURN_PIDS_MAX") };
        }

        #[test]
        fn our_cgroup_dir_parses_unified_entry() {
            // Only meaningful where /proc/self/cgroup exists; tolerate its absence.
            if let Ok(p) = our_cgroup_dir() {
                assert!(p.starts_with(CGROUP_MOUNT));
            }
        }
    }
}
