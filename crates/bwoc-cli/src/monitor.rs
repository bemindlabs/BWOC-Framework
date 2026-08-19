//! `bwoc monitor` — the Loop-Engineering L3 monitoring/alerting flagship.
//!
//! Watch a source on a cadence and alert **once per transition**, not every
//! poll. The "source" is an operator-supplied shell command (`--exec`): exit 0
//! is OK, any non-zero exit (or a spawn failure / signal death) is a TRIP.
//! Alerts go to a fleet member via `bwoc send` (`--alert`).
//!
//! ## Zero trust-model change (why this is the safe first L3 loop)
//!
//! The probe runs in **trusted wrapper code**, and **only trusted scalars — the
//! monitor id and the exit code — are delivered into a model's inbox** via
//! `bwoc send`. The command's stderr is captured for the operator's *local*
//! monitor log only; it is deliberately **never** put into the `bwoc send` body,
//! because that body lands in a fleet agent's inbox and is read by a model on
//! its next turn — forwarding untrusted, possibly service-derived probe output
//! there would be an indirect prompt-injection channel (the very thing #271
//! governs). By carrying only the scalar, the alert can't launder untrusted
//! content into a model context, so #271 stays un-engaged. No middle trust tier,
//! no dispatcher, no harness turn.
//!
//! ## Once-per-transition
//!
//! State transitions are latched through [`bwoc_core::idempotency::IdempotencyLedger`]
//! (durable, per-monitor sidecar under `.bwoc/monitors/`), so:
//!   - OK→TRIP fires one alert, then stays quiet while still tripped;
//!   - TRIP→OK fires a recovery alert (only after a real trip);
//!   - a restart mid-trip does not re-alert **as long as the last ledger write
//!     succeeded**. The write is best-effort (a failure is logged, not fatal), so
//!     a restart *after* a failed write can re-alert — a duplicate, never a
//!     missed edge (within a run, transitions are driven by in-process state, so
//!     a write failure can neither drop nor storm alerts).
//!
//! This runs off the daemon accept-thread — it is its own foreground process (or
//! external-cron one-shot) — so the single-threaded serve loop is a non-issue.

use std::path::{Path, PathBuf};
use std::process::Command;

use bwoc_core::idempotency::IdempotencyLedger;
use bwoc_core::loop_control::{Budget, Ticker};

/// Runtime args for `bwoc monitor`.
pub struct MonitorArgs {
    /// The shell command to probe. Exit 0 = OK, non-zero = TRIP.
    pub exec: String,
    /// Fleet member to `bwoc send` on a transition. `None` → log only.
    pub alert: Option<String>,
    /// Loop continuously (`--loop`); otherwise probe once and exit.
    pub loop_mode: bool,
    /// Cadence between probes in loop mode (seconds, floored at 1s).
    pub interval_secs: u64,
    /// Iteration budget in loop mode. `0` = unbounded (a monitor is a service —
    /// its stop is the operator / supervisor, like the daemon).
    pub max_iters: usize,
    /// Stable monitor id (ledger key + sidecar filename). `None` → derived from
    /// the exec command so the same monitor keeps its state across restarts.
    pub id: Option<String>,
    /// Workspace override — standard resolution when `None`.
    pub workspace: Option<PathBuf>,
}

/// The transition an observation produces, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alert {
    /// OK/unknown → TRIP.
    Trip,
    /// TRIP → OK (a real recovery, never on first-seen OK).
    Recover,
}

/// Probe state as a stable string (the ledger value).
const OK: &str = "ok";
const TRIP: &str = "trip";

/// Decide the alert for a `prev`→`state` transition. Pure — the transition logic
/// is unit-testable without probing or a ledger.
///
/// - to TRIP from anything-but-trip → [`Alert::Trip`]
/// - TRIP → OK → [`Alert::Recover`]
/// - first-seen OK, or OK→OK, or TRIP→TRIP → no alert (no spam, no false recovery)
fn decide_alert(prev: Option<&str>, state: &str) -> Option<Alert> {
    match (prev, state) {
        (p, TRIP) if p != Some(TRIP) => Some(Alert::Trip),
        (Some(TRIP), OK) => Some(Alert::Recover),
        _ => None,
    }
}

