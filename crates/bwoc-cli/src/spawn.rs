//! `bwoc spawn` — exec the configured LLM backend CLI in an agent's directory.
//!
//! Minimal Phase 1 v2.0 implementation: requires explicit `--path` (workspace
//! discovery and `agents.toml` lookup defer to Phase 2). Validates the path is
//! a BWOC agent (has `AGENTS.md`) before spawning. Propagates the backend's
//! exit code.
//!
//! ## Ollama dispatch
//!
//! All non-Ollama backends exec an external vendor CLI (`claude`, `agy`, …).
//! Ollama has no agentic CLI of its own, so `Backend::Ollama` instead execs
//! the `bwoc-harness` sibling binary.  Resolution order:
//!
//! 1. Same directory as the running `bwoc` binary (`std::env::current_exe()`).
//! 2. `cargo`-built artifact: `CARGO_BIN_EXE_bwoc-harness` env var (test only).
//! 3. `bwoc-harness` on `$PATH`.
//!
//! **Dep-quarantine invariant:** `bwoc-harness` is launched as a subprocess
//! and is never a Cargo dependency of `bwoc-cli`.  `tokio`/`reqwest`/`async-
//! trait`/`hyper` never appear in `bwoc-cli`'s dependency tree.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::i18n;
use crate::sessions::{SessionMarker, remove_marker, write_marker};

/// Which backend CLI to invoke.
///
/// Non-Ollama variants map 1:1 to an external CLI program name on PATH.
/// `Ollama` is special: it execs the `bwoc-harness` sibling binary.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Backend {
    Claude,
    Antigravity,
    Codex,
    Kimi,
    /// Self-hosted Ollama (or any OpenAI-compat endpoint).
    /// Execs the `bwoc-harness` sibling binary instead of an external CLI.
    Ollama,
}

impl Backend {
    /// External CLI program name for vendor backends.
    ///
    /// Returns `None` for `Ollama` — that backend uses `harness_binary()`
    /// instead. Callers that only care about the human-readable name should
    /// use `display_name()`.
    pub fn cli_name(self) -> Option<&'static str> {
        match self {
            Backend::Claude => Some("claude"),
            Backend::Antigravity => Some("agy"),
            Backend::Codex => Some("codex"),
            Backend::Kimi => Some("kimi"),
            Backend::Ollama => None,
        }
    }

    /// Short identifier used in human-readable messages and log lines.
    pub fn display_name(self) -> &'static str {
        match self {
            Backend::Claude => "claude",
            Backend::Antigravity => "agy",
            Backend::Codex => "codex",
            Backend::Kimi => "kimi",
            Backend::Ollama => "ollama",
        }
    }

    /// Curated catalog of common LLM model identifiers per backend, surfaced
    /// in the `bwoc new` interactive picker. First entry is the recommended
    /// default. Free-text input is still accepted for unlisted models — this
    /// is a convenience, not a whitelist. Update as backends release models.
    ///
    /// Antigravity (`agy`) is multi-vendor: it routes Gemini, Claude, and
    /// GPT-OSS model families through one CLI. Model keys are kebab-case
    /// slugs of the picker labels Google surfaces in the `agy` chooser.
    pub fn models(self) -> &'static [&'static str] {
        match self {
            Backend::Claude => &["claude-opus-4-7", "claude-sonnet-4-6", "claude-haiku-4-5"],
            Backend::Antigravity => &[
                "gemini-3.5-flash-medium",
                "gemini-3.5-flash-high",
                "gemini-3.1-pro-low",
                "gemini-3.1-pro-high",
                "claude-sonnet-4.6-thinking",
                "claude-opus-4.6-thinking",
                "gpt-oss-120b-medium",
            ],
            Backend::Codex => &["gpt-5", "gpt-5-mini", "o1"],
            Backend::Kimi => &["kimi-k2", "kimi-k1.5"],
            Backend::Ollama => &[
                "qwen2.5-coder:7b",
                "qwen2.5-coder:14b",
                "llama3.1:8b",
                "mistral-nemo",
                "gemma4:8b",
            ],
        }
    }

    /// Resolve the `bwoc-harness` binary path for the Ollama backend.
    ///
    /// Resolution order:
    /// 1. Sibling of the running `bwoc` binary.
    /// 2. `CARGO_BIN_EXE_bwoc-harness` (set by Cargo during `cargo test`).
    /// 3. `bwoc-harness` on `$PATH`.
    ///
    /// Returns `None` if none of the locations yield an executable.
    pub fn harness_binary() -> Option<PathBuf> {
        // 1. Sibling of the running binary.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let candidate = dir.join("bwoc-harness");
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        // 2. Cargo test env var (set by `cargo test` for workspace binaries).
        if let Ok(p) = std::env::var("CARGO_BIN_EXE_bwoc-harness") {
            let pb = PathBuf::from(&p);
            if pb.is_file() {
                return Some(pb);
            }
        }

        // 3. $PATH fallback.
        let path_env = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join("bwoc-harness");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

pub struct SpawnArgs {
    pub path: Option<PathBuf>,
    pub backend: Backend,
    pub extra: Vec<OsString>,
    pub lang: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("agent path does not exist: {0}")]
    PathMissing(PathBuf),
    #[error("not a BWOC agent: {0} (no AGENTS.md found)")]
    NotAnAgent(PathBuf),
    #[error("backend CLI '{backend}' not found on PATH; install it or pick another --backend")]
    BackendNotFound { backend: &'static str },
    #[error(
        "bwoc-harness binary not found; install it (`cargo install --path crates/bwoc-harness`) \
         or add it to PATH"
    )]
    HarnessNotFound,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// Entry point — returns the process exit code.
