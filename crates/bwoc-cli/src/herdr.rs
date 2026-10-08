//! Minimal herdr socket client — read-only agent state (herdr integration, Phase 1).
//!
//! herdr (<https://herdr.dev>) is an optional terminal-pane server that
//! classifies the agent in each pane as `working | blocked | done | idle |
//! unknown`. This client asks it, nothing more.
//!
//! ## Wire protocol (herdr socket API, read at v0.9.3)
//!
//! Newline-delimited JSON over a unix socket — one request line
//! `{"id","method","params"}`, one response line `{"id","result"}` or
//! `{"id","error":{"code","message"}}`. Not JSON-RPC 2.0.
//!
//! - `agent.list`, params `{}` → `result = {"type":"agent_list","agents":[AgentInfo]}`;
//!   we read `pane_id`, `agent_status`, `foreground_cwd` of each `AgentInfo`.
//! - `pane.process_info`, params `{"pane_id"}` →
//!   `result = {"type":"pane_process_info","process_info":{"pane_id","shell_pid"?,
//!   "foreground_process_group_id"?,"tty"?,"foreground_processes":[{"pid","name",…}]}}`.
//!
//! ## Failure contract
//!
//! herdr is pre-1.0 and its protocol moves, so every read is lenient (unknown
//! fields ignored, missing fields defaulted, a malformed list element skipped)
//! and **every** failure — no socket, refused or timed-out connect, timeout, an `error`
//! reply (incl. `protocol_mismatch`), bad JSON, a mismatched `id` — collapses
//! to `None`, i.e. "herdr unavailable". All calls made through one [`Client`]
//! share a single time budget, so herdr can never slow a `bwoc` command by
//! more than that budget.
//!
//! Phase 2 reuses [`Client::call`] for its own methods.
//!
//! The transport is unix-only (herdr uses a named pipe on Windows, out of
//! scope); elsewhere [`Client::call`] is a no-op that returns `None`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

/// Default budget for every herdr call a single command makes, combined.
pub const DEFAULT_BUDGET: Duration = Duration::from_millis(300);

/// Upper bound on one response line — a wedged or hostile peer must not make
/// us buffer without limit.
#[cfg(unix)]
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

// ── Socket resolution ─────────────────────────────────────────────────────────

