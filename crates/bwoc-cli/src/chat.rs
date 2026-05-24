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
//!   - `--tmux`: open `tmux new-window` running spawn; current shell
//!     stays put. Requires `$TMUX` (caller is inside a tmux session).
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
                 edit .bwoc/agents.toml to one of: claude, agy, codex, kimi, ollama",
                entry.id, entry.backend
            );
            return 1;
        }
    };
    let agent_path = workspace.join(&entry.path);

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
    if std::env::var_os("TMUX").is_none() {
        eprintln!(
            "bwoc chat --tmux: not inside a tmux session. Run `tmux new-session` first, \
             then re-run, or drop --tmux to exec in this shell."
        );
        return 2;
    }
    let path_str = agent_path.to_string_lossy().to_string();
    match std::process::Command::new("tmux")
        .args([
            "new-window",
            "-n",
            agent_id,
            "--",
            "bwoc",
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
                "Opened tmux window '{agent_id}' (backend: {})",
                backend.display_name()
            );
            0
        }
        Ok(s) => {
            eprintln!("bwoc chat --tmux: tmux new-window exited {s}");
            1
        }
        Err(e) => {
            eprintln!("bwoc chat --tmux: tmux exec failed: {e}");
            1
        }
    }
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
            "bwoc",
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
        "ollama" => Some(Backend::Ollama),
        "openai-compatible" => Some(Backend::OpenAiCompatible),
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
