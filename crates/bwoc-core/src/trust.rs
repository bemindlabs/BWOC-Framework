//! Ingress trust labeling (Phase 5 — saṃvara, gate t1).
//!
//! Every item the model ever sees enters the agent's context as a message; this
//! module is the *provenance vocabulary* stamped on each one at the boundary so
//! a later policy gate (t2+) can refuse to let untrusted-derived content drive
//! an effectful action.
//!
//! ## Two types, one of them never stored
//!
//! - [`Principal`] — *who/what produced this content*. Immutable provenance,
//!   carried with the message and **persisted** (a reloaded session keeps the
//!   provenance it had). Pure-serde, so it adds zero dependencies to the lean
//!   `bwoc-core` crate (the dep-quarantine holds).
//! - [`TrustLevel`] — *derived* `{Trusted | Untrusted}`, computed from a
//!   `Principal` via [`Principal::trust`]. It is **never serialized, never
//!   defaulted, never stored** — trust is recomputed from provenance every time,
//!   so there is no independent mutable trust bit to flip and "promote to
//!   trusted" is unrepresentable.
//!
//! ## Fail-closed
//!
//! The default principal is [`Principal::Unknown`] (Untrusted). An *unlabeled*
//! message — a legacy session line written before this field existed, or a wire
//! input that omits provenance — deserializes (via `#[serde(default)]` on the
//! embedding field) to `Unknown` → Untrusted. A *mis*labeled message (a present
//! but unrecognized provenance tag) fails to deserialize and is rejected, which
//! is also closed. Neither path can yield a Trusted principal.
//!
//! ## What this is not
//!
//! `bwoc-agent`'s `trust.rs` (`SigningMode` / `TrustContext`) gates A2A *sender
//! identity* on the inbox envelope — it proves *who sent*, not *content
//! benignity*, and never reaches a context message. This module is the
//! orthogonal in-context taint primitive: it reuses that vocabulary, not its
//! code. A signed A2A sender is still [`TrustLevel::Untrusted`] here — a
//! signature proves *who*, not *safe*.

use serde::{Deserialize, Serialize};

/// Derived trust classification — **never persisted**.
///
/// Computed from a [`Principal`] by [`Principal::trust`]. Deliberately carries
/// no `Default`, `Serialize`, or `Deserialize` (yudi's t1 ruling): trust is
/// recomputed from provenance, never stored, so it cannot drift from the
/// `Principal` it was derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// The agent's own constitution or the local operator — may drive effect.
    Trusted,
    /// Anything off-box or model-derived — fail-closed; gated before effect.
    Untrusted,
}

/// Immutable provenance of a piece of context content.
///
/// Internally tagged on the `kind` field so the wire form is self-describing
/// (`{"kind":"tool","name":"read_file"}`). [`Principal::Unknown`] is the
/// `#[default]` and the fail-closed bucket: an absent provenance field
/// deserializes to it (via `#[serde(default)]` at the embedding site).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Principal {
    /// The human operator typing into the local TUI — the only Trusted ingress.
    LocalOperator,
    /// This agent's own system prompt / constitution — Trusted by definition.
    SelfAgent,
    /// This agent's own model turn. Untrusted: model output can reflect
    /// untrusted context (laundering); a later taint-propagation gate (t3)
    /// refines this. Kept distinct from [`SelfAgent`](Principal::SelfAgent) so
    /// the `role:System ⇔ principal:SelfAgent` invariant holds (an assistant
    /// turn is never `SelfAgent`).
    Assistant,
    /// Output of a local tool — untrusted external content.
    Tool { name: String },
    /// Output of a remote MCP tool — untrusted external content.
    McpTool { server: String, name: String },
    /// A teammate agent's message from the shared team chat — untrusted.
    TeamPeer { agent: String },
    /// A chat-connector platform user (Telegram / Discord / LINE) — untrusted.
    /// (`platform` rather than `kind`: the internal serde tag is already `kind`.)
    Platform { platform: String, user_id: i64 },
    /// An A2A sender. Untrusted even when `verified`: a signature proves *who*,
    /// not *safe*.
    A2aSender { from: String, verified: bool },
    /// Provenance not declared / not recognized — the fail-closed default.
    #[default]
    Unknown,
}

