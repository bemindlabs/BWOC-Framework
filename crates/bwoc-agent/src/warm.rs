//! Warm (served) execution of **trusted** Saṅgha tasks — issue #301, PR B.
//!
//! Without warm mode, `bwoc-agent --serve` with `BWOC_AUTO_CLAIM=1` claims a
//! newly-available team task and then *tmux-wakes* a human/Claude session to do
//! it — paying a fresh backend cold-start per task. Warm mode keeps **one
//! resident `bwoc-harness --headless` process** loaded and feeds each claimed
//! task into it, so successive tasks reuse the warm provider/tools/conversation
//! instead of cold-starting.
//!
//! **Trust posture.** This path handles the agent's *own team queue* — tasks
//! added by the operator or a trusted teammate (`bwoc task add`). They are
//! injected as a **trusted** [`Principal::LocalOperator`], so the agent may use
//! effectful tools to actually *do* the work. This is the deliberate inverse of
//! [`crate::autoprocess`], which feeds **untrusted** internet-relayed input to a
//! read-only `--chat` harness with every permission auto-denied. The two never
//! mix: untrusted input is never routed here.
//!
//! **Safety.** Even though `--headless` auto-approves `ask`-mode tools (no human
//! to prompt), the harness's layer-1 guardrails, policy `deny` rules, and the
//! Phase-5 worktree sandbox still confine every tool. Warm mode is therefore
//! restricted to **confined (harness) backends**; an ambient backend (`cli`,
//! whose vendor CLI runs tools outside the harness) is refused — same fail-closed
//! rule as autoprocess.
//!
//! **Governance.** A task with `requires_plan` (Pavāraṇā: lead must approve a
//! plan before completion) is **not** auto-executed — the daemon can't stand in
//! for the lead's approval, so such a task falls back to the tmux-wake path.
//!
//! Opt-in: `BWOC_WARM=1`. Off by default (announce/wake behaviour is unchanged).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use bwoc_core::manifest::Manifest;
use bwoc_core::team;
use bwoc_core::trust::Principal;

/// Cap on harness output lines read per task — a backstop against a runaway
/// child flooding stdout (the loop also ends on the turn's `Message`/`TurnEnd`).
const MAX_EVENT_LINES: usize = 100_000;

/// Default idle timeout: the resident harness self-exits after this long with no
/// task, freeing the model's GPU/RAM. It respawns on the next claimed task.
const DEFAULT_IDLE: Duration = Duration::from_secs(600);

/// The resident harness child plus its piped stdio handles.
struct Resident {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// Lazily-spawned, idle-reaped resident `bwoc-harness --headless` that executes
/// claimed trusted tasks.
pub struct WarmHarness {
    /// `BWOC_WARM=1` set?
    enabled: bool,
    /// The agent directory (`--workdir` for the harness; `current_dir` for CLI).
    agent_dir: PathBuf,
    /// Workspace root (for `bwoc task complete --workspace`). `None` → can't
    /// mark tasks complete, so warm execution stays inert.
    workspace_root: Option<PathBuf>,
    self_id: String,
    harness: Option<PathBuf>,
    bwoc: Option<PathBuf>,
    model: Option<String>,
    endpoint: Option<String>,
    backend: Option<String>,
    /// True when the backend is an **ambient** tier (e.g. `cli`): tools escape the
    /// harness, so the headless auto-approve posture can't be confined. Warm
    /// execution is refused (fail-closed), same as autoprocess.
    ambient_backend: bool,
    /// Idle timeout before the resident child is reaped.
    idle_timeout: Duration,
    /// The live resident process, if currently spawned.
    resident: Option<Resident>,
    /// When the resident last did work — drives idle reaping.
    last_activity: Instant,
}

impl WarmHarness {
    /// Build from the agent directory + workspace root. Active only when
    /// `BWOC_WARM=1`, both `bwoc-harness` and `bwoc` resolve, the manifest has an
    /// `agentId`, and the backend is **not** ambient.
    pub fn detect(cwd: &Path, workspace_root: Option<&Path>) -> Self {
        let enabled = std::env::var_os("BWOC_WARM").is_some();
        let manifest = Manifest::load_from_path(&cwd.join("config.manifest.json")).ok();
        let backend = manifest.as_ref().and_then(|m| m.backend.clone());
        // Absent backend → default HTTP path (Confined); only an explicit ambient
        // backend (`cli`) trips the refusal.
        let ambient_backend = backend
            .as_deref()
            .map(|b| bwoc_core::trust::backend_trust_tier(b).is_ambient())
            .unwrap_or(false);
        let (harness, bwoc) = if enabled {
            (
                bwoc_core::exec::sibling_binary("bwoc-harness"),
                bwoc_core::exec::sibling_binary("bwoc"),
            )
        } else {
            (None, None)
        };
        Self {
            enabled,
            agent_dir: cwd.to_path_buf(),
            workspace_root: workspace_root.map(|r| r.to_path_buf()),
            self_id: manifest
                .as_ref()
                .map(|m| m.agent_id.clone())
                .unwrap_or_default(),
            harness,
            bwoc,
            model: manifest.as_ref().map(|m| m.primary_model.clone()),
            endpoint: manifest.as_ref().and_then(|m| m.base_url.clone()),
            backend,
            ambient_backend,
            idle_timeout: DEFAULT_IDLE,
            resident: None,
            last_activity: Instant::now(),
        }
    }

