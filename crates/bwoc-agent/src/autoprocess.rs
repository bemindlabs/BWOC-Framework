//! Untrusted auto-processing of inbound (gateway / remote) messages.
//!
//! When a standalone agent receives a message from a remote peer (relayed
//! through `bwoc-gateway` into `.bwoc/inbox.jsonl`) and the trust gate passes
//! it, `bwoc-agent --serve` can *automatically* run the agent on that message
//! and reply — closing the loop so a deployed agent actually responds across
//! machines, not just logs the arrival.
//!
//! **Security (Phase 5):** a relayed message is internet-sourced adversarial
//! input that drives a tool-capable harness. It is processed exactly like a
//! chat-connector turn — **UNTRUSTED**: the message is injected as
//! `ChatInput::User` with no explicit principal, so the harness defaults it to
//! `Principal::Unknown` → `TrustLevel::Untrusted` (read-only by default,
//! effectful tools capability-denied, every tool jailed per turn). The harness
//! stdin is a pipe (non-TTY), so any `ask`-mode tool fails closed; this code
//! also auto-denies every `PermissionRequest` — a remote sender can never
//! approve a tool. The reply is sent back with `bwoc send` (routing back out
//! through the same `transport = "gateway"` route).
//!
//! Opt-in: `interconnect/gateway.toml` `auto_process = true`. Off by default
//! (an agent that only relays/forwards need not run a model per message).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use bwoc_core::manifest::Manifest;
use bwoc_core::trust::Principal;

/// Cap on harness output lines read per message — a backstop against a runaway
/// child flooding stdout (the loop also ends on the turn's `Message` event).
const MAX_EVENT_LINES: usize = 100_000;

pub struct AutoProcessor {
    enabled: bool,
    agent_dir: PathBuf,
    self_id: String,
    harness: Option<PathBuf>,
    bwoc: Option<PathBuf>,
    model: Option<String>,
    endpoint: Option<String>,
    backend: Option<String>,
}

impl AutoProcessor {
    /// Build from the agent directory. Enabled only when
    /// `interconnect/gateway.toml` sets `auto_process = true` AND both the
    /// `bwoc-harness` and `bwoc` binaries resolve.
    pub fn detect(cwd: &Path) -> Self {
        let enabled = auto_process_enabled(&cwd.join("interconnect/gateway.toml"));
        let manifest = Manifest::load_from_path(&cwd.join("config.manifest.json")).ok();
        let (harness, bwoc) = if enabled {
            (
                bwoc_core::exec::sibling_binary("bwoc-harness"),
                bwoc_core::exec::sibling_binary("bwoc"),
            )
        } else {
            (None, None)
        };
        Self {
            enabled,
            agent_dir: cwd.to_path_buf(),
            self_id: manifest
                .as_ref()
                .map(|m| m.agent_id.clone())
                .unwrap_or_default(),
            harness,
            bwoc,
            model: manifest.as_ref().map(|m| m.primary_model.clone()),
            endpoint: manifest.as_ref().and_then(|m| m.base_url.clone()),
            backend: manifest.as_ref().and_then(|m| m.backend.clone()),
        }
    }

    /// True when auto-processing is active (config on + binaries resolved).
    pub fn is_active(&self) -> bool {
        self.enabled && self.harness.is_some() && self.bwoc.is_some() && !self.self_id.is_empty()
    }

    /// Log the auto-process posture once at startup.
    pub fn announce(&self) {
        if !self.enabled {
            return;
        }
        if self.is_active() {
            eprintln!("bwoc-agent --serve: gateway auto-process ON (untrusted harness turns)");
        } else {
            eprintln!(
                "bwoc-agent --serve: gateway auto-process requested but inactive — \
                 `bwoc-harness`/`bwoc` not resolved or no agentId."
            );
        }
    }

