//! MQTT transport for BWOC inter-workspace routing.
//!
//! `bwoc-core`'s routing table can mark a peer route as `transport = "mqtt"`
//! (see [`bwoc_core::routing::RouteTarget::Mqtt`]). The actual broker I/O lives
//! here — quarantined into its own crate so `bwoc-core`/`bwoc-cli` never link an
//! MQTT client. The wire payload is exactly the JSON envelope `bwoc send`
//! appends to a local `inbox.jsonl`, so MQTT delivery and local-FS delivery
//! produce identical inbox lines.
//!
//! Two roles:
//! - **publish** — the sender pushes one envelope to `broker`/`topic`.
//! - **serve** — the peer subscribes (default `bwoc/+/inbox`), and for each
//!   message resolves the recipient's agent from the local `AgentsRegistry` and
//!   appends the envelope to that agent's `inbox.jsonl` — exactly what a local
//!   `bwoc send` would have written.
//!
//! The pure helpers ([`topic_for`], [`recipient_from_envelope`],
//! [`inbox_path`], [`parse_broker`]) are unit-tested without a broker; the
//! `publish`/`serve` functions are thin synchronous `rumqttc` drivers.

// `rumqttc`'s `ClientError`/`ConnectionError` are large, so any `Result<_,
// MqttError>` trips `result_large_err`. Boxing every `#[from]` adds noise for a
// transport crate whose errors are inherently the broker's — allow it crate-wide.
#![allow(clippy::result_large_err)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bwoc_core::workspace::AgentsRegistry;
use rumqttc::{Client, Event, Incoming, MqttOptions, QoS};

#[derive(Debug, thiserror::Error)]
pub enum MqttError {
    #[error("invalid broker url `{0}` (expected mqtt://host[:port])")]
    BadBroker(String),
    #[error("mqtt client error: {0}")]
    Client(#[from] rumqttc::ClientError),
    #[error("mqtt connection error: {0}")]
    Conn(#[from] rumqttc::ConnectionError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace error: {0}")]
    Workspace(#[from] bwoc_core::workspace::WorkspaceError),
    #[error("payload is not a JSON object with a string `to` field")]
    BadEnvelope,
    #[error(
        "no broker: pass --broker, or set BWOC_MQTT_BROKER_FILE (path to a file holding the url) or BWOC_MQTT_BROKER"
    )]
    MissingBroker,
}

/// A parsed broker address, with optional credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broker {
    pub host: String,
    pub port: u16,
    /// Username from `mqtt://user[:pass]@host` userinfo, if present.
    pub username: Option<String>,
    /// Password from `mqtt://user:pass@host` userinfo, if present.
    pub password: Option<String>,
}

/// Default MQTT port.
pub const DEFAULT_PORT: u16 = 1883;

/// Parse a `mqtt://[user[:pass]@]host[:port]` (or bare `host[:port]`) broker URL.
/// The scheme, if present, must be `mqtt`. Optional `user:pass@` userinfo sets
/// credentials (RFC 3986: the first `@` separates userinfo from host, so a literal
/// `@` in the password must be percent-encoded). Port defaults to [`DEFAULT_PORT`].
pub fn parse_broker(url: &str) -> Result<Broker, MqttError> {
    // Any parse failure echoes a *redacted* url — never the raw input, which may
    // carry `user:pass@` userinfo that must not leak into error output / logs.
    let bad = || MqttError::BadBroker(bwoc_core::routing::redact_broker(url));
    let rest = match url.split_once("://") {
        Some(("mqtt", r)) => r,
        Some(_) => return Err(bad()),
        None => url,
    };
    let rest = rest.trim_end_matches('/');
    // Split optional `userinfo@` (first `@`) from `host[:port]`.
    let (userinfo, hostport) = match rest.split_once('@') {
        Some((ui, hp)) => (Some(ui), hp),
        None => (None, rest),
    };
    // Userinfo is percent-decoded (RFC 3986): a literal `@`/`:` in the password
    // is encoded as `%40`/`%3A`, so `mqtt://user:p%40ss@host` yields `p@ss`.
    let (username, password) = match userinfo {
        Some(ui) if !ui.is_empty() => match ui.split_once(':') {
            Some((u, p)) => (Some(percent_decode(u)), Some(percent_decode(p))),
            None => (Some(percent_decode(ui)), None),
        },
        _ => (None, None),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| bad())?),
        None => (hostport, DEFAULT_PORT),
    };
    if host.is_empty() {
        return Err(bad());
    }
    Ok(Broker {
        host: host.to_string(),
        port,
        username,
        password,
    })
}

/// Minimal RFC 3986 percent-decoder for userinfo. Decodes `%XX` hex escapes to
/// bytes (UTF-8, lossy on invalid sequences); leaves a malformed/truncated `%`
/// untouched. No dependency — the only encoded chars expected here are the
/// userinfo delimiters (`@` → `%40`, `:` → `%3A`).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The topic for a recipient: the explicit `override`, else `bwoc/<id>/inbox`.
/// Mirrors [`bwoc_core::routing::RouteTarget::Mqtt`]'s default.
pub fn topic_for(recipient_id: &str, override_topic: Option<&str>) -> String {
    match override_topic {
        Some(t) => t.to_string(),
        None => format!("bwoc/{recipient_id}/inbox"),
    }
}

