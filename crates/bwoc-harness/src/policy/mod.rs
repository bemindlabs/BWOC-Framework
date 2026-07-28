//! Policy / permission system and safety guardrails.
//!
//! Four-layer safety pipeline.  Every tool call passes through these layers
//! **before** sandbox execution, in the order below:
//!
//! ```text
//! CAPABILITY GATE → GUARDRAILS → PERMISSION → SANDBOX → execute
//! ```
//!
//! ## Layer 0 — Capability gate (Phase 5 t3)
//!
//! Trust-scoped and deny-only. On an **Untrusted** turn (the session-monotonic
//! latch in [`crate::session_trust`] has observed untrusted ingress) tools are
//! graded by blast radius ([`Capability`]): pure-read always proceeds; a
//! worktree-confined write proceeds only when its target stays inside the
//! worktree; every other effect (run_command, git, network egress, sub-agent
//! spawn, unclassified) is refused. A **Trusted** turn is a no-op here. Returns
//! [`PolicyOutcome::CapabilityDenied`] on a refusal.
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
//! the operator on TTY; without a TTY it either escalates to a human via the
//! opt-in approval console (`--approval-channel`, see [`approval`]) or, absent
//! that, falls back to the policy default (deny).  A channel can only turn a
//! would-be deny into an operator-approved allow, never weaken a deny. Denials
//! are fed back to the model as tool results.
//!
//! ## Layer 3 — Sandbox (`crate::sandbox`)
//!
//! Confines all tool effects to the agent's worktree: filesystem write
//! allowlist; `run_command` with env scrub and arg-level scan; OS-level
//! sandbox stub (macOS / Linux pluggable trait, v1 is worktree+allowlist).

pub mod approval;
pub mod guardrails;
pub mod permission;

pub use approval::{ApprovalChannel, ApprovalDecision, ApprovalRequest, FileApprovalChannel};
pub use guardrails::{GuardrailViolation, check as guardrail_check};
pub use permission::{
    HarnessPolicy, Mode, PermissionDecision, Policy, evaluate as permission_evaluate,
};

use bwoc_core::trust::TrustLevel;

/// The pure-read tool whitelist — the **single source of truth** for which
/// tools are read-only and side-effect-free (Phase 5 t4 — saṃvara).
///
/// This slice is the one authority three places defer to, so they can never
/// drift apart:
/// 1. [`classify_capability`] decides [`Capability::PureRead`] *only* by testing
///    membership here — the lone construction site for that tier (t4 BC-3b).
/// 2. The t4 behavioral proof (`tests/egress_pure_read.rs`) enumerates **this**
///    slice and exercises every entry under a deny-all-egress + read-only-FS
///    sandbox — no opt-out list, so a tool that cannot be exercised cannot be
///    PureRead (t4 BC-3a).
/// 3. The static forbidden-symbol floor scans each listed tool's source.
///
/// Adding a tool here is a deliberate act: it must survive the behavioral proof
/// on Linux and the static floor everywhere, or the test suite fails.
pub const PURE_READ_TOOLS: &[&str] = &["read_file", "list_dir", "grep", "memory_read"];

/// Layer-0 capability tier, **graded by blast radius** (Phase 5 t3 — saṃvara,
/// yudi's ruling (a)). This REPLACES t2's flat "deny every effectful tool on an
/// untrusted turn": an Untrusted turn may still do the low-blast-radius things
/// (read anything; write *inside its own worktree*) while the high-blast-radius
/// effects stay gated. Grading is **deny-biased with zero allow-by-omission**:
/// any tool not deliberately classified into [`Capability::PureRead`] or
/// [`Capability::WorktreeWrite`] falls to [`Capability::Gated`] and is refused on
/// an Untrusted turn, so a newly-added tool is denied-by-default.
#[derive(Debug, PartialEq, Eq)]
enum Capability {
    /// Read-only and side-effect-free — allowed on ANY turn. `read_file`,
    /// `list_dir`, `grep` only read the worktree; `memory_read` does a single
    /// confined `read_to_string` of `memories/` with no write / index / log
    /// side effect.
    PureRead,
    /// A write/edit whose blast radius is a single path argument — allowed on an
    /// Untrusted turn ONLY when that target resolves *inside the worktree*
    /// (path-confinement via [`crate::sandbox::confine_path`], which also rejects
    /// symlink escapes). `path` is the raw target extracted from the args, or
    /// `None` when the args are missing/malformed (fail-closed → denied).
    WorktreeWrite { path: Option<String> },
    /// Effectful beyond a confined worktree write — `run_command`, `git` (commit
    /// / push), `run_gates`, the `bwoc_*` sub-agent / message / task tools, every
    /// MCP tool (network egress), and any unclassified tool. Refused on an
    /// Untrusted turn, always. Covers yudi's gated list: run_command, git push,
    /// PR-create, network egress, and delete outside the worktree.
    Gated,
}

