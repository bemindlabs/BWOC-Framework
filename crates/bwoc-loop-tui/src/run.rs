//! Spawning + monitoring the L1 goal-loop subprocess for `bwoc loop`.
//!
//! The control center *drives* a goal-loop by launching a sibling
//! `bwoc-harness --lead --loop` as a **runtime subprocess** — never a build
//! dependency (the dep-quarantine the crate rests on). This module owns that
//! child: it captures both of the harness's output streams into a scrolling log,
//! polls the child for liveness, and tears it down via `Drop` so an early return
//! or panic can't leak a zombie.
//!
//! ## Process-group teardown
//!
//! The lead spawns a worker subprocess per task, and those workers inherit the
//! lead's (piped) stdout/stderr. If teardown killed only the lead, a mid-flight
//! worker would be orphaned *holding the capture pipe open* — the reader threads
//! would never see EOF. So on Unix the lead is spawned as its own process-group
//! leader and teardown SIGKILLs the whole group, reaping the workers too. The
//! reader threads are then left **detached** (never joined): killing the group
//! closes the pipes so they end on their own, and detaching guarantees teardown
//! can never *block* even on a platform where a stray worker couldn't be
//! group-killed.
//!
//! ## Why both stdout and stderr
//!
//! The lead splits its output: the banner, the `mode:` / `goal-loop:` header, and
//! the three final `GoalLoopOutcome` summary lines go to **stdout**, while the
//! per-task `[bwoc-harness] lead: …` progress lines go to **stderr**. A log pane
//! needs both, so both are piped and each gets its own reader thread feeding one
//! `mpsc` channel. Ordering between the two streams is not guaranteed, which is
//! fine for a human-readable tail.
//!
//! ## Outcome is in the text, not the exit code
//!
//! The harness exits `0` for Done, Blocked, **and** BudgetExhausted alike — the
//! terminal state is conveyed only by the final stdout summary line. So the run
//! parses that line ([`parse_outcome`]); a non-zero exit (or no summary line at
//! all) means a hard failure (bad `--tasks`, unreachable backend, not a git
//! repo), surfaced as such.

use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use ratatui::style::Color;

/// The terminal state of a finished goal-loop, parsed from the harness's final
/// stdout summary line (the exit code cannot distinguish these — see module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    Blocked,
    BudgetExhausted,
}

/// Parse the harness's final goal-loop summary line into an [`Outcome`].
///
/// The three lines (from `run_lead_mode`) are, verbatim shapes:
/// - `goal reached in N iteration(s): C completed, F failed.`             → Done
/// - `goal loop stopped after N iteration(s) — blocked: … (…).`           → Blocked
/// - `goal loop hit its N-iteration budget before DoD (…).`               → BudgetExhausted
///
/// Matched on the **full shape** of each line (prefix + interior markers + tail),
/// not just a prefix. The lead's per-task worker subprocesses inherit its stdout,
/// so arbitrary worker/LLM output can interleave into the same stream; a loose
/// prefix match could let such a line false-positive as a terminal outcome and
/// freeze the loop as falsely finished. The strict shape makes that vanishingly
/// unlikely. Any other line returns `None` (ordinary log output).
pub(crate) fn parse_outcome(line: &str) -> Option<Outcome> {
    let l = line.trim();
    if l.starts_with("goal reached in")
        && l.contains("iteration(s):")
        && l.contains("completed,")
        && l.ends_with("failed.")
    {
        Some(Outcome::Done)
    } else if l.starts_with("goal loop stopped after")
        && l.contains("blocked:")
        && l.ends_with(").")
    {
        Some(Outcome::Blocked)
    } else if l.starts_with("goal loop hit its")
        && l.contains("-iteration budget before DoD")
        && l.ends_with(").")
    {
        Some(Outcome::BudgetExhausted)
    } else {
        None
    }
}

