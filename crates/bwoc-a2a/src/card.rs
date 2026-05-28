//! Agent Card generation (A2A 1.0.0) — render `/.well-known/agent-card.json`
//! from a BWOC `config.manifest.json`. P1 of #48.

use bwoc_core::manifest::Manifest;

use crate::types::{AgentCapabilities, AgentCard, AgentSkill};

/// A2A protocol version this build targets (pinned — see `types`).
pub const A2A_PROTOCOL_VERSION: &str = "1.0.0";

/// Build an [`AgentCard`] from an agent's manifest + the base `url` its A2A
/// endpoint is served at. v1 advertises a single skill derived from the
/// agent's role/scope, JSON-RPC (text) modes, streaming via SSE (P3), and
/// push-config management (P5; webhook delivery itself lands with the auth phase).
pub fn card_from_manifest(m: &Manifest, url: &str) -> AgentCard {
    let skill = AgentSkill {
        id: m.agent_id.clone(),
        name: m.agent_role.clone(),
        description: m
            .scope_description
            .clone()
            .unwrap_or_else(|| m.agent_role.clone()),
        tags: vec!["bwoc".to_string()],
    };
    AgentCard {
        name: m.name.clone(),
        description: m.agent_role.clone(),
        url: url.to_string(),
        version: m.version.clone(),
        protocol_version: A2A_PROTOCOL_VERSION.to_string(),
        capabilities: AgentCapabilities {
            streaming: true,          // P3: SendStreamingMessage + SubscribeToTask (SSE)
            push_notifications: true, // P5: push-config CRUD (delivery: auth phase)
        },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills: vec![skill],
        // No auth by default; the serve layer calls `with_bearer_security()`
        // when a token is configured (AP1).
        security_schemes: None,
        security: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        // Minimal manifest via JSON to avoid pinning every field here.
        serde_json::from_str(
            r#"{"name":"oracle","agentId":"agent-oracle","agentRole":"architecture reviewer",
                "primaryModel":"m","memoryPath":"memories/","scopeDescription":"reviews designs",
                "lintCmd":"true","formatCmd":"true","testCmd":"true","buildCmd":"true",
                "version":"2.0"}"#,
        )
        .unwrap()
    }

    #[test]
    fn card_maps_manifest_fields() {
        let c = card_from_manifest(&manifest(), "http://localhost:41241");
        assert_eq!(c.name, "oracle");
        assert_eq!(c.protocol_version, "1.0.0");
        assert_eq!(c.url, "http://localhost:41241");
        assert_eq!(c.skills.len(), 1);
        assert_eq!(c.skills[0].id, "agent-oracle");
        assert_eq!(c.skills[0].description, "reviews designs");
        assert!(c.capabilities.streaming); // P3: SSE streaming advertised
        assert!(c.capabilities.push_notifications); // P5: push-config CRUD advertised
        // Serializes with A2A camelCase field names.
        let j = serde_json::to_string(&c).unwrap();
        assert!(j.contains("\"protocolVersion\":\"1.0.0\""));
        assert!(j.contains("\"defaultInputModes\":[\"text/plain\"]"));
    }
}
