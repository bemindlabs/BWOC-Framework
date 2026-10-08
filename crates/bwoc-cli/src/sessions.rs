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
//! ## Activity (v2)
//!
//! For alive sessions the module derives a `last_activity` (epoch-seconds)
//! and refines the state:
//!
//! - tmux pane present → query `#{window_activity}` via `tmux display-message`
//! - no tmux, marker source → use marker file mtime as a proxy
//! - no tmux, scan source  → `last_activity = None`
//!
//! State derivation (checked in order):
//! 1. dead pid                                  → `stale`
//! 2. alive + `last_activity` ≤ `idle_secs` ago → `working`
//! 3. alive + `last_activity` >  `idle_secs` ago → `idle`
//! 4. alive + no `last_activity`               → `running`  (unknown)
//!
//! Any tmux/mtime failure is best-effort: `last_activity = None` → `running`.
//!
//! ## herdr state provider (opt-in)
//!
//! With `[integrations.herdr] enabled = true` in `.bwoc/workspace.toml`, one
//! `agent.list` round-trip to a running herdr server (see [`crate::herdr`])
//! refines the state of **alive marker sessions** after the heuristic above:
//!
//! 1. dead pid → `stale`, untouched (BWOC's own liveness check wins);
//! 2. a herdr agent whose `foreground_cwd` is (inside) the agent's directory
//!    is that agent's;
//! 3. else, for marker agents still unmatched, a herdr pane whose
//!    `pane.process_info` foreground pids include the marker pid;
//! 4. else the herdr agent is not BWOC's and is ignored.
//!
//! A matched herdr status overrides the heuristic: `working|blocked|done|idle`
//! map 1:1, `unknown` → `running`. Any herdr failure leaves the heuristic
//! state in place; all herdr calls share one ~300 ms budget.
//!
//! `stateSource` (JSON) names what decided the state: `"herdr"`, `"tmux"`
//! (`#{window_activity}`), `"marker"` (marker-file mtime), or `null` when no
//! activity signal was used (stale and scan sessions, or an alive marker
//! whose mtime could not be read).
//!
//! ## Backend → process-name mapping
//!
//! Kept in one place: `BACKEND_PROCESSES`.  Adding a new backend is one
//! entry in that slice.
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
//!       "state": "working",
//!       "source": "marker",
//!       "startedAt": "2026-05-24T10:00:00Z",
//!       "tmux": "bwoc:0.0",
//!       "lastActivity": 1716545000,
//!       "stateSource": "tmux"
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
/// Richer, non-process detection (herdr's per-pane agent state) is layered on
/// top in [`collect_sessions`] rather than per backend here.
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
    /// Alive + last_activity within idle_secs threshold.
    Working,
    /// Alive + last_activity older than idle_secs threshold.
    Idle,
    /// Alive + no activity signal available (or herdr reports `unknown`).
    Running,
    /// Pid dead.
    Stale,
    /// Alive + herdr reports the agent waiting on the operator (only herdr
    /// can tell; the tmux heuristic never yields it).
    Blocked,
    /// Alive + herdr reports finished work not yet looked at (herdr-only).
    Done,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Working => "working",
            SessionState::Idle => "idle",
            SessionState::Running => "running",
            SessionState::Stale => "stale",
            SessionState::Blocked => "blocked",
            SessionState::Done => "done",
        }
    }

    fn from_herdr(status: crate::herdr::AgentStatus) -> Self {
        use crate::herdr::AgentStatus;
        match status {
            AgentStatus::Working => SessionState::Working,
            AgentStatus::Blocked => SessionState::Blocked,
            AgentStatus::Done => SessionState::Done,
            AgentStatus::Idle => SessionState::Idle,
            AgentStatus::Unknown => SessionState::Running,
        }
    }
}

/// What decided a session's state — `stateSource` in `--json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateSource {
    /// herdr's per-pane agent status.
    Herdr,
    /// tmux `#{window_activity}` age.
    Tmux,
    /// Marker-file mtime age.
    Marker,
}