    /// True when warm execution is engaged (opt-in on, binaries resolved, agentId
    /// present, workspace known, and the backend is confinable).
    pub fn is_active(&self) -> bool {
        self.enabled
            && self.harness.is_some()
            && self.bwoc.is_some()
            && !self.self_id.is_empty()
            && self.workspace_root.is_some()
            && !self.ambient_backend
    }

    /// Log the warm posture once at startup.
    pub fn announce(&self) {
        if !self.enabled {
            return;
        }
        if self.is_active() {
            eprintln!(
                "bwoc-agent --serve: warm mode ON — claimed tasks run in a resident \
                 `bwoc-harness --headless` (trusted, idle-reaped after {}s)",
                self.idle_timeout.as_secs()
            );
        } else if self.ambient_backend {
            eprintln!(
                "bwoc-agent --serve: ⚠ warm mode REFUSED — backend `{}` is AMBIENT \
                 (the vendor CLI runs tools outside the harness; the headless \
                 auto-approve posture cannot be confined). Use an HTTP/harness backend.",
                self.backend.as_deref().unwrap_or("cli")
            );
        } else {
            eprintln!(
                "bwoc-agent --serve: warm mode requested (BWOC_WARM=1) but inactive — \
                 `bwoc-harness`/`bwoc` not resolved, no agentId, or no workspace root."
            );
        }
    }

