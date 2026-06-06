//! `bwoc-connect` — run a chat connector that bridges an external platform to a
//! BWOC agent. PR1: `bwoc-connect telegram --agent <dir>`.
//!
//! Args are hand-parsed (no `clap` — this crate stays minimal; its weight is
//! the network stack, not the CLI). Token resolution in PR1 is the
//! `TELEGRAM_BOT_TOKEN` env var (the documented headless-server path); keyring
//! resolution lands with the CredentialBroker wiring in PR3.

use std::path::PathBuf;

use bwoc_connect::session::HarnessSessionFactory;
use bwoc_connect::telegram::TelegramTransport;
use bwoc_connect::{ConnectError, GroupBridge, TelegramConfig, run_bridge};

const TOKEN_ENV: &str = "TELEGRAM_BOT_TOKEN";

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("bwoc-connect: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ConnectError> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("telegram") => {}
        Some(other) => {
            return Err(ConnectError::Config(format!(
                "unknown connector '{other}' (only 'telegram' in PR1)"
            )));
        }
        None => {
            eprintln!("usage: bwoc-connect telegram --agent <dir> [--max-polls N]");
            return Err(ConnectError::Config("missing connector".into()));
        }
    }

    let agent_dir = flag(&args, "--agent")
        .map(PathBuf::from)
        .ok_or_else(|| ConnectError::Config("missing --agent <dir>".into()))?;
    // `--max-polls N` bounds the loop (manual smoke testing); absent = forever.
    let max_polls = flag(&args, "--max-polls").and_then(|s| s.parse::<usize>().ok());

    // Config: <agent>/connectors/telegram.toml
    let cfg_path = agent_dir.join("connectors").join("telegram.toml");
    let raw = std::fs::read_to_string(&cfg_path)
        .map_err(|e| ConnectError::Config(format!("read {}: {e}", cfg_path.display())))?;
    let cfg = TelegramConfig::parse(&raw)?;
    if !cfg.enabled {
        return Err(ConnectError::Config(format!(
            "connector disabled (set enabled = true in {})",
            cfg_path.display()
        )));
    }
    if cfg.allow_from.is_empty() {
        eprintln!(
            "[bwoc-connect] warning: allow_from is empty — the bridge will ignore \
             everyone (closed by default). Add platform user ids to {}.",
            cfg_path.display()
        );
    }

    let token = std::env::var(TOKEN_ENV).map_err(|_| ConnectError::NoToken(TOKEN_ENV.into()))?;
    let mut transport = TelegramTransport::new(&token)?;
    // Resolve the bot's @username up front (validates the token early + needed
    // for group mention-gating).
    transport.resolve_identity().await?;
    let dm_factory = HarnessSessionFactory::new(&agent_dir)?;

    // Group binding (PR2): if `[group].team` is set, resolve the team's shared
    // chat.jsonl and build a --team-chat factory for group rooms.
    let group_chat_log = match cfg.group.as_ref().and_then(|g| g.team.as_deref()) {
        Some(team) => {
            // Defence in depth: the team id becomes a path segment, so reject
            // anything that isn't a plain kebab/snake token (no `/`, `..`, etc.).
            if !valid_team_id(team) {
                return Err(ConnectError::Config(format!(
                    "invalid team id '{team}' (allowed: letters, digits, '-', '_')"
                )));
            }
            Some(team_chat_path(&agent_dir, team))
        }
        None => None,
    };
    let group_factory = match &group_chat_log {
        Some(log) => Some(HarnessSessionFactory::new(&agent_dir)?.with_team_chat(log.clone())),
        None => None,
    };
    let group_bridge = match (group_factory.as_ref(), group_chat_log.as_ref()) {
        (Some(f), Some(log)) => Some(GroupBridge {
            factory: f,
            chat_log: log.clone(),
        }),
        _ => None,
    };

    eprintln!(
        "[bwoc-connect] telegram bridge up for agent {} (allow_from: {:?}{})",
        agent_dir.display(),
        cfg.allow_from,
        group_chat_log
            .as_ref()
            .map(|p| format!(", team-chat: {}", p.display()))
            .unwrap_or_default()
    );
    run_bridge(&transport, &dm_factory, group_bridge, &cfg, max_polls).await
}

/// Resolve a team's shared chat log: walk up from the agent dir to the
/// workspace root (`.bwoc/workspace.toml`) and join
/// `.bwoc/teams/<team>/chat.jsonl`. Falls back to the agent dir's parent if no
/// workspace marker is found (best-effort; the harness creates the file).
fn team_chat_path(agent_dir: &std::path::Path, team: &str) -> PathBuf {
    let mut cur = agent_dir;
    let workspace = loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            break cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => break agent_dir.parent().unwrap_or(agent_dir).to_path_buf(),
        }
    };
    workspace
        .join(".bwoc")
        .join("teams")
        .join(team)
        .join("chat.jsonl")
}

/// A team id safe to use as a single filesystem path segment.
fn valid_team_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Value of `--flag <value>` from argv, or `None`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
