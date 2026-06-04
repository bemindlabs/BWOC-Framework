//! Shared liveness + inbox helpers.
//!
//! Consolidates what previously lived as 5 near-identical copies across
//! `status.rs`, `doctor.rs`, `workspace.rs`, `dashboard.rs`, and `start.rs`:
//!
//!   - `signal_zero_alive(pid)` — Unix signal-0 liveness probe
//!   - `running_pid(root, agent)` — PID file + signal-0
//!   - `query_uptime(root, agent)` — STATUS over the agent's Unix socket
//!   - `format_uptime(secs)` — short "42s" / "5m12s" / "3h07m" / "2d04h"
//!   - `inbox_count(root, agent)` — count of complete envelopes in inbox.jsonl
//!
//! All helpers are read-only and side-effect free (except `query_uptime`
//! which opens a Unix socket — bounded by a 300ms timeout). No new
//! dependencies — uses `libc` (already in bwoc-cli deps).

use std::path::Path;

use bwoc_core::workspace::AgentEntry;

/// Send signal 0 to `pid` — true iff the process exists and is signal-
/// reachable from this process. The standard Unix liveness test.
/// Conservatively false on Windows until Phase 2 named-pipe supervision.
#[cfg(unix)]
pub fn signal_zero_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) is a syscall with no side effects beyond
    // returning an errno; pid is u32 cast to libc::pid_t, signal=0.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Windows liveness: `tasklist /FI "PID eq N" /NH` prints the process row iff
/// the pid is alive ("INFO: No tasks…" otherwise). A shell-out keeps the
/// Windows path dependency-free; callers hit this at human cadence (status /
/// dashboard ticks), not in tight loops.
#[cfg(windows)]
pub fn signal_zero_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!(" {pid} ")))
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
pub fn signal_zero_alive(_pid: u32) -> bool {
    false
}