/// Resolve the herdr socket path: `configured` (workspace
/// `[integrations.herdr] socket`) > `HERDR_SOCKET_PATH` > `HERDR_SESSION`
/// (named session) > the default session socket.
pub fn resolve_socket(configured: Option<&Path>) -> Option<PathBuf> {
    resolve_socket_from(
        configured,
        std::env::var_os("HERDR_SOCKET_PATH").map(PathBuf::from),
        std::env::var("HERDR_SESSION").ok(),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// Pure core of [`resolve_socket`] (env injected, for tests).
///
/// herdr's config dir is `$XDG_CONFIG_HOME/herdr` when that is set, else
/// `~/.config/herdr`; the default socket is `<dir>/herdr.sock` and a named
/// session's is `<dir>/sessions/<name>/herdr.sock`.
fn resolve_socket_from(
    configured: Option<&Path>,
    socket_env: Option<PathBuf>,
    session_env: Option<String>,
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(p) = configured.filter(|p| !p.as_os_str().is_empty()) {
        return Some(p.to_path_buf());
    }
    if let Some(p) = socket_env.filter(|p| !p.as_os_str().is_empty()) {
        return Some(p);
    }
    let config_dir = match xdg_config_home.filter(|p| !p.as_os_str().is_empty()) {
        Some(x) => x.join("herdr"),
        None => home
            .filter(|p| !p.as_os_str().is_empty())?
            .join(".config/herdr"),
    };
    // A session name is a single path segment; anything else falls through to
    // the default socket rather than escaping the sessions dir.
    if let Some(name) = session_env
        .filter(|n| !n.is_empty() && n != "." && n != ".." && !n.contains('/') && !n.contains('\\'))
    {
        return Some(config_dir.join("sessions").join(name).join("herdr.sock"));
    }
    Some(config_dir.join("herdr.sock"))
}

// ── Typed views (lenient) ─────────────────────────────────────────────────────

/// herdr's semantic agent state. Anything unrecognised reads as `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[default]
    Unknown,
}

impl AgentStatus {
    fn parse(s: &str) -> Self {
        match s {
            "idle" => AgentStatus::Idle,
            "working" => AgentStatus::Working,
            "blocked" => AgentStatus::Blocked,
            "done" => AgentStatus::Done,
            _ => AgentStatus::Unknown,
        }
    }
}

/// The subset of herdr's `AgentInfo` BWOC reads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentInfo {
    pub pane_id: String,
    pub status: AgentStatus,
    /// cwd of the process currently controlling the pane PTY, when herdr can
    /// resolve it.
    pub foreground_cwd: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAgentInfo {
    pane_id: String,
    agent_status: Option<String>,
    foreground_cwd: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProcessInfo {
    foreground_process_group_id: Option<u32>,
    foreground_processes: Vec<Value>,
}

// ── Client ────────────────────────────────────────────────────────────────────

/// A herdr client bound to one socket and one shared time budget.
pub struct Client {
    socket: PathBuf,
    deadline: Instant,
    /// Set after the first failed call: herdr is "unavailable" for the rest of
    /// this command, so later calls return immediately.
    failed: std::cell::Cell<bool>,
    next_id: std::cell::Cell<u64>,
}

impl Client {
    /// A client whose calls, combined, finish within `budget` from now.
    pub fn new(socket: PathBuf, budget: Duration) -> Self {
        Self {
            socket,
            deadline: Instant::now() + budget,
            failed: std::cell::Cell::new(false),
            next_id: std::cell::Cell::new(1),
        }
    }

    /// Send one request; return its `result`, or `None` on any failure.
    pub fn call(&self, method: &str, params: Value) -> Option<Value> {
        if self.failed.get() {
            return None;
        }
        let id = format!("bwoc-{}", self.next_id.get());
        self.next_id.set(self.next_id.get() + 1);
        let out = self.round_trip(&id, method, params);
        if out.is_none() {
            self.failed.set(true);
        }
        out
    }

    #[cfg(unix)]
    fn round_trip(&self, id: &str, method: &str, params: Value) -> Option<Value> {
        use std::io::{Read, Write};

        let remaining = |deadline: Instant| {
            deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
        };

        let mut stream = connect_within(&self.socket, remaining(self.deadline)?)?;
        let mut line = serde_json::to_vec(&serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        }))
        .ok()?;
        line.push(b'\n');
        stream
            .set_write_timeout(Some(remaining(self.deadline)?))
            .ok()?;
        stream.write_all(&line).ok()?;

        // Read until the first newline, re-arming the read timeout with what is
        // left of the budget each time so a slow-drip peer cannot extend it.
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            stream
                .set_read_timeout(Some(remaining(self.deadline)?))
                .ok()?;
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                return None; // closed before a full line
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                buf.truncate(pos);
                break;
            }
            if buf.len() > MAX_RESPONSE_BYTES {
                return None;
            }
        }
        parse_response(&buf, id)
    }

    #[cfg(not(unix))]
    fn round_trip(&self, _id: &str, _method: &str, _params: Value) -> Option<Value> {
        let _ = (&self.socket, self.deadline);
        None
    }

    /// `agent.list` → every agent herdr currently tracks. Elements that do not
    /// parse are skipped; `None` only when the call itself failed.
    pub fn agent_list(&self) -> Option<Vec<AgentInfo>> {
        let result = self.call("agent.list", serde_json::json!({}))?;
        let agents = result.get("agents")?.as_array()?;
        Some(
            agents
                .iter()
                .filter_map(|v| serde_json::from_value::<RawAgentInfo>(v.clone()).ok())
                .filter(|r| !r.pane_id.is_empty())
                .map(|r| AgentInfo {
                    pane_id: r.pane_id,
                    status: r
                        .agent_status
                        .as_deref()
                        .map(AgentStatus::parse)
                        .unwrap_or_default(),
                    foreground_cwd: r.foreground_cwd.filter(|c| !c.is_empty()),
                })
                .collect(),
        )
    }

    /// `pane.process_info` → pids in the pane's foreground: every
    /// `foreground_processes[].pid` plus the foreground process-group id (the
    /// group leader's pid).
    pub fn pane_foreground_pids(&self, pane_id: &str) -> Option<Vec<u32>> {
        let result = self.call(
            "pane.process_info",
            serde_json::json!({ "pane_id": pane_id }),
        )?;
        let info: RawProcessInfo =
            serde_json::from_value(result.get("process_info")?.clone()).ok()?;
        let mut pids: Vec<u32> = info
            .foreground_processes
            .iter()
            .filter_map(|p| p.get("pid")?.as_u64())
            .filter_map(|p| u32::try_from(p).ok())
            .collect();
        pids.extend(info.foreground_process_group_id);
        Some(pids)
    }
}