/// The delivered alert body — **trusted scalars only** (monitor id + exit code).
/// This is the sole thing sent via `bwoc send` into a model-read inbox, so it
/// must never interpolate untrusted probe output (kept as its own fn so a
/// regression that re-adds `detail` is caught by [`alert_body_is_scalar_only`]).
fn alert_body(alert: Alert, id: &str, code: i32) -> String {
    match alert {
        Alert::Trip => format!("🔴 monitor '{id}' TRIPPED (exit {code})"),
        Alert::Recover => format!("🟢 monitor '{id}' RECOVERED (exit {code})"),
    }
}

/// A stable, filesystem-safe monitor id: the operator's `--id` if safe, else
/// `mon-<hex>` derived from a **toolchain-stable** hash of the exec command, so
/// the same command keeps the same ledger file across restarts *and across
/// binary/Rust upgrades* (`DefaultHasher`'s algorithm is not guaranteed stable
/// between versions, which would silently orphan a monitor's state on upgrade).
fn monitor_id(explicit: Option<&str>, exec: &str) -> String {
    if let Some(id) = explicit
        && is_safe_id(id)
    {
        return id.to_string();
    }
    format!("mon-{:016x}", fnv1a(exec.as_bytes()))
}

/// FNV-1a 64-bit — a small, fixed, version-stable hash for the persisted monitor
/// id (unlike `std`'s `DefaultHasher`, whose algorithm may change across
/// releases). Not cryptographic; only needs to be stable + well-distributed for
/// a filename.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A non-empty single path segment safe as a filename **on every platform** — the
/// id becomes `.bwoc/monitors/<id>.jsonl`, and a workspace may be shared across
/// OSes. Rejects separators, `.`/`..`, a leading dash, a trailing `.`/space
/// (invalid on Windows), and the Windows reserved device names (`CON`, `NUL`,
/// `COM1`…) so a monitor can't fail to persist its state on a Windows checkout.
fn is_safe_id(id: &str) -> bool {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.starts_with('-')
        || id.ends_with('.')
        || id.ends_with(' ')
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return false;
    }
    // Windows reserved device names, case-insensitive, with or without an
    // extension (`CON`, `con.txt`, `COM1` …).
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    !RESERVED.contains(&stem.as_str())
}

/// Entry point. Returns a process exit code (one-shot: 0 OK / 1 TRIP; loop: 0 on
/// budget-reached, 1 on a hard error).
pub fn run(args: MonitorArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace.clone()) else {
        eprintln!(
            "bwoc monitor: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    if args.exec.trim().is_empty() {
        eprintln!("bwoc monitor: --exec is required and must be a non-empty command.");
        return 2;
    }
    let id = monitor_id(args.id.as_deref(), &args.exec);
    let ledger = IdempotencyLedger::new(
        workspace
            .join(".bwoc")
            .join("monitors")
            .join(format!("{id}.jsonl")),
    );
    let bwoc = args
        .alert
        .as_ref()
        .and(bwoc_core::exec::sibling_binary("bwoc"));
    if args.alert.is_some() && bwoc.is_none() {
        eprintln!(
            "bwoc monitor: --alert set but `bwoc` not found (sibling / CARGO_BIN_EXE / PATH); \
             transitions will be logged, not sent."
        );
    }

    // Seed the last-known state from the durable ledger (restart survival). A
    // hard read error is distinguished from first-seen: we log it and treat the
    // state as unknown rather than fabricating a `None` that could spuriously
    // alert (finding-5). `None` = genuinely first-seen.
    let mut last = match ledger.get("state") {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "bwoc monitor '{id}': ledger read failed ({e}); starting from unknown state."
            );
            None
        }
    };

    if !args.loop_mode {
        let (code, next) = tick(
            &ledger,
            &id,
            &args,
            &workspace,
            bwoc.as_deref(),
            last.as_deref(),
        );
        let _ = next;
        return i32::from(code != 0);
    }

    let ticker = Ticker::every_secs(args.interval_secs);
    let budget = Budget::new(args.max_iters);
    eprintln!(
        "bwoc monitor '{id}': every {}s, budget {} — exec `{}`",
        ticker.interval().as_secs(),
        budget.describe(),
        args.exec
    );
    let mut iteration = 0usize;
    loop {
        iteration += 1;
        // Transition detection is driven by the **in-process** `last` state
        // (updated every tick regardless of whether the durable write below
        // succeeded), so a transient ledger-write failure can neither drop an
        // edge nor storm re-alerts (findings 3 & 7); the ledger is a best-effort
        // durability cache for the next restart only.
        let (_, next) = tick(
            &ledger,
            &id,
            &args,
            &workspace,
            bwoc.as_deref(),
            last.as_deref(),
        );
        last = Some(next);
        if budget.exhausted(iteration) {
            eprintln!("bwoc monitor '{id}': hit its {iteration}-iteration budget — stopping.");
            return 0;
        }
        std::thread::sleep(ticker.interval());
    }
}

