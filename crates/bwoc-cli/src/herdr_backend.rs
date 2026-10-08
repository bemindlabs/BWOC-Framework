//! [`HerdrBackend`] — the herdr implementation of [`PaneBackend`] (herdr
//! integration, Phase 2b).
//!
//! Every operation is one or two calls over herdr's socket through
//! [`crate::herdr::Client`]; nothing here runs the `herdr` binary except
//! [`PaneBackend::attach_fleet`], which has to hand the terminal to herdr's own
//! client.
//!
//! ## herdr methods used (schema read at v0.9.3, `src/api/schema/*.rs`)
//!
//! - `ping {}` → `{"type":"pong",…}` — availability.
//! - `workspace.list {}` → `{"type":"workspace_list","workspaces":[WorkspaceInfo]}`;
//!   a fleet is the workspace whose `label` is the fleet session name.
//! - `workspace.create {label,cwd,focus}` → `{"type":"workspace_created",
//!   "workspace":{workspace_id,…},"tab":{tab_id,…},"root_pane":{…}}`.
//! - `layout.apply {workspace_id,tab_id,tab_label,focus,root}` — builds a fresh
//!   tab from a split tree and, given `tab_id`, closes that tab afterwards (so
//!   the shell tab `workspace.create` made does not linger).
//! - `workspace.focus {workspace_id}` — before attaching.
//! - `pane.list {}` → `{"type":"pane_list","panes":[PaneInfo]}`; read
//!   `pane_id`, `label`, `agent`, `foreground_cwd`.
//! - `pane.send_text {pane_id,text}`, then `pane.send_keys {pane_id,keys:["enter"]}`.
//!
//! Panes run the same argv as the tmux backend
//! ([`crate::pane_backend::spawn_argv`]), so herdr detects the agent CLI on its
//! own — `agent.start` is deliberately not used: it accepts only herdr's
//! built-in agent kinds, and an ollama-backed agent would not start.
//!
//! herdr is pre-1.0: replies are read leniently and any failure is "herdr
//! unavailable", never a panic. The transport is unix-only; elsewhere
//! [`PaneBackend::check_available`] refuses with a remedy.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use crate::fleet_term::PaneLayout;
use crate::herdr::Client;
use crate::pane_backend::{FleetAgent, PaneBackend, PaneHandle, spawn_argv};

/// Budget for the socket calls one `bwoc fleet term` operation makes:
/// `layout.apply` starts one process per agent before it answers, which can
/// take longer than the 300 ms read budget.
pub(crate) const FLEET_BUDGET: Duration = Duration::from_secs(5);

/// Gap between typing the text and pressing Enter — the same 200 ms the tmux
/// backend uses, for the same reason (a TUI can drop a submit that arrives in
/// the same burst as the text).
const SUBMIT_GAP: Duration = Duration::from_millis(200);

/// Share of the main pane in the `main-*` layouts. tmux sizes its main pane
/// in cells; a herdr split takes a ratio, so this is BWOC's choice.
const MAIN_RATIO: f64 = 0.6;

/// herdr as a [`PaneBackend`].
pub(crate) struct HerdrBackend {
    /// `None` only when no socket path can be resolved at all (no `HOME`).
    socket: Option<PathBuf>,
    /// The socket came from `[integrations.herdr] socket`, so the operator's
    /// own `herdr` invocations need it spelled out (`HERDR_SOCKET_PATH=…`).
    socket_from_config: bool,
    /// Workspace root, for matching a pane to an agent by directory.
    workspace: Option<PathBuf>,
    /// Bound on each operation's socket calls, combined.
    budget: Duration,
}

impl HerdrBackend {
    pub(crate) fn new(
        socket: Option<PathBuf>,
        socket_from_config: bool,
        workspace: Option<PathBuf>,
        budget: Duration,
    ) -> Self {
        Self {
            socket,
            socket_from_config,
            workspace,
            budget,
        }
    }

