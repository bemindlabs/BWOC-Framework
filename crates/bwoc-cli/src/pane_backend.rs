//! `PaneBackend` — the terminal-multiplexer operations BWOC performs, behind one
//! trait so a second multiplexer can sit beside tmux without touching the
//! commands that use it.
//!
//! Callers: `bwoc fleet term` (open + attach a fleet layout, in
//! `fleet_term.rs`) and the inbox wakeup in `bwoc send` (locate an agent's pane,
//! deliver text to it, in `send.rs`). Implementations: [`TmuxBackend`] (the
//! default) and [`crate::herdr_backend::HerdrBackend`]. Which one runs is
//! `bwoc fleet term --backend`, else `[fleet] pane_backend` in
//! `.bwoc/workspace.toml` (see [`configured_kind`]), else tmux. `bwoc send`
//! follows the workspace key only.
//!
//! Out of scope on purpose: `bwoc chat --tmux` (a tmux-named flag whose
//! window-vs-session launch is tmux-specific), `spawn.rs`'s pane detection, and
//! `sessions.rs`'s activity lookup.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::fleet_term::PaneLayout;

/// Which [`PaneBackend`] to use — the `--backend` value and the
/// `[fleet] pane_backend` workspace key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum PaneBackendKind {
    /// tmux panes (the default).
    Tmux,
    /// herdr panes, driven over herdr's local socket.
    Herdr,
}

impl PaneBackendKind {
    /// Interpret a `[fleet] pane_backend` value. Absent → tmux; `Err` carries
    /// an unrecognised value so the caller can name it.
    pub(crate) fn from_config(raw: Option<&str>) -> Result<Self, String> {
        match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            None | Some("tmux") => Ok(Self::Tmux),
            Some("herdr") => Ok(Self::Herdr),
            Some(_) => Err(raw.unwrap_or_default().to_string()),
        }
    }
}

/// Set once the unknown-value warning has been printed, so a group send that
/// wakes many recipients warns once per process, not once per recipient.
static UNKNOWN_KIND_WARNED: AtomicBool = AtomicBool::new(false);

/// The workspace's `[fleet] pane_backend`. An unrecognised value prints one
/// warning (prefixed `bwoc <cmd>:`) and falls back to tmux.
pub(crate) fn configured_kind(workspace: &Path, cmd: &str) -> PaneBackendKind {
    let raw = bwoc_core::workspace::FleetSettings::load(workspace).pane_backend;
    match PaneBackendKind::from_config(raw.as_deref()) {
        Ok(kind) => kind,
        Err(value) => {
            if !UNKNOWN_KIND_WARNED.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "bwoc {cmd}: warning: unknown [fleet] pane_backend = \"{value}\" in \
                     .bwoc/workspace.toml (expected \"tmux\" or \"herdr\") — using tmux."
                );
            }
            PaneBackendKind::Tmux
        }
    }
}

/// The backend for `kind` in `workspace`. `budget` bounds herdr's socket calls
/// per operation (tmux ignores it).
pub(crate) fn backend_for(
    kind: PaneBackendKind,
    workspace: &Path,
    budget: std::time::Duration,
) -> Box<dyn PaneBackend> {
    match kind {
        PaneBackendKind::Tmux => Box::new(TmuxBackend),
        PaneBackendKind::Herdr => Box::new(crate::herdr_backend::HerdrBackend::for_workspace(
            workspace, budget,
        )),
    }
}

/// The argv each fleet pane runs: `bwoc spawn` in the agent's directory with
/// its backend. Shared by every [`PaneBackend`] so all of them launch the
/// identical command.
pub(crate) fn spawn_argv(bwoc_exe: &str, path: &str, backend: &str) -> Vec<String> {
    vec![
        bwoc_exe.into(),
        "spawn".into(),
        "--path".into(),
        path.into(),
        "--backend".into(),
        backend.into(),
    ]
}