/// Classify `tool_name` into its capability tier, extracting the confinement
/// target for worktree-confined writes from `arguments_json`.
fn classify_capability(tool_name: &str, arguments_json: &str) -> Capability {
    /// Pull a string field out of the (possibly malformed) JSON args.
    fn arg_str(args_json: &str, key: &str) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(args_json)
            .ok()
            .and_then(|v| v[key].as_str().map(str::to_string))
    }
    // PureRead is decided SOLELY by membership in the single-source-of-truth
    // whitelist — the one and only construction site for this tier (t4 BC-3b).
    // Keep this above the `match` so no second arm can mint a PureRead.
    if PURE_READ_TOOLS.contains(&tool_name) {
        return Capability::PureRead;
    }
    match tool_name {
        "write_file" | "edit_file" => Capability::WorktreeWrite {
            path: arg_str(arguments_json, "path"),
        },
        // memory_write targets `memories/<name>` under the worktree; confine on
        // that derived path so a `..`-escaping name is denied at the gate too.
        "memory_write" => Capability::WorktreeWrite {
            path: arg_str(arguments_json, "name").map(|n| format!("memories/{n}")),
        },
        // run_command, git, run_gates, bwoc_run/send/task, MCP (mcp__*), and any
        // tool nobody classified — gated, zero allow-by-omission.
        _ => Capability::Gated,
    }
}

