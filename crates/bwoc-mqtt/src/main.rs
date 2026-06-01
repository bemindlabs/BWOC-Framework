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
        /// Broker URL, e.g. `mqtt://host:1883`.
        #[arg(long)]
        broker: String,
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
        /// Broker URL, e.g. `mqtt://host:1883`.
        #[arg(long)]
        broker: String,
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

fn run(cli: Cli) -> Result<(), MqttError> {
    match cli.cmd {
        Cmd::Publish {
            broker,
            topic,
            payload,
            client_id,
        } => {
            let broker: Broker = parse_broker(&broker)?;
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
            let broker = parse_broker(&broker)?;
            serve(&broker, &workspace, &topic, &client_id, |line| {
                println!("{line}");
            })
        }
    }
}
