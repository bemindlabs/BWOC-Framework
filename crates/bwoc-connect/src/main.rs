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
use bwoc_connect::{ConnectError, TelegramConfig, run_bridge};

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
    let transport = TelegramTransport::new(&token)?;
    let factory = HarnessSessionFactory::new(&agent_dir)?;

    eprintln!(
        "[bwoc-connect] telegram bridge up for agent {} (allow_from: {:?})",
        agent_dir.display(),
        cfg.allow_from
    );
    run_bridge(&transport, &factory, &cfg, max_polls).await
}

/// Value of `--flag <value>` from argv, or `None`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