    /// Run an **untrusted** harness turn on `message` from `from`, then send the
    /// reply back to `from` via `bwoc send`. Best-effort: logs and returns on any
    /// failure (a bad turn must never crash the daemon). Blocks the serve loop
    /// for the duration of the turn (one message at a time — acceptable for a
    /// dedicated standalone agent; a worker pool is a follow-up).
    pub fn handle(&self, from: &str, message: &str) {
        let (Some(harness), Some(bwoc)) = (&self.harness, &self.bwoc) else {
            return;
        };
        let reply = match self.run_untrusted_turn(harness, message) {
            Ok(r) if !r.trim().is_empty() => r,
            Ok(_) => {
                eprintln!("bwoc-agent --serve: auto-process produced an empty reply for {from}");
                return;
            }
            Err(e) => {
                eprintln!("bwoc-agent --serve: auto-process turn failed for {from}: {e}");
                return;
            }
        };
        // Reply back through the routing table (transport=gateway for a remote
        // peer). `--no-wakeup` since there's no local TUI to poke.
        match Command::new(bwoc)
            .arg("send")
            .arg(from)
            .arg(&reply)
            .arg("--from")
            .arg(&self.self_id)
            .arg("--no-wakeup")
            .current_dir(&self.agent_dir)
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("bwoc-agent --serve: reply to {from} exited {s}"),
            Err(e) => eprintln!("bwoc-agent --serve: failed to send reply to {from}: {e}"),
        }
    }

    /// Spawn `bwoc-harness --chat` and drive one untrusted turn, returning the
    /// assistant's final reply text.
    fn run_untrusted_turn(&self, harness: &Path, message: &str) -> Result<String, String> {
        let mut cmd = Command::new(harness);
        cmd.arg("--chat").arg("--workdir").arg(&self.agent_dir);
        if let Some(m) = &self.model {
            cmd.arg("--model").arg(m);
        }
        if let Some(e) = &self.endpoint {
            cmd.arg("--endpoint").arg(e);
        }
        if let Some(b) = &self.backend {
            cmd.arg("--backend").arg(b);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn harness: {e}"))?;

        let mut stdin = child.stdin.take().ok_or("no harness stdin")?;
        let stdout = child.stdout.take().ok_or("no harness stdout")?;

        // Inject the remote message as UNTRUSTED user input (Unknown principal).
        let user = ChatInput::User {
            text: message.to_string(),
            principal: Principal::default(), // Unknown → Untrusted, fail-closed
        };
        writeln!(stdin, "{}", user.to_line().map_err(|e| e.to_string())?)
            .map_err(|e| format!("write user input: {e}"))?;

        // Read events: accumulate token deltas, auto-deny any permission
        // request, and **finish when the turn ends** — `Message` (the final
        // text) or, when the model produced no `Message` (e.g. an empty/errored
        // turn), `TurnEnd`. Breaking only on `Message` would hang the daemon on
        // any turn that ends without one (a model with no tool support, an empty
        // completion, …), because `--chat` then blocks waiting for more input.
        let mut reply = String::new();
        let mut acc = String::new();
        let mut lines = 0usize;
        for line in BufReader::new(stdout).lines() {
            let line = line.map_err(|e| format!("read harness: {e}"))?;
            lines += 1;
            if lines > MAX_EVENT_LINES {
                return Err("harness output exceeded line cap".into());
            }
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ChatEvent>(&line) {
                Ok(ChatEvent::Token { text }) => acc.push_str(&text),
                Ok(ChatEvent::Message { text }) => {
                    reply = text;
                    break; // turn complete with a final message
                }
                Ok(ChatEvent::TurnEnd { .. }) => break, // turn complete (no Message)
                Ok(ChatEvent::Error { message }) => {
                    return Err(format!("harness turn error: {message}"));
                }
                Ok(ChatEvent::PermissionRequest { id, .. }) => {
                    // A remote sender can never approve a tool — deny + continue.
                    let deny = ChatInput::Permission { id, allow: false };
                    let _ = writeln!(stdin, "{}", deny.to_line().map_err(|e| e.to_string())?);
                }
                _ => {} // other events (ready/restored/bye) / unparsable: ignore
            }
        }

        // End the session cleanly, then reap.
        let _ = writeln!(stdin, "{}", ChatInput::Quit.to_line().unwrap_or_default());
        drop(stdin);
        let _ = child.wait();

        Ok(if reply.is_empty() { acc } else { reply })
    }
}

/// `interconnect/gateway.toml` `auto_process = true`? Missing/malformed ⇒ false.
fn auto_process_enabled(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
        .and_then(|v| v.get("auto_process").and_then(toml::Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_gateway(dir: &Path, body: &str) {
        let p = dir.join("interconnect");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("gateway.toml"), body).unwrap();
    }

    #[test]
    fn auto_process_flag_parsing() {
        let tmp = TempDir::new().unwrap();
        write_gateway(
            tmp.path(),
            "enabled = true\nurl = \"wss://r\"\nauto_process = true\n",
        );
        assert!(auto_process_enabled(
            &tmp.path().join("interconnect/gateway.toml")
        ));
        write_gateway(tmp.path(), "enabled = true\nurl = \"wss://r\"\n");
        assert!(!auto_process_enabled(
            &tmp.path().join("interconnect/gateway.toml")
        ));
        assert!(!auto_process_enabled(&tmp.path().join("nope.toml")));
    }

    #[test]
    fn inactive_without_flag() {
        let tmp = TempDir::new().unwrap();
        write_gateway(tmp.path(), "enabled = true\nurl = \"wss://r\"\n");
        let ap = AutoProcessor::detect(tmp.path());
        assert!(!ap.is_active());
    }
}