    /// The backend for `workspace`: socket from `[integrations.herdr] socket`,
    /// else herdr's own resolution (`HERDR_SOCKET_PATH` > `HERDR_SESSION` >
    /// default). `[integrations.herdr] enabled` does not gate this — that key
    /// switches the Phase 1 state provider; choosing herdr panes is its own
    /// decision.
    pub(crate) fn for_workspace(workspace: &Path, budget: Duration) -> Self {
        let configured = bwoc_core::workspace::Integrations::load(workspace)
            .herdr
            .socket
            .filter(|p| !p.as_os_str().is_empty());
        Self::new(
            crate::herdr::resolve_socket(configured.as_deref()),
            configured.is_some(),
            Some(workspace.to_path_buf()),
            budget,
        )
    }

    /// A client for one operation, or `None` when there is no socket path.
    fn client(&self, budget: Duration) -> Option<Client> {
        Some(Client::new(self.socket.clone()?, budget))
    }

    /// One call on a fresh client with the standard budget.
    fn call(&self, method: &str, params: Value) -> Option<Value> {
        self.client(self.budget)?.call(method, params)
    }

    /// The id of the workspace labelled `label`, if herdr has one.
    fn find_workspace(&self, label: &str) -> Option<String> {
        let result = self.call("workspace.list", json!({}))?;
        result
            .get("workspaces")?
            .as_array()?
            .iter()
            .find(|w| w.get("label").and_then(Value::as_str) == Some(label))?
            .get("workspace_id")?
            .as_str()
            .map(str::to_string)
    }

    /// `HERDR_SOCKET_PATH='<socket>' ` when the socket came from workspace
    /// config (a bare `herdr` would reach a different server), else empty.
    /// The path is shell-quoted: it is operator-supplied and the hint is meant
    /// to be pasted into a shell.
    fn env_prefix(&self) -> String {
        match (&self.socket, self.socket_from_config) {
            (Some(s), true) => format!("HERDR_SOCKET_PATH={} ", shell_quote(&s.to_string_lossy())),
            _ => String::new(),
        }
    }
}

