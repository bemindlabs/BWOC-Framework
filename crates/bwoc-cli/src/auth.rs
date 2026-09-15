//! `bwoc auth` — provider API keys for `bwoc` sessions.
//!
//! Keys live per user in `~/.bwoc/secrets.toml` (`[<provider>] api_key`), the
//! file the harness already reads (`bwoc-harness` `provider::anthropic`). This
//! module is the only writer. Rules:
//!
//! - The key is never printed, and neither is its length.
//! - A new file is created `0600`. An existing file that is group- or
//!   world-accessible is refused (the harness ignores such a file too), and the
//!   user is told to `chmod 600` it. BWOC never loosens or silently fixes modes.
//! - Other sections and comments survive: the file is edited with `toml_edit`,
//!   then replaced atomically (temp file + rename).
//!
//! `.bwoc/secrets.toml` is deliberately unversioned (`COMPATIBILITY.en.md`).

use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;

use clap::{Args, Subcommand};

use crate::exit;

/// Providers `bwoc auth set` accepts.
pub const PROVIDER_NAMES: [&str; 4] = ["anthropic", "openrouter", "litellm", "openai-compatible"];

/// The env var the harness checks before `secrets.toml`, per provider. `None`:
/// the key comes from `secrets.toml` only. `openai-compatible` has no env var
/// on purpose: a generic `OPENAI_API_KEY` would be sent to whatever endpoint the
/// session points at.
pub fn env_var_for(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "litellm" => Some("LITELLM_API_KEY"),
        _ => None,
    }
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Store a provider API key in `~/.bwoc/secrets.toml` (created `0600`). The
    /// key is read from stdin (hidden on a terminal) or `--from-env VAR`, and is
    /// never printed.
    Set(AuthSetArgs),
    /// List which providers have a key and where it comes from. Never prints a
    /// key or its length.
    Status,
}

#[derive(Args, Debug)]
pub struct AuthSetArgs {
    /// Provider whose key to store.
    #[arg(value_parser = clap::builder::PossibleValuesParser::new(PROVIDER_NAMES))]
    pub provider: String,
    /// Read the key from this environment variable instead of stdin.
    #[arg(long = "from-env", value_name = "VAR")]
    pub from_env: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error(
        "{path} is group/world-accessible — refusing to write a key into it. \
         Run `chmod 600 {path}` and retry."
    )]
    Permissive { path: String },
    // No parser message: toml errors quote the offending line, which may hold a key.
    #[error("{path} is not valid TOML — fix it or move it aside, then retry")]
    Parse { path: String },
    #[error("the key is empty — nothing written")]
    Empty,
    #[error("the key spans several lines — paste a single-line key")]
    Multiline,
    #[error("{0}")]
    Io(#[from] io::Error),
}

pub fn run(cmd: AuthCommand) -> i32 {
    let home = match crate::user_home::bwoc_home() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("bwoc auth: {e}");
            return exit::ERROR;
        }
    };
    let path = home.join("secrets.toml");
    let getenv = |k: &str| std::env::var(k).ok();
    match cmd {
        AuthCommand::Set(args) => run_set(&args, &path),
        AuthCommand::Status => {
            print!("{}", render_status(&path, &getenv));
            exit::OK
        }
    }
}

fn run_set(args: &AuthSetArgs, path: &Path) -> i32 {
    let key = match &args.from_env {
        Some(var) => match std::env::var(var) {
            Ok(v) if !v.trim().is_empty() => v,
            _ => {
                eprintln!("bwoc auth set: environment variable {var} is unset or empty");
                return exit::USAGE;
            }
        },
        None => match read_key_from_stdin(&args.provider) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("bwoc auth set: could not read the key from stdin: {e}");
                return exit::ERROR;
            }
        },
    };
    match store_key(path, &args.provider, &key) {
        Ok(()) => {
            println!(
                "bwoc auth: stored the {} key in {}",
                args.provider,
                path.display()
            );
            exit::OK
        }
        Err(e @ (AuthError::Empty | AuthError::Multiline)) => {
            eprintln!("bwoc auth set: {e}");
            exit::USAGE
        }
        Err(e) => {
            eprintln!("bwoc auth set: {e}");
            exit::ERROR
        }
    }
}

/// Read one key from stdin. On a terminal the prompt goes to stderr and echo is
/// switched off; from a pipe the whole input is the key.
fn read_key_from_stdin(provider: &str) -> io::Result<String> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        eprint!("{provider} API key (input hidden): ");
        io::stderr().flush()?;
        let guard = EchoGuard::disable();
        let mut line = String::new();
        let read = stdin.read_line(&mut line);
        drop(guard);
        eprintln!();
        read?;
        Ok(line.trim().to_string())
    } else {
        let mut all = String::new();
        stdin.lock().read_to_string(&mut all)?;
        Ok(all.trim().to_string())
    }
}