/// Decide the status transition when `line` is drained, or `None` to keep the
/// current status. Pure — this is the core of [`LoopRun::drain`]'s outcome
/// recovery, isolated so the reap/deliver-ordering logic is unit-testable
/// without a live child.
///
/// - `Running` + a summary line → `Finished { Some(o), code: None }` (code is
///   topped up by `poll` on reap).
/// - `Finished { None, code }` + a summary line → `Finished { Some(o), code }`
///   (recovers the outcome when `poll` reaped the child before the summary line
///   arrived — otherwise a successful run would render as a code-only failure).
/// - `Finished { Some(_), .. }` → keep it: the first summary line wins.
fn upgraded_status(status: RunStatus, line: &str) -> Option<RunStatus> {
    let o = parse_outcome(line)?;
    match status {
        RunStatus::Running => Some(RunStatus::Finished {
            outcome: Some(o),
            code: None,
        }),
        RunStatus::Finished {
            outcome: None,
            code,
        } => Some(RunStatus::Finished {
            outcome: Some(o),
            code,
        }),
        RunStatus::Finished {
            outcome: Some(_), ..
        } => None,
    }
}

/// Live status of the loop subprocess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunStatus {
    Running,
    /// The child exited. `outcome` is the parsed summary (if one was seen);
    /// `code` is the raw process exit code (`None` if killed by signal).
    Finished {
        outcome: Option<Outcome>,
        code: Option<i32>,
    },
}

/// Render-ready `(text, colour)` for a run's status header — pure over the parts
/// so it is unit-testable without a live `Child`.
pub(crate) fn status_label(status: &RunStatus, team: &str, running_secs: u64) -> (String, Color) {
    match status {
        RunStatus::Running => (
            format!("running · team {team} · {running_secs}s"),
            Color::Yellow,
        ),
        RunStatus::Finished { outcome, code } => match outcome {
            Some(Outcome::Done) => (format!("done ✓ · team {team}"), Color::Green),
            Some(Outcome::Blocked) => (format!("blocked · team {team}"), Color::Red),
            Some(Outcome::BudgetExhausted) => {
                (format!("budget exhausted · team {team}"), Color::Magenta)
            }
            // Exited with no recognizable summary line → a hard failure
            // (bad --tasks, unreachable backend, not a git repo, …).
            None => (
                format!(
                    "exited (code {}) · team {team} — see log",
                    code.map(|c| c.to_string())
                        .unwrap_or_else(|| "killed".into())
                ),
                Color::Red,
            ),
        },
    }
}

/// A bounded, scrolling log of the harness's output lines (oldest dropped once
/// full). Kept small — a goal-loop is not chatty and only the tail is shown.
pub(crate) struct LogBuf {
    lines: std::collections::VecDeque<String>,
    cap: usize,
}

impl LogBuf {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            lines: std::collections::VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub(crate) fn push(&mut self, line: String) {
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The last `n` lines, oldest-first (for rendering a bottom-anchored tail).
    pub(crate) fn tail(&self, n: usize) -> impl Iterator<Item = &str> {
        let start = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(start).map(|s| s.as_str())
    }
}

/// Everything needed to launch a goal-loop over one team's task list.
pub(crate) struct LaunchSpec {
    /// Resolved sibling `bwoc-harness` path.
    pub(crate) harness: PathBuf,
    /// `<workspace>/.bwoc/teams/<id>/tasks.jsonl`.
    pub(crate) tasks_path: PathBuf,
    /// `--workdir` — the workspace root the harness worktrees hang off.
    pub(crate) workdir: PathBuf,
    pub(crate) interval_secs: u64,
    pub(crate) max_iters: usize,
    pub(crate) backend: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) endpoint: Option<String>,
}

impl LaunchSpec {
    /// Build the `bwoc-harness` argv (everything after the program name). Pure,
    /// so the flag wiring is unit-testable without spawning.
    pub(crate) fn args(&self) -> Vec<String> {
        let mut a = vec![
            "--lead".to_string(),
            "--loop".to_string(),
            "--tasks".to_string(),
            self.tasks_path.to_string_lossy().into_owned(),
            "--workdir".to_string(),
            self.workdir.to_string_lossy().into_owned(),
            "--loop-interval-secs".to_string(),
            self.interval_secs.to_string(),
            "--loop-max-iters".to_string(),
            self.max_iters.to_string(),
        ];
        if let Some(b) = &self.backend {
            a.push("--backend".into());
            a.push(b.clone());
        }
        if let Some(m) = &self.model {
            a.push("--model".into());
            a.push(m.clone());
        }
        if let Some(e) = &self.endpoint {
            a.push("--endpoint".into());
            a.push(e.clone());
        }
        a
    }
}

