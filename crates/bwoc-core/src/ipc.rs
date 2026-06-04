//! IPC endpoint naming — shared between the daemon (`bwoc-agent --serve`) and
//! its CLI clients (`bwoc ping` / `status` / `stop`).
//!
//! On Unix the endpoint is a filesystem socket at `<agent>/.bwoc/agent.sock`,
//! so the agent directory itself namespaces it. Windows named pipes live in a
//! single global namespace (`\\.\pipe\…`), so each agent needs a unique,
//! deterministic pipe name both sides can derive independently from the agent
//! directory — that derivation lives here. Dependency-free (FNV-1a inline) so
//! `bwoc-core` stays lean.

use std::path::Path;

/// Derive the per-agent named-pipe name (without the `\\.\pipe\` prefix) from
/// the agent directory. Deterministic: server and client compute it
/// independently and meet at the same pipe. The path is canonicalized when
/// possible so `./agents/x` and its absolute form agree; a non-existent path
/// falls back to hashing the lossy string as given.
pub fn pipe_name(agent_dir: &Path) -> String {
    let canon = std::fs::canonicalize(agent_dir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| agent_dir.to_string_lossy().into_owned());
    format!("bwoc-agent-{:016x}", fnv1a(canon.as_bytes()))
}

/// FNV-1a 64-bit — tiny, dependency-free, stable across platforms/releases.
/// Collision resistance is not a goal (pipe names are namespacing, not
/// security); 64 bits over a handful of agent dirs is plenty.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_name_is_deterministic() {
        let p = Path::new("/ws/agents/agent-x");
        assert_eq!(pipe_name(p), pipe_name(p));
    }

    #[test]
    fn pipe_name_distinct_per_agent() {
        assert_ne!(
            pipe_name(Path::new("/ws/agents/agent-x")),
            pipe_name(Path::new("/ws/agents/agent-y"))
        );
    }

    #[test]
    fn pipe_name_shape() {
        let n = pipe_name(Path::new("/nonexistent/agent-z"));
        assert!(n.starts_with("bwoc-agent-"));
        assert_eq!(n.len(), "bwoc-agent-".len() + 16);
    }
}
