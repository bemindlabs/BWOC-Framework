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
    ///
    /// DEFERRED (gate t3, condition c): a *source-aware* taint field on
    /// `Tool` / `McpTool` (e.g. distinguishing a read confined to the agent's
    /// own worktree from one that pulled in off-box bytes) is intentionally not
    /// added here yet. t3 treats every tool output as one undifferentiated
    /// Untrusted bucket — fail-closed and simpler. Refining this into a graded,
    /// source-aware taint is the next gate; do it by adding a field to these two
    /// variants and threading it through [`Principal::carries_untrusted_taint`].
    Tool { name: String },
    /// Output of a remote MCP tool — untrusted external content.
    /// (See the [`Tool`](Principal::Tool) defer marker for the source-aware
    /// taint refinement deferred past t3.)
    McpTool { server: String, name: String },
    /// A teammate agent's message from the shared team chat — untrusted.
    TeamPeer { agent: String },
    /// A chat-connector platform user (Telegram / Discord / LINE) — untrusted.
    /// (`platform` rather than `kind`: the internal serde tag is already `kind`.)
    Platform { platform: String, user_id: i64 },
    /// An A2A sender. Untrusted even when `verified`: a signature proves *who*,
    /// not *safe*.
    A2aSender { from: String, verified: bool },
    /// A compaction summary note that folded an earlier window of the history
    /// (gate t3). `tainted` is the **max-taint** of the folded window: `true`
    /// iff any folded message carried untrusted taint
    /// ([`Principal::carries_untrusted_taint`]). It is the ONLY non-[`SelfAgent`]
    /// principal permitted to wear a `System` role (the compaction note is
    /// folded as a system message), which is why the t3 invariant relaxes
    /// `System ⇔ SelfAgent` to the one-way `SelfAgent ⇒ System`.
    ///
    /// A `Summary` is **never** [`TrustLevel::Trusted`] — not even when
    /// `tainted` is `false`. [`trust`](Principal::trust) is fail-closed here so a
    /// *forged* `{"kind":"summary","tainted":false}` on disk or the wire can
    /// never launder into Trusted. The `tainted` flag instead feeds
    /// [`carries_untrusted_taint`](Principal::carries_untrusted_taint), which the
    /// per-turn scan uses: a clean summary does not force a turn Untrusted, but
    /// it can never *vouch* its way up to the constitution's trust either.
    Summary { tainted: bool },
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

    /// Whether this provenance carries **untrusted taint** for the
    /// taint-propagation scan and compaction max-taint fold (gate t3).
    ///
    /// For a [`Summary`](Principal::Summary) this is its `tainted` max-taint
    /// flag — a *clean* summary (a fold of trusted-only content) carries no
    /// taint, so it does not force a turn Untrusted. For every other principal
    /// it is exactly `trust() == Untrusted`.
    ///
    /// This is deliberately **distinct from [`trust`](Principal::trust)**, which
    /// is *always* Untrusted for a `Summary` (fail-closed against a forged
    /// untainted summary laundering to Trusted). Use this — never `trust()` — to
    /// decide whether a summary contributes taint, so a clean summary is not
    /// over-tainted into latching every post-compaction session Untrusted.
    pub fn carries_untrusted_taint(&self) -> bool {
        match self {
            Principal::Summary { tainted } => *tainted,
            other => other.trust() == TrustLevel::Untrusted,
        }
    }
}

/// Whether the harness can enforce its per-turn tool-confinement (Phase 5 — the
/// L1–L7 capability gate, FS jail, and egress filter) on a given **spawn
/// backend**.
///
/// This is orthogonal to [`TrustLevel`] (which classifies *content
/// provenance*). It classifies *where tool execution happens*:
///
/// - [`Confined`](BackendTrust::Confined) — the model returns `tool_calls` and
///   **the harness executes them**, so every effectful action passes the policy
///   gate ([`crate`] consumers: `bwoc_harness::policy`) and the per-turn jail.
///   The `#271` guarantee — *an Untrusted turn is effectively read-only* — holds.
/// - [`Ambient`](BackendTrust::Ambient) — the backend is a vendor subprocess
///   that runs its **own** tools internally (`backend = "cli"`; see
///   `bwoc_harness::provider::cli`). Tool execution escapes the harness
///   entirely: no capability gate, no jail, no egress filter reach it. The
///   `#271` read-only guarantee is **structurally unenforceable** here — not
///   merely weakened. Untrusted ingress must never be auto-processed on such a
///   backend (`bwoc-agent`'s gateway auto-process refuses it), and an
///   interactive operator is loudly warned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendTrust {
    /// Tool execution stays inside the harness — Phase 5 confinement applies.
    Confined,
    /// Tool execution escapes into a vendor subprocess — confinement does NOT
    /// apply (full ambient authority).
    Ambient,
}

