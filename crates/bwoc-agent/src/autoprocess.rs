//! Untrusted auto-processing of inbound (gateway / remote) messages.
//!
//! When a standalone agent receives a message from a remote peer (relayed
//! through `bwoc-gateway` into `.bwoc/inbox.jsonl`) and the trust gate passes
//! it, `bwoc-agent --serve` can *automatically* run the agent on that message
//! and reply — closing the loop so a deployed agent actually responds across
//! machines, not just logs the arrival.
//!
//! Each remote sender gets a **warm** `bwoc-harness --chat` session, kept alive
//! across messages and idle-reaped (#410) — so a back-and-forth reuses one
//! process + its conversation context instead of cold-starting a fresh harness
//! per message. These message sessions are deliberately **separate** from the
//! trusted task resident (`warm::WarmHarness`): the monotonic session-trust
//! latch means one Untrusted turn taints a session for good, so trusted task
//! execution and untrusted remote messages must never share a process.
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

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Lines, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use bwoc_core::chat_proto::{ChatEvent, ChatInput};
use bwoc_core::manifest::Manifest;
use bwoc_core::trust::Principal;

/// Cap on harness output lines read per message — a backstop against a runaway
/// child flooding stdout (the loop also ends on the turn's `TurnEnd` event).
const MAX_EVENT_LINES: usize = 100_000;

/// Idle timeout before a warm per-sender session is reaped. Mirrors the task
/// resident's `warm::DEFAULT_IDLE` — long enough to keep a conversation warm
/// across a human's think-time, short enough not to hoard processes.
const DEFAULT_MSG_IDLE: Duration = Duration::from_secs(600);

/// Upper bound on concurrent warm sessions. The pool is keyed by the envelope's
/// `from`, which is remote-controlled — an attacker could otherwise flood
/// distinct sender ids and spawn unbounded harness processes (resource DoS).
/// At the cap, a new sender evicts the least-recently-active session.
const MAX_WARM_SESSIONS: usize = 16;

/// How to spawn a per-sender `bwoc-harness --chat` — owned so it can be built
/// from `&AutoProcessor` and then used while `self.sessions` is mutated.
struct SessionCfg {
    harness: PathBuf,
    agent_dir: PathBuf,
    model: Option<String>,
    endpoint: Option<String>,
    backend: Option<String>,
}

/// A warm `bwoc-harness --chat` subprocess dedicated to one remote sender.
/// Kept alive across messages (idle-reaped) so a back-and-forth with a peer
/// reuses one process + its conversation context instead of cold-starting a
/// fresh harness per message (the previous behaviour, and #410's core gap).
/// Untrusted throughout: every turn injects `Principal::Unknown` and auto-denies
/// permission prompts, so the monotonic session-trust latch is a non-issue —
/// the session is Untrusted from its first turn and stays that way.
struct WarmSession {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    last_activity: Instant,
}

impl WarmSession {
    fn spawn(cfg: &SessionCfg) -> Result<Self, String> {
        let mut cmd = Command::new(&cfg.harness);
        cmd.arg("--chat").arg("--workdir").arg(&cfg.agent_dir);
        if let Some(m) = &cfg.model {
            cmd.arg("--model").arg(m);
        }
        if let Some(e) = &cfg.endpoint {
            cmd.arg("--endpoint").arg(e);
        }
        if let Some(b) = &cfg.backend {
            cmd.arg("--backend").arg(b);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn harness: {e}"))?;
        let stdin = child.stdin.take().ok_or("no harness stdin")?;
        let stdout = child.stdout.take().ok_or("no harness stdout")?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            last_activity: Instant::now(),
        })
    }

    /// Whether the child is still running (else it must be respawned).
    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Inject `message` as an UNTRUSTED user turn and return the assistant's
    /// final reply. The event loop lives in [`drive_turn`] (pure + tested); an
    /// error leaves the session for `handle` to drop (its `Drop` reaps the
    /// child, so a runaway that tripped the line cap is still killed).
    fn run_turn(&mut self, message: &str) -> Result<String, String> {
        let user = ChatInput::User {
            text: message.to_string(),
            principal: Principal::default(), // Unknown → Untrusted, fail-closed
        };
        writeln!(self.stdin, "{}", user.to_line().map_err(|e| e.to_string())?)
            .map_err(|e| format!("write user input: {e}"))?;
        // `by_ref` so the iterator survives for the *next* turn; disjoint from the
        // `stdin` borrow used to write denials.
        let out = drive_turn(&mut self.stdout, &mut self.stdin);
        self.last_activity = Instant::now();
        out
    }
}

