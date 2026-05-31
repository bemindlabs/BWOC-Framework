//! Saṅgha Phase B — daemon-side task watch (announce-only).
//!
//! While `bwoc-agent --serve` is running, it watches the shared task
//! lists of every team the agent belongs to and announces newly-claimable
//! tasks to stderr — the same shape as the inbox watch. "Claimable" means
//! `pending` with every dependency `completed`, in a team where this agent
//! is a member.
//!
//! Announce-only by design (Mattaññutā): the daemon does NOT auto-claim or
//! wake the agent in Phase B. Auto-claim / tmux-wakeup (mirroring the inbox
//! wakeup) is a deliberate follow-up once announce is proven in the wild.
//!
//! This module does the filesystem walk; the parsing + claimable rule reuse
//! the pure functions in `bwoc_core::team` (which stays IO-free).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bwoc_core::team::{self, TaskState, Team};

/// Watches the agent's team task lists for newly-claimable work.
pub struct TaskWatch {
    /// Workspace root (`.bwoc/teams/` lives under it). `None` → inert.
    workspace_root: Option<PathBuf>,
    /// This agent's id (e.g. `agent-pi`) — only its teams are watched.
    agent_id: String,
    /// (team_id, task_id) of claimable tasks already announced, so a task
    /// is announced once (not re-announced every poll while it stays open).
    seen: HashSet<(String, String)>,
    /// Opt-in (`BWOC_TASK_WAKEUP=1`): on a newly-claimable task, ping the
    /// agent's tmux session so a live Claude session notices and can claim
    /// it — the same best-effort wakeup the inbox uses. Default off:
    /// announce-only.
    wakeup: bool,
    /// Opt-in (`BWOC_AUTO_CLAIM=1`): on a newly-claimable task, the daemon
    /// claims it for this agent (via the locked `bwoc task claim` CLI path)
    /// and then wakes the agent to work it. The riskiest mode — autonomous
    /// mutation — so default off and gated separately from `wakeup`.
    auto_claim: bool,
}

impl TaskWatch {
    /// Build the watcher and snapshot the *currently* claimable tasks into
    /// `seen` WITHOUT announcing them — same posture as the inbox cursor
    /// starting at EOF (don't replay history on startup). Returns an inert
    /// watcher when there's no workspace root.
    pub fn build(agent_id: &str, workspace_root: Option<&Path>) -> Self {
        let mut w = Self {
            workspace_root: workspace_root.map(|r| r.to_path_buf()),
            agent_id: agent_id.to_string(),
            seen: HashSet::new(),
            wakeup: std::env::var_os("BWOC_TASK_WAKEUP").is_some(),
            auto_claim: std::env::var_os("BWOC_AUTO_CLAIM").is_some(),
        };
        // Pre-seed `seen` with what's already open so the first poll only
        // surfaces tasks that appear *after* the daemon started.
        for (team, task, _title) in w.scan_claimable() {
            w.seen.insert((team, task));
        }
        w
    }

    /// True when there's nothing to watch (no workspace) — lets the caller
    /// skip the poll entirely.
    pub fn is_inert(&self) -> bool {
        self.workspace_root.is_none()
    }

    /// Whether the opt-in tmux wakeup is enabled (`BWOC_TASK_WAKEUP=1`).
    /// Surfaced so the daemon can log its posture at startup.
    pub fn wakeup_enabled(&self) -> bool {
        self.wakeup
    }

    /// Whether autonomous auto-claim is enabled (`BWOC_AUTO_CLAIM=1`).
    pub fn auto_claim_enabled(&self) -> bool {
        self.auto_claim
    }

