//! `bwoc-connect` — run a chat connector that bridges an external platform to a
//! BWOC agent: `bwoc-connect <telegram|discord> --agent <dir>`.
//!
//! Args are hand-parsed (no `clap` — this crate stays minimal; its weight is
//! the network stack, not the CLI). Token resolution is **env-first** — the
//! platform env var (`TELEGRAM_BOT_TOKEN` / `DISCORD_BOT_TOKEN`) wins because it
//! is explicit and can never block — with the **OS keyring**
//! (`bwoc/<platform>` · agent-dir basename, macOS/Windows) as a timeout-bounded
//! fallback. Linux is env-only (no Secret Service dep). See `resolve_token`.

use std::path::PathBuf;

use bwoc_connect::discord::DiscordTransport;
use bwoc_connect::session::HarnessSessionFactory;
use bwoc_connect::telegram::TelegramTransport;
use bwoc_connect::{ConnectError, ConnectorConfig, GroupBridge, Transport, run_bridge};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("bwoc-connect: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ConnectError> {
    let args: Vec<String> = std::env::args().collect();

    // platform → token env var (`None` for iMessage — it drives the local
    // Messages.app, there is no bot token).
    let (platform, token_env): (&str, Option<&str>) = match args.get(1).map(String::as_str) {
        Some("telegram") => ("telegram", Some("TELEGRAM_BOT_TOKEN")),
        Some("discord") => ("discord", Some("DISCORD_BOT_TOKEN")),
        Some("line") => ("line", Some("LINE_CHANNEL_ACCESS_TOKEN")),
        Some("imessage") => ("imessage", None),
        Some(other) => {
            return Err(ConnectError::Config(format!(
                "unknown connector '{other}' (supported: telegram, discord, line, imessage)"
            )));
        }
        None => {
            eprintln!(
                "usage: bwoc-connect <telegram|discord|line|imessage> --agent <dir> [--max-polls N]"
            );
            return Err(ConnectError::Config("missing connector".into()));
        }
    };

    let agent_dir = flag(&args, "--agent")
        .map(PathBuf::from)
        .ok_or_else(|| ConnectError::Config("missing --agent <dir>".into()))?;
    // `--max-polls N` bounds the loop (manual smoke testing); absent = forever.
    let max_polls = flag(&args, "--max-polls").and_then(|s| s.parse::<usize>().ok());

    // Config: <agent>/connectors/<platform>.toml
    let cfg_path = agent_dir
        .join("connectors")
        .join(format!("{platform}.toml"));
    let raw = std::fs::read_to_string(&cfg_path)
        .map_err(|e| ConnectError::Config(format!("read {}: {e}", cfg_path.display())))?;
    let mut cfg = ConnectorConfig::parse(&raw)?;
    if !cfg.enabled {
        return Err(ConnectError::Config(format!(
            "connector disabled (set enabled = true in {})",
            cfg_path.display()
        )));
    }
    // LINE ids are strings — fold its `[line].allow_user_ids` into the numeric
    // `allow_from` (same hash the transport applies) so the shared allow-list
    // gate works unchanged.
    if platform == "line" {
        let line_cfg = cfg.line.as_ref().ok_or_else(|| {
            ConnectError::Config(format!(
                "line connector needs a [line] block in {}",
                cfg_path.display()
            ))
        })?;
        cfg.allow_from = line_cfg
            .allow_user_ids
            .iter()
            .map(|u| bwoc_connect::line::hash_id(u))
            .collect();
    }
    // iMessage handles are strings too — fold `[imessage].allow_handles` into the
    // numeric `allow_from` with the same hash the transport applies (#229).
    if platform == "imessage" {
        let im_cfg = cfg.imessage.as_ref().ok_or_else(|| {
            ConnectError::Config(format!(
                "imessage connector needs an [imessage] block in {}",
                cfg_path.display()
            ))
        })?;
        cfg.allow_from = im_cfg
            .allow_handles
            .iter()
            .map(|h| bwoc_connect::imessage::hash_id(h))
            .collect();
    }
    if cfg.allow_from.is_empty() {
        eprintln!(
            "[bwoc-connect] warning: allow_from is empty — the bridge will ignore \
             everyone (closed by default). Add platform user ids to {}.",
            cfg_path.display()
        );
    }

    // Token platforms resolve a bot token; iMessage has none (drives local
    // Messages.app), so `token_env` is `None` and we skip resolution.
    let token = match token_env {
        Some(env) => Some(resolve_token(platform, &agent_dir, env)?),
        None => None,
    };
    // Build the platform transport. Telegram resolves its @username up front
    // (validates the token + enables group mention-gating); Discord connects
    // its gateway. iMessage opens the local chat.db. All expose the same
    // `Transport`.
    let transport: Box<dyn Transport> = match platform {
        "telegram" => {
            let mut t = TelegramTransport::new(token.as_deref().expect("telegram token"))?;
            t.resolve_identity().await?;
            Box::new(t)
        }
        "discord" => {
            Box::new(DiscordTransport::connect(token.as_deref().expect("discord token")).await?)
        }
        "imessage" => {
            #[cfg(target_os = "macos")]
            {
                let im = cfg
                    .imessage
                    .as_ref()
                    .expect("imessage block validated above");
                let db = expand_tilde(&im.db_path);
                Box::new(bwoc_connect::imessage::ImessageTransport::open(
                    &db,
                    im.poll_interval_secs,
                )?)
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err(ConnectError::Config(
                    "the imessage connector is macOS-only — it drives the local Messages.app via \
                     osascript + reads ~/Library/Messages/chat.db; there is no server iMessage API"
                        .into(),
                ));
            }
        }
        "line" => {
            // The channel secret (webhook signature) is separate from the access
            // token; env-only (it's a server credential, like the headless path).
            let secret = std::env::var("LINE_CHANNEL_SECRET")
                .map_err(|_| ConnectError::NoToken("LINE_CHANNEL_SECRET".into()))?;
            let lc = cfg.line.as_ref().expect("line block validated above");
            Box::new(
                bwoc_connect::line::LineTransport::start(
                    token.as_deref().expect("line token"),
                    &secret,
                    &lc.bind,
                    &lc.path,
                )
                .await?,
            )
        }
        _ => unreachable!("platform validated above"),
    };
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
    // Short platform tag for logged-peer `from` fields (tg:/dc:/ln:<id>).
    let peer_prefix = match platform {
        "discord" => "dc",
        "line" => "ln",
        "imessage" => "im",
        _ => "tg",
    };
    let group_bridge = match (group_factory.as_ref(), group_chat_log.as_ref()) {
        (Some(f), Some(log)) => Some(GroupBridge {
            factory: f,
            chat_log: log.clone(),
            peer_prefix: peer_prefix.to_string(),
        }),
        _ => None,
    };

    eprintln!(
        "[bwoc-connect] {platform} bridge up for agent {} (allow_from: {:?}{})",
        agent_dir.display(),
        cfg.allow_from,
        group_chat_log
            .as_ref()
            .map(|p| format!(", team-chat: {}", p.display()))
            .unwrap_or_default()
    );
    run_bridge(
        transport.as_ref(),
        &dm_factory,
        group_bridge,
        &cfg,
        max_polls,
    )
    .await
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

/// Resolve the bot token: **env var first** (every platform), **OS keyring
/// fallback** (macOS/Windows; service `bwoc/<platform>`, account = the agent
/// dir's basename).
///
/// Env wins because it is explicit and — unlike the OS keyring — can never
/// block. A keyring read from a daemon-spawned subprocess (e.g. the connector
/// child of `bwoc-agent --serve`) has no interactive session to answer a
/// Keychain authorization prompt, so it can hang indefinitely; that wedged the
/// poll loop before it ever called `getUpdates` (#305). Checking env first means
/// the documented `TELEGRAM_BOT_TOKEN` path never touches the keychain, and the
/// keyring read itself is now timeout-bounded as a backstop.
fn resolve_token(
    platform: &str,
    agent_dir: &std::path::Path,
    token_env: &str,
) -> Result<String, ConnectError> {
    // Trim — a token stored with a trailing newline (common from `echo`/paste)
    // would otherwise corrupt the Bot auth header.
    if let Ok(tok) = std::env::var(token_env) {
        if !tok.trim().is_empty() {
            eprintln!("[bwoc-connect] token: env {token_env}");
            return Ok(tok.trim().to_string());
        }
    }
    let account = agent_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agent".to_string());
    let service = format!("bwoc/{platform}");
    if let Some(tok) = keyring_lookup(&service, &account) {
        eprintln!("[bwoc-connect] token: keyring {service}·{account}");
        return Ok(tok.trim().to_string());
    }
    Err(ConnectError::NoToken(format!(
        "{token_env} (or, on macOS/Windows, keyring entry {service}·{account})"
    )))
}

/// Non-empty token from the OS keyring, or `None`. macOS/Windows query the
/// native store; Linux has no keyring backend (env-only — see Cargo.toml).
///
/// The native read runs on a worker thread with a short timeout: a Keychain call
/// from a non-interactive (daemon-spawned) process can block on an authorization
/// prompt no one can answer (#305), so a hung read degrades to `None` — falling
/// through to the env error instead of wedging the connector forever.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn keyring_lookup(service: &str, account: &str) -> Option<String> {
    use std::sync::mpsc;
    use std::time::Duration;
    let (tx, rx) = mpsc::channel();
    let (s, a) = (service.to_string(), account.to_string());
    // Detached on purpose: if it hangs on a Keychain prompt we abandon it (the
    // process exits cleanly regardless). Bind the handle so it isn't a silent drop.
    let _worker = std::thread::spawn(move || {
        let tok = keyring::Entry::new(&s, &a)
            .ok()
            .and_then(|e| e.get_password().ok())
            .filter(|t| !t.trim().is_empty());
        let _ = tx.send(tok); // receiver may have timed out and gone — ignore.
    });
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(tok) => tok,
        Err(_) => {
            eprintln!(
                "[bwoc-connect] warning: keyring read timed out (no interactive session to \
                 authorize it?); set the bot token via env to avoid the keychain."
            );
            None
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keyring_lookup(_service: &str, _account: &str) -> Option<String> {
    None
}

/// A team id safe to use as a single filesystem path segment.
fn valid_team_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Expand a leading `~/` against `$HOME` (the iMessage `chat.db` default lives
/// under the user's home). Leaves the path unchanged if there's no leading `~/`
/// or `HOME` is unset. macOS-only (the only place the iMessage path is built).
#[cfg(target_os = "macos")]
fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::Path::new(&home).join(rest);
        }
    }
    std::path::PathBuf::from(path)
}

/// Value of `--flag <value>` from argv, or `None`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
