//! `config.manifest.json` types and I/O for an incarnated agent.
//!
//! The TEMPLATE's `config.manifest.json` is a schema document (lists
//! `requiredConfig` fields with type/description/default). An INCARNATED
//! agent's `config.manifest.json` is a flat resolved document with the
//! agent's concrete values. This module models the resolved form.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Resolved agent manifest. Every required field carries the value the
/// agent author supplied at incarnation time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
    #[serde(rename = "agentRole")]
    pub agent_role: String,
    #[serde(rename = "primaryModel")]
    pub primary_model: String,
    #[serde(rename = "fallbackModel", skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
    /// Candidate model pool for `primaryModel: "auto"` resolution.
    ///
    /// Ordered by operator preference (first = most preferred). Required, and
    /// only meaningful, when `primary_model == "auto"`: the harness probes
    /// these against the live provider and picks one by availability,
    /// context-fit, task class, and cost (preference order is the cost
    /// tie-break). Ignored when `primary_model` names a concrete model.
    #[serde(
        rename = "autoModels",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_models: Option<Vec<String>>,
    /// Reasoning-effort level passed through to the active backend, if it
    /// supports one.
    ///
    /// Backend-neutral and free-form: the **value space is backend-specific**,
    /// so this carries the operator's literal string rather than a fixed
    /// mapping. The OpenAI-compatible harness sends this as
    /// `reasoning_effort` on completion requests; backends without an effort
    /// control ignore it. `None` = leave the backend on its own default.
    #[serde(
        rename = "reasoningEffort",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_effort: Option<String>,
    #[serde(rename = "memoryPath")]
    pub memory_path: String,
    #[serde(rename = "sessionsPath", skip_serializing_if = "Option::is_none")]
    pub sessions_path: Option<String>,
    #[serde(rename = "deepMemoryCmd", skip_serializing_if = "Option::is_none")]
    pub deep_memory_cmd: Option<String>,
    #[serde(rename = "lintCmd")]
    pub lint_cmd: String,
    #[serde(rename = "formatCmd")]
    pub format_cmd: String,
    #[serde(rename = "testCmd")]
    pub test_cmd: String,
    #[serde(rename = "buildCmd")]
    pub build_cmd: String,
    #[serde(rename = "worktreeBase", skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    /// One-line description of what the agent DOES. Fills `{{scopeDescription}}`
    /// in AGENTS.md and persona/README.md at incarnation time.
    #[serde(rename = "scopeDescription", skip_serializing_if = "Option::is_none")]
    pub scope_description: Option<String>,
    /// One-line description of what the agent DOES NOT do. Fills
    /// `{{outOfScope}}` in AGENTS.md and persona/README.md.
    #[serde(rename = "outOfScope", skip_serializing_if = "Option::is_none")]
    pub out_of_scope: Option<String>,
    /// Which spawn backend this agent uses.
    ///
    /// Accepted values: `"claude"` | `"agy"` | `"codex"` | `"kimi"` |
    /// `"ollama"` | `"openai-compatible"`.
    ///
    /// Required for `openai-compatible`; optional/ignored for vendor backends
    /// that are selected on the CLI with `--backend`. When present, `bwoc
    /// spawn` can auto-select the backend without an explicit flag.
    #[serde(rename = "backend", skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Base URL of the OpenAI-compatible inference endpoint.
    ///
    /// Required when `backend` is `"openai-compatible"`. Passed to the harness
    /// as `--endpoint <baseUrl>`. Ignored for vendor backends that use their
    /// own CLI.
    ///
    /// Example: `"https://api.openai.com/v1"` or `"http://localhost:8000/v1"`.
    #[serde(rename = "baseUrl", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Inter-agent trust declaration per the Kalyāṇamitta 7 spec
    /// (`modules/agent-template/interconnect/trust.md`). Absent block
    /// means "no qualities declared, no qualities required" — the
    /// framework ships permissive by default; recipients opt in via
    /// `requiredTrust`. See `TrustBlock` for shape + the `Default`
    /// impl for how missing-field semantics resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<TrustBlock>,
    pub version: String,
}

