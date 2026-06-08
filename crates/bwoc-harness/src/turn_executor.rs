//! Phase 5 t5 — per-turn process isolation via `current_exe()` re-exec.
//!
//! After a tool call has been **approved by the safety pipeline**
//! (`PolicyOutcome::Proceed`), its *execution* no longer happens in the agent
//! process. Instead the parent re-execs its own binary as a hidden
//! `--__turn-executor` child, hands it a single framed request over an inherited
//! `socketpair`, and the child runs exactly that one tool then dies. The parent
//! keeps the LLM loop, the provider API keys, and the monotonic `SessionTrust`
//! latch; the child holds none of them.
//!
//! ```text
//! parent (LLM loop, keys, trust latch)
//!   │  policy Proceed ──► marshallable?
//!   │                       │ yes                       │ no (MCP/dynamic/cred)
//!   │  socketpair + token   ▼                           ▼
//!   └─ re-exec self ──► child (one tool, then _exit)    FAIL CLOSED (deny+surface)
//!        pre_exec: dup2→fd3, close others, setrlimit (CPU/AS/DATA/NOFILE/FSIZE
//!                  per-process + NPROC best-effort per-UID)
//! ```
//!
//! # C4 — resource containment (t6): per-process rlimits, NOT full isolation
//!
//! After the fd hygiene above, the child's `pre_exec` installs POSIX resource
//! limits via raw-libc `setrlimit` (the `// C3 / t6` slot below). All values are
//! hardcoded safe defaults, tunable within `[floor, ceiling]` via
//! `BWOC_TURN_RLIMIT_*` (read in the PARENT only). An override can never exceed
//! the ceiling and can never reach `unlimited` — no path yields `RLIM_INFINITY`.
//! What this does and does **not** contain:
//!
//! **Enforced per-process (Linux + macOS):**
//!   - `RLIMIT_CPU`    — soft 300 s / hard 600 s (SIGXCPU → SIGKILL): caps CPU spin.
//!   - `RLIMIT_NOFILE` — 1024: caps descriptor exhaustion.
//!   - `RLIMIT_FSIZE`  — 8 GiB: caps the disk-fill DoS (a single unbounded write).
//!
//! **Enforced per-process (Linux only):**
//!   - `RLIMIT_AS`     — 4 GiB default (8 GiB ceiling): caps address space / memory.
//!   - `RLIMIT_DATA`   — 4 GiB: caps the data segment.
//!
//! macOS `setrlimit` rejects `RLIMIT_AS`/`RLIMIT_DATA` (EINVAL), so **memory
//! capping is Linux-only**. The production host is Linux; on macOS dev boxes a
//! memory bomb is NOT contained by this layer.
//!
//! **Best-effort only — NOT full containment:**
//!   - `RLIMIT_NPROC` — soft = live per-UID process count + 128 (RELATIVE, not an
//!     absolute per-turn cap). Even where enforced it is per-UID, so it bounds a
//!     runaway fork but does **not** isolate this turn's process tree from the
//!     agent's other processes, and a busy host can perturb it. This is
//!     **PARTIAL** fork-bomb containment. Enforced on **Linux** (the kernel's
//!     per-UID counter and `/proc` enumeration agree); **macOS accepts the
//!     `setrlimit` but does not reliably enforce a usage-relative cap**, so on
//!     macOS it provides no real fork containment. The value is still set (cheap,
//!     finite, defence-in-depth) and the snapshot test proves it is finite.
//!
//! **Deferred gaps (still open after t6):**
//!   - A true per-turn process cap — cgroup v2 `pids.max` — ticket **t9**. Until
//!     then NPROC is the only fork guard and it is per-UID best-effort.
//!   - No cgroup memory/CPU controller, no `nice`/`ionice`, no PID/mount/net
//!     namespace. Non-Unix targets get NO rlimits (the spawn path is `cfg(unix)`).
//!
//! Net: t6 turns the bare process boundary t5 added into a resource-bounded one
//! for CPU, files, and disk (and memory on Linux) — but fork containment stays
//! best-effort until the cgroup work in t9. Do not describe this as full
//! isolation.
//!
//! # Authority model (C2) — unforgeable parent→child channel
//!
//! The child refuses to do anything unless BOTH hold:
//!   1. it inherited the IPC socket at the agreed fd (`BWOC_TURN_EXECUTOR_FD`), and
//!   2. it can present the one-time token the parent generated this spawn
//!      (`BWOC_TURN_EXECUTOR_TOKEN`), and the **framed request also carries that
//!      same token**, matched constant-time.
//!
//! Because the child does **not** re-run the safety gate, a valid framed request
//! is treated as full authority — so the channel must be unforgeable. The token
//! is 256 bits from `/dev/urandom`, fresh per spawn, and never written to disk.
//! An invocation with no token (or a wrong token) is rejected before any tool
//! runs (see [`run_executor_blocking`]). The token's env var name contains
//! `TOKEN`, so [`scrub_env`] strips it before any grandchild
//! (`run_command`'s child-of-child) is spawned — the capability does not leak
//! one level deeper.
//!
//! # Fail-closed marshalling (C5)
//!
//! Only **default-registry** tools are marshallable across the process boundary.
//! Dynamic / MCP (`mcp__*`) / credential-broker (keyring) tools cannot be
//! reconstructed in a fresh child and are therefore **denied (fail-closed)** —
//! never silently run in-process in the parent. See [`is_marshallable_tool`].
//!
//! # Filesystem confinement is preserved, not replaced (C7 / C12)
//!
//! The child receives the real worktree as its [`ToolContext::workdir`], so
//! `write_file` / `edit_file` / `memory_write` still pass `resolve_path` /
//! `confine_path` exactly as before — the process boundary is an *added* layer,
//! not a replacement for the FS allowlist. The child's *process cwd* is a
//! throwaway per-turn tempdir (hygiene), distinct from the confinement root.
//!
//! # Memory re-hydration (C8)
//!
//! The parent loop holds **no in-memory cache** of agent memory: `memory_read`
//! and `memory_write` are tools that touch `memories/` on disk, and both now run
//! in the child. A `memory_write` lands on disk in the child; the next
//! `memory_read` (also a child) reads it back from disk. There is no stale
//! parent-side copy to invalidate — re-hydration is automatic by construction.
//!
//! # Phase 5 t7a — FS jail (C1) + bounded IPC (C9)
//!
//! NOTE: the `C1`/`C3`/`C4` labels in the t6 section above are *t6's* condition
//! numbers (resource containment). The labels below are *t7a's*.
//!
//! - **C1 — FS jail on the executor process.** Beyond the bare process boundary
//!   (t5) and rlimits (t6), the child is now confined to a filesystem allowlist
//!   installed in `pre_exec`: a Landlock domain on Linux (rw = {worktree,
//!   per-turn tempdir}; read+exec = the binary + a minimal system allowlist;
//!   everything else — `$HOME`, the checkpoint dir, `/proc/<other>` — denied),
//!   or sandbox-exec write-confinement on macOS. The ruleset is built in the
//!   PARENT (allocation-heavy) and only the `landlock_restrict_self` syscall
//!   runs post-fork (async-signal-safe). See [`crate::jail`]. The binary is
//!   read+exec-ONLY (M3: `current_exe` can't be overwritten); the checkpoint dir
//!   is outside the rw set (M2: the latch is parent-written only). The
//!   `run_command` grandchild inherits this jail, so its own OS-sandbox layer is
//!   skipped when jailed (see [`ENV_JAILED`]).
//! - **C9 — bounded IPC.** The parent's read of the child's single response
//!   frame has a finite timeout ([`IPC_TIMEOUT`]) and the socket is shut down
//!   after one frame; the reader uses a plain `read()` with no cmsg buffer, so a
//!   malicious child cannot pass a descriptor back via `SCM_RIGHTS`.
//!
//! What t7a does NOT claim: mount-namespace isolation, or egress containment
//! (network / ssh-agent / abstract sockets) — that is t7b (ticket t11).