/// A running (or finished) goal-loop subprocess with its captured log.
pub(crate) struct LoopRun {
    child: Child,
    /// The lead's pid, also its process-group id on Unix (it is spawned as a new
    /// group leader) — used to kill the whole group on teardown.
    pid: u32,
    /// Whether the child has been reaped (`wait`/`try_wait` returned its status).
    /// Once reaped, the OS may recycle its pid (== pgid), so we must **never**
    /// `kill(-pgid)` again — doing so risks signalling an unrelated process group.
    reaped: bool,
    rx: Receiver<String>,
    /// Reader-thread handles. Deliberately **not joined** on teardown (see
    /// [`LoopRun::drop`]) — dropping the `Vec` detaches them, which is what keeps
    /// a stuck reader from ever hanging the TUI.
    _readers: Vec<JoinHandle<()>>,
    pub(crate) team: String,
    pub(crate) started: Instant,
    pub(crate) status: RunStatus,
    pub(crate) log: LogBuf,
}

/// Spawn a reader thread that forwards each line of `stream` to `tx` verbatim.
/// Ends on EOF (child stream closed) or when the receiver is gone.
fn spawn_reader<R: std::io::Read + Send + 'static>(
    stream: R,
    tx: Sender<String>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let buf = BufReader::new(stream);
        for line in buf.lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    })
}

impl LoopRun {
    /// Launch `bwoc-harness --lead --loop …` for a team, capturing both output
    /// streams. `team` is carried for display only (it is already validated as a
    /// safe path segment before its tasks path is built).
    pub(crate) fn spawn(spec: LaunchSpec, team: String) -> Result<Self, String> {
        let mut cmd = Command::new(&spec.harness);
        cmd.args(spec.args())
            // The lead loop has no stdin command protocol; give it nothing (and
            // never the TUI's raw-mode stdin).
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Put the lead in its OWN process group (leader). Its per-task worker
        // subprocesses inherit the group, so teardown can SIGKILL the whole group
        // at once — otherwise a killed lead orphans a mid-flight worker that
        // keeps the inherited capture pipe open, and the reader threads never see
        // EOF. Group id == the lead's pid.
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn bwoc-harness: {e}"))?;
        let pid = child.id();

        let (tx, rx) = mpsc::channel::<String>();
        let mut readers = Vec::new();
        if let Some(out) = child.stdout.take() {
            readers.push(spawn_reader(out, tx.clone()));
        }
        if let Some(err) = child.stderr.take() {
            readers.push(spawn_reader(err, tx));
        }

        Ok(Self {
            child,
            pid,
            reaped: false,
            rx,
            _readers: readers,
            team,
            started: Instant::now(),
            status: RunStatus::Running,
            log: LogBuf::new(500),
        })
    }

    pub(crate) fn is_running(&self) -> bool {
        matches!(self.status, RunStatus::Running)
    }

    /// Drain all buffered output lines into the log, recording a parsed outcome
    /// when a summary line appears. Non-blocking; call once per UI tick.
    ///
    /// The upgrade also fires when the status is already `Finished { outcome:
    /// None }`: the child can be reaped by `poll` (via `try_wait`) *before* the
    /// summary line has travelled reader-thread → channel, which would otherwise
    /// seal a successful run as a code-only hard failure. Recovering the outcome
    /// on a later drain — while preserving any exit code already recorded — makes
    /// the labelling correct regardless of the reap/deliver ordering.
    pub(crate) fn drain(&mut self) {
        // try_recv errors (Empty or Disconnected) end the drain for this tick.
        while let Ok(line) = self.rx.try_recv() {
            if let Some(next) = upgraded_status(self.status, &line) {
                self.status = next;
            }
            self.log.push(line);
        }
    }