/// `s` as one POSIX-shell word: wrapped in single quotes, each embedded `'`
/// written as `'\''` (close, escaped quote, reopen). Nothing is special inside
/// single quotes, so `$`, backticks, spaces and `;` stay literal.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl PaneBackend for HerdrBackend {
    fn name(&self) -> &'static str {
        "herdr"
    }

    #[cfg(unix)]
    fn check_available(&self) -> Result<(), String> {
        let Some(socket) = &self.socket else {
            return Err("cannot locate the herdr socket (HOME is unset). Set \
                 HERDR_SOCKET_PATH, or `[integrations.herdr] socket` in .bwoc/workspace.toml."
                .to_string());
        };
        match self.call("ping", json!({})) {
            Some(_) => Ok(()),
            None => Err(format!(
                "herdr is not answering on {}. Start it with `herdr` in another terminal, \
                 or point HERDR_SOCKET_PATH (or `[integrations.herdr] socket` in \
                 .bwoc/workspace.toml) at a running server. `--backend tmux` works without it.",
                socket.display()
            )),
        }
    }

    #[cfg(not(unix))]
    fn check_available(&self) -> Result<(), String> {
        Err(
            "the herdr pane backend is unix-only (herdr uses a named pipe on this \
             platform, which BWOC does not speak). Use `--backend tmux`."
                .to_string(),
        )
    }

    fn fleet_exists(&self, session: &str) -> bool {
        self.find_workspace(session).is_some()
    }

    fn open_fleet(
        &self,
        session: &str,
        agents: &[FleetAgent],
        layout: PaneLayout,
        bwoc_exe: &str,
    ) -> Result<String, String> {
        let Some(root) = layout_tree(agents, layout, bwoc_exe) else {
            return Err("no agents to lay out".to_string());
        };
        let client = self
            .client(self.budget)
            .ok_or_else(|| "cannot locate the herdr socket".to_string())?;
        let cwd = self
            .workspace
            .as_ref()
            .map(|w| w.to_string_lossy().into_owned())
            .unwrap_or_else(|| agents[0].path.clone());
        let created = client
            .call(
                "workspace.create",
                json!({ "label": session, "cwd": cwd, "focus": false }),
            )
            .ok_or_else(|| {
                format!("herdr did not create workspace '{session}' (no reply or an error).")
            })?;
        let workspace_id = created
            .pointer("/workspace/workspace_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!(
                    "herdr created workspace '{session}' but its reply carried no id — \
                     `{}herdr workspace list` to find and close it.",
                    self.env_prefix()
                )
            })?
            .to_string();
        let kill = format!("{}herdr workspace close {workspace_id}", self.env_prefix());

        let mut params = json!({
            "workspace_id": workspace_id,
            "tab_label": "fleet",
            "focus": true,
            "root": root,
        });
        // Replace the shell tab workspace.create opened instead of leaving it.
        if let Some(tab_id) = created.pointer("/tab/tab_id").and_then(Value::as_str) {
            params["tab_id"] = Value::String(tab_id.to_string());
        }
        client.call("layout.apply", params).ok_or_else(|| {
            format!(
                "herdr layout.apply failed — workspace '{session}' ({workspace_id}) may be \
                 partially built; `{kill}` to clear it."
            )
        })?;
        Ok(format!(
            "Opened {} agent panes in herdr workspace '{session}' ({workspace_id}, layout: {}).",
            agents.len(),
            layout_label(layout)
        ))
    }

    /// Focus the fleet's workspace, then run herdr's own client, which attaches
    /// to the running server (`herdr` = "launch or attach", cli-reference).
    fn attach_fleet(&self, session: &str) -> std::io::Result<()> {
        if let Some(id) = self.find_workspace(session) {
            let _ = self.call("workspace.focus", json!({ "workspace_id": id }));
        }
        let mut cmd = Command::new("herdr");
        if let (Some(s), true) = (&self.socket, self.socket_from_config) {
            cmd.env("HERDR_SOCKET_PATH", s);
        }
        // Exit status ignored, as for tmux: a detach is not a `bwoc` failure.
        cmd.status().map(|_| ())
    }

    fn attach_hint(&self, session: &str) -> String {
        let p = self.env_prefix();
        match self.find_workspace(session) {
            Some(id) => format!("{p}herdr workspace focus {id} && {p}herdr"),
            None => format!("{p}herdr  (then switch to workspace '{session}')"),
        }
    }

    fn kill_hint(&self, session: &str) -> String {
        let p = self.env_prefix();
        match self.find_workspace(session) {
            Some(id) => format!("{p}herdr workspace close {id}"),
            None => format!(
                "{p}herdr workspace list  (then `{p}herdr workspace close <id>` for '{session}')"
            ),
        }
    }

    /// The pane labelled `agent_id` (what [`open_fleet`](Self::open_fleet)
    /// sets), else a pane where herdr detected an agent whose `foreground_cwd`
    /// is inside the agent's directory — never a bare shell (see
    /// [`match_pane`]). Handle = herdr pane id (`w1:p2`).
    fn locate_agent(&self, agent_id: &str) -> Option<PaneHandle> {
        let result = self.call("pane.list", json!({}))?;
        let panes = result.get("panes")?.as_array()?;
        let dirs = || {
            self.workspace
                .as_deref()
                .map(|ws| crate::sessions::agent_dirs(ws, std::iter::once(agent_id)))
                .unwrap_or_default()
        };
        match_pane(panes, agent_id, dirs).map(PaneHandle::new)
    }

    /// `pane.send_text` (written verbatim — no key interpretation), then
    /// [`SUBMIT_GAP`], then `pane.send_keys ["enter"]`. Best-effort: Enter is
    /// skipped when the text did not land, and nothing is ever reported.
    fn submit_text(&self, pane: &PaneHandle, text: &str) {
        let Some(client) = self.client(self.budget + SUBMIT_GAP) else {
            return;
        };
        let id = pane.as_str();
        if client
            .call("pane.send_text", json!({ "pane_id": id, "text": text }))
            .is_none()
        {
            return;
        }
        std::thread::sleep(SUBMIT_GAP);
        let _ = client.call(
            "pane.send_keys",
            json!({ "pane_id": id, "keys": ["enter"] }),
        );
    }
}