/// Windows IPC client: one line-request / one line-reply over the agent's
/// named pipe. The pipe name derives deterministically from the agent dir
/// (`bwoc_core::ipc::pipe_name`) — same derivation the daemon uses, so no
/// rendezvous file is needed. `None` on connect/IO failure. The sync pipe
/// stream has no read-timeout API, so a hung-but-connected daemon blocks the
/// caller — accepted for the v1 Windows path (the daemon replies in
/// microseconds when healthy).
#[cfg(windows)]
pub(crate) fn pipe_request(agent_dir: &Path, cmd: &str) -> Option<String> {
    use interprocess::local_socket::{GenericNamespaced, Stream, prelude::*};
    use std::io::{BufRead, BufReader, Write};

    let name = bwoc_core::ipc::pipe_name(agent_dir)
        .to_ns_name::<GenericNamespaced>()
        .ok()?;
    let mut stream = Stream::connect(name).ok()?;
    stream.write_all(format!("{cmd}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(&mut stream).read_line(&mut line).ok()?;
    Some(line.trim().to_string())
}

/// Read `<agent>/.bwoc/agent.pid` and return the pid iff it's alive.
/// Stale pid files (file present but process gone) return None — `bwoc
/// doctor --auto` sweeps them separately.
pub fn running_pid(root: &Path, a: &AgentEntry) -> Option<u32> {
    let pid_path = root.join(&a.path).join(".bwoc/agent.pid");
    let raw = std::fs::read_to_string(&pid_path).ok()?;
    let pid: u32 = raw.trim().parse().ok()?;
    if signal_zero_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

/// Query the agent daemon's STATUS command via Unix socket. Returns
/// Some(uptime_secs) on a valid reply, None when the socket is missing
/// or the daemon doesn't answer. Bounded by 300ms timeout so a hung
/// daemon can't slow callers.
#[cfg(unix)]
pub fn query_uptime(root: &Path, a: &AgentEntry) -> Option<u64> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let sock_path = root.join(&a.path).join(".bwoc/agent.sock");
    if !sock_path.exists() {
        return None;
    }
    let mut stream = UnixStream::connect(&sock_path).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    stream.write_all(b"STATUS\n").ok()?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    // Expected reply: `OK uptime_secs=<N> pid=<N>\n`
    for token in line.split_whitespace() {
        if let Some(rest) = token.strip_prefix("uptime_secs=") {
            return rest.parse().ok();
        }
    }
    None
}

/// Windows: STATUS over the agent's named pipe (see [`pipe_request`]).
#[cfg(windows)]
pub fn query_uptime(root: &Path, a: &AgentEntry) -> Option<u64> {
    let line = pipe_request(&root.join(&a.path), "STATUS")?;
    // Expected reply: `OK uptime_secs=<N> pid=<N>`
    for token in line.split_whitespace() {
        if let Some(rest) = token.strip_prefix("uptime_secs=") {
            return rest.parse().ok();
        }
    }
    None
}

#[cfg(not(any(unix, windows)))]
pub fn query_uptime(_root: &Path, _a: &AgentEntry) -> Option<u64> {
    None
}

/// Format seconds as a short human term:
///   `< 1m`  → "42s"
///   `< 1h`  → "5m12s"
///   `< 1d`  → "3h07m"
///   `>= 1d` → "2d04h"
///
/// Keeps status lines one column-length even at long uptimes.
pub fn format_uptime(secs: u64) -> String {
    let (d, rem) = (secs / 86400, secs % 86400);
    let (h, rem) = (rem / 3600, rem % 3600);
    let (m, s) = (rem / 60, rem % 60);
    if d > 0 {
        format!("{d}d{h:02}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Count complete envelope lines in `<agent>/.bwoc/inbox.jsonl`. Returns
/// 0 when the file is missing or unreadable — same shape as a real
/// empty inbox, which keeps callers simple.
pub fn inbox_count(root: &Path, a: &AgentEntry) -> usize {
    let path = root.join(&a.path).join(".bwoc/inbox.jsonl");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return 0;
    };
    content.lines().filter(|l| !l.trim().is_empty()).count()
}

/// Count refusal records in the trust sidecar and return the most recent
/// one's `(reason, envelopeFrom)` fields. Best-effort: absent or malformed
/// sidecar returns `(0, None)`. Never loads the full inbox — only the
/// refusals file.
///
/// Used by the dashboard detail pane to show "Refused: N" without re-reading
/// all envelopes.
pub fn refusal_summary(root: &Path, a: &AgentEntry) -> (usize, Option<(String, String)>) {
    let path = root.join(&a.path).join(".bwoc/inbox.refusals.jsonl");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return (0, None);
    };

    let mut count: usize = 0;
    let mut latest: Option<(String, String, String)> = None; // (ts, reason, from)

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        count += 1;
        let ts = v
            .get("ts")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let reason = v
            .get("reason")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown")
            .to_string();
        let from = v
            .get("envelopeFrom")
            .and_then(|x| x.as_str())
            .unwrap_or("?")
            .to_string();
        match &latest {
            None => latest = Some((ts, reason, from)),
            Some((prev_ts, _, _)) if ts > *prev_ts => latest = Some((ts, reason, from)),
            _ => {}
        }
    }

    let detail = latest.map(|(_, reason, from)| (reason, from));
    (count, detail)
}

/// Count `.md` files in a directory, excluding template scaffolding
/// (`SPEC.md`, `README.md`). Returns 0 when the directory doesn't
/// exist or isn't readable — same shape as an empty dir.
///
/// Used for the per-agent mindsets/skills/memories counts and the
/// workspace-level projects/notes counts in the dashboard.
pub fn count_user_md_files(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.ends_with(".md") && s != "SPEC.md" && s != "README.md"
        })
        .count()
}

/// Count direct subdirectories of a workspace-level dir (e.g. `projects/`
/// for project subprojects, `notes/` for date-stamped subdirs if used).
/// Returns 0 on missing/unreadable.
pub fn count_subdirs(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .count()
}

/// Saṅgha task summary for one team, from a given agent's perspective.
pub struct AgentTeamSummary {
    pub team: String,
    /// In-progress tasks this agent has claimed.
    pub claimed_by_me: usize,
    /// Pending tasks whose dependencies are all completed (claimable now).
    pub available: usize,
    pub total: usize,
}