/// Refusal mode for the trust gate (`trust.mode` manifest field).
///
/// Controls how the daemon responds when an incoming envelope's sender
/// is missing one or more of the recipient's `requiredTrust` qualities.
///
/// **Backward-compat rule (Anicca / no silent security regression):**
/// When the `mode` field is **absent** from the manifest, the framework
/// computes an *effective mode* based on v1 behaviour:
/// - empty `requiredTrust` → `Off` (was: gate inert, pass-all)
/// - non-empty `requiredTrust` → `Refuse` (was: refuse on missing quality)
///
/// `Warn` is strictly opt-in: the agent author must write
/// `"mode": "warn"` explicitly. Existing agents without the field are
/// never silently flipped from `Refuse` to `Warn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RefusalMode {
    /// Gate is inert — envelope always passes, no log entry.
    /// Effective mode when `requiredTrust` is empty and `mode` is absent.
    #[default]
    Off,
    /// Envelope passes but the daemon emits a `trust_warn` log line
    /// naming the sender and missing qualities. Opt-in only.
    Warn,
    /// Envelope is refused: marked in `inbox.refusals.jsonl`, never
    /// deleted. Effective mode when `requiredTrust` is non-empty and
    /// `mode` is absent (v1 behaviour preserved).
    Refuse,
}

/// Kalyāṇamitta 7 trust block. Two halves: `declared` (what this agent
/// claims about itself) and `required_trust` (what this agent demands
/// from peers that want to message it). They're independent — see
/// `interconnect/trust.md` §"Manifest Schema".
///
/// `schema_version` is currently 1. Future spec revisions may add
/// fields to `TrustDeclared`; per the spec, missing fields in declared
/// are treated as `false` (Anicca seam). `serde(default)` on each
/// boolean implements this — a v2 agent's manifest with extra fields
/// deserializes cleanly on v1; a v1 manifest deserializing on v2
/// silently gets `false` for unknown new fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustBlock {
    /// Always `1` for this spec revision. Future spec bumps increment.
    /// Required field (no `default`) — a malformed manifest without
    /// it fails to load, which is the right escalation when trust
    /// semantics ride on the block being well-formed.
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    /// 7 booleans per the AN 7.36 (Mitta Sutta) canonical list. Missing
    /// fields → `false` per the spec.
    #[serde(default)]
    pub declared: TrustDeclared,
    /// Qualities required from peer senders. Empty vec ≡ no gating
    /// for this recipient. Names match the camelCase manifest keys
    /// in `TrustDeclared`.
    #[serde(
        rename = "requiredTrust",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub required_trust: Vec<String>,
    /// Optional explicit refusal mode. When absent, the effective mode
    /// is computed from v1 rules: `Off` if `requiredTrust` is empty,
    /// `Refuse` if non-empty. `Warn` is strictly opt-in.
    #[serde(rename = "mode", default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RefusalMode>,
    /// ed25519 public key (lowercase hex, 64 chars) used to verify this
    /// agent's signed message envelopes (HV2-4 / `docs/en/SIGNING.en.md`).
    /// `None` = no key published yet (the agent can't be authenticated; the
    /// signing mode decides whether that is refused). Set by `bwoc trust
    /// keygen`; the matching private key lives in `<agent>/.bwoc/agent.key`.
    #[serde(
        rename = "signingPublicKey",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub signing_public_key: Option<String>,
}

impl TrustBlock {
    /// Compute the *effective* `RefusalMode`.
    ///
    /// If `mode` is explicitly set, that value governs.
    /// Otherwise, fall back to v1 rules so existing agents are
    /// byte-for-byte compatible (Anicca — no silent regression):
    /// - empty `requiredTrust` → `Off`
    /// - non-empty `requiredTrust` → `Refuse`
    pub fn effective_mode(&self) -> RefusalMode {
        if let Some(m) = self.mode {
            return m;
        }
        if self.required_trust.is_empty() {
            RefusalMode::Off
        } else {
            RefusalMode::Refuse
        }
    }
}

