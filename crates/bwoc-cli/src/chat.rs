//! `bwoc chat <name>` — shortcut for `bwoc spawn` with auto-resolved
//! path and backend from the agent's registry entry.
//!
//! The same launcher behavior the dashboard's `t` hotkey provides, but
//! reachable from the command line so you don't have to launch the TUI
//! first. The agent's `primaryModel` (and `fallbackModel`) come from
//! its `config.manifest.json`, which `bwoc spawn` already reads —
//! "chat mode auto-select llm and model" without any extra prompts.
//!
//! Three modes:
//!   - default: exec the backend CLI in this shell (replaces the
//!     current process via spawn's existing flow)
//!   - `--tmux`: run spawn under tmux. Inside a tmux session it opens a
//!     `tmux new-window` (current shell stays put); outside one it
//!     auto-starts a dedicated session (`tmux new-session -A -s bwoc-<id>`)
//!     and attaches — no "run tmux first" dance.
//!   - `--ghostty`: open a new Ghostty terminal window running spawn;
//!     current shell stays put. macOS-only (Ghostty's CLI entry-point
//!     on macOS is `open -na Ghostty.app`).

use std::path::PathBuf;

use bwoc_core::workspace::AgentsRegistry;

use crate::spawn::{self, Backend};

pub struct ChatArgs {
    pub name: String,
    pub workspace: Option<PathBuf>,
    pub lang: String,
    /// Run inside `tmux new-window` instead of exec'ing in this shell.
    pub tmux: bool,
    /// Open a new Ghostty terminal window. macOS-only.
    pub ghostty: bool,
    /// Full-screen ratatui chat client driving `bwoc-harness --chat`. Harness
    /// backends only; falls back to the default exec path otherwise.
    pub tui: bool,
    /// Join a team's shared chat channel (HV3-3a): teammate replies are
    /// injected into context and this agent's replies broadcast back. Requires
    /// `--tui` with a harness backend (ollama / openai-compatible / openrouter);
    /// ignored otherwise. The agent must be a member of the team.
    pub team: Option<String>,
    /// Open the multi-agent **fleet** TUI (left sidebar of every agent, `Tab`
    /// to switch, one live session per agent) instead of a single-agent chat.
    /// Requires `--tui` with a harness backend; the named agent's backend/model
    /// seed the shared session config for the whole fleet.
    pub fleet: bool,
}

pub fn run(args: ChatArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc chat: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc chat: failed to read agents.toml: {e}");
            return 1;
        }
    };
    let lookup_id = if args.name.starts_with("agent-") {
        args.name.clone()
    } else {
        format!("agent-{}", args.name)
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc chat: no agent named '{}' in workspace {}. Try `bwoc list`.",
            args.name,
            workspace.display()
        );
        return 2;
    };

    let backend = match parse_backend(&entry.backend) {
        Some(b) => b,
        None => {
            eprintln!(
                "bwoc chat: agent '{}' has unknown backend '{}' in registry — \
                 edit .bwoc/agents.toml to one of: claude, agy, codex, kimi, copilot, ollama",
                entry.id, entry.backend
            );
            return 1;
        }
    };
    let agent_path = workspace.join(&entry.path);

    // Team chat broadcast (HV3-3a): resolve + validate the shared log when
    // `--team` is given. Membership is required; the path lives beside the
    // team's task list. Only harness-backed `--tui` sessions can use it (vendor
    // CLIs speak their own protocol), so warn-and-proceed-solo otherwise rather
    // than failing an otherwise-valid chat.
    let team_chat = match &args.team {
        None => None,
        Some(team_id) => match crate::sangha::load_team(&workspace, team_id) {
            Ok(team) => {
                if !team.has_member(&entry.id) {
                    eprintln!(
                        "bwoc chat: agent '{}' is not a member of team '{}' — \
                         add it with `bwoc team` or pick another team.",
                        entry.id, team_id
                    );
                    return 1;
                }
                Some(crate::sangha::team_chat_jsonl_path(&workspace, team_id))
            }
            Err(e) => {
                eprintln!("bwoc chat: {e}");
                return 1;
            }
        },
    };
    if team_chat.is_some() && !(args.tui && backend.uses_harness()) {
        eprintln!(
            "bwoc chat: --team needs --tui with a harness backend \
             (ollama / openai-compatible / openrouter); running this session solo."
        );
    }

    if args.tui && args.fleet {
        // Fleet TUI: every agent in the workspace, one live session each. The
        // named agent's backend/model seed the fleet *defaults*; each pane then
        // overrides them from its own `config.manifest.json` (see
        // `SessionConfig::for_agent`), so a mixed fleet drives every agent with
        // its author's declared backend/model. The launching agent must itself
        // be a harness backend so the seed is harness-shaped.
        if backend.uses_harness() {
            let manifest = bwoc_core::manifest::Manifest::load_from_path(
                &agent_path.join("config.manifest.json"),
            )
            .ok();
            let model = manifest
                .as_ref()
                .map(|m| m.primary_model.clone())
                .unwrap_or_default();
            let endpoint = manifest
                .as_ref()
                .and_then(|m| m.base_url.clone())
                .unwrap_or_else(|| "http://localhost:11434/v1".to_string());
            return bwoc_tui::run_fleet(bwoc_tui::FleetArgs {
                workdir: workspace.clone(),
                backend: backend.display_name().to_string(),
                model,
                endpoint,
            });
        }
        eprintln!(
            "bwoc chat --tui --fleet: agent '{}' uses the '{}' backend, which the TUI can't drive.",
            entry.id,
            backend.display_name()
        );
        return 1;
    }

    if args.tui {
        // The ratatui client only knows how to drive a `bwoc-harness --chat`
        // subprocess (the chat_proto wire format). Vendor backends (claude /
        // agy / codex / kimi) speak their own interactive protocol, so the TUI
        // can't render them — fall through to the default exec path with a hint.
        if backend.uses_harness() {
            return bwoc_tui::run(bwoc_tui::TuiArgs {
                agent_id: entry.id.clone(),
                agent_path,
                backend_name: backend.display_name().to_string(),
                team_chat,
            });
        }
        eprintln!(
            "bwoc chat --tui: agent '{}' uses the '{}' backend, which the TUI can't drive \
             (it only renders the bwoc-harness chat stream for ollama / openai-compatible / openrouter). \
             Launching the backend CLI directly instead.",
            entry.id,
            backend.display_name()
        );
    }

    if args.tmux {
        return open_in_tmux(&entry.id, &agent_path, backend);
    }

    if args.ghostty {
        return open_in_ghostty(&entry.id, &agent_path, backend);
    }

    // Default mode: hand off to spawn::run, which exec's the backend CLI
    // in the agent's directory. Standard error messages from spawn are
    // good enough — no special framing here.
    spawn::run(spawn::SpawnArgs {
        path: Some(agent_path),
        backend,
        extra: Vec::new(),
        lang: args.lang,
    })
}