/// One probe → decide → (maybe) alert → best-effort durable write. `prev` is the
/// last known state (in-memory for `--loop`, ledger-seeded for one-shot).
/// Returns `(exit_code, new_state)`. Ledger/alert failures are logged, never
/// fatal. The delivered alert body carries **only trusted scalars** (id + exit
/// code) — the probe's stderr goes to the local log, never into `bwoc send`.
fn tick(
    ledger: &IdempotencyLedger,
    id: &str,
    args: &MonitorArgs,
    workspace: &Path,
    bwoc: Option<&Path>,
    prev: Option<&str>,
) -> (i32, String) {
    let (code, detail) = probe(&args.exec);
    let state = if code == 0 { OK } else { TRIP };

    if let Some(alert) = decide_alert(prev, state) {
        // Alert BODY = trusted scalars only (see `alert_body`). `detail`
        // (untrusted probe stderr) is logged locally for the operator but never
        // delivered via `bwoc send` — it would otherwise land in a model's inbox
        // (indirect injection, #271).
        let msg = alert_body(alert, id, code);
        if detail.is_empty() {
            eprintln!("bwoc monitor: {msg}");
        } else {
            eprintln!("bwoc monitor: {msg}  [local stderr: {detail}]");
        }
        if let (Some(agent), Some(bwoc)) = (args.alert.as_deref(), bwoc) {
            send_alert(bwoc, agent, &msg, workspace, id);
        }
    } else {
        eprintln!("bwoc monitor '{id}': {state} (exit {code})");
    }

    // Best-effort durable write for the NEXT restart; transition detection above
    // does not depend on it, so a failure is logged, not fatal.
    if let Err(e) = ledger.latch("state", state) {
        eprintln!("bwoc monitor '{id}': ledger write failed (state not persisted): {e}");
    }

    (code, state.to_string())
}

/// Run the operator's probe command through the platform shell (so pipes /
/// redirects work), capturing the exit code + a short tail of stderr for the
/// **local** monitor log (never the delivered alert body). A spawn failure or
/// signal death counts as a TRIP (code != 0).
fn probe(exec: &str) -> (i32, String) {
    let output = shell_command(exec).output();
    match output {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let tail: String = stderr
                .trim()
                .lines()
                .last()
                .unwrap_or("")
                .chars()
                .take(200)
                .collect();
            (code, tail)
        }
        Err(e) => (-1, format!("could not run command: {e}")),
    }
}

#[cfg(unix)]
fn shell_command(exec: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(exec);
    c
}

#[cfg(windows)]
fn shell_command(exec: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(exec);
    c
}

/// Shell out `bwoc send --workspace <ws> -- <agent> <msg>` (options first, then
/// the `--` end-of-options marker so `<agent>`/`<msg>` are always positionals,
/// even with a leading dash). Best-effort — a send failure is logged, never
/// fatal to the monitor.
fn send_alert(bwoc: &Path, agent: &str, msg: &str, workspace: &Path, id: &str) {
    let status = Command::new(bwoc)
        .arg("send")
        .arg("--workspace")
        .arg(workspace)
        .arg("--")
        .arg(agent)
        .arg(msg)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => eprintln!("bwoc monitor '{id}': alert send to '{agent}' exited {s}"),
        Err(e) => eprintln!("bwoc monitor '{id}': alert send to '{agent}' failed: {e}"),
    }
}

