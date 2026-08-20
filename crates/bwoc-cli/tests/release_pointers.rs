//! Release gate: **every "Latest release" pointer names the newest release.**
//!
//! `CHANGELOG.md` is the source of truth — its topmost released section is the
//! release being cut. `README.md` and `VERSION.md` each carry a hand-maintained
//! `**Latest release:**` line, and `Cargo.toml` carries the SemVer. Nothing tied
//! them together, and all three drifted:
//!
//! - the root README sat **two releases stale** (pointing at 2.42.0 while 2.44.0
//!   shipped),
//! - `VERSION.md` sat **two releases stale** in the very same way, caught only by
//!   a reviewer,
//! - and the Homebrew formula went five releases stale in an earlier era.
//!
//! Every one of those was a *convention with nothing enforcing it* — the same
//! shape as `bwoc-harness` silently missing from release archives because
//! packaging was never asserted (#460). This test is the enforcement: cut a
//! release without refreshing a pointer and the suite goes red, exactly as
//! `whats_new::tests::highlights_cite_current_version` already does for the
//! What's New prose.
//!
//! Deliberately **offline** — it compares repo files to each other, never to
//! the GitHub API, so it cannot flake and works in an air-gapped CI.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/bwoc-cli is two levels below the repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// Every **released** CHANGELOG section as `(tag, semver)`, newest first — e.g.
/// `[("v2026.8.20-1", "2.44.1"), ("v2026.8.20-0", "2.44.0"), …]`.
/// `## [Unreleased]` is skipped: it is a staging area, not a release.
fn releases() -> Vec<(String, String)> {
    let body = read("CHANGELOG.md");
    let mut out = Vec::new();
    for line in body.lines() {
        let Some(rest) = line.strip_prefix("## [") else {
            continue;
        };
        let Some((tag, tail)) = rest.split_once(']') else {
            continue;
        };
        if tag == "Unreleased" {
            continue;
        }
        // `## [v2026.8.20-1] — 2026-08-20 — 2.44.1`. Require an EXACT numeric
        // `MAJOR.MINOR.PATCH`: a loose "3 dot-parts starting with a digit" check
        // would swallow `2.44.1-beta` or `2.44.1 (hotfix)` and then fail later,
        // in a comparison whose message points at the wrong thing.
        let semver = tail.rsplit('—').next().map(str::trim).filter(|s| {
            let mut parts = s.split('.');
            let ok = (0..3).all(|_| {
                parts
                    .next()
                    .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            });
            ok && parts.next().is_none()
        });
        match semver {
            Some(v) => out.push((tag.to_string(), v.to_string())),
            // Early entries predate the `— <SemVer>` heading convention (e.g.
            // `— BWOC 2.0`). Skip them: they are history, not a release anyone
            // is cutting. The NEWEST heading is held to the convention below,
            // so a malformed current release still fails loudly rather than
            // silently comparing against an older one.
            None => assert!(
                !out.is_empty(),
                "the newest CHANGELOG release heading does not end in a SemVer, so no \
                 pointer can be checked against it: `{line}`"
            ),
        }
    }
    assert!(
        !out.is_empty(),
        "CHANGELOG.md has no released `## [vX] — date — SemVer` section"
    );
    out
}

/// The newest released `(tag, semver)`.
fn newest_release() -> (String, String) {
    releases().swap_remove(0)
}

/// The `**Latest release:**` line of a doc, or `None` when the doc has none.
fn latest_release_line(doc: &str) -> Option<String> {
    read(doc)
        .lines()
        .find(|l| l.starts_with("**Latest release:**"))
        .map(str::to_string)
}