/// An opaque reference to one live pane (or session) that text can be
/// delivered to. The string is whatever the backend's own addressing uses: for
/// tmux a session name or a `%N` pane id; another backend can carry its own id
/// form (e.g. `w1:p2`). Callers only pass it back to the backend that made it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaneHandle(String);

impl PaneHandle {
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// One agent to lay out in a fleet: its id (pane title / name), its absolute
/// directory, and its BWOC backend name (passed to `bwoc spawn --backend`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FleetAgent {
    pub id: String,
    pub path: String,
    pub backend: String,
}

/// The multiplexer operations BWOC uses. Operator-facing strings returned by
/// these methods are printed verbatim by the caller (after its own
/// `bwoc <cmd>: ` prefix where the caller adds one).
pub(crate) trait PaneBackend {
    /// Short backend name, as an operator would type it (`"tmux"`).
    fn name(&self) -> &'static str;

    /// `Ok` when the multiplexer can be driven from here; `Err` carries the
    /// remedy to show the operator (how to install / enable it).
    fn check_available(&self) -> Result<(), String>;

    /// Whether a fleet named `session` is already open.
    fn fleet_exists(&self, session: &str) -> bool;

    /// Open one pane per agent (each running `bwoc spawn` in the agent's
    /// directory) in a new fleet named `session`, arranged by `layout`. Does
    /// not attach. `Ok` carries the one-line confirmation; `Err` the failure
    /// detail (including how to clean up a partial build).
    fn open_fleet(
        &self,
        session: &str,
        agents: &[FleetAgent],
        layout: PaneLayout,
        bwoc_exe: &str,
    ) -> Result<String, String>;

    /// Attach the current terminal to the fleet `session` (blocks until the
    /// operator detaches). `Err` only when the attach could not be started.
    fn attach_fleet(&self, session: &str) -> std::io::Result<()>;

    /// The command an operator runs to attach to `session` themselves.
    fn attach_hint(&self, session: &str) -> String;

    /// The command an operator runs to tear down `session`.
    fn kill_hint(&self, session: &str) -> String;

    /// Find the live pane belonging to agent `agent_id`, if any.
    fn locate_agent(&self, agent_id: &str) -> Option<PaneHandle>;

    /// Type `text` into `pane` and submit it. Best-effort: failures are
    /// swallowed (a wakeup must never fail the command that triggered it).
    fn submit_text(&self, pane: &PaneHandle, text: &str);
}

/// tmux — the multiplexer macOS and Linux both ship (or install identically).
pub(crate) struct TmuxBackend;