/// Switches terminal echo off for its lifetime (restored on drop).
#[cfg(unix)]
struct EchoGuard(Option<libc::termios>);

#[cfg(unix)]
impl EchoGuard {
    fn disable() -> Self {
        // SAFETY: tcgetattr/tcsetattr only read and set stdin's terminal flags;
        // `termios` is plain data, so a zeroed value is a valid out-parameter.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
                return Self(None);
            }
            let original = t;
            t.c_lflag &= !libc::ECHO;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t) != 0 {
                return Self(None);
            }
            arm_interrupt_restore(original);
            Self(Some(original))
        }
    }
}

#[cfg(unix)]
impl Drop for EchoGuard {
    fn drop(&mut self) {
        if let Some(original) = self.0 {
            take_saved_termios();
            // SAFETY: restores the flags captured in `disable`.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
            }
        }
    }
}

/// The terminal flags to put back if the hidden prompt is interrupted.
#[cfg(unix)]
static SAVED_TERMIOS: std::sync::Mutex<Option<libc::termios>> = std::sync::Mutex::new(None);

/// Ctrl-C ends the process without running `Drop`, which left echo off. Record
/// the original flags and, once per process, install a SIGINT handler that
/// restores them and exits 130 (the shell's interrupt code).
#[cfg(unix)]
fn arm_interrupt_restore(original: libc::termios) {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    *SAVED_TERMIOS.lock().unwrap_or_else(|e| e.into_inner()) = Some(original);
    INSTALL.call_once(|| {
        let _ = ctrlc::set_handler(|| {
            if let Some(original) = take_saved_termios() {
                // SAFETY: restores the flags captured in `EchoGuard::disable`.
                unsafe {
                    libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original);
                }
            }
            eprintln!();
            std::process::exit(130);
        });
    });
}