    /// Execute a claimed task in the resident harness, then mark it complete.
    /// Returns `true` when warm mode handled the task (so the caller skips the
    /// tmux wake), `false` when it declined (inactive, plan-gated, or a spawn/turn
    /// failure) so the caller can fall back. Best-effort: never panics, never
    /// crashes the daemon.
    pub fn process_task(&mut self, team: &str, task: &str, title: &str) -> bool {
        if !self.is_active() {
            return false;
        }
        // Pavāraṇā: a plan-gated task needs lead approval before completion — the
        // daemon can't approve, so don't auto-execute it. Fall back to the wake.
        if self.task_requires_plan(team, task) {
            eprintln!(
                "bwoc-agent --serve: warm skip {team}/{task} — requires_plan (needs lead approval)"
            );
            return false;
        }

        let reply = match self.run_task_turn(title) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("bwoc-agent --serve: warm turn failed for {team}/{task}: {e}");
                // Drop a possibly-wedged child so the next task respawns fresh.
                self.reap();
                return false;
            }
        };
        self.mark_complete(team, task, &reply);
        true
    }

    /// Reap the resident child if it has been idle longer than the timeout.
    /// Cheap to call every serve-loop tick. A no-op when there is no child.
    pub fn tick_idle(&mut self) {
        if self.resident.is_some() && self.last_activity.elapsed() >= self.idle_timeout {
            eprintln!("bwoc-agent --serve: warm harness idle — reaping resident process");
            self.reap();
        }
    }

    /// Quit + reap the resident child on daemon shutdown.
    pub fn shutdown(&mut self) {
        self.reap();
    }

    // ── internals ────────────────────────────────────────────────────────────

    /// Read the task's `requires_plan` flag from the team's `tasks.jsonl`.
    /// Missing/unreadable/unknown → `false` (don't block on a read error).
    fn task_requires_plan(&self, team: &str, task_id: &str) -> bool {
        let Some(root) = &self.workspace_root else {
            return false;
        };
        let path = root.join(".bwoc/teams").join(team).join("tasks.jsonl");
        let Ok(body) = std::fs::read_to_string(&path) else {
            return false;
        };
        team::parse_tasks(&body)
            .ok()
            .and_then(|tasks| {
                tasks
                    .into_iter()
                    .find(|t| t.id == task_id)
                    .map(|t| t.requires_plan)
            })
            .unwrap_or(false)
    }

    /// Ensure a live resident harness, then drive one trusted turn on `title`,
    /// returning the assistant's final reply text.
    fn run_task_turn(&mut self, title: &str) -> Result<String, String> {
        self.ensure_child()?;
        let prompt = format!(
            "You have claimed this Saṅgha task. Complete it, then briefly summarize \
             what you did.\n\nTask: {title}"
        );
        // A claimed team task is trusted operator/teammate-originated work →
        // LocalOperator, so effectful tools are permitted (the agent must DO it).
        let user = ChatInput::User {
            text: prompt,
            principal: Principal::LocalOperator,
        };
        let line = user.to_line().map_err(|e| e.to_string())?;

        let resident = self.resident.as_mut().ok_or("no resident harness")?;
        writeln!(resident.stdin, "{line}").map_err(|e| format!("write task input: {e}"))?;

        let mut reply = String::new();
        let mut acc = String::new();
        let mut buf = String::new();
        let mut lines = 0usize;
        loop {
            buf.clear();
            let n = resident
                .stdout
                .read_line(&mut buf)
                .map_err(|e| format!("read harness: {e}"))?;
            if n == 0 {
                return Err("harness closed stdout (process exited)".into());
            }
            lines += 1;
            if lines > MAX_EVENT_LINES {
                return Err("harness output exceeded line cap".into());
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<ChatEvent>(trimmed) {
                Ok(ChatEvent::Token { text }) => acc.push_str(&text),
                Ok(ChatEvent::Message { text }) => {
                    reply = text;
                    break;
                }
                Ok(ChatEvent::TurnEnd { .. }) => break,
                Ok(ChatEvent::Error { message }) => return Err(format!("harness turn: {message}")),
                // Ready/Restored (startup), tool_call/tool_result, and — defensively
                // — any permission_request (headless shouldn't emit one) are not
                // turn terminators: skip and keep reading.
                _ => {}
            }
        }
        self.last_activity = Instant::now();
        Ok(if reply.is_empty() { acc } else { reply })
    }

    /// Spawn the resident `bwoc-harness --headless` if absent or dead. Writes
    /// `.bwoc/harness.pid` so an operator can see/kill the warm process.
    fn ensure_child(&mut self) -> Result<(), String> {
        // Respawn if a prior child has exited.
        if let Some(r) = &mut self.resident {
            match r.child.try_wait() {
                Ok(Some(_)) => self.resident = None, // exited → respawn below
                Ok(None) => return Ok(()),           // still alive
                Err(_) => self.resident = None,      // unknowable → respawn
            }
        }
        let harness = self.harness.as_ref().ok_or("bwoc-harness not resolved")?;
        let mut cmd = Command::new(harness);
        cmd.arg("--headless").arg("--workdir").arg(&self.agent_dir);
        if let Some(m) = &self.model {
            cmd.arg("--model").arg(m);
        }
        if let Some(e) = &self.endpoint {
            cmd.arg("--endpoint").arg(e);
        }
        if let Some(b) = &self.backend {
            cmd.arg("--backend").arg(b);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn harness: {e}"))?;
        let stdin = child.stdin.take().ok_or("no harness stdin")?;
        let stdout = child.stdout.take().ok_or("no harness stdout")?;
        self.write_pid(child.id());
        self.resident = Some(Resident {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        });
        self.last_activity = Instant::now();
        Ok(())
    }

    /// Mark the task complete via the locked `bwoc task complete` CLI (no
    /// duplicated state logic). On a plan-gate or claim mismatch the CLI refuses
    /// and we log — the work ran but the state stays as-is for a human to resolve.
    fn mark_complete(&self, team: &str, task: &str, reply: &str) {
        let (Some(bwoc), Some(root)) = (&self.bwoc, &self.workspace_root) else {
            return;
        };
        let summary: String = reply.trim().chars().take(200).collect();
        match Command::new(bwoc)
            .args([
                "task",
                "complete",
                team,
                task,
                "--as",
                &self.self_id,
                "--workspace",
                &root.to_string_lossy(),
            ])
            .current_dir(&self.agent_dir)
            .output()
        {
            Ok(o) if o.status.success() => {
                eprintln!("bwoc-agent --serve: warm completed {team}/{task} — {summary}");
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let first = err.trim().lines().next().unwrap_or("unknown");
                eprintln!("bwoc-agent --serve: warm complete {team}/{task} refused: {first}");
            }
            Err(e) => eprintln!("bwoc-agent --serve: warm complete {team}/{task} exec failed: {e}"),
        }
    }

    /// Best-effort `.bwoc/harness.pid` writer (operator visibility).
    fn write_pid(&self, pid: u32) {
        let path = self.agent_dir.join(".bwoc").join("harness.pid");
        let _ = std::fs::write(path, pid.to_string());
    }

    /// Send `quit`, then reap the child and remove the pid file. Idempotent.
    fn reap(&mut self) {
        if let Some(mut r) = self.resident.take() {
            if let Ok(l) = ChatInput::Quit.to_line() {
                let _ = writeln!(r.stdin, "{l}");
            }
            drop(r.stdin);
            let _ = r.child.wait();
        }
        let _ = std::fs::remove_file(self.agent_dir.join(".bwoc").join("harness.pid"));
    }
}

impl Drop for WarmHarness {
    fn drop(&mut self) {
        self.reap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_manifest(dir: &Path, backend: &str) {
        fs::write(
            dir.join("config.manifest.json"),
            format!(
                r#"{{"name":"x","agentId":"agent-x","agentRole":"r","primaryModel":"m","backend":"{backend}","memoryPath":"memories/","lintCmd":"x","formatCmd":"x","testCmd":"x","buildCmd":"x","version":"1"}}"#
            ),
        )
        .unwrap();
    }

    /// With `BWOC_WARM` unset, warm mode is inert regardless of backend.
    #[test]
    fn inactive_without_env() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "ollama");
        // detect() reads the process env; this test never sets BWOC_WARM.
        let w = WarmHarness::detect(tmp.path(), Some(tmp.path()));
        if std::env::var_os("BWOC_WARM").is_none() {
            assert!(!w.is_active(), "warm must be off without BWOC_WARM");
        }
    }

    /// An ambient backend (`cli`) is flagged so warm execution is refused even if
    /// the env/binaries were present (fail-closed, mirroring autoprocess).
    #[test]
    fn ambient_backend_is_flagged_and_refused() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "cli");
        let w = WarmHarness::detect(tmp.path(), Some(tmp.path()));
        assert!(w.ambient_backend, "cli backend must be flagged ambient");
        assert!(!w.is_active(), "ambient backend must force warm off");
    }

    /// A confined (HTTP/harness) backend is not flagged ambient.
    #[test]
    fn confined_backend_not_flagged() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "ollama");
        let w = WarmHarness::detect(tmp.path(), Some(tmp.path()));
        assert!(!w.ambient_backend, "ollama backend must stay confined");
    }

    /// `process_task` on an inactive warm harness declines (returns false) so the
    /// caller falls back — and never spawns anything.
    #[test]
    fn inactive_process_task_declines() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "cli"); // ambient → inactive
        let mut w = WarmHarness::detect(tmp.path(), Some(tmp.path()));
        assert!(!w.process_task("squad", "t1", "do a thing"));
        assert!(w.resident.is_none(), "declined task must not spawn a child");
    }

    /// `requires_plan` is read from the team file; a plan-gated task is skipped.
    #[test]
    fn plan_gated_task_is_detected() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "ollama");
        let teams = tmp.path().join(".bwoc/teams/squad");
        fs::create_dir_all(&teams).unwrap();
        fs::write(
            teams.join("tasks.jsonl"),
            "{\"id\":\"t1\",\"title\":\"a\",\"state\":\"pending\",\"requires_plan\":true,\"created_at\":\"x\"}\n\
             {\"id\":\"t2\",\"title\":\"b\",\"state\":\"pending\",\"created_at\":\"x\"}\n",
        )
        .unwrap();
        let w = WarmHarness::detect(tmp.path(), Some(tmp.path()));
        assert!(w.task_requires_plan("squad", "t1"), "t1 is plan-gated");
        assert!(!w.task_requires_plan("squad", "t2"), "t2 is not");
        assert!(!w.task_requires_plan("squad", "missing"), "unknown → false");
    }
}