/// For each team in `<root>/.bwoc/teams/*.toml` that `agent_id` belongs to,
/// summarize the shared task list from that agent's perspective. Read-only;
/// silently skips any missing/unreadable/malformed file (returns whatever
/// it could read). Cheap enough to call on every dashboard draw — teams are
/// few and task files are small.
pub fn agent_team_summaries(root: &Path, agent_id: &str) -> Vec<AgentTeamSummary> {
    use bwoc_core::team::{self, TaskState, Team};

    let dir = root.join(".bwoc/teams");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(team) = Team::from_toml(&body) else {
            continue;
        };
        if !team.has_member(agent_id) {
            continue;
        }
        let tasks = std::fs::read_to_string(dir.join(&team.id).join("tasks.jsonl"))
            .ok()
            .and_then(|b| team::parse_tasks(&b).ok())
            .unwrap_or_default();
        let total = tasks.len();
        let claimed_by_me = tasks
            .iter()
            .filter(|t| {
                t.state == TaskState::InProgress && t.claimed_by.as_deref() == Some(agent_id)
            })
            .count();
        let available = tasks
            .iter()
            .filter(|t| {
                t.state == TaskState::Pending
                    && t.deps.iter().all(|d| {
                        tasks
                            .iter()
                            .any(|x| &x.id == d && x.state == TaskState::Completed)
                    })
            })
            .count();
        out.push(AgentTeamSummary {
            team: team.id,
            claimed_by_me,
            available,
            total,
        });
    }
    out.sort_by(|a, b| a.team.cmp(&b.team));
    out
}

/// Pad `text` with trailing spaces to `width` *visual columns*, not bytes.
/// `{:<N}` in Rust pads by byte count which misaligns CJK and Thai output;
/// this helper uses unicode-width to compute the actual display width.
///
/// If `text` is already ≥ `width` visual columns, returns the text
/// unchanged (no truncation — column overflow is preferable to losing
/// data, matches Rust's own `{:<N}` overflow behavior).
pub fn pad_visual(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let cur = text.width();
    if cur >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - cur))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_team_summaries_counts_mine_available_total() {
        // Temp workspace with one team `squad` (members pi + oracle) and
        // three tasks: t1 completed, t2 in_progress claimed by pi, t3
        // pending blocked by... nothing (claimable).
        let root = std::env::temp_dir().join(format!("bwoc-team-sum-{}", std::process::id()));
        let teams = root.join(".bwoc/teams");
        std::fs::create_dir_all(teams.join("squad")).unwrap();
        std::fs::write(
            teams.join("squad.toml"),
            "id = \"squad\"\nmembers = [\"agent-pi\", \"agent-oracle\"]\ncreated_at = \"2026-05-23T00:00:00Z\"\n",
        )
        .unwrap();
        let tasks = "\
{\"id\":\"t1\",\"title\":\"a\",\"state\":\"completed\",\"created_at\":\"x\",\"claimed_by\":\"agent-pi\",\"completed_at\":\"y\"}
{\"id\":\"t2\",\"title\":\"b\",\"state\":\"in_progress\",\"created_at\":\"x\",\"claimed_by\":\"agent-pi\"}
{\"id\":\"t3\",\"title\":\"c\",\"state\":\"pending\",\"created_at\":\"x\"}
";
        std::fs::write(teams.join("squad/tasks.jsonl"), tasks).unwrap();

        let pi = agent_team_summaries(&root, "agent-pi");
        assert_eq!(pi.len(), 1);
        assert_eq!(pi[0].team, "squad");
        assert_eq!(pi[0].claimed_by_me, 1); // t2
        assert_eq!(pi[0].available, 1); // t3 (pending, no deps)
        assert_eq!(pi[0].total, 3);

        // oracle is a member but claimed nothing.
        let oracle = agent_team_summaries(&root, "agent-oracle");
        assert_eq!(oracle[0].claimed_by_me, 0);
        assert_eq!(oracle[0].available, 1);

        // non-member sees no teams.
        assert!(agent_team_summaries(&root, "agent-ghost").is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn format_uptime_under_a_minute() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(42), "42s");
        assert_eq!(format_uptime(59), "59s");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(60), "1m00s");
        assert_eq!(format_uptime(312), "5m12s");
        assert_eq!(format_uptime(3599), "59m59s");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(3600), "1h00m");
        assert_eq!(format_uptime(11220), "3h07m");
        assert_eq!(format_uptime(86399), "23h59m");
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(format_uptime(86400), "1d00h");
        assert_eq!(format_uptime(187200), "2d04h");
    }

    #[cfg(unix)]
    #[test]
    fn signal_zero_on_self_returns_true() {
        let pid = std::process::id();
        assert!(signal_zero_alive(pid));
    }

    #[cfg(unix)]
    #[test]
    fn signal_zero_on_unlikely_pid_returns_false() {
        // PID 1 is init/launchd — alive but not signalable by non-root
        // (so kill returns -1 with EPERM, not 0). Test with a clearly
        // unused high PID instead — system limit is typically <100000.
        assert!(!signal_zero_alive(999_999));
    }
}