fn open_in_tmux(agent_id: &str, agent_path: &std::path::Path, backend: Backend) -> i32 {
    // Auto-start tmux when needed: inside a session we add a window; outside
    // one we create+attach a dedicated session instead of refusing with a
    // "run tmux new-session first" hint.
    let inside_tmux = std::env::var_os("TMUX").is_some();
    let path_str = agent_path.to_string_lossy().to_string();
    let args = tmux_launch_args(
        inside_tmux,
        agent_id,
        &path_str,
        backend.display_name(),
        &spawn::bwoc_exe(),
    );

    // The outside-tmux branch attaches and blocks until the user detaches, so a
    // post-`status()` message would only surface after they've left — announce
    // it *before* launching. The inside-tmux branch returns immediately (the
    // window opens in the background), so its confirmation prints after success.
    if !inside_tmux {
        println!(
            "Starting tmux session 'bwoc-{agent_id}' (backend: {})",
            backend.display_name()
        );
    }

    match std::process::Command::new("tmux").args(&args).status() {
        Ok(s) if s.success() => {
            if inside_tmux {
                println!(
                    "Opened tmux window '{agent_id}' (backend: {})",
                    backend.display_name()
                );
            }
            0
        }
        Ok(s) => {
            eprintln!("bwoc chat --tmux: tmux exited {s}");
            1
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // We now invoke tmux even when the caller isn't already in it, so a
            // missing binary is a likelier first encounter — say so plainly.
            eprintln!(
                "bwoc chat --tmux: tmux not found on PATH — install tmux, or drop \
                 --tmux to exec the backend in this shell."
            );
            1
        }
        Err(e) => {
            eprintln!("bwoc chat --tmux: tmux exec failed: {e}");
            1
        }
    }
}

/// Build the `tmux` argument vector (excluding the `tmux` program name) for
/// launching `bwoc spawn` against `agent_id`.
///
/// - **Inside** a tmux session → `new-window` in the current session.
/// - **Outside** one → `new-session -A -s bwoc-<id>` (attach-or-create), so a
///   bare `bwoc chat --tmux` from a plain shell still lands in tmux. `-A`
///   reattaches if a session for this agent already exists.
fn tmux_launch_args(
    inside_tmux: bool,
    agent_id: &str,
    path: &str,
    backend_name: &str,
    bwoc_exe: &str,
) -> Vec<String> {
    let mut args: Vec<String> = if inside_tmux {
        vec!["new-window".into(), "-n".into(), agent_id.into()]
    } else {
        vec![
            "new-session".into(),
            "-A".into(),
            "-s".into(),
            format!("bwoc-{agent_id}"),
            "-n".into(),
            agent_id.into(),
        ]
    };
    args.extend([
        "--".into(),
        bwoc_exe.into(),
        "spawn".into(),
        "--path".into(),
        path.into(),
        "--backend".into(),
        backend_name.into(),
    ]);
    args
}

