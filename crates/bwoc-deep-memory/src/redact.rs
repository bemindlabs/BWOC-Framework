//! Secret redaction for `mine`.
//!
//! Scrubs credentials out of chunk text **before** it is embedded and persisted,
//! so the deep-memory store never becomes a secret sink. This is the t32
//! security slice: of all the deferred deep-memory work, redaction is the one
//! piece with no CI risk and a real security payoff (a mined session log can
//! easily carry an API key or a private-key block).
//!
//! Precision over recall: every pattern is anchored to a high-signal shape
//! (known token prefixes, or an explicit `key = value` assignment with a
//! secret-ish value). We would rather miss an exotic secret than mangle ordinary
//! prose — a false positive corrupts a legitimate memory.

use regex::Regex;
use std::sync::OnceLock;

/// Substituted for every matched secret value.
const PLACEHOLDER: &str = "[REDACTED]";

/// One redaction rule: a compiled pattern plus the replacement template
/// (`$n` back-references are expanded by `Regex::replace_all`).
struct Rule {
    re: Regex,
    replacement: &'static str,
}

/// The redaction ruleset, compiled once. Patterns are static, so a failed
/// compile is a programmer error — `expect` is correct here.
fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |p: &str, replacement: &'static str| Rule {
            re: Regex::new(p).expect("static redaction pattern compiles"),
            replacement,
        };
        vec![
            // PEM private-key block (multi-line). Whole block → placeholder.
            r(
                r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
                PLACEHOLDER,
            ),
            // `key = value` / `key: value` assignment with a secret-ish key and a
            // contiguous value of 8+ token chars. Keep the key + separator +
            // optional opening quote; redact only the value. The value class
            // includes `=` so base64 secrets (incl. trailing `=` padding) are
            // consumed whole — otherwise the tail after a `=` would survive.
            r(
                r#"(?i)\b(api[_-]?key|secret|token|password|passwd|pwd|access[_-]?key|auth)\b(\s*[:=]\s*)(["']?)([A-Za-z0-9/+_.\-=]{8,})"#,
                "$1$2$3[REDACTED]",
            ),
            // AWS access key id.
            r(r"\bAKIA[0-9A-Z]{16}\b", PLACEHOLDER),
            // GitHub tokens (ghp_/gho_/ghu_/ghs_/ghr_).
            r(r"\bgh[pousr]_[A-Za-z0-9]{36,}\b", PLACEHOLDER),
            // OpenAI / Stripe-style `sk-` keys.
            r(r"\bsk-[A-Za-z0-9]{20,}\b", PLACEHOLDER),
            // Slack tokens.
            r(r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b", PLACEHOLDER),
            // JWT (three base64url segments, the first two are `{"...`).
            r(
                r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
                PLACEHOLDER,
            ),
        ]
    })
}

/// Redact secrets from `text`. Returns the scrubbed text and the number of
/// distinct matches replaced (across all rules).
pub fn redact(text: &str) -> (String, usize) {
    let mut out = text.to_string();
    let mut hits = 0;
    for rule in rules() {
        let n = rule.re.find_iter(&out).count();
        if n > 0 {
            hits += n;
            out = rule.re.replace_all(&out, rule.replacement).into_owned();
        }
    }
    (out, hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_prose_is_untouched() {
        let s = "Decided to use rustls instead of OpenSSL. The token bucket holds 5.";
        let (out, n) = redact(s);
        assert_eq!(n, 0);
        assert_eq!(out, s);
    }

    #[test]
    fn assignment_keeps_key_redacts_value() {
        let (out, n) = redact("api_key = sk_live_abcdef0123456789");
        assert_eq!(n, 1);
        assert!(out.starts_with("api_key = [REDACTED]"));
        assert!(!out.contains("abcdef0123456789"));
    }

    #[test]
    fn base64_value_with_padding_fully_redacted() {
        // The `=` padding must be consumed — otherwise the tail after `=` leaks.
        let (out, n) = redact("token = c2VjcmV0dmFsdWVoZXJl=");
        assert_eq!(n, 1);
        assert_eq!(out, "token = [REDACTED]");
    }

    #[test]
    fn quoted_assignment_keeps_quote() {
        let (out, _) = redact(r#"password: "hunter2hunter2""#);
        assert!(out.contains(r#"password: "[REDACTED]"#));
        assert!(!out.contains("hunter2"));
    }

    #[test]
    fn aws_and_github_tokens() {
        let (out, n) =
            redact("key AKIAIOSFODNN7EXAMPLE and ghp_0123456789abcdefghijklmnopqrstuvwxyz");
        assert_eq!(n, 2);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!out.contains("ghp_0123"));
        assert!(out.matches("[REDACTED]").count() == 2);
    }

    #[test]
    fn pem_private_key_block() {
        let s = "before\n-----BEGIN RSA PRIVATE KEY-----\nMIIEv...\nABC==\n-----END RSA PRIVATE KEY-----\nafter";
        let (out, n) = redact(s);
        assert_eq!(n, 1);
        assert!(out.contains("before"));
        assert!(out.contains("after"));
        assert!(out.contains("[REDACTED]"));
        assert!(!out.contains("MIIEv"));
    }

    #[test]
    fn jwt_is_redacted() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let (out, n) = redact(&format!("Authorization header carried {jwt} today"));
        assert_eq!(n, 1);
        assert!(!out.contains("eyJhbGci"));
    }

    #[test]
    fn short_value_after_keyword_is_not_a_false_positive() {
        // "token bucket" — no `=`/`:` separator, so the assignment rule misses it.
        let (_, n) = redact("the token bucket refills every second");
        assert_eq!(n, 0);
    }
}