    /// Poll the child for exit (the two reader threads make channel-disconnect an
    /// unreliable liveness signal, so ask the OS directly). Once reaped, records
    /// the exit code, preserving any outcome already parsed from the log.
    pub(crate) fn poll(&mut self) {
        // Once the child is reaped its pid may be recycled — never touch it again.
        if self.reaped {
            return;
        }
        if !self.is_running() {
            // Already Finished from a parsed summary line, but the code may still
            // be None until the process reaps — top it up once.
            if let RunStatus::Finished {
                code: None,
                outcome,
            } = self.status
                && let Ok(Some(exit)) = self.child.try_wait()
            {
                self.reaped = true;
                self.status = RunStatus::Finished {
                    outcome,
                    code: exit.code(),
                };
            }
            return;
        }
        if let Ok(Some(exit)) = self.child.try_wait() {
            // Reaped while still marked Running → no summary line was seen (hard
            // failure). Keep outcome None so the status renders as an error.
            self.reaped = true;
            self.status = RunStatus::Finished {
                outcome: None,
                code: exit.code(),
            };
        }
    }

    /// Kill the lead **and its worker subprocesses** in one shot. On Unix the
    /// lead is a process-group leader, so a group SIGKILL (`kill(-pgid)`) reaps
    /// the whole tree — including a mid-flight worker that inherited the capture
    /// pipe, which is what lets the reader threads reach EOF. Elsewhere fall back
    /// to killing the lead alone.
    fn kill_tree(&mut self) {
        #[cfg(unix)]
        {
            // SAFETY: `kill(2)` with a negative pid targets the process group of
            // that id. `self.pid` is our own child, spawned as its own group
            // leader (`process_group(0)`), so the group id equals the pid; SIGKILL
            // is always deliverable and cannot be caught, so no worker survives.
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
    }

    /// Kill the whole group and reap the lead — but **only if not already
    /// reaped**. This is the one place a signal is sent, and it is guarded so a
    /// recycled pid (after `poll` reaped the child) is never signalled. Returns
    /// the reaped exit code, or `None` when nothing was killed (already reaped)
    /// or the lead died by signal.
    fn kill_and_reap(&mut self) -> Option<i32> {
        if self.reaped {
            return None;
        }
        self.kill_tree();
        let code = self.child.wait().ok().and_then(|s| s.code());
        self.reaped = true;
        code
    }

    /// Operator-requested stop: kill the whole group and reap the lead. Idempotent
    /// and pid-reuse-safe — a no-op signal-wise if the loop already finished.
    pub(crate) fn stop(&mut self) {
        let killed_code = self.kill_and_reap();
        // Preserve a parsed outcome + any code already recorded (e.g. the loop
        // finished on its own just before `x`); otherwise take the code from our
        // own reap. A signal-killed lead has no exit code → renders as "killed".
        let (outcome, prior_code) = match self.status {
            RunStatus::Finished { outcome, code } => (outcome, code),
            RunStatus::Running => (None, None),
        };
        self.status = RunStatus::Finished {
            outcome,
            code: prior_code.or(killed_code),
        };
    }

    /// `(text, colour)` for the status header.
    pub(crate) fn status_label(&self) -> (String, Color) {
        status_label(&self.status, &self.team, self.started.elapsed().as_secs())
    }
}

impl Drop for LoopRun {
    fn drop(&mut self) {
        // Best-effort teardown: kill the whole process group (lead + any workers)
        // then reap the lead. All ignored — a dying TUI must not panic here.
        //
        // The reader threads are deliberately **not joined**: they are detached
        // (the `_readers` handles just drop). Killing the group closes the capture
        // pipes so the readers hit EOF and end on their own; detaching is the
        // belt-and-suspenders that guarantees teardown can never *block* even if a
        // reader is wedged (e.g. a non-Unix fallback that couldn't group-kill a
        // stray worker). A lingering detached reader is reaped at process exit.
        //
        // `kill_and_reap` is a no-op if `poll` already reaped the child — vital,
        // since the pid (== pgid) may have been recycled by then.
        self.kill_and_reap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real subprocess exercise (Unix): use `/bin/echo` as a stand-in harness —
    /// it prints the argv and exits 0 immediately. Verifies spawn → drain → poll
    /// reaps the child, marks `reaped`, and that a subsequent `stop()` is a
    /// no-op signal-wise (the pid-reuse guard) and doesn't panic.
    #[cfg(unix)]
    #[test]
    fn reaped_guard_holds_after_child_exits() {
        let spec = LaunchSpec {
            harness: PathBuf::from("/bin/echo"),
            tasks_path: PathBuf::from("t.jsonl"),
            workdir: PathBuf::from("."),
            interval_secs: 1,
            max_iters: 0,
            backend: None,
            model: None,
            endpoint: None,
        };
        let mut run = LoopRun::spawn(spec, "squad".into()).expect("spawn /bin/echo");
        // echo exits at once; poll until reaped (bounded so a hang can't wedge CI).
        for _ in 0..400 {
            run.drain();
            run.poll();
            if !run.is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!run.is_running(), "echo should have exited");
        assert!(run.reaped, "poll must mark the child reaped");

        // The captured output contains the argv we passed (proves the pipe +
        // reader-thread path works end to end).
        //
        // Reaping the child and having its output land in `log` are INDEPENDENT
        // events: the loop above breaks on the `poll()` that observes the exit,
        // but its last `drain()` ran *before* that poll, so anything the reader
        // thread delivers in between is still in flight. On a fast machine the
        // loop spins enough times to pick it up; on a loaded CI runner the child
        // can exit before the first poll while the reader thread has not been
        // scheduled at all — which is exactly how this test flaked on `main`
        // (`argv not captured:` with an empty tail). So drain until the output
        // actually arrives, bounded so a genuine regression still fails fast.
        let mut joined = String::new();
        for _ in 0..400 {
            run.drain();
            joined = run.log.tail(100).collect::<Vec<_>>().join(" ");
            if joined.contains("--lead") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(joined.contains("--lead"), "argv not captured: {joined}");
        // stop() after reap: pid-reuse guard → no signal, no panic, still Finished.
        run.stop();
        assert!(run.reaped);
        assert!(matches!(run.status, RunStatus::Finished { .. }));
    }

    #[test]
    fn parse_outcome_matches_the_three_summary_shapes() {
        assert_eq!(
            parse_outcome("goal reached in 3 iteration(s): 5 completed, 0 failed."),
            Some(Outcome::Done)
        );
        assert_eq!(
            parse_outcome(
                "goal loop stopped after 2 iteration(s) — blocked: 1 task(s) not completed and none claimable (dependency-blocked) (0 completed so far)."
            ),
            Some(Outcome::Blocked)
        );
        assert_eq!(
            parse_outcome(
                "goal loop hit its 20-iteration budget before DoD (2 completed, 1 failed)."
            ),
            Some(Outcome::BudgetExhausted)
        );
        // Leading-space (the header lines are two-space indented) still parses.
        assert_eq!(
            parse_outcome("  goal reached in 1 iteration(s): 1 completed, 0 failed."),
            Some(Outcome::Done)
        );
    }

    #[test]
    fn parse_outcome_ignores_ordinary_log_lines() {
        assert_eq!(
            parse_outcome("[bwoc-harness] lead: `t1` done — wrote parser"),
            None
        );
        assert_eq!(
            parse_outcome("  goal-loop: drive tasks → all Completed (ticker 5s, budget 20 iters)"),
            None
        );
        assert_eq!(parse_outcome(""), None);
        // A blocked-shaped line missing the "blocked:" marker must not match.
        assert_eq!(
            parse_outcome("goal loop stopped after 2 iteration(s)"),
            None
        );
        // Strict shape: a worker/LLM line that merely borrows the "goal reached
        // in" prefix but lacks the summary tail must NOT false-positive.
        assert_eq!(
            parse_outcome("goal reached in the design doc, see notes"),
            None
        );
        assert_eq!(parse_outcome("goal loop hit its stride today"), None);
    }

    #[test]
    fn upgraded_status_recovers_outcome_after_early_reap() {
        let done = "goal reached in 1 iteration(s): 1 completed, 0 failed.";
        // Running + summary → Finished{Some, code:None}.
        assert_eq!(
            upgraded_status(RunStatus::Running, done),
            Some(RunStatus::Finished {
                outcome: Some(Outcome::Done),
                code: None
            })
        );
        // The reap-race: poll already sealed Finished{None, code:0}; a later drain
        // of the summary line must recover the outcome AND keep the code.
        assert_eq!(
            upgraded_status(
                RunStatus::Finished {
                    outcome: None,
                    code: Some(0)
                },
                done
            ),
            Some(RunStatus::Finished {
                outcome: Some(Outcome::Done),
                code: Some(0)
            })
        );
        // An already-resolved outcome is not overwritten (first summary wins).
        assert_eq!(
            upgraded_status(
                RunStatus::Finished {
                    outcome: Some(Outcome::Blocked),
                    code: Some(0)
                },
                done
            ),
            None
        );
        // A non-summary line never transitions.
        assert_eq!(
            upgraded_status(RunStatus::Running, "[bwoc-harness] lead: `t1` done"),
            None
        );
    }

    #[test]
    fn status_label_colours_each_state() {
        assert_eq!(
            status_label(&RunStatus::Running, "squad", 12).1,
            Color::Yellow
        );
        assert_eq!(
            status_label(
                &RunStatus::Finished {
                    outcome: Some(Outcome::Done),
                    code: Some(0)
                },
                "squad",
                0
            )
            .1,
            Color::Green
        );
        assert_eq!(
            status_label(
                &RunStatus::Finished {
                    outcome: Some(Outcome::Blocked),
                    code: Some(0)
                },
                "squad",
                0
            )
            .1,
            Color::Red
        );
        assert_eq!(
            status_label(
                &RunStatus::Finished {
                    outcome: Some(Outcome::BudgetExhausted),
                    code: Some(0)
                },
                "squad",
                0
            )
            .1,
            Color::Magenta
        );
        // Exited with no summary → error (red), text mentions the code.
        let (txt, color) = status_label(
            &RunStatus::Finished {
                outcome: None,
                code: Some(1),
            },
            "squad",
            0,
        );
        assert_eq!(color, Color::Red);
        assert!(txt.contains("code 1"), "got: {txt}");
    }

    #[test]
    fn launch_spec_args_wire_the_required_flags() {
        let spec = LaunchSpec {
            harness: PathBuf::from("/x/bwoc-harness"),
            tasks_path: PathBuf::from("/ws/.bwoc/teams/squad/tasks.jsonl"),
            workdir: PathBuf::from("/ws"),
            interval_secs: 5,
            max_iters: 20,
            backend: None,
            model: None,
            endpoint: None,
        };
        let a = spec.args();
        // Required flags present in order-independent form.
        for want in [
            "--lead",
            "--loop",
            "--tasks",
            "/ws/.bwoc/teams/squad/tasks.jsonl",
            "--workdir",
            "/ws",
            "--loop-interval-secs",
            "5",
            "--loop-max-iters",
            "20",
        ] {
            assert!(a.iter().any(|x| x == want), "missing {want} in {a:?}");
        }
        // No optional flags when unset.
        assert!(!a.iter().any(|x| x == "--backend"));
        assert!(!a.iter().any(|x| x == "--model"));
        assert!(!a.iter().any(|x| x == "--endpoint"));
    }

    #[test]
    fn launch_spec_args_include_optional_backend_model_endpoint() {
        let spec = LaunchSpec {
            harness: PathBuf::from("bwoc-harness"),
            tasks_path: PathBuf::from("t.jsonl"),
            workdir: PathBuf::from("."),
            interval_secs: 1,
            max_iters: 0,
            backend: Some("ollama".into()),
            model: Some("gemma4".into()),
            endpoint: Some("http://localhost:11434/v1".into()),
        };
        let a = spec.args();
        let pair = |flag: &str, val: &str| a.windows(2).any(|w| w[0] == flag && w[1] == val);
        assert!(pair("--backend", "ollama"));
        assert!(pair("--model", "gemma4"));
        assert!(pair("--endpoint", "http://localhost:11434/v1"));
        // Unbounded budget still passes through as 0.
        assert!(pair("--loop-max-iters", "0"));
    }

    #[test]
    fn logbuf_caps_and_tails() {
        let mut b = LogBuf::new(3);
        for i in 0..5 {
            b.push(format!("line{i}"));
        }
        assert_eq!(b.tail(99).count(), 3); // capped: oldest dropped
        let tail: Vec<&str> = b.tail(2).collect();
        assert_eq!(tail, vec!["line3", "line4"]);
        // tail(n) larger than len returns everything.
        let all: Vec<&str> = b.tail(99).collect();
        assert_eq!(all, vec!["line2", "line3", "line4"]);
    }
}