/// `--ghostty` mode — open a new Ghostty terminal window running
/// `bwoc spawn` for the agent. macOS-only because Ghostty's CLI
/// launcher on macOS is `open -na Ghostty.app` (per Ghostty's own
/// `--help`: "On macOS, launching the terminal emulator from the CLI
/// is not supported"). On other platforms the call falls through
/// with an exit-2 explanation rather than silently failing.
fn open_in_ghostty(agent_id: &str, agent_path: &std::path::Path, backend: Backend) -> i32 {
    if !cfg!(target_os = "macos") {
        eprintln!(
            "bwoc chat --ghostty: macOS-only. Ghostty on Linux/BSD has its own CLI entry — \
             drop --ghostty and run `ghostty -e bwoc spawn --path <p> --backend <b>` manually."
        );
        return 2;
    }
    let path_str = agent_path.to_string_lossy().to_string();
    let wd_arg = format!("--working-directory={path_str}");
    let exe = spawn::bwoc_exe();
    // `open -na Ghostty.app --args --working-directory=<p> -e bwoc spawn --path <p> --backend <b>`
    // -n forces a new window even if Ghostty is already running.
    // --args passes the rest through to Ghostty itself.
    // -e collects all subsequent tokens as the command to run.
    match std::process::Command::new("open")
        .args([
            "-na",
            "Ghostty.app",
            "--args",
            wd_arg.as_str(),
            "-e",
            exe.as_str(),
            "spawn",
            "--path",
            path_str.as_str(),
            "--backend",
            backend.display_name(),
        ])
        .status()
    {
        Ok(s) if s.success() => {
            println!(
                "Opened Ghostty window for '{agent_id}' (backend: {})",
                backend.display_name()
            );
            0
        }
        Ok(s) => {
            eprintln!(
                "bwoc chat --ghostty: `open -na Ghostty.app` exited {s} \
                 (is Ghostty installed in /Applications?)"
            );
            1
        }
        Err(e) => {
            eprintln!("bwoc chat --ghostty: `open` exec failed: {e}");
            1
        }
    }
}

fn parse_backend(s: &str) -> Option<Backend> {
    match s {
        "claude" => Some(Backend::Claude),
        "agy" => Some(Backend::Antigravity),
        "codex" => Some(Backend::Codex),
        "kimi" => Some(Backend::Kimi),
        "copilot" => Some(Backend::Copilot),
        "ollama" => Some(Backend::Ollama),
        "openai-compatible" => Some(Backend::OpenAiCompatible),
        "openrouter" => Some(Backend::OpenRouter),
        "litellm" => Some(Backend::LiteLlm),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_tmux_adds_a_window() {
        let a = tmux_launch_args(true, "agent-pi", "/ws/agent-pi", "claude", "/opt/bin/bwoc");
        assert_eq!(
            a,
            [
                "new-window",
                "-n",
                "agent-pi",
                "--",
                "/opt/bin/bwoc",
                "spawn",
                "--path",
                "/ws/agent-pi",
                "--backend",
                "claude"
            ]
        );
    }

    #[test]
    fn outside_tmux_auto_starts_an_attached_session() {
        let a = tmux_launch_args(false, "agent-pi", "/ws/agent-pi", "ollama", "/opt/bin/bwoc");
        assert_eq!(
            a,
            [
                "new-session",
                "-A",
                "-s",
                "bwoc-agent-pi",
                "-n",
                "agent-pi",
                "--",
                "/opt/bin/bwoc",
                "spawn",
                "--path",
                "/ws/agent-pi",
                "--backend",
                "ollama"
            ]
        );
    }

    /// The launcher must re-invoke the running binary verbatim — including a
    /// dev-build absolute path — never collapse it to a bare `bwoc` PATH lookup.
    #[test]
    fn launch_args_use_the_given_bwoc_exe_verbatim() {
        let a = tmux_launch_args(
            true,
            "agent-pi",
            "/ws/agent-pi",
            "claude",
            "./target/debug/bwoc",
        );
        assert!(a.contains(&"./target/debug/bwoc".to_string()));
        assert!(!a.contains(&"bwoc".to_string()));
    }
}