#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

#[cfg(unix)]
use crate::jail::{JailSpec, JailStatus};
use crate::sandbox::OsSandbox;
#[cfg(unix)]
use crate::sandbox::{make_os_sandbox, run_sandboxed, scrub_env};
use crate::tools::registry::{default_registry, dispatch};
use crate::tools::{ToolContext, ToolRegistry};
use bwoc_core::trust::TrustLevel;

// ---------------------------------------------------------------------------
// Wire constants
// ---------------------------------------------------------------------------

/// Hidden subcommand flag that switches the binary into turn-executor mode.
pub const EXECUTOR_FLAG: &str = "--__turn-executor";

/// Env var carrying the one-time capability token (C2). Name contains `TOKEN`
/// so [`scrub_env`] strips it before any grandchild process (C2-token).
const ENV_TOKEN: &str = "BWOC_TURN_EXECUTOR_TOKEN";

/// Env var carrying the inherited IPC socket fd number.
const ENV_FD: &str = "BWOC_TURN_EXECUTOR_FD";

/// Set (to `1`) when the parent installed the C1 FS jail on the executor
/// process. The child reads it to decide whether the `run_command` grandchild
/// still needs its own OS-sandbox layer: when jailed, the grandchild *inherits*
/// the executor's confinement (Landlock nests; macOS sandbox-exec is inherited
/// AND cannot be re-applied — nesting fails with EPERM), so the child uses a
/// no-op OS sandbox. When the jail is unavailable, the child falls back to the
/// per-grandchild [`make_os_sandbox`] so confinement is never silently lost.
#[cfg(unix)]
const ENV_JAILED: &str = "BWOC_TURN_EXECUTOR_JAILED";

/// The fixed fd the parent dup's the child socket onto in `pre_exec`.
#[cfg(unix)]
const EXECUTOR_FD: std::os::unix::io::RawFd = 3;

/// Hard cap on a single IPC frame (defence against a corrupt length prefix).
#[cfg(unix)]
const MAX_FRAME: usize = 64 * 1024 * 1024;

/// C9 — bound the parent's wait on the child's single response frame. A hung or
/// hostile child can never block the parent loop forever; the cap is generous
/// (> the t6 `RLIMIT_CPU` hard limit of 600 s) so a legitimately slow tool
/// (e.g. a build) is not cut off, but it is finite. Paired with an explicit
/// socket shutdown after the one frame is read (close-after-one-frame).
#[cfg(unix)]
const IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct WireRequest {
    /// One-time capability token; must equal the env token (C2/C13).
    token: String,
    tool_name: String,
    args_json: String,
    workdir: PathBuf,
    confine: bool,
    /// When true the child returns an introspection report instead of running a
    /// tool — used by the isolation tests (C9–C12).
    selftest: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct WireResponse {
    content: String,
}

// ---------------------------------------------------------------------------
// Resource limits (C3 / t6)
// ---------------------------------------------------------------------------
//
// The parent computes the entire rlimit plan (read env, measure UID usage,
// clamp to hardcoded ceilings) and captures it as **plain integers** into the
// `pre_exec` closure. The closure then only issues async-signal-safe
// `setrlimit` syscalls (C1) — it never parses env or allocates.
//
// Tunable via `BWOC_TURN_RLIMIT_*` (read in the PARENT only). Every override is
// clamped to `[floor, ceiling]`; a malformed / out-of-range / unset value falls
// back to the hardcoded default. No path can ever reach `RLIM_INFINITY`.

/// One resource limit as seen by callers/tests: name + the finite soft/hard the
/// parent intends (or the child actually applied). Values are byte/second/count
/// units depending on the resource; `RLIM_INFINITY_U64` would mean "unlimited"
/// and is asserted to never appear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RlimitView {
    pub name: String,
    pub soft: u64,
    pub hard: u64,
    /// `RLIMIT_NPROC` is per-UID and RELATIVE (live usage + headroom), so its
    /// exact value depends on the process count at spawn time. Callers
    /// range-check relative limits instead of asserting `==`.
    pub relative: bool,
}

