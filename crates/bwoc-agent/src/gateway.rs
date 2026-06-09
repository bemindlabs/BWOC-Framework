//! Gateway receive-bridge supervision (standalone agents).
//!
//! When an agent declares an **enabled** gateway in `interconnect/gateway.toml`
//! (`enabled = true`, `url = "wss://…/v1/connect"`), `bwoc-agent --serve` spawns
//! the `bwoc-gateway-recv` subprocess and keeps it alive: respawn on exit (a
//! respawn is a reconnect), kill on daemon shutdown. The recv bridge dials the
//! relay, authenticates with the agent's ed25519 keypair (the keypair IS the
//! login), and appends each relayed envelope to `.bwoc/inbox.jsonl` — so a
//! cross-machine message arrives exactly like a local one and the existing
//! inbox poller + trust gate process it unchanged.
//!
//! `bwoc-gateway-recv` carries the network deps (WebSocket/TLS) and lives in the
//! `bwoc-gateway` repo; the daemon only *supervises* it as a child — the same
//! pattern as `bwoc-connect`, so the dep-quarantine HARD RULE holds
//! (`bwoc-agent` stays free of WS/TLS).

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Wait between a child exit and a respawn (avoids a crash-loop spinning hot).
const RESPAWN_BACKOFF: Duration = Duration::from_secs(5);

pub struct GatewayRecvSupervisor {
    /// Resolved `bwoc-gateway-recv` binary; `None` ⇒ disabled or not found.
    exe: Option<PathBuf>,
    agent_dir: PathBuf,
    /// `wss://…/v1/connect`; `Some` only when a gateway is enabled.
    url: Option<String>,
    /// The agent id presented at the relay handshake.
    agent_id: Option<String>,
    child: Option<Child>,
    last_spawn: Option<Instant>,
}

impl GatewayRecvSupervisor {
    /// Inspect `cwd/interconnect/gateway.toml`; resolve url + agent-id + binary.
    pub fn detect(cwd: &Path) -> Self {
        let cfg = read_gateway_config(&cwd.join("interconnect/gateway.toml"));
        let (url, agent_id) = match cfg {
            Some((url, cfg_id)) => {
                // agent_id: explicit in gateway.toml, else manifest `agentId`.
                let id = cfg_id.or_else(|| read_agent_id(&cwd.join("config.manifest.json")));
                (Some(url), id)
            }
            None => (None, None),
        };
        // Lazy: only probe for the binary when a gateway is actually enabled.
        let exe = url
            .as_ref()
            .and_then(|_| bwoc_core::exec::sibling_binary("bwoc-gateway-recv"));
        Self {
            exe,
            agent_dir: cwd.to_path_buf(),
            url,
            agent_id,
            child: None,
            last_spawn: None,
        }
    }

    /// Log what (if anything) will be supervised — called once at startup.
    pub fn announce(&self) {
        match (&self.url, &self.agent_id, &self.exe) {
            (Some(url), Some(id), Some(_)) => {
                eprintln!("bwoc-agent --serve: gateway recv bridge for '{id}' → {url}")
            }
            (Some(_), None, _) => eprintln!(
                "bwoc-agent --serve: gateway enabled but no agent id (set `agent_id` in \
                 interconnect/gateway.toml or `agentId` in config.manifest.json) — not bridging."
            ),
            (Some(url), Some(_), None) => eprintln!(
                "bwoc-agent --serve: gateway '{url}' enabled but the `bwoc-gateway-recv` binary \
                 could not be resolved (sibling of bwoc-agent / CARGO_BIN_EXE / PATH) — not \
                 bridging. Install it or put it on PATH."
            ),
            (None, _, _) => {} // no gateway configured — silent
        }
    }