    /// Announce any claimable task not seen before; record it. Also drops
    /// from `seen` any task that is no longer claimable (claimed/completed),
    /// so if it ever returns to pending it announces afresh.
    pub fn poll(&mut self) {
        if self.workspace_root.is_none() {
            return;
        }
        let current: Vec<(String, String, String)> = self.scan_claimable();
        let current_keys: HashSet<(String, String)> = current
            .iter()
            .map(|(t, k, _)| (t.clone(), k.clone()))
            .collect();

        for (team, task, title) in &current {
            let key = (team.clone(), task.clone());
            if self.seen.insert(key) {
                // newly inserted ⇒ not seen before ⇒ announce.
                eprintln!("bwoc-agent: task available ← {team}/{task}: {title}");
                if self.auto_claim {
                    // Claim it for this agent + wake it to work the task.
                    // A lost race (someone claimed first) just logs — the
                    // task leaves the claimable set and drops from `seen`.
                    self.try_auto_claim(team, task, title);
                } else if self.wakeup {
                    wake_session(&self.agent_id, team, task, title);
                }
            }
        }
        // Forget tasks that left the claimable set (claimed by someone, or
        // completed) so a future re-open re-announces.
        self.seen.retain(|k| current_keys.contains(k));
    }

    /// Auto-claim a task via the locked `bwoc task claim` CLI path (no
    /// duplicated lock logic — the CLI handles the race), then wake the
    /// agent to work it. Best-effort: a lost race or any failure logs and
    /// returns. Only called when `BWOC_AUTO_CLAIM=1`.
    fn try_auto_claim(&self, team: &str, task: &str, title: &str) {
        let Some(root) = &self.workspace_root else {
            return;
        };
        let root_str = root.to_string_lossy().to_string();
        // Resolve `bwoc` as a sibling of the running agent, not a stale PATH copy.
        let result = std::process::Command::new(bwoc_core::exec::binary_or_name("bwoc"))
            .args([
                "task",
                "claim",
                team,
                task,
                "--as",
                &self.agent_id,
                "--workspace",
                root_str.as_str(),
            ])
            .output();
        match result {
            Ok(o) if o.status.success() => {
                eprintln!("bwoc-agent: auto-claimed {team}/{task} — {title}");
                // The agent now owns work — tell it regardless of the
                // wakeup flag (auto-claim implies "go do this").
                wake_session(&self.agent_id, team, task, title);
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                let first = err.trim().lines().next().unwrap_or("unknown");
                eprintln!("bwoc-agent: auto-claim {team}/{task} skipped: {first}");
            }
            Err(e) => {
                eprintln!(
                    "bwoc-agent: auto-claim {team}/{task} exec failed: {e} (is bwoc on PATH?)"
                );
            }
        }
    }

    /// Walk `.bwoc/teams/*.toml`, keep teams this agent is a member of, and
    /// collect `(team_id, task_id, title)` for every claimable task. Pure
    /// read; silently skips missing/unreadable/malformed files.
    fn scan_claimable(&self) -> Vec<(String, String, String)> {
        let Some(root) = &self.workspace_root else {
            return Vec::new();
        };
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
            let Ok(t) = Team::from_toml(&body) else {
                continue;
            };
            if !t.has_member(&self.agent_id) {
                continue;
            }
            let tasks = std::fs::read_to_string(dir.join(&t.id).join("tasks.jsonl"))
                .ok()
                .and_then(|b| team::parse_tasks(&b).ok())
                .unwrap_or_default();
            for task in &tasks {
                if task.state != TaskState::Pending {
                    continue;
                }
                let unblocked = task.deps.iter().all(|d| {
                    tasks
                        .iter()
                        .any(|x| &x.id == d && x.state == TaskState::Completed)
                });
                if unblocked {
                    out.push((t.id.clone(), task.id.clone(), task.title.clone()));
                }
            }
        }
        out
    }
}