/// Standard workspace resolution: explicit > `BWOC_WORKSPACE` > ancestor walk.
fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE")
        && !env_path.is_empty()
    {
        return Some(PathBuf::from(env_path));
    }
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_alert_transitions() {
        // First-seen states: trip alerts, ok does not (no false recovery).
        assert_eq!(decide_alert(None, TRIP), Some(Alert::Trip));
        assert_eq!(decide_alert(None, OK), None);
        // Steady states: no repeat alerts.
        assert_eq!(decide_alert(Some(OK), OK), None);
        assert_eq!(decide_alert(Some(TRIP), TRIP), None);
        // Real transitions.
        assert_eq!(decide_alert(Some(OK), TRIP), Some(Alert::Trip));
        assert_eq!(decide_alert(Some(TRIP), OK), Some(Alert::Recover));
    }

    #[test]
    fn alert_body_is_scalar_only() {
        // Pin the EXACT delivered body: only the id + exit code, no probe output.
        // If a change re-interpolated untrusted stderr, these literals would break
        // (the #271 regression guard).
        assert_eq!(
            alert_body(Alert::Trip, "db", 7),
            "🔴 monitor 'db' TRIPPED (exit 7)"
        );
        assert_eq!(
            alert_body(Alert::Recover, "db", 0),
            "🟢 monitor 'db' RECOVERED (exit 0)"
        );
    }

    #[test]
    fn fnv1a_is_stable_and_distinct() {
        // Pin known vectors to EXACT values, so a change to the hash algorithm —
        // which would orphan every derived-id monitor's ledger across upgrades —
        // trips this test (self-comparison alone would not catch it).
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325); // offset basis
        assert_eq!(fnv1a(b"bwoc"), 0x11d8_3d9b_ed64_ec28);
        assert_eq!(fnv1a(b"curl -sf https://x"), 0xfe8a_a4eb_c9b2_a20c);
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
    }

    #[test]
    fn is_safe_id_guards_the_filename() {
        assert!(is_safe_id("db-health"));
        assert!(is_safe_id("mon-01ab"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id(".."));
        assert!(!is_safe_id("-x"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id("a b"));
        // Windows-invalid: reserved device names (any case, with/without ext) +
        // trailing dot/space.
        assert!(!is_safe_id("CON"));
        assert!(!is_safe_id("nul"));
        assert!(!is_safe_id("Com1"));
        assert!(!is_safe_id("con.txt"));
        assert!(!is_safe_id("lpt9"));
        assert!(!is_safe_id("db."));
        // Not reserved: a device name as a substring is fine.
        assert!(is_safe_id("console"));
        assert!(is_safe_id("com10"));
    }

    #[test]
    fn monitor_id_is_stable_and_safe() {
        // Same command → same derived id (stable across restarts for the ledger).
        let a = monitor_id(None, "curl -sf https://x");
        let b = monitor_id(None, "curl -sf https://x");
        assert_eq!(a, b);
        assert!(is_safe_id(&a), "derived id must be filename-safe: {a}");
        // Different command → different id.
        assert_ne!(a, monitor_id(None, "curl -sf https://y"));
        // A safe explicit id wins; an unsafe one falls back to the derived hash.
        assert_eq!(monitor_id(Some("db"), "cmd"), "db");
        assert!(monitor_id(Some("../evil"), "cmd").starts_with("mon-"));
    }

    #[cfg(unix)]
    #[test]
    fn probe_reads_exit_code() {
        assert_eq!(probe("exit 0").0, 0);
        assert_eq!(probe("exit 3").0, 3);
        // stderr tail is captured.
        let (code, tail) = probe("echo boom >&2; exit 1");
        assert_eq!(code, 1);
        assert!(tail.contains("boom"), "stderr tail: {tail}");
    }

    #[cfg(windows)]
    #[test]
    fn probe_reads_exit_code() {
        // Same contract through `cmd /C` on the Windows CI lane.
        assert_eq!(probe("exit 0").0, 0);
        assert_eq!(probe("exit 3").0, 3);
        let (code, tail) = probe("echo boom 1>&2 & exit 1");
        assert_eq!(code, 1);
        assert!(tail.contains("boom"), "stderr tail: {tail}");
    }
}
