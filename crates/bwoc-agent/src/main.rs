//! `bwoc-agent` — minimal runtime shipped with each incarnated BWOC agent.
//!
//! Two modes:
//!   - **default** (no args): print the liveness banner from
//!     `config.manifest.json` in cwd and exit. Phase 1 v2.0 DoD.
//!   - **--serve**: write `<cwd>/.bwoc/agent.pid` and block until
//!     SIGTERM / SIGINT. This is the first foundation step toward
//!     Phase 2's control socket — `bwoc status` can detect a running
//!     agent via the PID file + signal-0 liveness test even before
//!     the full IPC protocol lands.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bwoc_core::manifest::Manifest;

mod i18n;
#[cfg(unix)]
mod task_watch;
#[cfg(unix)]
mod trust;

fn main() -> ExitCode {
    // Lightweight arg handling — keeps the daemon binary clap-free (it
    // only ever takes 1-2 flags, not a real subcommand tree).
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("bwoc-agent {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return ExitCode::SUCCESS;
    }
    let serve = args.iter().any(|a| a == "--serve");
    let lang = i18n::resolve_lang();
    let bundle = i18n::bundle_for(&lang);

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manifest_path = cwd.join("config.manifest.json");

    if !manifest_path.exists() {
        let cwd_display = cwd.display().to_string();
        eprintln!(
            "{}",
            i18n::t_with(&bundle, "error-missing-manifest", &[("cwd", &cwd_display)])
        );
        return ExitCode::from(2);
    }

    let manifest = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "bwoc-agent: failed to load manifest at {}: {e}",
                manifest_path.display()
            );
            return ExitCode::from(1);
        }
    };

    println!("{}", liveness_banner(&manifest, &bundle));

    if serve {
        return serve_loop(&cwd, &manifest);
    }
    ExitCode::SUCCESS
}

fn print_usage() {
    println!(
        "bwoc-agent {} — runtime shipped with each incarnated BWOC agent

USAGE:
    bwoc-agent [FLAGS]

FLAGS:
    --serve         Run as daemon: write .bwoc/agent.pid, open Unix socket
                    at .bwoc/agent.sock, watch inbox, block on SIGTERM/SIGINT.
                    Unix-only (Windows named-pipe path queued).

    --version, -V   Print version and exit
    --help, -h      Print this message and exit

DEFAULT (no flags):
    Print the liveness banner from `config.manifest.json` in cwd and exit.
    Used by `bwoc check` and Phase 1 sanity tests.

ENV:
    BWOC_LANG       Locale for output (en | th). Falls back to $LANG then en.

SEE ALSO:
    bwoc help daemon    — IPC protocol, doctor sweeps, lifecycle.",
        env!("CARGO_PKG_VERSION")
    );
}

/// Non-Unix stub. The full `--serve` daemon mode relies on Unix domain
/// sockets, signal handling via signal-0, and ctrlc — none of which
/// have shipped on the Windows path yet. Document the gap clearly so
/// users hitting `bwoc start` on Windows see the right error rather
/// than a cryptic compile-or-runtime failure.
#[cfg(not(unix))]
fn serve_loop(_cwd: &std::path::Path, _manifest: &Manifest) -> ExitCode {
    eprintln!(
        "bwoc-agent --serve: daemon mode is currently Unix-only \
         (uses Unix domain sockets + signal-0 liveness). \
         Windows named-pipe support is queued; see ROADMAP."
    );
    ExitCode::from(2)
}