/// `RLIM_INFINITY` as a `u64`, platform-correct (macOS = 2^63-1, Linux = u64::MAX).
/// The snapshot test asserts no applied limit equals this sentinel.
#[cfg(unix)]
pub const RLIM_INFINITY_U64: u64 = libc::RLIM_INFINITY;

/// The `resource` argument type for `setrlimit`/`getrlimit` differs by platform
/// (`__rlimit_resource_t` on Linux, `c_int` elsewhere).
#[cfg(all(unix, target_os = "linux"))]
type RlimitRes = libc::__rlimit_resource_t;
#[cfg(all(unix, not(target_os = "linux")))]
type RlimitRes = libc::c_int;

/// Internal table row: the resource id plus the finite caps the parent intends.
#[cfg(unix)]
struct RlimitEntry {
    name: &'static str,
    resource: RlimitRes,
    soft: u64,
    hard: u64,
    relative: bool,
}

/// Parse `BWOC_TURN_RLIMIT_*` (PARENT only). Within `[floor, ceiling]` → use it;
/// malformed / out-of-range / unset → hardcoded `default`. Never `RLIM_INFINITY`.
#[cfg(unix)]
fn env_or_default(var: &str, default: u64, floor: u64, ceiling: u64) -> u64 {
    match std::env::var(var) {
        Err(_) => default,
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(v) if (floor..=ceiling).contains(&v) => v,
            Ok(v) => {
                eprintln!(
                    "[bwoc-harness:t6] {var}={v} out of [{floor},{ceiling}]; \
                     using hardcoded default {default}"
                );
                default
            }
            Err(_) => {
                eprintln!(
                    "[bwoc-harness:t6] {var}={raw:?} malformed; \
                     using hardcoded default {default}"
                );
                default
            }
        },
    }
}

/// Current hard limit for `resource` (parent), or `None` if it reads as
/// `RLIM_INFINITY` / the query fails. Used to clamp our hard DOWN so we never
/// attempt to RAISE a hard limit (which would need privilege and could EPERM).
#[cfg(unix)]
fn current_hard(resource: RlimitRes) -> Option<u64> {
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit only reads the limit for a valid resource id.
    if unsafe { libc::getrlimit(resource, &mut rl) } != 0 {
        return None;
    }
    let hard = rl.rlim_max;
    if hard == RLIM_INFINITY_U64 {
        None
    } else {
        Some(hard)
    }
}

/// Build a finite, never-raising row: clamp `hard` down to the inherited hard
/// limit, then `soft <= hard`.
#[cfg(unix)]
fn entry(
    name: &'static str,
    resource: RlimitRes,
    soft: u64,
    hard: u64,
    relative: bool,
) -> RlimitEntry {
    let hard = match current_hard(resource) {
        Some(cur) => hard.min(cur),
        None => hard, // inherited hard is unlimited → our finite ceiling stands.
    };
    let soft = soft.min(hard);
    RlimitEntry {
        name,
        resource,
        soft,
        hard,
        relative,
    }
}

/// The UID of this process.
#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid is always safe and never fails.
    unsafe { libc::getuid() as u32 }
}

/// Best-effort count of live processes owned by this UID (PARENT side; alloc OK).
/// Backs the RELATIVE NPROC limit. A conservative non-zero floor is used if the
/// measurement is unavailable so the cap is never absurdly small.
#[cfg(all(unix, target_os = "linux"))]
fn current_uid_proc_count() -> u64 {
    use std::os::unix::fs::MetadataExt;
    let uid = current_uid();
    let mut count = 0u64;
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for ent in rd.flatten() {
            let name = ent.file_name();
            let Some(s) = name.to_str() else { continue };
            if !s.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            if let Ok(meta) = std::fs::metadata(ent.path()) {
                if meta.uid() == uid {
                    count += 1;
                }
            }
        }
    }
    if count == 0 { 64 } else { count }
}

/// Darwin/BSD: `proc_listpids(PROC_UID_ONLY, uid, NULL, 0)` returns the byte size
/// of the matching pid array; dividing by `sizeof(pid)` gives the process count.
/// (A size-query avoids allocating or enumerating the pids themselves.)
#[cfg(all(unix, not(target_os = "linux")))]
fn current_uid_proc_count() -> u64 {
    // PROC_UID_ONLY from <libproc.h>; not re-exported by libc, so spelled here.
    const PROC_UID_ONLY: u32 = 4;
    let uid = current_uid();
    // SAFETY: a size query (null buffer, 0 size) only returns the needed byte
    // length; no buffer is written.
    let bytes = unsafe { libc::proc_listpids(PROC_UID_ONLY, uid, std::ptr::null_mut(), 0) };
    if bytes > 0 {
        (bytes as u64) / (std::mem::size_of::<libc::c_int>() as u64)
    } else {
        64
    }
}