impl Principal {
    /// Derive the [`TrustLevel`]. **Exactly** the agent's own constitution and
    /// the local operator are Trusted; everything else — including signed A2A
    /// and the agent's own model turns — is Untrusted (the `_` arm is the
    /// fail-closed catch-all, so adding a variant can never silently become
    /// Trusted).
    pub fn trust(&self) -> TrustLevel {
        match self {
            Principal::LocalOperator | Principal::SelfAgent => TrustLevel::Trusted,
            _ => TrustLevel::Untrusted,
        }
    }

    /// True when this provenance is [`TrustLevel::Untrusted`].
    pub fn is_untrusted(&self) -> bool {
        self.trust() == TrustLevel::Untrusted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The complete provenance vocabulary, used to prove the trusted set is
    /// *exactly* `{LocalOperator, SelfAgent}` and that every other variant is
    /// fail-closed. Listing them here makes adding a variant a deliberate act:
    /// the `assert_eq!` count below breaks until the new variant is classified.
    fn all_variants() -> Vec<Principal> {
        vec![
            Principal::LocalOperator,
            Principal::SelfAgent,
            Principal::Assistant,
            Principal::Tool { name: "t".into() },
            Principal::McpTool {
                server: "s".into(),
                name: "t".into(),
            },
            Principal::TeamPeer { agent: "a".into() },
            Principal::Platform {
                platform: "telegram".into(),
                user_id: 1,
            },
            Principal::A2aSender {
                from: "peer".into(),
                verified: true,
            },
            Principal::Unknown,
        ]
    }

    #[test]
    fn exactly_two_principals_are_trusted() {
        let all = all_variants();
        assert_eq!(all.len(), 9, "vocabulary changed — re-audit trust()");
        let trusted: Vec<_> = all
            .iter()
            .filter(|p| p.trust() == TrustLevel::Trusted)
            .collect();
        assert_eq!(
            trusted,
            vec![&Principal::LocalOperator, &Principal::SelfAgent],
            "only the constitution and the local operator may be Trusted"
        );
    }

    #[test]
    fn default_is_unknown_and_untrusted() {
        assert_eq!(Principal::default(), Principal::Unknown);
        assert_eq!(Principal::default().trust(), TrustLevel::Untrusted);
    }

    #[test]
    fn every_variant_round_trips_through_serde() {
        // Unknown is a normal serializable variant (not `#[serde(other)]`), so
        // a persisted-then-reloaded message keeps its provenance verbatim.
        for p in all_variants() {
            let line = serde_json::to_string(&p).unwrap();
            let back: Principal = serde_json::from_str(&line).unwrap();
            assert_eq!(p, back, "round-trip failed for {p:?}");
        }
    }

    #[test]
    fn absent_field_fails_closed_to_untrusted() {
        // The legacy-disk / connector-omitted case: a struct whose principal is
        // `#[serde(default)]` and absent must default to Untrusted.
        #[derive(Deserialize)]
        struct Carrier {
            #[serde(default)]
            principal: Principal,
        }
        let c: Carrier = serde_json::from_str("{}").unwrap();
        assert_eq!(c.principal.trust(), TrustLevel::Untrusted);
    }

    proptest! {
        /// Fuzz the serde boundary: for an arbitrary `kind` tag, parsing either
        /// fails (rejected = closed) or yields a principal — and it may only be
        /// Trusted when the tag is exactly one of the two literal trusted tags.
        /// No other byte string can deserialize into a Trusted principal.
        #[test]
        fn no_unexpected_tag_deserializes_to_trusted(kind in ".*") {
            let json = format!(r#"{{"kind":{}}}"#, serde_json::to_string(&kind).unwrap());
            if let Ok(p) = serde_json::from_str::<Principal>(&json) {
                if p.trust() == TrustLevel::Trusted {
                    prop_assert!(
                        kind == "local_operator" || kind == "self_agent",
                        "tag {kind:?} deserialized to a Trusted principal"
                    );
                }
            }
        }

        /// Fuzz arbitrary bytes through the parser: no input may produce a
        /// Trusted principal that is not one of the two trusted variants.
        #[test]
        fn arbitrary_bytes_never_launder_to_trusted(s in ".*") {
            if let Ok(p) = serde_json::from_str::<Principal>(&s) {
                if p.trust() == TrustLevel::Trusted {
                    prop_assert!(matches!(
                        p,
                        Principal::LocalOperator | Principal::SelfAgent
                    ));
                }
            }
        }
    }
}