pub fn run(args: SpawnArgs) -> i32 {
    match spawn(args) {
        Ok(code) => code,
        Err(
            e @ (SpawnError::PathMissing(_)
            | SpawnError::NotAnAgent(_)
            | SpawnError::BackendNotFound { .. }
            | SpawnError::HarnessNotFound),
        ) => {
            eprintln!("bwoc spawn: {e}");
            2
        }
        Err(e) => {
            eprintln!("bwoc spawn: {e}");
            1
        }
    }
}

pub fn spawn(args: SpawnArgs) -> Result<i32, SpawnError> {
    let bundle = i18n::bundle_for(&args.lang);
    let path = args
        .path
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    validate_agent_path(&path)?;

    let backend_name = args.backend.display_name();
    let path_display = path.display().to_string();
    eprintln!(
        "{}",
        i18n::t_with(
            &bundle,
            "spawn-exec-status",
            &[("backend", backend_name), ("path", &path_display)],
        )
    );

    // Detect the agent id from the agent directory (basename convention
    // is "agent-<name>"; fall back to the dir name verbatim).
    let agent_id = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());

    // Resolve the workspace root for the session marker (walk up from agent dir).
    // Best-effort — if we can't find it we skip the marker write silently.
    let workspace_root = find_workspace_root(&path);

    // ── Spawn the backend ────────────────────────────────────────────────────
    // Ollama → exec bwoc-harness; all other backends → exec their external CLI.
    let mut cmd = if args.backend == Backend::Ollama {
        let harness = Backend::harness_binary().ok_or(SpawnError::HarnessNotFound)?;
        let mut c = Command::new(&harness);
        c.current_dir(&path).args(&args.extra);
        c
    } else {
        let cli = args
            .backend
            .cli_name()
            .expect("non-Ollama backend always has a cli_name");
        let mut c = Command::new(cli);
        c.current_dir(&path).args(&args.extra);
        c
    };

    let mut child = cmd.spawn().map_err(|e| {
        if args.backend != Backend::Ollama {
            let cli = args
                .backend
                .cli_name()
                .expect("non-Ollama backend always has a cli_name");
            if e.kind() == io::ErrorKind::NotFound {
                return SpawnError::BackendNotFound { backend: cli };
            }
        } else if e.kind() == io::ErrorKind::NotFound {
            return SpawnError::HarnessNotFound;
        }
        SpawnError::Io(e)
    })?;

    let pid = child.id();

    // ── Write session marker (best-effort) ──────────────────────────────────
    if let Some(ref ws) = workspace_root {
        let started_at = iso8601_now();
        let tmux = detect_tmux_pane();
        let marker = SessionMarker {
            agent_id: agent_id.clone(),
            backend: backend_name.to_string(),
            pid,
            started_at,
            tmux,
        };
        write_marker(ws, &marker);
    }

    // ── Wait for the backend to exit ─────────────────────────────────────────
    let status = child.wait().map_err(SpawnError::Io)?;

    // ── Remove marker on clean exit (best-effort) ───────────────────────────
    if let Some(ref ws) = workspace_root {
        remove_marker(ws, &agent_id);
    }

    Ok(status.code().unwrap_or(1))
}

