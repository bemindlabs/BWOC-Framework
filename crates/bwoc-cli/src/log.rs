//! `bwoc log <agent> [--follow] [--lines N]` — view daemon stderr log.
//!
//! `bwoc start` redirects `bwoc-agent --serve`'s stderr to
//! `<agent>/.bwoc/agent.log` (append mode). This command reads it.
//!
//! Three modes:
//!   - default: print the last 50 lines (like `tail -n50`)
//!   - `--lines N` / `-n N`: print the last N lines
//!   - `--follow` / `-f`: print, then block + stream new lines as they
//!     arrive (Ctrl-C exits). Pairs with `bwoc start` in another
//!     terminal for live observation.
//!
//! Missing log file is NOT an error — empty agents that haven't been
//! started yet just print "(no log yet — run `bwoc start <agent>`)".

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bwoc_core::workspace::AgentsRegistry;

pub struct LogArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
    pub follow: bool,
    pub lines: usize,
    /// Truncate the log file before printing. Useful when the log has
    /// grown and you want to start a fresh observation. Mirror of
    /// `bwoc inbox --clear`.
    pub clear: bool,
}

pub fn run(args: LogArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc log: no workspace found. Pass --workspace, set BWOC_WORKSPACE, or run from a workspace dir."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc log: failed to read agents.toml: {e}");
            return 1;
        }
    };
    let lookup_id = if args.agent.starts_with("agent-") {
        args.agent.clone()
    } else {
        format!("agent-{}", args.agent)
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc log: no agent named '{}' in workspace {}. Try `bwoc list`.",
            args.agent,
            workspace.display()
        );
        return 2;
    };

    let log_path = workspace.join(&entry.path).join(".bwoc/agent.log");
    if !log_path.exists() {
        println!(
            "(no log yet — run `bwoc start {}` to spawn the daemon; logs land in {})",
            entry.id,
            log_path.display()
        );
        return 0;
    }

    // --clear: truncate before printing. Like `bwoc inbox --clear` it
    // happens with the file in place (preserves inode), so a running
    // daemon's open stderr handle keeps writing — it just starts at
    // offset 0 again.
    if args.clear {
        let prior_size = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
        match std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&log_path)
        {
            Ok(_) => {
                println!(
                    "Cleared {} ({} byte(s) discarded).",
                    log_path.display(),
                    prior_size
                );
            }
            Err(e) => {
                eprintln!(
                    "bwoc log --clear: failed to truncate {}: {e}",
                    log_path.display()
                );
                return 1;
            }
        }
        if !args.follow {
            return 0;
        }
        println!();
    }

    // 1. Print the tail.
    match print_tail(&log_path, args.lines) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("bwoc log: failed to read {}: {e}", log_path.display());
            return 1;
        }
    }

    // 2. If --follow, stream new lines.
    if args.follow {
        let start_offset = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
        if let Err(e) = follow_log(&log_path, start_offset) {
            eprintln!("bwoc log: follow loop ended: {e}");
            return 1;
        }
    }
    0
}

/// Read the last `n` lines of `path` into a `Vec<String>`. Uses a simple
/// read-into-memory + split approach — adequate for daemon logs that
/// are kept small (most agents will be hundreds of lines, not MBs).
/// If/when log rotation becomes a real concern, switch to a backwards-
/// reading byte-windowed approach.
///
/// Extracted so non-stdout consumers (the dashboard's detail-pane log
/// tail, refreshed on its 2s tick) can reuse the exact same tail logic
/// without shelling out or duplicating the read.
pub fn tail_lines(path: &Path, n: usize) -> std::io::Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..].iter().map(|l| l.to_string()).collect())
}

/// Print the last `n` lines of `path` to stdout.
fn print_tail(path: &Path, n: usize) -> std::io::Result<()> {
    for line in tail_lines(path, n)? {
        println!("{line}");
    }
    Ok(())
}

/// Block reading new appended content. Polls the file size every 300ms;
/// reads + prints any growth past the last-seen offset. Exit with Ctrl-C
/// (default SIGINT — no graceful state to flush since this is read-only).
fn follow_log(path: &Path, mut offset: u64) -> std::io::Result<()> {
    eprintln!();
    eprintln!("(following — Ctrl-C to stop)");
    loop {
        let Ok(meta) = std::fs::metadata(path) else {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        let size = meta.len();
        if size < offset {
            // Truncation (manual rotate / clear); reset to current EOF.
            eprintln!("(log truncated; resuming from new EOF)");
            offset = size;
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        if size == offset {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        for line in buf.split_inclusive('\n') {
            if line.ends_with('\n') {
                print!("{line}");
                offset += line.len() as u64;
            } else {
                // Partial last line; wait for the rest on next poll.
                break;
            }
        }
    }
}

fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return Some(PathBuf::from(env_path));
        }
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
    fn tail_lines_returns_last_n() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("agent.log");
        std::fs::write(&path, "l1\nl2\nl3\nl4\nl5\n").unwrap();
        let got = tail_lines(&path, 2).unwrap();
        assert_eq!(got, vec!["l4".to_string(), "l5".to_string()]);
    }

    #[test]
    fn tail_lines_caps_at_available_when_fewer_than_n() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("agent.log");
        std::fs::write(&path, "only\ntwo\n").unwrap();
        // Asking for more lines than exist returns all of them, not an error.
        let got = tail_lines(&path, 50).unwrap();
        assert_eq!(got, vec!["only".to_string(), "two".to_string()]);
    }

    #[test]
    fn tail_lines_missing_file_is_err() {
        // Missing log → Err, so callers can degrade gracefully (the
        // dashboard renders "(no log yet)" rather than blank).
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("nope.log");
        assert!(tail_lines(&missing, 10).is_err());
    }
}