impl Default for TrustBlock {
    fn default() -> Self {
        Self {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: Vec::new(),
            mode: None,
            signing_public_key: None,
        }
    }
}

/// The 7 Kalyāṇamitta qualities as declared booleans. Each is
/// `#[serde(default)]` so missing fields deserialize as `false` —
/// implements the spec's Anicca seam: a v2 spec adding `mudu` doesn't
/// silently refuse v1 peers who never declared it. Names match the
/// camelCase manifest keys: piyo / garu / bhavaniyo / vatta /
/// vacanakkhamo / gambhira / noCatthana.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TrustDeclared {
    /// Piyo — pleasant to delegate to.
    #[serde(default)]
    pub piyo: bool,
    /// Garu — respectable in capability.
    #[serde(default)]
    pub garu: bool,
    /// Bhāvanīyo — helps us improve.
    #[serde(default)]
    pub bhavaniyo: bool,
    /// Vattā — speaks beneficial truth.
    #[serde(default)]
    pub vatta: bool,
    /// Vacanakkhamo — can take feedback.
    #[serde(default)]
    pub vacanakkhamo: bool,
    /// Gambhīrañca kathaṃ kattā — can explain depth.
    #[serde(default)]
    pub gambhira: bool,
    /// No caṭṭhāne niyojaye — does not lead astray.
    #[serde(rename = "noCatthana", default)]
    pub no_catthana: bool,
}

impl TrustDeclared {
    /// Read a quality by its camelCase manifest key. Unknown keys
    /// resolve to `false` — implements the spec's "missing fields in
    /// declared → false" rule (interconnect/trust.md §schemaVersion).
    pub fn has(&self, key: &str) -> bool {
        match key {
            "piyo" => self.piyo,
            "garu" => self.garu,
            "bhavaniyo" => self.bhavaniyo,
            "vatta" => self.vatta,
            "vacanakkhamo" => self.vacanakkhamo,
            "gambhira" => self.gambhira,
            "noCatthana" => self.no_catthana,
            _ => false,
        }
    }
}

