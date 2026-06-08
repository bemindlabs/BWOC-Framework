//! Phase 5 t6 — resource-limit integration tests.
//!
//! These drive the **real** `bwoc-harness` binary in its hidden
//! `--__turn-executor` mode and verify the `setrlimit` plan installed in the
//! production `pre_exec` path (C3 / t6).
//!
//! Two kinds of test live here:
//!
//! - **Always-on snapshot gate** ([`snapshot_limits_on_production_preexec_path`]):
//!   reads back, via `getrlimit` inside the child, the limits actually in force
//!   and asserts (a) every intended limit is present, (b) each finite value ==
//!   the parent's intended value, (c) NONE is `RLIM_INFINITY`, (d) `soft <= hard`.
//!   `RLIMIT_NPROC` is RELATIVE (per-UID usage + headroom), so it is range-checked
//!   instead of `==` (C2). This test is the gate proof and runs on every CI pass.
//!
//! - **Default-OFF resource bombs** (`bomb_*`): fork / CPU / file-size DoS attempts
//!   that should be contained by the limits. They are **destructive by intent** and
//!   only run when `BWOC_T6_RUN_BOMBS=1`. Each bomb is deliberately *bounded* (a
//!   fixed iteration/byte/process budget) so it can never hang or runaway even if a
//!   limit fails to bite. The memory bomb (`RLIMIT_AS`) is Linux-only: macOS
//!   `setrlimit` rejects `RLIMIT_AS` (EINVAL), so address-space capping — and its
//!   bomb — exist only on the Linux production host (see the C4 notice in
//!   `src/turn_executor.rs`).
//!
//! Run the bombs (serially, to avoid env-override cross-talk):
//!   `BWOC_T6_RUN_BOMBS=1 cargo test -p bwoc-harness --test resource_limits \
//!        bomb_ -- --test-threads=1 --nocapture`

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bwoc_harness::turn_executor::{
    RLIM_INFINITY_U64, SelfTestReport, ToolInvocation, intended_rlimits, run_isolated,
    run_isolated_selftest,
};

/// Serializes tests in THIS binary: the bombs mutate process-global
/// `BWOC_TURN_RLIMIT_*` env vars around a spawn, which would otherwise race the
/// snapshot test (which reads the same env). Poison is ignored — a panicking
/// test must not wedge the rest.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}

fn harness_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_bwoc-harness"))
}

fn parse_report(content: &str) -> SelfTestReport {
    serde_json::from_str(content).expect("selftest report should be valid JSON")
}

fn inv(tool_name: &str, args_json: &str, workdir: &Path, confine: bool) -> ToolInvocation {
    ToolInvocation {
        tool_name: tool_name.to_string(),
        args_json: args_json.to_string(),
        workdir: workdir.to_path_buf(),
        confine,
    }
}

fn bombs_enabled() -> bool {
    std::env::var("BWOC_T6_RUN_BOMBS").as_deref() == Ok("1")
}

// ── Always-on snapshot gate (condition 5) ───────────────────────────────────
#[test]
fn snapshot_limits_on_production_preexec_path() {
    let _g = guard();
    let work = tempfile::tempdir().unwrap();
    let out = run_isolated_selftest(&harness_bin(), work.path()).unwrap();
    let report = parse_report(&out.content);

    let intended = intended_rlimits();
    assert!(
        !intended.is_empty(),
        "the platform must intend at least one resource limit"
    );

    for want in &intended {
        // (a) every intended limit is present in the child's live snapshot.
        let got = report
            .rlimits
            .iter()
            .find(|r| r.name == want.name)
            .unwrap_or_else(|| {
                panic!(
                    "(a) limit {} missing from child snapshot: {:?}",
                    want.name, report.rlimits
                )
            });

        // (c) NONE is RLIM_INFINITY — the limit is genuinely finite.
        assert_ne!(
            got.soft, RLIM_INFINITY_U64,
            "(c) {} soft is RLIM_INFINITY (unlimited)",
            want.name
        );
        assert_ne!(
            got.hard, RLIM_INFINITY_U64,
            "(c) {} hard is RLIM_INFINITY (unlimited)",
            want.name
        );

        // (d) soft <= hard.
        assert!(
            got.soft <= got.hard,
            "(d) {} soft ({}) > hard ({})",
            want.name,
            got.soft,
            got.hard
        );

        if want.relative {
            // NPROC: per-UID RELATIVE — exact value depends on live process count
            // at spawn, so range-check plausibility instead of `==`.
            assert!(
                got.soft >= 1 && got.soft < 1_000_000,
                "{} relative soft implausible (not a usage+headroom value): {}",
                want.name,
                got.soft
            );
            assert!(got.hard >= got.soft, "{} relative hard < soft", want.name);
        } else {
            // (b) finite value == the parent's intended value, on the real path.
            assert_eq!(
                got.soft, want.soft,
                "(b) {} soft mismatch (intended {}, applied {})",
                want.name, want.soft, got.soft
            );
            assert_eq!(
                got.hard, want.hard,
                "(b) {} hard mismatch (intended {}, applied {})",
                want.name, want.hard, got.hard
            );
        }
    }
}