/// Extract the recipient id (`to`) from an envelope JSON string.
pub fn recipient_from_envelope(payload: &str) -> Result<String, MqttError> {
    let v: serde_json::Value = serde_json::from_str(payload).map_err(|_| MqttError::BadEnvelope)?;
    v.get("to")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(MqttError::BadEnvelope)
}

/// Resolve a recipient agent's `inbox.jsonl` under `workspace`, via the
/// `AgentsRegistry`. Returns `None` if the agent is unknown to this workspace.
pub fn inbox_path(
    workspace: &Path,
    registry: &AgentsRegistry,
    recipient_id: &str,
) -> Option<PathBuf> {
    let entry = registry.agents.iter().find(|a| a.id == recipient_id)?;
    Some(
        workspace
            .join(&entry.path)
            .join(".bwoc")
            .join("inbox.jsonl"),
    )
}

/// Append one envelope line (newline-terminated) to `inbox_path`, creating the
/// `.bwoc/` parent if needed. The line is the raw envelope JSON, exactly as a
/// local `bwoc send` writes it.
pub fn append_envelope(inbox_path: &Path, envelope_line: &str) -> Result<(), MqttError> {
    if let Some(dir) = inbox_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(inbox_path)?;
    writeln!(f, "{}", envelope_line.trim_end())?;
    Ok(())
}

fn mqtt_options(broker: &Broker, client_id: &str) -> MqttOptions {
    let mut opts = MqttOptions::new(client_id, &broker.host, broker.port);
    opts.set_keep_alive(Duration::from_secs(30));
    if let Some(username) = &broker.username {
        opts.set_credentials(username, broker.password.clone().unwrap_or_default());
    }
    opts
}

/// Publish one `payload` (an envelope JSON line) to `topic` on `broker`, QoS 1.
/// Blocks until the broker acks the publish, then disconnects.
pub fn publish(
    broker: &Broker,
    topic: &str,
    payload: &str,
    client_id: &str,
) -> Result<(), MqttError> {
    let (client, mut conn) = Client::new(mqtt_options(broker, client_id), 10);
    client.publish(topic, QoS::AtLeastOnce, false, payload.as_bytes())?;

    // Pump the event loop until the PubAck (QoS1) lands, then disconnect.
    for event in conn.iter() {
        match event? {
            Event::Incoming(Incoming::PubAck(_)) => {
                client.disconnect()?;
                return Ok(());
            }
            Event::Incoming(Incoming::Disconnect) => break,
            _ => {}
        }
    }
    Ok(())
}

/// What a served message resulted in — returned by [`deliver`] so a caller (and
/// tests) can observe routing without a live broker.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Appended to the named agent's inbox.
    Delivered(String),
    /// The envelope named a recipient this workspace does not host.
    UnknownRecipient(String),
}

/// Resolve + deliver one received `payload` into the right inbox under
/// `workspace`. Pure except for the final file append — the broker loop in
/// [`serve`] calls this per message, and tests call it directly.
pub fn deliver(workspace: &Path, payload: &str) -> Result<Delivery, MqttError> {
    let recipient = recipient_from_envelope(payload)?;
    let registry = AgentsRegistry::load(workspace)?;
    match inbox_path(workspace, &registry, &recipient) {
        Some(path) => {
            append_envelope(&path, payload)?;
            Ok(Delivery::Delivered(recipient))
        }
        None => Ok(Delivery::UnknownRecipient(recipient)),
    }
}

