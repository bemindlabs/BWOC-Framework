//! Permission system — per-tool / per-pattern allow | ask | deny.
//!
//! Runs **after** guardrails (which cannot be overridden) and **before**
//! sandbox execution.  Denials at this layer are fed back to the model as tool
//! results, not as hard errors.
//!
//! # Configuration: `.bwoc/harness-policy.toml`
//!
//! ```toml
//! # Global default for any tool/pattern not explicitly listed.
//! # Valid values: "allow" | "ask" | "deny"
//! # Fail-safe: defaults to "deny" when absent.
//! default_mode = "allow"
//!
//! # Per-tool overrides.  The key is the exact tool name.
//! [tools]
//! read_file   = "allow"
//! list_dir    = "allow"
//! write_file  = "ask"
//! run_command = "deny"
//!
//! # Pattern rules: checked against the full JSON arguments string.
//! # Rules are evaluated in order; the first match wins.
//! [[patterns]]
//! pattern = "git push"
//! mode    = "deny"
//! reason  = "git push requires human review"
//!
//! [[patterns]]
//! pattern = "cargo test"
//! mode    = "allow"
//! ```
//!
//! # `ask` mode in non-TTY / autonomous contexts
//!
//! When the harness is running without a controlling TTY (e.g. in CI, in a
//! background agent, or spawned by `bwoc spawn`), there is no operator to
//! ask.  In that case `ask` falls back to the `default_mode` — which itself
//! defaults to `deny`.  This is the fail-safe behaviour required by the
//! design note.
//!
//! # Taṇhā 3 mapping
//!
//! | Root | How permission addresses it |
//! |---|---|
//! | Kāma-taṇhā (craving) | `ask`/`deny` intercepts tool calls driven by unchecked model output |
//! | Bhava-taṇhā (becoming) | `deny` on persistence-altering tools by default |
//! | Vibhava-taṇhā (destruction) | `deny` / `ask` on destructive commands |

use std::io::{self, BufRead, Write};
use std::path::Path;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The decision returned by the permission layer for a single tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    /// The call is allowed to proceed.
    Allow,
    /// The call was denied by policy (or by the operator when `ask` was used).
    Deny {
        /// Human-readable reason surfaced to the model as tool result.
        reason: String,
    },
}

/// The permission mode for a tool or pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Allow,
    Ask,
    Deny,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Allow => write!(f, "allow"),
            Mode::Ask => write!(f, "ask"),
            Mode::Deny => write!(f, "deny"),
        }
    }
}

/// A pattern rule entry from `harness-policy.toml`.
#[derive(Debug, Clone)]
pub struct PatternRule {
    pub pattern: String,
    pub mode: Mode,
    pub reason: Option<String>,
}

/// Loaded permission policy.
///
/// Constructed from `HarnessPolicy` (the TOML schema) via `into()`, or
/// created directly for tests.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Default mode when no tool or pattern matches.
    pub default_mode: Mode,
    /// Per-tool overrides (tool name → mode).
    pub tools: std::collections::HashMap<String, Mode>,
    /// Pattern rules, evaluated in declaration order; first match wins.
    pub patterns: Vec<PatternRule>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            default_mode: Mode::Deny, // fail-safe
            tools: std::collections::HashMap::new(),
            patterns: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// TOML schema (the structs serde deserialises into)
// ---------------------------------------------------------------------------

/// Top-level structure of `.bwoc/harness-policy.toml`.
#[derive(Debug, serde::Deserialize, Default)]
pub struct HarnessPolicy {
    #[serde(default = "default_mode_str")]
    pub default_mode: String,
    #[serde(default)]
    pub tools: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub patterns: Vec<PatternRuleToml>,
}

fn default_mode_str() -> String {
    "deny".to_string()
}