/// The outcome of the full policy pipeline (guardrails + permission).
///
/// Used by `agent_loop` to decide whether to proceed to the sandbox and
/// execute the tool, or to return a denial message to the model.
#[derive(Debug, Clone)]
pub enum PolicyOutcome {
    /// All policy layers approved; proceed to sandbox then execute.
    Proceed,
    /// The Layer-0 capability gate refused a tool because the turn's trust was
    /// Untrusted and the tool's blast radius was too high for an untrusted turn
    /// (Phase 5 t3): an out-of-worktree write, or a fully gated effect
    /// (run_command / git / network / sub-agent / unclassified). Distinct from a
    /// guardrail or permission denial — it is a trust-policy refusal — so the
    /// caller counts it separately (`capability_denials`). The model receives the
    /// reason as the tool result and can fall back to a pure-read tool or a
    /// worktree-confined write.
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
                "DENIED by capability gate [t3]: tool `{tool}` is not permitted on an \
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
    // Layer 0: Capability gate (Phase 5 t3, ruling (a)). Runs FIRST and is
    // deny-only — it can refuse, never grant. On an Untrusted turn the tool is
    // graded by blast radius: pure-read always proceeds; a worktree-confined
    // write proceeds ONLY when its target stays inside the worktree; everything
    // else (run_command, git, network egress, sub-agent spawn, unclassified) is
    // refused before the guardrail/permission layers run. A Trusted turn is a
    // no-op here and falls straight through. Zero allow-by-omission: an
    // unclassified tool is `Gated` and denied by default.
    if turn_trust == TrustLevel::Untrusted {
        match classify_capability(tool_name, arguments_json) {
            Capability::PureRead => {} // always allowed — fall through
            Capability::WorktreeWrite { path } => {
                let confined = path
                    .as_deref()
                    .is_some_and(|p| crate::sandbox::confine_path(p, worktree_root).is_ok());
                if !confined {
                    return PolicyOutcome::CapabilityDenied {
                        tool: tool_name.to_string(),
                        reason: "write target escapes the worktree — an untrusted turn \
                                 may write only inside its own worktree"
                            .to_string(),
                    };
                }
                // Confined write — fall through to guardrails/permission.
            }
            Capability::Gated => {
                return PolicyOutcome::CapabilityDenied {
                    tool: tool_name.to_string(),
                    reason: "effectful capability blocked on an untrusted turn \
                             (run_command / git / network / sub-agent / external delete \
                             are gated; only pure-read and worktree-confined writes are \
                             permitted)"
                        .to_string(),
                };
            }
        }
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
            ..Default::default()
        }
    }

    fn deny_policy() -> Policy {
        Policy {
            default_mode: Mode::Deny,
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
            ..Default::default()
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

    #[test]
    fn computer_use_denied_on_untrusted_turn() {
        // The `computer` tool is `Gated`, so the Layer-0 capability gate refuses
        // it on an Untrusted turn even under an allow policy — computer-use can
        // never be driven by untrusted (taint-bearing) context.
        let policy = allow_policy();
        let outcome = run_pipeline(
            "computer",
            r#"{"action":"screenshot"}"#,
            wt(),
            &policy,
            false,
            TrustLevel::Untrusted,
        );
        assert!(matches!(
            outcome,
            PolicyOutcome::CapabilityDenied { tool, .. } if tool == "computer"
        ));
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
    fn untrusted_turn_allows_worktree_confined_write() {
        // Ruling (a): a write whose target stays inside the worktree proceeds
        // even on an Untrusted turn. Uses a real tempdir so path-confinement
        // canonicalization is apples-to-apples (macOS /tmp → /private/tmp).
        let dir = tempfile::tempdir().unwrap();
        let policy = allow_policy();
        for (tool, args) in [
            ("write_file", r#"{"path": "out.txt", "content": "x"}"#),
            (
                "edit_file",
                r#"{"path": "sub/f.rs", "old_string": "a", "new_string": "b"}"#,
            ),
            ("memory_write", r#"{"name": "note.md", "content": "x"}"#),
        ] {
            let outcome = run_pipeline(
                tool,
                args,
                dir.path(),
                &policy,
                false,
                TrustLevel::Untrusted,
            );
            assert!(
                matches!(outcome, PolicyOutcome::Proceed),
                "worktree-confined `{tool}` must proceed on an untrusted turn, got {outcome:?}"
            );
        }
    }

    #[test]
    fn untrusted_turn_denies_out_of_worktree_write() {
        // A write/edit whose target escapes the worktree is capability-denied on
        // an Untrusted turn — even under allow-all. Includes a `..` escape, an
        // absolute outside path, and a `..`-escaping memory name.
        let dir = tempfile::tempdir().unwrap();
        let policy = allow_policy();
        for (tool, args) in [
            (
                "write_file",
                r#"{"path": "../../etc/passwd", "content": "x"}"#,
            ),
            (
                "edit_file",
                r#"{"path": "/etc/hosts", "old_string": "a", "new_string": "b"}"#,
            ),
            (
                "memory_write",
                r#"{"name": "../../escape.md", "content": "x"}"#,
            ),
        ] {
            let outcome = run_pipeline(
                tool,
                args,
                dir.path(),
                &policy,
                false,
                TrustLevel::Untrusted,
            );
            assert!(
                matches!(outcome, PolicyOutcome::CapabilityDenied { .. }),
                "out-of-worktree `{tool}` must be capability-denied, got {outcome:?}"
            );
        }
    }

    #[test]
    fn untrusted_turn_denies_write_with_malformed_args_fail_closed() {
        // A worktree-confined-write tool whose args omit the target path is
        // fail-closed (no path to confine → denied), never allow-by-omission.
        let dir = tempfile::tempdir().unwrap();
        let policy = allow_policy();
        let outcome = run_pipeline(
            "write_file",
            r#"{"content": "x"}"#,
            dir.path(),
            &policy,
            false,
            TrustLevel::Untrusted,
        );
        assert!(matches!(outcome, PolicyOutcome::CapabilityDenied { .. }));
    }

    #[test]
    fn untrusted_turn_gates_effectful_non_write_tools() {
        // The fully-gated tier: run_command, git, run_gates, the bwoc_* tools,
        // and MCP tools are denied on an Untrusted turn regardless of args.
        let policy = allow_policy();
        for (tool, args) in [
            ("git", r#"{"args": "push"}"#),
            ("run_gates", "{}"),
            ("bwoc_run", r#"{"task": "x"}"#),
            ("bwoc_send", r#"{"to": "peer", "message": "x"}"#),
            ("bwoc_task", r#"{"team": "t", "task": "x"}"#),
            ("mcp__server__some_tool", "{}"),
        ] {
            let outcome = run_pipeline(tool, args, wt(), &policy, false, TrustLevel::Untrusted);
            assert!(
                matches!(outcome, PolicyOutcome::CapabilityDenied { .. }),
                "gated `{tool}` must be capability-denied on an untrusted turn, got {outcome:?}"
            );
        }
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

    // ── t4 BC-5 — extracting PURE_READ_TOOLS is a behavior-preserving refactor ─

    #[test]
    fn classify_capability_golden_table_is_behavior_preserving() {
        // A golden table pinning the classification of every tool the harness
        // knows. Extracting `PURE_READ_TOOLS` and routing PureRead through a
        // membership check must NOT change any verdict: the 4 whitelist strings
        // stay PureRead, the writes stay WorktreeWrite, everything else stays
        // Gated. If a future edit shifts any tool's tier, this test fails.
        let pr = Capability::PureRead;
        let gated = Capability::Gated;
        // (tool, args, expected)
        let cases: &[(&str, &str, Capability)] = &[
            ("read_file", r#"{"path":"a"}"#, Capability::PureRead),
            ("list_dir", "{}", Capability::PureRead),
            ("grep", r#"{"pattern":"x"}"#, Capability::PureRead),
            ("memory_read", "{}", Capability::PureRead),
            (
                "write_file",
                r#"{"path":"a","content":"x"}"#,
                Capability::WorktreeWrite {
                    path: Some("a".to_string()),
                },
            ),
            (
                "edit_file",
                r#"{"path":"b"}"#,
                Capability::WorktreeWrite {
                    path: Some("b".to_string()),
                },
            ),
            (
                "memory_write",
                r#"{"name":"n.md"}"#,
                Capability::WorktreeWrite {
                    path: Some("memories/n.md".to_string()),
                },
            ),
            ("run_command", r#"{"command":"echo"}"#, Capability::Gated),
            ("git", r#"{"subcommand":"status"}"#, Capability::Gated),
            ("run_gates", "{}", Capability::Gated),
            ("bwoc_task", r#"{"action":"list"}"#, Capability::Gated),
            ("bwoc_send", "{}", Capability::Gated),
            ("bwoc_run", "{}", Capability::Gated),
            ("mcp__srv__tool", "{}", Capability::Gated),
            ("computer", r#"{"action":"screenshot"}"#, Capability::Gated),
            ("totally_unknown_tool", "{}", Capability::Gated),
        ];
        for (tool, args, expected) in cases {
            assert_eq!(
                &classify_capability(tool, args),
                expected,
                "classification of `{tool}` drifted — refactor was not behavior-preserving"
            );
        }
        // Sanity: the whitelist drives PureRead and only PureRead.
        for t in PURE_READ_TOOLS {
            assert_eq!(classify_capability(t, "{}"), pr, "`{t}` must be PureRead");
        }
        assert_ne!(classify_capability("write_file", "{}"), gated);
    }

    #[test]
    fn pure_read_tier_comes_only_from_the_whitelist() {
        // Membership in PURE_READ_TOOLS ⇔ PureRead. A tool NOT in the slice is
        // never PureRead, regardless of args (the t4 BC-3b invariant, checked
        // behaviorally here; the source-level single-construction-site guard
        // lives in tests/egress_pure_read.rs).
        for tool in [
            "write_file",
            "edit_file",
            "memory_write",
            "run_command",
            "git",
            "x",
        ] {
            assert!(
                !PURE_READ_TOOLS.contains(&tool),
                "test premise broken: `{tool}` unexpectedly in whitelist"
            );
            assert_ne!(
                classify_capability(tool, "{}"),
                Capability::PureRead,
                "`{tool}` is not whitelisted yet classified PureRead — second construction site?"
            );
        }
    }

    #[test]
    fn into_tool_result_capability_denied_contains_tool_and_marker() {
        let msg = PolicyOutcome::CapabilityDenied {
            tool: "run_command".to_string(),
            reason: "untrusted".to_string(),
        }
        .into_tool_result()
        .unwrap();
        assert!(msg.contains("DENIED by capability gate [t3]"));
        assert!(msg.contains("run_command"));
    }
}
