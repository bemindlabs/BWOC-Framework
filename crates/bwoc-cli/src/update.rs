//! `bwoc update [--check]` — release-drift detection (read-only).
//!
//! ## Design
//!
//! The binary's own release identity is embedded at compile time via
//! `option_env!("BWOC_RELEASE_CALVER")`. Released binaries set this to the
//! Git tag (e.g. `v2026.5.24-0`). Dev/source builds leave it unset.
//!
//! `--check` fetches the latest GitHub release tag and compares CalVer tuples.
//! Plain `bwoc update` detects the install method and delegates the upgrade to
//! the package manager (brew / cargo) or points a raw binary at the release
//! page — it never self-replaces the running binary (that path is deferred).
//!
//! ## Fetch strategy
//!
//! 1. Primary: `gh release view --json tagName -q .tagName`
//! 2. Fallback: `curl -s https://api.github.com/repos/bemindlabs/BWOC-Framework/releases/latest`
//!    parsed with `serde_json`.
//!
//! Both are shells-out — no HTTP client dep added. `ShellRunner` is a small
//! stdout-capture seam for offline unit-testability. It is intentionally
//! distinct from `run.rs`'s `CommandRunner` (which adds cwd / timeout / rich
//! `RunError` handling for launching agent processes) — these are different
//! needs, not one abstraction, so they are kept separate rather than merged.

use serde_json::Value;
use std::io::IsTerminal;
use std::time::{SystemTime, UNIX_EPOCH};

// ── CalVer types ──────────────────────────────────────────────────────────────

/// Parsed form of `vYYYY.M.D-<patch>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CalVer {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub patch: u32,
}

impl CalVer {
    /// Parse `vYYYY.M.D-<patch>` (leading `v` optional).
    /// Returns `None` on any parse failure.
    pub fn parse(s: &str) -> Option<Self> {
        // Strip optional leading 'v'.
        let s = s.strip_prefix('v').unwrap_or(s);
        // Format: YYYY.M.D-patch  (M and D are not zero-padded in the spec)
        let (date_part, patch_str) = s.split_once('-')?;
        let mut parts = date_part.splitn(3, '.');
        let year: u32 = parts.next()?.parse().ok()?;
        let month: u32 = parts.next()?.parse().ok()?;
        let day: u32 = parts.next()?.parse().ok()?;
        let patch: u32 = patch_str.parse().ok()?;
        Some(Self {
            year,
            month,
            day,
            patch,
        })
    }

    /// Canonical string form: `vYYYY.M.D-patch`.
    pub fn to_tag(&self) -> String {
        format!("v{}.{}.{}-{}", self.year, self.month, self.day, self.patch)
    }
}

// ── ShellRunner seam (stdout-capture; distinct from run.rs's CommandRunner) ──

/// Result of one shelled-out command.
pub struct ShellOutcome {
    pub exit_code: i32,
    pub stdout: String,
}

/// Abstraction over shell-out so tests inject a mock.
pub trait ShellRunner {
    fn run(&self, program: &str, args: &[&str]) -> ShellOutcome;
}

/// Production runner: forks the real process, captures stdout.
pub struct ProcessShellRunner;

impl ShellRunner for ProcessShellRunner {
    fn run(&self, program: &str, args: &[&str]) -> ShellOutcome {
        use std::process::{Command, Stdio};
        let result = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        match result {
            Ok(out) => ShellOutcome {
                exit_code: out.status.code().unwrap_or(1),
                stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
            },
            Err(_) => ShellOutcome {
                exit_code: 127,
                stdout: String::new(),
            },
        }
    }
}

// ── Fetch latest tag ──────────────────────────────────────────────────────────

const GITHUB_REPO: &str = "bemindlabs/BWOC-Framework";
const GITHUB_API_URL: &str =
    "https://api.github.com/repos/bemindlabs/BWOC-Framework/releases/latest";

/// Fetch the latest release tag. Tries `gh` first; falls back to `curl` + JSON.
/// Returns `None` if both fail or return empty.
pub fn fetch_latest_tag(runner: &dyn ShellRunner) -> Option<String> {
    // Primary: gh CLI
    let gh = runner.run(
        "gh",
        &[
            "release",
            "view",
            "--repo",
            GITHUB_REPO,
            "--json",
            "tagName",
            "-q",
            ".tagName",
        ],
    );
    if gh.exit_code == 0 && !gh.stdout.is_empty() {
        return Some(gh.stdout.trim().to_string());
    }

    // Fallback: curl + serde_json parse
    let curl = runner.run("curl", &["-s", GITHUB_API_URL]);
    if curl.exit_code == 0 && !curl.stdout.is_empty() {
        if let Ok(v) = serde_json::from_str::<Value>(&curl.stdout) {
            if let Some(tag) = v.get("tag_name").and_then(|t| t.as_str()) {
                let tag = tag.trim().to_string();
                if !tag.is_empty() {
                    return Some(tag);
                }
            }
        }
    }

    None
}

// ── Check result ──────────────────────────────────────────────────────────────

/// Outcome of the `--check` comparison.
#[derive(Debug, PartialEq, Eq)]
pub enum CheckResult {
    /// Binary CalVer == latest.
    UpToDate { tag: String },
    /// Binary CalVer < latest.
    UpdateAvailable { current: String, latest: String },
    /// Binary CalVer > latest (unusual; dev build tagged ahead).
    AheadOfLatest { current: String, latest: String },
    /// `BWOC_RELEASE_CALVER` not set — source/dev build.
    SourceBuild { latest: String },
    /// Latest tag could not be fetched.
    FetchFailed,
    /// Latest tag was fetched but is malformed.
    MalformedLatestTag { raw: String },
    /// The embedded CalVer is malformed (unusual; build misconfigured).
    MalformedCurrentTag { raw: String, latest: String },
}

