//! Fleet discovery + a live `bwoc-harness --chat` session.
//!
//! A [`Session`] spawns the harness in `--chat` mode and bridges its line
//! protocol ([`bwoc_core::chat_proto`]) to the TUI: a reader thread parses each
//! stdout line into a [`ChatEvent`] and forwards it over an `mpsc` channel;
//! [`Session::send`] writes a [`ChatInput`] to the child's stdin.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use serde::Deserialize;

/// One agent as reported by `bwoc list --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub backend: String,
    /// Workspace-relative agent directory (e.g. `agents/agent-anna`), used to
    /// locate the agent's `config.manifest.json` for per-agent model/endpoint
    /// resolution. `default` tolerates an older `bwoc` that omits the field.
    #[serde(default)]
    pub path: String,
    pub running: bool,
    #[serde(rename = "inbox_count", default)]
    pub inbox_count: u32,
}

/// Whether the fleet can drive `backend` through `bwoc-harness --chat`. Mirrors
/// `bwoc_cli::spawn::Backend::uses_harness` (the TUI must not depend on
/// `bwoc-cli`): the OpenAI-compatible family speaks `chat_proto`, while vendor
/// CLIs (claude / agy / codex / kimi / copilot) speak their own interactive
/// protocol and must be opened with `bwoc chat` directly.
pub fn is_harness_drivable(backend: &str) -> bool {
    matches!(
        backend,
        "ollama" | "openai-compatible" | "openrouter" | "litellm"
    )
}

/// Whether `path` is safe to join under the workspace root: non-empty and every
/// component a plain name — no root (`/`), drive prefix (`C:\`), or `..` that
/// could escape the workspace. Guards the untrusted `path` from
/// `bwoc list --json` before it selects which `config.manifest.json` to read.
fn is_safe_relative_path(path: &str) -> bool {
    use std::path::Component;
    let mut has_name = false;
    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) => has_name = true,
            Component::CurDir => {}
            // RootDir / Prefix (absolute) or ParentDir (`..`) — reject.
            _ => return false,
        }
    }
    has_name
}

#[derive(Debug, Deserialize)]
struct FleetSnapshot {
    agents: Vec<AgentInfo>,
    #[allow(dead_code)]
    workspace: String,
}

/// Run `<bwoc> list --json` and parse the fleet. Returns an empty list on any
/// failure; the caller decides what an empty fleet means (`run_fleet` treats it
/// as a user error and exits rather than opening an empty sidebar).
pub fn fetch_fleet(bwoc_bin: &str, workdir: &str) -> Vec<AgentInfo> {
    let out = Command::new(bwoc_bin)
        .args(["list", "--json", "--workspace", workdir])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    serde_json::from_slice::<FleetSnapshot>(&out.stdout)
        .map(|s| s.agents)
        .unwrap_or_default()
}

/// Where the harness binary lives + how to reach the model — passed straight
/// through to `bwoc-harness --chat`.
///
/// The `backend` / `model` / `endpoint` here are the **fleet defaults** (seeded
/// from the launching agent). [`SessionConfig::for_agent`] derives a per-agent
/// config that overrides them from each agent's own `config.manifest.json`, so a
/// mixed fleet drives every pane with its author's declared backend and model.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub harness_bin: String,
    pub workdir: String,
    pub backend: String,
    pub model: String,
    pub endpoint: String,
}

impl SessionConfig {
    /// A per-agent config: the agent's own `backend`, plus `model` and
    /// `endpoint` read from its `<workdir>/<path>/config.manifest.json`. Any
    /// field the manifest omits (or a missing/unreadable manifest) falls back to
    /// this config's default value — never a broken empty string.
    pub fn for_agent(&self, agent: &AgentInfo) -> SessionConfig {
        // `agent.path` is untrusted `bwoc list --json` output. `Path::join`
        // treats an absolute path as a new base (dropping `workdir`) and honours
        // `..`, so a rooted or traversing path could read a manifest *outside*
        // the workspace and silently override this session's endpoint/model.
        // Only load when the path stays inside the workspace; otherwise fall
        // back to the fleet defaults (the backend still comes from the registry).
        let manifest = if is_safe_relative_path(&agent.path) {
            let path = Path::new(&self.workdir)
                .join(&agent.path)
                .join("config.manifest.json");
            bwoc_core::manifest::Manifest::load_from_path(&path).ok()
        } else {
            None
        };
        let model = manifest.as_ref().map(|m| m.primary_model.clone());
        let endpoint = manifest.as_ref().and_then(|m| m.base_url.clone());
        self.with_overrides(&agent.backend, model, endpoint)
    }