/// Build the full rlimit table for the current platform + env (PARENT side).
/// Single source of truth for the `pre_exec` plan, [`intended_rlimits`], and the
/// child's snapshot resource set.
#[cfg(unix)]
fn build_rlimit_table() -> Vec<RlimitEntry> {
    const MIB: u64 = 1024 * 1024;
    let mut t = Vec::with_capacity(6);

    // CPU seconds: soft → SIGXCPU, hard → SIGKILL. Bounds CPU spin.
    let cpu = env_or_default("BWOC_TURN_RLIMIT_CPU_SECS", 300, 1, 600);
    t.push(entry("CPU", libc::RLIMIT_CPU, cpu, 600, false));

    // Open files: bounds descriptor exhaustion. (Close loop above runs to 1024.)
    let nofile = env_or_default("BWOC_TURN_RLIMIT_NOFILE", 1024, 64, 8192);
    t.push(entry("NOFILE", libc::RLIMIT_NOFILE, nofile, 8192, false));

    // Max file size: closes the disk-fill DoS (one unbounded write).
    let fsize = env_or_default("BWOC_TURN_RLIMIT_FSIZE_MIB", 8192, 1, 32768);
    t.push(entry(
        "FSIZE",
        libc::RLIMIT_FSIZE,
        fsize * MIB,
        32768 * MIB,
        false,
    ));

    // Address space + data segment bound memory — Linux only: macOS `setrlimit`
    // rejects RLIMIT_AS/RLIMIT_DATA (EINVAL), so memory capping is Linux-only.
    // The production host is Linux; the C4 notice documents this gap.
    #[cfg(target_os = "linux")]
    {
        let as_mib = env_or_default("BWOC_TURN_RLIMIT_AS_MIB", 4096, 256, 8192);
        t.push(entry(
            "AS",
            libc::RLIMIT_AS,
            as_mib * MIB,
            8192 * MIB,
            false,
        ));
        let data_mib = env_or_default("BWOC_TURN_RLIMIT_DATA_MIB", 4096, 256, 8192);
        t.push(entry(
            "DATA",
            libc::RLIMIT_DATA,
            data_mib * MIB,
            8192 * MIB,
            false,
        ));
    }

    // NPROC — PARTIAL containment: per-UID and RELATIVE (live usage + headroom),
    // NOT an absolute per-turn cap. Bounds a runaway fork but does not isolate
    // this turn's tree from the agent's other processes. A true per-turn cap
    // (cgroup v2 pids.max) is deferred to ticket t9.
    let headroom = env_or_default("BWOC_TURN_RLIMIT_NPROC_HEADROOM", 128, 1, 4096);
    let usage = current_uid_proc_count();
    t.push(entry(
        "NPROC",
        libc::RLIMIT_NPROC,
        usage + headroom,
        usage + headroom * 2,
        true,
    ));

    t
}

/// The resource-limit plan the PARENT intends to apply, env-aware and
/// platform-aware. Tests compare the child's actually-applied limits against
/// this. (Relative limits — NPROC — are range-checked, not `==`.)
#[cfg(unix)]
pub fn intended_rlimits() -> Vec<RlimitView> {
    build_rlimit_table()
        .into_iter()
        .map(|e| RlimitView {
            name: e.name.to_string(),
            soft: e.soft,
            hard: e.hard,
            relative: e.relative,
        })
        .collect()
}

/// A single limit as plain integers, captured by-copy into the `pre_exec`
/// closure. No `String`, no allocation — safe to touch post-fork.
#[cfg(unix)]
#[derive(Clone, Copy)]
struct RlimitSpec {
    resource: RlimitRes,
    soft: libc::rlim_t,
    hard: libc::rlim_t,
}

/// Fixed-capacity, `Copy` plan moved into the `pre_exec` closure (≤ 6 limits).
#[cfg(unix)]
#[derive(Clone, Copy)]
struct RlimitPlan {
    specs: [RlimitSpec; 6],
    count: usize,
}

/// Lower the parent-computed table into the `Copy` plan the closure captures.
#[cfg(unix)]
fn build_rlimit_plan() -> RlimitPlan {
    let zero = RlimitSpec {
        resource: 0 as RlimitRes,
        soft: 0,
        hard: 0,
    };
    let mut specs = [zero; 6];
    let mut count = 0;
    for e in build_rlimit_table().into_iter().take(6) {
        specs[count] = RlimitSpec {
            resource: e.resource,
            soft: e.soft as libc::rlim_t,
            hard: e.hard as libc::rlim_t,
        };
        count += 1;
    }
    RlimitPlan { specs, count }
}

/// Child-side snapshot of the limits ACTUALLY in force after `pre_exec`, read via
/// `getrlimit`. Drives the always-on snapshot gate proof.
#[cfg(unix)]
fn snapshot_rlimits() -> Vec<RlimitView> {
    build_rlimit_table()
        .into_iter()
        .map(|e| {
            let mut rl = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: getrlimit only reads the limit for a valid resource id.
            let ok = unsafe { libc::getrlimit(e.resource, &mut rl) } == 0;
            RlimitView {
                name: e.name.to_string(),
                soft: if ok { rl.rlim_cur } else { RLIM_INFINITY_U64 },
                hard: if ok { rl.rlim_max } else { RLIM_INFINITY_U64 },
                relative: e.relative,
            }
        })
        .collect()
}

/// Introspection payload the child returns for a `selftest` request.
///
/// Captured **before** any async runtime is built, so `fds` reflects exactly the
/// inherited descriptor set (stdio + IPC) and not tokio's epoll/eventfd.
#[derive(Debug, Serialize, Deserialize)]
pub struct SelfTestReport {
    pub pid: u32,
    pub cwd: String,
    /// Every fd in `[0, 1024)` that is currently open in the child.
    pub fds: Vec<i32>,
    pub env_keys: Vec<String>,
    pub ipc_fd: i32,
    /// Reads a process-static atomic (initial 0), then sets it to 1. A fresh
    /// process always reports 0 — proves no shared static memory across turns.
    pub static_probe: u64,
    /// The resource limits ACTUALLY in force in this child (read via `getrlimit`
    /// after `pre_exec`), proving t6's `setrlimit` ran on the production path.
    pub rlimits: Vec<RlimitView>,
}

// ---------------------------------------------------------------------------
// Marshallability (C5)
// ---------------------------------------------------------------------------

/// Is `name` a default-registry tool, and therefore safe to marshal into a fresh
/// executor child?
///
/// Dynamic / MCP (`mcp__*`) / credential tools are absent from
/// [`default_registry`], so they return `false` here and are denied
/// fail-closed by [`execute_proceeded`] — never run in-process as a fallback.
pub fn is_marshallable_tool(name: &str) -> bool {
    static SET: OnceLock<std::collections::HashSet<String>> = OnceLock::new();
    let set = SET.get_or_init(|| default_registry().tool_names().into_iter().collect());
    set.contains(name)
}

// ---------------------------------------------------------------------------
// Parent-side execution outcome
// ---------------------------------------------------------------------------