impl TrustBlock {
    /// Given a peer's declaration, return the qualities this block's
    /// `required_trust` demands that the peer does NOT satisfy. Result
    /// preserves the order of `required_trust`. Empty result ≡ peer
    /// satisfies every required quality (no refusal).
    pub fn missing_in(&self, declared: &TrustDeclared) -> Vec<String> {
        self.required_trust
            .iter()
            .filter(|q| !declared.has(q))
            .cloned()
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("io error reading manifest: {0}")]
    Io(#[from] io::Error),
    #[error("invalid JSON in manifest: {0}")]
    Json(#[from] serde_json::Error),
}

impl Manifest {
    /// Parse a manifest from a JSON file on disk.
    pub fn load_from_path(path: &Path) -> Result<Self, ManifestError> {
        let content = fs::read_to_string(path)?;
        let m = serde_json::from_str(&content)?;
        Ok(m)
    }

    /// Serialize the manifest as pretty JSON and write to `path`.
    pub fn save_to_path(&self, path: &Path) -> Result<(), ManifestError> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, format!("{json}\n"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            name: "demo".into(),
            agent_id: "agent-demo".into(),
            agent_role: "demo agent".into(),
            primary_model: "model-x".into(),
            fallback_model: None,
            auto_models: None,
            reasoning_effort: None,
            memory_path: "memories/".into(),
            sessions_path: None,
            deep_memory_cmd: None,
            lint_cmd: "true".into(),
            format_cmd: "true".into(),
            test_cmd: "true".into(),
            build_cmd: "true".into(),
            worktree_base: None,
            scope_description: None,
            out_of_scope: None,
            backend: None,
            base_url: None,
            trust: None,
            version: "2.0".into(),
        }
    }

    #[test]
    fn roundtrip_json() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn camel_case_keys() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"agentId\""));
        assert!(json.contains("\"primaryModel\""));
        assert!(json.contains("\"lintCmd\""));
        // Optional none fields skipped
        assert!(!json.contains("\"fallbackModel\""));
        // Trust block absent → no "trust" key in serialized output.
        assert!(!json.contains("\"trust\""));
    }

    /// `autoModels` is optional: absent → `None` and omitted on serialize;
    /// present → preserved in order. Drives `primaryModel: "auto"` resolution.
    #[test]
    fn auto_models_serde() {
        // Absent → None, not serialized.
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("\"autoModels\""));
        assert!(m.auto_models.is_none());

        // Present → order preserved through roundtrip.
        let mut m2 = sample();
        m2.primary_model = "auto".into();
        m2.auto_models = Some(vec!["big".into(), "small".into()]);
        let json2 = serde_json::to_string(&m2).unwrap();
        assert!(json2.contains("\"autoModels\":[\"big\",\"small\"]"));
        let back: Manifest = serde_json::from_str(&json2).unwrap();
        assert_eq!(back.auto_models, Some(vec!["big".into(), "small".into()]));
    }

    /// `reasoningEffort` is optional: absent → `None` and omitted on
    /// serialize; present → preserved verbatim (free-form, backend-mapped).
    #[test]
    fn reasoning_effort_serde() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        assert!(!json.contains("\"reasoningEffort\""));
        assert!(m.reasoning_effort.is_none());

        let mut m2 = sample();
        m2.reasoning_effort = Some("max".into());
        let json2 = serde_json::to_string(&m2).unwrap();
        assert!(json2.contains("\"reasoningEffort\":\"max\""));
        let back: Manifest = serde_json::from_str(&json2).unwrap();
        assert_eq!(back.reasoning_effort, Some("max".into()));
    }

    // ---- TrustBlock tests ---------------------------------------------------

    /// Backward-compat: a manifest without a `trust` block deserializes
    /// fine with `trust = None`. This is the most important test — every
    /// existing agent's manifest predates the trust spec.
    #[test]
    fn trust_block_absent_is_none() {
        let json = r#"{
            "name": "demo", "agentId": "agent-demo", "agentRole": "x",
            "primaryModel": "m", "memoryPath": "memories/",
            "lintCmd": "true", "formatCmd": "true", "testCmd": "true",
            "buildCmd": "true", "version": "2.0"
        }"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert!(m.trust.is_none());
    }

    /// Full-block roundtrip — every quality declared, requiredTrust array.
    #[test]
    fn trust_block_full_roundtrip() {
        let mut m = sample();
        m.trust = Some(TrustBlock {
            schema_version: 1,
            declared: TrustDeclared {
                piyo: true,
                garu: false,
                bhavaniyo: true,
                vatta: true,
                vacanakkhamo: true,
                gambhira: false,
                no_catthana: true,
            },
            required_trust: vec!["vatta".into(), "noCatthana".into()],
            mode: None,
            signing_public_key: None,
        });
        let json = serde_json::to_string(&m).unwrap();
        // Wire format uses camelCase + the rename for noCatthana.
        assert!(json.contains("\"trust\""));
        assert!(json.contains("\"schemaVersion\":1"));
        assert!(json.contains("\"noCatthana\":true"));
        assert!(json.contains("\"requiredTrust\":[\"vatta\",\"noCatthana\"]"));
        // Roundtrip preserves every boolean.
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    /// Missing-field rule (Anicca seam): a partial `declared` block
    /// with only 2 fields → other 5 are `false`. Critical because a
    /// v2 spec that adds quality `mudu` must not silently refuse all
    /// v1 manifests that never declared it.
    #[test]
    fn trust_declared_partial_missing_fields_are_false() {
        let json = r#"{ "piyo": true, "vatta": true }"#;
        let d: TrustDeclared = serde_json::from_str(json).unwrap();
        assert!(d.piyo);
        assert!(d.vatta);
        // The other 5 default to false.
        assert!(!d.garu);
        assert!(!d.bhavaniyo);
        assert!(!d.vacanakkhamo);
        assert!(!d.gambhira);
        assert!(!d.no_catthana);
    }

    /// `requiredTrust` empty array is the same as missing — both serialize
    /// out of the picture (`skip_serializing_if = Vec::is_empty`) and both
    /// deserialize back to the empty vec.
    #[test]
    fn required_trust_empty_skipped_on_serialize() {
        let block = TrustBlock::default();
        let json = serde_json::to_string(&block).unwrap();
        assert!(!json.contains("\"requiredTrust\""));
        let back: TrustBlock = serde_json::from_str(&json).unwrap();
        assert!(back.required_trust.is_empty());
    }

    /// Forward-compat: a v2 manifest that adds an unknown field to
    /// `TrustDeclared` deserializes cleanly on v1 (serde ignores unknown
    /// JSON keys by default). This is the other half of the Anicca seam.
    #[test]
    fn trust_declared_unknown_field_ignored() {
        let json = r#"{ "piyo": true, "mudu": true, "futureField": "anything" }"#;
        let d: TrustDeclared = serde_json::from_str(json).unwrap();
        assert!(d.piyo);
        // Unknown fields don't error or attach.
    }

    // ---- Refusal-helper tests (step 4) --------------------------------------

    #[test]
    fn has_returns_declared_value_for_known_keys() {
        let d = TrustDeclared {
            vatta: true,
            no_catthana: true,
            ..Default::default()
        };
        assert!(d.has("vatta"));
        assert!(d.has("noCatthana"));
        assert!(!d.has("piyo"));
    }

    #[test]
    fn has_returns_false_for_unknown_key() {
        let d = TrustDeclared {
            piyo: true,
            ..Default::default()
        };
        // Future spec quality not yet known — must be false, not panic.
        assert!(!d.has("mudu"));
        assert!(!d.has("")); // empty key
        assert!(!d.has("PIYO")); // case-sensitive — wrong case is unknown
    }

    #[test]
    fn missing_in_empty_required_returns_empty() {
        let block = TrustBlock::default();
        let declared = TrustDeclared::default();
        assert!(block.missing_in(&declared).is_empty());
    }

    #[test]
    fn missing_in_all_satisfied_returns_empty() {
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec!["vatta".into(), "noCatthana".into()],
            mode: None,
            signing_public_key: None,
        };
        let peer = TrustDeclared {
            vatta: true,
            no_catthana: true,
            ..Default::default()
        };
        assert!(block.missing_in(&peer).is_empty());
    }

    #[test]
    fn missing_in_partial_returns_only_missing() {
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec!["vatta".into(), "noCatthana".into(), "gambhira".into()],
            mode: None,
            signing_public_key: None,
        };
        let peer = TrustDeclared {
            vatta: true,
            // no_catthana: false → missing
            // gambhira: false → missing
            ..Default::default()
        };
        let missing = block.missing_in(&peer);
        assert_eq!(missing, vec!["noCatthana", "gambhira"]);
    }

    #[test]
    fn missing_in_preserves_required_order() {
        // Order in the required_trust array is the order reported back —
        // recipient's preferences drive the surfaced diagnostic.
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec!["gambhira".into(), "vatta".into(), "piyo".into()],
            mode: None,
            signing_public_key: None,
        };
        let peer = TrustDeclared::default(); // nothing declared
        assert_eq!(block.missing_in(&peer), vec!["gambhira", "vatta", "piyo"]);
    }

    #[test]
    fn missing_in_unknown_quality_is_always_missing() {
        // A recipient that requires a future-spec quality the sender's
        // v1 manifest doesn't know about → quality is missing (since
        // unknown → false). Forward-compat works as expected.
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec!["mudu".into()], // future-spec quality
            mode: None,
            signing_public_key: None,
        };
        let peer = TrustDeclared {
            piyo: true,
            vatta: true,
            ..Default::default()
        };
        assert_eq!(block.missing_in(&peer), vec!["mudu"]);
    }

    // ---- RefusalMode + effective_mode tests ---------------------------------

    /// v1 backward-compat: absent mode + empty requiredTrust → Off.
    #[test]
    fn effective_mode_off_when_no_mode_and_empty_required() {
        let block = TrustBlock::default(); // mode: None, required_trust: []
        assert_eq!(block.effective_mode(), RefusalMode::Off);
    }

    /// v1 backward-compat: absent mode + non-empty requiredTrust → Refuse.
    /// This is the critical regression guard — existing manifests with
    /// requiredTrust but no explicit mode must keep refusing, not warn-passing.
    #[test]
    fn effective_mode_refuse_when_no_mode_and_nonempty_required() {
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec!["vatta".into()],
            mode: None,
            signing_public_key: None,
        };
        assert_eq!(block.effective_mode(), RefusalMode::Refuse);
    }

    /// Explicit mode overrides the v1 inference — warn is strictly opt-in.
    #[test]
    fn effective_mode_explicit_warn_overrides() {
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec!["vatta".into()],
            mode: Some(RefusalMode::Warn),
            signing_public_key: None,
        };
        assert_eq!(block.effective_mode(), RefusalMode::Warn);
    }

    /// Explicit Refuse survives even with empty requiredTrust (unusual but
    /// valid — caller declared mode explicitly).
    #[test]
    fn effective_mode_explicit_refuse_with_empty_required() {
        let block = TrustBlock {
            schema_version: 1,
            declared: TrustDeclared::default(),
            required_trust: vec![],
            mode: Some(RefusalMode::Refuse),
            signing_public_key: None,
        };
        assert_eq!(block.effective_mode(), RefusalMode::Refuse);
    }

    /// `"mode": "warn"` round-trips through serde as the camelCase string.
    #[test]
    fn refusal_mode_serde_roundtrip() {
        let json = r#""warn""#;
        let m: RefusalMode = serde_json::from_str(json).unwrap();
        assert_eq!(m, RefusalMode::Warn);
        assert_eq!(serde_json::to_string(&m).unwrap(), json);

        let json_off = r#""off""#;
        let m_off: RefusalMode = serde_json::from_str(json_off).unwrap();
        assert_eq!(m_off, RefusalMode::Off);

        let json_refuse = r#""refuse""#;
        let m_refuse: RefusalMode = serde_json::from_str(json_refuse).unwrap();
        assert_eq!(m_refuse, RefusalMode::Refuse);
    }

    /// `mode` field absent from trust block → `None`; `mode: "warn"` serializes
    /// into the block; `mode: null` or missing both round-trip to `None`.
    #[test]
    fn trust_block_mode_field_serde() {
        // Absent → None
        let json_no_mode = r#"{"schemaVersion":1}"#;
        let b: TrustBlock = serde_json::from_str(json_no_mode).unwrap();
        assert!(b.mode.is_none());

        // Present → Some(Warn)
        let json_warn = r#"{"schemaVersion":1,"mode":"warn"}"#;
        let b2: TrustBlock = serde_json::from_str(json_warn).unwrap();
        assert_eq!(b2.mode, Some(RefusalMode::Warn));

        // Roundtrip with mode=Warn serializes the field
        let serialized = serde_json::to_string(&b2).unwrap();
        assert!(serialized.contains("\"mode\":\"warn\""));

        // Roundtrip without mode omits the field (skip_serializing_if = None)
        let serialized_no_mode = serde_json::to_string(&b).unwrap();
        assert!(!serialized_no_mode.contains("\"mode\""));
    }
}
