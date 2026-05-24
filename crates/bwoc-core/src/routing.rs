//! Inter-workspace routing — `.bwoc/interconnect/routes.toml`.
//!
//! Implements the routing table described in
//! `modules/agent-template/interconnect/routing.md`.
//!
//! # Resolution contract (spec §"Resolution Order")
//!
//! 1. Caller first checks the local `AgentsRegistry` (not this module's
//!    concern). On a miss, call `Routes::load` then `Routes::resolve`.
//! 2. `resolve` returns the peer workspace root on a match, `None` on no
//!    match. The caller then loads the peer `AgentsRegistry` and retargets
//!    the inbox path.
//! 3. No match → caller emits `NotFound` unchanged.
//!
//! # v1 scope constraints
//!
//! - Local filesystem peers only. No network transport.
//! - Single hop: a route resolves to a terminal workspace, never another
//!   routing table.
//! - No discovery or gossip: peers are declared by hand in `routes.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Deserialised view of `.bwoc/interconnect/routes.toml`.
///
/// Construct via [`Routes::load`]. An absent file is not an error; it
/// produces an empty [`Routes`] equivalent to today's single-workspace
/// behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Routes {
    pub routes: Vec<Route>,
}

/// One entry in `[[route]]`.
///
/// Exactly one of `agent` or `namespace` must be set. `workspace` is
/// always required and must be an absolute path to the peer workspace
/// root (the directory that holds that workspace's `.bwoc/agents.toml`).
///
/// Validation (see [`RouteValidationError`]) is enforced at load time via
/// [`Routes::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Peer workspace root — the directory holding `.bwoc/agents.toml`.
    pub workspace: PathBuf,
    /// Discriminant: `agent` (exact id match) or `namespace` (prefix match).
    pub kind: RouteKind,
}

/// How a [`Route`] matches a recipient id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteKind {
    /// Exact match against the recipient's canonical id (e.g. `"agent-neo"`).
    Agent(String),
    /// Prefix match: routes any recipient id that starts with `<namespace>`.
    /// E.g. `"team-b"` routes `"team-b-foo"`, `"team-b-bar"`, etc.
    Namespace(String),
}

/// Error returned when `routes.toml` is malformed or a route entry is invalid.
#[derive(Debug, thiserror::Error)]
pub enum RoutingError {
    #[error("io error reading routes.toml: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid TOML in routes.toml: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("route validation error: {0}")]
    Validation(#[from] RouteValidationError),
}

/// Validation failures for individual route entries.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RouteValidationError {
    #[error(
        "route for workspace '{workspace}' has both 'agent' and 'namespace' set — exactly one is required"
    )]
    BothKeys { workspace: String },
    #[error(
        "route for workspace '{workspace}' has neither 'agent' nor 'namespace' — exactly one is required"
    )]
    NeitherKey { workspace: String },
}

// ── Raw TOML shape ────────────────────────────────────────────────────────────

/// Internal raw TOML representation. Never exposed publicly; converted to
/// [`Route`] after validation in [`Routes::load`].
#[derive(Debug, Deserialize, Serialize)]
struct RawRoutes {
    #[serde(default, rename = "route")]
    routes: Vec<RawRoute>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RawRoute {
    /// Peer workspace root directory (absolute path).
    workspace: String,
    /// Exact recipient id.
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    /// Namespace prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
}

// ── Load + validate ───────────────────────────────────────────────────────────

impl Routes {
    /// Load and validate `<workspace_root>/.bwoc/interconnect/routes.toml`.
    ///
    /// An absent file is not an error — returns an empty [`Routes`] which
    /// preserves today's single-workspace behaviour (spec §"The Routing Table":
    /// "Absent file ≡ no peers ≡ today's behaviour").
    pub fn load(workspace_root: &Path) -> Result<Self, RoutingError> {
        let path = workspace_root
            .join(".bwoc")
            .join("interconnect")
            .join("routes.toml");

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)?;
        if content.trim().is_empty() {
            return Ok(Self::default());
        }