/// `--serve` mode: write a PID file at `.bwoc/agent.pid`, open a Unix
/// domain socket at `.bwoc/agent.sock`, and accept simple line-based
/// requests until SIGTERM / SIGINT. Removes both files on exit.
///
/// Phase 0 IPC protocol — line-based, one request per connection:
///   `PING\n`       → `PONG\n`
///   anything else  → `ERR unknown command\n`
///
/// Future commands (STATUS / LOG / SEND / STOP) will slot in here as
/// they're spec'd. Keeping it line-text instead of binary so it's
/// debuggable with `nc -U`.
#[cfg(unix)]
fn serve_loop(cwd: &std::path::Path, manifest: &Manifest) -> ExitCode {
    use std::io::ErrorKind;
    use std::os::unix::net::UnixListener;

    let bwoc_dir = cwd.join(".bwoc");
    if let Err(e) = std::fs::create_dir_all(&bwoc_dir) {
        eprintln!("bwoc-agent --serve: failed to create .bwoc/: {e}");
        return ExitCode::from(1);
    }
    let pid_path = bwoc_dir.join("agent.pid");
    let sock_path = bwoc_dir.join("agent.sock");

    // If a previous run left a socket behind, remove it (the pid file is
    // handled by the doctor stale-sweep separately).
    let _ = std::fs::remove_file(&sock_path);

    let pid = std::process::id();
    if let Err(e) = std::fs::write(&pid_path, format!("{pid}\n")) {
        eprintln!(
            "bwoc-agent --serve: failed to write {}: {e}",
            pid_path.display()
        );
        return ExitCode::from(1);
    }
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "bwoc-agent --serve: failed to bind {}: {e}",
                sock_path.display()
            );
            let _ = std::fs::remove_file(&pid_path);
            return ExitCode::from(1);
        }
    };
    if let Err(e) = listener.set_nonblocking(true) {
        eprintln!("bwoc-agent --serve: failed to set non-blocking: {e}");
        let _ = std::fs::remove_file(&pid_path);
        let _ = std::fs::remove_file(&sock_path);
        return ExitCode::from(1);
    }

    eprintln!("bwoc-agent --serve: pid {pid} → {}", pid_path.display());
    eprintln!("bwoc-agent --serve: socket → {}", sock_path.display());
    eprintln!("bwoc-agent --serve: blocking on SIGTERM / SIGINT (Ctrl-C)");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    if let Err(e) = ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }) {
        eprintln!("bwoc-agent --serve: failed to install signal handler: {e}");
        let _ = std::fs::remove_file(&pid_path);
        let _ = std::fs::remove_file(&sock_path);
        return ExitCode::from(1);
    }

    // Daemon start time — used by the STATUS command to report uptime.
    let start = Instant::now();

    // Inbox watching — track byte offset into `.bwoc/inbox.jsonl` and
    // announce new envelopes to stderr. Cursor persists across restarts
    // via `.bwoc/inbox.cursor` so a daemon offline period doesn't skip
    // messages that arrived while it was down.
    let inbox_path = bwoc_dir.join("inbox.jsonl");
    let cursor_path = bwoc_dir.join("inbox.cursor");
    let refusals_path = bwoc_dir.join("inbox.refusals.jsonl");
    let inbox_size: u64 = std::fs::metadata(&inbox_path).map(|m| m.len()).unwrap_or(0);
    let mut inbox_pos: u64 = match load_cursor(&cursor_path) {
        Some(c) if c <= inbox_size => c,
        Some(c) => {
            eprintln!(
                "bwoc-agent --serve: cursor ({c}) > inbox size ({inbox_size}) — resetting to EOF (file truncated)"
            );
            inbox_size
        }
        None => inbox_size, // first run; start at EOF (don't replay history)
    };
    if inbox_path.is_file() {
        eprintln!(
            "bwoc-agent --serve: watching inbox → {} (cursor {inbox_pos} / size {inbox_size})",
            inbox_path.display()
        );
    } else {
        eprintln!(
            "bwoc-agent --serve: watching inbox → {} (will create on first send)",
            inbox_path.display()
        );
    }

    // Kalyāṇamitta-7 trust posture. Inert (gating off OR requiredTrust
    // empty) ≡ pre-step-4 behavior; the evaluate call short-circuits
    // before parsing any envelopes. Built once here so env reads + the
    // ancestor workspace walk happen only once per daemon lifetime.
    let trust_ctx = trust::TrustContext::build(manifest, cwd);
    if trust_ctx.gating_enabled {
        if trust_ctx.is_inert() {
            eprintln!(
                "bwoc-agent --serve: trust gating ON (BWOC_TRUST_GATING=1) but \
                 requiredTrust empty → no refusals will fire"
            );
        } else {
            eprintln!(
                "bwoc-agent --serve: trust gating ON — refusing senders missing {:?}",
                trust_ctx.required
            );
            if trust_ctx.workspace_root.is_none() {
                eprintln!(
                    "bwoc-agent --serve: warning — no workspace found in ancestors; \
                     non-`user` envelopes will refuse with reason=no_workspace"
                );
            }
        }
    }

    // Saṅgha task watch (Phase B, announce-only). Reuses the workspace
    // root the trust context already resolved. Snapshots currently-
    // claimable tasks at startup (no replay) and announces new ones to
    // stderr. Polled at a slower cadence than the inbox — tasks change
    // rarely.
    let mut task_watch =
        task_watch::TaskWatch::build(&manifest.agent_id, trust_ctx.workspace_root.as_deref());
    if !task_watch.is_inert() {
        let mode = if task_watch.auto_claim_enabled() {
            " (BWOC_AUTO_CLAIM=1 — will claim new tasks + wake the agent)"
        } else if task_watch.wakeup_enabled() {
            " (BWOC_TASK_WAKEUP=1 — will ping tmux session on new tasks)"
        } else {
            ""
        };
        eprintln!(
            "bwoc-agent --serve: watching Saṅgha tasks for member '{}'{mode}",
            manifest.agent_id
        );
    }
    const TASK_POLL_EVERY: Duration = Duration::from_secs(2);
    let mut last_task_poll = Instant::now();

    // Single-threaded accept loop with poll. Each accept is non-blocking
    // and yields control quickly so the signal check stays responsive.
    while running.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _addr)) => handle_client(stream, &running, &start),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                // Idle: check the inbox for new envelopes since last poll.
                let new_pos =
                    check_inbox_for_new(&inbox_path, inbox_pos, &trust_ctx, &refusals_path);
                if new_pos != inbox_pos {
                    inbox_pos = new_pos;
                    save_cursor(&cursor_path, inbox_pos);
                }
                // Saṅgha tasks change rarely — poll on a slower cadence than
                // the 100ms inbox tick to avoid re-reading team files 10×/s.
                if !task_watch.is_inert() && last_task_poll.elapsed() >= TASK_POLL_EVERY {
                    task_watch.poll();
                    last_task_poll = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("bwoc-agent --serve: accept error: {e}");
                break;
            }
        }
    }

    // Graceful exit — remove PID file + socket.
    if let Err(e) = std::fs::remove_file(&pid_path) {
        eprintln!(
            "bwoc-agent --serve: warning — failed to remove {}: {e}",
            pid_path.display()
        );
    }
    if let Err(e) = std::fs::remove_file(&sock_path) {
        eprintln!(
            "bwoc-agent --serve: warning — failed to remove {}: {e}",
            sock_path.display()
        );
    }
    eprintln!("bwoc-agent --serve: stopped cleanly");
    ExitCode::SUCCESS
}