/// Connect attempts given up on whose helper thread has not returned yet.
#[cfg(unix)]
static ABANDONED_CONNECTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Stop starting connect threads while this many abandoned ones are still
/// blocked: herdr is wedged, so say "unavailable" at once instead of piling up
/// threads (the dashboard calls herdr on every refresh).
#[cfg(unix)]
const MAX_ABANDONED_CONNECTS: usize = 4;

/// `UnixStream::connect`, given up after `budget`.
///
/// std's connect has no timeout, and on Linux a connect to a listener whose
/// accept backlog is full blocks until the server accepts — a stalled herdr
/// would hang `bwoc sessions`, the dashboard refresh and the `bwoc send`
/// wakeup past [`DEFAULT_BUDGET`]. So the connect runs on a helper thread and
/// we wait for it with `recv_timeout`. On timeout the thread is **abandoned**,
/// deliberately: it holds only the path and a not-yet-connected fd, and ends
/// by itself when the connect resolves (herdr accepts, herdr exits, or this
/// process does); a stream it obtains after we left is dropped (closed) at
/// once. [`MAX_ABANDONED_CONNECTS`] bounds how many can be outstanding.
/// Chosen over a non-blocking `libc` connect + `poll` to avoid hand-building a
/// `sockaddr_un` in `unsafe` code; no extra crates either way.
#[cfg(unix)]
fn connect_within(socket: &Path, budget: Duration) -> Option<std::os::unix::net::UnixStream> {
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};

    const PENDING: u8 = 0;
    const DONE: u8 = 1;
    const ABANDONED: u8 = 2;

    if ABANDONED_CONNECTS.load(Ordering::SeqCst) >= MAX_ABANDONED_CONNECTS {
        return None;
    }
    let state = Arc::new(AtomicU8::new(PENDING));
    let thread_state = state.clone();
    let path = socket.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("bwoc-herdr-connect".into())
        .spawn(move || {
            let res = UnixStream::connect(&path);
            if thread_state
                .compare_exchange(PENDING, DONE, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                // The caller gave up; it counted us as abandoned.
                ABANDONED_CONNECTS.fetch_sub(1, Ordering::SeqCst);
            }
            let _ = tx.send(res); // receiver gone → the stream is dropped
        })
        .ok()?;
    match rx.recv_timeout(budget) {
        Ok(res) => res.ok(),
        Err(_) => {
            // Count first, then claim: if the thread finished in between, the
            // claim fails and we undo the count (the thread did not see us).
            ABANDONED_CONNECTS.fetch_add(1, Ordering::SeqCst);
            if state
                .compare_exchange(PENDING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                ABANDONED_CONNECTS.fetch_sub(1, Ordering::SeqCst);
            }
            None
        }
    }
}