impl PaneBackend for TmuxBackend {
    fn name(&self) -> &'static str {
        "tmux"
    }

    fn check_available(&self) -> Result<(), String> {
        let missing = Command::new("tmux")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true);
        if missing {
            return Err(
                "tmux not found on PATH. Install it (macOS: `brew install tmux`, \
                 Linux: `apt install tmux`) — this command lays the fleet out in tmux panes."
                    .to_string(),
            );
        }
        Ok(())
    }

    fn fleet_exists(&self, session: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", session])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn open_fleet(
        &self,
        session: &str,
        agents: &[FleetAgent],
        layout: PaneLayout,
        bwoc_exe: &str,
    ) -> Result<String, String> {
        let tuples: Vec<(String, String, String)> = agents
            .iter()
            .map(|a| (a.id.clone(), a.path.clone(), a.backend.clone()))
            .collect();
        for cmd in tmux_fleet_commands(session, &tuples, layout, bwoc_exe) {
            match Command::new("tmux")
                .args(&cmd)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .status()
            {
                Ok(s) if s.success() => {}
                Ok(s) => {
                    return Err(format!(
                        "`tmux {}` exited {s} — the session may be partially built; \
                         `{}` to clear it.",
                        cmd.first().map(String::as_str).unwrap_or("?"),
                        self.kill_hint(session)
                    ));
                }
                Err(e) => return Err(format!("failed to run tmux: {e}")),
            }
        }
        Ok(format!(
            "Opened {} agent panes in tmux session '{}' (layout: {}). \
             Cycle layouts live with `<prefix> Space`.",
            agents.len(),
            session,
            layout.tmux_name()
        ))
    }

    fn attach_fleet(&self, session: &str) -> std::io::Result<()> {
        // Exit status deliberately ignored: a detach and a closed session both
        // end the attach, and neither is a `bwoc` failure.
        Command::new("tmux")
            .args(["attach", "-t", session])
            .status()
            .map(|_| ())
    }

    fn attach_hint(&self, session: &str) -> String {
        format!("tmux attach -t {session}")
    }

    fn kill_hint(&self, session: &str) -> String {
        format!("tmux kill-session -t {session}")
    }

    /// A whole session named for the agent (single-agent launches), else a
    /// **pane** titled for the agent — which is how `bwoc fleet term` tiles a
    /// fleet (one titled pane per agent) so a peer message still wakes the
    /// right tile, not just whatever pane is active. The handle is a session
    /// name or a pane id (`%N`); both are valid `send-keys -t` targets.
    fn locate_agent(&self, agent_id: &str) -> Option<PaneHandle> {
        if let Some(session) = resolve_tmux_session(agent_id) {
            return Some(PaneHandle::new(session));
        }
        let out = Command::new("tmux")
            .args(["list-panes", "-a", "-F", "#{pane_title}\t#{pane_id}"])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        match_pane_by_title(
            &String::from_utf8_lossy(&out.stdout),
            &tmux_session_candidates(agent_id),
        )
        .map(PaneHandle::new)
    }

    /// Two-step send (literal text → 200ms → Enter) — a single-call submission
    /// gets dropped by Claude Code's TUI input layer. The text goes via
    /// `send-keys -l` (literal) so a body containing a tmux key token (`Enter`,
    /// `C-c`, `;`, …) is injected verbatim, not reinterpreted as a keypress.
    /// Verified against a live Claude Code TUI: idle → submits immediately;
    /// mid-turn → queues and runs when the current turn ends.
    fn submit_text(&self, pane: &PaneHandle, text: &str) {
        let target = pane.as_str();
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", target, "-l", "--", text])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = Command::new("tmux")
            .args(["send-keys", "-t", target, "Enter"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Build the ordered tmux command sequence (each inner `Vec` is one `tmux`
/// invocation's argv, program name excluded) that opens one pane per agent in
/// `session` and arranges them by `layout`. Pure + tested.
///
/// `agents` is `(agent_id, abs_path, backend)`. The first agent seeds the
/// session window; each subsequent agent adds a `split-window`. The grid is
/// rebalanced with `tiled` *after every split* so tmux never runs out of room
/// ("no space for new pane") on a fleet of many agents; a final `select-layout`
/// applies the requested arrangement. Each pane's border is titled with the
/// agent id (`pane-border-status top` makes those visible).
pub(crate) fn tmux_fleet_commands(
    session: &str,
    agents: &[(String, String, String)],
    layout: PaneLayout,
    bwoc_exe: &str,
) -> Vec<Vec<String>> {
    // `--` ends tmux's own options; the rest is the shared pane argv.
    let spawn_argv = |path: &str, backend: &str| -> Vec<String> {
        let mut argv = vec!["--".to_string()];
        argv.extend(spawn_argv(bwoc_exe, path, backend));
        argv
    };
    let mut cmds: Vec<Vec<String>> = Vec::new();
    let Some(((id0, p0, b0), rest)) = agents.split_first() else {
        return cmds; // no agents — the caller refuses before running
    };

    // Detached session with the first agent in window 0.
    let mut first = vec![
        "new-session".into(),
        "-d".into(),
        "-s".into(),
        session.into(),
        "-n".into(),
        "fleet".into(),
    ];
    first.extend(spawn_argv(p0, b0));
    cmds.push(first);
    // Keep a pane visible after its agent exits (dead panes read "[exited]")
    // instead of closing — so one finished/crashed agent can't collapse the
    // layout (or the whole session, if it were the last pane). Set immediately
    // after the session exists, before any split.
    cmds.push(vec![
        "set-option".into(),
        "-t".into(),
        session.into(),
        "remain-on-exit".into(),
        "on".into(),
    ]);
    cmds.push(title_cmd(session, id0));

    // One pane per remaining agent; rebalance to `tiled` between splits so the
    // next split always has room, then title the freshly-created (active) pane.
    for (id, path, backend) in rest {
        let mut split = vec!["split-window".into(), "-t".into(), session.into()];
        split.extend(spawn_argv(path, backend));
        cmds.push(split);
        cmds.push(vec![
            "select-layout".into(),
            "-t".into(),
            session.into(),
            "tiled".into(),
        ]);
        cmds.push(title_cmd(session, id));
    }

    // Show the pane-border titles, then apply the requested layout last.
    cmds.push(vec![
        "set-option".into(),
        "-t".into(),
        session.into(),
        "pane-border-status".into(),
        "top".into(),
    ]);
    cmds.push(vec![
        "select-layout".into(),
        "-t".into(),
        session.into(),
        layout.tmux_name().into(),
    ]);
    cmds
}

/// Title the *active* pane of `session` with `id`.
fn title_cmd(session: &str, id: &str) -> Vec<String> {
    vec![
        "select-pane".into(),
        "-t".into(),
        session.into(),
        "-T".into(),
        id.into(),
    ]
}

/// Candidate tmux session names for an `agent-<x>` recipient, **most-specific
/// first** so a coincidentally-named session can't steal the wake: the
/// unambiguous `bwoc-agent-<x>` (what `bwoc chat --tmux` creates —
/// `new-session -s bwoc-<agent_id>` in `chat.rs`) and `agent-<x>` are tried
/// before the bare `<x>`, which is the most likely to collide with an unrelated
/// session. Targeting only the bare name silently missed bwoc-launched
/// sessions, so the wake never landed and the agent never woke.
fn tmux_session_candidates(to: &str) -> Vec<String> {
    let bare = to.strip_prefix("agent-").unwrap_or(to);
    vec![
        format!("bwoc-{to}"),   // bwoc-agent-<x> (bwoc chat --tmux) — most specific
        to.to_string(),         // agent-<x>      (full recipient id)
        format!("bwoc-{bare}"), // bwoc-<x>
        bare.to_string(),       // <x>            (bare / upstream — collision-prone, last)
    ]
}

/// First candidate session that tmux reports as live, if any.
fn resolve_tmux_session(to: &str) -> Option<String> {
    tmux_session_candidates(to).into_iter().find(|s| {
        Command::new("tmux")
            .args(["has-session", "-t", s])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|st| st.success())
            .unwrap_or(false)
    })
}

/// From a `tmux list-panes -a -F '#{pane_title}\t#{pane_id}'` listing, the pane
/// id (`%N`) of the first pane whose title matches one of `candidates`. Pure so
/// the title-matching is unit-testable without a running tmux server.
fn match_pane_by_title(listing: &str, candidates: &[String]) -> Option<String> {
    listing.lines().find_map(|line| {
        let (title, pane_id) = line.split_once('\t')?;
        candidates
            .iter()
            .any(|c| c == title)
            .then(|| pane_id.to_string())
    })
}

/// A recording [`PaneBackend`] for call-site tests: every trait call is logged
/// as one string, and the canned answers are set per test.
#[cfg(test)]
pub(crate) mod fake {
    use super::{FleetAgent, PaneBackend, PaneHandle};
    use crate::fleet_term::PaneLayout;
    use std::cell::RefCell;

    #[derive(Default)]
    pub(crate) struct FakeBackend {
        pub calls: RefCell<Vec<String>>,
        pub unavailable: bool,
        pub exists: bool,
        pub open_fails: bool,
        /// `Some(id)` → `locate_agent` returns a handle with this id.
        pub pane: Option<String>,
    }

    impl FakeBackend {
        fn log(&self, s: String) {
            self.calls.borrow_mut().push(s);
        }

        pub(crate) fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl PaneBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn check_available(&self) -> Result<(), String> {
            self.log("check_available".into());
            if self.unavailable {
                Err("fake missing".into())
            } else {
                Ok(())
            }
        }
        fn fleet_exists(&self, session: &str) -> bool {
            self.log(format!("fleet_exists {session}"));
            self.exists
        }
        fn open_fleet(
            &self,
            session: &str,
            agents: &[FleetAgent],
            layout: PaneLayout,
            bwoc_exe: &str,
        ) -> Result<String, String> {
            let ids: Vec<String> = agents
                .iter()
                .map(|a| format!("{}@{}:{}", a.id, a.path, a.backend))
                .collect();
            self.log(format!(
                "open_fleet {session} [{}] {layout:?} {bwoc_exe}",
                ids.join(",")
            ));
            if self.open_fails {
                Err("open broke".into())
            } else {
                Ok("opened".into())
            }
        }
        fn attach_fleet(&self, session: &str) -> std::io::Result<()> {
            self.log(format!("attach_fleet {session}"));
            Ok(())
        }
        fn attach_hint(&self, session: &str) -> String {
            self.log(format!("attach_hint {session}"));
            format!("fake attach {session}")
        }
        fn kill_hint(&self, session: &str) -> String {
            self.log(format!("kill_hint {session}"));
            format!("fake kill {session}")
        }
        fn locate_agent(&self, agent_id: &str) -> Option<PaneHandle> {
            self.log(format!("locate_agent {agent_id}"));
            self.pane.clone().map(PaneHandle::new)
        }
        fn submit_text(&self, pane: &PaneHandle, text: &str) {
            self.log(format!("submit_text {} {text}", pane.as_str()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agents() -> Vec<(String, String, String)> {
        vec![
            (
                "agent-pi".into(),
                "/ws/agents/agent-pi".into(),
                "claude".into(),
            ),
            (
                "agent-ji".into(),
                "/ws/agents/agent-ji".into(),
                "ollama".into(),
            ),
            (
                "agent-mu".into(),
                "/ws/agents/agent-mu".into(),
                "codex".into(),
            ),
        ]
    }

    #[test]
    fn first_agent_seeds_the_session_and_spawns() {
        let c = tmux_fleet_commands("bwoc-fleet", &agents(), PaneLayout::Grid, "/bin/bwoc");
        let first = &c[0];
        assert_eq!(first[0], "new-session");
        assert!(first.contains(&"-d".to_string()), "detached: {first:?}");
        assert!(first.windows(2).any(|w| w == ["-s", "bwoc-fleet"]));
        // spawns the first agent in its dir with its backend
        assert!(
            first
                .windows(2)
                .any(|w| w == ["--path", "/ws/agents/agent-pi"])
        );
        assert!(first.windows(2).any(|w| w == ["--backend", "claude"]));
        // remain-on-exit is set right after the session is created so an exiting
        // agent can't collapse the layout.
        assert!(
            c.iter()
                .any(|cmd| cmd.contains(&"remain-on-exit".to_string()))
        );
    }

    #[test]
    fn one_split_per_additional_agent_and_final_layout() {
        let c = tmux_fleet_commands("s", &agents(), PaneLayout::Columns, "bwoc");
        let splits = c
            .iter()
            .filter(|cmd| cmd.first().map(String::as_str) == Some("split-window"))
            .count();
        assert_eq!(splits, 2, "3 agents → 2 splits");
        // rebalanced to tiled between splits so tmux never runs out of room
        assert!(c.iter().any(
            |cmd| cmd.first().map(String::as_str) == Some("select-layout")
                && cmd.contains(&"tiled".to_string())
        ));
        // the LAST command applies the requested layout
        let last = c.last().unwrap();
        assert_eq!(last[0], "select-layout");
        assert_eq!(last.last().unwrap(), "even-horizontal");
    }

    #[test]
    fn every_agent_pane_is_titled() {
        let c = tmux_fleet_commands("s", &agents(), PaneLayout::Grid, "bwoc");
        for id in ["agent-pi", "agent-ji", "agent-mu"] {
            assert!(
                c.iter()
                    .any(|cmd| cmd.first().map(String::as_str) == Some("select-pane")
                        && cmd.contains(&id.to_string())),
                "pane titled for {id}"
            );
        }
        assert!(
            c.iter()
                .any(|cmd| cmd.contains(&"pane-border-status".to_string()))
        );
    }

    #[test]
    fn no_agents_yields_no_commands() {
        assert!(tmux_fleet_commands("s", &[], PaneLayout::Grid, "bwoc").is_empty());
    }

    #[test]
    fn match_pane_by_title_finds_the_fleet_tile() {
        // `tmux fleet term` titles each pane with the agent id; a peer message to
        // agent-ji must resolve to that pane (%7), not just any active pane.
        let listing = "agent-pi\t%3\nagent-ji\t%7\nagent-mu\t%9";
        let got = match_pane_by_title(listing, &tmux_session_candidates("agent-ji"));
        assert_eq!(got.as_deref(), Some("%7"));
        // bare-name pane title also matches (via the candidate set).
        let bare = "ji\t%2\nother\t%4";
        assert_eq!(
            match_pane_by_title(bare, &tmux_session_candidates("agent-ji")).as_deref(),
            Some("%2")
        );
        // no matching title → None.
        assert!(match_pane_by_title("zzz\t%1", &tmux_session_candidates("agent-ji")).is_none());
    }

    #[test]
    fn tmux_candidates_cover_the_launch_conventions() {
        let c = tmux_session_candidates("agent-pi");
        // bare (upstream / manual), full id, and the bwoc chat --tmux name.
        assert!(c.contains(&"pi".to_string()), "bare name");
        assert!(c.contains(&"agent-pi".to_string()), "full recipient id");
        assert!(
            c.contains(&"bwoc-agent-pi".to_string()),
            "bwoc chat --tmux session (was silently missed): {c:?}"
        );
    }

    #[test]
    fn pane_backend_config_values() {
        use PaneBackendKind::*;
        assert_eq!(
            PaneBackendKind::from_config(None),
            Ok(Tmux),
            "absent → tmux"
        );
        assert_eq!(PaneBackendKind::from_config(Some("tmux")), Ok(Tmux));
        assert_eq!(PaneBackendKind::from_config(Some("herdr")), Ok(Herdr));
        assert_eq!(PaneBackendKind::from_config(Some(" Herdr ")), Ok(Herdr));
        assert_eq!(
            PaneBackendKind::from_config(Some("zellij")),
            Err("zellij".to_string())
        );
        assert_eq!(PaneBackendKind::from_config(Some("")), Err(String::new()));
    }

    #[test]
    fn tmux_panes_run_the_shared_spawn_argv() {
        let c = tmux_fleet_commands("s", &agents(), PaneLayout::Grid, "/bin/bwoc");
        let mut want = vec!["--".to_string()];
        want.extend(spawn_argv("/bin/bwoc", "/ws/agents/agent-pi", "claude"));
        assert!(c[0].ends_with(&want), "{:?}", c[0]);
    }

    #[test]
    fn tmux_hints_match_the_strings_operators_were_shown() {
        // These were inlined in fleet_term.rs before the trait; the wording is
        // operator-facing, so the extraction must not change it.
        let t = TmuxBackend;
        assert_eq!(t.name(), "tmux");
        assert_eq!(t.attach_hint("s1"), "tmux attach -t s1");
        assert_eq!(t.kill_hint("s1"), "tmux kill-session -t s1");
    }
}