/// Load the persisted inbox cursor (byte offset into inbox.jsonl).
/// Returns None if the file is missing, unreadable, or malformed —
/// callers treat that as "first run; start at current EOF".
#[cfg(unix)]
fn load_cursor(path: &std::path::Path) -> Option<u64> {
    let raw = std::fs::read_to_string(path).ok()?;
    raw.trim().parse::<u64>().ok()
}

/// Save the inbox cursor. Best-effort — failure logs to stderr but
/// doesn't bring down the daemon (cursor staleness costs at-most one
/// redundant message announcement on next restart).
#[cfg(unix)]
fn save_cursor(path: &std::path::Path, pos: u64) {
    if let Err(e) = std::fs::write(path, format!("{pos}\n")) {
        eprintln!(
            "bwoc-agent --serve: warning — failed to save cursor {}: {e}",
            path.display()
        );
    }
}

/// Read everything past `from_offset` in the inbox file and print any
/// new lines to stderr (one envelope per line). Returns the new offset
/// after consumption. Idempotent on no-change — returns the same offset.
/// Tolerant of: missing file (offset stays), file truncation (resets to
/// EOF), partial last-line (only consumes complete `\n`-terminated lines).
#[cfg(unix)]
fn check_inbox_for_new(
    path: &std::path::Path,
    from_offset: u64,
    trust_ctx: &trust::TrustContext,
    refusals_path: &std::path::Path,
) -> u64 {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return from_offset;
    };
    let Ok(meta) = file.metadata() else {
        return from_offset;
    };
    let size = meta.len();
    if size < from_offset {
        // File was truncated; reset to current EOF.
        eprintln!("bwoc-agent --serve: inbox truncated ({size} < {from_offset}); resetting cursor");
        return size;
    }
    if size == from_offset {
        return from_offset; // No new data.
    }
    if file.seek(SeekFrom::Start(from_offset)).is_err() {
        return from_offset;
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return from_offset;
    }
    // Process complete lines only; if the tail lacks `\n`, leave it for
    // the next poll.
    let mut consumed: u64 = 0;
    for line in buf.split_inclusive('\n') {
        if !line.ends_with('\n') {
            break; // partial — don't advance past it
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let envelope_offset = from_offset + consumed;
            match trust::evaluate(trust_ctx, trimmed, envelope_offset) {
                trust::TrustOutcome::Pass => announce(trimmed),
                trust::TrustOutcome::Warn {
                    ref from,
                    ref missing,
                } => {
                    announce(trimmed);
                    announce_warned(from, missing);
                }
                trust::TrustOutcome::Refuse(ref refusal) => {
                    record_refusal(refusals_path, refusal);
                    announce_refused(trimmed, refusal);
                }
            }
        }
        consumed += line.len() as u64;
    }
    from_offset + consumed
}

