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
//!        pre_exec: dup2→fd3, close others   [t6: setrlimit HERE]
//! ```
//!
//! # ⚠️⚠️ C4 — t5 ships NO RESOURCE LIMITS (rlimits are t6) ⚠️⚠️
//!
//! The child is a **separate process** but it is **NOT resource-isolated**.
//! There is no `setrlimit` (RLIMIT_AS / RLIMIT_NPROC / RLIMIT_CPU /
//! RLIMIT_NOFILE / RLIMIT_FSIZE), no cgroup, no `nice`. A single malicious or
//! buggy tool turn can therefore still **fork-bomb**, **exhaust memory**, or
//! **spin the CPU** within that one process subtree and take the host down with
//! it. The process boundary t5 adds buys credential isolation and fd hygiene —
//! it does **not** bound resource consumption.
//!
//! GAP(t6): wire `setrlimit` (and optionally cgroup placement) into the
//! `pre_exec` block marked `// C3 / GAP(t6)` below. The block is deliberately
//! structured so t6 only adds raw-libc `setrlimit` calls there — no
//! re-architecting of the spawn path.
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

#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

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

/// The fixed fd the parent dup's the child socket onto in `pre_exec`.
#[cfg(unix)]
const EXECUTOR_FD: std::os::unix::io::RawFd = 3;

/// Hard cap on a single IPC frame (defence against a corrupt length prefix).
#[cfg(unix)]
const MAX_FRAME: usize = 64 * 1024 * 1024;

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

    let mut cmd = Command::new(program);
    cmd.arg(EXECUTOR_FLAG)
        .current_dir(&tmp_path)
        .env_clear()
        .envs(&safe_env)
        .env(ENV_FD, EXECUTOR_FD.to_string());
    if forge != ForgeMode::NoEnvToken {
        cmd.env(ENV_TOKEN, &token);
    }

    // SAFETY (C1): this closure runs in the forked child, after fork and before
    // exec. It calls ONLY raw libc, all async-signal-safe (`dup2`, `fcntl`,
    // `close`) — no allocation, no locks, no Rust std that could deadlock a
    // post-fork child. Do NOT introduce nix/rustix wrappers here.
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
            // C10: close every other inherited fd so the child holds exactly
            // {0,1,2,EXECUTOR_FD}. CLOEXEC already covers Rust-opened fds; this
            // loop is the belt-and-suspenders guarantee the fd test asserts.
            let mut fd = EXECUTOR_FD + 1;
            while fd < 1024 {
                libc::close(fd);
                fd += 1;
            }
            // C3 / GAP(t6): resource limits go HERE — add raw-libc setrlimit
            // calls (RLIMIT_AS / RLIMIT_NPROC / RLIMIT_CPU / RLIMIT_NOFILE /
            // RLIMIT_FSIZE) in t6. t5 ships none (see the module C4 notice).
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    let child_pid = child.id();
    // Parent closes its copy of the child end; only `parent_sock` remains.
    drop(child_sock);

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

    // C11: reap unconditionally — no zombie, even on the fail-closed path.
    let _ = child.wait();
    // `tmp` drops here → the per-turn cwd is removed.

    let resp_bytes = resp_result?;
    let resp: WireResponse = serde_json::from_slice(&resp_bytes).map_err(std::io::Error::other)?;
    Ok(ExecutorOutcome {
        content: resp.content,
        child_pid,
        cwd: tmp_path,
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
#[cfg(unix)]
async fn execute_one(req: &WireRequest) -> String {
    let ctx = if req.confine {
        ToolContext::new(req.workdir.clone())
    } else {
        ToolContext::unconfined(req.workdir.clone())
    };
    let os_sandbox = make_os_sandbox(&req.workdir);
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
