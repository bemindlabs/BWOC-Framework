//! Runtime config for bare `bwoc` — a coding session in the current directory.
//!
//! Decides which provider backend, model and endpoint the session uses. No
//! workspace, registered agent or manifest is involved. Precedence, highest
//! first:
//!
//! 1. flags on bare `bwoc`: `--backend`, `--model`, `--endpoint`
//! 2. env: `BWOC_BACKEND`, `BWOC_MODEL`, `BWOC_ENDPOINT`
//! 3. project `.bwoc/config.toml`, in the cwd or an ancestor up to the git root
//! 4. user `~/.bwoc/config.toml`
//! 5. auto-detect: an Anthropic key ⇒ `anthropic`; else a local Ollama with at
//!    least one model ⇒ `ollama`; else a "no provider configured" help screen
//!    that names any vendor CLI found on `PATH`
//!
//! A harness backend (`anthropic`, `ollama`, …) opens the chat TUI on
//! `bwoc-harness`. A vendor-CLI backend (`claude`, `codex`, `agy`, `kimi`,
//! `grok`, `copilot`) hands the terminal to that CLI in the current directory:
//! it runs on the CLI's own login (a subscription needs no API key) with its own
//! tools and permissions, so harness tools and trust gates do not apply (#529).
//!
//! Fields resolve independently, with one guard: a layer that names a
//! *different* backend from the winning one contributes nothing else, so a
//! model written for one provider is never sent to another.
//!
//! Config file shape (both files):
//!
//! ```toml
//! schema_version = 3
//! [runtime]
//! backend    = "ollama"                      # or a vendor CLI: "claude", …
//! model      = "<model>"
//! endpoint   = "http://localhost:11434/v1"   # optional
//! max_tokens = 8192                          # optional
//! max_context = 32768                        # optional: model context window
//! ```
//!
//! `[defaults] backend` (the fleet default for new agents) stands in for an
//! absent `[runtime] backend` in the same file. Absent `schema_version` reads as
//! legacy (`bwoc-core::schema`); a newer one is refused. Unknown keys and tables
//! are ignored.

use std::path::{Path, PathBuf};

use bwoc_core::schema::SchemaVersion;
use serde::Deserialize;

use crate::exit;
use crate::spawn::Backend;

/// Model used when auto-detect picks `anthropic` and nothing names a model.
/// The one place a vendor model id appears in this path; override with
/// `--model`, `BWOC_MODEL` or `[runtime] model`.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";

pub const ENV_BACKEND: &str = "BWOC_BACKEND";
pub const ENV_MODEL: &str = "BWOC_MODEL";
pub const ENV_ENDPOINT: &str = "BWOC_ENDPOINT";

/// One source of runtime settings. Every field optional.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RuntimeLayer {
    pub backend: Option<String>,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub max_tokens: Option<u32>,
    pub max_context: Option<u32>,
}

impl RuntimeLayer {
    /// Blank strings count as unset.
    fn cleaned(self) -> Self {
        let keep = |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        Self {
            backend: keep(self.backend),
            model: keep(self.model),
            endpoint: keep(self.endpoint),
            max_tokens: self.max_tokens,
            max_context: self.max_context,
        }
    }

    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    schema_version: SchemaVersion,
    #[serde(default)]
    runtime: RuntimeTable,
    #[serde(default)]
    defaults: DefaultsTable,
}

/// `[defaults]` — only `backend` is read here, as the fallback for
/// `[runtime] backend`.
#[derive(Debug, Deserialize, Default)]
struct DefaultsTable {
    backend: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RuntimeTable {
    backend: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
    max_tokens: Option<u32>,
    max_context: Option<u32>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("{path}: invalid config: {msg}")]
    Parse { path: String, msg: String },
    #[error(
        "{path}: schema_version = {found} is newer than this bwoc reads (up to {max}); \
         upgrade bwoc or correct schema_version"
    )]
    FutureSchema { path: String, found: u32, max: u32 },
    #[error("{path}: {msg}")]
    Io { path: String, msg: String },
}

impl RuntimeError {
    /// A malformed or too-new config is the user's to fix (usage); an
    /// unreadable file is an environment error.
    pub fn exit_code(&self) -> i32 {
        match self {
            RuntimeError::Parse { .. } | RuntimeError::FutureSchema { .. } => exit::USAGE,
            RuntimeError::Io { .. } => exit::ERROR,
        }
    }
}