/// Decode one response line. `Some(result)` only for a well-formed success
/// reply carrying our `id`.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_response(line: &[u8], id: &str) -> Option<Value> {
    let v: Value = serde_json::from_slice(line).ok()?;
    if v.get("id").and_then(Value::as_str) != Some(id) {
        return None;
    }
    if v.get("error").is_some() {
        return None;
    }
    v.get("result").cloned()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn socket_resolution_order() {
        let home = Some(PathBuf::from("/home/u"));
        // Configured wins over everything.
        assert_eq!(
            resolve_socket_from(
                Some(Path::new("/cfg.sock")),
                Some("/env.sock".into()),
                Some("work".into()),
                None,
                home.clone()
            ),
            Some(PathBuf::from("/cfg.sock"))
        );
        // HERDR_SOCKET_PATH next.
        assert_eq!(
            resolve_socket_from(
                None,
                Some("/env.sock".into()),
                Some("work".into()),
                None,
                home.clone()
            ),
            Some(PathBuf::from("/env.sock"))
        );
        // HERDR_SESSION → named session socket.
        assert_eq!(
            resolve_socket_from(None, None, Some("work".into()), None, home.clone()),
            Some(PathBuf::from(
                "/home/u/.config/herdr/sessions/work/herdr.sock"
            ))
        );
        // Default.
        assert_eq!(
            resolve_socket_from(None, None, None, None, home.clone()),
            Some(PathBuf::from("/home/u/.config/herdr/herdr.sock"))
        );
        // XDG_CONFIG_HOME relocates the config dir.
        assert_eq!(
            resolve_socket_from(None, None, None, Some("/xdg".into()), home.clone()),
            Some(PathBuf::from("/xdg/herdr/herdr.sock"))
        );
        // Empty values are ignored; a path-like session name is not trusted.
        assert_eq!(
            resolve_socket_from(
                Some(Path::new("")),
                Some("".into()),
                Some("../x".into()),
                None,
                home
            ),
            Some(PathBuf::from("/home/u/.config/herdr/herdr.sock"))
        );
        // No home and no XDG → nothing to try.
        assert_eq!(resolve_socket_from(None, None, None, None, None), None);
    }

    #[test]
    fn parse_response_requires_matching_id_and_no_error() {
        assert_eq!(
            parse_response(br#"{"id":"a","result":{"type":"pong"}}"#, "a"),
            Some(serde_json::json!({"type":"pong"}))
        );
        assert_eq!(
            parse_response(br#"{"id":"b","result":{}}"#, "a"),
            None,
            "foreign id"
        );
        assert_eq!(
            parse_response(
                br#"{"id":"a","error":{"code":"protocol_mismatch","message":"x"}}"#,
                "a"
            ),
            None
        );
        assert_eq!(parse_response(b"not json", "a"), None);
    }

    #[test]
    fn status_parse_is_lenient() {
        assert_eq!(AgentStatus::parse("blocked"), AgentStatus::Blocked);
        assert_eq!(AgentStatus::parse("done"), AgentStatus::Done);
        assert_eq!(AgentStatus::parse("thinking"), AgentStatus::Unknown);
    }

    /// Fake herdr: answer each request line with `respond(method, params, id)`
    /// (or never answer when it returns `None`), on a tempdir socket.
    #[cfg(unix)]
    pub(crate) fn fake_server<F>(respond: F) -> (tempfile::TempDir, PathBuf)
    where
        F: Fn(&str, &Value, &str) -> Option<String> + Send + 'static,
    {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let dir = tempfile::TempDir::new().unwrap();
        let sock = dir.path().join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                let mut line = String::new();
                if BufReader::new(&conn).read_line(&mut line).is_err() {
                    continue;
                }
                let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                let method = req["method"].as_str().unwrap_or("").to_string();
                let id = req["id"].as_str().unwrap_or("").to_string();
                match respond(&method, &req["params"], &id) {
                    Some(reply) => {
                        let _ = conn.write_all(reply.as_bytes());
                        let _ = conn.write_all(b"\n");
                    }
                    // Never reply: hold the connection open past the budget.
                    None => std::thread::sleep(Duration::from_secs(2)),
                }
            }
        });
        (dir, sock)
    }

    #[cfg(unix)]
    #[test]
    fn agent_list_happy_path_ignores_unknown_and_defaults_missing() {
        let (_dir, sock) = fake_server(|method, _params, id| {
            assert_eq!(method, "agent.list");
            Some(format!(
                r#"{{"id":"{id}","result":{{"type":"agent_list","future":1,"agents":[
                    {{"pane_id":"w1:p1","agent_status":"blocked","foreground_cwd":"/ws/agents/a","new_field":{{"x":1}},"revision":3}},
                    {{"pane_id":"w1:p2"}},
                    {{"pane_id":42}},
                    {{"pane_id":"w1:p3","agent_status":"sleeping"}}
                ]}}}}"#
            ).replace('\n', ""))
        });
        let c = Client::new(sock, Duration::from_secs(2));
        let agents = c.agent_list().unwrap();
        assert_eq!(agents.len(), 3, "malformed element skipped");
        assert_eq!(agents[0].status, AgentStatus::Blocked);
        assert_eq!(agents[0].foreground_cwd.as_deref(), Some("/ws/agents/a"));
        assert_eq!(agents[1].status, AgentStatus::Unknown, "missing → default");
        assert!(agents[1].foreground_cwd.is_none());
        assert_eq!(agents[2].status, AgentStatus::Unknown, "unknown → Unknown");
    }

    #[cfg(unix)]
    #[test]
    fn process_info_collects_foreground_pids() {
        let (_dir, sock) = fake_server(|method, params, id| {
            assert_eq!(method, "pane.process_info");
            assert_eq!(params["pane_id"], "w1:p1");
            Some(format!(
                r#"{{"id":"{id}","result":{{"type":"pane_process_info","process_info":{{"pane_id":"w1:p1","shell_pid":10,"foreground_process_group_id":20,"foreground_processes":[{{"pid":20,"name":"claude"}},{{"pid":21,"name":"node","cwd":"/x"}}]}}}}}}"#
            ))
        });
        let c = Client::new(sock, Duration::from_secs(2));
        let mut pids = c.pane_foreground_pids("w1:p1").unwrap();
        pids.sort_unstable();
        assert_eq!(pids, vec![20, 20, 21]);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_is_bounded_by_budget_and_poisons_client() {
        let (_dir, sock) = fake_server(|_, _, _| None);
        let c = Client::new(sock, Duration::from_millis(150));
        let t = Instant::now();
        assert!(c.agent_list().is_none());
        assert!(
            t.elapsed() < Duration::from_millis(1000),
            "call must give up at the budget, took {:?}",
            t.elapsed()
        );
        // Unavailable for the rest of the command — no second wait.
        let t = Instant::now();
        assert!(c.pane_foreground_pids("w1:p1").is_none());
        assert!(t.elapsed() < Duration::from_millis(50));
    }

    /// A listener that never accepts, with its backlog shrunk to 1 (macOS
    /// reads 0 as "default", i.e. 128) and then filled: on Linux a further
    /// blocking connect hangs (the case the bound exists for); on macOS it is
    /// refused at once. Either way `connect_within` must answer `None` inside
    /// the budget.
    #[cfg(unix)]
    #[test]
    fn connect_is_bounded_when_the_backlog_is_full() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::{UnixListener, UnixStream};

        let dir = tempfile::TempDir::new().unwrap();
        let sock = dir.path().join("herdr.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        // SAFETY: re-`listen` on a socket we own, only to shrink its backlog.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        // Fill the queue. Fillers past its capacity block (Linux) until the
        // listener closes at the end of the test, or fail at once (macOS).
        for _ in 0..8 {
            let s = sock.clone();
            std::thread::spawn(move || {
                let held = UnixStream::connect(s);
                std::thread::sleep(Duration::from_secs(5));
                drop(held);
            });
        }
        std::thread::sleep(Duration::from_millis(300));

        let t = Instant::now();
        let got = connect_within(&sock, Duration::from_millis(200));
        assert!(
            t.elapsed() < Duration::from_millis(1000),
            "connect must give up at the budget, took {:?}",
            t.elapsed()
        );
        assert!(got.is_none(), "a full backlog is \"herdr unavailable\"");
        drop(listener);
    }

    #[cfg(unix)]
    #[test]
    fn connect_within_reaches_a_live_listener() {
        let (_dir, sock) =
            fake_server(|_, _, id| Some(format!(r#"{{"id":"{id}","result":{{}}}}"#)));
        assert!(connect_within(&sock, Duration::from_secs(2)).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn error_reply_and_missing_socket_are_unavailable() {
        let (_dir, sock) = fake_server(|_, _, id| {
            Some(format!(
                r#"{{"id":"{id}","error":{{"code":"protocol_mismatch","message":"client v14, server v15"}}}}"#
            ))
        });
        assert!(Client::new(sock, DEFAULT_BUDGET).agent_list().is_none());
        let gone = PathBuf::from("/nonexistent/bwoc-herdr-test.sock");
        assert!(Client::new(gone, DEFAULT_BUDGET).agent_list().is_none());
    }
}
