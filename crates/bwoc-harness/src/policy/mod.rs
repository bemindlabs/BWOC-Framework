//! Policy / permission system and safety guardrails.
//!
//! Three-layer safety pipeline.  Every tool call passes through these layers
//! **before** sandbox execution, in the order below:
//!
//! ```text
//! GUARDRAILS → PERMISSION → SANDBOX → execute
//! ```
//!
//! ## Layer 1 — Guardrails (`guardrails`)
//!
//! Hard policy engine grounded in Sīla 5 + Taṇhā 3.  Runs first, always.
//! Cannot be overridden by permission config or any operator action.
//! Blocks: `rm -rf` of repo/worktree root; secret writes; identity spoof;
//! gate-bypass flags (`--no-verify`, `--force`/`-f` on push); privilege
//! escalation (`sudo`, `su`, `doas`).
//!
//! Returns [`guardrails::GuardrailViolation`] on a hit.  The violation is
//! fed back to the model as the tool result.
//!
//! ## Layer 2 — Permission (`permission`)
//!
//! Per-tool / per-pattern `allow | ask | deny` modes loaded from
//! `config.manifest.json` and `.bwoc/harness-policy.toml`.  `ask` prompts
//! the operator on TTY; in non-TTY / autonomous mode it falls back to the
//! policy default (deny).  Denials are fed back to the model as tool results.
//!
//! ## Layer 3 — Sandbox (`crate::sandbox`)
//!
//! Confines all tool effects to the agent's worktree: filesystem write
//! allowlist; `run_command` with env scrub and arg-level scan; OS-level
//! sandbox stub (macOS / Linux pluggable trait, v1 is worktree+allowlist).

pub mod guardrails;
pub mod permission;

pub use guardrails::{GuardrailViolation, check as guardrail_check};
pub use permission::{
    HarnessPolicy, Mode, PermissionDecision, Policy, evaluate as permission_evaluate,
};

use bwoc_core::trust::TrustLevel;

/// Layer-0 capability whitelist (Phase 5 t2 — saṃvara).
///
/// The **closed** set of PURE-READ tools an Untrusted turn may still invoke.
/// The gate is **deny-only with zero allow-by-omission**: any tool not named
/// here is refused when the turn's trust is [`TrustLevel::Untrusted`], so a
/// newly-added tool is denied-by-default until it is deliberately classified.
///
/// Every entry is read-only and side-effect-free (verified against the tool
/// implementations): `read_file`, `list_dir`, and `grep` only read the
/// worktree; `memory_read` performs a single confined `read_to_string` of
/// `memories/` with **no** write, lazy-index, or access-log side effect.
/// Effectful tools (`write_file`, `edit_file`, `run_command`, `git`,
/// `memory_write`, MCP, …) are absent by construction.
const UNTRUSTED_CAPABILITY_WHITELIST: &[&str] = &["read_file", "list_dir", "grep", "memory_read"];

/// Whether `tool_name` is on the closed pure-read capability whitelist.
fn is_whitelisted_capability(tool_name: &str) -> bool {
    UNTRUSTED_CAPABILITY_WHITELIST.contains(&tool_name)
}

/// The outcome of the full policy pipeline (guardrails + permission).
///
/// Used by `agent_loop` to decide whether to proceed to the sandbox and
/// execute the tool, or to return a denial message to the model.
#[derive(Debug, Clone)]
pub enum PolicyOutcome {
    /// All policy layers approved; proceed to sandbox then execute.
    Proceed,
    /// The Layer-0 capability gate refused an effectful tool because the turn's
    /// trust was Untrusted (Phase 5 t2). Distinct from a guardrail or permission
    /// denial — it is a trust-policy refusal — so the caller counts it
    /// separately (`capability_denials`). The model receives the reason as the
    /// tool result and can fall back to a whitelisted read-only tool.
    CapabilityDenied { tool: String, reason: String },
    /// A guardrail rule fired.  The model receives this as the tool result.
    GuardrailBlocked(GuardrailViolation),
    /// The permission layer denied the call.  The model receives this as the
    /// tool result so it can adapt (e.g., try a different approach).
    PermissionDenied(String),
}

impl PolicyOutcome {
    /// Convert to the string that will be fed back to the model as the
    /// tool result when the call is blocked.
    pub fn into_tool_result(self) -> Option<String> {
        match self {
            PolicyOutcome::Proceed => None,
            PolicyOutcome::CapabilityDenied { tool, reason } => Some(format!(
                "DENIED by capability gate [t2]: tool `{tool}` is not permitted on an \
                 untrusted turn — {reason}"
            )),
            PolicyOutcome::GuardrailBlocked(v) => Some(format!(
                "BLOCKED by safety guardrail [{rule}]: {reason}",
                rule = v.rule,
                reason = v.reason,
            )),
            PolicyOutcome::PermissionDenied(reason) => {
                Some(format!("DENIED by permission policy: {reason}"))
            }
        }
    }
}