/// Best-effort tmux wakeup mirroring the inbox's `notify_tmux` (send.rs):
/// recipient `agent-<x>` → tmux session `<x>`. Sends a `[bwoc task …]`
/// marker so a live Claude session running this agent notices the
/// available work and can `bwoc task claim` it. Two-step send (text →
/// 200ms → Enter) for the Claude TUI input quirk. Silent no-op when the
/// agent isn't `agent-*`, `tmux` is missing, or no session matches.
fn wake_session(agent_id: &str, team: &str, task: &str, title: &str) {
    let Some(session) = agent_id.strip_prefix("agent-") else {
        return;
    };
    let has_session = std::process::Command::new("tmux")
        .args(["has-session", "-t", session])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !has_session {
        return;
    }
    let notify = format!("[bwoc task {team}/{task}] {title}");
    let _ = std::process::Command::new("tmux")
        .args(["send-keys", "-t", session, "--", &notify])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = std::process::Command::new("tmux")
        .args(["send-keys", "-t", session, "Enter"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_team(dir: &Path, id: &str, members: &str, tasks: &str) {
        std::fs::create_dir_all(dir.join(id)).unwrap();
        std::fs::write(
            dir.join(format!("{id}.toml")),
            format!("id = \"{id}\"\nmembers = [{members}]\ncreated_at = \"x\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join(id).join("tasks.jsonl"), tasks).unwrap();
    }

    #[test]
    fn announces_only_new_claimable_after_build() {
        let root = std::env::temp_dir().join(format!("bwoc-tw-{}", std::process::id()));
        let teams = root.join(".bwoc/teams");
        std::fs::create_dir_all(&teams).unwrap();
        // squad: pi member; t1 pending (claimable at build → pre-seeded).
        write_team(
            &teams,
            "squad",
            "\"agent-pi\"",
            "{\"id\":\"t1\",\"title\":\"a\",\"state\":\"pending\",\"created_at\":\"x\"}\n",
        );

        let mut w = TaskWatch::build("agent-pi", Some(&root));
        // t1 was open at build → already in `seen`, so a poll announces nothing.
        assert!(w.seen.contains(&("squad".to_string(), "t1".to_string())));

        // Add t2 (new, claimable) → next scan should see it as new.
        std::fs::write(
            teams.join("squad/tasks.jsonl"),
            "{\"id\":\"t1\",\"title\":\"a\",\"state\":\"pending\",\"created_at\":\"x\"}\n\
             {\"id\":\"t2\",\"title\":\"b\",\"state\":\"pending\",\"created_at\":\"x\"}\n",
        )
        .unwrap();
        let before = w.seen.len();
        w.poll();
        assert!(w.seen.contains(&("squad".to_string(), "t2".to_string())));
        assert_eq!(w.seen.len(), before + 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn non_member_team_is_ignored() {
        let root = std::env::temp_dir().join(format!("bwoc-tw-nm-{}", std::process::id()));
        let teams = root.join(".bwoc/teams");
        std::fs::create_dir_all(&teams).unwrap();
        write_team(
            &teams,
            "other",
            "\"agent-oracle\"",
            "{\"id\":\"t1\",\"title\":\"a\",\"state\":\"pending\",\"created_at\":\"x\"}\n",
        );
        let w = TaskWatch::build("agent-pi", Some(&root));
        assert!(w.seen.is_empty(), "pi is not a member of 'other'");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn blocked_task_is_not_claimable() {
        let root = std::env::temp_dir().join(format!("bwoc-tw-blk-{}", std::process::id()));
        let teams = root.join(".bwoc/teams");
        std::fs::create_dir_all(&teams).unwrap();
        // t2 depends on t1 which is still pending → t2 not claimable; t1 is.
        write_team(
            &teams,
            "squad",
            "\"agent-pi\"",
            "{\"id\":\"t1\",\"title\":\"a\",\"state\":\"pending\",\"created_at\":\"x\"}\n\
             {\"id\":\"t2\",\"title\":\"b\",\"state\":\"pending\",\"deps\":[\"t1\"],\"created_at\":\"x\"}\n",
        );
        let w = TaskWatch::build("agent-pi", Some(&root));
        assert!(w.seen.contains(&("squad".to_string(), "t1".to_string())));
        assert!(!w.seen.contains(&("squad".to_string(), "t2".to_string())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn inert_without_workspace() {
        let w = TaskWatch::build("agent-pi", None);
        assert!(w.is_inert());
    }
}
