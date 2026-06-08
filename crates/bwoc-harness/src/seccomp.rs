//! Phase 5 t11 (= t7b, the egress half of t7) — seccomp-bpf network-egress
//! containment for the turn-executor.
//!
//! # The closure theorem (yudi, adopting nezha's red-team result)
//!
//! > **egress contained ⟺ (A) the child can't acquire a network fd ∧ (B) it
//! > holds none ∧ (D) the arch-guard is tight.**
//!
//! The **no-fd invariant** (A ∧ B) is the PRIMARY control; the send-side deny
//! (`sendto`/`sendmsg`/…) is belt-and-suspenders for a fd that should not exist
//! in the first place. A ∧ B ∧ D are **proven by test** (the red-team arms in
//! `src/bin/sandbox_redteam.rs` + `tests/sandbox_escape.rs`), never assumed.
//!
//! | Leg | Claim | Mechanism | Proof arm |
//! |---|---|---|---|
//! | **A** | child can't *acquire* a network fd | `socket`/`socketpair`/`accept*`/`pidfd_getfd`/`io_uring_*` killed | net-acquire arms + `pidfd_getfd`-steal + abstract-socket connect |
//! | **B** | child *holds* no network fd | `close_range(3,~0,0)` + stdio (0/1/2) socket audit (turn_executor) | fd-snapshot arm (no `S_IFSOCK`) |
//! | **D** | the arch-guard is tight | seccompiler arch prologue (non-native → KILL) + x32 deny-set renumber guard | i386 `int 0x80` + x32-renumber arms |
//!
//! # Scope (honest)
//!
//! This is **network egress containment (Linux)**. It does NOT contain local,
//! same-uid covert channels — keyring / SysV-IPC / POSIX-mq / shared-file /
//! signals — those are explicitly **out of scope** (a same-uid sibling can
//! always reach them; seccomp is the wrong tool). See the THREAT-MODEL enforced
//! section + the charter.
//!
//! # Action choice: KILL_PROCESS, not ERRNO
//!
//! The deny set uses `SECCOMP_RET_KILL_PROCESS` — **never** `ERRNO`. An ERRNO
//! filter is a syscall-enumeration *oracle*: a hostile child could probe which
//! numbers return `-EPERM` vs `-ENOSYS` and map the filter. KILL_PROCESS gives
//! it one bit (death) and no oracle. The cost is that any code path legitimately
//! needing a denied syscall dies — by design: the turn-executor runs exactly one
//! approved tool and the harness's own network (provider HTTP) lives in the
//! *parent*, never the child.
//!
//! # Async-signal-safety (C1)
//!
//! [`build_filter`] (seccompiler BPF assembly — allocates) runs in the **parent**
//! before fork. Only [`install_in_child`] runs post-fork in `pre_exec`: two raw
//! `prctl` syscalls, no allocation, no locks. The same install fn is shared by
//! `turn_executor::roundtrip` (the production path) and `jail::jail_command` (the
//! red-team / standalone path), so they cannot drift.
//!
//! # Fail-closed (Linux)
//!
//! Egress containment is **mandatory**. If the filter cannot be installed the
//! child REFUSES to run (the `pre_exec` returns `Err`, the spawn fails) — the
//! harness never falls back to an unfiltered executor. Availability for *tests*
//! (which must LOUD-skip, not false-pass, on a seccomp-less kernel) is probed
//! non-destructively via [`available`].

