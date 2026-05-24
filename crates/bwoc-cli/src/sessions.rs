//! `bwoc sessions` — discover and monitor agent sessions across backends.
//!
//! Two data sources are merged into a unified session list:
//!
//! ## Markers (primary source)
//!
//! When `bwoc spawn` launches a backend, it writes a session marker at
//! `<workspace>/.bwoc/sessions/<agentId>.json`.  This module reads every
//! `*.json` marker and validates pid liveness via `libc::kill(pid, 0)`.
//!
//! - Alive pid → `running`, source `marker`
//! - Dead pid  → `stale`, source `marker`; best-effort deletion of stale file
//!
//! ## Scan fallback (heuristic)
//!
//! For sessions without a live marker the module shells out (via a
//! `ScanRunner` trait seam, mirroring `run.rs`'s `CommandRunner`) to
//! `pgrep -l <name>` / `pgrep <name>` and looks for processes matching
//! backend CLI program names.  The scan result is heuristic: no agentId
//! can be inferred from process names alone, so those entries appear with
//! `agentId: null`.
//!
//! ## Backend → process-name mapping
//!
//! Kept in one place: `BACKEND_PROCESSES`.  Adding a new backend is one
//! entry in that slice.
//!
//! ```text
//! // TODO(extension): to add per-backend custom detection (e.g. port-scan
//! // for a local model server, socket probe for bwoc-harness), implement
//! // BackendDetector trait and register it alongside BACKEND_PROCESSES.
//! ```
//!
//! ## Output
//!
//! Pretty table (default) or JSON (`--json`):
//! ```json
//! {
//!   "sessions": [
//!     {
//!       "backend": "claude",
//!       "agentId": "agent-oracle",
//!       "pid": 12345,
//!       "state": "running",
//!       "source": "marker",
//!       "startedAt": "2026-05-24T10:00:00Z",
//!       "tmux": null
//!     }
//!   ]
//! }
//! ```

use std::path::{Path, PathBuf};

// ── Backend → process-name catalog ───────────────────────────────────────────

/// One entry per known backend: (backend_display_name, process_name_on_PATH).
///
/// `process_name` is the basename of the executable `pgrep` will match.
/// For `ollama`/`bwoc-harness`, two names cover both.
///
/// // TODO(extension): to add a per-backend custom detector (socket probe,
/// // port scan, etc.) implement a `BackendDetector` trait and register
/// // one instance per backend alongside this table.
static BACKEND_PROCESSES: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("agy", "agy"),
    ("codex", "codex"),
    ("kimi", "kimi"),
    ("ollama", "ollama"),
    ("ollama", "bwoc-harness"),
];

// ── Session state types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    Running,
    Stale,
}