/// Parse a `config.toml` into its `[runtime]` layer.
pub fn parse_config(text: &str, path: &Path) -> Result<RuntimeLayer, RuntimeError> {
    let shown = path.display().to_string();
    let file: ConfigFile = toml::from_str(text).map_err(|e| RuntimeError::Parse {
        path: shown.clone(),
        msg: e.message().to_string(),
    })?;
    if file.schema_version.is_future() {
        return Err(RuntimeError::FutureSchema {
            path: shown,
            found: file.schema_version.0,
            max: SchemaVersion::CURRENT.0,
        });
    }
    let r = file.runtime;
    let backend = r
        .backend
        .filter(|b| !b.trim().is_empty())
        .or(file.defaults.backend);
    Ok(RuntimeLayer {
        backend,
        model: r.model,
        endpoint: r.endpoint,
        max_tokens: r.max_tokens,
        max_context: r.max_context,
    }
    .cleaned())
}

/// Load a config file; a missing file is `Ok(None)`.
pub fn load_config(path: &Path) -> Result<Option<RuntimeLayer>, RuntimeError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_config(&text, path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(RuntimeError::Io {
            path: path.display().to_string(),
            msg: e.to_string(),
        }),
    }
}

/// The nearest ancestor of `start` (inclusive) holding a `.git` entry.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// `.bwoc/config.toml` in `cwd` or an ancestor up to the git root. Outside a
/// git repository only `cwd` itself is checked, so a stray file higher up the
/// tree never configures an unrelated directory.
pub fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    let stop = find_git_root(cwd);
    for dir in cwd.ancestors() {
        let candidate = dir.join(".bwoc").join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        match &stop {
            Some(root) if dir == root => break,
            None => break,
            _ => {}
        }
    }
    None
}

pub fn env_layer(getenv: &dyn Fn(&str) -> Option<String>) -> RuntimeLayer {
    RuntimeLayer {
        backend: getenv(ENV_BACKEND),
        model: getenv(ENV_MODEL),
        endpoint: getenv(ENV_ENDPOINT),
        max_tokens: None,
        max_context: None,
    }
    .cleaned()
}

/// Collect the layers in precedence order: flags, env, project, user.
pub fn gather_layers(
    flags: RuntimeLayer,
    getenv: &dyn Fn(&str) -> Option<String>,
    cwd: &Path,
    user_config: Option<&Path>,
) -> Result<Vec<RuntimeLayer>, RuntimeError> {
    let mut layers = vec![flags.cleaned(), env_layer(getenv)];
    let project = find_project_config(cwd);
    if let Some(p) = &project {
        if let Some(layer) = load_config(p)? {
            layers.push(layer);
        }
    }
    if let Some(user) = user_config {
        if project.as_deref() != Some(user) {
            if let Some(layer) = load_config(user)? {
                layers.push(layer);
            }
        }
    }
    Ok(layers)
}

/// Field-wise merge, highest layer first (see the module docs for the guard).
pub fn merge(layers: &[RuntimeLayer]) -> RuntimeLayer {
    let backend = layers.iter().find_map(|l| l.backend.clone());
    let compatible = |l: &&RuntimeLayer| match (&l.backend, &backend) {
        (Some(own), Some(winner)) => own == winner,
        _ => true,
    };
    RuntimeLayer {
        model: layers
            .iter()
            .filter(compatible)
            .find_map(|l| l.model.clone()),
        endpoint: layers
            .iter()
            .filter(compatible)
            .find_map(|l| l.endpoint.clone()),
        max_tokens: layers.iter().filter(compatible).find_map(|l| l.max_tokens),
        max_context: layers.iter().filter(compatible).find_map(|l| l.max_context),
        backend,
    }
}

/// Side-effecting checks the resolver needs, injectable for tests.
pub struct Probes<'a> {
    /// Is an Anthropic API key available (env or `~/.bwoc/secrets.toml`)?
    pub anthropic_key: &'a dyn Fn() -> bool,
    /// Models served by Ollama at the endpoint (`None` = default localhost),
    /// or `None` when it does not answer.
    pub ollama_models: &'a dyn Fn(Option<&str>) -> Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub backend: String,
    pub model: String,
    pub endpoint: Option<String>,
    pub max_tokens: Option<u32>,
    pub max_context: Option<u32>,
}

/// A session handed to a vendor coding CLI instead of `bwoc-harness`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorSession {
    pub backend: Backend,
    /// `None` leaves the CLI on its own default model.
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(Resolved),
    /// A vendor-CLI backend: exec the CLI in the working directory.
    Vendor(VendorSession),
    /// Nothing configured and nothing detected.
    NoProvider,
    /// Configured, but unusable — the message says what to change.
    Invalid(String),
}

/// Answers the in-session `/models`, `/backends`, `/settings` and `/doctor`
/// (see [`bwoc_tui::EnvironmentInfo`]). It holds what the session resolved from
/// so every answer is this session's truth, not a fresh guess.
pub struct Environment {
    /// The backend the session runs on.
    pub backend: String,
    /// The endpoint in use, when one was resolved.
    pub endpoint: Option<String>,
    /// Resolved runtime values with the layer each came from, captured when the
    /// session started (the layers cannot change under a running session).
    pub settings: Vec<(String, String)>,
    /// `~/.bwoc`, when the user has one.
    pub home: Option<PathBuf>,
}