#[test]
fn readme_and_version_point_at_the_newest_release() {
    let (tag, semver) = newest_release();
    for doc in ["README.md", "VERSION.md"] {
        let line = latest_release_line(doc)
            .unwrap_or_else(|| panic!("{doc} has no `**Latest release:**` line to check"));
        assert!(
            line.contains(&tag),
            "{doc}'s `Latest release` line does not name the newest CHANGELOG release `{tag}` \
             — refresh it as part of the release.\n  line: {line}"
        );
        assert!(
            line.contains(&semver),
            "{doc}'s `Latest release` line names `{tag}` but not its version `{semver}` \
             — the tag and SemVer must agree.\n  line: {line}"
        );
    }
}

#[test]
fn cargo_version_matches_the_newest_release() {
    let (tag, semver) = newest_release();
    let manifest = read("Cargo.toml");
    // Bounded to the `[workspace.package]` section — `skip_while` alone would
    // scan to EOF and happily read a `version = "…"` belonging to some later
    // table if this one ever lost its own.
    let declared = manifest
        .lines()
        .skip_while(|l| !l.starts_with("[workspace.package]"))
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .find_map(|l| l.strip_prefix("version = \""))
        .and_then(|v| v.split('"').next())
        .expect("[workspace.package] declares a version");
    assert_eq!(
        declared, semver,
        "Cargo.toml [workspace.package] version is `{declared}` but the newest CHANGELOG \
         release ({tag}) is `{semver}` — bump one or the other before cutting the release"
    );
}

/// The formula's `version` is the CalVer tag with `v` dropped and `-N` flattened
/// to `.N` (`v2026.8.20-1` -> `2026.8.20.1`).
fn formula_version_for(tag: &str) -> String {
    tag.trim_start_matches('v').replace('-', ".")
}

#[test]
fn the_homebrew_formula_is_at_most_one_release_behind() {
    // The formula once lagged FIVE releases, because bumping it is a separate PR
    // that only exists after the release artifacts do (its sha256s are computed
    // from them). So it legitimately trails during a release: the release PR
    // merges, the tag builds, and only then can the formula PR land — a window
    // of minutes to hours.
    //
    // Pinning the formula to the newest release would therefore paint `main` red
    // for that whole window and block unrelated PRs. Allowing exactly ONE release
    // of lag tolerates the real publish window while still catching sustained
    // drift: forget the formula PR and the *next* release trips this.
    let rels = releases();
    let formula = read("Formula/bwoc.rb");
    let declared = formula
        .lines()
        .find_map(|l| l.trim().strip_prefix("version \""))
        .and_then(|v| v.split('"').next())
        .expect("Formula/bwoc.rb declares a version")
        .to_string();

    let allowed: Vec<(String, String)> = rels
        .iter()
        .take(2)
        .map(|(t, _)| (t.clone(), formula_version_for(t)))
        .collect();

    let hit = allowed.iter().find(|(_, v)| *v == declared);
    assert!(
        hit.is_some(),
        "Formula/bwoc.rb version is `{declared}`, which is neither the newest release \
         ({}) nor the one before it ({}) — the formula bump is part of cutting a release, \
         not optional.",
        allowed[0].1,
        allowed.get(1).map(|a| a.1.as_str()).unwrap_or("n/a"),
    );

    // Whichever release it claims, EVERY per-platform download url must point
    // there. Checking that *some* url mentions the tag would let a single
    // missed platform through — and a stale url means that platform installs
    // the wrong binaries, exactly the failure this gate exists to catch.
    let (tag, _) = hit.expect("checked above");
    let urls: Vec<&str> = formula
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("url \""))
        .collect();
    assert!(
        !urls.is_empty(),
        "Formula/bwoc.rb declares no download urls to check"
    );
    let wrong: Vec<&&str> = urls
        .iter()
        .filter(|l| !l.contains(&format!("/download/{tag}/")))
        .collect();
    assert!(
        wrong.is_empty(),
        "Formula/bwoc.rb says version `{declared}` ({tag}) but {} of its {} download urls \
         point elsewhere — every platform must be bumped together:\n  {}",
        wrong.len(),
        urls.len(),
        wrong
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
