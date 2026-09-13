//! BWOC workspace types — `.bwoc/workspace.toml` and `.bwoc/agents.toml`.
//!
//! Per the spec in `docs/en/WORKSPACE.en.md`, a workspace is a directory
//! containing a `.bwoc/` marker with `workspace.toml` (metadata + defaults)
//! and `agents.toml` (registry of incarnated agents).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::schema::SchemaVersion;

/// Top-level structure of `.bwoc/workspace.toml`.
///
/// `schema_version` is declared first because TOML serialization requires every
/// scalar key to precede the first table — moving it below `workspace` makes
/// [`Workspace::save`] fail at runtime, not at compile time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    /// Absent in anything 2.x wrote ⇒ [`SchemaVersion::LEGACY`]. See
    /// [`crate::schema`] for the seam.
    #[serde(default)]
    pub schema_version: SchemaVersion,
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
///
/// Field order matters for the same reason it does on [`Workspace`]: the
/// scalar marker must precede the `[[agent]]` array of tables.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentsRegistry {
    /// Absent in anything 2.x wrote ⇒ [`SchemaVersion::LEGACY`].
    #[serde(default)]
    pub schema_version: SchemaVersion,
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
    /// One resolver for "where does this agent's inbox live", shared by the
    /// registry-driven readers/writers — the `bwoc inbox` reader and the
    /// a2a/gateway writer (`resolve_agent`) both go through here, so a reader and
    /// a writer can never disagree about the path (issue #302). External writers
    /// (e.g. a launchd `gateway-recv`) should derive the path via
    /// `bwoc inbox <agent> --path` rather than hardcode it.
    ///
    /// (`bwoc check` computes the same suffix against the agent dir it is already
    /// pointed at, not via the registry, so it does not call this.)
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
    ///
    /// The write always stamps [`SchemaVersion::CURRENT`], regardless of what
    /// was loaded: the file that comes back out was written by *this* build, so
    /// claiming an older revision would be a lie. It also means any command
    /// that rewrites the workspace carries a legacy file forward, and
    /// `bwoc migrate` is load-then-save rather than a bespoke rewriter.
    pub fn save(&self, root: &Path) -> Result<(), WorkspaceError> {
        let dir = root.join(".bwoc");
        fs::create_dir_all(&dir)?;
        let p = dir.join("workspace.toml");
        let mut out = self.clone();
        out.schema_version = SchemaVersion::CURRENT;
        let content = toml::to_string_pretty(&out)?;
        fs::write(&p, content)?;
        Ok(())
    }
}

impl AgentsRegistry {
    /// Load `<root>/.bwoc/agents.toml`. Returns empty registry if file is missing.
    pub fn load(root: &Path) -> Result<Self, WorkspaceError> {
        let p = root.join(".bwoc/agents.toml");
        if !p.exists() {
            return Ok(Self::fresh());
        }
        let content = fs::read_to_string(&p)?;
        if content.trim().is_empty() {
            return Ok(Self::fresh());
        }
        let reg = toml::from_str(&content)?;
        Ok(reg)
    }

    /// An empty registry that no 2.x binary wrote.
    ///
    /// Distinct from [`Default`]: the derived default carries
    /// [`SchemaVersion::LEGACY`], which is the right reading for a *file* whose
    /// marker is absent but the wrong one for a file that does not exist —
    /// reporting "needs migration" for a workspace with no registry yet would
    /// be noise.
    fn fresh() -> Self {
        Self {
            schema_version: SchemaVersion::CURRENT,
            agents: Vec::new(),
        }
    }