impl SessionState {
    fn as_str(&self) -> &'static str {
        match self {
            SessionState::Running => "running",
            SessionState::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSource {
    Marker,
    Scan,
}

impl SessionSource {
    fn as_str(&self) -> &'static str {
        match self {
            SessionSource::Marker => "marker",
            SessionSource::Scan => "scan",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub backend: String,
    pub agent_id: Option<String>,
    pub pid: u32,
    pub state: SessionState,
    pub source: SessionSource,
    pub started_at: Option<String>,
    pub tmux: Option<String>,
}

// ── Marker file schema ────────────────────────────────────────────────────────

/// Schema written by `bwoc spawn` to `.bwoc/sessions/<agentId>.json`.
#[derive(Debug)]
pub struct SessionMarker {
    pub agent_id: String,
    pub backend: String,
    pub pid: u32,
    pub started_at: String,
    pub tmux: Option<String>,
}

impl SessionMarker {
    /// Serialize to JSON (hand-rolled — dep-lean).
    pub fn to_json(&self) -> String {
        let agent_id = json_escape(&self.agent_id);
        let backend = json_escape(&self.backend);
        let started_at = json_escape(&self.started_at);
        let tmux_field = match &self.tmux {
            Some(t) => format!("\"{}\"", json_escape(t)),
            None => "null".to_string(),
        };
        format!(
            "{{\n  \"agentId\": \"{agent_id}\",\n  \"backend\": \"{backend}\",\
            \n  \"pid\": {},\n  \"startedAt\": \"{started_at}\",\n  \"tmux\": {tmux_field}\n}}",
            self.pid
        )
    }

    /// Parse from JSON string. Best-effort: returns None on any parse failure.
    pub fn from_json(s: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        let agent_id = v.get("agentId")?.as_str()?.to_string();
        let backend = v.get("backend")?.as_str()?.to_string();
        let pid = v.get("pid")?.as_u64()? as u32;
        let started_at = v.get("startedAt")?.as_str()?.to_string();
        let tmux = v
            .get("tmux")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        Some(Self {
            agent_id,
            backend,
            pid,
            started_at,
            tmux,
        })
    }
}

// ── ScanRunner seam (mirrors run.rs's CommandRunner) ─────────────────────────

/// Result of a `pgrep` scan invocation.
pub struct ScanOutcome {
    /// Lines of output, each "pid[ name]".
    pub lines: Vec<String>,
}

/// Abstraction over the scan shell-out. `ProcessScanRunner` in production;
/// `MockScanRunner` in unit tests.
pub trait ScanRunner {
    /// Run `pgrep` (or equivalent) for `process_name`. Returns pid lines.
    /// Failures (not found, no matches) return an empty Vec — never Err.
    fn scan_pids(&self, process_name: &str) -> ScanOutcome;
}

/// Production scan runner — shells out to `pgrep`.
pub struct ProcessScanRunner;

impl ScanRunner for ProcessScanRunner {
    fn scan_pids(&self, process_name: &str) -> ScanOutcome {
        // `pgrep -x <name>` matches exact process name (not substring).
        // On macOS and Linux. Falls back to empty on error/not-found.
        let result = std::process::Command::new("pgrep")
            .args(["-x", process_name])
            .output();
        let lines = match result {
            Ok(out) if out.status.success() || out.status.code() == Some(1) => {
                // exit 1 = no matches (not an error)
                String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.to_string())
                    .collect()
            }
            _ => Vec::new(),
        };
        ScanOutcome { lines }
    }
}

// ── Pid liveness ─────────────────────────────────────────────────────────────

/// True iff the process with `pid` is alive and signal-reachable.
/// Uses `libc::kill(pid, 0)` — the standard Unix liveness probe.
/// Always false on non-Unix (no `sysinfo`/`procfs` needed).
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) has no side effects — probe only.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn pid_alive(_pid: u32) -> bool {
    false
}

// ── Workspace resolution ──────────────────────────────────────────────────────

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

// ── Marker I/O ────────────────────────────────────────────────────────────────

/// Directory where session markers live.
pub fn sessions_dir(workspace: &Path) -> PathBuf {
    workspace.join(".bwoc/sessions")
}

/// Write a session marker. Best-effort: never panics or propagates errors.
pub fn write_marker(workspace: &Path, marker: &SessionMarker) {
    let dir = sessions_dir(workspace);
    // Silently create the directory if needed.
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", marker.agent_id));
    let _ = std::fs::write(path, marker.to_json());
}

/// Remove a session marker. Best-effort.
pub fn remove_marker(workspace: &Path, agent_id: &str) {
    let path = sessions_dir(workspace).join(format!("{agent_id}.json"));
    let _ = std::fs::remove_file(path);
}

/// Read all markers from `.bwoc/sessions/*.json`. Returns (marker, path) pairs.
fn read_markers(workspace: &Path) -> Vec<(SessionMarker, PathBuf)> {
    let dir = sessions_dir(workspace);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(m) = SessionMarker::from_json(&content) {
            out.push((m, path));
        }
    }
    out
}

// ── Core logic ────────────────────────────────────────────────────────────────