/// Append one refusal record to `<agent>/.bwoc/inbox.refusals.jsonl`.
/// Best-effort — failure logs a warning and continues. The inbox cursor
/// still advances even if the sidecar write fails, since we'd rather
/// drop a refusal note than reread the envelope forever.
#[cfg(unix)]
fn record_refusal(path: &std::path::Path, refusal: &trust::Refusal) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let ts = bwoc_core::time::utc_now_iso8601();
    let line = refusal.to_jsonl(&ts);
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                eprintln!(
                    "bwoc-agent --serve: warning — failed to write refusal to {}: {e}",
                    path.display()
                );
            }
        }
        Err(e) => {
            eprintln!(
                "bwoc-agent --serve: warning — failed to open {}: {e}",
                path.display()
            );
        }
    }
}

/// Variant of `announce` for refused envelopes — flags REFUSED + lists
/// missing qualities so the operator sees the policy fire on `bwoc log -f`.
#[cfg(unix)]
fn announce_refused(line: &str, refusal: &trust::Refusal) {
    let from = serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .as_ref()
        .and_then(|v| v.get("from").and_then(|x| x.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "?".into());
    eprintln!(
        "bwoc-agent: inbox REFUSED ← {from}: reason={} missing={:?}",
        refusal.reason, refusal.missing
    );
}

/// Emit a `trust_warn` log line when an envelope passes despite the
/// sender missing required qualities (mode=warn). The envelope has
/// already been announced via `announce`; this is the policy-level note.
///
/// Task (f): daemon emits this line on Warn and does NOT record a refusal.
#[cfg(unix)]
fn announce_warned(from: &str, missing: &[String]) {
    eprintln!("bwoc-agent: trust_warn ← {from}: missing={missing:?}");
}

/// Print one inbox envelope to stderr in a one-line form. Tries to parse
/// as JSON and pretty-print {from, message}; falls back to raw line.
#[cfg(unix)]
fn announce(line: &str) {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let from = v.get("from").and_then(|x| x.as_str()).unwrap_or("?");
        let msg = v.get("message").and_then(|x| x.as_str()).unwrap_or(line);
        eprintln!("bwoc-agent: inbox ← {from}: {msg}");
    } else {
        eprintln!("bwoc-agent: inbox (raw) ← {line}");
    }
}