impl bwoc_tui::EnvironmentInfo for Environment {
    fn models(&self) -> Result<Vec<String>, String> {
        // Only a backend with a model index can answer honestly. Ollama has
        // one; a hosted API does not expose one we can trust here.
        if !matches!(self.backend.as_str(), "ollama" | "openai-compatible") {
            return Err(format!(
                "`{}` does not list models — pass one to /model <name>",
                self.backend
            ));
        }
        probe_ollama(self.endpoint.as_deref())
            .ok_or_else(|| "no model list came back from the endpoint".to_string())
    }

    fn backends(&self) -> Vec<(String, String)> {
        let getenv = |k: &str| std::env::var(k).ok();
        let secrets = self
            .home
            .as_ref()
            .map(|h| crate::auth::read_secrets(&h.join("secrets.toml")))
            .unwrap_or(crate::auth::Secrets::Missing);
        let mut rows: Vec<(String, String)> = session_backends()
            .into_iter()
            .map(|name| {
                let note = match crate::auth::key_source(name, &getenv, &secrets) {
                    Some(src) => format!("key from {src}"),
                    None if name == "ollama" => "no key needed".to_string(),
                    None => "no key configured".to_string(),
                };
                let mark = if name == self.backend { " (in use)" } else { "" };
                (name.to_string(), format!("{note}{mark}"))
            })
            .collect();
        let on_path = vendor_clis_on_path();
        for cli in vendor_cli_backends() {
            let note = if on_path.contains(&cli) {
                "vendor CLI on PATH — runs on its own login"
            } else {
                "vendor CLI not installed"
            };
            rows.push((cli.to_string(), note.to_string()));
        }
        rows
    }

    fn settings(&self) -> Vec<(String, String)> {
        self.settings.clone()
    }

    fn doctor(&self) -> Result<Vec<(String, String)>, String> {
        // Run the real `bwoc doctor --json` rather than reimplementing checks:
        // a second implementation would drift from the one operators trust.
        let exe = std::env::current_exe().map_err(|e| format!("cannot find bwoc: {e}"))?;
        let out = std::process::Command::new(exe)
            .args(["doctor", "--json"])
            .output()
            .map_err(|e| format!("could not run bwoc doctor: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let parsed: serde_json::Value =
            serde_json::from_str(text.trim()).map_err(|e| format!("doctor --json: {e}"))?;
        let results = parsed
            .get("results")
            .and_then(|r| r.as_array())
            .ok_or_else(|| "doctor --json had no results".to_string())?;
        Ok(results
            .iter()
            .map(|r| {
                let field = |k: &str| {
                    r.get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };
                let status = field("status");
                let detail = field("detail");
                let mark = match status.as_str() {
                    "pass" => "✓",
                    "warn" => "⚠",
                    _ => "✗",
                };
                (field("name"), format!("{mark} {status}  {detail}"))
            })
            .collect())
    }
}

/// Registry backend names that run on `bwoc-harness` (the chat TUI).
pub fn session_backends() -> Vec<&'static str> {
    use clap::ValueEnum;
    Backend::value_variants()
        .iter()
        .filter(|b| b.uses_harness())
        .map(|b| b.display_name())
        .collect()
}

/// Registry backend names that hand the session to a vendor CLI.
pub fn vendor_backends() -> Vec<&'static str> {
    use clap::ValueEnum;
    Backend::value_variants()
        .iter()
        .filter(|b| b.cli_name().is_some())
        .map(|b| b.display_name())
        .collect()
}

