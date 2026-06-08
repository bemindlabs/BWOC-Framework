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
//! # Transports
//!
//! Each route declares a [`RouteTarget`]: `local` (a peer workspace path on
//! this machine — the v1 default, written directly into the peer's inbox) or
//! `mqtt` (publish to a broker for cross-machine federation; the actual publish
//! lives in the `bwoc-mqtt` crate so `bwoc-core` stays dependency-free).
//!
//! # Scope constraints
//!
//! - Single hop: a route resolves to a terminal target, never another routing
//!   table.
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
/// Exactly one of `agent` or `namespace` must be set (the match discriminant).
/// The delivery [`RouteTarget`] is `local` (a peer workspace path, the v1
/// default) or `mqtt` (publish to a broker — cross-machine federation).
///
/// Validation (see [`RouteValidationError`]) is enforced at load time via
/// [`Routes::load`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Discriminant: `agent` (exact id match) or `namespace` (prefix match).
    pub kind: RouteKind,
    /// Where a matched message is delivered.
    pub target: RouteTarget,
}

/// Where a matched [`Route`] delivers its envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTarget {
    /// A peer workspace root on the local filesystem — the directory holding
    /// that workspace's `.bwoc/agents.toml`. The caller writes directly into
    /// the peer agent's `inbox.jsonl` (v1 default, no network).
    Local(PathBuf),
    /// A remote peer reached over MQTT: the sender publishes the envelope to
    /// `broker`, and the peer's `bwoc-mqtt serve` daemon drops it into the
    /// recipient's `inbox.jsonl`. `topic` defaults to `bwoc/<recipient-id>/inbox`
    /// when omitted. No MQTT dependency reaches `bwoc-core` — these are plain
    /// strings; the publish lives in the `bwoc-mqtt` crate.
    Mqtt {
        broker: String,
        topic: Option<String>,
    },
}

/// Redact any `user:pass@` userinfo from a broker URL for safe display in logs
/// and listings — a broker may carry a password that must never be printed.
/// `mqtt://u:p@host:1883` becomes `mqtt://host:1883`; a URL without userinfo (or
/// without a scheme) is returned unchanged.
pub fn redact_broker(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let host = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
            format!("{scheme}://{host}")
        }
        None => url.to_string(),
    }
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

/// Validation failures for individual route entries. `route` labels the entry
/// by its `agent`/`namespace` key (a route need no longer carry a workspace).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RouteValidationError {
    #[error("route '{route}' has both 'agent' and 'namespace' set — exactly one is required")]
    BothKeys { route: String },
    #[error("route '{route}' has neither 'agent' nor 'namespace' — exactly one is required")]
    NeitherKey { route: String },
    #[error("route '{route}' uses transport 'local' but has no 'workspace'")]
    LocalMissingWorkspace { route: String },
    #[error("route '{route}' uses transport 'mqtt' but has no 'broker'")]
    MqttMissingBroker { route: String },
    #[error("route '{route}' has unknown transport '{transport}' (expected 'local' or 'mqtt')")]
    UnknownTransport { route: String, transport: String },
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
    /// Peer workspace root directory (absolute path). Required for the `local`
    /// transport; absent for `mqtt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
    /// Exact recipient id.
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    /// Namespace prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    /// Delivery transport: `local` (default) or `mqtt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
    /// MQTT broker URL (e.g. `mqtt://host:1883`). Required for `mqtt`.
    #[serde(skip_serializing_if = "Option::is_none")]
    broker: Option<String>,
    /// MQTT topic override. Defaults to `bwoc/<recipient-id>/inbox`.
    #[serde(skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
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
        match self.resolve_target(recipient_id)? {
            RouteTarget::Local(ws) => Some(ws),
            // An MQTT route has no local workspace path — callers that only
            // understand local delivery (e.g. `bwoc peer`) treat it as no match.
            RouteTarget::Mqtt { .. } => None,
        }
    }

    /// Resolve a recipient id to its delivery [`RouteTarget`] (local path or
    /// MQTT broker). Same precedence as [`Self::resolve`] — exact `agent` match
    /// first, then longest `namespace` prefix.
    pub fn resolve_target(&self, recipient_id: &str) -> Option<&RouteTarget> {
        // Pass 1: exact agent match.
        for route in &self.routes {
            if let RouteKind::Agent(id) = &route.kind {
                if id == recipient_id {
                    return Some(&route.target);
                }
            }
        }

        // Pass 2: longest namespace prefix match.
        let mut best: Option<(&str, &RouteTarget)> = None;
        for route in &self.routes {
            if let RouteKind::Namespace(ns) = &route.kind {
                if recipient_id.starts_with(ns.as_str()) {
                    let is_longer = best.is_none_or(|(prev, _)| ns.len() > prev.len());
                    if is_longer {
                        best = Some((ns.as_str(), &route.target));
                    }
                }
            }
        }
        best.map(|(_, t)| t)
    }
}

// ── Validation helper ─────────────────────────────────────────────────────────