/// A single `[[patterns]]` entry in the TOML.
#[derive(Debug, serde::Deserialize)]
pub struct PatternRuleToml {
    pub pattern: String,
    pub mode: String,
    #[serde(default)]
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// TOML loading
// ---------------------------------------------------------------------------

impl HarnessPolicy {
    /// Load from a `.bwoc/harness-policy.toml` file.
    ///
    /// Returns a default (fail-safe deny-all) policy if the file does not
    /// exist.  Returns an error if the file exists but cannot be parsed.
    pub fn load(workspace_root: &Path) -> Result<Self, String> {
        let policy_path = workspace_root.join(".bwoc").join("harness-policy.toml");
        if !policy_path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&policy_path)
            .map_err(|e| format!("cannot read harness-policy.toml: {e}"))?;
        toml::from_str(&raw).map_err(|e| format!("cannot parse harness-policy.toml: {e}"))
    }
}

impl From<HarnessPolicy> for Policy {
    fn from(hp: HarnessPolicy) -> Self {
        let default_mode = parse_mode(&hp.default_mode).unwrap_or(Mode::Deny);

        let tools = hp
            .tools
            .into_iter()
            .filter_map(|(name, mode_str)| parse_mode(&mode_str).map(|m| (name, m)))
            .collect();

        let patterns = hp
            .patterns
            .into_iter()
            .filter_map(|p| {
                parse_mode(&p.mode).map(|m| PatternRule {
                    pattern: p.pattern,
                    mode: m,
                    reason: p.reason,
                })
            })
            .collect();

        Self {
            default_mode,
            tools,
            patterns,
        }
    }
}

