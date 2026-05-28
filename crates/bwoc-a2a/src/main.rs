//! `bwoc-a2a` binary — the A2A transport runner (#48 P1-serve).
//!
//! Split from `bwoc-cli` on purpose: this binary links the HTTP stack
//! (axum/tokio), which the CLI must not (the dep-quarantine invariant in
//! `bwoc-cli/src/spawn.rs`). `bwoc a2a card`/`serve` in the CLI resolve this
//! binary as a sibling and exec it — exactly how `bwoc spawn` runs the Ollama
//! `bwoc-harness` sibling.
//!
//! - `bwoc-a2a card <agent>`  — print the agent's Agent Card JSON.
//! - `bwoc-a2a serve <agent>` — run the listener (loopback-only by default).

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;
use clap::{Parser, Subcommand};

use bwoc_a2a::card::card_from_manifest;
use bwoc_a2a::serve::{ServeConfig, serve_blocking};

#[derive(Parser, Debug)]
#[command(
    name = "bwoc-a2a",
    about = "A2A protocol transport runner for BWOC agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print the agent's A2A Agent Card (JSON) derived from its manifest.
    Card {
        /// Agent name or id (the `agent-` prefix is optional).
        agent: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Advertised endpoint URL. Defaults to `http://<bind>:<port>/`.
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value = "127.0.0.1")]
        bind: IpAddr,
        #[arg(long, default_value_t = 41241)]
        port: u16,
    },
    /// Run the A2A HTTP listener: Agent Card at the well-known path + a
    /// JSON-RPC endpoint that drops inbound messages into the agent's inbox.
    Serve {
        /// Agent name or id (the `agent-` prefix is optional).
        agent: String,
        #[arg(long = "workspace")]
        workspace: Option<PathBuf>,
        /// Address to bind. Defaults to loopback (`127.0.0.1`). A non-loopback
        /// bind requires an auth token (`BWOC_A2A_TOKEN` / `.bwoc/a2a.token`)
        /// or `--allow-unauthenticated`, otherwise the listener refuses to start.
        #[arg(long, default_value = "127.0.0.1")]
        bind: IpAddr,
        #[arg(long, default_value_t = 41241)]
        port: u16,
        /// Expose this team's shared task list over A2A `tasks/*` (`GetTask`/
        /// `ListTasks`). Resolves `.bwoc/teams/<team>/tasks.jsonl`.
        #[arg(long)]
        team: Option<String>,
        /// Permit a non-loopback bind with NO auth token — the listener serves
        /// with a loud warning instead of refusing. For trusted networks or a
        /// front proxy that adds auth; never expose an unauthenticated listener
        /// to an untrusted network.
        #[arg(long)]
        allow_unauthenticated: bool,
    },
    /// Fetch and print an external agent's A2A Agent Card.
    FetchCard {
        /// Base URL of the remote A2A agent (the well-known path is appended).
        url: String,
    },
    /// Send a text message to an external A2A agent via `SendMessage`, printing
    /// the JSON-RPC result (a Task or Message).
    Send {
        /// The remote agent's JSON-RPC endpoint URL.
        url: String,
        /// Message text to send.
        message: String,
        /// Optional A2A `contextId` to associate the message with.
        #[arg(long)]
        context: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Card {
            agent,
            workspace,
            url,
            bind,
            port,
        } => run_card(&agent, workspace, url, bind, port),
        Command::Serve {
            agent,
            workspace,
            bind,
            port,
            team,
            allow_unauthenticated,
        } => run_serve(&agent, workspace, bind, port, team, allow_unauthenticated),
        Command::FetchCard { url } => run_fetch_card(&url),
        Command::Send {
            url,
            message,
            context,
        } => run_send(&url, &message, context.as_deref()),
    };
    ExitCode::from(code)
}

/// Run an async client call to completion on a one-off current-thread runtime,
/// so the binary's command handlers stay synchronous.
fn block_on<F: std::future::Future>(fut: F) -> std::io::Result<F::Output> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(fut))
}

fn run_fetch_card(url: &str) -> u8 {
    let result = match block_on(bwoc_a2a::client::fetch_card(url)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc-a2a fetch-card: runtime error: {e}");
            return 1;
        }
    };
    match result {
        Ok(card) => match serde_json::to_string_pretty(&card) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("bwoc-a2a fetch-card: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("bwoc-a2a fetch-card: {e}");
            1
        }
    }
}

fn run_send(url: &str, message: &str, context: Option<&str>) -> u8 {
    let message_id = format!(
        "bwoc-a2a-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let result = match block_on(bwoc_a2a::client::send_message(
        url,
        message,
        context,
        &message_id,
    )) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc-a2a send: runtime error: {e}");
            return 1;
        }
    };
    match result {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("bwoc-a2a send: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("bwoc-a2a send: {e}");
            1
        }
    }
}

