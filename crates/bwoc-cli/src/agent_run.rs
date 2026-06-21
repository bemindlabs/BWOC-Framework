//! `bwoc agent run --as-user <user>` — launch an agent session as an
//! unprivileged user (#322, the helper half).
//!
//! This automates the documented root-only-VPS pattern (see
//! `docs/en/DEPLOYMENT.en.md`): from `root`, drop to a dedicated unprivileged
//! user and launch the agent **in its own directory** (so the backend reads
//! `config.manifest.json` / `AGENTS.md` from CWD). Without an explicit `--`
//! command it runs `bwoc-agent --serve`; with one, it runs that instead (e.g.
//! `claude --remote-control … --dangerously-skip-permissions`).
//!
//! Deliberately conservative (it manages privilege, so blast radius matters):
//! - **Unix-only.** Privilege-drop is a POSIX concept; other platforms error.
//! - **Must be run as `root`.** Dropping privilege only makes sense downward.
//! - **Does NOT create the user or `chown` anything.** The docs own those
//!   one-time steps; a launcher silently rewriting ownership would be a footgun.
//!   It *warns* (not fails) when the agent dir isn't owned by the target user.
//! - Drops privilege via `runuser` (util-linux; non-interactive from root,
//!   preserves CWD) — never a hand-rolled setuid.

use std::path::PathBuf;

pub struct AgentRunArgs {
    /// Agent id or bare name (e.g. `pi` or `agent-pi`).
    pub agent: String,
    /// Unprivileged user to drop to.
    pub as_user: String,
    pub workspace: Option<PathBuf>,
    /// Command after `--`; empty → the default `bwoc-agent --serve`.
    pub command: Vec<String>,
}

pub fn run(args: AgentRunArgs) -> i32 {
    #[cfg(not(unix))]
    {
        let _ = args;
        eprintln!("bwoc agent run --as-user is only supported on Unix (it drops POSIX privilege).");
        2
    }
    #[cfg(unix)]
    {
        unix::run(args)
    }
}

/// Assemble the `runuser` argv that drops to `user` and runs `cmd`.
/// `runuser -u <user> -- <cmd…>` keeps the current working directory (set by the
/// caller via `Command::current_dir`) and does not prompt from root.
fn runuser_argv(user: &str, cmd: &[String]) -> Vec<String> {
    let mut v = vec!["-u".to_string(), user.to_string(), "--".to_string()];
    v.extend(cmd.iter().cloned());
    v
}

/// The command to launch: the explicit `-- <cmd…>` if given, else the default
/// `bwoc-agent --serve` (resolved binary path or the bare name).
fn launch_command(explicit: &[String], bwoc_agent: &str) -> Vec<String> {
    if explicit.is_empty() {
        vec![bwoc_agent.to_string(), "--serve".to_string()]
    } else {
        explicit.to_vec()
    }
}