pub fn resolve(merged: &RuntimeLayer, probes: &Probes) -> Resolution {
    let ready = |backend: &str, model: String| {
        Resolution::Ready(Resolved {
            backend: backend.to_string(),
            model,
            endpoint: merged.endpoint.clone(),
            max_tokens: merged.max_tokens,
            max_context: merged.max_context,
        })
    };

    let Some(backend) = merged.backend.as_deref() else {
        if (probes.anthropic_key)() {
            let model = merged
                .model
                .clone()
                .unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string());
            return ready("anthropic", model);
        }
        return match (probes.ollama_models)(merged.endpoint.as_deref()) {
            Some(models) if !models.is_empty() => {
                let model = merged.model.clone().unwrap_or_else(|| models[0].clone());
                ready("ollama", model)
            }
            _ => Resolution::NoProvider,
        };
    };

    match Backend::from_registry_name(backend) {
        Some(b) if b.uses_harness() => {}
        Some(b) if b.cli_name().is_some() => {
            return Resolution::Vendor(VendorSession {
                backend: b,
                model: merged.model.clone(),
            });
        }
        _ => {
            return Resolution::Invalid(format!(
                "backend '{backend}' is not a session backend — use one of: {} \
                 (bwoc-harness), or a vendor CLI: {}",
                session_backends().join(", "),
                vendor_backends().join(", ")
            ));
        }
    }
    if let Some(model) = merged.model.clone() {
        return ready(backend, model);
    }
    match backend {
        "anthropic" => ready(backend, DEFAULT_ANTHROPIC_MODEL.to_string()),
        "ollama" => match (probes.ollama_models)(merged.endpoint.as_deref()) {
            Some(models) if !models.is_empty() => ready(backend, models[0].clone()),
            _ => Resolution::Invalid(
                "backend 'ollama' has no model: Ollama did not answer or has no models \
                 (`ollama pull <model>`), and none is set (--model, BWOC_MODEL, [runtime] model)"
                    .to_string(),
            ),
        },
        other => Resolution::Invalid(format!(
            "backend '{other}' needs a model — set --model, BWOC_MODEL, or [runtime] model"
        )),
    }
}

/// Models on the Ollama answering at `endpoint` (default localhost:11434).
pub fn probe_ollama(endpoint: Option<&str>) -> Option<Vec<String>> {
    let addr = crate::doctor::ollama_addr(endpoint).ok()?;
    crate::doctor::ollama_models(&addr)
}

/// Vendor-CLI backends whose program is on `PATH`, in registry order.
pub fn vendor_clis_on_path() -> Vec<&'static str> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let found = |cli: &str| {
        dirs.iter().any(|d| {
            d.join(cli).is_file() || (cfg!(windows) && d.join(format!("{cli}.exe")).is_file())
        })
    };
    use clap::ValueEnum;
    Backend::value_variants()
        .iter()
        .filter(|b| b.cli_name().is_some_and(found))
        .map(|b| b.display_name())
        .collect()
}

/// Every vendor-CLI backend in the registry, installed or not (the `/backends`
/// view says which). [`vendor_clis_on_path`] is the installed subset.
pub fn vendor_cli_backends() -> Vec<&'static str> {
    use clap::ValueEnum;
    Backend::value_variants()
        .iter()
        .filter(|b| b.cli_name().is_some())
        .map(|b| b.display_name())
        .collect()
}

/// Printed (to stderr) when nothing is configured or detected. `vendor_clis`
/// are the vendor-CLI backends found on `PATH` (see [`vendor_clis_on_path`]).
pub fn no_provider_help(vendor_clis: &[&str]) -> String {
    let mut out = String::from(
        "\
bwoc: no model provider configured for this session.

Pick one:
  API key     bwoc auth set anthropic      (or: export ANTHROPIC_API_KEY=...)
              bwoc auth set openrouter     then: export BWOC_BACKEND=openrouter BWOC_MODEL=<model>
  Local       ollama serve && ollama pull <model>   (found automatically on localhost:11434)
",
    );
    let (cmd, why) = match vendor_clis.first() {
        Some(first) => (
            format!("bwoc --backend {first}"),
            format!("found on PATH: {}", vendor_clis.join(", ")),
        ),
        None => (
            "bwoc --backend <cli>".to_string(),
            vendor_backends().join(", "),
        ),
    };
    out.push_str(&format!(
        "  Vendor CLI  {cmd:<29}({why}; runs on the CLI's own login)\n"
    ));
    out.push_str(
        "
Or pin it in ~/.bwoc/config.toml, or <repo>/.bwoc/config.toml:
  schema_version = 3
  [runtime]
  backend = \"ollama\"
  model   = \"<model>\"

`bwoc --help` lists commands; `bwoc about` prints the banner.
",
    );
    out
}

/// Argv (after the program name) for a vendor-CLI session. Every supported
/// CLI takes `--model <id>`; without a model the CLI keeps its own default.
pub fn vendor_argv(v: &VendorSession) -> Vec<String> {
    match &v.model {
        Some(m) => vec!["--model".to_string(), m.clone()],
        None => Vec::new(),
    }
}

/// One-line stderr notice before the vendor CLI takes the terminal.
pub fn vendor_notice(v: &VendorSession, cli: &str) -> String {
    format!(
        "bwoc: handing this session to `{cli}` (backend '{}') — it runs on its own login, \
         tools and permissions; bwoc-harness tools and trust gates do not apply.",
        v.backend.display_name()
    )
}

