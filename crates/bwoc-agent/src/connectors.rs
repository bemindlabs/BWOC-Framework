//! Connector supervision (chat-connectors PR3).
//!
//! When an agent declares an **enabled** connector (e.g.
//! `connectors/telegram.toml` with `enabled = true`), `bwoc-agent --serve`
//! spawns the `bwoc-connect` subprocess and keeps it alive: respawn on exit
//! (with a backoff so a crash-loop can't spin hot), kill on daemon shutdown.
//!
//! `bwoc-connect` carries the network deps (reqwest/tokio); the daemon only
//! *supervises* it as a child — exactly the `bwoc-harness` spawn pattern, so
//! the dep-quarantine HARD RULE holds (`bwoc-agent` stays lean).

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Wait between a child exit and a respawn (avoids a crash-loop spinning hot).
const RESPAWN_BACKOFF: Duration = Duration::from_secs(5);

/// Connector platforms `bwoc-agent` knows how to supervise, by config filename.
const KNOWN: &[(&str, &str)] = &[
    ("telegram", "connectors/telegram.toml"),
    ("discord", "connectors/discord.toml"),
    ("line", "connectors/line.toml"),
];

pub struct ConnectorSupervisor {
    /// Resolved `bwoc-connect` binary; `None` ⇒ none enabled, or not found.
    exe: Option<PathBuf>,
    agent_dir: PathBuf,
    /// Enabled connector platform (PR3 supervises the first one).
    platform: Option<&'static str>,
    child: Option<Child>,
    last_spawn: Option<Instant>,
}

impl ConnectorSupervisor {
    /// Inspect `cwd` for an enabled connector config and resolve the binary.
    pub fn detect(cwd: &Path) -> Self {
        let platform = KNOWN
            .iter()
            .find(|(_, file)| connector_enabled(&cwd.join(file)))
            .map(|(name, _)| *name);
        // Lazy: only probe for the binary when a connector is actually enabled
        // (the common "no connector" startup does no path lookup).
        let exe = platform.and_then(|_| bwoc_core::exec::sibling_binary("bwoc-connect"));
        Self {
            exe,
            agent_dir: cwd.to_path_buf(),
            platform,
            child: None,
            last_spawn: None,
        }
    }

    /// Log what (if anything) will be supervised — called once at startup.
    pub fn announce(&self) {
        match (self.platform, &self.exe) {
            (Some(p), Some(_)) => {
                eprintln!("bwoc-agent --serve: supervising connector '{p}' (bwoc-connect)")
            }
            (Some(p), None) => eprintln!(
                "bwoc-agent --serve: connector '{p}' enabled but the `bwoc-connect` binary \
                 could not be resolved (sibling of bwoc-agent / CARGO_BIN_EXE / PATH) — not \
                 supervising. Install it or put it on PATH."
            ),
            (None, _) => {} // no connector configured — silent
        }
    }