fn run_card(
    agent: &str,
    workspace: Option<PathBuf>,
    url: Option<String>,
    bind: IpAddr,
    port: u16,
) -> u8 {
    let (manifest, _, _) = match resolve_agent(agent, workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let url = url.unwrap_or_else(|| format!("http://{}/", SocketAddr::new(bind, port)));
    let card = card_from_manifest(&manifest, &url);
    match serde_json::to_string_pretty(&card) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => {
            eprintln!("bwoc-a2a card: failed to serialize card: {e}");
            1
        }
    }
}

fn run_serve(
    agent: &str,
    workspace: Option<PathBuf>,
    bind: IpAddr,
    port: u16,
    team: Option<String>,
    allow_unauthenticated: bool,
) -> u8 {
    // Reject a team id that could escape `.bwoc/teams/` — defence in depth even
    // though the id is operator-supplied (it still ends up in a path join).
    if let Some(id) = &team {
        if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
            eprintln!(
                "bwoc-a2a serve: invalid --team '{id}' — a team id must be a single \
                 path segment (no '/', '\\', or '..')."
            );
            return 2;
        }
    }
    let (manifest, inbox_path, workspace_root) = match resolve_agent(agent, workspace) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let team = team.map(|id| {
        let path = workspace_root.join(format!(".bwoc/teams/{id}/tasks.jsonl"));
        (id, path)
    });
    let addr = SocketAddr::new(bind, port);
    // AP1: a Bearer token from `BWOC_A2A_TOKEN` (env, wins) or the agent's
    // `.bwoc/a2a.token` file enables auth on the JSON-RPC + SSE endpoints.
    let auth_token = match normalize_token(std::env::var("BWOC_A2A_TOKEN").ok()) {
        Some(t) => Some(t),
        None => match inbox_path.parent().map(|p| p.join("a2a.token")) {
            Some(path) => match read_token_file(&path) {
                Ok(tok) => tok,
                Err(msg) => {
                    eprintln!("bwoc-a2a serve: {msg}");
                    return 1;
                }
            },
            None => None,
        },
    };
    // AP2: a non-loopback bind with no auth is refused by default; serving it
    // open is a deliberate opt-in (`--allow-unauthenticated`, loud warning).
    match bind_policy(
        bind.is_loopback(),
        auth_token.is_some(),
        allow_unauthenticated,
    ) {
        BindPolicy::Serve => {}
        BindPolicy::Warn => {
            eprintln!(
                "bwoc-a2a serve: WARNING — binding {addr} is NOT loopback and NO \
                 auth token is set; serving anyway because --allow-unauthenticated \
                 was passed, so anyone who can reach this address can write to the \
                 agent's inbox."
            );
        }
        BindPolicy::Refuse => {
            eprintln!(
                "bwoc-a2a serve: refusing non-loopback bind {addr} with no auth \
                 token — anyone who can reach it could write to the agent's inbox. \
                 Set BWOC_A2A_TOKEN (or .bwoc/a2a.token), bind 127.0.0.1, or pass \
                 --allow-unauthenticated to override."
            );
            return 2;
        }
    }
    let mut card = card_from_manifest(&manifest, &format!("http://{addr}/"));
    if auth_token.is_some() {
        card = card.with_bearer_security();
    }
    let agent_id = manifest.agent_id.clone();
    println!(
        "bwoc-a2a serve: agent '{agent_id}' on http://{addr}/ (Agent Card at \
         http://{addr}/.well-known/agent-card.json) — auth {}. Ctrl-C to stop.",
        if auth_token.is_some() {
            "ON (Bearer)"
        } else {
            "OFF"
        }
    );
    match serve_blocking(ServeConfig {
        agent_id,
        inbox_path,
        card,
        addr,
        team,
        auth_token,
    }) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bwoc-a2a serve: listener error on {addr}: {e}");
            1
        }
    }
}

/// Resolve an agent's manifest + inbox path + the workspace root from the
/// registry. `Err(code)` carries the process exit code after printing to stderr.
fn resolve_agent(
    agent: &str,
    workspace: Option<PathBuf>,
) -> Result<(Manifest, PathBuf, PathBuf), u8> {
    let Some(workspace) = resolve_workspace(workspace) else {
        eprintln!(
            "bwoc-a2a: no workspace found. Pass --workspace, set BWOC_WORKSPACE, \
             or run `bwoc init`."
        );
        return Err(2);
    };
    let registry = AgentsRegistry::load(&workspace).map_err(|e| {
        eprintln!("bwoc-a2a: failed to read agents.toml: {e}");
        1u8
    })?;
    let lookup_id = if agent.starts_with("agent-") {
        agent.to_string()
    } else {
        format!("agent-{agent}")
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc-a2a: no agent named '{agent}' in workspace {}. Try `bwoc list`.",
            workspace.display()
        );
        return Err(2);
    };
    let agent_dir = workspace.join(&entry.path);
    let manifest =
        Manifest::load_from_path(&agent_dir.join("config.manifest.json")).map_err(|e| {
            eprintln!("bwoc-a2a: failed to read manifest for '{agent}': {e}");
            1u8
        })?;
    let inbox = agent_dir.join(".bwoc/inbox.jsonl");
    Ok((manifest, inbox, workspace))
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