        let raw: RawRoutes = toml::from_str(&content)?;
        let routes = raw
            .routes
            .into_iter()
            .map(validate_route)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { routes })
    }

    /// Remove all routes whose `RouteKind` is `Agent(agent_id)` and rewrite
    /// `<workspace_root>/.bwoc/interconnect/routes.toml` with the remainder.
    ///
    /// Used by `bwoc retire` Step 3 (interconnect deregister): dead agents
    /// must not remain reachable from the routing table.
    ///
    /// Idempotency: if no routes match `agent_id`, the file is rewritten
    /// unchanged (still a valid operation). If the file is absent, nothing
    /// is written and `Ok(0)` is returned.
    ///
    /// Returns the count of routes removed.
    pub fn remove_agent_routes(
        workspace_root: &Path,
        agent_id: &str,
    ) -> Result<usize, RoutingError> {
        let path = workspace_root
            .join(".bwoc")
            .join("interconnect")
            .join("routes.toml");

        if !path.exists() {
            return Ok(0);
        }

        let content = std::fs::read_to_string(&path)?;
        if content.trim().is_empty() {
            return Ok(0);
        }

        let raw: RawRoutes = toml::from_str(&content)?;
        let before = raw.routes.len();
        let kept: Vec<RawRoute> = raw
            .routes
            .into_iter()
            .filter(|r| r.agent.as_deref() != Some(agent_id))
            .collect();
        let removed = before - kept.len();

        // Rewrite the file with the surviving routes (preserves workspace-
        // scoped routes and namespace routes untouched).
        let out = RawRoutes { routes: kept };
        let toml_str = toml::to_string(&out).map_err(|e| {
            // toml::ser::Error doesn't implement std::io::Error, so wrap via Io
            // using a fabricated io::Error with the serialization message.
            RoutingError::Io(std::io::Error::other(e.to_string()))
        })?;
        std::fs::write(&path, toml_str)?;

        Ok(removed)
    }

    /// Resolve a recipient id to its peer workspace root.
    ///
    /// Resolution order (spec §"Resolution Order" step 2):
    /// 1. Exact `agent` match wins first.
    /// 2. Longest `namespace` prefix match (by prefix byte length) wins next.
    /// 3. No match → `None`.
    ///
    /// The caller is responsible for the local-registry check (step 1) before
    /// calling this method.
    pub fn resolve(&self, recipient_id: &str) -> Option<&Path> {
        // Pass 1: exact agent match.
        for route in &self.routes {
            if let RouteKind::Agent(id) = &route.kind {
                if id == recipient_id {
                    return Some(&route.workspace);
                }
            }
        }

        // Pass 2: longest namespace prefix match.
        let mut best: Option<(&str, &Path)> = None;
        for route in &self.routes {
            if let RouteKind::Namespace(ns) = &route.kind {
                if recipient_id.starts_with(ns.as_str()) {
                    let is_longer = best.is_none_or(|(prev, _)| ns.len() > prev.len());
                    if is_longer {
                        best = Some((ns.as_str(), &route.workspace));
                    }
                }
            }
        }
        best.map(|(_, ws)| ws)
    }
}

// ── Validation helper ─────────────────────────────────────────────────────────

fn validate_route(raw: RawRoute) -> Result<Route, RouteValidationError> {
    let kind = match (raw.agent, raw.namespace) {
        (Some(_), Some(_)) => {
            return Err(RouteValidationError::BothKeys {
                workspace: raw.workspace,
            });
        }
        (None, None) => {
            return Err(RouteValidationError::NeitherKey {
                workspace: raw.workspace,
            });
        }
        (Some(id), None) => RouteKind::Agent(id),
        (None, Some(ns)) => RouteKind::Namespace(ns),
    };
    Ok(Route {
        workspace: PathBuf::from(raw.workspace),
        kind,
    })
}

// ── Shared-allowlist (cross-workspace learn, #20) ──────────────────────────────

/// A peer's declared allowlist of doc-kinds readable across workspaces, from
/// `<peer>/.bwoc/interconnect/shared.toml`. Absent or empty → nothing shared
/// (opt-in; the safe default). The peer controls its own exposure.
#[derive(Debug, Default, serde::Deserialize)]
pub struct SharedAllowlist {
    /// Doc-kind names the peer exposes (e.g. `["research", "retrospectives"]`).
    #[serde(default)]
    pub share: Vec<String>,
}

impl SharedAllowlist {
    /// Load a peer's shared-allowlist. Absent or unparseable → empty (never errors).
    pub fn load(peer_ws: &std::path::Path) -> Self {
        let path = peer_ws.join(".bwoc/interconnect/shared.toml");
        match std::fs::read_to_string(&path) {
            Ok(c) => toml::from_str::<Self>(&c).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_ws(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-routes-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".bwoc/interconnect")).unwrap();
        root
    }

    fn write_routes(root: &Path, content: &str) {
        fs::write(root.join(".bwoc/interconnect/routes.toml"), content).unwrap();
    }

    // ── Absent / empty file ───────────────────────────────────────────────────

