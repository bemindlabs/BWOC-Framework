//! A2A push-notification **config management** (#48 **P5**).
//!
//! Stores the per-task webhook configs a client registers via
//! `CreateTaskPushNotificationConfig` (+ Get/List/Delete) in a team-scoped
//! `push-configs.json` next to the team's `tasks.jsonl`.
//!
//! **Delivery is deliberately NOT implemented here.** Actually POSTing task
//! updates to a registered webhook is a network egress that, under P1's no-auth
//! posture, is both an SSRF amplifier (webhook → internal services) and a
//! data-exfil path (a client registers an external sink for a task's updates).
//! So P5 ships only the config CRUD; delivery lands with the auth phase, which
//! is where the webhook URL can be safely constrained and the registrant
//! authenticated. Storing an inert config is surfaced here and in the docs
//! rather than implying notifications will fire (Musāvāda).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One registered push-notification config for a task. A task may have several
/// (keyed by `config_id`), per the A2A model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushConfig {
    /// The task this webhook is registered against.
    #[serde(rename = "taskId")]
    pub task_id: String,
    /// Stable id for this config (server-assigned on create).
    #[serde(rename = "id")]
    pub config_id: String,
    /// Webhook URL the agent would POST task updates to (delivery: auth phase).
    pub url: String,
    /// Optional bearer-style token the agent would present to the webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Errors from the config store. Kept minimal — the RPC layer maps these to
/// JSON-RPC error codes.
#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("push-configs read/parse error at {path}: {message}")]
    Io { path: String, message: String },
}

/// The `push-configs.json` path for the team whose `tasks.jsonl` is `tasks_path`.
pub fn configs_path(tasks_path: &Path) -> PathBuf {
    let dir = tasks_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join("push-configs.json")
}

/// Load all stored configs. A missing file is an empty list, not an error.
pub fn load(path: &Path) -> Result<Vec<PushConfig>, PushError> {
    match std::fs::read_to_string(path) {
        Ok(body) if body.trim().is_empty() => Ok(Vec::new()),
        Ok(body) => serde_json::from_str(&body).map_err(|e| PushError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(PushError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        }),
    }
}

/// Persist the full config list (create the parent dir if needed). Writes to a
/// sibling `.tmp` then renames, so a crash mid-write can't truncate the store.
pub fn save(path: &Path, configs: &[PushConfig]) -> Result<(), PushError> {
    let io_err = |e: std::io::Error| PushError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_err)?;
    }
    let body = serde_json::to_string_pretty(configs).map_err(|e| PushError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(io_err)?;
    std::fs::rename(&tmp, path).map_err(io_err)?;
    // The file can hold a registrant's bearer token — lock it to the owner,
    // matching how the signing key (`.bwoc/agent.key`) is protected.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(io_err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(task: &str, id: &str, url: &str) -> PushConfig {
        PushConfig {
            task_id: task.into(),
            config_id: id.into(),
            url: url.into(),
            token: None,
        }
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("none.json")).unwrap().is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("teams/x/push-configs.json");
        let configs = vec![cfg("t1", "c1", "https://hook.example/1")];
        save(&p, &configs).unwrap();
        assert_eq!(load(&p).unwrap(), configs);
    }

    #[test]
    fn configs_path_sits_beside_tasks_jsonl() {
        let p = configs_path(Path::new("/ws/.bwoc/teams/sec/tasks.jsonl"));
        assert_eq!(p, Path::new("/ws/.bwoc/teams/sec/push-configs.json"));
    }

    #[cfg(unix)]
    #[test]
    fn saved_file_is_owner_only_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("push-configs.json");
        save(&p, &[cfg("t1", "c1", "https://h/1")]).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "push-configs.json must be owner-only");
    }

    #[test]
    fn token_is_omitted_when_absent() {
        let c = cfg("t1", "c1", "https://h/1");
        let j = serde_json::to_string(&c).unwrap();
        assert!(!j.contains("token"));
        assert!(j.contains("\"taskId\":\"t1\""));
        assert!(j.contains("\"id\":\"c1\""));
    }
}