/// Collect sessions from markers + scan fallback.
///
/// 1. Read all markers; validate pid liveness.
///    - Alive → `running/marker`; Dead → `stale/marker` + best-effort remove.
/// 2. Collect pids seen in live markers (to skip them in scan).
/// 3. For each backend process name, scan via `runner`; skip pids already
///    accounted for by a marker.
pub fn collect_sessions(workspace: &Path, runner: &dyn ScanRunner) -> Vec<Session> {
    let mut sessions: Vec<Session> = Vec::new();
    let mut live_marker_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    // ── Phase 1: markers ─────────────────────────────────────────────────────
    for (marker, path) in read_markers(workspace) {
        if pid_alive(marker.pid) {
            live_marker_pids.insert(marker.pid);
            sessions.push(Session {
                backend: marker.backend,
                agent_id: Some(marker.agent_id),
                pid: marker.pid,
                state: SessionState::Running,
                source: SessionSource::Marker,
                started_at: Some(marker.started_at),
                tmux: marker.tmux,
            });
        } else {
            // Stale — best-effort cleanup.
            let _ = std::fs::remove_file(&path);
            sessions.push(Session {
                backend: marker.backend,
                agent_id: Some(marker.agent_id),
                pid: marker.pid,
                state: SessionState::Stale,
                source: SessionSource::Marker,
                started_at: Some(marker.started_at),
                tmux: marker.tmux,
            });
        }
    }

    // ── Phase 2: scan fallback ────────────────────────────────────────────────
    // De-dup process names to avoid double-scanning when two entries share a name.
    let mut scanned_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &(backend_name, process_name) in BACKEND_PROCESSES {
        if !scanned_names.insert(process_name) {
            continue; // already scanned this executable name
        }
        let outcome = runner.scan_pids(process_name);
        for line in &outcome.lines {
            let pid: u32 = match line.split_whitespace().next() {
                Some(s) => match s.parse() {
                    Ok(n) => n,
                    Err(_) => continue,
                },
                None => continue,
            };
            // Skip pids already covered by a live marker.
            if live_marker_pids.contains(&pid) {
                continue;
            }
            // Verify pid is alive (scan output may lag real process state).
            if !pid_alive(pid) {
                continue;
            }
            sessions.push(Session {
                backend: backend_name.to_string(),
                agent_id: None,
                pid,
                state: SessionState::Running,
                source: SessionSource::Scan,
                started_at: None,
                tmux: None,
            });
        }
    }

    sessions
}

// ── Public args + entry points ────────────────────────────────────────────────

pub struct SessionsArgs {
    pub workspace: Option<PathBuf>,
    pub json: bool,
}

/// Entry point called from `main.rs`.
pub fn run(args: SessionsArgs) -> i32 {
    run_with(args, &ProcessScanRunner)
}

/// Testable entry point accepting a `ScanRunner` impl.
pub fn run_with(args: SessionsArgs, runner: &dyn ScanRunner) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc sessions: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };

    let sessions = collect_sessions(&workspace, runner);

    if args.json {
        emit_json(&sessions)
    } else {
        emit_table(&sessions)
    }
}

fn emit_table(sessions: &[Session]) -> i32 {
    println!();
    if sessions.is_empty() {
        println!("No active or stale agent sessions detected.");
        println!();
        return 0;
    }
    println!(
        "{:<14} {:<24} {:<8} {:<9} {:<8}",
        "BACKEND", "AGENT", "PID", "STATE", "SOURCE"
    );
    println!(
        "{:<14} {:<24} {:<8} {:<9} {:<8}",
        "─".repeat(14),
        "─".repeat(24),
        "─".repeat(8),
        "─".repeat(9),
        "─".repeat(8),
    );
    for s in sessions {
        let agent = s.agent_id.as_deref().unwrap_or("—");
        let state_mark = match s.state {
            SessionState::Running => "●",
            SessionState::Stale => "○",
        };
        println!(
            "{:<14} {:<24} {:<8} {}{:<8} {:<8}",
            s.backend,
            agent,
            s.pid,
            state_mark,
            s.state.as_str(),
            s.source.as_str(),
        );
    }
    println!();
    0
}

fn emit_json(sessions: &[Session]) -> i32 {
    let arr: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "backend": s.backend,
                "agentId": s.agent_id,
                "pid": s.pid,
                "state": s.state.as_str(),
                "source": s.source.as_str(),
                "startedAt": s.started_at,
                "tmux": s.tmux,
            })
        })
        .collect();
    let value = serde_json::json!({ "sessions": arr });
    match serde_json::to_string_pretty(&value) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc sessions: failed to serialize JSON: {e}");
            1
        }
    }
}