#[cfg(unix)]
fn handle_client(
    mut stream: std::os::unix::net::UnixStream,
    running: &Arc<AtomicBool>,
    start: &Instant,
) {
    use std::io::{BufRead, BufReader, Write};
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let cmd = line.trim();

    // STATUS needs a dynamic response — uptime varies per call. Handle it
    // before the static-byte-slice branch.
    if cmd == "STATUS" {
        let uptime = start.elapsed().as_secs();
        let pid = std::process::id();
        let response = format!("OK uptime_secs={uptime} pid={pid}\n");
        let _ = stream.write_all(response.as_bytes());
        return;
    }

    let response: &[u8] = match cmd {
        "PING" => b"PONG\n",
        "STOP" => {
            // Mark for shutdown; the accept loop will see this on its
            // next iteration (within ~100ms) and exit cleanly. Reply
            // BEFORE flipping the flag so the client always reads our
            // response — otherwise the loop might race-clean the socket
            // before write_all returns.
            running.store(false, Ordering::SeqCst);
            b"OK shutting down\n"
        }
        _ => b"ERR unknown command\n",
    };
    let _ = stream.write_all(response);
}

/// Pure-data formatter for the liveness output. Kept separate from `main` so
/// it can be unit-tested without needing a real manifest on disk.
fn liveness_banner(
    m: &Manifest,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) -> String {
    let mut lines = Vec::with_capacity(8);
    lines.push(i18n::t_with(
        bundle,
        "liveness-alive",
        &[("agent_id", m.agent_id.as_str())],
    ));
    lines.push(i18n::t_with(
        bundle,
        "liveness-role",
        &[("role", m.agent_role.as_str())],
    ));
    lines.push(i18n::t_with(
        bundle,
        "liveness-model",
        &[("model", m.primary_model.as_str())],
    ));
    if let Some(ref fb) = m.fallback_model {
        lines.push(i18n::t_with(
            bundle,
            "liveness-fallback",
            &[("fallback", fb.as_str())],
        ));
    }
    lines.push(i18n::t_with(
        bundle,
        "liveness-memory",
        &[("memory_path", m.memory_path.as_str())],
    ));
    lines.push(i18n::t_with(
        bundle,
        "liveness-version",
        &[("version", m.version.as_str())],
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            name: "demo".into(),
            agent_id: "agent-demo".into(),
            agent_role: "demo role".into(),
            primary_model: "model-x".into(),
            fallback_model: Some("model-y".into()),
            memory_path: "memories/".into(),
            sessions_path: None,
            deep_memory_cmd: None,
            lint_cmd: "true".into(),
            format_cmd: "true".into(),
            test_cmd: "true".into(),
            build_cmd: "true".into(),
            worktree_base: None,
            scope_description: None,
            out_of_scope: None,
            trust: None,
            version: "2.0".into(),
        }
    }

    #[test]
    fn banner_shows_required_fields_en() {
        let bundle = i18n::bundle_for("en");
        let b = liveness_banner(&sample(), &bundle);
        assert!(b.contains("I am alive: agent-demo"));
        assert!(b.contains("demo role"));
        assert!(b.contains("model-x"));
        assert!(b.contains("model-y"));
        assert!(b.contains("memories/"));
        assert!(b.contains("2.0"));
    }

    #[test]
    fn banner_shows_required_fields_th() {
        let bundle = i18n::bundle_for("th");
        let b = liveness_banner(&sample(), &bundle);
        assert!(b.contains("ฉันยังมีชีวิตอยู่: agent-demo"));
        assert!(b.contains("demo role"));
        assert!(b.contains("model-x"));
    }

    #[test]
    fn banner_omits_optional_fallback_when_none() {
        let bundle = i18n::bundle_for("en");
        let mut m = sample();
        m.fallback_model = None;
        let b = liveness_banner(&m, &bundle);
        assert!(b.contains("I am alive:"));
        assert!(!b.contains("fallback:"));
    }
}