    /// Pure merge (no I/O): the agent's `backend` plus optional manifest `model`
    /// / `endpoint` layered over this config's defaults. An empty or `None`
    /// override keeps the default; a concrete value (including `"auto"`, which
    /// the harness resolves itself) is passed through.
    fn with_overrides(
        &self,
        backend: &str,
        model: Option<String>,
        endpoint: Option<String>,
    ) -> SessionConfig {
        let pick = |over: Option<String>, default: &str| {
            over.filter(|s| !s.is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        SessionConfig {
            harness_bin: self.harness_bin.clone(),
            workdir: self.workdir.clone(),
            backend: backend.to_string(),
            model: pick(model, &self.model),
            endpoint: pick(endpoint, &self.endpoint),
        }
    }
}

/// A running chat session bound to one agent.
pub struct Session {
    child: Child,
    stdin: ChildStdin,
    /// Chat events parsed off the child's stdout by the reader thread.
    pub rx: Receiver<ChatEvent>,
}

impl Session {
    /// Spawn `bwoc-harness --chat` for `agent` and start the reader thread.
    pub fn spawn(agent: &str, cfg: &SessionConfig) -> std::io::Result<Self> {
        // An empty model omits `--model` entirely — matching the single-agent
        // path, so the harness falls back to its own default rather than being
        // handed an empty (invalid) model string.
        let mut args: Vec<&str> = vec![
            "--chat",
            "--agent",
            agent,
            "--workdir",
            &cfg.workdir,
            "--backend",
            &cfg.backend,
            "--endpoint",
            &cfg.endpoint,
        ];
        if !cfg.model.is_empty() {
            args.push("--model");
            args.push(&cfg.model);
        }
        let mut child = Command::new(&cfg.harness_bin)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherit stderr like the single-agent TUI: invisible under the alt
            // screen, but still captured by a shell redirect so a crashed
            // session is diagnosable (it would otherwise vanish silently).
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx): (Sender<ChatEvent>, Receiver<ChatEvent>) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<ChatEvent>(line) {
                    Ok(ev) => {
                        let is_bye = matches!(ev, ChatEvent::Bye);
                        if tx.send(ev).is_err() || is_bye {
                            break;
                        }
                    }
                    // Non-JSON chatter (a stray print) — ignore, keep reading.
                    Err(_) => continue,
                }
            }
        });

        Ok(Self { child, stdin, rx })
    }

    /// Whether the child is still running. Used to reap a session whose harness
    /// exited *without* emitting `Bye` (channel disconnect alone can't be told
    /// apart from "no messages yet" via `try_iter`).
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Send one input line to the session. Best-effort — a broken pipe means the
    /// child died; the caller notices via the reader channel closing.
    pub fn send(&mut self, input: &ChatInput) {
        if let Ok(mut line) = input.to_line() {
            line.push('\n');
            let _ = self.stdin.write_all(line.as_bytes());
            let _ = self.stdin.flush();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Ask the session to exit cleanly, then reap so no orphan lingers.
        self.send(&ChatInput::Quit);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> SessionConfig {
        SessionConfig {
            harness_bin: "/bin/bwoc-harness".into(),
            workdir: "/ws".into(),
            backend: "ollama".into(),
            model: "seed-model".into(),
            endpoint: "http://seed/v1".into(),
        }
    }

    #[test]
    fn harness_drivable_matches_uses_harness() {
        for b in ["ollama", "openai-compatible", "openrouter", "litellm"] {
            assert!(is_harness_drivable(b), "{b} should be drivable");
        }
        for b in ["claude", "agy", "codex", "kimi", "copilot", "cli", ""] {
            assert!(!is_harness_drivable(b), "{b} should not be drivable");
        }
    }

    #[test]
    fn overrides_use_agent_values_when_present() {
        let cfg = defaults().with_overrides(
            "openrouter",
            Some("vendor/model-x".into()),
            Some("https://openrouter.ai/api/v1".into()),
        );
        assert_eq!(cfg.backend, "openrouter");
        assert_eq!(cfg.model, "vendor/model-x");
        assert_eq!(cfg.endpoint, "https://openrouter.ai/api/v1");
        // Shared bits are preserved verbatim.
        assert_eq!(cfg.harness_bin, "/bin/bwoc-harness");
        assert_eq!(cfg.workdir, "/ws");
    }

    #[test]
    fn overrides_fall_back_on_missing_or_empty() {
        // None (manifest field absent) and empty string both keep the default.
        let cfg = defaults().with_overrides("openai-compatible", None, Some(String::new()));
        assert_eq!(cfg.backend, "openai-compatible");
        assert_eq!(cfg.model, "seed-model");
        assert_eq!(cfg.endpoint, "http://seed/v1");
    }

    #[test]
    fn safe_relative_path_accepts_workspace_dirs() {
        for p in ["agents/agent-anna", "./agents/x", "agent-pi", "a/b/c"] {
            assert!(is_safe_relative_path(p), "{p} should be accepted");
        }
    }

    #[test]
    fn safe_relative_path_rejects_escape_and_empty() {
        for p in [
            "",
            "/etc/passwd",
            "../secrets",
            "agents/../../etc",
            "..",
            "./..",
        ] {
            assert!(!is_safe_relative_path(p), "{p} should be rejected");
        }
    }

    #[test]
    fn for_agent_ignores_manifest_on_unsafe_path() {
        // An absolute path must not be joined/loaded — endpoint/model stay on the
        // fleet defaults (backend still taken from the untrusted-but-gated field).
        let agent = AgentInfo {
            id: "agent-x".into(),
            backend: "openrouter".into(),
            path: "/etc".into(),
            running: false,
            inbox_count: 0,
        };
        let cfg = defaults().for_agent(&agent);
        assert_eq!(cfg.backend, "openrouter");
        assert_eq!(cfg.model, "seed-model");
        assert_eq!(cfg.endpoint, "http://seed/v1");
    }

    #[test]
    fn override_passes_auto_model_through() {
        // "auto" is a real primaryModel value the harness resolves — not empty,
        // so it must reach the session rather than collapsing to the default.
        let cfg = defaults().with_overrides("ollama", Some("auto".into()), None);
        assert_eq!(cfg.model, "auto");
    }
}