// ── Minimal JSON string escaping ──────────────────────────────────────────────

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // ── Mock ScanRunner ───────────────────────────────────────────────────────

    struct MockScanRunner {
        /// Map process_name → list of pid strings to return.
        responses: std::collections::HashMap<String, Vec<String>>,
        /// Capture which names were scanned.
        scanned: RefCell<Vec<String>>,
    }

    impl MockScanRunner {
        fn new(responses: &[(&str, &[u32])]) -> Self {
            let mut map = std::collections::HashMap::new();
            for &(name, pids) in responses {
                map.insert(
                    name.to_string(),
                    pids.iter().map(|p| p.to_string()).collect(),
                );
            }
            Self {
                responses: map,
                scanned: RefCell::new(Vec::new()),
            }
        }

        fn empty() -> Self {
            Self::new(&[])
        }
    }

    impl ScanRunner for MockScanRunner {
        fn scan_pids(&self, process_name: &str) -> ScanOutcome {
            self.scanned.borrow_mut().push(process_name.to_string());
            let lines = self
                .responses
                .get(process_name)
                .cloned()
                .unwrap_or_default();
            ScanOutcome { lines }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_workspace() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".bwoc/sessions")).unwrap();
        std::fs::write(
            root.join(".bwoc/workspace.toml"),
            "[workspace]\nname = 'test'\nversion = '0.1'\ncreated = '2026-01-01'\n",
        )
        .unwrap();
        dir
    }

    fn current_pid() -> u32 {
        std::process::id()
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// (a) Live marker: current process pid → listed as `running/marker`.
    // pid-liveness (`libc::kill`) is unix-only — the "running" state can't be
    // exercised on non-unix (Windows stubs it false). Session-monitor is unix-first.
    #[cfg(unix)]
    #[test]
    fn live_marker_is_running() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        let marker = SessionMarker {
            agent_id: "agent-test".to_string(),
            backend: "claude".to_string(),
            pid,
            started_at: "2026-05-24T10:00:00Z".to_string(),
            tmux: None,
        };
        write_marker(root, &marker);

        let runner = MockScanRunner::empty();
        let sessions = collect_sessions(root, &runner);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Running);
        assert_eq!(sessions[0].source, SessionSource::Marker);
        assert_eq!(sessions[0].agent_id.as_deref(), Some("agent-test"));
        assert_eq!(sessions[0].backend, "claude");
        assert_eq!(sessions[0].pid, pid);
    }

    /// (b) Marker with bogus dead pid → listed as `stale/marker`.
    #[test]
    fn stale_marker_dead_pid() {
        let dir = make_workspace();
        let root = dir.path();

        // PID 999_999 is extremely unlikely to exist.
        let dead_pid: u32 = 999_999;

        let marker = SessionMarker {
            agent_id: "agent-ghost".to_string(),
            backend: "codex".to_string(),
            pid: dead_pid,
            started_at: "2026-05-24T09:00:00Z".to_string(),
            tmux: None,
        };
        write_marker(root, &marker);

        let runner = MockScanRunner::empty();
        let sessions = collect_sessions(root, &runner);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Stale);
        assert_eq!(sessions[0].source, SessionSource::Marker);
        assert_eq!(sessions[0].agent_id.as_deref(), Some("agent-ghost"));

        // Stale marker file should have been deleted.
        let marker_path = sessions_dir(root).join("agent-ghost.json");
        assert!(!marker_path.exists(), "stale marker should be cleaned up");
    }

    /// (c) Scan via mock runner surfaces an unmarked backend process.
    #[cfg(unix)]
    #[test]
    fn scan_surfaces_unmarked_process() {
        let dir = make_workspace();
        let root = dir.path();

        // Use current process pid so kill(pid, 0) returns alive.
        let pid = current_pid();
        let runner = MockScanRunner::new(&[("claude", &[pid])]);
        let sessions = collect_sessions(root, &runner);

        // Should find exactly one scan entry with no agentId.
        let scan_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.source == SessionSource::Scan)
            .collect();
        assert_eq!(scan_sessions.len(), 1);
        assert_eq!(scan_sessions[0].state, SessionState::Running);
        assert_eq!(scan_sessions[0].backend, "claude");
        assert!(scan_sessions[0].agent_id.is_none());
        assert_eq!(scan_sessions[0].pid, pid);
    }

    /// Scan entry is suppressed when the same pid is already in a live marker.
    #[cfg(unix)]
    #[test]
    fn scan_skips_pid_already_in_live_marker() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        // Write a live marker for this pid.
        let marker = SessionMarker {
            agent_id: "agent-alpha".to_string(),
            backend: "claude".to_string(),
            pid,
            started_at: "2026-05-24T10:00:00Z".to_string(),
            tmux: None,
        };
        write_marker(root, &marker);

        // Scan also returns the same pid for "claude".
        let runner = MockScanRunner::new(&[("claude", &[pid])]);
        let sessions = collect_sessions(root, &runner);

        // Should be exactly one entry (from marker), not two.
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].source, SessionSource::Marker);
    }

    /// (d) JSON output shape: all required keys present.
    // Builds a live (running) marker → unix-only (see `live_marker_is_running`).
    #[cfg(unix)]
    #[test]
    fn json_shape_contains_required_keys() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        let marker = SessionMarker {
            agent_id: "agent-foo".to_string(),
            backend: "claude".to_string(),
            pid,
            started_at: "2026-05-24T10:00:00Z".to_string(),
            tmux: Some("bwoc:0.0".to_string()),
        };
        write_marker(root, &marker);

        let runner = MockScanRunner::empty();
        let sessions = collect_sessions(root, &runner);

        let arr: Vec<serde_json::Value> = sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "backend": s.backend,
                    "agentId": s.agent_id,
                    "pid": s.pid,
                    "state": s.state.as_str(),
                    "source": s.source.as_str(),
                    "startedAt": s.started_at,
                    "tmux": s.tmux,
                })
            })
            .collect();
        let json = serde_json::json!({ "sessions": arr });
        let text = serde_json::to_string_pretty(&json).unwrap();

        // Top-level shape.
        assert!(text.contains(r#""sessions""#));
        // Per-session required keys.
        assert!(text.contains(r#""backend""#));
        assert!(text.contains(r#""agentId""#));
        assert!(text.contains(r#""pid""#));
        assert!(text.contains(r#""state""#));
        assert!(text.contains(r#""source""#));
        assert!(text.contains(r#""startedAt""#));
        assert!(text.contains(r#""tmux""#));
        // Value spot-checks.
        assert!(text.contains("agent-foo"));
        assert!(text.contains("running"));
        assert!(text.contains("marker"));
        assert!(text.contains("bwoc:0.0"));
    }

    /// Marker round-trip: to_json() → from_json() preserves all fields.
    #[test]
    fn marker_round_trip() {
        let m = SessionMarker {
            agent_id: "agent-oracle".to_string(),
            backend: "agy".to_string(),
            pid: 42,
            started_at: "2026-05-24T12:34:56Z".to_string(),
            tmux: Some("main:1.2".to_string()),
        };
        let json = m.to_json();
        let parsed = SessionMarker::from_json(&json).unwrap();
        assert_eq!(parsed.agent_id, "agent-oracle");
        assert_eq!(parsed.backend, "agy");
        assert_eq!(parsed.pid, 42);
        assert_eq!(parsed.started_at, "2026-05-24T12:34:56Z");
        assert_eq!(parsed.tmux.as_deref(), Some("main:1.2"));
    }

    #[test]
    fn marker_round_trip_null_tmux() {
        let m = SessionMarker {
            agent_id: "agent-pi".to_string(),
            backend: "ollama".to_string(),
            pid: 7,
            started_at: "2026-05-24T00:00:00Z".to_string(),
            tmux: None,
        };
        let json = m.to_json();
        let parsed = SessionMarker::from_json(&json).unwrap();
        assert!(parsed.tmux.is_none());
    }

    #[test]
    fn from_json_returns_none_on_garbage() {
        assert!(SessionMarker::from_json("not json at all").is_none());
        assert!(SessionMarker::from_json("{}").is_none()); // missing required fields
    }
}
