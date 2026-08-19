//! `bwoc digest` — a recurring digest loop (Loop-Engineering L3).
//!
//! Run an operator command **once per period** — durably, so a restart or the
//! next poll tick doesn't re-run it — and deliver its rendered output. The
//! once-per-period gate is the value a bare cron can't give: a restart mid-period
//! would re-run the job, and a poll-driven loop re-fires every tick. The period
//! is latched through [`bwoc_core::idempotency::IdempotencyLedger::seen_or_record`]
//! (the durable dedup half of the L3 primitive), so a single digest loop owning
//! its ledger delivers each bucket once.
//!
//! The latch is a durable check-then-write, **not** a cross-process mutex: two
//! `bwoc digest` processes racing on the *same* ledger can both observe a bucket
//! as unseen and both deliver. True concurrency on one ledger is out of scope by
//! the same rule as the primitive (a loop owns its ledger) — run one loop per
//! ledger (a distinct `--id`) rather than overlapping invocations; an advisory
//! lock is a later hardening if that need ever appears.
//!
//! ## Delivery + trust (why v1 is stdout/file only)
//!
//! A digest **delivers content** (the command's stdout), unlike `bwoc monitor`
//! which delivers only a scalar. Forwarding arbitrary command output into a fleet
//! agent's inbox would put it in front of a model on its next turn — an indirect
//! prompt-injection channel if the aggregation touches untrusted data (the exact
//! trust defect the monitor review caught). So v1 delivers to **stdout** (or
//! `--out <file>`): the operator collects it, no model is involved, and the trust
//! posture is unchanged. Delivery into an agent's inbox (`--to`) is deferred with
//! the same trust design as the middle tier (a down-trusted machine identity).
//!
//! Its cadence is the `Every` ticker; the *period* is an epoch bucket
//! (`floor(now / period_secs)`), so no calendar/cron parser is needed (that stays
//! deferred until a loop genuinely needs calendar alignment).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use bwoc_core::idempotency::IdempotencyLedger;
use bwoc_core::loop_control::{Budget, Ticker};

/// How often the digest should fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Hourly,
    Daily,
    Weekly,
}

impl Period {
    fn seconds(self) -> u64 {
        match self {
            Period::Hourly => 3600,
            Period::Daily => 86_400,
            Period::Weekly => 604_800,
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Period::Hourly => "h",
            Period::Daily => "d",
            Period::Weekly => "w",
        }
    }
    /// Parse `--period`. Unknown → `None` (the caller reports the valid set).
    pub fn parse(s: &str) -> Option<Period> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hourly" | "hour" | "h" => Some(Period::Hourly),
            "daily" | "day" | "d" => Some(Period::Daily),
            "weekly" | "week" | "w" => Some(Period::Weekly),
            _ => None,
        }
    }
}

/// Runtime args for `bwoc digest`.
pub struct DigestArgs {
    /// The command whose stdout is the digest content (operator-defined).
    pub exec: String,
    /// Delivery period.
    pub period: Period,
    /// Write the digest to this file instead of stdout. Either way it is a local
    /// sink — never a model inbox in v1.
    pub out: Option<PathBuf>,
    /// Run continuously; without it, deliver at most once (for the current
    /// period) and exit — the cron-driven mode.
    pub loop_mode: bool,
    /// Poll cadence in `--loop` mode (seconds, floored 1s) — how often to check
    /// whether a new period has begun.
    pub interval_secs: u64,
    /// Iteration budget in `--loop` mode (`0` = unbounded service).
    pub max_iters: usize,
    /// Stable digest id (ledger + sidecar). `None` → derived from the exec + period.
    pub id: Option<String>,
    /// Workspace override.
    pub workspace: Option<PathBuf>,
}