/// The operator-facing layout name (`main-vertical`, …), as `--layout` takes it.
fn layout_label(layout: PaneLayout) -> String {
    clap::ValueEnum::to_possible_value(&layout)
        .map(|v| v.get_name().to_string())
        .unwrap_or_else(|| format!("{layout:?}"))
}

/// The pane a wakeup may type into: the first pane labelled `agent_id` (what
/// [`PaneBackend::open_fleet`] sets); else the first pane where herdr reports a
/// detected agent **and** whose `foreground_cwd` falls inside `agent_id`'s
/// directory (`dirs` is only built when the label pass misses). The pane `cwd`
/// is never used: it is the pane/workspace cwd (socket-api.mdx), so a shell or
/// editor `cd`'d into an agent dir would otherwise receive the text + Enter.
/// No eligible pane → `None` (no wakeup). Directory matching reuses the
/// Phase 1 state provider's helpers, so both agree on which pane is whose.
///
/// "Detected agent" = a non-empty `PaneInfo.agent` (herdr v0.9.3
/// `src/api/schema/panes.rs` L463-464, `agent: Option<String>`; filled from
/// `terminal.effective_agent_label()` in `src/app/creation.rs` L345, which
/// `src/terminal/state.rs` L2039-2050 derives from a `pane.report_agent`
/// authority or herdr's own process detection, and drops once the detected
/// agent process exits — so a plain shell or editor pane has none).
fn match_pane(
    panes: &[Value],
    agent_id: &str,
    dirs: impl FnOnce() -> Vec<(String, PathBuf)>,
) -> Option<String> {
    let pane_id = |p: &Value| {
        p.get("pane_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if let Some(id) = panes
        .iter()
        .filter(|p| p.get("label").and_then(Value::as_str) == Some(agent_id))
        .find_map(pane_id)
    {
        return Some(id);
    }
    let has_agent = |p: &Value| {
        p.get("agent")
            .and_then(Value::as_str)
            .is_some_and(|a| !a.trim().is_empty())
    };
    let dirs = dirs();
    let fg_owned = |p: &Value| {
        p.get("foreground_cwd")
            .and_then(Value::as_str)
            .and_then(|c| crate::sessions::agent_for_cwd(Path::new(c), &dirs))
            .as_deref()
            == Some(agent_id)
    };
    panes
        .iter()
        .filter(|p| has_agent(p) && fg_owned(p))
        .find_map(pane_id)
}

// ── Layout tree ───────────────────────────────────────────────────────────────

/// The `layout.apply` `root` for `agents` arranged by `layout`, one pane per
/// agent (label = id, cwd = agent dir, command = the shared `bwoc spawn` argv).
/// `None` when there are no agents. Shapes mirror the tmux layouts:
///
/// - `grid` — rows of `ceil(sqrt(n))` panes, last row shorter (tmux `tiled`).
/// - `columns` / `rows` — equal panes side by side / stacked.
/// - `main-vertical` / `main-horizontal` — first agent in a [`MAIN_RATIO`] main
///   pane on the left / top, the rest stacked / in a row beside it.
pub(crate) fn layout_tree(
    agents: &[FleetAgent],
    layout: PaneLayout,
    bwoc_exe: &str,
) -> Option<Value> {
    let panes: Vec<Value> = agents
        .iter()
        .map(|a| {
            json!({
                "type": "pane",
                "label": a.id,
                "cwd": a.path,
                "command": spawn_argv(bwoc_exe, &a.path, &a.backend),
            })
        })
        .collect();
    if panes.is_empty() {
        return None;
    }
    Some(match layout {
        PaneLayout::Grid => {
            let n = panes.len();
            let cols = (1..=n).find(|c| c * c >= n).unwrap_or(n);
            let rows: Vec<Value> = panes
                .chunks(cols)
                .map(|row| even(row.to_vec(), "right"))
                .collect();
            even(rows, "down")
        }
        PaneLayout::Columns => even(panes, "right"),
        PaneLayout::Rows => even(panes, "down"),
        PaneLayout::MainVertical => main_and_rest(panes, "right", "down"),
        PaneLayout::MainHorizontal => main_and_rest(panes, "down", "right"),
    })
}

fn split(direction: &str, ratio: f64, first: Value, second: Value) -> Value {
    json!({
        "type": "split",
        "direction": direction,
        "ratio": ratio,
        "first": first,
        "second": second,
    })
}

/// `nodes` in equal shares along `direction`: a right-leaning chain where the
/// head takes `1/k` of what is left (k = nodes remaining). Ratios are rounded
/// to 3 places. `nodes` must be non-empty.
fn even(mut nodes: Vec<Value>, direction: &str) -> Value {
    if nodes.len() == 1 {
        return nodes.remove(0);
    }
    let ratio = (1000.0 / nodes.len() as f64).round() / 1000.0;
    let head = nodes.remove(0);
    split(direction, ratio, head, even(nodes, direction))
}

/// The first node as a main pane along `main_dir`; the rest evenly along
/// `rest_dir` beside it. `nodes` must be non-empty.
fn main_and_rest(mut nodes: Vec<Value>, main_dir: &str, rest_dir: &str) -> Value {
    let main = nodes.remove(0);
    if nodes.is_empty() {
        return main;
    }
    split(main_dir, MAIN_RATIO, main, even(nodes, rest_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet3() -> Vec<FleetAgent> {
        ["agent-pi:claude", "agent-ji:ollama", "agent-mu:codex"]
            .iter()
            .map(|s| {
                let (id, backend) = s.split_once(':').unwrap();
                FleetAgent {
                    id: id.into(),
                    path: format!("/ws/agents/{id}"),
                    backend: backend.into(),
                }
            })
            .collect()
    }

    fn pane(id: &str, backend: &str) -> Value {
        let path = format!("/ws/agents/{id}");
        json!({
            "type": "pane",
            "label": id,
            "cwd": path,
            "command": ["/bin/bwoc", "spawn", "--path", path, "--backend", backend],
        })
    }

    #[test]
    fn grid_of_three_is_two_rows_like_tmux_tiled() {
        let got = layout_tree(&fleet3(), PaneLayout::Grid, "/bin/bwoc").unwrap();
        let want = split(
            "down",
            0.5,
            split(
                "right",
                0.5,
                pane("agent-pi", "claude"),
                pane("agent-ji", "ollama"),
            ),
            pane("agent-mu", "codex"),
        );
        assert_eq!(got, want, "{}", serde_json::to_string_pretty(&got).unwrap());
    }

    #[test]
    fn main_vertical_puts_first_agent_left_and_stacks_the_rest() {
        let got = layout_tree(&fleet3(), PaneLayout::MainVertical, "/bin/bwoc").unwrap();
        let want = split(
            "right",
            MAIN_RATIO,
            pane("agent-pi", "claude"),
            split(
                "down",
                0.5,
                pane("agent-ji", "ollama"),
                pane("agent-mu", "codex"),
            ),
        );
        assert_eq!(got, want);
    }

    #[test]
    fn even_layouts_split_in_equal_shares() {
        let got = layout_tree(&fleet3(), PaneLayout::Columns, "/bin/bwoc").unwrap();
        assert_eq!(got["direction"], "right");
        assert_eq!(got["ratio"], 0.333);
        assert_eq!(got["second"]["ratio"], 0.5);
        let rows = layout_tree(&fleet3(), PaneLayout::Rows, "/bin/bwoc").unwrap();
        assert_eq!(rows["direction"], "down");
        // One agent → a bare pane; none → nothing to apply.
        let one = layout_tree(&fleet3()[..1], PaneLayout::MainHorizontal, "/bin/bwoc").unwrap();
        assert_eq!(one, pane("agent-pi", "claude"));
        assert!(layout_tree(&[], PaneLayout::Grid, "/bin/bwoc").is_none());
    }

    #[test]
    fn grid_rows_hold_ceil_sqrt_panes() {
        let five: Vec<FleetAgent> = (0..5)
            .map(|i| FleetAgent {
                id: format!("agent-{i}"),
                path: format!("/ws/agents/agent-{i}"),
                backend: "claude".into(),
            })
            .collect();
        let got = layout_tree(&five, PaneLayout::Grid, "bwoc").unwrap();
        // 5 → 3 per row: [0,1,2] then [3,4].
        assert_eq!(got["direction"], "down");
        assert_eq!(got["first"]["direction"], "right");
        assert_eq!(got["first"]["ratio"], 0.333);
        assert_eq!(got["second"]["first"]["label"], "agent-3");
        assert_eq!(got["second"]["second"]["label"], "agent-4");
    }

    fn ji_dirs() -> Vec<(String, PathBuf)> {
        vec![("agent-ji".to_string(), PathBuf::from("/ws/agents/agent-ji"))]
    }

    #[test]
    fn match_pane_prefers_label_then_detected_agent_by_foreground_cwd() {
        let panes = vec![
            json!({"pane_id":"w1:p1","label":"other","cwd":"/ws/agents/agent-ji"}),
            json!({"pane_id":"w1:p2","agent":"claude",
                   "foreground_cwd":"/ws/agents/agent-ji/src"}),
            json!({"pane_id":"w1:p3","label":"agent-ji"}),
        ];
        assert_eq!(
            match_pane(&panes, "agent-ji", ji_dirs).as_deref(),
            Some("w1:p3"),
            "label wins"
        );
        assert_eq!(
            match_pane(&panes[..2], "agent-ji", ji_dirs).as_deref(),
            Some("w1:p2"),
            "detected agent with foreground_cwd in the dir"
        );
        // `agents/agent-ji` never claims `agents/agent-jix`.
        let near = vec![json!({"pane_id":"w1:p9","agent":"claude",
                               "foreground_cwd":"/ws/agents/agent-jix"})];
        assert!(match_pane(&near, "agent-ji", ji_dirs).is_none());
    }

    #[test]
    fn match_pane_never_picks_a_shell_in_the_agent_dir() {
        // An operator's zsh (or vim) `cd`'d into the agent dir: no `agent`.
        let shell = vec![json!({"pane_id":"w1:p4","cwd":"/ws/agents/agent-ji",
                                "foreground_cwd":"/ws/agents/agent-ji"})];
        assert!(match_pane(&shell, "agent-ji", ji_dirs).is_none());
        let blank = vec![json!({"pane_id":"w1:p4","agent":" ",
                                "foreground_cwd":"/ws/agents/agent-ji"})];
        assert!(match_pane(&blank, "agent-ji", ji_dirs).is_none());
    }

    #[test]
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("/a b/c.sock"), "'/a b/c.sock'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn match_pane_ignores_pane_cwd() {
        // Only the pane cwd is in the agent dir — even with an agent detected,
        // the process in the foreground is elsewhere.
        let cwd_only = vec![
            json!({"pane_id":"w1:p5","cwd":"/ws/agents/agent-ji"}),
            json!({"pane_id":"w1:p6","agent":"claude","cwd":"/ws/agents/agent-ji",
                   "foreground_cwd":"/tmp"}),
            json!({"pane_id":"w1:p7","agent":"claude","cwd":"/ws/agents/agent-ji"}),
        ];
        assert!(match_pane(&cwd_only, "agent-ji", ji_dirs).is_none());
    }

    // ── against a fake herdr socket ──────────────────────────────────────────

    #[cfg(unix)]
    mod socket {
        use super::*;
        use crate::herdr::tests::fake_server;
        use std::sync::{Arc, Mutex};

        type Log = Arc<Mutex<Vec<(String, Value)>>>;

        /// A fake herdr that records every request and answers from `reply`
        /// (`None` → an `error` reply).
        fn recording<F>(reply: F) -> (tempfile::TempDir, PathBuf, Log)
        where
            F: Fn(&str, &Value) -> Option<Value> + Send + 'static,
        {
            let log: Log = Arc::default();
            let rec = log.clone();
            let (dir, sock) = fake_server(move |method, params, id| {
                rec.lock()
                    .unwrap()
                    .push((method.to_string(), params.clone()));
                Some(match reply(method, params) {
                    Some(result) => json!({ "id": id, "result": result }).to_string(),
                    None => {
                        json!({ "id": id, "error": {"code":"not_found","message":"x"} }).to_string()
                    }
                })
            });
            (dir, sock, log)
        }

        fn backend(sock: PathBuf) -> HerdrBackend {
            HerdrBackend::new(
                Some(sock),
                false,
                Some(PathBuf::from("/ws")),
                Duration::from_secs(2),
            )
        }

        fn methods(log: &Log) -> Vec<String> {
            log.lock().unwrap().iter().map(|(m, _)| m.clone()).collect()
        }

        #[test]
        fn open_fleet_creates_workspace_then_applies_the_tree() {
            let (_d, sock, log) = recording(|method, _| match method {
                "workspace.create" => Some(json!({
                    "type": "workspace_created",
                    "workspace": {"workspace_id": "w7", "label": "fleet-x"},
                    "tab": {"tab_id": "w7:t1"},
                    "root_pane": {"pane_id": "w7:p1"},
                })),
                "layout.apply" => Some(json!({"type": "layout_apply", "layout": {}})),
                _ => None,
            });
            let msg = backend(sock)
                .open_fleet("fleet-x", &fleet3(), PaneLayout::Grid, "/bin/bwoc")
                .unwrap();
            assert!(msg.contains("w7") && msg.contains("layout: grid"), "{msg}");

            let log = log.lock().unwrap();
            assert_eq!(log.len(), 2);
            let (m0, p0) = &log[0];
            assert_eq!(m0, "workspace.create");
            assert_eq!(p0["label"], "fleet-x");
            assert_eq!(p0["cwd"], "/ws");
            assert_eq!(p0["focus"], false);
            let (m1, p1) = &log[1];
            assert_eq!(m1, "layout.apply");
            assert_eq!(p1["workspace_id"], "w7");
            assert_eq!(p1["tab_id"], "w7:t1", "replaces the initial shell tab");
            assert_eq!(
                p1["root"],
                layout_tree(&fleet3(), PaneLayout::Grid, "/bin/bwoc").unwrap()
            );
        }

        #[test]
        fn open_fleet_main_vertical_sends_its_tree() {
            let (_d, sock, log) = recording(|method, _| match method {
                "workspace.create" => Some(json!({
                    "type": "workspace_created",
                    "workspace": {"workspace_id": "w2"},
                    "tab": {"tab_id": "w2:t1"},
                })),
                _ => Some(json!({"type": "layout_apply"})),
            });
            backend(sock)
                .open_fleet("s", &fleet3(), PaneLayout::MainVertical, "/bin/bwoc")
                .unwrap();
            let log = log.lock().unwrap();
            let root = &log[1].1["root"];
            assert_eq!(root["direction"], "right");
            assert_eq!(root["first"]["label"], "agent-pi");
            assert_eq!(
                root["first"]["command"],
                json!([
                    "/bin/bwoc",
                    "spawn",
                    "--path",
                    "/ws/agents/agent-pi",
                    "--backend",
                    "claude"
                ])
            );
            assert_eq!(root["second"]["direction"], "down");
        }

        #[test]
        fn open_fleet_layout_failure_names_the_cleanup() {
            let (_d, sock, _log) = recording(|method, _| match method {
                "workspace.create" => Some(json!({
                    "type": "workspace_created",
                    "workspace": {"workspace_id": "w3"},
                })),
                _ => None,
            });
            let err = backend(sock)
                .open_fleet("s", &fleet3(), PaneLayout::Rows, "bwoc")
                .unwrap_err();
            assert!(err.contains("herdr workspace close w3"), "{err}");
        }

        #[test]
        fn fleet_exists_and_hints_resolve_the_workspace_by_label() {
            let (_d, sock, _log) = recording(|method, _| match method {
                "workspace.list" => Some(json!({
                    "type": "workspace_list",
                    "workspaces": [
                        {"workspace_id": "w1", "label": "other"},
                        {"workspace_id": "w4", "label": "bwoc-fleet-x", "future": 1},
                    ],
                })),
                _ => None,
            });
            let b = backend(sock);
            assert!(b.fleet_exists("bwoc-fleet-x"));
            assert!(!b.fleet_exists("nope"));
            assert_eq!(b.kill_hint("bwoc-fleet-x"), "herdr workspace close w4");
            assert_eq!(
                b.attach_hint("bwoc-fleet-x"),
                "herdr workspace focus w4 && herdr"
            );
            assert!(b.attach_hint("nope").contains("'nope'"));
        }

        #[test]
        fn locate_by_label_then_by_detected_agent_cwd() {
            let (_d, sock, _log) = recording(|method, _| match method {
                "pane.list" => Some(json!({
                    "type": "pane_list",
                    "panes": [
                        {"pane_id": "w1:p1", "label": "agent-pi", "cwd": "/ws/agents/agent-pi"},
                        {"pane_id": "w1:p2", "cwd": "/elsewhere", "agent": "codex",
                         "foreground_cwd": "/ws/agents/agent-ji/sub"},
                        {"pane_id": "w1:p3", "cwd": "/ws/agents/agent-mu",
                         "foreground_cwd": "/ws/agents/agent-mu"},
                    ],
                })),
                _ => None,
            });
            let b = backend(sock);
            assert_eq!(b.locate_agent("agent-pi").unwrap().as_str(), "w1:p1");
            assert_eq!(b.locate_agent("agent-ji").unwrap().as_str(), "w1:p2");
            // w1:p3 is a plain shell in agent-mu's dir: never a wakeup target.
            assert!(b.locate_agent("agent-mu").is_none());
        }

        #[test]
        fn submit_text_sends_text_then_enter() {
            let (_d, sock, log) = recording(|_, _| Some(json!({"type": "ok"})));
            backend(sock).submit_text(&PaneHandle::new("w1:p2"), "[bwoc inbox m1 from a] hi");
            let log = log.lock().unwrap();
            assert_eq!(log.len(), 2);
            assert_eq!(log[0].0, "pane.send_text");
            assert_eq!(
                log[0].1,
                json!({"pane_id": "w1:p2", "text": "[bwoc inbox m1 from a] hi"})
            );
            assert_eq!(log[1].0, "pane.send_keys");
            assert_eq!(log[1].1, json!({"pane_id": "w1:p2", "keys": ["enter"]}));
        }

        #[test]
        fn submit_skips_enter_when_the_text_did_not_land() {
            let (_d, sock, log) = recording(|_, _| None);
            backend(sock).submit_text(&PaneHandle::new("w1:p2"), "x");
            assert_eq!(methods(&log), vec!["pane.send_text"]);
        }

        #[test]
        fn check_available_pings() {
            let (_d, sock, log) = recording(|_, _| Some(json!({"type": "pong"})));
            assert!(backend(sock).check_available().is_ok());
            assert_eq!(methods(&log), vec!["ping"]);
        }

        #[test]
        fn unavailable_herdr_refuses_with_a_remedy_and_locates_nothing() {
            let gone = PathBuf::from("/nonexistent/bwoc-herdr-p2b.sock");
            let b = backend(gone);
            let err = b.check_available().unwrap_err();
            assert!(err.contains("herdr is not answering"), "{err}");
            assert!(err.contains("--backend tmux"), "{err}");
            assert!(!b.fleet_exists("s"));
            assert!(b.locate_agent("agent-pi").is_none());
            // An error reply is "unavailable" too.
            let (_d, sock, _log) = recording(|_, _| None);
            assert!(backend(sock).check_available().is_err());
        }

        #[test]
        fn configured_socket_is_spelled_out_in_hints() {
            let b = HerdrBackend::new(
                Some(PathBuf::from("/nonexistent/h.sock")),
                true,
                None,
                Duration::from_millis(50),
            );
            assert_eq!(
                b.kill_hint("s"),
                "HERDR_SOCKET_PATH='/nonexistent/h.sock' herdr workspace list  \
                 (then `HERDR_SOCKET_PATH='/nonexistent/h.sock' herdr workspace close <id>` for 's')"
            );
        }

        #[test]
        fn configured_socket_with_space_and_quote_is_shell_quoted() {
            let b = HerdrBackend::new(
                Some(PathBuf::from("/nonexistent/my dir/it's; $(x).sock")),
                true,
                None,
                Duration::from_millis(50),
            );
            assert_eq!(
                b.env_prefix(),
                r"HERDR_SOCKET_PATH='/nonexistent/my dir/it'\''s; $(x).sock' "
            );
        }
    }
}