// ── Bomb: fork (NPROC, best-effort per-UID containment — Linux only) ─────────
#[test]
fn bomb_fork_contained_by_nproc() {
    if !bombs_enabled() {
        eprintln!("[t6] bomb_fork skipped (set BWOC_T6_RUN_BOMBS=1 to run)");
        return;
    }
    if !cfg!(target_os = "linux") {
        // macOS accepts setrlimit(RLIMIT_NPROC) but does not reliably enforce a
        // usage-relative cap (verified: 40 forks past the live count with zero
        // EAGAIN). NPROC fork containment is a Linux property; on Linux the
        // kernel's per-UID counter and /proc enumeration agree. See the C4 notice.
        eprintln!(
            "[t6] bomb_fork skipped: RLIMIT_NPROC is not reliably enforced on macOS; \
             fork containment is exercised on the Linux production host"
        );
        return;
    }
    let _g = guard();
    let work = tempfile::tempdir().unwrap();
    let markers = work.path().join("m");
    std::fs::create_dir_all(&markers).unwrap();
    let md = markers.to_str().unwrap();

    // Tight headroom so the per-UID NPROC cap bites quickly. BOUNDED: 60 bg
    // subshells, each touches a marker then sleeps 3s; capped forks ⇒ < 60 markers.
    // SAFETY: env mutation is serialized by ENV_GUARD; removed before unlock.
    unsafe {
        std::env::set_var("BWOC_TURN_RLIMIT_NPROC_HEADROOM", "8");
    }
    let script = format!(
        "for i in $(seq 1 60); do ( : > {md}/p.$i; sleep 3 ) & done; sleep 1; ls {md} | wc -l"
    );
    let args = serde_json::json!({ "command": script }).to_string();
    let out = run_isolated(
        &harness_bin(),
        &inv("run_command", &args, work.path(), true),
    )
    .unwrap();
    unsafe {
        std::env::remove_var("BWOC_TURN_RLIMIT_NPROC_HEADROOM");
    }

    let created = std::fs::read_dir(&markers).map(|d| d.count()).unwrap_or(0);
    eprintln!(
        "[t6] bomb_fork: {created}/60 subshells forked under NPROC cap; out={:?}",
        out.content.trim()
    );
    assert!(
        created < 60,
        "NPROC did not contain the fork bomb: all 60 subshells forked"
    );
}

// ── Bomb: memory (RLIMIT_AS — Linux only) ────────────────────────────────────
#[test]
fn bomb_mem_contained_by_as() {
    if !bombs_enabled() {
        eprintln!("[t6] bomb_mem skipped (set BWOC_T6_RUN_BOMBS=1 to run)");
        return;
    }
    if !cfg!(target_os = "linux") {
        eprintln!(
            "[t6] bomb_mem skipped: RLIMIT_AS is Linux-only (macOS setrlimit AS => EINVAL); \
             memory capping is enforced on the Linux production host"
        );
        return;
    }
    let _g = guard();
    let work = tempfile::tempdir().unwrap();
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("[t6] bomb_mem skipped: python3 unavailable to drive the allocation");
        return;
    }

    // SAFETY: serialized by ENV_GUARD; removed before unlock.
    unsafe {
        std::env::set_var("BWOC_TURN_RLIMIT_AS_MIB", "256");
    }
    // BOUNDED single 400 MiB allocation > 256 MiB AS cap ⇒ MemoryError / kill.
    let script =
        "python3 -c 'a=bytearray(400*1024*1024); print(\"ALLOC_OK\", len(a))' 2>&1; echo rc=$?";
    let args = serde_json::json!({ "command": script }).to_string();
    let out = run_isolated(
        &harness_bin(),
        &inv("run_command", &args, work.path(), true),
    )
    .unwrap();
    unsafe {
        std::env::remove_var("BWOC_TURN_RLIMIT_AS_MIB");
    }

    eprintln!("[t6] bomb_mem: out={:?}", out.content.trim());
    assert!(
        !out.content.contains("ALLOC_OK"),
        "RLIMIT_AS did not contain the 400 MiB allocation: {}",
        out.content
    );
}