/// The dedup key for the period containing `now_secs`: `<tag><bucket>` where the
/// bucket is `floor(now / period_secs)`. Pure — no wall-clock, so it is testable.
fn period_bucket(period: Period, now_secs: u64) -> String {
    format!("{}{}", period.tag(), now_secs / period.seconds())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Entry point. Returns a process exit code.
pub fn run(args: DigestArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace.clone()) else {
        eprintln!(
            "bwoc digest: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    if args.exec.trim().is_empty() {
        eprintln!("bwoc digest: --exec is required and must be a non-empty command.");
        return 2;
    }
    let id = digest_id(args.id.as_deref(), &args.exec, args.period);
    let ledger = IdempotencyLedger::new(
        workspace
            .join(".bwoc")
            .join("digests")
            .join(format!("{id}.jsonl")),
    );

    if !args.loop_mode {
        return tick(&ledger, &id, &args);
    }

    let ticker = Ticker::every_secs(args.interval_secs);
    let budget = Budget::new(args.max_iters);
    eprintln!(
        "bwoc digest '{id}': {:?}, checking every {}s, budget {} — exec `{}`",
        args.period,
        ticker.interval().as_secs(),
        budget.describe(),
        args.exec
    );
    let mut iteration = 0usize;
    loop {
        iteration += 1;
        let _ = tick(&ledger, &id, &args);
        if budget.exhausted(iteration) {
            eprintln!("bwoc digest '{id}': hit its {iteration}-iteration budget — stopping.");
            return 0;
        }
        std::thread::sleep(ticker.interval());
    }
}

/// One check: if this period hasn't been delivered yet, run the command and emit
/// its output to the sink, then record the period. Returns 0 when a digest was
/// delivered (or nothing to do), 2 on a command failure.
fn tick(ledger: &IdempotencyLedger, id: &str, args: &DigestArgs) -> i32 {
    let bucket = period_bucket(args.period, now_secs());
    // seen_or_record is the durable once-per-bucket gate for this loop's own
    // ledger: only the FIRST call for a bucket returns `false` (deliver);
    // everything after returns `true` (skip). It is not a cross-process mutex —
    // see the module doc for the single-owner scope.
    match ledger.seen_or_record(&bucket) {
        Ok(true) => 0, // already delivered this period
        Err(e) => {
            eprintln!(
                "bwoc digest '{id}': ledger error ({e}); skipping this period to stay at-most-once."
            );
            0
        }
        Ok(false) => {
            let (code, output) = render(&args.exec);
            if code != 0 {
                eprintln!("bwoc digest '{id}': exec exited {code} — delivering its output anyway.");
            }
            deliver(id, &bucket, &output, args.out.as_deref());
            i32::from(code != 0) * 2
        }
    }
}

/// Run the operator's command and capture stdout+stderr as the digest body.
fn render(exec: &str) -> (i32, String) {
    match shell_command(exec).output() {
        Ok(o) => {
            let code = o.status.code().unwrap_or(-1);
            let mut body = String::from_utf8_lossy(&o.stdout).into_owned();
            if !o.stderr.is_empty() {
                body.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            (code, body)
        }
        Err(e) => (-1, format!("bwoc digest: could not run command: {e}\n")),
    }
}

/// Deliver to the local sink: a file (`--out`) or stdout. Never a model inbox
/// (see the module doc). File-write failures fall back to stdout so a digest is
/// never silently lost.
fn deliver(id: &str, bucket: &str, body: &str, out: Option<&Path>) {
    if let Some(path) = out {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(mut f) => {
                let _ = writeln!(f, "── digest {id} [{bucket}] ──");
                let _ = f.write_all(body.as_bytes());
                if !body.ends_with('\n') {
                    let _ = writeln!(f);
                }
                eprintln!(
                    "bwoc digest '{id}': delivered [{bucket}] → {}",
                    path.display()
                );
                return;
            }
            Err(e) => eprintln!(
                "bwoc digest '{id}': could not write {} ({e}); falling back to stdout.",
                path.display()
            ),
        }
    }
    // Match the file branch: a broken pipe (EPIPE — e.g. `bwoc digest … | head -1`)
    // must be a swallowed IO error, not a panic out of the `print!`/`println!`
    // macros (they `panic!("failed printing to stdout")` on any write error).
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "── digest {id} [{bucket}] ──");
    let _ = o.write_all(body.as_bytes());
    if !body.ends_with('\n') {
        let _ = writeln!(o);
    }
}

/// A stable, filesystem-safe digest id — the operator's `--id` if safe, else
/// `dig-<hex>` derived from a toolchain-stable hash of the exec + period.
fn digest_id(explicit: Option<&str>, exec: &str, period: Period) -> String {
    if let Some(id) = explicit
        && is_safe_id(id)
    {
        return id.to_string();
    }
    let mut seed = exec.as_bytes().to_vec();
    seed.push(b'\0');
    seed.extend_from_slice(period.tag().as_bytes());
    format!("dig-{:016x}", fnv1a(&seed))
}

// NOTE: `fnv1a` / `is_safe_id` mirror the small helpers in `monitor.rs` (both L3
// loops need a stable, filesystem-safe id). Kept duplicated while there are only
// two consumers; consolidate into a shared `loop_util` once a third appears
// (Mattaññutā — the refactor's blast radius isn't earned yet).

