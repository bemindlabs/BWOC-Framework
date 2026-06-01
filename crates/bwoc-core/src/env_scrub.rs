//! Environment scrub — build a credential-free environment for child processes.
//!
//! Lives in `bwoc-core` so every crate that spawns a *less-trusted* child shares
//! one allowlist and one filter. Two callers run code the operator does not
//! fully trust and must never hand it the ambient secrets:
//!   - `bwoc-harness` sandbox — model-driven `run_command` children.
//!   - `bwoc-cli` audit runner — third-party audit plugins installed from a
//!     git/tarball URL.
//!
//! Pure `std`; no new dependencies (safe for the lean core crate).

use std::collections::HashMap;

/// Variables safe to pass through to a child process. Everything else is
/// stripped. Permissive for development convenience (PATH, LANG, …) but
/// excludes all credential patterns.
pub const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TMP",
    "TEMP",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUST_LOG",
    "RUST_BACKTRACE",
    // Git identity (non-sensitive).
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    // SSH agent socket (needed for git operations; not a secret itself).
    "SSH_AUTH_SOCK",
];

/// Credential-like name patterns — env vars whose names match these are
/// stripped even if they appear in `ENV_ALLOWLIST` (belt-and-suspenders).
pub const ENV_SENSITIVE_PATTERNS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "KEY",
    "CREDENTIAL",
    "AUTH",
    "API_KEY",
    "APIKEY",
    "AWS_",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "NPM_TOKEN",
    "PYPI_TOKEN",
];

/// Build a scrubbed environment map for a child process.
///
/// - Passes through only keys in [`ENV_ALLOWLIST`].
/// - Additionally drops any key matching a sensitive pattern (case-insensitive).
///
/// Intended to be applied as `Command::env_clear().envs(scrub_env())` so the
/// child starts from a known-empty environment, not the inherited one.
pub fn scrub_env() -> HashMap<String, String> {
    std::env::vars()
        .filter(|(k, _)| {
            let upper = k.to_uppercase();
            let in_allowlist = ENV_ALLOWLIST.contains(&k.as_str());
            let is_sensitive = ENV_SENSITIVE_PATTERNS.iter().any(|p| upper.contains(*p));
            in_allowlist && !is_sensitive
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pure-logic check: we can't mutate the process env in isolation (and
    // `std::env::set_var` is `unsafe` + racy under parallel tests), so verify
    // the filter against known inputs by reusing the exact allowlist/pattern
    // rule `scrub_env` applies.
    fn keep(k: &str) -> bool {
        let upper = k.to_uppercase();
        ENV_ALLOWLIST.contains(&k) && !ENV_SENSITIVE_PATTERNS.iter().any(|p| upper.contains(*p))
    }

    #[test]
    fn keeps_safe_vars_and_strips_secrets() {
        assert!(keep("PATH"));
        assert!(keep("HOME"));
        assert!(keep("LANG"));
        assert!(!keep("GITHUB_TOKEN"));
        assert!(!keep("AWS_SECRET_ACCESS_KEY"));
        assert!(!keep("MY_API_KEY"));
        // Not in the allowlist at all → dropped even though it's non-sensitive.
        assert!(!keep("GITHUB_ACTIONS"));
    }

    #[test]
    fn scrub_env_output_is_allowlist_bounded() {
        // Whatever the ambient env is (incl. CI secrets), the result must only
        // contain allowlisted, non-sensitive keys.
        for (k, _) in scrub_env() {
            assert!(
                ENV_ALLOWLIST.contains(&k.as_str()),
                "scrub_env leaked non-allowlisted key: {k}"
            );
            let upper = k.to_uppercase();
            assert!(
                !ENV_SENSITIVE_PATTERNS.iter().any(|p| upper.contains(*p)),
                "scrub_env leaked sensitive key: {k}"
            );
        }
    }
}
