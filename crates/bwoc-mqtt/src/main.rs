//! `bwoc-mqtt` — MQTT transport CLI for BWOC inter-workspace routing.
//!
//!   bwoc-mqtt publish --broker mqtt://host:1883 --topic bwoc/agent-neo/inbox [--payload '<json>']
//!   bwoc-mqtt serve   --broker mqtt://host:1883 --workspace /path/to/ws [--topic 'bwoc/+/inbox']
//!
//! `publish` reads the envelope from `--payload` or stdin. `serve` subscribes
//! and appends each received envelope to the matching agent's `inbox.jsonl`.

// `MqttError` wraps rumqttc's large error types — see the note in lib.rs.
#![allow(clippy::result_large_err)]

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use bwoc_mqtt::{Broker, MqttError, parse_broker, publish, serve};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bwoc-mqtt", about = "MQTT transport for BWOC routing")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish one envelope (JSON) to a broker topic.
    Publish {
        /// Broker URL, e.g. `mqtt://[user:pass@]host:1883`. Optional — falls back
        /// to `BWOC_MQTT_BROKER_FILE` (a file holding the url) then `BWOC_MQTT_BROKER`,
        /// keeping credentials out of argv for daemons.
        #[arg(long)]
        broker: Option<String>,
        /// Topic to publish to (e.g. `bwoc/agent-neo/inbox`).
        #[arg(long)]
        topic: String,
        /// Envelope JSON. If omitted, read from stdin.
        #[arg(long)]
        payload: Option<String>,
        /// MQTT client id.
        #[arg(long, default_value = "bwoc-mqtt-pub")]
        client_id: String,
    },
    /// Subscribe and deliver received envelopes into agent inboxes.
    Serve {
        /// Broker URL, e.g. `mqtt://[user:pass@]host:1883`. Optional — falls back
        /// to `BWOC_MQTT_BROKER_FILE` (a file holding the url) then `BWOC_MQTT_BROKER`,
        /// keeping credentials out of argv for daemons.
        #[arg(long)]
        broker: Option<String>,
        /// Workspace root holding `.bwoc/agents.toml` (recipient registry).
        #[arg(long)]
        workspace: PathBuf,
        /// Subscription topic filter. Defaults to every agent's inbox.
        #[arg(long, default_value = "bwoc/+/inbox")]
        topic: String,
        /// MQTT client id.
        #[arg(long, default_value = "bwoc-mqtt-serve")]
        client_id: String,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bwoc-mqtt error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Resolve the broker URL from the flag, else `BWOC_MQTT_BROKER_FILE` (read the
/// file), else `BWOC_MQTT_BROKER`. Letting a daemon source the url from a file
/// or env keeps credentials out of the process argv (visible in `ps`).
fn resolve_broker(flag: Option<String>) -> Result<String, MqttError> {
    resolve_broker_from(
        flag,
        std::env::var("BWOC_MQTT_BROKER_FILE").ok(),
        std::env::var("BWOC_MQTT_BROKER").ok(),
    )
}

/// Pure precedence core (env values passed in, so it is deterministic to test):
/// `flag` > read(`file`) > `env`. Empty strings are treated as unset.
fn resolve_broker_from(
    flag: Option<String>,
    file: Option<String>,
    env: Option<String>,
) -> Result<String, MqttError> {
    if let Some(b) = flag.filter(|s| !s.is_empty()) {
        return Ok(b);
    }
    if let Some(path) = file.filter(|s| !s.is_empty()) {
        let url = std::fs::read_to_string(&path)?.trim().to_string();
        if !url.is_empty() {
            return Ok(url);
        }
    }
    if let Some(url) = env.filter(|s| !s.is_empty()) {
        return Ok(url);
    }
    Err(MqttError::MissingBroker)
}

fn run(cli: Cli) -> Result<(), MqttError> {
    match cli.cmd {
        Cmd::Publish {
            broker,
            topic,
            payload,
            client_id,
        } => {
            let broker: Broker = parse_broker(&resolve_broker(broker)?)?;
            let payload = match payload {
                Some(p) => p,
                None => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
            };
            let payload = payload.trim();
            if payload.is_empty() {
                eprintln!("bwoc-mqtt: empty payload");
                return Ok(());
            }
            publish(&broker, &topic, payload, &client_id)?;
            eprintln!("published to `{topic}`");
            Ok(())
        }
        Cmd::Serve {
            broker,
            workspace,
            topic,
            client_id,
        } => {
            let broker = parse_broker(&resolve_broker(broker)?)?;
            serve(&broker, &workspace, &topic, &client_id, |line| {
                println!("{line}");
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_wins_over_file_and_env() {
        let r = resolve_broker_from(
            Some("mqtt://flag:1883".into()),
            Some("/nope".into()),
            Some("mqtt://env:1883".into()),
        );
        assert_eq!(r.unwrap(), "mqtt://flag:1883");
    }

    #[test]
    fn file_read_when_no_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broker");
        std::fs::write(&path, "  mqtt://bwoc:p@host:1883\n").unwrap();
        let r = resolve_broker_from(
            None,
            Some(path.to_string_lossy().into_owned()),
            Some("mqtt://env:1883".into()),
        );
        // file beats env; whitespace trimmed
        assert_eq!(r.unwrap(), "mqtt://bwoc:p@host:1883");
    }

    #[test]
    fn env_fallback_when_no_flag_or_file() {
        let r = resolve_broker_from(None, None, Some("mqtt://env:1883".into()));
        assert_eq!(r.unwrap(), "mqtt://env:1883");
    }

    #[test]
    fn empty_strings_are_unset() {
        // empty flag + empty env, empty file path → MissingBroker
        let r = resolve_broker_from(
            Some(String::new()),
            Some(String::new()),
            Some(String::new()),
        );
        assert!(matches!(r, Err(MqttError::MissingBroker)));
    }

    #[test]
    fn none_everywhere_errors() {
        assert!(matches!(
            resolve_broker_from(None, None, None),
            Err(MqttError::MissingBroker)
        ));
    }
}
