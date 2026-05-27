//! "What's New" surface — shown two ways:
//!   1. A full section in the no-subcommand banner (always visible there).
//!   2. A one-line upgrade notice on any subcommand, printed once per
//!      MAJOR.MINOR change (npm-style) so it never spams across patch
//!      bumps and never pollutes piped/`--json` stdout.
//!
//! Highlights live here as the single source — the banner imports them.
//! The `HEADLINE` version is derived from Cargo at compile time; update only
//! its prose tagline + `HIGHLIGHTS` on each release that's worth shouting.

use std::io::IsTerminal;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One-line headline for the current release. The `MAJOR.MINOR` is built from
/// Cargo's version at compile time (`concat!` + `env!`) so the auto-version
/// hook can never desync the headline from the binary it ships in (BWOC-32).
pub const HEADLINE: &str = concat!(
    "BWOC ",
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR"),
    " — installable plugins & skills + ISO audits"
);

/// Short highlight bullets for the current MAJOR.MINOR. Keep ≤6, each a
/// single line — they render in the banner and inform the upgrade notice.
pub const HIGHLIGHTS: &[&str] = &[
    "Installable plugins — `bwoc plugin install/list` (git/tarball, SHA-256 gate)",
    "Installable skills — `bwoc skill` (verify gates opt-in via `--run-gates`)",
    "ISO-compliance audit plugins — `bwoc audit` (9001 · 27001 · 20000-1 · 29110)",
    "Signed-attestation evidence model for audit findings",
    "Prior 2.6.0: ed25519 signed messages + durable Saṅgha harness-v2",
];

/// `MAJOR.MINOR` of the current build (the patch component churns on every
/// edit via the auto-version hook, so the upgrade notice keys on the
/// release-significant prefix only).
fn major_minor() -> String {
    let mut it = VERSION.split('.');
    let major = it.next().unwrap_or("0");
    let minor = it.next().unwrap_or("0");
    format!("{major}.{minor}")
}

/// Print a one-line "you upgraded" notice to **stderr** if the stored
/// last-seen MAJOR.MINOR differs from this build, then record the current
/// one. No-op when:
///   - stdout is not a TTY (pipes / CI / `--json` consumers)
///   - `BWOC_NO_WHATSNEW=1` is set
///   - `~/.bwoc/` is unavailable (best-effort — never blocks a command)
///
/// Call this for subcommands only; the bare-`bwoc` banner already shows
/// the full What's New block.
pub fn notify_if_updated() {
    if std::env::var_os("BWOC_NO_WHATSNEW").is_some() {
        return;
    }
    // Gate on stdout TTY so piped/scripted output stays clean even though
    // we print to stderr (a consumer tailing both shouldn't get surprised).
    if !std::io::stdout().is_terminal() {
        return;
    }
    let Ok(home) = crate::user_home::bwoc_home() else {
        return;
    };
    let marker = home.join("last-seen-version");
    let current = major_minor();
    let seen = std::fs::read_to_string(&marker)
        .ok()
        .map(|s| s.trim().to_string());
    if seen.as_deref() == Some(current.as_str()) {
        return; // already greeted on this MAJOR.MINOR
    }
    // Record first so a write failure doesn't loop the notice forever.
    let _ = std::fs::write(&marker, &current);

    let tty = std::io::stderr().is_terminal();
    let (cyan, dim, reset) = if tty {
        ("\x1b[1;36m", "\x1b[2m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    eprintln!(
        "{cyan}✨ {HEADLINE}{reset}  {dim}(run `bwoc` for what's new · `BWOC_NO_WHATSNEW=1` to hush){reset}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_minor_strips_patch() {
        // VERSION is the live Cargo version (e.g. 2.0.48) — assert the
        // prefix shape, not a literal that the auto-version hook churns.
        let mm = major_minor();
        let parts: Vec<&str> = mm.split('.').collect();
        assert_eq!(parts.len(), 2, "major.minor has exactly two parts");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }

    #[test]
    fn highlights_are_lean() {
        assert!(!HIGHLIGHTS.is_empty());
        assert!(HIGHLIGHTS.len() <= 6, "keep the What's New list short");
        assert!(HIGHLIGHTS.iter().all(|h| !h.contains('\n')));
    }

    #[test]
    fn headline_version_matches_build() {
        // Guard against the stale-HEADLINE class of bug: the headline must
        // name the current MAJOR.MINOR, so a `bwoc` build never greets users
        // with a version it isn't. Bumping Cargo without updating HEADLINE
        // fails here (same lesson as the formula auto-bump, #52).
        let expected = format!("BWOC {}", major_minor());
        assert!(
            HEADLINE.starts_with(&expected),
            "HEADLINE {HEADLINE:?} must start with {expected:?} (CARGO_PKG_VERSION major.minor)"
        );
    }
}