/// Drive one turn's events off `lines`, auto-denying permission prompts via
/// `deny`. Drains to `TurnEnd` (capturing `Message` en route) and **stops
/// there** — leaving `lines` positioned for the next turn — or ends on EOF.
/// Pure over its two streams, so it is unit-testable with in-memory data:
/// breaking early on `Message` would leave a stray `TurnEnd` that a reused
/// session's next turn would consume as an instant empty reply.
fn drive_turn<I, W>(lines: &mut I, deny: &mut W) -> Result<String, String>
where
    I: Iterator<Item = std::io::Result<String>>,
    W: Write,
{
    let mut reply = String::new();
    let mut acc = String::new();
    let mut err: Option<String> = None;
    let mut n = 0usize;
    for line in lines {
        let line = line.map_err(|e| format!("read harness: {e}"))?;
        n += 1;
        if n > MAX_EVENT_LINES {
            return Err("harness output exceeded line cap".into());
        }
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ChatEvent>(&line) {
            Ok(ChatEvent::Token { text }) => acc.push_str(&text),
            Ok(ChatEvent::Message { text }) => reply = text, // capture; keep draining
            Ok(ChatEvent::TurnEnd { .. }) => break,          // clean turn boundary
            Ok(ChatEvent::Error { message }) => err = Some(message),
            Ok(ChatEvent::PermissionRequest { id, .. }) => {
                // A remote sender can never approve a tool — deny + continue.
                let d = ChatInput::Permission { id, allow: false };
                if let Ok(l) = d.to_line() {
                    let _ = writeln!(deny, "{l}");
                }
            }
            _ => {} // ready/restored/bye / unparsable: ignore
        }
    }
    if let Some(e) = err {
        return Err(format!("harness turn error: {e}"));
    }
    Ok(if reply.is_empty() { acc } else { reply })
}