fn parse_mode(s: &str) -> Option<Mode> {
    match s.to_lowercase().trim() {
        "allow" => Some(Mode::Allow),
        "ask" => Some(Mode::Ask),
        "deny" => Some(Mode::Deny),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Decision logic
// ---------------------------------------------------------------------------

/// Evaluate the permission policy for a single tool call.
///
/// # Arguments
/// - `policy`         — loaded policy (from TOML or defaults)
/// - `tool_name`      — the tool being called
/// - `arguments_json` — raw JSON argument string (used for pattern matching)
/// - `is_tty`         — whether the harness has a controlling TTY available
///
/// # Behaviour
/// 1. Check per-tool overrides.
/// 2. Check pattern rules in order; first match wins.
/// 3. Fall back to `default_mode`.
/// 4. If the resolved mode is `ask`:
///    - `is_tty == true`  → prompt on stdin/stdout; operator types `y`/`n`.
///    - `is_tty == false` → fall back to `default_mode` (fail-safe deny).
pub fn evaluate(
    policy: &Policy,
    tool_name: &str,
    arguments_json: &str,
    is_tty: bool,
) -> PermissionDecision {
    let mode = resolve_mode(policy, tool_name, arguments_json);
    apply_mode(mode, policy, tool_name, arguments_json, is_tty)
}

/// Resolve the effective [`Mode`] for a tool call without applying the `ask`
/// TTY-prompt logic.
///
/// The interactive `--chat` driver needs to distinguish `ask` from `allow` /
/// `deny` so it can route the prompt to the frontend (via a `PermissionRequest`
/// event) instead of the controlling TTY that [`evaluate`] assumes. This is the
/// same resolution [`evaluate`] runs internally, exposed as the bare mode.
pub fn resolve_effective_mode(policy: &Policy, tool_name: &str, arguments_json: &str) -> Mode {
    resolve_mode(policy, tool_name, arguments_json).mode
}

/// Tools that default to `ask` regardless of `default_mode`. These have a wide
/// blast radius (computer-use can drive the screen and keyboard), so an operator
/// must approve them unless they opt in with an explicit per-tool policy entry.
/// In non-interactive (no-TTY) runs these fail-safe to *deny*, so autoprocess
/// never silently drives computer-use — defense in depth with the Layer-0
/// capability gate (`computer` is `Gated`) and the t30 ambient-backend refusal.
pub const ASK_BY_DEFAULT_TOOLS: &[&str] = &["computer"];

/// Resolve the effective mode without applying `ask` logic.
fn resolve_mode(policy: &Policy, tool_name: &str, arguments_json: &str) -> ResolvedMode {
    // 1. Per-tool override.
    if let Some(m) = policy.tools.get(tool_name) {
        return ResolvedMode {
            mode: m.clone(),
            reason: None,
        };
    }

    // 2. Pattern rules (first match wins).
    //
    // NOTE (C3): this is a raw-JSON substring match, NOT the first-token bug
    // class fixed in `guardrails.rs`. A shell-operator chain like
    // `true && git push` still contains the `git push` substring, so a `deny`
    // pattern continues to fire on the chained segment — the operator split
    // there would not change the match outcome. The known weakness here is the
    // opposite of guardrails': the pattern can match incidental JSON content
    // (e.g. a path or commit message), not that a segment slips past. Tightening
    // it to structured argv matching is a separate concern, kept out of this
    // narrow security fix (Mattaññutā).
    for rule in &policy.patterns {
        if arguments_json.contains(&rule.pattern) {
            return ResolvedMode {
                mode: rule.mode.clone(),
                reason: rule.reason.clone(),
            };
        }
    }

    // 2b. High-blast-radius tools default to `ask` even when `default_mode` is
    // `allow`. A per-tool entry (step 1) still wins, so an operator can opt in.
    if ASK_BY_DEFAULT_TOOLS.contains(&tool_name) {
        return ResolvedMode {
            mode: Mode::Ask,
            reason: Some(format!(
                "`{tool_name}` is high-blast-radius (computer-use); approval required \
                 unless explicitly set in `.bwoc/harness-policy.toml`"
            )),
        };
    }

    // 3. Default.
    ResolvedMode {
        mode: policy.default_mode.clone(),
        reason: None,
    }
}

struct ResolvedMode {
    mode: Mode,
    reason: Option<String>,
}

fn apply_mode(
    resolved: ResolvedMode,
    policy: &Policy,
    tool_name: &str,
    arguments_json: &str,
    is_tty: bool,
) -> PermissionDecision {
    match resolved.mode {
        Mode::Allow => PermissionDecision::Allow,

        Mode::Deny => PermissionDecision::Deny {
            reason: resolved.reason.unwrap_or_else(|| {
                format!(
                    "tool `{tool_name}` is denied by policy. \
                     Check `.bwoc/harness-policy.toml` to adjust permissions."
                )
            }),
        },

        Mode::Ask => {
            if is_tty {
                prompt_operator(tool_name, arguments_json)
            } else if ASK_BY_DEFAULT_TOOLS.contains(&tool_name) {
                // High-blast-radius tool with no explicit per-tool grant (an
                // explicit `allow` would have resolved to `Allow` upstream and
                // never reached here). Non-TTY → fail-safe *deny* regardless of
                // `default_mode`, so autoprocess can never silently drive it.
                PermissionDecision::Deny {
                    reason: format!(
                        "tool `{tool_name}` is computer-use (high blast radius) and requires \
                         operator approval, but no TTY is available. Denied by fail-safe \
                         policy. Set `{tool_name} = \"allow\"` in `.bwoc/harness-policy.toml` \
                         to permit it in autonomous mode."
                    ),
                }
            } else {
                // Non-TTY: fail-safe to default_mode (which is deny unless
                // explicitly set to allow, which would be unusual).
                match &policy.default_mode {
                    Mode::Allow => PermissionDecision::Allow,
                    _ => PermissionDecision::Deny {
                        reason: format!(
                            "tool `{tool_name}` requires operator approval (`ask` mode) \
                             but no TTY is available. Denied by fail-safe policy. \
                             Set mode to `allow` in `.bwoc/harness-policy.toml` to \
                             permit this tool in autonomous mode."
                        ),
                    },
                }
            }
        }
    }
}

/// Prompt the operator on the controlling TTY.
///
/// Prints the tool name + arguments and waits for `y`/`Y` (allow) or
/// anything else (deny).  Returns after one line of input.
fn prompt_operator(tool_name: &str, arguments_json: &str) -> PermissionDecision {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let _ = writeln!(
        err,
        "\n[bwoc-harness permission] Tool `{tool_name}` wants to run with args:\n  {arguments_json}\nAllow? [y/N] "
    );
    let _ = err.flush();

    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return PermissionDecision::Deny {
            reason: format!("operator prompt failed for `{tool_name}`; denied by default"),
        };
    }

    let answer = line.trim().to_lowercase();
    if answer == "y" || answer == "yes" {
        PermissionDecision::Allow
    } else {
        PermissionDecision::Deny {
            reason: format!("operator declined `{tool_name}` at the TTY prompt"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn allow_all() -> Policy {
        Policy {
            default_mode: Mode::Allow,
            tools: HashMap::new(),
            patterns: Vec::new(),
        }
    }

    fn deny_all() -> Policy {
        Policy {
            default_mode: Mode::Deny,
            tools: HashMap::new(),
            patterns: Vec::new(),
        }
    }

    fn policy_with_tool_rule(tool: &str, mode: Mode) -> Policy {
        let mut p = allow_all();
        p.tools.insert(tool.to_string(), mode);
        p
    }

    fn policy_with_pattern(pattern: &str, mode: Mode, reason: Option<&str>) -> Policy {
        Policy {
            default_mode: Mode::Allow,
            tools: HashMap::new(),
            patterns: vec![PatternRule {
                pattern: pattern.to_string(),
                mode,
                reason: reason.map(|s| s.to_string()),
            }],
        }
    }

    // ── allow / deny basics ──────────────────────────────────────────────────

    #[test]
    fn default_allow_passes_all_tools() {
        let policy = allow_all();
        let d = evaluate(&policy, "write_file", r#"{"path":"x"}"#, false);
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn default_deny_blocks_all_tools() {
        let policy = deny_all();
        let d = evaluate(&policy, "write_file", r#"{"path":"x"}"#, false);
        assert!(matches!(d, PermissionDecision::Deny { .. }));
    }

    // ── per-tool overrides ───────────────────────────────────────────────────

    #[test]
    fn tool_allow_override_on_deny_default() {
        let mut policy = deny_all();
        policy.tools.insert("read_file".to_string(), Mode::Allow);
        let d = evaluate(&policy, "read_file", r#"{"path":"x"}"#, false);
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn tool_deny_override_on_allow_default() {
        let policy = policy_with_tool_rule("run_command", Mode::Deny);
        let d = evaluate(&policy, "run_command", r#"{"command":"ls"}"#, false);
        assert!(matches!(d, PermissionDecision::Deny { .. }));
    }

    // ── pattern rules ────────────────────────────────────────────────────────

    #[test]
    fn pattern_deny_matches_args() {
        let policy = policy_with_pattern("git push", Mode::Deny, Some("requires review"));
        let d = evaluate(
            &policy,
            "run_command",
            r#"{"command":"git push origin feat"}"#,
            false,
        );
        assert!(
            matches!(d, PermissionDecision::Deny { reason } if reason.contains("requires review"))
        );
    }

    #[test]
    fn pattern_allow_matches_args() {
        let policy = policy_with_pattern("cargo test", Mode::Allow, None);
        // Override default to deny so we can confirm the pattern lifts it.
        let mut p = deny_all();
        p.patterns = policy.patterns;
        let d = evaluate(
            &p,
            "run_command",
            r#"{"command":"cargo test --workspace"}"#,
            false,
        );
        assert_eq!(d, PermissionDecision::Allow);
    }

    #[test]
    fn pattern_first_match_wins() {
        let mut policy = allow_all();
        policy.patterns = vec![
            PatternRule {
                pattern: "git push".to_string(),
                mode: Mode::Deny,
                reason: Some("first rule".to_string()),
            },
            PatternRule {
                pattern: "git push".to_string(),
                mode: Mode::Allow,
                reason: Some("second rule — should not be reached".to_string()),
            },
        ];
        let d = evaluate(
            &policy,
            "run_command",
            r#"{"command":"git push origin feat"}"#,
            false,
        );
        assert!(matches!(d, PermissionDecision::Deny { reason } if reason.contains("first rule")));
    }

    // ── ask mode ────────────────────────────────────────────────────────────

    #[test]
    fn ask_non_tty_falls_back_to_default_deny() {
        let policy = policy_with_tool_rule("write_file", Mode::Ask);
        // default_mode is Allow in policy_with_tool_rule, so switch to Deny.
        let mut p = policy;
        p.default_mode = Mode::Deny;
        let d = evaluate(
            &p,
            "write_file",
            r#"{"path":"x"}"#,
            false, /* non-TTY */
        );
        assert!(matches!(d, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn ask_non_tty_falls_back_to_default_allow() {
        let mut policy = policy_with_tool_rule("write_file", Mode::Ask);
        policy.default_mode = Mode::Allow;
        // Non-TTY + default=allow → allow.
        let d = evaluate(&policy, "write_file", r#"{"path":"x"}"#, false);
        assert_eq!(d, PermissionDecision::Allow);
    }

    // ── TOML loading ─────────────────────────────────────────────────────────

    #[test]
    fn toml_load_missing_file_returns_default_deny() {
        let tmp = tempfile::TempDir::new().unwrap();
        let hp = HarnessPolicy::load(tmp.path()).unwrap();
        let policy: Policy = hp.into();
        // Default policy is fail-safe deny-all.
        let d = evaluate(&policy, "write_file", r#"{}"#, false);
        assert!(matches!(d, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn toml_load_parses_correctly() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bwoc")).unwrap();

        // NOTE: TOML literal strings (single-quoted) used for paths to avoid
        // Windows backslash issues — matches the constraint in the task spec.
        let toml_content = r#"
default_mode = 'allow'

[tools]
read_file   = 'allow'
write_file  = 'ask'
run_command = 'deny'

[[patterns]]
pattern = 'git push'
mode    = 'deny'
reason  = 'push requires review'

[[patterns]]
pattern = 'cargo test'
mode    = 'allow'
"#;
        std::fs::write(
            tmp.path().join(".bwoc").join("harness-policy.toml"),
            toml_content,
        )
        .unwrap();

        let hp = HarnessPolicy::load(tmp.path()).unwrap();
        let policy: Policy = hp.into();

        assert_eq!(policy.default_mode, Mode::Allow);
        assert_eq!(policy.tools.get("read_file"), Some(&Mode::Allow));
        assert_eq!(policy.tools.get("write_file"), Some(&Mode::Ask));
        assert_eq!(policy.tools.get("run_command"), Some(&Mode::Deny));
        assert_eq!(policy.patterns.len(), 2);
        assert_eq!(policy.patterns[0].pattern, "git push");
        assert_eq!(policy.patterns[0].mode, Mode::Deny);
        assert_eq!(
            policy.patterns[0].reason.as_deref(),
            Some("push requires review")
        );
    }

    #[test]
    fn toml_deny_reason_propagated_to_decision() {
        let mut policy = deny_all();
        policy.patterns.push(PatternRule {
            pattern: "rm -rf".to_string(),
            mode: Mode::Deny,
            reason: Some("explicit denial from policy".to_string()),
        });
        let d = evaluate(
            &policy,
            "run_command",
            r#"{"command":"rm -rf build/"}"#,
            false,
        );
        assert!(
            matches!(d, PermissionDecision::Deny { reason } if reason.contains("explicit denial"))
        );
    }

    #[test]
    fn computer_asks_by_default_even_under_allow_default() {
        // `allow` default must NOT silently allow computer-use; it resolves to ask.
        let mode = resolve_effective_mode(&allow_all(), "computer", r#"{"action":"screenshot"}"#);
        assert_eq!(mode, Mode::Ask);
    }

    #[test]
    fn computer_denied_non_tty_despite_allow_default() {
        // No TTY + no explicit grant → fail-safe deny, even with default allow.
        let d = evaluate(
            &allow_all(),
            "computer",
            r#"{"action":"screenshot"}"#,
            false,
        );
        assert!(
            matches!(d, PermissionDecision::Deny { reason } if reason.contains("computer-use"))
        );
    }

    #[test]
    fn computer_explicit_allow_opts_in() {
        // An explicit per-tool allow lets the operator run it autonomously.
        let policy = policy_with_tool_rule("computer", Mode::Allow);
        let d = evaluate(&policy, "computer", r#"{"action":"screenshot"}"#, false);
        assert_eq!(d, PermissionDecision::Allow);
    }
}