/// Take the flags recorded by [`arm_interrupt_restore`], leaving nothing armed.
#[cfg(unix)]
fn take_saved_termios() -> Option<libc::termios> {
    SAVED_TERMIOS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

#[cfg(not(unix))]
struct EchoGuard;

#[cfg(not(unix))]
impl EchoGuard {
    fn disable() -> Self {
        eprint!("(input is visible on this platform) ");
        Self
    }
}

/// True when a file's mode lets anyone but the owner read or write it.
#[cfg(unix)]
fn is_permissive(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o077 != 0
}

#[cfg(not(unix))]
fn is_permissive(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Write `[provider] api_key = key` into the secrets file at `path`, creating
/// it `0600` or editing it in place (other sections and comments preserved).
pub fn store_key(path: &Path, provider: &str, key: &str) -> Result<(), AuthError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AuthError::Empty);
    }
    if key.contains(['\n', '\r']) {
        return Err(AuthError::Multiline);
    }
    let shown = path.display().to_string();
    let existing = match std::fs::metadata(path) {
        Ok(meta) => {
            if is_permissive(&meta) {
                return Err(AuthError::Permissive { path: shown });
            }
            std::fs::read_to_string(path)?
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc: toml_edit::DocumentMut = existing
        .parse()
        .map_err(|_| AuthError::Parse { path: shown })?;
    if doc.get(provider).and_then(|i| i.as_table_like()).is_none() {
        doc.insert(provider, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    doc[provider]
        .as_table_like_mut()
        .expect("table ensured above")
        .insert("api_key", toml_edit::value(key));
    write_private(path, doc.to_string().as_bytes())?;
    Ok(())
}

/// Replace `path` atomically with a `0600` file holding `bytes`.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".secrets.toml.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// What reading the secrets file yielded.
pub enum Secrets {
    Missing,
    /// Group/world-accessible — ignored, as the harness ignores it.
    Refused,
    Invalid,
    Table(toml::Table),
}

pub fn read_secrets(path: &Path) -> Secrets {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Secrets::Missing,
    };
    if is_permissive(&meta) {
        return Secrets::Refused;
    }
    match std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
    {
        Some(t) => Secrets::Table(t),
        None => Secrets::Invalid,
    }
}

/// Where a provider's key would come from, or `None` when it has none.
pub fn key_source(
    provider: &str,
    getenv: &dyn Fn(&str) -> Option<String>,
    secrets: &Secrets,
) -> Option<String> {
    if let Some(var) = env_var_for(provider) {
        if getenv(var).is_some_and(|v| !v.trim().is_empty()) {
            return Some(format!("env {var}"));
        }
    }
    let Secrets::Table(t) = secrets else {
        return None;
    };
    let stored = t
        .get(provider)
        .and_then(|s| s.get("api_key"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty());
    stored.then(|| "secrets.toml".to_string())
}

/// `bwoc auth status` output. Names and sources only.
pub fn render_status(path: &Path, getenv: &dyn Fn(&str) -> Option<String>) -> String {
    let secrets = read_secrets(path);
    let mut out = String::new();
    match secrets {
        Secrets::Refused => out.push_str(&format!(
            "warning: {} is group/world-accessible and is ignored — run `chmod 600 {}`\n",
            path.display(),
            path.display()
        )),
        Secrets::Invalid => out.push_str(&format!(
            "warning: {} is not valid TOML and is ignored\n",
            path.display()
        )),
        Secrets::Missing | Secrets::Table(_) => {}
    }
    for provider in PROVIDER_NAMES {
        let source = key_source(provider, getenv, &secrets).unwrap_or_else(|| "not set".into());
        out.push_str(&format!("{provider:<18} {source}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[cfg(unix)]
    #[test]
    fn interrupt_restore_state_is_taken_once() {
        // The SIGINT handler and `Drop` both take the recorded flags, so the
        // terminal is restored at most once and nothing stays armed afterwards.
        // (The flags are set directly: `arm_interrupt_restore` would install a
        // real handler, and restoring would touch the test's terminal.)
        // SAFETY: `termios` is plain data; a zeroed value is only stored and compared.
        let flags: libc::termios = unsafe { std::mem::zeroed() };
        *SAVED_TERMIOS.lock().unwrap() = Some(flags);
        assert!(take_saved_termios().is_some());
        assert!(take_saved_termios().is_none());
    }

    #[test]
    fn store_creates_file_with_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("secrets.toml");
        store_key(&path, "anthropic", "  sk-test-one\n").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let t: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(t["anthropic"]["api_key"].as_str(), Some("sk-test-one"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[cfg(unix)]
    #[test]
    fn store_preserves_other_sections_and_comments() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        std::fs::write(
            &path,
            "# keep this comment\n[jira]\ntoken = \"j\"\n\n[anthropic]\napi_key = \"old\"\nextra = 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        store_key(&path, "anthropic", "new").unwrap();
        store_key(&path, "openai-compatible", "oc").unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep this comment"));
        let t: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(t["jira"]["token"].as_str(), Some("j"));
        assert_eq!(t["anthropic"]["api_key"].as_str(), Some("new"));
        assert_eq!(t["anthropic"]["extra"].as_integer(), Some(1));
        assert_eq!(t["openai-compatible"]["api_key"].as_str(), Some("oc"));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn store_refuses_group_readable_file_and_leaves_it_untouched() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        std::fs::write(&path, "[jira]\ntoken = \"j\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let err = store_key(&path, "anthropic", "sk-nope").unwrap_err();
        assert!(matches!(err, AuthError::Permissive { .. }));
        assert!(err.to_string().contains("chmod 600"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[jira]\ntoken = \"j\"\n"
        );
    }

    #[test]
    fn store_rejects_empty_and_multiline_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        assert!(matches!(
            store_key(&path, "anthropic", "  \n"),
            Err(AuthError::Empty)
        ));
        assert!(matches!(
            store_key(&path, "anthropic", "a\nb"),
            Err(AuthError::Multiline)
        ));
        assert!(!path.exists());
    }

    #[test]
    fn parse_error_does_not_quote_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        std::fs::write(&path, "[anthropic\napi_key = \"sk-leaky-value\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let err = store_key(&path, "anthropic", "x").unwrap_err();
        assert!(!err.to_string().contains("sk-leaky-value"));
    }

    #[test]
    fn status_lists_names_and_sources_never_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        let key = "sk-test-SECRETVALUE";
        store_key(&path, "openrouter", key).unwrap();
        let env = |k: &str| (k == "ANTHROPIC_API_KEY").then(|| "sk-env-SECRETVALUE".to_string());

        let out = render_status(&path, &env);
        assert!(!out.contains("SECRETVALUE"), "{out}");
        assert!(!out.contains(&key.len().to_string()), "{out}");
        assert!(
            out.contains("anthropic          env ANTHROPIC_API_KEY"),
            "{out}"
        );
        assert!(out.contains("openrouter         secrets.toml"), "{out}");
        assert!(out.contains("litellm            not set"), "{out}");
        assert!(out.contains("openai-compatible  not set"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn status_warns_and_ignores_permissive_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.toml");
        std::fs::write(&path, "[anthropic]\napi_key = \"sk-x\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let out = render_status(&path, &no_env);
        assert!(out.starts_with("warning:"), "{out}");
        assert!(out.contains("anthropic          not set"), "{out}");
    }
}