/// FNV-1a 64-bit — a fixed, toolchain-stable hash for the persisted id.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A non-empty, cross-platform-safe single filename segment (rejects separators,
/// `.`/`..`, leading dash, trailing dot/space, and Windows reserved device names).
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
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    !RESERVED.contains(&stem.as_str())
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
    fn period_parse_and_seconds() {
        assert_eq!(Period::parse("daily"), Some(Period::Daily));
        assert_eq!(Period::parse("H"), Some(Period::Hourly));
        assert_eq!(Period::parse("week"), Some(Period::Weekly));
        assert_eq!(Period::parse("fortnightly"), None);
        assert_eq!(Period::Daily.seconds(), 86_400);
    }

    #[test]
    fn period_bucket_rolls_over_exactly_at_the_boundary() {
        // Two instants in the same day share a bucket; crossing 86_400 rolls it.
        let day0 = period_bucket(Period::Daily, 100);
        assert_eq!(day0, period_bucket(Period::Daily, 86_399));
        assert_ne!(day0, period_bucket(Period::Daily, 86_400));
        // Hourly is finer-grained than daily for the same instant.
        assert_ne!(
            period_bucket(Period::Hourly, 7_200),
            period_bucket(Period::Hourly, 3_599)
        );
        // Tag distinguishes periods so their buckets never collide in one ledger.
        assert!(period_bucket(Period::Daily, 0).starts_with('d'));
        assert!(period_bucket(Period::Weekly, 0).starts_with('w'));
    }

    #[test]
    fn digest_id_stable_safe_and_period_scoped() {
        let a = digest_id(None, "bwoc fleet health", Period::Daily);
        assert_eq!(a, digest_id(None, "bwoc fleet health", Period::Daily));
        assert!(is_safe_id(&a));
        // Same command, different period → different id (separate ledgers).
        assert_ne!(a, digest_id(None, "bwoc fleet health", Period::Hourly));
        // Safe explicit id wins; unsafe falls back to the derived hash.
        assert_eq!(digest_id(Some("nightly"), "x", Period::Daily), "nightly");
        assert!(digest_id(Some("con"), "x", Period::Daily).starts_with("dig-"));
    }

    // Drives the REAL `tick()` gate (not the ledger primitive, which
    // `idempotency.rs` already pins) so the module's core guarantees are pinned:
    // record-before-deliver at-most-once, a burnt bucket on exec failure, and the
    // `code!=0 → 2` exit mapping. Reordering deliver-before-record, or dropping
    // the header, now fails here instead of passing green.
    #[cfg(unix)]
    #[test]
    fn tick_delivers_once_per_bucket_and_burns_on_failure() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bwoc-digest-tick-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mk = |exec: &str, out: &Path| DigestArgs {
            exec: exec.to_string(),
            // Weekly → the epoch bucket is stable for the whole test run.
            period: Period::Weekly,
            out: Some(out.to_path_buf()),
            loop_mode: false,
            interval_secs: 1,
            max_iters: 0,
            id: Some("t".to_string()),
            workspace: Some(dir.clone()),
        };

        // Success path: first tick delivers exactly one block (exit 0 → return 0)
        // and records the bucket; a second tick in the same bucket is a no-op.
        let out = dir.join("ok.out");
        let ledger = IdempotencyLedger::new(dir.join("ok.jsonl"));
        let args = mk("echo NIGHTLY-OK", &out);
        assert_eq!(tick(&ledger, "t", &args), 0);
        let after_first = std::fs::read_to_string(&out).unwrap();
        assert_eq!(after_first.matches("── digest").count(), 1);
        assert!(after_first.contains("NIGHTLY-OK"));
        assert_eq!(tick(&ledger, "t", &args), 0);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), after_first); // unchanged

        // Failure path: exec exits non-zero → tick returns 2 but still delivers,
        // and the bucket is burnt (recorded before render), so a retry is a no-op —
        // at-most-once, never a re-run.
        let out2 = dir.join("fail.out");
        let ledger2 = IdempotencyLedger::new(dir.join("fail.jsonl"));
        let fail = mk("echo PARTIAL; exit 4", &out2);
        assert_eq!(tick(&ledger2, "t", &fail), 2);
        let burnt = std::fs::read_to_string(&out2).unwrap();
        assert!(burnt.contains("PARTIAL"));
        assert_eq!(tick(&ledger2, "t", &fail), 0); // bucket consumed → no re-run
        assert_eq!(std::fs::read_to_string(&out2).unwrap(), burnt); // unchanged

        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn render_captures_output_and_code() {
        let (code, body) = render("echo hello; echo warn >&2");
        assert_eq!(code, 0);
        assert!(body.contains("hello") && body.contains("warn"));
        assert_eq!(render("exit 4").0, 4);
    }
}