/// Replace this process with the vendor CLI in `cwd` (Unix `exec`, so the CLI
/// owns the terminal and its signals); elsewhere run it and pass its exit code
/// through. Returns only on failure, or with the child's code off Unix.
fn exec_vendor(v: &VendorSession, cwd: &Path, merged: &RuntimeLayer) -> i32 {
    let cli = v
        .backend
        .cli_name()
        .expect("Vendor resolution only for backends with a cli_name");
    if merged.endpoint.is_some() || merged.max_tokens.is_some() || merged.max_context.is_some() {
        eprintln!(
            "bwoc: note: endpoint / max_tokens / max_context apply to bwoc-harness backends; \
             ignored for '{}'",
            v.backend.display_name()
        );
    }
    eprintln!("{}", vendor_notice(v, cli));
    let mut cmd = std::process::Command::new(cli);
    cmd.args(vendor_argv(v)).current_dir(cwd);
    let not_found = |e: &std::io::Error| {
        if e.kind() == std::io::ErrorKind::NotFound {
            eprintln!(
                "bwoc: backend '{}' needs the `{cli}` CLI on PATH — install it, or pick another \
                 --backend",
                v.backend.display_name()
            );
            exit::USAGE
        } else {
            eprintln!("bwoc: cannot run `{cli}`: {e}");
            exit::ERROR
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        not_found(&e)
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => status.code().unwrap_or(exit::ERROR),
            Err(e) => not_found(&e),
        }
    }
}

/// Where bare `bwoc` goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BareRoute {
    /// Interactive terminal: open the coding session.
    Session,
    /// Not a terminal, no session flags: print the banner, as before.
    Banner,
    /// Session flags given, but not on a terminal.
    NeedsTerminal,
}

pub fn bare_route(stdin_tty: bool, stdout_tty: bool, session_flags: bool) -> BareRoute {
    match (stdin_tty && stdout_tty, session_flags) {
        (true, _) => BareRoute::Session,
        (false, false) => BareRoute::Banner,
        (false, true) => BareRoute::NeedsTerminal,
    }
}