/// Run the full policy pipeline for one tool call.
///
/// # Arguments
/// - `tool_name`      — the tool being called
/// - `arguments_json` — the raw JSON argument string from the model
/// - `worktree_root`  — absolute path of the agent's worktree root
/// - `policy`         — the loaded permission policy
/// - `is_tty`         — whether a controlling TTY is available for `ask` prompts
/// - `turn_trust`     — the trust verdict for the current turn (Phase 5 t2).
///   **Required, not optional**: there is no defaulting path — a caller MUST
///   supply a verdict, so a forgotten argument is a compile error rather than a
///   silent `Trusted`. Fail-closed is the caller's job (an empty/derived-only
///   turn yields [`TrustLevel::Untrusted`]); see
///   [`crate::session_trust::scan_turn_trust`].
///
/// # Returns
/// [`PolicyOutcome::Proceed`] if all layers approve, or a blocking variant
/// that the caller should surface as the tool result.
pub fn run_pipeline(
    tool_name: &str,
    arguments_json: &str,
    worktree_root: &std::path::Path,
    policy: &Policy,
    is_tty: bool,
    turn_trust: TrustLevel,
) -> PolicyOutcome {
    // Layer 0: Capability gate (Phase 5 t2). Runs FIRST and is deny-only — it
    // can refuse, never grant. On an Untrusted turn, only the closed pure-read
    // whitelist may proceed; every other (effectful) tool is refused before the
    // guardrail/permission layers even run. A Trusted turn is a no-op here and
    // falls straight through to the existing layers. Zero allow-by-omission:
    // the check is membership in the whitelist, so an unclassified tool is
    // denied by default.
    if turn_trust == TrustLevel::Untrusted && !is_whitelisted_capability(tool_name) {
        return PolicyOutcome::CapabilityDenied {
            tool: tool_name.to_string(),
            reason: "effectful capability blocked on an untrusted turn \
                     (only pure-read tools are permitted)"
                .to_string(),
        };
    }

    // Layer 1: Guardrails (non-overridable, always runs first).
    if let Err(violation) = guardrail_check(tool_name, arguments_json, worktree_root) {
        return PolicyOutcome::GuardrailBlocked(violation);
    }

    // Layer 2: Permission.
    match permission_evaluate(policy, tool_name, arguments_json, is_tty) {
        PermissionDecision::Allow => PolicyOutcome::Proceed,
        PermissionDecision::Deny { reason } => PolicyOutcome::PermissionDenied(reason),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn allow_policy() -> Policy {
        Policy {
            default_mode: Mode::Allow,
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
        }
    }

    fn deny_policy() -> Policy {
        Policy {
            default_mode: Mode::Deny,
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
        }
    }

    fn wt() -> &'static Path {
        Path::new("/tmp/agent-oracle/test-worktree")
    }

    // ── Guardrails fire before permission ────────────────────────────────────

    #[test]
    fn guardrail_blocks_before_permission_allow() {
        // Even with allow_policy, a guardrail violation must block.
        let policy = allow_policy();
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "rm -rf /"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        assert!(matches!(outcome, PolicyOutcome::GuardrailBlocked(_)));
    }

    #[test]
    fn guardrail_blocks_no_verify_before_permission() {
        let policy = allow_policy();
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "git commit --no-verify -m 'skip'"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        assert!(matches!(outcome, PolicyOutcome::GuardrailBlocked(_)));
    }

    // ── Permission deny after guardrails pass ────────────────────────────────

    #[test]
    fn permission_deny_blocks_safe_command() {
        let policy = deny_policy();
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "echo hello"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        assert!(matches!(outcome, PolicyOutcome::PermissionDenied(_)));
    }

    // ── Proceed when both layers pass ────────────────────────────────────────

    #[test]
    fn proceed_when_all_layers_pass() {
        let policy = allow_policy();
        let outcome = run_pipeline(
            "read_file",
            r#"{"path": "README.md"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        assert!(matches!(outcome, PolicyOutcome::Proceed));
    }

    // ── into_tool_result ─────────────────────────────────────────────────────

    #[test]
    fn into_tool_result_proceed_is_none() {
        assert!(PolicyOutcome::Proceed.into_tool_result().is_none());
    }

    #[test]
    fn into_tool_result_guardrail_blocked_contains_rule() {
        let v = GuardrailViolation {
            rule: "sila_panatatipata",
            reason: "test".to_string(),
        };
        let msg = PolicyOutcome::GuardrailBlocked(v)
            .into_tool_result()
            .unwrap();
        assert!(msg.contains("sila_panatatipata"));
        assert!(msg.contains("BLOCKED by safety guardrail"));
    }

    #[test]
    fn into_tool_result_permission_denied_contains_reason() {
        let msg = PolicyOutcome::PermissionDenied("operator said no".to_string())
            .into_tool_result()
            .unwrap();
        assert!(msg.contains("DENIED by permission policy"));
        assert!(msg.contains("operator said no"));
    }

    // ── Denial is NOT a hard error — it is a tool result ────────────────────
    // (This is a documentation-as-test: the outcome never panics)

    #[test]
    fn denial_does_not_panic() {
        let policy = deny_policy();
        // This must return a PolicyOutcome, not panic or return Err.
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "sudo rm -rf /"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        // Guardrail fires first (sudo → bhava_tanha_escalation).
        assert!(matches!(outcome, PolicyOutcome::GuardrailBlocked(_)));
    }

    // ── Layer 0 — capability gate (Phase 5 t2) ───────────────────────────────

    #[test]
    fn untrusted_turn_denies_effectful_tool_before_guardrails() {
        // An effectful tool on an Untrusted turn is refused at Layer 0 — even
        // under an allow-all policy and a benign command (no guardrail hit). The
        // refusal must be CapabilityDenied, not a guardrail/permission denial.
        let policy = allow_policy();
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "echo hello"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Untrusted,
        );
        match outcome {
            PolicyOutcome::CapabilityDenied { tool, .. } => assert_eq!(tool, "run_command"),
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }
    }

    #[test]
    fn untrusted_turn_denies_write_file() {
        let policy = allow_policy();
        let outcome = run_pipeline(
            "write_file",
            r#"{"path": "out.txt", "content": "x"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Untrusted,
        );
        assert!(matches!(outcome, PolicyOutcome::CapabilityDenied { .. }));
    }

    #[test]
    fn untrusted_turn_allows_whitelisted_read_tools() {
        // Every pure-read whitelist entry proceeds even on an Untrusted turn.
        let policy = allow_policy();
        for (tool, args) in [
            ("read_file", r#"{"path": "README.md"}"#),
            ("list_dir", "{}"),
            ("grep", r#"{"pattern": "x"}"#),
            ("memory_read", "{}"),
        ] {
            let outcome = run_pipeline(tool, args, wt(), &policy, false, TrustLevel::Untrusted);
            assert!(
                matches!(outcome, PolicyOutcome::Proceed),
                "whitelisted `{tool}` must proceed on an untrusted turn, got {outcome:?}"
            );
        }
    }

    #[test]
    fn untrusted_turn_denies_unknown_tool_zero_allow_by_omission() {
        // A tool nobody classified is denied by default on an untrusted turn —
        // the gate is membership-based, never allow-by-omission.
        let policy = allow_policy();
        let outcome = run_pipeline(
            "some_new_unclassified_tool",
            "{}",
            wt(),
            &policy,
            false,
            TrustLevel::Untrusted,
        );
        assert!(matches!(outcome, PolicyOutcome::CapabilityDenied { .. }));
    }

    #[test]
    fn trusted_turn_is_a_noop_for_the_capability_gate() {
        // A Trusted turn falls straight through Layer 0; an effectful tool under
        // allow-all proceeds (the gate granted nothing — it simply did not deny).
        let policy = allow_policy();
        let outcome = run_pipeline(
            "write_file",
            r#"{"path": "out.txt", "content": "x"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        assert!(matches!(outcome, PolicyOutcome::Proceed));
    }

    #[test]
    fn capability_gate_does_not_override_guardrails_on_trusted_turn() {
        // Layer 0 is deny-only and trust-scoped; it must not weaken guardrails.
        // A Trusted turn still hits the guardrail for `rm -rf /`.
        let policy = allow_policy();
        let outcome = run_pipeline(
            "run_command",
            r#"{"command": "rm -rf /"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Trusted,
        );
        assert!(matches!(outcome, PolicyOutcome::GuardrailBlocked(_)));
    }

    #[test]
    fn into_tool_result_capability_denied_contains_tool_and_marker() {
        let msg = PolicyOutcome::CapabilityDenied {
            tool: "run_command".to_string(),
            reason: "untrusted".to_string(),
        }
        .into_tool_result()
        .unwrap();
        assert!(msg.contains("DENIED by capability gate [t2]"));
        assert!(msg.contains("run_command"));
    }
}
