//! Outbound A2A client credentials (AP5, #80).
//!
//! When a BWOC agent *calls* an external A2A agent (`bwoc a2a send` /
//! `fetch-card`) it may need to authenticate to that peer. This stores the
//! per-peer bearer tokens an operator configures in
//! `<workspace>/.bwoc/a2a-credentials.json` — a flat map of remote **origin**
//! (`scheme://host[:port]`) to token:
//!
//! ```json
//! { "https://peer.example": "tokenA", "https://other.example:8443": "tokenB" }
//! ```
//!
//! Lookups match by canonical origin, so a configured `https://peer.example`
//! covers `https://peer.example/rpc` and `https://peer.example:443/x` alike, but
//! not a different host or port. The file holds secrets, so on Unix it must be
//! `0600` or stricter — a group/world-readable file is **refused**, matching the
//! inbound `.bwoc/a2a.token` gate. The `--token` flag overrides any file lookup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CredsError {
    #[error("a2a-credentials read error at {path}: {message}")]
    Io { path: String, message: String },
    #[error("a2a-credentials parse error at {path}: {message}")]
    Parse { path: String, message: String },
    #[error("{0}")]
    Perms(String),
}

/// Per-origin outbound bearer tokens, keyed by canonical origin.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Credentials {
    by_origin: HashMap<String, String>,
}

/// The credentials file path for a workspace.
pub fn credentials_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".bwoc/a2a-credentials.json")
}

impl Credentials {
    /// Load from `<workspace>/.bwoc/a2a-credentials.json`. A missing file is an
    /// empty set (no error). On Unix the file must be `0600` or stricter.
    pub fn load(workspace_root: &Path) -> Result<Self, CredsError> {
        Self::load_path(&credentials_path(workspace_root))
    }

    fn load_path(path: &Path) -> Result<Self, CredsError> {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(CredsError::Io {
                    path: path.display().to_string(),
                    message: e.to_string(),
                });
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .map_err(|e| CredsError::Io {
                    path: path.display().to_string(),
                    message: e.to_string(),
                })?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                return Err(CredsError::Perms(format!(
                    "credentials file {} is group/world-accessible (mode {:04o}); it \
                     holds peer bearer tokens. Run `chmod 600 {}`.",
                    path.display(),
                    mode & 0o7777,
                    path.display()
                )));
            }
        }
        let map: HashMap<String, String> =
            serde_json::from_str(&raw).map_err(|e| CredsError::Parse {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
        // Canonicalize each configured key to a comparable origin; skip entries
        // whose key isn't a parseable absolute URL rather than failing the load.
        let by_origin = map
            .into_iter()
            .filter_map(|(k, v)| origin_of(&k).map(|o| (o, v)))
            .collect();
        Ok(Self { by_origin })
    }

    /// The token to present to `url`, matched by origin, if one is configured.
    pub fn token_for(&self, url: &str) -> Option<&str> {
        let origin = origin_of(url)?;
        self.by_origin.get(&origin).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.by_origin.is_empty()
    }
}

/// Canonical origin (`scheme://host[:port]`) of a URL, or `None` if it isn't a
/// parseable absolute URL with a tuple origin.
fn origin_of(url: &str) -> Option<String> {
    let origin = reqwest::Url::parse(url).ok()?.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_creds(dir: &Path, json: &str) -> PathBuf {
        let p = credentials_path(dir);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, json).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        p
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let c = Credentials::load(dir.path()).unwrap();
        assert!(c.is_empty());
        assert_eq!(c.token_for("https://peer.example"), None);
    }

    #[test]
    fn matches_by_origin_ignoring_path_and_default_port() {
        let dir = tempfile::tempdir().unwrap();
        write_creds(
            dir.path(),
            r#"{"https://peer.example": "tokA", "https://other.example:8443": "tokB"}"#,
        );
        let c = Credentials::load(dir.path()).unwrap();
        assert_eq!(c.token_for("https://peer.example/"), Some("tokA"));
        assert_eq!(c.token_for("https://peer.example/rpc"), Some("tokA"));
        // Explicit default port is the same origin.
        assert_eq!(c.token_for("https://peer.example:443/x"), Some("tokA"));
        assert_eq!(c.token_for("https://other.example:8443/y"), Some("tokB"));
        // Different host or port is a different origin.
        assert_eq!(c.token_for("https://unknown.example"), None);
        assert_eq!(c.token_for("https://peer.example:9999"), None);
    }

    #[test]
    fn skips_unparseable_keys() {
        let dir = tempfile::tempdir().unwrap();
        write_creds(
            dir.path(),
            r#"{"not a url": "x", "https://ok.example": "tok"}"#,
        );
        let c = Credentials::load(dir.path()).unwrap();
        assert_eq!(c.token_for("https://ok.example"), Some("tok"));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_group_or_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = write_creds(dir.path(), r#"{"https://p.example":"t"}"#);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        let err = Credentials::load(dir.path()).unwrap_err();
        assert!(matches!(err, CredsError::Perms(_)), "got {err:?}");
    }
}