/// The result of executing one approved tool call.
pub struct ExecOutcome {
    pub content: String,
    pub denied: bool,
    pub capability_denied: bool,
}

/// Execute one `Proceed`-approved tool call.
///
/// On **unix production builds**: marshallable tools run in an isolated
/// re-exec'd child; un-marshallable (dynamic/MCP/credential) tools are denied
/// fail-closed (C5) — never run in-process.
///
/// Under **`cfg(test)`** (this crate's own unit tests) the marshallable tool
/// runs in-process: a unit-test binary's `current_exe()` is the libtest harness,
/// which cannot serve as the executor. The real re-exec isolation is exercised
/// by the dedicated `tests/process_isolation.rs` integration suite, where the lib
/// is compiled non-test and drives the shipped `bwoc-harness` binary. The shipped
/// binary therefore contains **no** in-process path for marshallable tools.
///
/// On **non-unix** targets the re-exec isolation is unavailable
/// (no `socketpair`/`pre_exec`), so the prior in-process path is retained.
pub async fn execute_proceeded(
    tool_name: &str,
    args_json: &str,
    ctx: &ToolContext,
    registry: &ToolRegistry,
    os_sandbox: &dyn OsSandbox,
    turn_trust: TrustLevel,
) -> ExecOutcome {
    // C5 applies in every unix build: an un-marshallable tool is denied
    // fail-closed and never reaches in-process execution.
    #[cfg(unix)]
    if !is_marshallable_tool(tool_name) {
        eprintln!(
            "[bwoc-harness] process-isolation DENY (un-marshallable) \
             tool=`{tool_name}` turn_trust={turn_trust:?}"
        );
        return ExecOutcome {
            content: format!(
                "error: tool `{tool_name}` cannot run in the isolated turn-executor \
                 (dynamic/MCP/credential tool); denied (fail-closed). The executor only \
                 marshals default-registry tools — others are denied until IPC supports \
                 them. [Phase 5 t5]"
            ),
            denied: true,
            capability_denied: false,
        };
    }

    // Marshallable tool — production unix re-execs an isolated child.
    #[cfg(all(unix, not(test)))]
    {
        let _ = (registry, os_sandbox);
        let inv = ToolInvocation {
            tool_name: tool_name.to_string(),
            args_json: args_json.to_string(),
            workdir: ctx.workdir.clone(),
            confine: ctx.confine,
        };
        ExecOutcome {
            content: execute_via_isolated_process(inv).await,
            denied: false,
            capability_denied: false,
        }
    }
    // Unit-test build, or non-unix: execute in-process (see the doc note above).
    #[cfg(not(all(unix, not(test))))]
    {
        let _ = turn_trust;
        let content = run_in_process(tool_name, args_json, ctx, registry, os_sandbox).await;
        ExecOutcome {
            content,
            denied: false,
            capability_denied: false,
        }
    }
}

/// In-process tool execution (run_command sandboxed; others dispatched).
///
/// On unix this is the *child*'s execution body; on non-unix it is the retained
/// in-parent path (no process isolation available).
async fn run_in_process(
    tool_name: &str,
    args_json: &str,
    ctx: &ToolContext,
    registry: &ToolRegistry,
    os_sandbox: &dyn OsSandbox,
) -> String {
    if tool_name == "run_command" {
        match serde_json::from_str::<serde_json::Value>(args_json)
            .ok()
            .and_then(|v| v["command"].as_str().map(|s| s.to_string()))
        {
            #[cfg(unix)]
            Some(cmd) => match run_sandboxed(&cmd, &ctx.workdir, os_sandbox).await {
                Ok(output) => output.into_tool_result(),
                Err(e) => format!("error: {e}"),
            },
            #[cfg(not(unix))]
            Some(cmd) => {
                let _ = (&cmd, os_sandbox);
                dispatch(registry, tool_name, args_json, ctx).await
            }
            None => dispatch(registry, tool_name, args_json, ctx).await,
        }
    } else {
        let _ = os_sandbox;
        dispatch(registry, tool_name, args_json, ctx).await
    }
}

// ===========================================================================
// Unix re-exec mechanism
// ===========================================================================

/// A single tool invocation to be executed in an isolated child.
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub args_json: String,
    pub workdir: PathBuf,
    pub confine: bool,
}

/// What the parent observed about one isolated execution. The diagnostic fields
/// (`child_pid`, `cwd`) back the isolation tests (C11/C12 + baseline).
#[cfg(unix)]
#[derive(Debug)]
pub struct ExecutorOutcome {
    pub content: String,
    pub child_pid: u32,
    /// The per-turn tempdir handed to the child as its process cwd.
    pub cwd: PathBuf,
    /// C1 — whether the FS jail was actually enforced on this spawn. The
    /// redteam suite asserts this is `Enforced` on Linux (never silent-pass);
    /// macOS reports `WriteConfineOnly` (reads not jailed — see `jail` docs).
    pub jail_status: JailStatus,
}

/// Token-forging mode — test-only knob to exercise C2/C13.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeMode {
    /// Correct token in both env and request (production path).
    Valid,
    /// Omit the env token entirely (C13: invocation without the C2 token).
    NoEnvToken,
    /// Set a request token that does not match the env token (C2 mismatch).
    BadRequestToken,
}

/// Parent entry point used by the agent loop: run `inv` in an isolated child and
/// return the tool result string. Any spawn/IPC failure is surfaced as an error
/// string — there is **no in-process fallback** (fail-closed).
#[cfg(unix)]
pub async fn execute_via_isolated_process(inv: ToolInvocation) -> String {
    let program = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return format!(
                "error: process-isolation could not resolve current_exe: {e} \
                 (fail-closed, no in-process fallback) [Phase 5 t5]"
            );
        }
    };
    tokio::task::spawn_blocking(
        move || match roundtrip(&program, &inv, false, ForgeMode::Valid) {
            Ok(out) => out.content,
            Err(e) => format!(
                "error: isolated tool execution failed: {e} \
             (fail-closed, no in-process fallback) [Phase 5 t5]"
            ),
        },
    )
    .await
    .unwrap_or_else(|e| format!("error: executor task join failed: {e} [Phase 5 t5]"))
}