/// Subscribe to `topic` (default `bwoc/+/inbox`) on `broker` and deliver every
/// received envelope into the matching agent's inbox under `workspace`. Runs
/// until the connection errors or the process is killed. `on_event` is called
/// with a human-readable line per message (for logging).
pub fn serve(
    broker: &Broker,
    workspace: &Path,
    topic: &str,
    client_id: &str,
    mut on_event: impl FnMut(String),
) -> Result<(), MqttError> {
    let (client, mut conn) = Client::new(mqtt_options(broker, client_id), 64);
    client.subscribe(topic, QoS::AtLeastOnce)?;
    on_event(format!(
        "subscribed to `{topic}` on {}:{} — delivering into {}",
        broker.host,
        broker.port,
        workspace.display()
    ));

    for event in conn.iter() {
        if let Event::Incoming(Incoming::Publish(p)) = event? {
            let payload = String::from_utf8_lossy(&p.payload).to_string();
            match deliver(workspace, &payload) {
                Ok(Delivery::Delivered(id)) => on_event(format!("→ delivered to {id}")),
                Ok(Delivery::UnknownRecipient(id)) => on_event(format!(
                    "⚠ dropped — `{id}` is not hosted in this workspace"
                )),
                Err(e) => on_event(format!("⚠ malformed message dropped: {e}")),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_broker_forms() {
        assert_eq!(
            parse_broker("mqtt://broker.local:1884").unwrap(),
            Broker {
                host: "broker.local".into(),
                port: 1884,
                username: None,
                password: None,
            }
        );
        // default port
        assert_eq!(
            parse_broker("mqtt://host").unwrap(),
            Broker {
                host: "host".into(),
                port: 1883,
                username: None,
                password: None,
            }
        );
        // bare host:port (no scheme)
        assert_eq!(parse_broker("127.0.0.1:1883").unwrap().port, 1883);
        // userinfo: user:pass@host:port
        assert_eq!(
            parse_broker("mqtt://bwoc:s3cret@broker.local:1883").unwrap(),
            Broker {
                host: "broker.local".into(),
                port: 1883,
                username: Some("bwoc".into()),
                password: Some("s3cret".into()),
            }
        );
        // userinfo: user only (no password)
        let u = parse_broker("mqtt://bwoc@host").unwrap();
        assert_eq!((u.username.as_deref(), u.password), (Some("bwoc"), None));
        // wrong scheme / empty host
        assert!(parse_broker("tcp://host:1883").is_err());
        assert!(parse_broker("mqtt://:1883").is_err());
        assert!(parse_broker("mqtt://host:notaport").is_err());
    }

    #[test]
    fn parse_broker_percent_decodes_userinfo() {
        // `@` and `:` in the password are percent-encoded so the first-`@` /
        // first-`:` split still works; the decoded value is the real credential.
        let b = parse_broker("mqtt://user:p%40ss%3Aword@broker.local:1883").unwrap();
        assert_eq!(b.username.as_deref(), Some("user"));
        assert_eq!(b.password.as_deref(), Some("p@ss:word"));
        // a stray, malformed `%` is left untouched (no panic).
        let b = parse_broker("mqtt://u%zz@host").unwrap();
        assert_eq!(b.username.as_deref(), Some("u%zz"));
    }

    #[test]
    fn parse_broker_error_redacts_credentials() {
        // A parse failure must not echo `user:pass@` back into the error string.
        let err = parse_broker("mqtt://user:s3cret@host:notaport").unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("s3cret"), "credentials leaked: {msg}");
        assert!(!msg.contains("user:"), "userinfo leaked: {msg}");
        assert!(msg.contains("host"), "redacted host should remain: {msg}");
    }

    #[test]
    fn topic_default_and_override() {
        assert_eq!(topic_for("agent-neo", None), "bwoc/agent-neo/inbox");
        assert_eq!(topic_for("agent-neo", Some("custom/topic")), "custom/topic");
    }

    #[test]
    fn recipient_extracted_from_envelope() {
        let env = r#"{"ts":"t","messageId":"m","from":"a","to":"agent-neo","message":"hi"}"#;
        assert_eq!(recipient_from_envelope(env).unwrap(), "agent-neo");
        // missing / empty / non-object → error
        assert!(recipient_from_envelope(r#"{"from":"a"}"#).is_err());
        assert!(recipient_from_envelope(r#"{"to":""}"#).is_err());
        assert!(recipient_from_envelope("not json").is_err());
    }

    fn write_ws(dir: &Path) {
        let bwoc = dir.join(".bwoc");
        std::fs::create_dir_all(&bwoc).unwrap();
        std::fs::write(
            bwoc.join("agents.toml"),
            "[[agent]]\nid = \"agent-neo\"\npath = \"agents/agent-neo\"\nbackend = \"ollama\"\nincarnated = \"t\"\nstatus = \"active\"\n",
        )
        .unwrap();
    }

    #[test]
    fn deliver_appends_to_known_recipient_inbox() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_ws(tmp.path());
        let env = r#"{"ts":"t","messageId":"m1","from":"a","to":"agent-neo","message":"hi"}"#;

        let d = deliver(tmp.path(), env).unwrap();
        assert_eq!(d, Delivery::Delivered("agent-neo".into()));

        let inbox = tmp.path().join("agents/agent-neo/.bwoc/inbox.jsonl");
        let body = std::fs::read_to_string(&inbox).unwrap();
        assert!(body.contains("\"messageId\":\"m1\""));
        assert!(body.ends_with('\n'));

        // A second message appends a second line.
        let env2 = r#"{"ts":"t","messageId":"m2","from":"a","to":"agent-neo","message":"yo"}"#;
        deliver(tmp.path(), env2).unwrap();
        let lines = std::fs::read_to_string(&inbox).unwrap();
        assert_eq!(lines.lines().count(), 2);
    }

    #[test]
    fn deliver_drops_unknown_recipient() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_ws(tmp.path());
        let env = r#"{"ts":"t","messageId":"m","from":"a","to":"agent-ghost","message":"hi"}"#;
        assert_eq!(
            deliver(tmp.path(), env).unwrap(),
            Delivery::UnknownRecipient("agent-ghost".into())
        );
        assert!(!tmp.path().join("agents/agent-ghost").exists());
    }
}