    #[test]
    fn absent_file_is_empty_routes() {
        let root = temp_ws("absent");
        let routes = Routes::load(&root).unwrap();
        assert!(routes.routes.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_file_is_empty_routes() {
        let root = temp_ws("empty-file");
        write_routes(&root, "   \n");
        let routes = Routes::load(&root).unwrap();
        assert!(routes.routes.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    // ── Happy-path deserialization ────────────────────────────────────────────

    #[test]
    fn load_exact_agent_route() {
        let root = temp_ws("agent-route");
        write_routes(
            &root,
            r#"
[[route]]
agent = "agent-neo"
workspace = "/srv/ws-b"
"#,
        );
        let routes = Routes::load(&root).unwrap();
        assert_eq!(routes.routes.len(), 1);
        assert_eq!(routes.routes[0].kind, RouteKind::Agent("agent-neo".into()));
        assert_eq!(routes.routes[0].workspace, PathBuf::from("/srv/ws-b"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn load_namespace_route() {
        let root = temp_ws("ns-route");
        write_routes(
            &root,
            r#"
[[route]]
namespace = "team-b"
workspace = "/srv/team-b-ws"
"#,
        );
        let routes = Routes::load(&root).unwrap();
        assert_eq!(routes.routes.len(), 1);
        assert_eq!(routes.routes[0].kind, RouteKind::Namespace("team-b".into()));
        let _ = fs::remove_dir_all(&root);
    }

    // ── Validation errors ─────────────────────────────────────────────────────

    #[test]
    fn both_keys_is_validation_error() {
        let root = temp_ws("both-keys");
        write_routes(
            &root,
            r#"
[[route]]
agent = "agent-neo"
namespace = "team-b"
workspace = "/srv/ws"
"#,
        );
        let err = Routes::load(&root).unwrap_err();
        assert!(
            matches!(
                err,
                RoutingError::Validation(RouteValidationError::BothKeys { .. })
            ),
            "expected BothKeys, got {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn neither_key_is_validation_error() {
        let root = temp_ws("neither-key");
        write_routes(
            &root,
            r#"
[[route]]
workspace = "/srv/ws"
"#,
        );
        let err = Routes::load(&root).unwrap_err();
        assert!(
            matches!(
                err,
                RoutingError::Validation(RouteValidationError::NeitherKey { .. })
            ),
            "expected NeitherKey, got {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    // ── Resolution order ──────────────────────────────────────────────────────

    #[test]
    fn resolve_exact_agent_match() {
        let routes = Routes {
            routes: vec![Route {
                workspace: PathBuf::from("/peer/ws"),
                kind: RouteKind::Agent("agent-neo".into()),
            }],
        };
        assert_eq!(routes.resolve("agent-neo"), Some(Path::new("/peer/ws")));
    }

    #[test]
    fn resolve_namespace_prefix_match() {
        let routes = Routes {
            routes: vec![Route {
                workspace: PathBuf::from("/team-b/ws"),
                kind: RouteKind::Namespace("agent-team-b".into()),
            }],
        };
        assert_eq!(
            routes.resolve("agent-team-b-worker"),
            Some(Path::new("/team-b/ws"))
        );
    }

    #[test]
    fn resolve_exact_wins_over_namespace() {
        // Even when a namespace would also match, the exact agent route wins.
        let routes = Routes {
            routes: vec![
                Route {
                    workspace: PathBuf::from("/namespace/ws"),
                    kind: RouteKind::Namespace("agent-neo".into()),
                },
                Route {
                    workspace: PathBuf::from("/exact/ws"),
                    kind: RouteKind::Agent("agent-neo".into()),
                },
            ],
        };
        assert_eq!(routes.resolve("agent-neo"), Some(Path::new("/exact/ws")));
    }

    #[test]
    fn resolve_longest_namespace_wins() {
        let routes = Routes {
            routes: vec![
                Route {
                    workspace: PathBuf::from("/short/ws"),
                    kind: RouteKind::Namespace("team".into()),
                },
                Route {
                    workspace: PathBuf::from("/long/ws"),
                    kind: RouteKind::Namespace("team-b".into()),
                },
            ],
        };
        // "team-b-worker" matches both "team" and "team-b"; longest wins.
        assert_eq!(routes.resolve("team-b-worker"), Some(Path::new("/long/ws")));
    }

    #[test]
    fn resolve_no_match_returns_none() {
        let routes = Routes {
            routes: vec![Route {
                workspace: PathBuf::from("/peer/ws"),
                kind: RouteKind::Agent("agent-neo".into()),
            }],
        };
        assert_eq!(routes.resolve("agent-unknown"), None);
    }

    #[test]
    fn resolve_empty_routes_returns_none() {
        let routes = Routes::default();
        assert_eq!(routes.resolve("agent-anything"), None);
    }
}