fn normalize_agent(a: &str) -> String {
    if a.starts_with("agent-") {
        a.to_string()
    } else {
        format!("agent-{a}")
    }
}

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;
    use std::process::Command;

    use bwoc_core::workspace::AgentsRegistry;

    use super::{AgentRunArgs, launch_command, normalize_agent, runuser_argv};

    pub fn run(args: AgentRunArgs) -> i32 {
        // 1. Must be root to drop privilege.
        match current_uid() {
            Some(0) => {}
            Some(_) => {
                eprintln!(
                    "bwoc agent run --as-user: must be run as root to drop to '{}' \
                     (you are not root).",
                    args.as_user
                );
                return 2;
            }
            None => {
                eprintln!(
                    "bwoc agent run --as-user: could not determine the current uid (`id -u`)."
                );
                return 2;
            }
        }

        // 2. Target user must exist.
        let Some(target_uid) = user_uid(&args.as_user) else {
            eprintln!(
                "bwoc agent run --as-user: user '{}' does not exist — create it first \
                 (see docs/en/DEPLOYMENT.en.md).",
                args.as_user
            );
            return 2;
        };

        // 3. Resolve workspace + the agent's own directory.
        let Some(workspace) = resolve_workspace(args.workspace.clone()) else {
            eprintln!(
                "bwoc agent run --as-user: no workspace (pass --workspace, set BWOC_WORKSPACE, \
                 or run inside one)."
            );
            return 2;
        };
        let registry = match AgentsRegistry::load(&workspace) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("bwoc agent run --as-user: cannot load agent registry: {e}");
                return 1;
            }
        };
        let want = normalize_agent(&args.agent);
        let Some(entry) = registry.agents.iter().find(|a| a.id == want) else {
            eprintln!(
                "bwoc agent run --as-user: agent '{}' not in the workspace registry.",
                args.agent
            );
            return 2;
        };
        let agent_dir = entry.dir(&workspace);
        if !agent_dir.is_dir() {
            eprintln!(
                "bwoc agent run --as-user: agent directory missing: {}",
                agent_dir.display()
            );
            return 2;
        }

        // 4. Ownership preflight — WARN (not fail): a process running as the
        // target user can't write a dir it doesn't own.
        if let Ok(meta) = std::fs::metadata(&agent_dir) {
            if meta.uid() != target_uid {
                eprintln!(
                    "bwoc agent run --as-user: warning — {} is not owned by '{}' (uid {} != {}); \
                     the session may fail to write. `chown -R {}: {}` first \
                     (see DEPLOYMENT docs).",
                    agent_dir.display(),
                    args.as_user,
                    meta.uid(),
                    target_uid,
                    args.as_user,
                    agent_dir.display()
                );
            }
        }

        // 5. `runuser` must be available (the privilege-drop mechanism).
        if which("runuser").is_none() {
            eprintln!(
                "bwoc agent run --as-user: `runuser` not found (util-linux). Install it, or use \
                 the systemd `User=` unit from docs/en/DEPLOYMENT.en.md."
            );
            return 2;
        }

        // 6. Build + launch: runuser drops to the user; current_dir is the agent
        // dir so the backend reads its manifest/persona from CWD.
        let bwoc_agent = bwoc_core::exec::sibling_binary("bwoc-agent")
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "bwoc-agent".to_string());
        let cmd = launch_command(&args.command, &bwoc_agent);
        let argv = runuser_argv(&args.as_user, &cmd);

        eprintln!(
            "bwoc agent run: dropping to '{}' in {} → {}",
            args.as_user,
            agent_dir.display(),
            cmd.join(" ")
        );
        match Command::new("runuser")
            .args(&argv)
            .current_dir(&agent_dir)
            .status()
        {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("bwoc agent run --as-user: failed to launch runuser: {e}");
                1
            }
        }
    }

    /// Current effective uid via `id -u` (avoids a libc dep). `None` on failure.
    fn current_uid() -> Option<u32> {
        let out = Command::new("id").arg("-u").output().ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    /// Resolve a username to its uid via `id -u <user>`; `None` if it doesn't exist.
    fn user_uid(user: &str) -> Option<u32> {
        let out = Command::new("id").args(["-u", user]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    /// First PATH entry containing an executable `cmd` (no `which` crate).
    fn which(cmd: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(cmd);
            if cand.is_file() {
                return Some(cand);
            }
        }
        None
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_command_defaults_to_serve() {
        assert_eq!(
            launch_command(&[], "/opt/bin/bwoc-agent"),
            vec!["/opt/bin/bwoc-agent".to_string(), "--serve".to_string()]
        );
    }

    #[test]
    fn launch_command_uses_explicit() {
        let explicit = vec!["claude".to_string(), "--remote-control".to_string()];
        assert_eq!(launch_command(&explicit, "bwoc-agent"), explicit);
    }

    #[test]
    fn runuser_argv_shape() {
        let cmd = vec!["bwoc-agent".to_string(), "--serve".to_string()];
        assert_eq!(
            runuser_argv("bwoc", &cmd),
            vec!["-u", "bwoc", "--", "bwoc-agent", "--serve"]
        );
    }

    #[test]
    fn normalize_agent_prefixes_bare_name() {
        assert_eq!(normalize_agent("pi"), "agent-pi");
        assert_eq!(normalize_agent("agent-pi"), "agent-pi");
    }
}