/// Normalize a raw token source (env value or file contents): trim surrounding
/// whitespace, and treat empty / whitespace-only as **absent** — so an empty
/// `BWOC_A2A_TOKEN` or `.bwoc/a2a.token` never enables auth-with-an-empty-token
/// (which would accept `Authorization: Bearer ` from anyone).
fn normalize_token(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// What to do for a `bind` + auth combination (AP2). Loopback or auth-on ⇒
/// serve silently. Non-loopback with no auth ⇒ refuse, unless the operator
/// passed `--allow-unauthenticated`, which downgrades the refusal to a loud
/// warning (the pre-AP2 escape hatch for trusted-proxy / LAN-test setups).
#[derive(Debug, PartialEq, Eq)]
enum BindPolicy {
    Serve,
    Warn,
    Refuse,
}

fn bind_policy(is_loopback: bool, has_auth: bool, allow_unauthenticated: bool) -> BindPolicy {
    if is_loopback || has_auth {
        BindPolicy::Serve
    } else if allow_unauthenticated {
        BindPolicy::Warn
    } else {
        BindPolicy::Refuse
    }
}

/// Read the agent's `.bwoc/a2a.token`. A missing file ⇒ `Ok(None)` (auth stays
/// off). On Unix the file must not be group/world-accessible (`mode & 0o077 ==
/// 0`, i.e. `0600` or stricter); a laxer file is **refused** with `Err` rather
/// than silently trusted — another local user could read the bearer secret
/// (issue #80 mandates `0600`). `BWOC_A2A_TOKEN` supplies the token without a
/// file, so it is the override when the file's perms can't be tightened.
fn read_token_file(path: &Path) -> Result<Option<String>, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read token file {}: {e}", path.display())),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|e| format!("cannot stat token file {}: {e}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(format!(
                "token file {} is group/world-accessible (mode {:04o}); another \
                 local user could read the bearer secret. Run `chmod 600 {}` (or \
                 set BWOC_A2A_TOKEN instead).",
                path.display(),
                mode & 0o7777,
                path.display()
            ));
        }
    }
    Ok(normalize_token(Some(raw)))
}

#[cfg(test)]
mod tests {
    use super::{BindPolicy, bind_policy, normalize_token, read_token_file};

    #[test]
    fn bind_policy_covers_the_matrix() {
        // Loopback always serves silently, regardless of auth or the flag.
        assert_eq!(bind_policy(true, false, false), BindPolicy::Serve);
        assert_eq!(bind_policy(true, false, true), BindPolicy::Serve);
        assert_eq!(bind_policy(true, true, false), BindPolicy::Serve);
        // Non-loopback WITH auth serves silently (AP2: no more warning).
        assert_eq!(bind_policy(false, true, false), BindPolicy::Serve);
        assert_eq!(bind_policy(false, true, true), BindPolicy::Serve);
        // Non-loopback, no auth: refuse by default, warn only with the override.
        assert_eq!(bind_policy(false, false, false), BindPolicy::Refuse);
        assert_eq!(bind_policy(false, false, true), BindPolicy::Warn);
    }

    #[test]
    fn empty_or_whitespace_token_is_absent() {
        assert_eq!(normalize_token(None), None);
        assert_eq!(normalize_token(Some(String::new())), None);
        assert_eq!(normalize_token(Some("   \n\t ".into())), None);
    }

    #[test]
    fn real_token_is_trimmed_but_interior_preserved() {
        assert_eq!(
            normalize_token(Some("  s3cr3t\n".into())).as_deref(),
            Some("s3cr3t")
        );
        // Interior spaces survive (only the edges are trimmed).
        assert_eq!(normalize_token(Some("a b".into())).as_deref(), Some("a b"));
    }

    #[test]
    fn missing_token_file_is_absent_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a2a.token");
        assert_eq!(read_token_file(&path), Ok(None));
    }

    #[cfg(unix)]
    #[test]
    fn private_token_file_is_read() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a2a.token");
        std::fs::write(&path, "  s3cr3t\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_token_file(&path).unwrap().as_deref(), Some("s3cr3t"));
    }

    #[cfg(unix)]
    #[test]
    fn group_or_world_readable_token_file_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a2a.token");
        std::fs::write(&path, "s3cr3t").unwrap();
        for mode in [0o640, 0o644, 0o604, 0o660] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let err = read_token_file(&path).unwrap_err();
            assert!(
                err.contains("group/world-accessible"),
                "mode {mode:o}: {err}"
            );
        }
    }
}