impl BackendTrust {
    /// True for [`Ambient`](BackendTrust::Ambient) — the harness cannot confine
    /// this backend's tool use.
    pub fn is_ambient(self) -> bool {
        self == BackendTrust::Ambient
    }
}

/// Classify a spawn-backend string into its [`BackendTrust`] tier.
///
/// **Known-ambient allowlist, fail-*safe* by construction.** Only `"cli"`
/// relocates tool execution out of the harness today, so it is the sole
/// [`Ambient`](BackendTrust::Ambient) value; every other backend (`claude`,
/// `anthropic`, `openrouter`, `ollama`, `openai-compatible`, and the
/// HTTP-routed vendor aliases) drives the harness's own tool loop and is
/// [`Confined`](BackendTrust::Confined). An *unknown* backend is treated as
/// Confined because the harness routes it to the OpenAI-compatible HTTP client
/// (`build_provider`'s `_` arm), where the harness still owns tool execution.
///
/// ⚠ **Contract for new backends:** any future backend whose tools execute
/// *outside* the harness (another delegated-CLI/agent provider) MUST be added to
/// the `Ambient` arm here — the classification is the single source of truth the
/// auto-process refusal, the operator warning, and `bwoc status`/`bwoc list`
/// all read. The companion test `cli_is_the_only_ambient_backend` pins this.
pub fn backend_trust_tier(backend: &str) -> BackendTrust {
    match backend {
        "cli" => BackendTrust::Ambient,
        _ => BackendTrust::Confined,
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
            // Listed in its `tainted` form: a Summary is never Trusted (see
            // `exactly_two_principals_are_trusted`), and this representative
            // proves it stays out of the trusted set.
            Principal::Summary { tainted: true },
            Principal::Unknown,
        ]
    }

    #[test]
    fn exactly_two_principals_are_trusted() {
        let all = all_variants();
        assert_eq!(all.len(), 10, "vocabulary changed — re-audit trust()");
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
    fn cli_is_the_only_ambient_backend() {
        // `cli` relocates tool execution into a vendor subprocess → ambient.
        assert!(backend_trust_tier("cli").is_ambient());
        // Every harness-confined backend keeps Phase 5 tool-confinement.
        for b in [
            "claude",
            "anthropic",
            "openrouter",
            "ollama",
            "openai-compatible",
            "agy",
            "codex",
            "kimi",
        ] {
            assert_eq!(
                backend_trust_tier(b),
                BackendTrust::Confined,
                "{b} must stay Confined — its tools run inside the harness"
            );
        }
        // Fail-safe: an unknown backend routes to the HTTP client (harness owns
        // the tool loop), so it is Confined, not silently Ambient.
        assert_eq!(backend_trust_tier("something-new"), BackendTrust::Confined);
    }

    #[test]
    fn summary_is_never_trusted_even_when_clean() {
        // A Summary is fail-closed at the trust() boundary regardless of its
        // taint flag — this is what stops a forged untainted summary on disk /
        // the wire from laundering into Trusted (see the proptests below).
        assert_eq!(
            Principal::Summary { tainted: true }.trust(),
            TrustLevel::Untrusted
        );
        assert_eq!(
            Principal::Summary { tainted: false }.trust(),
            TrustLevel::Untrusted
        );
    }

    #[test]
    fn carries_untrusted_taint_reads_summary_flag_but_trust_for_others() {
        // For a Summary, taint follows the max-taint flag (NOT trust()): a clean
        // fold carries none, a dirty fold carries it.
        assert!(!Principal::Summary { tainted: false }.carries_untrusted_taint());
        assert!(Principal::Summary { tainted: true }.carries_untrusted_taint());
        // For every other principal it is exactly `trust() == Untrusted`.
        assert!(!Principal::LocalOperator.carries_untrusted_taint());
        assert!(!Principal::SelfAgent.carries_untrusted_taint());
        assert!(Principal::Tool { name: "t".into() }.carries_untrusted_taint());
        assert!(Principal::Unknown.carries_untrusted_taint());
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
