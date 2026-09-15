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
//! backend    = "ollama"
//! model      = "<model>"
//! endpoint   = "http://localhost:11434/v1"   # optional
//! max_tokens = 8192                          # optional
//! ```
//!
//! Absent `schema_version` reads as legacy (`bwoc-core::schema`); a newer one is
//! refused. Unknown keys and tables are ignored.

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
}

#[derive(Debug, Deserialize, Default)]
struct RuntimeTable {
    backend: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
    max_tokens: Option<u32>,
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
    Ok(RuntimeLayer {
        backend: r.backend,
        model: r.model,
        endpoint: r.endpoint,
        max_tokens: r.max_tokens,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(Resolved),
    /// Nothing configured and nothing detected.
    NoProvider,
    /// Configured, but unusable — the message says what to change.
    Invalid(String),
}

/// Registry backend names a session can use: those that run `bwoc-harness`.
pub fn session_backends() -> Vec<&'static str> {
    use clap::ValueEnum;
    Backend::value_variants()
        .iter()
        .filter(|b| b.uses_harness())
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

    if !Backend::from_registry_name(backend).is_some_and(|b| b.uses_harness()) {
        return Resolution::Invalid(format!(
            "backend '{backend}' can't run a bwoc session — use one of: {}",
            session_backends().join(", ")
        ));
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

/// Printed (to stderr) when nothing is configured or detected.
pub fn no_provider_help() -> String {
    "\
bwoc: no model provider configured for this session.

Pick one:
  API key     bwoc auth set anthropic      (or: export ANTHROPIC_API_KEY=...)
              bwoc auth set openrouter     then: export BWOC_BACKEND=openrouter BWOC_MODEL=<model>
  Local       ollama serve && ollama pull <model>   (found automatically on localhost:11434)

Or pin it in ~/.bwoc/config.toml, or <repo>/.bwoc/config.toml:
  schema_version = 3
  [runtime]
  backend = \"ollama\"
  model   = \"<model>\"

`bwoc --help` lists commands; `bwoc about` prints the banner.
"
    .to_string()
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

/// The session's persisted conversation: one file per directory under
/// `~/.bwoc/sessions/`, so a session never writes into the repository itself.
pub fn session_file_for(bwoc_home: &Path, cwd: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(cwd.to_string_lossy().as_bytes());
    let id: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    bwoc_home.join("sessions").join(format!("{id}.json"))
}

/// Bare `bwoc` on a terminal: resolve the runtime and open the chat TUI.
pub fn run_session(flags: RuntimeLayer) -> i32 {
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
            return exit::ERROR;
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
                    session_file: home.as_deref().map(|h| session_file_for(h, &cwd)),
                }),
            })
        }
        Resolution::NoProvider => {
            eprint!("{}", no_provider_help());
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
             endpoint = \"http://h:1/v1\"\nmax_tokens = 4096\nnew_knob = true\n[other]\nx = 1\n",
            Path::new("c.toml"),
        )
        .unwrap();
        assert_eq!(l.backend.as_deref(), Some("ollama"));
        assert_eq!(l.model.as_deref(), Some("m"));
        assert_eq!(l.endpoint.as_deref(), Some("http://h:1/v1"));
        assert_eq!(l.max_tokens, Some(4096));
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
        };
        let m = merge(&[RuntimeLayer::default(), env, user]);
        assert_eq!(m.backend.as_deref(), Some("ollama"));
        assert_eq!(m.model, None);
        assert_eq!(m.endpoint, None);
        assert_eq!(m.max_tokens, None);
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
        assert!(no_provider_help().contains("bwoc auth set anthropic"));
    }

    #[test]
    fn explicit_backend_rules() {
        let key = || true;
        let down = |_: Option<&str>| None;
        let p = probes(&key, &down);

        // A vendor-CLI backend can't drive the harness.
        let r = resolve(&layer(Some("claude"), None), &p);
        assert!(
            matches!(r, Resolution::Invalid(ref m) if m.contains("anthropic")),
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
    fn session_file_is_per_directory_under_bwoc_home() {
        let home = Path::new("/h/.bwoc");
        let a = session_file_for(home, Path::new("/src/a"));
        let b = session_file_for(home, Path::new("/src/b"));
        assert_ne!(a, b);
        assert!(a.starts_with("/h/.bwoc/sessions"));
        assert_eq!(a, session_file_for(home, Path::new("/src/a")));
    }
}