/// Run the `--check` comparison. Returns a `CheckResult` and the integer exit
/// code: `0` = up-to-date, ahead-of-latest, or source build; `1` = update
/// available; `2` = error (fetch failed or a malformed tag).
pub fn check(runner: &dyn ShellRunner) -> (CheckResult, i32) {
    let latest_raw = match fetch_latest_tag(runner) {
        Some(t) => t,
        None => return (CheckResult::FetchFailed, 2),
    };

    let latest_ver = match CalVer::parse(&latest_raw) {
        Some(v) => v,
        None => {
            return (
                CheckResult::MalformedLatestTag {
                    raw: latest_raw.clone(),
                },
                2,
            );
        }
    };

    // The binary's embedded release tag (set at compile time by release.yml).
    match option_env!("BWOC_RELEASE_CALVER") {
        None => (
            CheckResult::SourceBuild {
                latest: latest_ver.to_tag(),
            },
            0,
        ),
        Some(current_raw) => match CalVer::parse(current_raw) {
            None => (
                CheckResult::MalformedCurrentTag {
                    raw: current_raw.to_string(),
                    latest: latest_ver.to_tag(),
                },
                2,
            ),
            Some(current_ver) => {
                use std::cmp::Ordering;
                match current_ver.cmp(&latest_ver) {
                    Ordering::Equal => (
                        CheckResult::UpToDate {
                            tag: latest_ver.to_tag(),
                        },
                        0,
                    ),
                    Ordering::Less => (
                        CheckResult::UpdateAvailable {
                            current: current_ver.to_tag(),
                            latest: latest_ver.to_tag(),
                        },
                        1,
                    ),
                    Ordering::Greater => (
                        CheckResult::AheadOfLatest {
                            current: current_ver.to_tag(),
                            latest: latest_ver.to_tag(),
                        },
                        0,
                    ),
                }
            }
        },
    }
}

// ── Public args struct ────────────────────────────────────────────────────────

pub struct UpdateArgs {
    /// When true, perform the read-only release-drift check and exit.
    pub check: bool,
    /// When applying (no `--check`), actually execute the delegated upgrade
    /// command (e.g. `brew upgrade bwoc`) instead of only printing it.
    /// Self-replacing a raw binary is never done — that path is deferred.
    pub run: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Entry point called from `main.rs` — returns process exit code.
pub fn run(args: UpdateArgs) -> i32 {
    run_with(args, &ProcessShellRunner)
}

/// Testable entry accepting a `ShellRunner` impl.
pub fn run_with(args: UpdateArgs, runner: &dyn ShellRunner) -> i32 {
    if args.check {
        let (result, code) = check(runner);
        print_check_result(&result);
        return code;
    }
    apply(args.run, runner, &detect_install_method())
}

fn print_check_result(result: &CheckResult) {
    match result {
        CheckResult::UpToDate { tag } => {
            println!("bwoc update: up to date ({tag})");
        }
        CheckResult::UpdateAvailable { current, latest } => {
            println!("bwoc update: update available: {current} → {latest}");
            println!("  Download: https://github.com/{GITHUB_REPO}/releases/tag/{latest}");
        }
        CheckResult::AheadOfLatest { current, latest } => {
            println!(
                "bwoc update: ahead of latest release (dev build: {current}, latest: {latest})"
            );
        }
        CheckResult::SourceBuild { latest } => {
            println!("bwoc update: running a source build; latest release is {latest}");
        }
        CheckResult::FetchFailed => {
            eprintln!(
                "bwoc update: could not fetch latest release tag \
                 (requires 'gh' CLI or 'curl' on PATH with network access)"
            );
        }
        CheckResult::MalformedLatestTag { raw } => {
            eprintln!("bwoc update: latest release tag '{raw}' is not valid CalVer (vYYYY.M.D-N)");
        }
        CheckResult::MalformedCurrentTag { raw, latest } => {
            eprintln!(
                "bwoc update: embedded release tag '{raw}' is not valid CalVer; \
                 latest is {latest}"
            );
        }
    }
}

// ── Install method + delegate-only apply ───────────────────────────────────────

/// How the running `bwoc` binary was installed — picks the upgrade route.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallMethod {
    /// Homebrew (Cellar prefix). Upgrade by delegating to `brew`.
    Homebrew,
    /// `cargo install` (under `~/.cargo/bin`). Upgrade by delegating to `cargo`.
    Cargo,
    /// A raw binary on `PATH`, managed by no package manager.
    Raw,
}

/// Classify an executable path into an [`InstallMethod`]. Pure (takes the path)
/// so it is unit-testable without touching the filesystem. `current_exe()`
/// resolves symlinks, so a Homebrew install surfaces its `…/Cellar/…` path.
pub fn classify_install_path(exe: &str) -> InstallMethod {
    if exe.contains("/Cellar/")
        || exe.starts_with("/opt/homebrew/")
        || exe.starts_with("/home/linuxbrew/")
    {
        InstallMethod::Homebrew
    } else if exe.contains("/.cargo/bin/") {
        InstallMethod::Cargo
    } else {
        InstallMethod::Raw
    }
}