impl Drop for WarmSession {
    fn drop(&mut self) {
        let _ = writeln!(
            self.stdin,
            "{}",
            ChatInput::Quit.to_line().unwrap_or_default()
        );
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct AutoProcessor {
    enabled: bool,
    agent_dir: PathBuf,
    self_id: String,
    harness: Option<PathBuf>,
    bwoc: Option<PathBuf>,
    model: Option<String>,
    endpoint: Option<String>,
    backend: Option<String>,
    /// True when the manifest backend is an **ambient** tier (e.g. `cli`): the
    /// model's tools run in a vendor subprocess the harness cannot confine, so
    /// the Untrusted read-only guarantee (#271) is structurally unenforceable.
    /// Auto-processing untrusted remote input on such a backend is refused.
    ambient_backend: bool,
    /// Warm `bwoc-harness --chat` sessions, one per remote sender, kept alive
    /// across messages and idle-reaped. Replaces the previous cold-start-per-
    /// message behaviour (#410) so a peer conversation reuses one process +
    /// context. Untrusted throughout — never the trusted task resident.
    sessions: HashMap<String, WarmSession>,
    /// Idle timeout for reaping a warm session in [`Self::tick_idle`].
    idle_timeout: Duration,
}

impl AutoProcessor {
    /// Build from the agent directory. Enabled only when
    /// `interconnect/gateway.toml` sets `auto_process = true` AND both the
    /// `bwoc-harness` and `bwoc` binaries resolve.
    pub fn detect(cwd: &Path) -> Self {
        let enabled = auto_process_enabled(&cwd.join("interconnect/gateway.toml"));
        let manifest = Manifest::load_from_path(&cwd.join("config.manifest.json")).ok();
        let backend = manifest.as_ref().and_then(|m| m.backend.clone());
        // An absent backend resolves to the default HTTP path (Confined); only
        // an explicit ambient backend (`cli`) trips the refusal.
        let ambient_backend = backend
            .as_deref()
            .map(|b| bwoc_core::trust::backend_trust_tier(b).is_ambient())
            .unwrap_or(false);
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
            backend,
            ambient_backend,
            sessions: HashMap::new(),
            idle_timeout: DEFAULT_MSG_IDLE,
        }
    }

    /// True when auto-processing is active (config on + binaries resolved + the
    /// backend can be confined). An **ambient** backend (`cli`) is refused here:
    /// auto-processing feeds Untrusted internet input to a tool-capable harness,
    /// and on an ambient backend the harness cannot enforce the read-only
    /// posture (#271) — so the gate fails closed and never engages.
    pub fn is_active(&self) -> bool {
        self.enabled
            && self.harness.is_some()
            && self.bwoc.is_some()
            && !self.self_id.is_empty()
            && !self.ambient_backend
    }

    /// Log the auto-process posture once at startup.
    pub fn announce(&self) {
        if !self.enabled {
            return;
        }
        if self.is_active() {
            eprintln!("bwoc-agent --serve: gateway auto-process ON (untrusted harness turns)");
        } else if self.ambient_backend {
            // The security-relevant refusal: config asked for auto-process but
            // the backend is ambient, so we fail closed rather than feed
            // untrusted remote input to an unconfined tool-capable subprocess.
            eprintln!(
                "bwoc-agent --serve: ⚠ gateway auto-process REFUSED — backend `{}` is AMBIENT \
                 (the vendor CLI runs its own tools outside the harness; the Untrusted read-only \
                 guarantee #271 cannot be enforced). Switch to an HTTP backend to auto-process \
                 remote messages.",
                self.backend.as_deref().unwrap_or("cli")
            );
        } else {
            eprintln!(
                "bwoc-agent --serve: gateway auto-process requested but inactive — \
                 `bwoc-harness`/`bwoc` not resolved or no agentId."
            );
        }
    }

    /// Build the spawn config for a per-sender warm session (owned clones, so it
    /// can be used while `self.sessions` is mutated). Only called after the
    /// harness path is known to resolve.
    fn session_cfg(&self) -> Option<SessionCfg> {
        Some(SessionCfg {
            harness: self.harness.clone()?,
            agent_dir: self.agent_dir.clone(),
            model: self.model.clone(),
            endpoint: self.endpoint.clone(),
            backend: self.backend.clone(),
        })
    }

    /// Run an **untrusted** harness turn on `message` from `from`, then send the
    /// reply back to `from` via `bwoc send`. Best-effort: logs and returns on any
    /// failure (a bad turn must never crash the daemon). Reuses a **warm** per-
    /// sender session (respawning if it died); blocks the serve loop for the
    /// turn (one message at a time — a concurrent pool is a follow-up).
    pub fn handle(&mut self, from: &str, message: &str) {
        if self.bwoc.is_none() {
            return;
        }
        let Some(cfg) = self.session_cfg() else {
            return;
        };
        // Reuse this sender's warm session, else (re)spawn one.
        let alive = self
            .sessions
            .get_mut(from)
            .map(WarmSession::is_alive)
            .unwrap_or(false);
        if !alive {
            // Bound the pool against a flood of distinct remote sender ids: at
            // the cap, evict the least-recently-active session before adding a
            // new sender. (A present-but-dead sender just respawns in place — no
            // eviction, the map size is unchanged.)
            if !self.sessions.contains_key(from) && self.sessions.len() >= MAX_WARM_SESSIONS {
                if let Some(victim) = self
                    .sessions
                    .iter()
                    .min_by_key(|(_, s)| s.last_activity)
                    .map(|(k, _)| k.clone())
                {
                    eprintln!(
                        "bwoc-agent --serve: warm message pool at cap ({MAX_WARM_SESSIONS}) — \
                         evicting least-recent sender `{victim}`"
                    );
                    self.sessions.remove(&victim); // Drop reaps the child
                }
            }
            match WarmSession::spawn(&cfg) {
                Ok(s) => {
                    self.sessions.insert(from.to_string(), s);
                }
                Err(e) => {
                    eprintln!("bwoc-agent --serve: auto-process spawn failed for {from}: {e}");
                    return;
                }
            }
        }
        let turn = self
            .sessions
            .get_mut(from)
            .expect("session present after spawn")
            .run_turn(message);
        let reply = match turn {
            Ok(r) if !r.trim().is_empty() => r,
            Ok(_) => {
                eprintln!("bwoc-agent --serve: auto-process produced an empty reply for {from}");
                return;
            }
            Err(e) => {
                eprintln!("bwoc-agent --serve: auto-process turn failed for {from}: {e}");
                // A failed turn may have left the session wedged — drop it so the
                // next message from this sender starts a fresh harness.
                self.sessions.remove(from);
                return;
            }
        };
        self.send_reply(from, &reply);
    }

    /// Reply back through the routing table (transport=gateway for a remote
    /// peer). `--no-wakeup` since there's no local TUI to poke. The `--`
    /// separator terminates option parsing so a `from` or `reply` that happens
    /// to start with `-`/`--` (remote-controlled text) can never be misread as a
    /// flag (arg injection).
    fn send_reply(&self, from: &str, reply: &str) {
        let Some(bwoc) = &self.bwoc else { return };
        match Command::new(bwoc)
            .arg("send")
            .arg("--from")
            .arg(&self.self_id)
            .arg("--no-wakeup")
            .arg("--")
            .arg(from)
            .arg(reply)
            .current_dir(&self.agent_dir)
            .status()
        {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("bwoc-agent --serve: reply to {from} exited {s}"),
            Err(e) => eprintln!("bwoc-agent --serve: failed to send reply to {from}: {e}"),
        }
    }

    /// Reap warm sessions that have been idle past the timeout (or died). Called
    /// on the serve loop's idle tick, mirroring `warm::WarmHarness::tick_idle`.
    /// Dropping the entry runs `WarmSession::drop` (Quit + kill + wait).
    pub fn tick_idle(&mut self) {
        let timeout = self.idle_timeout;
        self.sessions
            .retain(|_, s| s.is_alive() && s.last_activity.elapsed() < timeout);
    }

    /// Reap every warm session on daemon shutdown.
    pub fn shutdown(&mut self) {
        self.sessions.clear();
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

    /// Build an iterator of `io::Result<String>` events for `drive_turn`.
    fn ev(lines: &[&str]) -> std::vec::IntoIter<std::io::Result<String>> {
        lines
            .iter()
            .map(|s| Ok(s.to_string()))
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn drive_turn_drains_to_turn_end_and_stops() {
        let mut it = ev(&[
            r#"{"type":"token","text":"hi "}"#,
            r#"{"type":"message","text":"final answer"}"#,
            r#"{"type":"turn_end","prompt_tokens":1,"completion_tokens":1}"#,
            r#"{"type":"message","text":"NEXT TURN — must NOT be read now"}"#,
        ]);
        let mut deny: Vec<u8> = Vec::new();
        let reply = drive_turn(&mut it, &mut deny).unwrap();
        assert_eq!(reply, "final answer"); // captured Message, not the token acc
        assert!(deny.is_empty(), "no permission prompts → no denials");
        // Stopped exactly at TurnEnd: the next turn's line is still queued, proving
        // a reused session won't eat a stray TurnEnd.
        assert!(
            it.next().is_some(),
            "over-consumed past the TurnEnd boundary"
        );
    }

    #[test]
    fn drive_turn_auto_denies_permission() {
        let mut it = ev(&[
            r#"{"type":"permission_request","id":"p1","tool":"run_command","detail":"rm -rf /"}"#,
            r#"{"type":"message","text":"ok"}"#,
            r#"{"type":"turn_end","prompt_tokens":0,"completion_tokens":0}"#,
        ]);
        let mut deny: Vec<u8> = Vec::new();
        let reply = drive_turn(&mut it, &mut deny).unwrap();
        assert_eq!(reply, "ok");
        let denied = String::from_utf8(deny).unwrap();
        assert!(
            denied.contains("\"id\":\"p1\""),
            "denial targets the request: {denied}"
        );
        assert!(denied.contains("\"allow\":false"), "auto-denied: {denied}");
    }

    #[test]
    fn drive_turn_surfaces_error() {
        let mut it = ev(&[
            r#"{"type":"error","message":"boom"}"#,
            r#"{"type":"turn_end","prompt_tokens":0,"completion_tokens":0}"#,
        ]);
        let mut deny: Vec<u8> = Vec::new();
        assert!(drive_turn(&mut it, &mut deny).is_err());
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

    /// Auto-process is REFUSED on an ambient backend (`cli`) even when the flag
    /// is on and a manifest is present: feeding Untrusted remote input to an
    /// unconfined tool-capable subprocess would void the #271 read-only
    /// guarantee, so the gate fails closed. (`is_active()` also requires the
    /// sibling binaries, but `ambient_backend` must independently force it off —
    /// this asserts the field is set so the refusal holds wherever it is read.)
    #[test]
    fn ambient_backend_refuses_auto_process() {
        let tmp = TempDir::new().unwrap();
        write_gateway(
            tmp.path(),
            "enabled = true\nurl = \"wss://r\"\nauto_process = true\n",
        );
        fs::write(
            tmp.path().join("config.manifest.json"),
            r#"{"name":"x","agentId":"agent-x","agentRole":"r","primaryModel":"m","backend":"cli","memoryPath":"memories/","lintCmd":"x","formatCmd":"x","testCmd":"x","buildCmd":"x","version":"1"}"#,
        )
        .unwrap();
        let ap = AutoProcessor::detect(tmp.path());
        assert!(ap.ambient_backend, "cli backend must be flagged ambient");
        assert!(
            !ap.is_active(),
            "ambient backend must force auto-process off (fail-closed)"
        );
    }

    /// A confined (HTTP) backend is NOT flagged ambient — the refusal is
    /// specific to backends whose tools escape the harness.
    #[test]
    fn confined_backend_is_not_flagged_ambient() {
        let tmp = TempDir::new().unwrap();
        write_gateway(
            tmp.path(),
            "enabled = true\nurl = \"wss://r\"\nauto_process = true\n",
        );
        fs::write(
            tmp.path().join("config.manifest.json"),
            r#"{"name":"x","agentId":"agent-x","agentRole":"r","primaryModel":"m","backend":"claude","memoryPath":"memories/","lintCmd":"x","formatCmd":"x","testCmd":"x","buildCmd":"x","version":"1"}"#,
        )
        .unwrap();
        let ap = AutoProcessor::detect(tmp.path());
        assert!(!ap.ambient_backend, "claude backend must stay confined");
    }
}