// ── Bomb: CPU (RLIMIT_CPU) ───────────────────────────────────────────────────
#[test]
fn bomb_cpu_contained_by_cpu_limit() {
    if !bombs_enabled() {
        eprintln!("[t6] bomb_cpu skipped (set BWOC_T6_RUN_BOMBS=1 to run)");
        return;
    }
    let _g = guard();
    let work = tempfile::tempdir().unwrap();

    // SAFETY: serialized by ENV_GUARD; removed before unlock.
    unsafe {
        std::env::set_var("BWOC_TURN_RLIMIT_CPU_SECS", "1");
    }
    // 1s CPU soft ⇒ SIGXCPU (128+24 = 152). BOUNDED busy loop: even if the limit
    // failed to bite, the awk loop terminates after a finite count (cannot hang).
    let script = "awk 'BEGIN{x=0; for(i=0;i<500000000;i++){x+=i}; print x}' 2>&1; echo rc=$?";
    let args = serde_json::json!({ "command": script }).to_string();
    let started = std::time::Instant::now();
    let out = run_isolated(
        &harness_bin(),
        &inv("run_command", &args, work.path(), true),
    )
    .unwrap();
    let elapsed = started.elapsed();
    unsafe {
        std::env::remove_var("BWOC_TURN_RLIMIT_CPU_SECS");
    }

    eprintln!(
        "[t6] bomb_cpu: elapsed={:?} out={:?}",
        elapsed,
        out.content.trim()
    );
    // SIGXCPU is signal 24 ⇒ shell reports 152 for the killed awk; the trailing
    // `echo rc=$?` then prints it. A surviving (rc=0) awk means the limit failed.
    assert!(
        out.content.contains("rc=152"),
        "RLIMIT_CPU did not SIGXCPU the busy loop (expected rc=152): {}",
        out.content
    );
}

// ── Bomb: file size (RLIMIT_FSIZE — closes the disk-fill DoS) ────────────────
#[test]
fn bomb_fsize_contained() {
    if !bombs_enabled() {
        eprintln!("[t6] bomb_fsize skipped (set BWOC_T6_RUN_BOMBS=1 to run)");
        return;
    }
    let _g = guard();
    let work = tempfile::tempdir().unwrap();

    // SAFETY: serialized by ENV_GUARD; removed before unlock.
    unsafe {
        std::env::set_var("BWOC_TURN_RLIMIT_FSIZE_MIB", "16");
    }
    // Try to write 64 MiB > 16 MiB FSIZE cap ⇒ SIGXFSZ; the file is capped near
    // 16 MiB. BOUNDED: dd stops at count=64 even if the limit failed.
    let script = "dd if=/dev/zero of=big.bin bs=1048576 count=64 2>&1; echo rc=$?";
    let args = serde_json::json!({ "command": script }).to_string();
    let out = run_isolated(
        &harness_bin(),
        &inv("run_command", &args, work.path(), true),
    )
    .unwrap();
    unsafe {
        std::env::remove_var("BWOC_TURN_RLIMIT_FSIZE_MIB");
    }

    let sz = std::fs::metadata(work.path().join("big.bin"))
        .map(|m| m.len())
        .unwrap_or(0);
    eprintln!(
        "[t6] bomb_fsize: file={} bytes ({} MiB) out={:?}",
        sz,
        sz / (1024 * 1024),
        out.content.trim()
    );
    assert!(
        sz > 0 && sz <= 20 * 1024 * 1024,
        "RLIMIT_FSIZE did not cap the write near 16 MiB: {sz} bytes"
    );
}