impl StateSource {
    fn as_str(self) -> &'static str {
        match self {
            StateSource::Herdr => "herdr",
            StateSource::Tmux => "tmux",
            StateSource::Marker => "marker",
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
    /// Epoch-seconds of last observed activity.  None = unknown.
    pub last_activity: Option<u64>,
    /// What decided `state`; `None` = no activity signal (stale / scan).
    pub state_source: Option<StateSource>,
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

    /// Query the epoch-seconds of last activity for a tmux pane target
    /// (e.g. `"bwoc:0.0"`).  Returns `None` on any failure — best-effort only.
    ///
    /// Default implementation shells out to
    /// `tmux display-message -p -t <pane> '#{window_activity}'`.
    fn tmux_pane_activity(&self, pane: &str) -> Option<u64> {
        let out = std::process::Command::new("tmux")
            .args(["display-message", "-p", "-t", pane, "#{window_activity}"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout);
        s.trim().parse::<u64>().ok()
    }
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
    // tmux_pane_activity uses the default implementation (shells out to tmux).
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

// ── Wall-clock helper (std-only, no chrono) ───────────────────────────────────

/// Current time as epoch-seconds (u64).  Uses `std::time::SystemTime`.
fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// mtime of a file as epoch-seconds.  Returns `None` on any error.
fn file_mtime_secs(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

// ── Activity derivation ───────────────────────────────────────────────────────

/// Derive `last_activity` (epoch-seconds) for an alive session.
///
/// Priority:
/// 1. tmux pane  → `ScanRunner::tmux_pane_activity`
/// 2. marker file mtime (marker_path is Some)
/// 3. None
///
/// Returns the timestamp together with where it came from.
fn derive_last_activity(
    tmux: Option<&str>,
    marker_path: Option<&Path>,
    runner: &dyn ScanRunner,
) -> (Option<u64>, Option<StateSource>) {
    if let Some(pane) = tmux {
        if let Some(ts) = runner.tmux_pane_activity(pane) {
            return (Some(ts), Some(StateSource::Tmux));
        }
    }
    if let Some(path) = marker_path {
        if let Some(ts) = file_mtime_secs(path) {
            return (Some(ts), Some(StateSource::Marker));
        }
    }
    (None, None)
}

/// Derive `SessionState` for an alive session given `last_activity` and threshold.
fn alive_state(last_activity: Option<u64>, idle_secs: u64) -> SessionState {
    match last_activity {
        None => SessionState::Running,
        Some(ts) => {
            let now = now_epoch_secs();
            let age = now.saturating_sub(ts);
            if age <= idle_secs {
                SessionState::Working
            } else {
                SessionState::Idle
            }
        }
    }
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
///    - Alive → derive activity → state in {working, idle, running}; source `marker`.
///    - Dead  → `stale/marker`; best-effort remove.
/// 2. Collect pids seen in live markers (to skip them in scan).
/// 3. For each backend process name, scan via `runner`; skip pids already
///    accounted for by a marker.
/// 4. When the workspace enables `[integrations.herdr]`, refine alive marker
///    sessions from herdr (see the module docs).
pub fn collect_sessions(workspace: &Path, runner: &dyn ScanRunner, idle_secs: u64) -> Vec<Session> {
    let herdr = bwoc_core::workspace::Integrations::load(workspace).herdr;
    let socket = if herdr.enabled {
        crate::herdr::resolve_socket(herdr.socket.as_deref())
    } else {
        None
    };
    collect_sessions_with_herdr(workspace, runner, idle_secs, socket.as_deref())
}

/// [`collect_sessions`] with the herdr socket decided by the caller
/// (`None` = herdr off; no socket is touched).
fn collect_sessions_with_herdr(
    workspace: &Path,
    runner: &dyn ScanRunner,
    idle_secs: u64,
    herdr_socket: Option<&Path>,
) -> Vec<Session> {
    let mut sessions: Vec<Session> = Vec::new();
    let mut live_marker_pids: std::collections::HashSet<u32> = std::collections::HashSet::new();

    // ── Phase 1: markers ─────────────────────────────────────────────────────
    for (marker, path) in read_markers(workspace) {
        if pid_alive(marker.pid) {
            live_marker_pids.insert(marker.pid);
            let (last_activity, state_source) =
                derive_last_activity(marker.tmux.as_deref(), Some(&path), runner);
            let state = alive_state(last_activity, idle_secs);
            sessions.push(Session {
                backend: marker.backend,
                agent_id: Some(marker.agent_id),
                pid: marker.pid,
                state,
                source: SessionSource::Marker,
                started_at: Some(marker.started_at),
                tmux: marker.tmux,
                last_activity,
                state_source,
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
                last_activity: None,
                state_source: None,
            });
        }
    }

    // ── Phase 1b: herdr refinement (opt-in) ──────────────────────────────────
    if let Some(socket) = herdr_socket {
        apply_herdr_states(workspace, socket, &mut sessions);
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
            // Scan sessions have no tmux and no marker path — last_activity is None.
            sessions.push(Session {
                backend: backend_name.to_string(),
                agent_id: None,
                pid,
                state: SessionState::Running,
                source: SessionSource::Scan,
                started_at: None,
                tmux: None,
                last_activity: None,
                state_source: None,
            });
        }
    }

    sessions
}

// ── herdr matching ────────────────────────────────────────────────────────────

/// Override the state of alive marker sessions with herdr's view, when herdr
/// answers. Stale and scan sessions are never touched. One `agent.list`, plus
/// at most one `pane.process_info` per herdr pane not matched by cwd — and
/// only while some alive marker agent is still unmatched. Every call shares
/// [`crate::herdr::DEFAULT_BUDGET`]; the first failure ends the lookup.
fn apply_herdr_states(workspace: &Path, socket: &Path, sessions: &mut [Session]) {
    let alive: Vec<(String, u32)> = sessions
        .iter()
        .filter(|s| s.source == SessionSource::Marker && s.state != SessionState::Stale)
        .filter_map(|s| Some((s.agent_id.clone()?, s.pid)))
        .collect();
    if alive.is_empty() {
        return; // nothing herdr could refine — do not touch the socket
    }
    let client = crate::herdr::Client::new(socket.to_path_buf(), crate::herdr::DEFAULT_BUDGET);
    let Some(agents) = client.agent_list() else {
        return;
    };

    let dirs = agent_dirs(workspace, alive.iter().map(|(id, _)| id.as_str()));
    let mut matched: std::collections::HashMap<String, SessionState> =
        std::collections::HashMap::new();
    let mut by_pid = Vec::new();
    for a in &agents {
        match a
            .foreground_cwd
            .as_deref()
            .and_then(|c| agent_for_cwd(Path::new(c), &dirs))
        {
            // First herdr agent (in herdr's order) wins for a given BWOC agent.
            Some(id) => {
                matched
                    .entry(id)
                    .or_insert_with(|| SessionState::from_herdr(a.status));
            }
            None => by_pid.push(a),
        }
    }
    for a in by_pid {
        if alive.iter().all(|(id, _)| matched.contains_key(id)) {
            break;
        }
        let Some(pids) = client.pane_foreground_pids(&a.pane_id) else {
            break; // herdr unavailable for the rest of this call
        };
        if let Some((id, _)) = alive
            .iter()
            .find(|(id, pid)| !matched.contains_key(id) && pids.contains(pid))
        {
            matched.insert(id.clone(), SessionState::from_herdr(a.status));
        }
    }

    for s in sessions.iter_mut() {
        if s.source != SessionSource::Marker || s.state == SessionState::Stale {
            continue;
        }
        if let Some(state) = s.agent_id.as_ref().and_then(|id| matched.get(id)) {
            s.state = state.clone();
            s.state_source = Some(StateSource::Herdr);
        }
    }
}

/// Every registered agent's directory (plus `agents/<id>` for marker agents
/// the registry does not list), each also in canonical form so a cwd herdr
/// reports through resolved symlinks (`/private/var/…` on macOS) still
/// matches. A herdr agent inside a registered agent's dir belongs to that
/// agent even when it holds no marker — it is then simply not shown.
pub(crate) fn agent_dirs<'a>(
    workspace: &Path,
    marker_ids: impl Iterator<Item = &'a str>,
) -> Vec<(String, PathBuf)> {
    let mut base: Vec<(String, PathBuf)> = bwoc_core::workspace::AgentsRegistry::load(workspace)
        .map(|r| {
            r.agents
                .iter()
                .map(|e| (e.id.clone(), e.dir(workspace)))
                .collect()
        })
        .unwrap_or_default();
    for id in marker_ids {
        if !base.iter().any(|(known, _)| known == id) {
            base.push((id.to_string(), workspace.join("agents").join(id)));
        }
    }
    let mut out = Vec::with_capacity(base.len() * 2);
    for (id, dir) in base {
        if let Ok(canon) = std::fs::canonicalize(&dir) {
            if canon != dir {
                out.push((id.clone(), canon));
            }
        }
        out.push((id, dir));
    }
    out
}

/// The agent whose directory equals or contains `cwd` (component-wise, so
/// `agents/a` never claims `agents/ab`); the deepest directory wins.
pub(crate) fn agent_for_cwd(cwd: &Path, dirs: &[(String, PathBuf)]) -> Option<String> {
    dirs.iter()
        .filter(|(_, dir)| cwd.starts_with(dir))
        .max_by_key(|(_, dir)| dir.components().count())
        .map(|(id, _)| id.clone())
}

// ── Public args + entry points ────────────────────────────────────────────────

pub struct SessionsArgs {
    pub workspace: Option<PathBuf>,
    pub json: bool,
    /// Seconds of inactivity before a session transitions from `working` to `idle`.
    /// Default: 60.
    pub idle_secs: u64,
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

    let sessions = collect_sessions(&workspace, runner, args.idle_secs);

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
            SessionState::Working => "●",
            SessionState::Idle => "◑",
            SessionState::Running => "●",
            SessionState::Stale => "○",
            SessionState::Blocked => "◆",
            SessionState::Done => "✓",
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
                "lastActivity": s.last_activity,
                "stateSource": s.state_source.map(StateSource::as_str),
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
        /// Map tmux pane target → epoch-seconds activity timestamp.
        tmux_activity: std::collections::HashMap<String, u64>,
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
                tmux_activity: std::collections::HashMap::new(),
                scanned: RefCell::new(Vec::new()),
            }
        }

        fn empty() -> Self {
            Self::new(&[])
        }

        /// Register a tmux pane → activity epoch mapping.
        fn with_tmux_activity(mut self, pane: &str, epoch: u64) -> Self {
            self.tmux_activity.insert(pane.to_string(), epoch);
            self
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

        fn tmux_pane_activity(&self, pane: &str) -> Option<u64> {
            self.tmux_activity.get(pane).copied()
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

    // ── v1 tests (unchanged behaviour) ───────────────────────────────────────

    /// (a) Live marker: current process pid → listed as alive/marker.
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

        // idle_secs=0 → any mtime ≥ now means working; but since no tmux and
        // the marker was just written, mtime ~= now → could be working.
        // Use idle_secs=0 to verify alive (not stale); state may be working or idle.
        let runner = MockScanRunner::empty();
        let sessions = collect_sessions(root, &runner, 60);

        assert_eq!(sessions.len(), 1);
        // Must be alive — not Stale.
        assert_ne!(sessions[0].state, SessionState::Stale);
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
        let sessions = collect_sessions(root, &runner, 60);

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
        let sessions = collect_sessions(root, &runner, 60);

        // Should find exactly one scan entry with no agentId.
        let scan_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.source == SessionSource::Scan)
            .collect();
        assert_eq!(scan_sessions.len(), 1);
        // Scan entries have no tmux/marker → Running (activity unknown).
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
        let sessions = collect_sessions(root, &runner, 60);

        // Should be exactly one entry (from marker), not two.
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].source, SessionSource::Marker);
    }

    /// (d) JSON output shape: all required keys present, including new v2 keys.
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
        let sessions = collect_sessions(root, &runner, 60);

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
                    "lastActivity": s.last_activity,
                })
            })
            .collect();
        let json = serde_json::json!({ "sessions": arr });
        let text = serde_json::to_string_pretty(&json).unwrap();

        // Top-level shape.
        assert!(text.contains(r#""sessions""#));
        // Per-session required keys (v1).
        assert!(text.contains(r#""backend""#));
        assert!(text.contains(r#""agentId""#));
        assert!(text.contains(r#""pid""#));
        assert!(text.contains(r#""state""#));
        assert!(text.contains(r#""source""#));
        assert!(text.contains(r#""startedAt""#));
        assert!(text.contains(r#""tmux""#));
        // v2 additive keys.
        assert!(text.contains(r#""lastActivity""#));
        // Value spot-checks.
        assert!(text.contains("agent-foo"));
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

    // ── v2 activity tests ─────────────────────────────────────────────────────

    /// tmux session with activity within idle-secs → `working`.
    #[cfg(unix)]
    #[test]
    fn tmux_activity_within_idle_secs_is_working() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        let marker = SessionMarker {
            agent_id: "agent-busy".to_string(),
            backend: "claude".to_string(),
            pid,
            started_at: "2026-05-24T10:00:00Z".to_string(),
            tmux: Some("bwoc:0.0".to_string()),
        };
        write_marker(root, &marker);

        // Report activity 5 seconds ago — well within the 60-second threshold.
        let recent = now_epoch_secs().saturating_sub(5);
        let runner = MockScanRunner::empty().with_tmux_activity("bwoc:0.0", recent);
        let sessions = collect_sessions(root, &runner, 60);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Working);
        assert_eq!(sessions[0].last_activity, Some(recent));
    }

    /// tmux session with activity older than idle-secs → `idle`.
    #[cfg(unix)]
    #[test]
    fn tmux_activity_older_than_idle_secs_is_idle() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        let marker = SessionMarker {
            agent_id: "agent-quiet".to_string(),
            backend: "agy".to_string(),
            pid,
            started_at: "2026-05-24T10:00:00Z".to_string(),
            tmux: Some("bwoc:1.0".to_string()),
        };
        write_marker(root, &marker);

        // Report activity 120 seconds ago — beyond the 60-second threshold.
        let old = now_epoch_secs().saturating_sub(120);
        let runner = MockScanRunner::empty().with_tmux_activity("bwoc:1.0", old);
        let sessions = collect_sessions(root, &runner, 60);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Idle);
        assert_eq!(sessions[0].last_activity, Some(old));
    }

    /// Alive session with no tmux and no marker path fallback → `running`.
    /// (Scan-sourced sessions: no tmux, no marker file.)
    #[cfg(unix)]
    #[test]
    fn alive_scan_session_no_activity_signal_is_running() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        // No marker written — session appears only via scan.
        let runner = MockScanRunner::new(&[("claude", &[pid])]);
        let sessions = collect_sessions(root, &runner, 60);

        let scan_sessions: Vec<_> = sessions
            .iter()
            .filter(|s| s.source == SessionSource::Scan)
            .collect();
        assert_eq!(scan_sessions.len(), 1);
        assert_eq!(scan_sessions[0].state, SessionState::Running);
        assert!(scan_sessions[0].last_activity.is_none());
    }

    /// Dead pid → `stale`, regardless of idle_secs. (v1 regression guard.)
    #[test]
    fn dead_pid_is_stale_v2() {
        let dir = make_workspace();
        let root = dir.path();

        let marker = SessionMarker {
            agent_id: "agent-dead".to_string(),
            backend: "kimi".to_string(),
            pid: 999_998,
            started_at: "2026-05-24T08:00:00Z".to_string(),
            tmux: None,
        };
        write_marker(root, &marker);

        let runner = MockScanRunner::empty();
        let sessions = collect_sessions(root, &runner, 60);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Stale);
        assert!(sessions[0].last_activity.is_none());
    }

    /// --json includes `lastActivity` and derived state; v1 keys intact.
    #[cfg(unix)]
    #[test]
    fn json_includes_last_activity_and_derived_state() {
        let dir = make_workspace();
        let root = dir.path();
        let pid = current_pid();

        let marker = SessionMarker {
            agent_id: "agent-json".to_string(),
            backend: "codex".to_string(),
            pid,
            started_at: "2026-05-24T10:00:00Z".to_string(),
            tmux: Some("main:0.0".to_string()),
        };
        write_marker(root, &marker);

        // Recent activity → working.
        let recent = now_epoch_secs().saturating_sub(10);
        let runner = MockScanRunner::empty().with_tmux_activity("main:0.0", recent);
        let sessions = collect_sessions(root, &runner, 60);

        // Simulate emit_json output.
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
                    "lastActivity": s.last_activity,
                })
            })
            .collect();
        let text = serde_json::to_string_pretty(&serde_json::json!({ "sessions": arr })).unwrap();

        // v1 keys still present.
        assert!(text.contains(r#""backend""#));
        assert!(text.contains(r#""agentId""#));
        assert!(text.contains(r#""pid""#));
        assert!(text.contains(r#""source""#));
        assert!(text.contains(r#""startedAt""#));
        assert!(text.contains(r#""tmux""#));
        // v2 additive.
        assert!(text.contains(r#""lastActivity""#));
        assert!(text.contains("working"));
        // lastActivity value is a number (not null).
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let la = &parsed["sessions"][0]["lastActivity"];
        assert!(la.is_number(), "lastActivity should be a number, got {la}");
    }

    // ── herdr provider tests (fake herdr socket, no herdr binary) ────────────

    /// Append `[integrations.herdr]` to the test workspace's workspace.toml.
    #[cfg(unix)]
    fn set_herdr(root: &Path, enabled: bool, socket: &Path) {
        let p = root.join(".bwoc/workspace.toml");
        let mut body = std::fs::read_to_string(&p).unwrap();
        body.push_str(&format!(
            "\n[integrations.herdr]\nenabled = {enabled}\nsocket = '{}'\n",
            socket.display()
        ));
        std::fs::write(p, body).unwrap();
    }

    /// Live marker for `id` on the current pid (+ its agent dir); returns the
    /// canonical agent dir, as herdr would report a cwd.
    #[cfg(unix)]
    fn live_agent(root: &Path, id: &str) -> PathBuf {
        let dir = root.join("agents").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        write_marker(
            root,
            &SessionMarker {
                agent_id: id.to_string(),
                backend: "claude".to_string(),
                pid: current_pid(),
                started_at: "2026-10-08T00:00:00Z".to_string(),
                tmux: None,
            },
        );
        std::fs::canonicalize(dir).unwrap()
    }

    #[cfg(unix)]
    fn agent_list_reply(id: &str, agents: &str) -> String {
        format!(r#"{{"id":"{id}","result":{{"type":"agent_list","agents":[{agents}]}}}}"#)
    }

    #[cfg(unix)]
    #[test]
    fn herdr_cwd_match_overrides_heuristic() {
        let dir = make_workspace();
        let root = dir.path();
        let agent_dir = live_agent(root, "agent-a");
        let cwd = agent_dir.join("src").display().to_string();
        let (_h, sock) = crate::herdr::tests::fake_server(move |method, _, id| {
            assert_eq!(method, "agent.list", "cwd match needs no process_info");
            Some(agent_list_reply(
                id,
                &format!(
                    r#"{{"pane_id":"w1:p1","agent_status":"blocked","foreground_cwd":"{cwd}","extra":true}}"#
                ),
            ))
        });
        set_herdr(root, true, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::Blocked);
        assert_eq!(sessions[0].state_source, Some(StateSource::Herdr));
        // lastActivity still comes from the heuristic signal.
        assert!(sessions[0].last_activity.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn herdr_pid_fallback_match() {
        let dir = make_workspace();
        let root = dir.path();
        live_agent(root, "agent-b");
        let pid = current_pid();
        let (_h, sock) = crate::herdr::tests::fake_server(move |method, params, id| match method {
            "agent.list" => Some(agent_list_reply(
                id,
                r#"{"pane_id":"w1:p2","agent_status":"done","foreground_cwd":"/somewhere/else"}"#,
            )),
            "pane.process_info" => {
                assert_eq!(params["pane_id"], "w1:p2");
                Some(format!(
                    r#"{{"id":"{id}","result":{{"type":"pane_process_info","process_info":{{"pane_id":"w1:p2","foreground_processes":[{{"pid":{pid},"name":"claude"}}]}}}}}}"#
                ))
            }
            _ => None,
        });
        set_herdr(root, true, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_eq!(sessions[0].state, SessionState::Done);
        assert_eq!(sessions[0].state_source, Some(StateSource::Herdr));
    }

    #[cfg(unix)]
    #[test]
    fn herdr_unmatched_cwd_is_ignored() {
        let dir = make_workspace();
        let root = dir.path();
        live_agent(root, "agent-c");
        let (_h, sock) = crate::herdr::tests::fake_server(|method, _, id| match method {
            "agent.list" => Some(agent_list_reply(
                id,
                r#"{"pane_id":"w9:p1","agent_status":"blocked","foreground_cwd":"/other/project"}"#,
            )),
            // Foreground holds some other process, not our marker pid.
            "pane.process_info" => Some(format!(
                r#"{{"id":"{id}","result":{{"type":"pane_process_info","process_info":{{"pane_id":"w9:p1","foreground_processes":[{{"pid":1,"name":"init"}}]}}}}}}"#
            )),
            _ => None,
        });
        set_herdr(root, true, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_ne!(sessions[0].state, SessionState::Blocked);
        assert_eq!(sessions[0].state_source, Some(StateSource::Marker));
    }

    #[cfg(unix)]
    #[test]
    fn herdr_timeout_falls_back_within_budget() {
        let dir = make_workspace();
        let root = dir.path();
        live_agent(root, "agent-d");
        let (_h, sock) = crate::herdr::tests::fake_server(|_, _, _| None); // never replies
        set_herdr(root, true, &sock);

        let t = std::time::Instant::now();
        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert!(
            t.elapsed() < std::time::Duration::from_millis(1500),
            "herdr must not hold bwoc sessions past its budget, took {:?}",
            t.elapsed()
        );
        assert_eq!(sessions[0].state_source, Some(StateSource::Marker));
    }

    #[cfg(unix)]
    #[test]
    fn herdr_protocol_mismatch_falls_back() {
        let dir = make_workspace();
        let root = dir.path();
        live_agent(root, "agent-e");
        let (_h, sock) = crate::herdr::tests::fake_server(|_, _, id| {
            Some(format!(
                r#"{{"id":"{id}","error":{{"code":"protocol_mismatch","message":"unsupported"}}}}"#
            ))
        });
        set_herdr(root, true, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_eq!(sessions[0].state_source, Some(StateSource::Marker));
    }

    #[cfg(unix)]
    #[test]
    fn herdr_missing_and_unknown_status_maps_to_running() {
        let dir = make_workspace();
        let root = dir.path();
        let a = live_agent(root, "agent-f").display().to_string();
        let (_h, sock) = crate::herdr::tests::fake_server(move |_, _, id| {
            // No agent_status at all, plus fields this client has never seen.
            Some(agent_list_reply(
                id,
                &format!(
                    r#"{{"pane_id":"w1:p1","foreground_cwd":"{a}","tokens":{{"k":"v"}},"launch_pending":true}}"#
                ),
            ))
        });
        set_herdr(root, true, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_eq!(sessions[0].state, SessionState::Running);
        assert_eq!(sessions[0].state_source, Some(StateSource::Herdr));
    }

    #[cfg(unix)]
    #[test]
    fn herdr_never_revives_a_dead_pid() {
        let dir = make_workspace();
        let root = dir.path();
        std::fs::create_dir_all(root.join("agents/agent-g")).unwrap();
        write_marker(
            root,
            &SessionMarker {
                agent_id: "agent-g".to_string(),
                backend: "claude".to_string(),
                pid: 999_997,
                started_at: "2026-10-08T00:00:00Z".to_string(),
                tmux: None,
            },
        );
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        let (_h, sock) = crate::herdr::tests::fake_server(move |_, _, id| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(agent_list_reply(
                id,
                r#"{"pane_id":"w1:p1","agent_status":"working"}"#,
            ))
        });
        set_herdr(root, true, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_eq!(sessions[0].state, SessionState::Stale);
        assert_eq!(sessions[0].state_source, None);
        // No alive marker → nothing to refine → socket untouched.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[cfg(unix)]
    #[test]
    fn herdr_disabled_by_config_touches_no_socket() {
        let dir = make_workspace();
        let root = dir.path();
        let a = live_agent(root, "agent-h").display().to_string();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        let (_h, sock) = crate::herdr::tests::fake_server(move |_, _, id| {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(agent_list_reply(
                id,
                &format!(
                    r#"{{"pane_id":"w1:p1","agent_status":"blocked","foreground_cwd":"{a}"}}"#
                ),
            ))
        });
        set_herdr(root, false, &sock);

        let sessions = collect_sessions(root, &MockScanRunner::empty(), 60);
        assert_ne!(sessions[0].state, SessionState::Blocked);
        assert_eq!(sessions[0].state_source, Some(StateSource::Marker));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn agent_for_cwd_is_component_wise_and_deepest_wins() {
        let dirs = vec![
            ("agent-a".to_string(), PathBuf::from("/ws/agents/agent-a")),
            ("agent-ab".to_string(), PathBuf::from("/ws/agents/agent-ab")),
        ];
        assert_eq!(
            agent_for_cwd(Path::new("/ws/agents/agent-ab/x"), &dirs).as_deref(),
            Some("agent-ab")
        );
        assert_eq!(
            agent_for_cwd(Path::new("/ws/agents/agent-a"), &dirs).as_deref(),
            Some("agent-a")
        );
        assert_eq!(agent_for_cwd(Path::new("/ws/agents"), &dirs), None);
    }

    #[test]
    fn herdr_states_map_and_render() {
        use crate::herdr::AgentStatus;
        assert_eq!(
            SessionState::from_herdr(AgentStatus::Blocked).as_str(),
            "blocked"
        );
        assert_eq!(SessionState::from_herdr(AgentStatus::Done).as_str(), "done");
        assert_eq!(
            SessionState::from_herdr(AgentStatus::Working).as_str(),
            "working"
        );
        assert_eq!(SessionState::from_herdr(AgentStatus::Idle).as_str(), "idle");
        assert_eq!(
            SessionState::from_herdr(AgentStatus::Unknown).as_str(),
            "running"
        );
        assert_eq!(StateSource::Herdr.as_str(), "herdr");
    }
}