/// Detect how the current process's binary was installed.
fn detect_install_method() -> InstallMethod {
    std::env::current_exe()
        .map(|p| classify_install_path(&p.to_string_lossy()))
        .unwrap_or(InstallMethod::Raw)
}

/// The decided apply action — pure, derived from the check result + install
/// method, so the routing is fully unit-testable.
#[derive(Debug, PartialEq, Eq)]
pub enum ApplyAction {
    /// Nothing to do — already at (or ahead of) the latest release.
    AlreadyCurrent { tag: String },
    /// Source/dev build — "upgrade" means pull + rebuild.
    SourceRebuild,
    /// Delegate to a package manager (program + args).
    Delegate { program: String, args: Vec<String> },
    /// Raw binary, update available — point at the release page (no self-swap).
    Manual,
    /// The drift check itself failed — don't upgrade blindly.
    CheckError,
}

/// Decide the apply action. Pure: no I/O, no process exec.
pub fn apply_action(result: &CheckResult, method: &InstallMethod) -> ApplyAction {
    match result {
        CheckResult::UpToDate { tag } => ApplyAction::AlreadyCurrent { tag: tag.clone() },
        CheckResult::AheadOfLatest { current, .. } => ApplyAction::AlreadyCurrent {
            tag: current.clone(),
        },
        CheckResult::SourceBuild { .. } => ApplyAction::SourceRebuild,
        CheckResult::FetchFailed
        | CheckResult::MalformedLatestTag { .. }
        | CheckResult::MalformedCurrentTag { .. } => ApplyAction::CheckError,
        CheckResult::UpdateAvailable { .. } => match method {
            InstallMethod::Homebrew => ApplyAction::Delegate {
                program: "brew".to_string(),
                args: vec!["upgrade".to_string(), "bwoc".to_string()],
            },
            InstallMethod::Cargo => ApplyAction::Delegate {
                program: "cargo".to_string(),
                args: vec![
                    "install".to_string(),
                    "--git".to_string(),
                    format!("https://github.com/{GITHUB_REPO}"),
                    "bwoc-cli".to_string(),
                ],
            },
            InstallMethod::Raw => ApplyAction::Manual,
        },
    }
}

/// Apply (delegate-only). Runs the drift check, decides the action, prints it,
/// and — only with `run` and only for a package-manager delegate — executes it.
/// A raw binary is never self-replaced here (that path is deferred for review).
fn apply(run: bool, runner: &dyn ShellRunner, method: &InstallMethod) -> i32 {
    let (result, _) = check(runner);
    match apply_action(&result, method) {
        ApplyAction::AlreadyCurrent { tag } => {
            println!("bwoc update: already up to date ({tag})");
            0
        }
        ApplyAction::SourceRebuild => {
            println!("bwoc update: running a source build — upgrade by rebuilding from latest:");
            println!("  git pull && cargo install --path crates/bwoc-cli");
            0
        }
        ApplyAction::CheckError => {
            // Surface the underlying check failure; never upgrade on uncertainty.
            print_check_result(&result);
            2
        }
        ApplyAction::Manual => {
            println!("bwoc update: a newer release is available.");
            println!("  This binary is managed by neither brew nor cargo — download the latest:");
            println!("  https://github.com/{GITHUB_REPO}/releases/latest");
            println!("  Verify the SHA-256 against the published checksum, then replace it.");
            0
        }
        ApplyAction::Delegate { program, args } => {
            let display = format!("{program} {}", args.join(" "));
            if run {
                println!("bwoc update: a newer release is available — running `{display}`");
                exec_delegated(&program, &args)
            } else {
                println!("bwoc update: a newer release is available. Upgrade with:");
                println!("  {display}");
                println!("  (or re-run `bwoc update --run` to execute it)");
                0
            }
        }
    }
}

/// Execute a delegated upgrade command with inherited stdio so the package
/// manager's progress streams straight to the terminal. Returns its exit code.
fn exec_delegated(program: &str, args: &[String]) -> i32 {
    use std::process::Command;
    match Command::new(program).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => {
            eprintln!("bwoc update: could not run '{program}' — is it installed and on PATH?");
            127
        }
    }
}

// ── Startup drift guard (issue #44) ─────────────────────────────────────────────
//
// On a normal interactive invocation, surface a one-line "update available"
// notice — reusing `fetch_latest_tag` + `CalVer` above. Three moving parts,
// each kept pure + unit-testable behind a seam:
//   • a throttle cache at `~/.bwoc/update-check.json` (network ≤ once / 24h),
//   • a detached background refresh (never blocks the command), and
//   • a guarded notice that degrades silently offline (Musāvāda — never a
//     false "up to date").
// `notify_if_drifted` is the only impure piece; it wires the real env / TTY /
// clock / filesystem to the pure decisions (`should_check`, `drift_notice`,
// `throttle_elapsed`).

/// Env var the parent sets on the detached child so it runs only the
/// background refresh (fetch + cache write) and exits before any arg parsing.
pub const REFRESH_ENV: &str = "BWOC__UPDATE_REFRESH";

/// Env var an operator sets to opt out of the startup drift notice entirely.
const OPT_OUT_ENV: &str = "BWOC_NO_UPDATE_CHECK";

/// The network is hit at most once per this window; other runs read the cache.
const THROTTLE_SECS: u64 = 24 * 60 * 60;