fn validate_route(raw: RawRoute) -> Result<Route, RouteValidationError> {
    // Label the route by its match key for error messages (a route need not
    // carry a workspace anymore, so the old workspace-based label is gone).
    let route_label = raw
        .agent
        .clone()
        .or_else(|| raw.namespace.clone())
        .unwrap_or_default();

    let kind = match (raw.agent, raw.namespace) {
        (Some(_), Some(_)) => {
            return Err(RouteValidationError::BothKeys { route: route_label });
        }
        (None, None) => {
            return Err(RouteValidationError::NeitherKey { route: route_label });
        }
        (Some(id), None) => RouteKind::Agent(id),
        (None, Some(ns)) => RouteKind::Namespace(ns),
    };

    // Transport defaults to `local` so every pre-MQTT route (just `workspace`)
    // keeps its meaning unchanged.
    let target = match raw.transport.as_deref().unwrap_or("local") {
        "local" | "fs" => {
            let ws = raw
                .workspace
                .ok_or(RouteValidationError::LocalMissingWorkspace { route: route_label })?;
            RouteTarget::Local(PathBuf::from(ws))
        }
        "mqtt" => {
            let broker = raw
                .broker
                .ok_or(RouteValidationError::MqttMissingBroker { route: route_label })?;
            RouteTarget::Mqtt {
                broker,
                topic: raw.topic,
            }
        }
        other => {
            return Err(RouteValidationError::UnknownTransport {
                route: route_label,
                transport: other.to_string(),
            });
        }
    };

    Ok(Route { kind, target })
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
    fn redact_broker_strips_userinfo() {
        assert_eq!(
            redact_broker("mqtt://bwoc:s3cret@host:1883"),
            "mqtt://host:1883"
        );
        // no userinfo / no scheme → unchanged
        assert_eq!(redact_broker("mqtt://host:1883"), "mqtt://host:1883");
        assert_eq!(redact_broker("host:1883"), "host:1883");
        // user-only userinfo is still stripped
        assert_eq!(redact_broker("mqtt://bwoc@host"), "mqtt://host");
    }

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
        assert_eq!(
            routes.routes[0].target,
            RouteTarget::Local(PathBuf::from("/srv/ws-b"))
        );
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

    /// A `Route` delivering to a local peer workspace (the common test shape).
    fn local(ws: &str, kind: RouteKind) -> Route {
        Route {
            kind,
            target: RouteTarget::Local(PathBuf::from(ws)),
        }
    }

    #[test]
    fn resolve_exact_agent_match() {
        let routes = Routes {
            routes: vec![local("/peer/ws", RouteKind::Agent("agent-neo".into()))],
        };
        assert_eq!(routes.resolve("agent-neo"), Some(Path::new("/peer/ws")));
    }

    #[test]
    fn resolve_namespace_prefix_match() {
        let routes = Routes {
            routes: vec![local(
                "/team-b/ws",
                RouteKind::Namespace("agent-team-b".into()),
            )],
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
                local("/namespace/ws", RouteKind::Namespace("agent-neo".into())),
                local("/exact/ws", RouteKind::Agent("agent-neo".into())),
            ],
        };
        assert_eq!(routes.resolve("agent-neo"), Some(Path::new("/exact/ws")));
    }

    #[test]
    fn resolve_longest_namespace_wins() {
        let routes = Routes {
            routes: vec![
                local("/short/ws", RouteKind::Namespace("team".into())),
                local("/long/ws", RouteKind::Namespace("team-b".into())),
            ],
        };
        // "team-b-worker" matches both "team" and "team-b"; longest wins.
        assert_eq!(routes.resolve("team-b-worker"), Some(Path::new("/long/ws")));
    }

    #[test]
    fn resolve_no_match_returns_none() {
        let routes = Routes {
            routes: vec![local("/peer/ws", RouteKind::Agent("agent-neo".into()))],
        };
        assert_eq!(routes.resolve("agent-unknown"), None);
    }

    #[test]
    fn resolve_empty_routes_returns_none() {
        let routes = Routes::default();
        assert_eq!(routes.resolve("agent-anything"), None);
    }

    // ── MQTT transport ────────────────────────────────────────────────────────

    #[test]
    fn load_mqtt_route_parses_broker_and_topic() {
        let root = temp_ws("mqtt-route");
        write_routes(
            &root,
            r#"
[[route]]
agent = "agent-remote"
transport = "mqtt"
broker = "mqtt://broker.local:1883"
topic = "bwoc/agent-remote/inbox"
"#,
        );
        let routes = Routes::load(&root).unwrap();
        assert_eq!(
            routes.routes[0].target,
            RouteTarget::Mqtt {
                broker: "mqtt://broker.local:1883".into(),
                topic: Some("bwoc/agent-remote/inbox".into()),
            }
        );
        // An MQTT route has no local path → `resolve` (local-only) returns None,
        // while `resolve_target` surfaces the broker.
        assert_eq!(routes.resolve("agent-remote"), None);
        assert!(matches!(
            routes.resolve_target("agent-remote"),
            Some(RouteTarget::Mqtt { .. })
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mqtt_route_without_broker_is_error() {
        let root = temp_ws("mqtt-no-broker");
        write_routes(
            &root,
            "\n[[route]]\nagent = \"agent-remote\"\ntransport = \"mqtt\"\n",
        );
        let err = Routes::load(&root).unwrap_err();
        assert!(
            matches!(
                err,
                RoutingError::Validation(RouteValidationError::MqttMissingBroker { .. })
            ),
            "expected MqttMissingBroker, got {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_transport_is_error() {
        let root = temp_ws("bad-transport");
        write_routes(
            &root,
            "\n[[route]]\nagent = \"agent-x\"\ntransport = \"carrier-pigeon\"\n",
        );
        let err = Routes::load(&root).unwrap_err();
        assert!(
            matches!(
                err,
                RoutingError::Validation(RouteValidationError::UnknownTransport { .. })
            ),
            "expected UnknownTransport, got {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_route_without_workspace_is_error() {
        let root = temp_ws("local-no-ws");
        write_routes(&root, "\n[[route]]\nagent = \"agent-x\"\n");
        let err = Routes::load(&root).unwrap_err();
        assert!(
            matches!(
                err,
                RoutingError::Validation(RouteValidationError::LocalMissingWorkspace { .. })
            ),
            "expected LocalMissingWorkspace, got {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