/// Run a tool in an isolated child (test/explicit-program form).
#[cfg(unix)]
pub fn run_isolated(program: &Path, inv: &ToolInvocation) -> std::io::Result<ExecutorOutcome> {
    roundtrip(program, inv, false, ForgeMode::Valid)
}

/// Drive a `selftest` round trip against `program`, returning the introspection
/// report as the outcome `content` (JSON).
#[cfg(unix)]
pub fn run_isolated_selftest(program: &Path, workdir: &Path) -> std::io::Result<ExecutorOutcome> {
    let inv = ToolInvocation {
        tool_name: "__selftest".to_string(),
        args_json: "{}".to_string(),
        workdir: workdir.to_path_buf(),
        confine: true,
    };
    roundtrip(program, &inv, true, ForgeMode::Valid)
}

/// Run a tool round trip with a forged authority channel (test-only, C13/C2).
#[cfg(unix)]
pub fn run_isolated_forged(
    program: &Path,
    inv: &ToolInvocation,
    forge: ForgeMode,
) -> std::io::Result<ExecutorOutcome> {
    roundtrip(program, inv, false, forge)
}

/// The parent half of the one-shot IPC: spawn the child, send one framed
/// request, read one framed response, and **always reap the child** (C11).
#[cfg(unix)]
fn roundtrip(
    program: &Path,
    inv: &ToolInvocation,
    selftest: bool,
    forge: ForgeMode,
) -> std::io::Result<ExecutorOutcome> {
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    // One-time 256-bit capability token (C2). Fresh per spawn, never persisted.
    let token = gen_token()?;

    // Bidirectional channel; both ends are CLOEXEC (UnixStream::pair), so only
    // the end we deliberately dup survives the child's exec.
    let (mut parent_sock, child_sock) = UnixStream::pair()?;
    let child_fd = child_sock.as_raw_fd();

    // Per-turn throwaway cwd (C12): distinct per spawn (baseline: separate
    // tempdir). The confinement root stays the real worktree in `inv.workdir`.
    let tmp = tempfile::Builder::new().prefix("bwoc-turn-").tempdir()?;
    let tmp_path = tmp.path().to_path_buf();

    // C6/C9: child env is the scrubbed parent env — no API/provider keys — plus
    // the isolation token + fd. scrub_env() must run before the token is added
    // (it would strip the token, whose name contains TOKEN).
    let safe_env = scrub_env();

    // C1 — the per-spawn FS jail spec for the executor: rw on the real worktree
    // + the per-turn tempdir; read+exec on the binary + a minimal system
    // allowlist. The binary is read+exec-ONLY (M3: `current_exe` cannot be
    // overwritten from inside the jail). The SessionTrust checkpoint dir lives
    // under `$BWOC_HOME`/`$HOME` — outside this rw set — so the child cannot
    // write it (M2 / C8): confinement is structural, not a runtime check.
    let jail_spec = JailSpec::executor(&inv.workdir, &tmp_path, program);

    // Prepare the platform jail. Linux: a Landlock ruleset built in the PARENT
    // (only the restrict syscall runs post-fork — async-signal-safe). macOS: a
    // write-confinement sandbox-exec wrapper (reads NOT jailed — see `jail`
    // docs). Both degrade gracefully with a LOUD warning; never silent-pass.
    #[cfg(target_os = "linux")]
    let (landlock_fd, jail_status) = match crate::jail::build_ruleset(&jail_spec) {
        Some(fd) => (Some(fd), JailStatus::Enforced),
        None => {
            eprintln!(
                "[bwoc-harness:t7a] WARNING: Landlock unavailable; turn-executor runs WITHOUT the \
                 FS jail (read/write confinement NOT enforced). [Phase 5 t7a/C1]"
            );
            (None, JailStatus::Unavailable)
        }
    };
    #[cfg(target_os = "macos")]
    let (macos_profile, jail_status) = match crate::jail::which_sandbox_exec() {
        Some(_) => (
            Some(crate::jail::macos_write_confine_profile(&jail_spec)),
            JailStatus::WriteConfineOnly,
        ),
        None => {
            eprintln!(
                "[bwoc-harness:t7a] WARNING: sandbox-exec not found; turn-executor runs WITHOUT \
                 write confinement. [Phase 5 t7a/C1]"
            );
            (None, JailStatus::Unavailable)
        }
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let jail_status = {
        let _ = &jail_spec;
        JailStatus::Unavailable
    };

    // Build the base command. On macOS the executor is wrapped in `sandbox-exec`
    // (the fd-dup/rlimit `pre_exec` still runs first, then execve(sandbox-exec),
    // which execs the real binary — fd3 + rlimits inherit through it).
    #[cfg(target_os = "macos")]
    let mut cmd = match &macos_profile {
        Some(profile) => {
            let sb = crate::jail::which_sandbox_exec().expect("sandbox-exec present");
            let mut c = Command::new(sb);
            c.arg("-p").arg(profile).arg(program).arg(EXECUTOR_FLAG);
            c
        }
        None => {
            let mut c = Command::new(program);
            c.arg(EXECUTOR_FLAG);
            c
        }
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = {
        let mut c = Command::new(program);
        c.arg(EXECUTOR_FLAG);
        c
    };

    cmd.current_dir(&tmp_path)
        .env_clear()
        .envs(&safe_env)
        .env(ENV_FD, EXECUTOR_FD.to_string());
    // Tell the child whether it is already FS-jailed (so the run_command
    // grandchild skips its now-redundant — and on macOS un-nestable — OS-sandbox
    // layer). Set after env_clear/envs so scrub_env can't strip it; the
    // grandchild's own scrub then drops it (not allowlisted) so it goes no
    // deeper.
    if jail_status != JailStatus::Unavailable {
        cmd.env(ENV_JAILED, "1");
    }
    if forge != ForgeMode::NoEnvToken {
        cmd.env(ENV_TOKEN, &token);
    }

    // C3/t6: compute the resource-limit plan in the PARENT (read env, measure
    // UID usage, clamp to ceilings). It is captured by-copy as plain integers;
    // the closure below never parses env or allocates.
    let rlimit_plan = build_rlimit_plan();

    // SAFETY (C1/C3): this closure runs in the forked child, after fork and
    // before exec. It calls ONLY raw libc / the async-signal-safe
    // `landlock_restrict_self` syscall — no allocation, no locks, no Rust std
    // that could deadlock a post-fork child of the multi-threaded parent. The
    // Landlock ruleset was BUILT in the parent (above); here we only issue the
    // final restrict syscall on the inherited (CLOEXEC) ruleset fd. Do NOT
    // introduce nix/rustix wrappers here.
    unsafe {
        cmd.pre_exec(move || {
            // Place the IPC socket at the agreed fd and clear CLOEXEC (dup2 of
            // distinct fds clears it; fcntl handles the oldfd==newfd no-op case).
            if libc::dup2(child_fd, EXECUTOR_FD) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(EXECUTOR_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // C3 / t6: install POSIX resource limits (plan computed in parent).
            // CPU/NOFILE/FSIZE (+ AS/DATA on Linux) per-process; NPROC is a
            // best-effort per-UID fork guard (cgroup pids.max deferred to t9).
            let mut i = 0;
            while i < rlimit_plan.count {
                let s = rlimit_plan.specs[i];
                let rl = libc::rlimit {
                    rlim_cur: s.soft,
                    rlim_max: s.hard,
                };
                if libc::setrlimit(s.resource, &rl) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                i += 1;
            }
            // C1: install the Landlock FS jail (Linux). MUST run BEFORE the
            // close loop below — the ruleset fd was inherited from the parent
            // and the close loop would otherwise close it pre-restrict. After
            // restrict_self the CLOEXEC ruleset fd is dropped at execve. (macOS
            // jails via the sandbox-exec wrapper — no per-fd action here.)
            #[cfg(target_os = "linux")]
            if let Some(ref fd) = landlock_fd {
                crate::jail::restrict_current_thread(fd.as_raw_fd())?;
            }
            // C10: close every other inherited fd so the child holds exactly
            // {0,1,2,EXECUTOR_FD}. CLOEXEC already covers Rust-opened fds; this
            // loop is the belt-and-suspenders guarantee the fd test asserts.
            let mut fd = EXECUTOR_FD + 1;
            while fd < 1024 {
                libc::close(fd);
                fd += 1;
            }
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    let child_pid = child.id();
    // Parent closes its copy of the child end; only `parent_sock` remains.
    drop(child_sock);

    // C9: bound every parent read/write on the IPC socket so a hung or hostile
    // child can never block the parent loop forever. The frame reader uses a
    // plain `read()` with NO ancillary (cmsg) buffer, so any SCM_RIGHTS fd a
    // malicious child tries to pass back is discarded by the kernel (MSG_CTRUNC)
    // — the channel cannot smuggle a descriptor into the parent.
    let _ = parent_sock.set_read_timeout(Some(IPC_TIMEOUT));
    let _ = parent_sock.set_write_timeout(Some(IPC_TIMEOUT));

    // Send the single framed request.
    let req = WireRequest {
        token: match forge {
            ForgeMode::BadRequestToken => format!("{token}-tampered"),
            _ => token.clone(),
        },
        tool_name: inv.tool_name.clone(),
        args_json: inv.args_json.clone(),
        workdir: inv.workdir.clone(),
        confine: inv.confine,
        selftest,
    };
    let payload = serde_json::to_vec(&req).map_err(std::io::Error::other)?;
    write_frame(&mut parent_sock, &payload)?;

    // Read the response (a refused child closes the socket → read error).
    let resp_result = read_frame(&mut parent_sock);

    // C9: close-after-one-frame — exactly one response is expected; drop the
    // half-open channel immediately so no further bytes can ever be read.
    let _ = parent_sock.shutdown(std::net::Shutdown::Both);

    // C11: reap unconditionally — no zombie, even on the fail-closed path.
    let _ = child.wait();
    // `tmp` drops here → the per-turn cwd is removed.

    let resp_bytes = resp_result?;
    let resp: WireResponse = serde_json::from_slice(&resp_bytes).map_err(std::io::Error::other)?;
    Ok(ExecutorOutcome {
        content: resp.content,
        child_pid,
        cwd: tmp_path,
        jail_status,
    })
}

// ---------------------------------------------------------------------------
// Child side
// ---------------------------------------------------------------------------

/// True when this process was invoked as the hidden turn-executor.
#[cfg(unix)]
pub fn is_executor_invocation() -> bool {
    std::env::args().any(|a| a == EXECUTOR_FLAG)
}

/// Fresh-process probe — read-then-set; a brand-new process always reads 0.
#[cfg(unix)]
static FRESH_PROBE: AtomicU64 = AtomicU64::new(0);

/// The child entry point. Verifies authority (C2/C13), then either returns a
/// selftest report or executes exactly one marshallable tool. Returns the
/// process exit code. Builds **no** async runtime until after the pre-runtime
/// snapshot, so the inherited-fd set stays clean (C10).
#[cfg(unix)]
pub fn run_executor_blocking() -> i32 {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::net::UnixStream;

    // C2/C13: the one-time token MUST be present in env. Absent → refuse before
    // anything runs. This alone rejects a bare `--__turn-executor` invocation.
    let env_token = match std::env::var(ENV_TOKEN) {
        Ok(t) if !t.is_empty() => t,
        _ => {
            eprintln!("[bwoc-harness:executor] refused: missing capability token");
            return 2;
        }
    };
    let fd: std::os::unix::io::RawFd = match std::env::var(ENV_FD).ok().and_then(|s| s.parse().ok())
    {
        Some(f) => f,
        None => {
            eprintln!("[bwoc-harness:executor] refused: missing IPC fd");
            return 2;
        }
    };

    // SAFETY: `fd` is the IPC socket the parent dup'd to EXECUTOR_FD and cleared
    // CLOEXEC on; we take sole ownership of it here.
    let mut sock = unsafe { UnixStream::from_raw_fd(fd) };

    let req: WireRequest = match read_frame(&mut sock)
        .and_then(|b| serde_json::from_slice(&b).map_err(std::io::Error::other))
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[bwoc-harness:executor] refused: bad request frame: {e}");
            return 3;
        }
    };

    // C2/C13: the framed token must match the env token (constant-time).
    if !ct_eq(req.token.as_bytes(), env_token.as_bytes()) {
        eprintln!("[bwoc-harness:executor] refused: capability token mismatch");
        return 4;
    }

    if req.selftest {
        // Snapshot BEFORE any runtime exists so `fds` is the inherited set only.
        let report = SelfTestReport {
            pid: std::process::id(),
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            fds: snapshot_open_fds(),
            env_keys: std::env::vars().map(|(k, _)| k).collect(),
            ipc_fd: EXECUTOR_FD,
            static_probe: FRESH_PROBE.swap(1, Ordering::SeqCst),
            rlimits: snapshot_rlimits(),
        };
        let content = serde_json::to_string(&report).unwrap_or_default();
        let _ = respond(&mut sock, content);
        return 0;
    }

    // C5 (defence in depth): even past the gate, only default-registry tools run
    // in the child — a fresh child has no MCP/dynamic/keyring tools registered.
    if !is_marshallable_tool(&req.tool_name) {
        let content = format!(
            "error: tool `{}` is not marshallable into the isolated executor; \
             denied (fail-closed) [Phase 5 t5]",
            req.tool_name
        );
        let _ = respond(&mut sock, content);
        return 0;
    }

    // Only now build a runtime — its epoll/eventfd would otherwise pollute the
    // selftest fd snapshot above.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[bwoc-harness:executor] runtime build failed: {e}");
            return 5;
        }
    };
    let content = rt.block_on(execute_one(&req));
    let _ = respond(&mut sock, content);
    0
}

/// Execute one marshallable tool inside the child.
///
/// The confinement root stays the real worktree (`req.workdir`) so
/// `resolve_path`/`confine_path` still reject escapes (C7/C12); the process cwd
/// is the unrelated per-turn tempdir set by the parent.
///
/// C1 interaction: when the executor itself is FS-jailed (`ENV_JAILED` set), the
/// `run_command` grandchild inherits that jail, so its own OS-sandbox layer is
/// redundant — and on macOS re-applying `sandbox-exec` inside an existing
/// sandbox fails with EPERM. We therefore use a no-op OS sandbox in the jailed
/// case (the arg-scan + env-scrub in `run_sandboxed` still apply), and fall back
/// to [`make_os_sandbox`] only when the executor jail was unavailable.
#[cfg(unix)]
async fn execute_one(req: &WireRequest) -> String {
    let ctx = if req.confine {
        ToolContext::new(req.workdir.clone())
    } else {
        ToolContext::unconfined(req.workdir.clone())
    };
    let os_sandbox: Box<dyn OsSandbox> = if std::env::var(ENV_JAILED).is_ok() {
        Box::new(crate::sandbox::NoopOsSandbox)
    } else {
        make_os_sandbox(&req.workdir)
    };
    run_in_process(
        &req.tool_name,
        &req.args_json,
        &ctx,
        &default_registry(),
        &*os_sandbox,
    )
    .await
}

#[cfg(unix)]
fn respond(sock: &mut std::os::unix::net::UnixStream, content: String) -> std::io::Result<()> {
    let payload = serde_json::to_vec(&WireResponse { content }).map_err(std::io::Error::other)?;
    write_frame(sock, &payload)
}

/// Every open fd in `[0, 1024)` (probed via `fcntl(F_GETFD)` — no fd of its own,
/// unlike reading `/proc/self/fd`, so the result is exact).
#[cfg(unix)]
fn snapshot_open_fds() -> Vec<i32> {
    let mut fds = Vec::new();
    for fd in 0..1024 {
        // SAFETY: F_GETFD only queries the descriptor flags; no side effects.
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1 {
            fds.push(fd);
        }
    }
    fds
}

// ---------------------------------------------------------------------------
// Framing + helpers
// ---------------------------------------------------------------------------

/// Write a length-prefixed frame: 4-byte big-endian length + payload.
#[cfg(unix)]
fn write_frame<W: Write>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| std::io::Error::other("frame too large"))?;
    w.write_all(&len.to_be_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Read one length-prefixed frame.
#[cfg(unix)]
fn read_frame<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("frame length exceeds cap"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Constant-time byte-slice equality (token comparison; C2).
#[cfg(unix)]
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 256-bit token from `/dev/urandom`, hex-encoded.
#[cfg(unix)]
fn gen_token() -> std::io::Result<String> {
    let mut buf = [0u8; 32];
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(&mut buf)?;
    let mut s = String::with_capacity(buf.len() * 2);
    for b in buf {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    Ok(s)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn marshallable_accepts_default_tools_rejects_mcp() {
        assert!(is_marshallable_tool("write_file"));
        assert!(is_marshallable_tool("run_command"));
        assert!(is_marshallable_tool("memory_write"));
        // Dynamic / MCP / unknown tools are not marshallable.
        assert!(!is_marshallable_tool("mcp__server__do_thing"));
        assert!(!is_marshallable_tool("totally_unknown_tool"));
    }

    #[test]
    fn ct_eq_basic() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn gen_token_is_64_hex_chars() {
        let t = gen_token().unwrap();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        // Two draws must differ (256-bit entropy).
        assert_ne!(t, gen_token().unwrap());
    }

    #[test]
    fn frame_roundtrip() {
        let mut buf: Vec<u8> = Vec::new();
        write_frame(&mut buf, b"hello frame").unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let got = read_frame(&mut cursor).unwrap();
        assert_eq!(got, b"hello frame");
    }
}