    /// Save to `<root>/.bwoc/agents.toml`.
    ///
    /// Stamps [`SchemaVersion::CURRENT`] for the same reason [`Workspace::save`]
    /// does.
    pub fn save(&self, root: &Path) -> Result<(), WorkspaceError> {
        let dir = root.join(".bwoc");
        fs::create_dir_all(&dir)?;
        let p = dir.join("agents.toml");
        let mut out = self.clone();
        out.schema_version = SchemaVersion::CURRENT;
        let content = if out.agents.is_empty() {
            format!(
                "# Agents registry — managed by the bwoc CLI.\n# Entries are added by `bwoc new` and removed by `bwoc retire`.\nschema_version = {}\n",
                SchemaVersion::CURRENT.0
            )
        } else {
            toml::to_string_pretty(&out)?
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
    fn legacy_files_load_without_a_marker() {
        // The 2.x shape verbatim: no `schema_version` anywhere.
        let ws: Workspace = toml::from_str(
            "[workspace]\nname = 'demo'\nversion = '0.1.0'\ncreated = '2026-05-22T06:00:00Z'\n",
        )
        .unwrap();
        assert_eq!(ws.schema_version, SchemaVersion::LEGACY);

        let reg: AgentsRegistry = toml::from_str(
            "[[agent]]\nid = 'agent-a'\npath = 'agents/agent-a'\nbackend = 'claude'\nincarnated = '2026-05-22T06:00:00Z'\nstatus = 'active'\n",
        )
        .unwrap();
        assert_eq!(reg.schema_version, SchemaVersion::LEGACY);
        assert_eq!(reg.agents.len(), 1);
    }

    #[test]
    fn save_stamps_current_over_a_legacy_load() {
        let dir = fresh_temp_dir("stamp");
        let mut ws = Workspace {
            schema_version: SchemaVersion::LEGACY,
            workspace: WorkspaceMeta {
                name: "demo".into(),
                version: "0.1.0".into(),
                created: "2026-05-22T06:00:00Z".into(),
            },
            defaults: WorkspaceDefaults::default(),
        };
        ws.save(&dir).unwrap();
        assert_eq!(
            Workspace::load(&dir).unwrap().schema_version,
            SchemaVersion::CURRENT
        );

        // The in-memory value the caller held is untouched — `save` stamps the
        // file, it does not mutate the caller's struct.
        assert_eq!(ws.schema_version, SchemaVersion::LEGACY);
        ws.schema_version = SchemaVersion::CURRENT;
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_toml_marker_precedes_the_array_of_tables() {
        // TOML requires every scalar key before the first table, so a
        // `schema_version` declared after `agents` would make this serialize to
        // a document that cannot be read back. Guard the field order.
        let dir = fresh_temp_dir("ordering");
        let reg = AgentsRegistry {
            schema_version: SchemaVersion::CURRENT,
            agents: vec![AgentEntry {
                id: "agent-a".into(),
                path: "agents/agent-a".into(),
                backend: "claude".into(),
                incarnated: "2026-05-22T06:00:00Z".into(),
                status: "active".into(),
            }],
        };
        reg.save(&dir).unwrap();
        let text = fs::read_to_string(dir.join(".bwoc/agents.toml")).unwrap();
        assert!(
            text.find("schema_version").unwrap() < text.find("[[agent]]").unwrap(),
            "marker must precede the array of tables:\n{text}"
        );
        assert_eq!(AgentsRegistry::load(&dir).unwrap(), reg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_registry_file_still_carries_the_marker() {
        let dir = fresh_temp_dir("emptyreg");
        AgentsRegistry::fresh().save(&dir).unwrap();
        assert_eq!(
            AgentsRegistry::load(&dir).unwrap().schema_version,
            SchemaVersion::CURRENT
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn absent_registry_is_current_not_legacy() {
        // No file is not a 2.x file — reporting "needs migration" for a
        // workspace that has never registered an agent would be noise.
        let dir = fresh_temp_dir("absentreg");
        assert_eq!(
            AgentsRegistry::load(&dir).unwrap().schema_version,
            SchemaVersion::CURRENT
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspace_roundtrip() {
        let ws = Workspace {
            schema_version: crate::schema::SchemaVersion::CURRENT,
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
            schema_version: crate::schema::SchemaVersion::CURRENT,
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
