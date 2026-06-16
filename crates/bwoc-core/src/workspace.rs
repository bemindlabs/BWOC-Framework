//! BWOC workspace types — `.bwoc/workspace.toml` and `.bwoc/agents.toml`.
//!
//! Per the spec in `docs/en/WORKSPACE.en.md`, a workspace is a directory
//! containing a `.bwoc/` marker with `workspace.toml` (metadata + defaults)
//! and `agents.toml` (registry of incarnated agents).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Top-level structure of `.bwoc/workspace.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub workspace: WorkspaceMeta,
    #[serde(default)]
    pub defaults: WorkspaceDefaults,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceMeta {
    pub name: String,
    pub version: String,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default = "default_agents_dir")]
    pub agents_dir: String,
}

impl Default for WorkspaceDefaults {
    fn default() -> Self {
        Self {
            backend: None,
            lang: None,
            agents_dir: default_agents_dir(),
        }
    }
}

fn default_agents_dir() -> String {
    "agents".to_string()
}

/// Top-level structure of `.bwoc/agents.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentsRegistry {
    #[serde(default, rename = "agent")]
    pub agents: Vec<AgentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentEntry {
    pub id: String,
    pub path: String,
    pub backend: String,
    pub incarnated: String,
    pub status: String,
}

impl AgentEntry {
    /// The agent's directory on disk: `<workspace>/<path>`.
    pub fn dir(&self, workspace: &Path) -> PathBuf {
        workspace.join(&self.path)
    }

    /// The canonical inbox path: `<workspace>/<path>/.bwoc/inbox.jsonl`.
    ///
    /// Single source of truth for "where does this agent's inbox live". The
    /// `bwoc inbox` reader, the a2a/gateway writer, and `bwoc check` all resolve
    /// through here, so a reader and a writer can never disagree about the path
    /// (issue #302). External writers (e.g. a launchd `gateway-recv`) should
    /// derive the path via `bwoc inbox <agent> --path` rather than hardcode it.
    pub fn inbox_path(&self, workspace: &Path) -> PathBuf {
        self.dir(workspace).join(".bwoc/inbox.jsonl")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid TOML: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("serialize TOML: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
}

impl Workspace {
    /// Load `<root>/.bwoc/workspace.toml`.
    pub fn load(root: &Path) -> Result<Self, WorkspaceError> {
        let p = root.join(".bwoc/workspace.toml");
        let content = fs::read_to_string(&p)?;
        let ws = toml::from_str(&content)?;
        Ok(ws)
    }

    /// Save to `<root>/.bwoc/workspace.toml` (creating `.bwoc/` if needed).
    pub fn save(&self, root: &Path) -> Result<(), WorkspaceError> {
        let dir = root.join(".bwoc");
        fs::create_dir_all(&dir)?;
        let p = dir.join("workspace.toml");
        let content = toml::to_string_pretty(self)?;
        fs::write(&p, content)?;
        Ok(())
    }
}

impl AgentsRegistry {
    /// Load `<root>/.bwoc/agents.toml`. Returns empty registry if file is missing.
    pub fn load(root: &Path) -> Result<Self, WorkspaceError> {
        let p = root.join(".bwoc/agents.toml");
        if !p.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&p)?;
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        let reg = toml::from_str(&content)?;
        Ok(reg)
    }

    /// Save to `<root>/.bwoc/agents.toml`.
    pub fn save(&self, root: &Path) -> Result<(), WorkspaceError> {
        let dir = root.join(".bwoc");
        fs::create_dir_all(&dir)?;
        let p = dir.join("agents.toml");
        let content = if self.agents.is_empty() {
            "# Agents registry — managed by the bwoc CLI.\n# Entries are added by `bwoc new` and removed by `bwoc retire`.\n".to_string()
        } else {
            toml::to_string_pretty(self)?
        };
        fs::write(&p, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn fresh_temp_dir(label: &str) -> std::path::PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "bwoc-workspace-test-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn workspace_roundtrip() {
        let ws = Workspace {
            workspace: WorkspaceMeta {
                name: "demo".into(),
                version: "0.1.0".into(),
                created: "2026-05-22T06:00:00Z".into(),
            },
            defaults: WorkspaceDefaults::default(),
        };
        let dir = fresh_temp_dir("roundtrip");
        ws.save(&dir).unwrap();
        let back = Workspace::load(&dir).unwrap();
        assert_eq!(ws, back);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_registry_empty_file_is_ok() {
        let dir = fresh_temp_dir("empty");
        let reg = AgentsRegistry::default();
        reg.save(&dir).unwrap();
        let back = AgentsRegistry::load(&dir).unwrap();
        assert!(back.agents.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_registry_with_entries_roundtrip() {
        let dir = fresh_temp_dir("with-entries");
        let reg = AgentsRegistry {
            agents: vec![AgentEntry {
                id: "agent-foo".into(),
                path: "agents/agent-foo".into(),
                backend: "claude".into(),
                incarnated: "2026-05-22T06:00:00Z".into(),
                status: "active".into(),
            }],
        };
        reg.save(&dir).unwrap();
        let back = AgentsRegistry::load(&dir).unwrap();
        assert_eq!(reg, back);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_entry_inbox_path_is_canonical() {
        // The single source of truth the `bwoc inbox` reader and the a2a/gateway
        // writer both resolve through (issue #302) — they can't disagree.
        let entry = AgentEntry {
            id: "agent-foo".into(),
            path: "agents/agent-foo".into(),
            backend: "claude".into(),
            incarnated: "2026-05-22T06:00:00Z".into(),
            status: "active".into(),
        };
        let ws = Path::new("/ws");
        assert_eq!(entry.dir(ws), Path::new("/ws/agents/agent-foo"));
        assert_eq!(
            entry.inbox_path(ws),
            Path::new("/ws/agents/agent-foo/.bwoc/inbox.jsonl")
        );
    }
}