/// Whether kernel seccomp-bpf with the `kill_process` action is available — a
/// non-destructive probe (reads `/proc/sys/kernel/seccomp/actions_avail`, which
/// exists iff `CONFIG_SECCOMP_FILTER`). Tests use this to choose *enforce* vs
/// *LOUD-skip*; production does NOT consult it (it fail-closes on install error).
#[cfg(target_os = "linux")]
pub fn available() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/seccomp/actions_avail")
        .map(|s| s.split_whitespace().any(|a| a == "kill_process"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
pub fn available() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub use linux_impl::{BpfProgram, build_filter, install_in_child};

#[cfg(target_os = "linux")]
mod linux_impl {
    use seccompiler::{SeccompAction, SeccompFilter, TargetArch};
    use std::collections::BTreeMap;

    pub use seccompiler::BpfProgram;

    /// `__X32_SYSCALL_BIT` — set in the syscall number for the x32 ABI on
    /// x86_64. A denied syscall renumbered with this bit is a DIFFERENT number,
    /// so we add the renumbered variants to the deny map too (control D).
    #[cfg(target_arch = "x86_64")]
    const X32_SYSCALL_BIT: i64 = 0x4000_0000;

    /// The network-acquisition deny set (KILL) — control A: there is **no way to
    /// obtain a network fd** in the child. `socket` covers every family (AF_INET/
    /// INET6/PACKET/… *and* AF_UNIX); `connect`/`accept*`/`bind`/`listen`/
    /// `getpeername` are the reach/address primitives; `pidfd_getfd`/`pidfd_open`
    /// let a child *steal* a live fd (incl. a socket) from a sibling — an
    /// acquisition path an FS jail cannot see; `io_uring_*` can perform network
    /// ops without the classic syscalls. `sendmmsg`/`recvmmsg` (the **batched**
    /// send/recv variants, binding cond #3) are denied as a send-side
    /// belt-and-suspenders: unlike the unary family they are not used by the
    /// executor's IPC or tokio, but CAN drive bulk egress on a leaked fd.
    ///
    /// Deliberately **NOT** denied (they are not network-egress primitives, and
    /// denying them would break the executor itself — see the module docs):
    ///   - `socketpair` — a *local* connected AF_UNIX pair that can only talk to
    ///     itself/descendants; tokio's runtime (used to drive a `run_command`
    ///     grandchild) creates one for its SIGCHLD self-pipe. (Contrast
    ///     `socket(AF_UNIX)`, which IS killed: it + `connect` can reach an
    ///     abstract-namespace listener; a socketpair can reach nothing new.)
    ///   - the **unary** `sendto`/`sendmsg`/`recvfrom`/`recvmsg` — once A ∧ B hold
    ///     there is no network fd to send/recv *on*, so the deny is redundant;
    ///     meanwhile the executor's own parent-IPC (an AF_UNIX socket, SCM_RIGHTS
    ///     via `sendmsg`) and tokio's internal sockets read/write via exactly
    ///     these, so a blanket KILL severs them. Only the batched `*mmsg` forms
    ///     (above), which neither uses, are denied.
    ///
    /// `socketcall` (the i386 socket multiplexer) has no x86_64/aarch64 number and
    /// is covered by the arch-mismatch KILL (control D), so it is not listed.
    pub(super) fn net_deny() -> Vec<libc::c_long> {
        vec![
            libc::SYS_socket,
            libc::SYS_connect,
            libc::SYS_accept,
            libc::SYS_accept4,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_getpeername,
            // Batched send/recv (binding cond #3) — NOT the unary family, which
            // the IPC/tokio need; the *mmsg forms are unused by either.
            libc::SYS_sendmmsg,
            libc::SYS_recvmmsg,
            libc::SYS_pidfd_getfd,
            libc::SYS_pidfd_open,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
        ]
    }

    /// Ptrace / cross-process-RAM deny set (KILL). Defence-in-depth alongside the
    /// parent's `PR_SET_DUMPABLE(0)` (C4): even a dumpable target cannot be
    /// inspected if the inspecting syscalls are gone.
    fn ptrace_deny() -> Vec<libc::c_long> {
        vec![
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_kcmp,
            libc::SYS_perf_event_open,
        ]
    }

    /// The native arch seccompiler must validate against. The arch prologue it
    /// emits KILLs any syscall whose `seccomp_data.arch` differs (control D:
    /// i386 `int 0x80`, any non-native personality).
    fn target_arch() -> Option<TargetArch> {
        #[cfg(target_arch = "x86_64")]
        {
            Some(TargetArch::x86_64)
        }
        #[cfg(target_arch = "aarch64")]
        {
            Some(TargetArch::aarch64)
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            None
        }
    }

    /// Compile the egress BPF filter (allocates — call in the PARENT only).
    /// `None` ⇒ this arch is unsupported or seccompiler refused the rules; on
    /// Linux the caller treats `None` as fail-closed.
    // `nr as i64`: `libc::SYS_*`/`c_long` is i64 on 64-bit (cast is a no-op
    // there) but i32 on a 32-bit target — the cast is load-bearing off 64-bit.
    #[allow(clippy::unnecessary_cast)]
    pub fn build_filter() -> Option<BpfProgram> {
        let arch = target_arch()?;

        // Allow-by-default; KILL the deny set. seccompiler maps an empty rule
        // vec to "match this syscall unconditionally" → `match_action`.
        let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
        for nr in net_deny().into_iter().chain(ptrace_deny()) {
            let nr = nr as i64;
            rules.insert(nr, vec![]);
            // Control D — x32 renumber guard: a denied syscall reached via the
            // x32 ABI carries a different number (nr | __X32_SYSCALL_BIT). Deny
            // that number too so the deny set cannot be slipped via x32.
            #[cfg(target_arch = "x86_64")]
            rules.insert(nr | X32_SYSCALL_BIT, vec![]);
        }

        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,       // mismatch (not in deny map) → allow
            SeccompAction::KillProcess, // match (in deny map) → SIGSYS-kill
            arch,
        )
        .ok()?;

        let bpf: BpfProgram = filter.try_into().ok()?;
        Some(bpf)
    }

    /// Install `bpf` on the **calling (post-fork) thread**. async-signal-safe:
    /// only `prctl`, no allocation. seccomp persists across `execve` and is
    /// inherited by any `run_command` grandchild, so it too is egress-contained.
    ///
    /// # Safety
    /// Must be called post-fork in `pre_exec` (before `execve`). `bpf` must be a
    /// live program built by [`build_filter`]; it must outlive this call (the
    /// caller keeps it owned in the closure).
    pub fn install_in_child(bpf: &BpfProgram) -> std::io::Result<()> {
        // PR_SET_NO_NEW_PRIVS — idempotent; set here so seccomp installs even
        // when Landlock (which also sets it) was unavailable. Do NOT rely on
        // Landlock for this prerequisite.
        // SAFETY: prctl(PR_SET_NO_NEW_PRIVS) only mutates this thread's own flag.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // seccompiler's BpfProgram is `Vec<sock_filter>` with the canonical
        // 8-byte Linux layout (code:u16, jt:u8, jf:u8, k:u32) — identical to
        // `libc::sock_filter`, so the slice reinterprets soundly.
        const _: () = assert!(core::mem::size_of::<libc::sock_filter>() == 8);
        let prog = libc::sock_fprog {
            len: bpf.len() as libc::c_ushort,
            filter: bpf.as_ptr() as *mut libc::sock_filter,
        };
        // SAFETY: `prog.filter` points at `bpf`'s live storage of `prog.len`
        // instructions; PR_SET_SECCOMP copies the program into the kernel.
        if unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER as libc::c_ulong,
                &prog as *const libc::sock_fprog as libc::c_ulong,
                0,
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn filter_builds_on_supported_arch() {
        // On x86_64/aarch64 the egress filter must compile; a None here on a
        // supported arch is a fail-closed condition in production.
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        assert!(
            build_filter().is_some(),
            "t11: seccomp egress filter failed to compile on a supported arch"
        );
    }

    /// Binding condition #3 + control-A coverage — a DETERMINISTIC membership
    /// check on the deny set (no kernel needed). It pins exactly what must be
    /// killed (incl. `pidfd_*` and the batched `*mmsg` egress variants cond #3
    /// names) AND what must stay allowed (the unary send/recv family + socketpair
    /// the IPC/tokio runtime needs). A future edit that drops a deny or denies an
    /// IPC primitive re-reds here, off any specific kernel.
    #[test]
    fn net_deny_set_membership_is_pinned() {
        let deny = linux_impl::net_deny();
        // MUST be killed (acquire / steal / batched egress).
        for nr in [
            libc::SYS_socket,
            libc::SYS_connect,
            libc::SYS_accept,
            libc::SYS_accept4,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_getpeername,
            libc::SYS_pidfd_getfd,
            libc::SYS_pidfd_open,
            libc::SYS_sendmmsg,
            libc::SYS_recvmmsg,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
        ] {
            assert!(
                deny.contains(&nr),
                "t11 cond#3: the net deny set is missing required syscall nr {nr}"
            );
        }
        // MUST stay allowed — denying any of these severs the executor's AF_UNIX
        // parent-IPC (SCM_RIGHTS via sendmsg) or tokio's runtime.
        for nr in [
            libc::SYS_sendto,
            libc::SYS_sendmsg,
            libc::SYS_recvfrom,
            libc::SYS_recvmsg,
            libc::SYS_socketpair,
        ] {
            assert!(
                !deny.contains(&nr),
                "t11: syscall nr {nr} must stay ALLOWED (IPC/tokio); denying it severs the runtime"
            );
        }
    }

    // Classic-BPF opcodes + seccomp constants (linux/filter.h, linux/seccomp.h).
    // These are fixed kernel ABI; the arch-guard MUST encode exactly this.
    const BPF_LD_W_ABS: u16 = 0x20; // BPF_LD(0x00) | BPF_W(0x00) | BPF_ABS(0x20)
    const BPF_JEQ_K: u16 = 0x15; // BPF_JMP(0x05) | BPF_JEQ(0x10) | BPF_K(0x00)
    const BPF_RET_K: u16 = 0x06; // BPF_RET(0x06) | BPF_K(0x00)
    const SECCOMP_DATA_ARCH_OFFSET: u32 = 4;
    const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
    #[cfg(target_arch = "x86_64")]
    const NATIVE_AUDIT_ARCH: u32 = 62 | 0x8000_0000 | 0x4000_0000; // AUDIT_ARCH_X86_64
    #[cfg(target_arch = "aarch64")]
    const NATIVE_AUDIT_ARCH: u32 = 183 | 0x8000_0000 | 0x4000_0000; // AUDIT_ARCH_AARCH64

    /// P0-1 — DETERMINISTIC, arch-independent proof that the egress filter opens
    /// with a tight arch-guard: load `seccomp_data.arch`, `JEQ` the NATIVE audit
    /// arch (skip the kill on match), else `RET SECCOMP_RET_KILL_PROCESS`. This
    /// inspects the actual emitted BPF, so it proves the prologue KILLs a
    /// non-native arch even on a host where we cannot *fire* a foreign syscall
    /// (e.g. aarch64, where the i386 `int 0x80` / x32 dynamic arms LOUD-skip).
    /// On x86_64 CI the dynamic red-team arms additionally fire the real probes.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn arch_guard_prologue_kills_non_native_arch() {
        let bpf = build_filter().expect("filter must compile on a supported arch");
        assert!(
            bpf.len() >= 3,
            "t11/P0-1: filter too short to hold the arch prologue"
        );
        // [0] A = seccomp_data.arch (load word at offset 4).
        assert_eq!(
            bpf[0].code, BPF_LD_W_ABS,
            "P0-1: prologue[0] must load a word"
        );
        assert_eq!(
            bpf[0].k, SECCOMP_DATA_ARCH_OFFSET,
            "P0-1: prologue[0] must load the arch field (offset 4)"
        );
        // [1] if A == native arch → skip the kill (jt=1); else fall through (jf=0).
        assert_eq!(bpf[1].code, BPF_JEQ_K, "P0-1: prologue[1] must be JEQ K");
        assert_eq!(
            bpf[1].k, NATIVE_AUDIT_ARCH,
            "P0-1: prologue[1] must compare against the NATIVE audit arch"
        );
        assert_eq!(
            (bpf[1].jt, bpf[1].jf),
            (1, 0),
            "P0-1: prologue[1] must skip-on-match / fall-through-on-mismatch"
        );
        // [2] the arch-mismatch path is an unconditional KILL_PROCESS.
        assert_eq!(bpf[2].code, BPF_RET_K, "P0-1: prologue[2] must be RET K");
        assert_eq!(
            bpf[2].k, SECCOMP_RET_KILL_PROCESS,
            "P0-1: arch mismatch MUST be SECCOMP_RET_KILL_PROCESS (not allow/errno)"
        );
    }

    /// The deny set must use `KILL_PROCESS` (not ERRNO — no enumeration oracle):
    /// every instruction in the compiled filter that is a `RET K` returns either
    /// `KILL_PROCESS` (deny) or `ALLOW` (the default for a non-denied syscall).
    /// No `RET` ever yields an `ERRNO` action.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn deny_set_uses_kill_process_never_errno() {
        const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
        const SECCOMP_RET_ERRNO: u32 = 0x0005_0000; // action mask of an errno RET
        const SECCOMP_RET_ACTION_FULL: u32 = 0xffff_0000;
        let bpf = build_filter().expect("filter must compile");
        for (i, inst) in bpf.iter().enumerate() {
            if inst.code == BPF_RET_K {
                let action = inst.k & SECCOMP_RET_ACTION_FULL;
                assert_ne!(
                    action, SECCOMP_RET_ERRNO,
                    "t11: RET[{i}] is ERRNO — the deny set must KILL, not give a syscall oracle"
                );
                assert!(
                    inst.k == SECCOMP_RET_KILL_PROCESS || action == SECCOMP_RET_ALLOW,
                    "t11: RET[{i}] action {:#x} is neither KILL_PROCESS nor ALLOW",
                    inst.k
                );
            }
        }
    }
}