/// Walk up from `start` to find the nearest `.bwoc/workspace.toml`.
fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Best-effort ISO-8601 UTC timestamp using only std — no chrono/time crate.
/// Format: `YYYY-MM-DDTHH:MM:SSZ` (second precision is sufficient for markers).
fn iso8601_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Compute year/month/day/hour/min/sec from Unix epoch seconds.
    // Algorithm: days-since-epoch → Gregorian date (civil_from_days, Neri-Schneider).
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hh = time_of_day / 3600;
    let mm = (time_of_day % 3600) / 60;
    let ss = time_of_day % 60;

    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    // Shift epoch to 0001-03-01 for Gregorian cycle math.
    // Using the Euclidean algorithm for civil_from_days (public domain).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

/// Detect the current tmux pane/window string if running inside tmux.
/// Returns `None` when `$TMUX` is not set. Format: `<session>:<window>.<pane>`.
fn detect_tmux_pane() -> Option<String> {
    std::env::var("TMUX").ok()?; // Only probe when inside tmux.
    let out = std::process::Command::new("tmux")
        .args(["display-message", "-p", "#S:#I.#P"])
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() { None } else { Some(s) }
    } else {
        None
    }
}

fn validate_agent_path(path: &Path) -> Result<(), SpawnError> {
    if !path.is_dir() {
        return Err(SpawnError::PathMissing(path.to_path_buf()));
    }
    let agents_md = path.join("AGENTS.md");
    if !agents_md.exists() {
        return Err(SpawnError::NotAnAgent(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_cli_names() {
        assert_eq!(Backend::Claude.cli_name(), Some("claude"));
        assert_eq!(Backend::Antigravity.cli_name(), Some("agy"));
        assert_eq!(Backend::Codex.cli_name(), Some("codex"));
        assert_eq!(Backend::Kimi.cli_name(), Some("kimi"));
        // Ollama has no external CLI — uses bwoc-harness instead.
        assert_eq!(Backend::Ollama.cli_name(), None);
    }

    #[test]
    fn backend_display_names() {
        assert_eq!(Backend::Claude.display_name(), "claude");
        assert_eq!(Backend::Antigravity.display_name(), "agy");
        assert_eq!(Backend::Codex.display_name(), "codex");
        assert_eq!(Backend::Kimi.display_name(), "kimi");
        assert_eq!(Backend::Ollama.display_name(), "ollama");
    }

    #[test]
    fn ollama_has_models() {
        assert!(!Backend::Ollama.models().is_empty());
    }

    #[test]
    fn validate_rejects_missing_path() {
        assert!(matches!(
            validate_agent_path(Path::new("/nonexistent/path/xyz123")),
            Err(SpawnError::PathMissing(_))
        ));
    }

    #[test]
    fn validate_rejects_non_agent_dir() {
        // Use the platform's actual temp dir — exists on every OS, and
        // is extremely unlikely to contain AGENTS.md. (Hardcoding "/tmp"
        // broke on Windows where it resolves to the current drive's \tmp.)
        let tmp = std::env::temp_dir();
        if !tmp.join("AGENTS.md").exists() {
            assert!(matches!(
                validate_agent_path(&tmp),
                Err(SpawnError::NotAnAgent(_))
            ));
        }
    }

    #[test]
    fn validate_accepts_agent_template() {
        let template =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/agent-template");
        assert!(validate_agent_path(&template).is_ok());
    }
}