    /// Idle-tick hook: (re)spawn the connector child if it isn't running.
    /// Cheap (`try_wait`) and backoff-bounded — safe to call on every poll.
    pub fn tick(&mut self) {
        let (Some(exe), Some(platform)) = (self.exe.as_ref(), self.platform) else {
            return;
        };
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(None) => return, // still running
                Ok(Some(status)) => {
                    eprintln!("bwoc-agent --serve: connector '{platform}' exited ({status})");
                    self.child = None;
                    self.write_status("exited", None);
                }
                Err(e) => {
                    eprintln!("bwoc-agent --serve: connector wait error: {e}");
                    self.child = None;
                }
            }
        }
        // Crash-loop throttle: refuse to (re)spawn within RESPAWN_BACKOFF of the
        // last spawn. A child that ran healthily for a while (last_spawn long
        // ago) respawns promptly; one that exits within the window waits — so a
        // misconfigured connector can't spin spawn→exit→spawn hot.
        if let Some(t) = self.last_spawn {
            if t.elapsed() < RESPAWN_BACKOFF {
                return;
            }
        }
        self.last_spawn = Some(Instant::now());
        match Command::new(exe)
            .arg(platform)
            .arg("--agent")
            .arg(&self.agent_dir)
            .spawn()
        {
            Ok(c) => {
                let pid = c.id();
                eprintln!("bwoc-agent --serve: spawned connector '{platform}' (pid {pid})");
                self.child = Some(c);
                self.write_status("running", Some(pid));
            }
            Err(e) => eprintln!("bwoc-agent --serve: failed to spawn connector '{platform}': {e}"),
        }
    }

    /// Write the connector health marker (`<agent>/.bwoc/connector.status`) so
    /// `bwoc status` can surface it. Best-effort; only when a connector is
    /// configured. `state` is `running` / `exited` / `stopped`.
    fn write_status(&self, state: &str, pid: Option<u32>) {
        let Some(platform) = self.platform else {
            return;
        };
        let path = self.agent_dir.join(".bwoc").join("connector.status");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = serde_json::json!({ "platform": platform, "state": state, "pid": pid });
        // Atomic: write a temp sibling then rename, so `bwoc status` never reads
        // a half-written marker (rename is atomic on the same filesystem).
        let tmp = path.with_extension("status.tmp");
        if std::fs::write(&tmp, body.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Kill the connector child on daemon shutdown (best-effort).
    pub fn shutdown(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        // Unconditionally mark stopped (even if the child had already exited),
        // so a stale `running` marker can't linger after the daemon is down.
        self.write_status("stopped", None);
    }
}

/// Is a connector config present AND `enabled = true`? Best-effort: a missing
/// or malformed file (or `enabled` absent/false) ⇒ not enabled.
fn connector_enabled(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
        .and_then(|v| v.get("enabled").and_then(toml::Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("connectors");
        fs::create_dir_all(&p).unwrap();
        let f = p.join("telegram.toml");
        fs::write(&f, body).unwrap();
        f
    }

    #[test]
    fn connector_enabled_reads_the_flag() {
        let tmp = TempDir::new().unwrap();
        let f = write(tmp.path(), "enabled = true\nallow_from = [1]\n");
        assert!(connector_enabled(&f));
    }

    #[test]
    fn connector_enabled_false_or_absent_or_malformed() {
        let tmp = TempDir::new().unwrap();
        assert!(!connector_enabled(&write(tmp.path(), "enabled = false\n")));
        assert!(
            !connector_enabled(&write(tmp.path(), "allow_from = [1]\n")),
            "no flag ⇒ false"
        );
        assert!(!connector_enabled(&write(
            tmp.path(),
            "this is not toml {{{"
        )));
        assert!(!connector_enabled(
            &tmp.path().join("connectors/absent.toml")
        ));
    }

    #[test]
    fn detect_finds_enabled_telegram() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "enabled = true\n");
        let sup = ConnectorSupervisor::detect(tmp.path());
        assert_eq!(sup.platform, Some("telegram"));
        // is_active depends on whether a bwoc-connect binary resolves in the
        // test env, so we only assert the platform was detected here.
    }

    #[test]
    fn detect_none_when_disabled() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "enabled = false\n");
        let sup = ConnectorSupervisor::detect(tmp.path());
        assert_eq!(sup.platform, None);
    }

    #[test]
    fn write_status_emits_marker_when_configured() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "enabled = true\n");
        let sup = ConnectorSupervisor::detect(tmp.path());
        sup.write_status("running", Some(42));
        let body = fs::read_to_string(tmp.path().join(".bwoc/connector.status")).unwrap();
        assert!(body.contains("\"platform\":\"telegram\""), "{body}");
        assert!(body.contains("\"state\":\"running\""));
        assert!(body.contains("\"pid\":42"));

        // No connector configured ⇒ no marker written.
        let tmp2 = TempDir::new().unwrap();
        write(tmp2.path(), "enabled = false\n");
        ConnectorSupervisor::detect(tmp2.path()).write_status("running", Some(1));
        assert!(!tmp2.path().join(".bwoc/connector.status").exists());
    }
}