/// Cache filename under `~/.bwoc/`.
const CACHE_FILE: &str = "update-check.json";

// ── Mockable clock seam ─────────────────────────────────────────────────────────

/// Wall-clock seam so throttle-window logic is unit-testable without sleeping.
pub trait Clock {
    /// Seconds since the Unix epoch.
    fn now_unix(&self) -> u64;
}

/// Production clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

// ── Throttle cache ──────────────────────────────────────────────────────────────

/// Parsed `~/.bwoc/update-check.json`. `latest_seen` is `""` until a fetch
/// lands, so an offline first run records the check time without a false
/// version. Hand-parsed via `serde_json::Value` to match this file's existing
/// JSON style (no serde-derive dep added).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCache {
    pub last_checked: u64,
    pub latest_seen: String,
}

impl UpdateCache {
    /// Parse on-disk JSON. Tolerant: a missing/garbled field defaults rather
    /// than failing the whole read (a corrupt cache must never wedge the CLI).
    fn from_json(s: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(s).ok()?;
        Some(Self {
            last_checked: v.get("last_checked").and_then(|x| x.as_u64()).unwrap_or(0),
            latest_seen: v
                .get("latest_seen")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }

    fn to_json(&self) -> String {
        serde_json::json!({
            "last_checked": self.last_checked,
            "latest_seen": self.latest_seen,
        })
        .to_string()
    }
}

fn read_cache(path: &std::path::Path) -> Option<UpdateCache> {
    UpdateCache::from_json(&std::fs::read_to_string(path).ok()?)
}

fn write_cache(path: &std::path::Path, cache: &UpdateCache) -> std::io::Result<()> {
    std::fs::write(path, cache.to_json())
}

/// True once the throttle window has fully elapsed since `last_checked`.
/// Pure — `now` and `window` are injected so tests need no real clock.
/// `saturating_sub` makes a future `last_checked` (clock skew) read as "fresh".
fn throttle_elapsed(last_checked: u64, now: u64, window: u64) -> bool {
    now.saturating_sub(last_checked) >= window
}

// ── Drift detection (pure) ────────────────────────────────────────────────────────

/// If `latest_seen` parses as a CalVer strictly newer than `current`, return
/// the `(current, latest)` canonical tags to show. Otherwise `None` — including
/// when either side is unparseable (offline / empty cache → silent).
pub fn drift_notice(current: &str, latest_seen: &str) -> Option<(String, String)> {
    let cur = CalVer::parse(current)?;
    let lat = CalVer::parse(latest_seen)?;
    (lat > cur).then(|| (cur.to_tag(), lat.to_tag()))
}

// ── Guard decision (pure) ─────────────────────────────────────────────────────────

/// Everything that decides whether the startup check runs at all. Gathered
/// from the real environment by `notify_if_drifted`, but kept as plain data so
/// `should_check` is exhaustively unit-testable.
pub struct GuardContext {
    /// The invoked subcommand is `update` itself (it does its own check).
    pub is_update_command: bool,
    /// `--json` appears on the command line (machine-readable mode).
    pub is_json: bool,
    /// stdout is a real terminal (false ⇒ piped / redirected / CI).
    pub stdout_is_tty: bool,
    /// `BWOC_NO_UPDATE_CHECK` is set.
    pub opt_out: bool,
    /// A source/dev build (`BWOC_RELEASE_CALVER` unset) — only released
    /// binaries drift-check, since there's no embedded version to compare.
    pub is_source_build: bool,
}

/// Pure gate: run the drift check only on an interactive, released,
/// non-JSON, non-`update` invocation that hasn't opted out.
pub fn should_check(ctx: &GuardContext) -> bool {
    !ctx.is_update_command
        && !ctx.is_json
        && ctx.stdout_is_tty
        && !ctx.opt_out
        && !ctx.is_source_build
}

// ── Orchestration + notice ──────────────────────────────────────────────────────

/// Startup hook — call beside `whats_new::notify_if_updated()` for subcommands.
/// Prints the *cached* drift notice (if any) this run, then — only when the
/// throttle window has elapsed — spawns a detached refresh for next time. Never
/// blocks on the network, never fails a command, silent offline.
pub fn notify_if_drifted(is_update_command: bool) {
    // Released binaries only: source builds have no embedded CalVer to compare.
    let current = match option_env!("BWOC_RELEASE_CALVER") {
        Some(c) => c,
        None => return,
    };
    let ctx = GuardContext {
        is_update_command,
        is_json: std::env::args().any(|a| a == "--json"),
        stdout_is_tty: std::io::stdout().is_terminal(),
        opt_out: std::env::var_os(OPT_OUT_ENV).is_some(),
        is_source_build: false, // current is Some ⇒ released
    };
    if !should_check(&ctx) {
        return;
    }
    let Ok(home) = crate::user_home::bwoc_home() else {
        return;
    };
    let path = home.join(CACHE_FILE);

    match read_cache(&path) {
        Some(cache) => {
            // Print what we already know this run (cheap, no network).
            if let Some((cur, lat)) = drift_notice(current, &cache.latest_seen) {
                print_drift_notice(&cur, &lat);
            }
            // Refresh in the background only once the window has elapsed.
            if throttle_elapsed(cache.last_checked, SystemClock.now_unix(), THROTTLE_SECS) {
                spawn_background_refresh();
            }
        }
        // No cache yet → silent this run; seed it in the background for next time.
        None => spawn_background_refresh(),
    }
}

/// One-line notice to **stderr** (so it never pollutes stdout / JSON). Colored
/// only when stderr is a TTY.
fn print_drift_notice(current: &str, latest: &str) {
    let (yellow, reset) = if std::io::stderr().is_terminal() {
        ("\x1b[1;33m", "\x1b[0m")
    } else {
        ("", "")
    };
    eprintln!("{yellow}⬆ bwoc {latest} available (you have {current}) — run 'bwoc update'{reset}");
}

// ── Detached background refresh ──────────────────────────────────────────────────

/// Run only by the detached child (`REFRESH_ENV` set): ensure `~/.bwoc/`, hit
/// the network once, update the cache, exit. Production entry — wires the real
/// runner + clock + path.
pub fn run_background_refresh() {
    let _ = crate::user_home::ensure_initialized();
    let Ok(home) = crate::user_home::bwoc_home() else {
        return;
    };
    refresh_cache_at(&home.join(CACHE_FILE), &ProcessShellRunner, &SystemClock);
}

/// Testable core: fetch the latest tag and write the cache at an explicit path.
/// Always advances `last_checked` (so an offline run still throttles), but only
/// overwrites `latest_seen` on a valid fetch — never fabricate a version, never
/// wipe a known one (Musāvāda).
fn refresh_cache_at(path: &std::path::Path, runner: &dyn ShellRunner, clock: &dyn Clock) {
    let previous = read_cache(path).map(|c| c.latest_seen).unwrap_or_default();
    let latest_seen = match fetch_latest_tag(runner) {
        Some(tag) if CalVer::parse(&tag).is_some() => tag,
        _ => previous,
    };
    let _ = write_cache(
        path,
        &UpdateCache {
            last_checked: clock.now_unix(),
            latest_seen,
        },
    );
}

/// Spawn ourselves detached with `REFRESH_ENV` set, null stdio, and no wait.
/// The child outlives this process (reparented on exit) and writes the cache
/// for the *next* run; this process returns immediately — the command never
/// blocks on the network. Spawn failure is swallowed: a missed refresh just
/// means one more cached run, never a broken command.
fn spawn_background_refresh() {
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(exe)
        .env(REFRESH_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── CalVer parse + compare ─────────────────────────────────────────────

    #[test]
    fn calver_parse_full() {
        let v = CalVer::parse("v2026.5.24-0").unwrap();
        assert_eq!(
            v,
            CalVer {
                year: 2026,
                month: 5,
                day: 24,
                patch: 0
            }
        );
    }

    #[test]
    fn calver_parse_without_v_prefix() {
        let v = CalVer::parse("2026.12.31-9").unwrap();
        assert_eq!(
            v,
            CalVer {
                year: 2026,
                month: 12,
                day: 31,
                patch: 9
            }
        );
    }

    #[test]
    fn calver_parse_malformed_returns_none() {
        assert!(CalVer::parse("v2026.5").is_none());
        assert!(CalVer::parse("not-a-version").is_none());
        assert!(CalVer::parse("").is_none());
        assert!(CalVer::parse("v2026.5.24").is_none()); // missing patch
    }

    #[test]
    fn calver_ordering_patch_wins_same_date() {
        let v0 = CalVer::parse("v2026.5.24-0").unwrap();
        let v1 = CalVer::parse("v2026.5.24-1").unwrap();
        assert!(v1 > v0);
    }

    #[test]
    fn calver_ordering_date_wins_over_patch() {
        let old = CalVer::parse("v2026.5.24-9").unwrap();
        let new = CalVer::parse("v2026.5.25-0").unwrap();
        assert!(new > old);
    }

    #[test]
    fn calver_to_tag_round_trips() {
        let tag = "v2026.5.24-0";
        assert_eq!(CalVer::parse(tag).unwrap().to_tag(), tag);
    }

    // ── Mock runner ────────────────────────────────────────────────────────

    struct MockRunner {
        /// Maps (program, first_arg) → (exit_code, stdout).
        responses: Vec<(String, String, i32, String)>,
        /// Records every invocation as (program, full_args) so tests can assert
        /// the complete command contract, not just the dispatch key.
        calls: std::cell::RefCell<Vec<(String, Vec<String>)>>,
    }

    impl MockRunner {
        /// Each entry: (program, first_arg, exit_code, stdout).
        fn new(responses: Vec<(&str, &str, i32, &str)>) -> Self {
            Self {
                responses: responses
                    .into_iter()
                    .map(|(p, a, c, s)| (p.to_string(), a.to_string(), c, s.to_string()))
                    .collect(),
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }

        /// The full argument vector the runner received for `program`, if called.
        fn args_for(&self, program: &str) -> Option<Vec<String>> {
            self.calls
                .borrow()
                .iter()
                .find(|(p, _)| p == program)
                .map(|(_, a)| a.clone())
        }
    }

    impl ShellRunner for MockRunner {
        fn run(&self, program: &str, args: &[&str]) -> ShellOutcome {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            let first_arg = args.first().copied().unwrap_or("");
            for (p, a, code, out) in &self.responses {
                if p == program && a == first_arg {
                    return ShellOutcome {
                        exit_code: *code,
                        stdout: out.clone(),
                    };
                }
            }
            // No match → simulate not-found.
            ShellOutcome {
                exit_code: 127,
                stdout: String::new(),
            }
        }
    }

    // ── fetch_latest_tag ───────────────────────────────────────────────────

    #[test]
    fn fetch_uses_gh_primary() {
        let runner = MockRunner::new(vec![("gh", "release", 0, "v2026.5.24-0")]);
        let tag = fetch_latest_tag(&runner);
        assert_eq!(tag, Some("v2026.5.24-0".to_string()));
    }

    #[test]
    fn fetch_invokes_full_command_contract() {
        // gh fails so both branches run; assert the EXACT args of each — incl.
        // the `--repo` value and the curl URL, not just the first arg.
        let json = r#"{"tag_name":"v2026.5.24-0"}"#;
        let runner = MockRunner::new(vec![("gh", "release", 1, ""), ("curl", "-s", 0, json)]);
        let _ = fetch_latest_tag(&runner);

        let expected_gh: Vec<String> = [
            "release",
            "view",
            "--repo",
            "bemindlabs/BWOC-Framework",
            "--json",
            "tagName",
            "-q",
            ".tagName",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(runner.args_for("gh").unwrap(), expected_gh);

        let expected_curl: Vec<String> = ["-s", GITHUB_API_URL]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(runner.args_for("curl").unwrap(), expected_curl);
    }

    #[test]
    fn fetch_falls_back_to_curl_when_gh_fails() {
        let json = r#"{"tag_name":"v2026.5.24-0","name":"v2026.5.24-0"}"#;
        let runner = MockRunner::new(vec![
            ("gh", "release", 1, ""), // gh fails
            ("curl", "-s", 0, json),  // curl succeeds
        ]);
        let tag = fetch_latest_tag(&runner);
        assert_eq!(tag, Some("v2026.5.24-0".to_string()));
    }

    #[test]
    fn fetch_returns_none_when_both_fail() {
        let runner = MockRunner::new(vec![("gh", "release", 1, ""), ("curl", "-s", 1, "")]);
        assert!(fetch_latest_tag(&runner).is_none());
    }

    // ── check() scenario matrix ────────────────────────────────────────────
    //
    // Note: option_env!("BWOC_RELEASE_CALVER") is resolved at compile time.
    // In a dev/test build this env is NOT set, so all check() tests land on
    // the SourceBuild branch. The scenarios below test the pure comparison
    // logic directly (compare_versions) rather than going through check().
    // The FetchFailed + MalformedLatestTag paths do NOT depend on the env.

    /// Directly test the comparison function logic used inside check().
    fn compare(current: &str, latest: &str) -> CheckResult {
        let latest_ver = CalVer::parse(latest).expect("test: bad latest");
        match CalVer::parse(current) {
            None => CheckResult::MalformedCurrentTag {
                raw: current.to_string(),
                latest: latest_ver.to_tag(),
            },
            Some(cv) => {
                use std::cmp::Ordering;
                match cv.cmp(&latest_ver) {
                    Ordering::Equal => CheckResult::UpToDate {
                        tag: latest_ver.to_tag(),
                    },
                    Ordering::Less => CheckResult::UpdateAvailable {
                        current: cv.to_tag(),
                        latest: latest_ver.to_tag(),
                    },
                    Ordering::Greater => CheckResult::AheadOfLatest {
                        current: cv.to_tag(),
                        latest: latest_ver.to_tag(),
                    },
                }
            }
        }
    }

    #[test]
    fn scenario_update_available() {
        let r = compare("v2026.5.20-0", "v2026.5.24-0");
        assert!(matches!(r, CheckResult::UpdateAvailable { .. }));
        if let CheckResult::UpdateAvailable { current, latest } = r {
            assert_eq!(current, "v2026.5.20-0");
            assert_eq!(latest, "v2026.5.24-0");
        }
    }

    #[test]
    fn scenario_up_to_date() {
        let r = compare("v2026.5.24-0", "v2026.5.24-0");
        assert!(matches!(r, CheckResult::UpToDate { .. }));
    }

    #[test]
    fn scenario_ahead_of_latest() {
        let r = compare("v2026.5.25-0", "v2026.5.24-0");
        assert!(matches!(r, CheckResult::AheadOfLatest { .. }));
    }

    #[test]
    fn scenario_malformed_latest_tag() {
        // check() itself handles this; replicate via fetch_latest_tag returning bad tag.
        let runner = MockRunner::new(vec![("gh", "release", 0, "not-a-calver")]);
        let tag = fetch_latest_tag(&runner);
        assert_eq!(tag, Some("not-a-calver".to_string()));
        // CalVer parse fails.
        assert!(CalVer::parse("not-a-calver").is_none());
    }

    #[test]
    fn scenario_source_build_flag() {
        // option_env!(BWOC_RELEASE_CALVER) == None in test builds.
        // Verify the arm is reachable: check() returns SourceBuild when env unset.
        let runner = MockRunner::new(vec![("gh", "release", 0, "v2026.5.24-0")]);
        let (result, code) = check(&runner);
        // In a source build (no env set), we always get SourceBuild and code 0.
        assert_eq!(code, 0);
        assert!(
            matches!(result, CheckResult::SourceBuild { .. }),
            "expected SourceBuild for dev build, got {result:?}"
        );
        if let CheckResult::SourceBuild { latest } = result {
            assert_eq!(latest, "v2026.5.24-0");
        }
    }

    #[test]
    fn scenario_fetch_failed() {
        let runner = MockRunner::new(vec![("gh", "release", 1, ""), ("curl", "-s", 1, "")]);
        let (result, code) = check(&runner);
        assert_eq!(code, 2);
        assert_eq!(result, CheckResult::FetchFailed);
    }

    // ── install-method classification ─────────────────────────────────────

    #[test]
    fn classify_homebrew_cellar() {
        // current_exe() resolves the brew symlink to a Cellar path.
        assert_eq!(
            classify_install_path("/opt/homebrew/Cellar/bwoc/2.2.0/bin/bwoc"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            classify_install_path("/usr/local/Cellar/bwoc/2.2.0/bin/bwoc"),
            InstallMethod::Homebrew
        );
        assert_eq!(
            classify_install_path("/home/linuxbrew/.linuxbrew/bin/bwoc"),
            InstallMethod::Homebrew
        );
    }

    #[test]
    fn classify_cargo_bin() {
        assert_eq!(
            classify_install_path("/Users/dev/.cargo/bin/bwoc"),
            InstallMethod::Cargo
        );
    }

    #[test]
    fn classify_raw_binary() {
        assert_eq!(
            classify_install_path("/usr/local/bin/bwoc"),
            InstallMethod::Raw
        );
        assert_eq!(classify_install_path("/tmp/bwoc"), InstallMethod::Raw);
    }

    // ── apply-action routing (pure) ────────────────────────────────────────

    fn update_available() -> CheckResult {
        CheckResult::UpdateAvailable {
            current: "v2026.5.23-3".to_string(),
            latest: "v2026.5.24-0".to_string(),
        }
    }

    #[test]
    fn apply_homebrew_delegates_to_brew() {
        let action = apply_action(&update_available(), &InstallMethod::Homebrew);
        assert_eq!(
            action,
            ApplyAction::Delegate {
                program: "brew".to_string(),
                args: vec!["upgrade".to_string(), "bwoc".to_string()],
            }
        );
    }

    #[test]
    fn apply_cargo_delegates_to_cargo() {
        let action = apply_action(&update_available(), &InstallMethod::Cargo);
        match action {
            ApplyAction::Delegate { program, args } => {
                assert_eq!(program, "cargo");
                assert_eq!(args[0], "install");
                assert!(args.contains(&"bwoc-cli".to_string()));
            }
            other => panic!("expected cargo delegate, got {other:?}"),
        }
    }

    #[test]
    fn apply_raw_is_manual_no_self_swap() {
        let action = apply_action(&update_available(), &InstallMethod::Raw);
        assert_eq!(action, ApplyAction::Manual);
    }

    #[test]
    fn apply_up_to_date_is_noop() {
        let r = CheckResult::UpToDate {
            tag: "v2026.5.24-0".to_string(),
        };
        assert_eq!(
            apply_action(&r, &InstallMethod::Homebrew),
            ApplyAction::AlreadyCurrent {
                tag: "v2026.5.24-0".to_string()
            }
        );
    }

    #[test]
    fn apply_source_build_is_rebuild() {
        let r = CheckResult::SourceBuild {
            latest: "v2026.5.24-0".to_string(),
        };
        assert_eq!(
            apply_action(&r, &InstallMethod::Raw),
            ApplyAction::SourceRebuild
        );
    }

    #[test]
    fn apply_fetch_failure_is_check_error_not_blind_upgrade() {
        assert_eq!(
            apply_action(&CheckResult::FetchFailed, &InstallMethod::Homebrew),
            ApplyAction::CheckError
        );
    }

    // ── apply integration via run_with (no --check) ────────────────────────

    #[test]
    fn run_with_apply_source_build_returns_zero() {
        // In a test build `BWOC_RELEASE_CALVER` is unset → check() = SourceBuild
        // (given a successful fetch) → apply = source-rebuild guidance, code 0.
        let runner = MockRunner::new(vec![("gh", "release", 0, "v2026.5.24-0")]);
        let code = run_with(
            UpdateArgs {
                check: false,
                run: false,
            },
            &runner,
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn run_with_apply_check_failure_returns_two() {
        // Fetch fails → never upgrade on uncertainty.
        let runner = MockRunner::new(vec![]);
        let code = run_with(
            UpdateArgs {
                check: false,
                run: false,
            },
            &runner,
        );
        assert_eq!(code, 2);
    }

    // ── Startup drift guard (issue #44) ────────────────────────────────────

    struct MockClock(u64);
    impl Clock for MockClock {
        fn now_unix(&self) -> u64 {
            self.0
        }
    }

    fn clear_ctx() -> GuardContext {
        GuardContext {
            is_update_command: false,
            is_json: false,
            stdout_is_tty: true,
            opt_out: false,
            is_source_build: false,
        }
    }

    // — guard skips —

    #[test]
    fn guard_runs_on_clear_interactive_release() {
        assert!(should_check(&clear_ctx()));
    }

    #[test]
    fn guard_skips_the_update_command_itself() {
        let ctx = GuardContext {
            is_update_command: true,
            ..clear_ctx()
        };
        assert!(!should_check(&ctx));
    }

    #[test]
    fn guard_skips_json_mode() {
        let ctx = GuardContext {
            is_json: true,
            ..clear_ctx()
        };
        assert!(!should_check(&ctx));
    }

    #[test]
    fn guard_skips_when_stdout_is_not_a_tty() {
        let ctx = GuardContext {
            stdout_is_tty: false,
            ..clear_ctx()
        };
        assert!(!should_check(&ctx));
    }

    #[test]
    fn guard_skips_when_opted_out() {
        let ctx = GuardContext {
            opt_out: true,
            ..clear_ctx()
        };
        assert!(!should_check(&ctx));
    }

    #[test]
    fn guard_skips_source_builds() {
        let ctx = GuardContext {
            is_source_build: true,
            ..clear_ctx()
        };
        assert!(!should_check(&ctx));
    }

    // — CalVer-newer detection —

    #[test]
    fn drift_notice_fires_when_latest_is_newer() {
        let r = drift_notice("v2026.5.20-0", "v2026.5.25-0");
        assert_eq!(
            r,
            Some(("v2026.5.20-0".to_string(), "v2026.5.25-0".to_string()))
        );
    }

    #[test]
    fn drift_notice_silent_when_equal() {
        assert_eq!(drift_notice("v2026.5.25-0", "v2026.5.25-0"), None);
    }

    #[test]
    fn drift_notice_silent_when_current_is_ahead() {
        // Dev/local binary tagged ahead of the published release — no notice.
        assert_eq!(drift_notice("v2026.5.26-0", "v2026.5.25-0"), None);
    }

    #[test]
    fn drift_notice_silent_on_empty_or_malformed_cache() {
        // Offline first run leaves latest_seen "" → must stay silent (Musāvāda).
        assert_eq!(drift_notice("v2026.5.25-0", ""), None);
        assert_eq!(drift_notice("v2026.5.25-0", "garbage"), None);
        assert_eq!(drift_notice("garbage", "v2026.5.25-0"), None);
    }

    // — throttle window —

    #[test]
    fn throttle_skips_within_window() {
        // 12h since last check < 24h window → do not hit the network.
        assert!(!throttle_elapsed(0, 12 * 60 * 60, THROTTLE_SECS));
    }

    #[test]
    fn throttle_fires_at_and_past_window() {
        assert!(throttle_elapsed(0, THROTTLE_SECS, THROTTLE_SECS)); // exactly 24h
        assert!(throttle_elapsed(0, THROTTLE_SECS + 1, THROTTLE_SECS)); // past
    }

    #[test]
    fn throttle_treats_future_last_checked_as_fresh() {
        // Clock skew: last_checked ahead of now → saturating_sub = 0 → skip.
        assert!(!throttle_elapsed(2_000, 1_000, THROTTLE_SECS));
    }

    // — cache round-trip —

    #[test]
    fn cache_json_round_trips() {
        let c = UpdateCache {
            last_checked: 1_700_000_000,
            latest_seen: "v2026.5.25-0".to_string(),
        };
        assert_eq!(UpdateCache::from_json(&c.to_json()).unwrap(), c);
    }

    #[test]
    fn cache_parse_is_tolerant_of_missing_fields() {
        let c = UpdateCache::from_json("{}").unwrap();
        assert_eq!(c.last_checked, 0);
        assert_eq!(c.latest_seen, "");
        assert!(UpdateCache::from_json("not json").is_none());
    }

    // — background refresh (mock runner + clock, explicit path; no $HOME) —

    fn tmp_cache_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "bwoc-update-check-{}-{}.json",
            std::process::id(),
            tag
        ))
    }

    #[test]
    fn refresh_writes_fetched_tag_and_clock() {
        let path = tmp_cache_path("fetched");
        let _ = std::fs::remove_file(&path);
        let runner = MockRunner::new(vec![("gh", "release", 0, "v2026.5.25-0")]);
        refresh_cache_at(&path, &runner, &MockClock(1_000));

        let cache = read_cache(&path).unwrap();
        assert_eq!(cache.last_checked, 1_000);
        assert_eq!(cache.latest_seen, "v2026.5.25-0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refresh_offline_keeps_prior_version_but_advances_clock() {
        // Pre-seed a known-newer version, then go offline. The version must
        // survive (no false "up to date") while the clock advances so the
        // 24h throttle still holds.
        let path = tmp_cache_path("offline-keep");
        write_cache(
            &path,
            &UpdateCache {
                last_checked: 0,
                latest_seen: "v2026.5.24-0".to_string(),
            },
        )
        .unwrap();
        let offline = MockRunner::new(vec![]); // gh + curl both 127
        refresh_cache_at(&path, &offline, &MockClock(2_000));

        let cache = read_cache(&path).unwrap();
        assert_eq!(cache.latest_seen, "v2026.5.24-0", "keep known version");
        assert_eq!(cache.last_checked, 2_000, "throttle clock advanced");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refresh_offline_first_run_records_no_version() {
        // No prior cache + offline → empty latest_seen (silent), time recorded.
        let path = tmp_cache_path("offline-first");
        let _ = std::fs::remove_file(&path);
        let offline = MockRunner::new(vec![]);
        refresh_cache_at(&path, &offline, &MockClock(500));

        let cache = read_cache(&path).unwrap();
        assert_eq!(cache.latest_seen, "");
        assert_eq!(cache.last_checked, 500);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn refresh_ignores_malformed_fetched_tag() {
        // A non-CalVer tag must not be cached as a version.
        let path = tmp_cache_path("malformed");
        let _ = std::fs::remove_file(&path);
        let runner = MockRunner::new(vec![("gh", "release", 0, "not-a-calver")]);
        refresh_cache_at(&path, &runner, &MockClock(700));

        let cache = read_cache(&path).unwrap();
        assert_eq!(cache.latest_seen, "");
        assert_eq!(cache.last_checked, 700);
        let _ = std::fs::remove_file(&path);
    }
}