/// Bare `bwoc` on a terminal: resolve the runtime and open the chat TUI.
pub fn run_session(flags: RuntimeLayer, pick: crate::coding_session::SessionPick) -> i32 {
    let cwd = match std::env::current_dir().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bwoc: cannot resolve the current directory: {e}");
            return exit::ERROR;
        }
    };
    let home = crate::user_home::bwoc_home().ok();
    let getenv = |k: &str| std::env::var(k).ok();
    let user_config = home.as_ref().map(|h| h.join("config.toml"));
    let layers = match gather_layers(flags, &getenv, &cwd, user_config.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bwoc: {e}");
            return e.exit_code();
        }
    };
    let merged = merge(&layers);

    let secrets = home.as_ref().map(|h| h.join("secrets.toml"));
    let anthropic_key = || {
        let table = secrets
            .as_deref()
            .map(crate::auth::read_secrets)
            .unwrap_or(crate::auth::Secrets::Missing);
        crate::auth::key_source("anthropic", &getenv, &table).is_some()
    };
    let probes = Probes {
        anthropic_key: &anthropic_key,
        ollama_models: &probe_ollama,
    };

    match resolve(&merged, &probes) {
        Resolution::Ready(r) => {
            // The conversation lives under ~/.bwoc/sessions/, never in the
            // repository. No home means no persistence, as before.
            let mut sessions: Option<Box<dyn bwoc_tui::SessionControl>> = None;
            let session_file = match home.as_deref() {
                Some(h) => {
                    let store = crate::coding_session::SessionStore::new(h, &cwd);
                    match store
                        .prepare()
                        .and_then(|()| store.resolve(&pick, std::time::SystemTime::now()))
                    {
                        Ok(path) => {
                            // Same store, handed to the TUI so `/session`,
                            // `/new` and `/fork` work from inside the session.
                            let open = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                            sessions = Some(Box::new(crate::coding_session::TuiSessions::new(
                                crate::coding_session::SessionStore::new(h, &cwd),
                                open,
                            )));
                            Some(path)
                        }
                        Err(e) => {
                            eprintln!("bwoc: {e}");
                            return match e {
                                crate::coding_session::SessionError::Io(_) => exit::ERROR,
                                _ => exit::USAGE,
                            };
                        }
                    }
                }
                None => None,
            };
            // Labelled now, while the layers that produced them are in scope.
            let project_config = find_project_config(&cwd);
            let mut settings = vec![
                ("backend".to_string(), r.backend.clone()),
                ("model".to_string(), r.model.clone()),
            ];
            if let Some(e) = &r.endpoint {
                settings.push(("endpoint".to_string(), e.clone()));
            }
            if let Some(n) = r.max_tokens {
                settings.push(("max_tokens".to_string(), n.to_string()));
            }
            if let Some(n) = r.max_context {
                settings.push(("max_context".to_string(), n.to_string()));
            }
            settings.push((
                "project config".to_string(),
                project_config
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(none)".to_string()),
            ));
            settings.push((
                "user config".to_string(),
                user_config
                    .as_ref()
                    .filter(|p| p.is_file())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(none)".to_string()),
            ));
            settings.push((
                "precedence".to_string(),
                "flags → env → project config → user config → auto-detect".to_string(),
            ));
            let environment: Option<Box<dyn bwoc_tui::EnvironmentInfo>> =
                Some(Box::new(Environment {
                    backend: r.backend.clone(),
                    endpoint: r.endpoint.clone(),
                    settings,
                    home: home.clone(),
                }));

            let name = cwd
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".to_string());
            bwoc_tui::run(bwoc_tui::TuiArgs {
                agent_id: name,
                agent_path: cwd.clone(),
                backend_name: r.backend,
                team_chat: None,
                project: Some(bwoc_tui::ProjectSession {
                    model: r.model,
                    endpoint: r.endpoint,
                    max_tokens: r.max_tokens,
                    max_context: r.max_context,
                    session_file,
                    sessions,
                    environment,
                }),
            })
        }
        Resolution::Vendor(_) if pick != crate::coding_session::SessionPick::Latest => {
            eprintln!(
                "bwoc: --new / --session pick a bwoc conversation; a vendor CLI keeps its own \
                 sessions"
            );
            exit::USAGE
        }
        Resolution::Vendor(v) => exec_vendor(&v, &cwd, &merged),
        Resolution::NoProvider => {
            eprint!("{}", no_provider_help(&vendor_clis_on_path()));
            exit::USAGE
        }
        Resolution::Invalid(msg) => {
            eprintln!("bwoc: {msg}");
            exit::USAGE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(backend: Option<&str>, model: Option<&str>) -> RuntimeLayer {
        RuntimeLayer {
            backend: backend.map(str::to_string),
            model: model.map(str::to_string),
            ..Default::default()
        }
    }

    fn probes<'a>(
        key: &'a dyn Fn() -> bool,
        ollama: &'a dyn Fn(Option<&str>) -> Option<Vec<String>>,
    ) -> Probes<'a> {
        Probes {
            anthropic_key: key,
            ollama_models: ollama,
        }
    }

    #[test]
    fn parse_reads_runtime_table_and_ignores_unknown_keys() {
        let l = parse_config(
            "schema_version = 3\nfuture_key = 1\n[runtime]\nbackend = \"ollama\"\nmodel = \"m\"\n\
             endpoint = \"http://h:1/v1\"\nmax_tokens = 4096\nmax_context = 32768\nnew_knob = true\n\
             [other]\nx = 1\n",
            Path::new("c.toml"),
        )
        .unwrap();
        assert_eq!(l.backend.as_deref(), Some("ollama"));
        assert_eq!(l.model.as_deref(), Some("m"));
        assert_eq!(l.endpoint.as_deref(), Some("http://h:1/v1"));
        assert_eq!(l.max_tokens, Some(4096));
        assert_eq!(l.max_context, Some(32768));
    }

    #[test]
    fn parse_absent_marker_is_legacy_and_readable() {
        let l = parse_config("[runtime]\nmodel = \"m\"\n", Path::new("c.toml")).unwrap();
        assert_eq!(l.model.as_deref(), Some("m"));
        // The stub `bwoc` has always written: comments only.
        let empty = parse_config("# bwoc user-level config\n", Path::new("c.toml")).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn parse_future_schema_is_refused_naming_schema_version() {
        let err = parse_config(
            "schema_version = 99\n[runtime]\nmodel = \"m\"\n",
            Path::new("c.toml"),
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::FutureSchema { found: 99, .. }));
        assert!(err.to_string().contains("schema_version"));
    }

    #[test]
    fn merge_follows_precedence_field_by_field() {
        let flags = RuntimeLayer {
            model: Some("flag-model".into()),
            ..Default::default()
        };
        let env = layer(Some("ollama"), Some("env-model"));
        let project = RuntimeLayer {
            endpoint: Some("http://project/v1".into()),
            max_tokens: Some(1000),
            ..Default::default()
        };
        let user = layer(Some("ollama"), Some("user-model"));
        let m = merge(&[flags, env, project, user]);
        assert_eq!(m.backend.as_deref(), Some("ollama"));
        assert_eq!(m.model.as_deref(), Some("flag-model"));
        assert_eq!(m.endpoint.as_deref(), Some("http://project/v1"));
        assert_eq!(m.max_tokens, Some(1000));
    }

    #[test]
    fn merge_drops_settings_from_a_layer_naming_another_backend() {
        let env = layer(Some("ollama"), None);
        let user = RuntimeLayer {
            backend: Some("anthropic".into()),
            model: Some("vendor-model".into()),
            endpoint: Some("https://vendor".into()),
            max_tokens: Some(9),
            max_context: Some(99),
        };
        let m = merge(&[RuntimeLayer::default(), env, user]);
        assert_eq!(m.backend.as_deref(), Some("ollama"));
        assert_eq!(m.model, None);
        assert_eq!(m.endpoint, None);
        assert_eq!(m.max_tokens, None);
        assert_eq!(m.max_context, None);
    }

    #[test]
    fn gather_layers_orders_flags_env_project_user() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let sub = repo.join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join(".bwoc")).unwrap();
        std::fs::write(
            repo.join(".bwoc/config.toml"),
            "[runtime]\nbackend = \"ollama\"\nmodel = \"project-model\"\nendpoint = \"http://p/v1\"\n",
        )
        .unwrap();
        let user = root.path().join("user.toml");
        std::fs::write(&user, "[runtime]\nmodel = \"user-model\"\nmax_tokens = 7\n").unwrap();

        let env = |k: &str| (k == ENV_MODEL).then(|| "env-model".to_string());
        let layers = gather_layers(RuntimeLayer::default(), &env, &sub, Some(&user)).unwrap();
        let m = merge(&layers);
        assert_eq!(m.backend.as_deref(), Some("ollama"));
        assert_eq!(m.model.as_deref(), Some("env-model"));
        assert_eq!(m.endpoint.as_deref(), Some("http://p/v1"));
        assert_eq!(m.max_tokens, Some(7));

        let flags = layer(None, Some("flag-model"));
        let m = merge(&gather_layers(flags, &env, &sub, Some(&user)).unwrap());
        assert_eq!(m.model.as_deref(), Some("flag-model"));
    }

    #[test]
    fn gather_layers_refuses_future_user_config() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("config.toml");
        std::fs::write(&user, "schema_version = 4\n").unwrap();
        let err = gather_layers(RuntimeLayer::default(), &|_| None, root.path(), Some(&user))
            .unwrap_err();
        assert!(err.to_string().contains("schema_version = 4"));
    }

    #[test]
    fn project_config_search_stops_at_git_root() {
        let root = tempfile::tempdir().unwrap();
        // A config above the repository must not apply inside it.
        std::fs::create_dir_all(root.path().join(".bwoc")).unwrap();
        std::fs::write(root.path().join(".bwoc/config.toml"), "").unwrap();
        let repo = root.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        assert_eq!(find_project_config(&repo.join("src")), None);

        // Outside git, only the cwd itself counts.
        let loose = root.path().join("loose");
        std::fs::create_dir_all(&loose).unwrap();
        assert_eq!(find_project_config(&loose), None);
        assert_eq!(
            find_project_config(root.path()),
            Some(root.path().join(".bwoc/config.toml"))
        );
    }

    #[test]
    fn default_prefers_anthropic_key() {
        let key = || true;
        let ollama = |_: Option<&str>| panic!("ollama must not be probed when a key exists");
        let r = resolve(&RuntimeLayer::default(), &probes(&key, &ollama));
        assert_eq!(
            r,
            Resolution::Ready(Resolved {
                backend: "anthropic".into(),
                model: DEFAULT_ANTHROPIC_MODEL.into(),
                endpoint: None,
                max_tokens: None,
                max_context: None,
            })
        );
    }

    #[test]
    fn default_falls_back_to_first_ollama_model() {
        let key = || false;
        let ollama = |ep: Option<&str>| {
            assert_eq!(ep, None);
            Some(vec!["local-a:latest".to_string(), "local-b".to_string()])
        };
        match resolve(&RuntimeLayer::default(), &probes(&key, &ollama)) {
            Resolution::Ready(r) => {
                assert_eq!(r.backend, "ollama");
                assert_eq!(r.model, "local-a:latest");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn default_without_key_or_ollama_is_no_provider() {
        let key = || false;
        let down = |_: Option<&str>| None;
        let empty = |_: Option<&str>| Some(Vec::new());
        assert_eq!(
            resolve(&RuntimeLayer::default(), &probes(&key, &down)),
            Resolution::NoProvider
        );
        assert_eq!(
            resolve(&RuntimeLayer::default(), &probes(&key, &empty)),
            Resolution::NoProvider
        );
        assert!(no_provider_help(&[]).contains("bwoc auth set anthropic"));
    }

    #[test]
    fn explicit_backend_rules() {
        let key = || true;
        let down = |_: Option<&str>| None;
        let p = probes(&key, &down);

        // `cli` is the harness's generic subscription provider, not a session
        // backend: the error lists both families so the vendor name is findable.
        let r = resolve(&layer(Some("cli"), None), &p);
        assert!(
            matches!(r, Resolution::Invalid(ref m) if m.contains("anthropic") && m.contains("claude")),
            "{r:?}"
        );

        // openrouter without a model is a usage error, not a guess.
        assert!(matches!(
            resolve(&layer(Some("openrouter"), None), &p),
            Resolution::Invalid(_)
        ));
        // ollama without a model and no server.
        assert!(matches!(
            resolve(&layer(Some("ollama"), None), &p),
            Resolution::Invalid(_)
        ));
        // An explicit backend + model never probes anything.
        let never = || panic!("no probe");
        let never_o = |_: Option<&str>| panic!("no probe");
        match resolve(
            &layer(Some("litellm"), Some("m")),
            &probes(&never, &never_o),
        ) {
            Resolution::Ready(r) => {
                assert_eq!((r.backend.as_str(), r.model.as_str()), ("litellm", "m"))
            }
            other => panic!("{other:?}"),
        }
    }

    /// #529: every vendor coding CLI resolves to a vendor session — no key,
    /// no Ollama, no probe — carrying the model only when one is named.
    #[test]
    fn vendor_backends_resolve_without_probing() {
        let never = || panic!("no probe");
        let never_o = |_: Option<&str>| panic!("no probe");
        let p = probes(&never, &never_o);
        for name in ["claude", "codex", "agy", "kimi", "grok", "copilot"] {
            let expected = Backend::from_registry_name(name).unwrap();
            assert_eq!(
                resolve(&layer(Some(name), None), &p),
                Resolution::Vendor(VendorSession {
                    backend: expected,
                    model: None,
                }),
                "{name}"
            );
            match resolve(&layer(Some(name), Some("m1")), &p) {
                Resolution::Vendor(v) => assert_eq!(v.model.as_deref(), Some("m1")),
                other => panic!("{name}: {other:?}"),
            }
        }
        assert_eq!(
            vendor_backends(),
            ["claude", "agy", "codex", "kimi", "copilot", "grok"]
        );
    }

    #[test]
    fn vendor_argv_forwards_only_a_named_model() {
        let with = VendorSession {
            backend: Backend::Codex,
            model: Some("gpt-x".into()),
        };
        assert_eq!(vendor_argv(&with), ["--model", "gpt-x"]);
        let without = VendorSession {
            backend: Backend::Kimi,
            model: None,
        };
        assert!(vendor_argv(&without).is_empty());
        let notice = vendor_notice(&with, "codex");
        assert!(notice.contains("`codex`") && notice.contains("trust gates"));
    }

    /// #529: `[defaults] backend` (what the reporter's config declared) now
    /// reaches the session when `[runtime]` names none; `[runtime]` still wins.
    #[test]
    fn defaults_backend_stands_in_for_runtime_backend() {
        let p = Path::new("c.toml");
        let l = parse_config("[defaults]\nbackend = \"claude\"\n", p).unwrap();
        assert_eq!(l.backend.as_deref(), Some("claude"));
        let l = parse_config(
            "[defaults]\nbackend = \"claude\"\n[runtime]\nbackend = \"ollama\"\n",
            p,
        )
        .unwrap();
        assert_eq!(l.backend.as_deref(), Some("ollama"));
        let l = parse_config(
            "[defaults]\nbackend = \"codex\"\n[runtime]\nbackend = \" \"\n",
            p,
        )
        .unwrap();
        assert_eq!(l.backend.as_deref(), Some("codex"));
    }

    #[test]
    fn no_provider_help_names_vendor_clis_found_on_path() {
        let found = no_provider_help(&["claude", "codex"]);
        assert!(found.contains("bwoc --backend claude"), "{found}");
        assert!(found.contains("found on PATH: claude, codex"), "{found}");
        let none = no_provider_help(&[]);
        assert!(none.contains("bwoc --backend <cli>"), "{none}");
        assert!(none.contains("kimi") && none.contains("grok"), "{none}");
    }

    #[test]
    fn bare_route_only_opens_a_session_on_a_full_tty() {
        assert_eq!(bare_route(true, true, false), BareRoute::Session);
        assert_eq!(bare_route(true, true, true), BareRoute::Session);
        for (i, o) in [(false, false), (true, false), (false, true)] {
            assert_eq!(bare_route(i, o, false), BareRoute::Banner);
            assert_eq!(bare_route(i, o, true), BareRoute::NeedsTerminal);
        }
    }

    #[test]
    fn config_errors_map_to_the_exit_code_contract() {
        let p = Path::new("/x/.bwoc/config.toml");
        let parse = parse_config("runtime = [", p).unwrap_err();
        assert_eq!(parse.exit_code(), exit::USAGE);
        let future = parse_config("schema_version = 999\n", p).unwrap_err();
        assert!(matches!(future, RuntimeError::FutureSchema { .. }));
        assert_eq!(future.exit_code(), exit::USAGE);
        let io = RuntimeError::Io {
            path: "p".into(),
            msg: "denied".into(),
        };
        assert_eq!(io.exit_code(), exit::ERROR);
    }
}