    /// Idle-tick hook: (re)spawn the recv bridge if it isn't running.
    /// Cheap (`try_wait`) and backoff-bounded — safe to call on every poll.
    pub fn tick(&mut self) {
        let (Some(exe), Some(url), Some(agent_id)) =
            (self.exe.as_ref(), self.url.as_ref(), self.agent_id.as_ref())
        else {
            return;
        };
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(None) => return, // still running
                Ok(Some(status)) => {
                    eprintln!("bwoc-agent --serve: gateway recv exited ({status})");
                    self.child = None;
                    self.write_status("exited", None);
                }
                Err(e) => {
                    eprintln!("bwoc-agent --serve: gateway recv wait error: {e}");
                    self.child = None;
                }
            }
        }
        // Crash-loop throttle (same discipline as the connector supervisor): a
        // child that exits within RESPAWN_BACKOFF of its last spawn waits before
        // respawning, so a dead relay can't spin spawn→exit→spawn hot.
        if let Some(t) = self.last_spawn {
            if t.elapsed() < RESPAWN_BACKOFF {
                return;
            }
        }
        self.last_spawn = Some(Instant::now());
        let key = self.agent_dir.join(".bwoc").join("agent.key");
        let inbox = self.agent_dir.join(".bwoc").join("inbox.jsonl");
        match Command::new(exe)
            .arg("--url")
            .arg(url)
            .arg("--agent-id")
            .arg(agent_id)
            .arg("--key-file")
            .arg(&key)
            .arg("--inbox")
            .arg(&inbox)
            .spawn()
        {
            Ok(c) => {
                let pid = c.id();
                eprintln!("bwoc-agent --serve: spawned gateway recv '{agent_id}' (pid {pid})");
                self.child = Some(c);
                self.write_status("running", Some(pid));
            }
            Err(e) => eprintln!("bwoc-agent --serve: failed to spawn gateway recv: {e}"),
        }
    }

    /// Write the bridge health marker (`<agent>/.bwoc/gateway.status`).
    fn write_status(&self, state: &str, pid: Option<u32>) {
        let Some(agent_id) = &self.agent_id else {
            return;
        };
        let path = self.agent_dir.join(".bwoc").join("gateway.status");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::json!({ "agentId": agent_id, "state": state, "pid": pid });
        let tmp = path.with_extension("status.tmp");
        if std::fs::write(&tmp, body.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Kill the recv child on daemon shutdown (best-effort).
    pub fn shutdown(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        if self.url.is_some() {
            self.write_status("stopped", None);
        }
    }
}

/// Read `(url, agent_id?)` from a gateway config IF present AND `enabled = true`.
/// A missing/malformed file, `enabled` false/absent, or a missing `url` ⇒ `None`.
fn read_gateway_config(path: &Path) -> Option<(String, Option<String>)> {
    let v: toml::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())?;
    if !v
        .get("enabled")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let url = v.get("url").and_then(toml::Value::as_str)?.to_string();
    let agent_id = v
        .get("agent_id")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    Some((url, agent_id))
}

/// Best-effort read of `agentId` from `config.manifest.json`.
fn read_agent_id(path: &Path) -> Option<String> {
    let v: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())?;
    v.get("agentId")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_cfg(dir: &Path, body: &str) {
        let p = dir.join("interconnect");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("gateway.toml"), body).unwrap();
    }

    #[test]
    fn reads_enabled_url_and_agent_id() {
        let tmp = TempDir::new().unwrap();
        write_cfg(
            tmp.path(),
            "enabled = true\nurl = \"wss://r/v1/connect\"\nagent_id = \"agent-x\"\n",
        );
        let cfg = read_gateway_config(&tmp.path().join("interconnect/gateway.toml")).unwrap();
        assert_eq!(cfg.0, "wss://r/v1/connect");
        assert_eq!(cfg.1.as_deref(), Some("agent-x"));
    }

    #[test]
    fn disabled_absent_or_missing_url_is_none() {
        let tmp = TempDir::new().unwrap();
        write_cfg(tmp.path(), "enabled = false\nurl = \"wss://r\"\n");
        assert!(read_gateway_config(&tmp.path().join("interconnect/gateway.toml")).is_none());
        write_cfg(tmp.path(), "enabled = true\n"); // no url
        assert!(read_gateway_config(&tmp.path().join("interconnect/gateway.toml")).is_none());
        assert!(read_gateway_config(&tmp.path().join("interconnect/absent.toml")).is_none());
    }

    #[test]
    fn detect_falls_back_to_manifest_agent_id() {
        let tmp = TempDir::new().unwrap();
        write_cfg(tmp.path(), "enabled = true\nurl = \"wss://r/v1/connect\"\n");
        fs::write(
            tmp.path().join("config.manifest.json"),
            "{\"agentId\":\"agent-neo\"}",
        )
        .unwrap();
        let sup = GatewayRecvSupervisor::detect(tmp.path());
        assert_eq!(sup.url.as_deref(), Some("wss://r/v1/connect"));
        assert_eq!(sup.agent_id.as_deref(), Some("agent-neo"));
    }

    #[test]
    fn detect_none_when_disabled() {
        let tmp = TempDir::new().unwrap();
        write_cfg(tmp.path(), "enabled = false\n");
        let sup = GatewayRecvSupervisor::detect(tmp.path());
        assert!(sup.url.is_none());
    }

    #[test]
    fn write_status_emits_marker() {
        let tmp = TempDir::new().unwrap();
        write_cfg(tmp.path(), "enabled = true\nurl = \"wss://r/v1/connect\"\n");
        fs::write(
            tmp.path().join("config.manifest.json"),
            "{\"agentId\":\"agent-neo\"}",
        )
        .unwrap();
        let sup = GatewayRecvSupervisor::detect(tmp.path());
        sup.write_status("running", Some(7));
        let body = fs::read_to_string(tmp.path().join(".bwoc/gateway.status")).unwrap();
        assert!(body.contains("\"agentId\":\"agent-neo\""), "{body}");
        assert!(body.contains("\"state\":\"running\""));
    }
}
