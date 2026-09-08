//! Cross-module helpers for `bwoc-cli`. Time helpers live in `bwoc-core::time`
//! since both `bwoc-cli` and `bwoc-agent` consume them.

use std::path::{Component, Path};

pub use bwoc_core::time::utc_now_iso8601;

/// Reject a tar listing that contains members which would escape the
/// extraction directory. `listing` is the stdout of `tar -tzf`, one member
/// path per line. Returns `Err` naming the first offending member.
///
/// SECURITY (BWOC-38): the install paths extract untrusted archives. A crafted
/// tarball can carry members with `..` traversal components or absolute paths
/// that, on extraction, write outside the staged directory (tar-slip). Both
/// `skill install` and `plugin install` MUST call this on the archive listing
/// BEFORE running `tar -xzf`.
pub fn assert_safe_tar_listing(listing: &str) -> Result<(), String> {
    for raw in listing.lines() {
        let member = raw.trim_end_matches('\r');
        if member.is_empty() {
            continue;
        }
        assert_safe_tar_member(member)?;
    }
    Ok(())
}

fn assert_safe_tar_member(member: &str) -> Result<(), String> {
    // Absolute paths ignore the `-C <dir>` extraction root entirely.
    if member.starts_with('/') || member.starts_with('\\') {
        return Err(format!("unsafe tar member '{member}': absolute path"));
    }
    // `Path::components` normalizes `.` and collapses separators, so this also
    // catches forms like `a/../../etc/passwd`.
    for comp in Path::new(member).components() {
        match comp {
            Component::ParentDir => {
                return Err(format!(
                    "unsafe tar member '{member}': '..' path-traversal component"
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("unsafe tar member '{member}': absolute path"));
            }
            _ => {}
        }
    }
    Ok(())
}

/// The framework version a plugin's `[plugin].compat` range is resolved
/// against. Canonical in `Cargo.toml` `[workspace.package].version`.
pub const FRAMEWORK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Resolve a plugin's `[plugin].compat` range against a framework version.
///
/// `PLUGINS.en.md` has promised since 2.5 that the framework "refuses to load"
/// a plugin whose `compat` does not match, and that `bwoc check` gates the
/// range as parseable. Neither was ever implemented: the field was parsed,
/// printed, and otherwise ignored. This is the single source of truth that
/// closes both halves, shared by the static manifest check (`check.rs`) and the
/// runtime resolvers so the two cannot drift — the same pattern
/// [`validate_plugin_entry`] establishes.
///
/// **Ranges must be bounded above.** An open-ended `>=3.0.0` matches 4.0, 9.0
/// and everything after, so a plugin written against today's contracts would
/// keep claiming compatibility with majors that break them — which makes
/// `compat` decorative exactly when it matters. `>=3.0.0, <4.0.0` states what a
/// plugin author can actually vouch for, and re-declaring it at the next major
/// is the point of the exercise, not a cost of it.
pub fn check_plugin_compat(compat: &str, framework: &str) -> Result<(), String> {
    let req = semver::VersionReq::parse(compat)
        .map_err(|e| format!("[plugin].compat {compat:?} is not a valid semver range: {e}"))?;
    let version = semver::Version::parse(framework)
        .map_err(|e| format!("framework version {framework:?} is not valid semver: {e}"))?;

    if req.matches(&version) {
        return Ok(());
    }
    Err(format!(
        "[plugin].compat {compat:?} does not match this framework ({framework})"
    ))
}

/// Does this range leave its upper end open?
///
/// Separate from [`check_plugin_compat`] because it is advice, not a refusal: a
/// plugin whose range is open-ended still *works* today. It is reported so the
/// ecosystem converges on bounded ranges before the next major, rather than
/// discovering at 4.0 that no plugin ever declared a ceiling.
pub fn compat_range_is_unbounded(compat: &str) -> bool {
    let Ok(req) = semver::VersionReq::parse(compat) else {
        return false; // Unparseable is a separate, louder finding.
    };
    !req.comparators.iter().any(|c| {
        matches!(
            c.op,
            semver::Op::Less | semver::Op::LessEq | semver::Op::Caret | semver::Op::Tilde
        ) || c.op == semver::Op::Exact
    })
}

/// Reject a plugin `[plugin].entry` value that could escape the plugin
/// directory and execute an arbitrary host binary (path-traversal RCE).
///
/// `bwoc audit run` spawns the entry via `Command::new(plugin_dir.join(entry))`.
/// `Path::join` makes an absolute `entry` (`/tmp/evil`) discard `plugin_dir`
/// entirely, and a `..` component (`../../../../tmp/evil`) climbs out of it —
/// either way an attacker-authored manifest runs an arbitrary program. A safe
/// entry is EITHER a bare program name resolved on `PATH`, OR a relative path
/// that stays contained within the plugin directory. This is the single source
/// of truth shared by the runtime guard (`audit.rs`) and the static manifest
/// check (`check.rs`) so the two cannot drift.
pub fn validate_plugin_entry(entry: &str) -> Result<(), String> {
    for component in Path::new(entry).components() {
        match component {
            Component::ParentDir => {
                return Err(format!(
                    "[plugin].entry '{entry}' contains a '..' component — \
                     entry must stay within the plugin directory"
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "[plugin].entry '{entry}' is an absolute path — entry must be a \
                     bare program name or a relative path contained in the plugin directory"
                ));
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_members() {
        let listing = "pkg-1.0/\npkg-1.0/manifest.toml\npkg-1.0/SPEC.md\npkg-1.0/sub/dir/file";
        assert!(assert_safe_tar_listing(listing).is_ok());
    }

    #[test]
    fn accepts_leading_dot_slash() {
        // `./pkg/file` normalizes to `pkg/file` — no traversal.
        assert!(assert_safe_tar_listing("./pkg/file\n").is_ok());
    }

    #[test]
    fn rejects_parent_dir_member() {
        let err = assert_safe_tar_listing("pkg/../../etc/passwd\n").unwrap_err();
        assert!(err.contains("traversal"), "{err}");
    }

    #[test]
    fn rejects_bare_parent_dir() {
        assert!(assert_safe_tar_listing("../evil\n").is_err());
    }

    #[test]
    fn rejects_absolute_member() {
        let err = assert_safe_tar_listing("/etc/passwd\n").unwrap_err();
        assert!(err.contains("absolute"), "{err}");
    }

    #[test]
    fn rejects_backslash_absolute_member() {
        assert!(assert_safe_tar_listing("\\windows\\system32\n").is_err());
    }

    #[test]
    fn one_bad_member_among_good_fails() {
        let listing = "pkg/ok\npkg/also-ok\npkg/../../escape\npkg/more";
        assert!(assert_safe_tar_listing(listing).is_err());
    }

    #[test]
    fn entry_accepts_bare_name() {
        assert!(validate_plugin_entry("audit.sh").is_ok());
    }

    #[test]
    fn entry_accepts_contained_relative() {
        assert!(validate_plugin_entry("bin/audit.sh").is_ok());
    }

    #[test]
    fn entry_rejects_parent_traversal() {
        let err = validate_plugin_entry("../../../../tmp/evil").unwrap_err();
        assert!(err.contains(".."), "{err}");
    }

    #[test]
    fn entry_rejects_absolute() {
        let err = validate_plugin_entry("/tmp/evil").unwrap_err();
        assert!(err.contains("absolute"), "{err}");
    }

    // ── [plugin].compat ──────────────────────────────────────────────────────

    #[test]
    fn compat_matches_inside_the_declared_range() {
        assert!(check_plugin_compat(">=3.0.0, <4.0.0", "3.0.0").is_ok());
        assert!(check_plugin_compat(">=3.0.0, <4.0.0", "3.7.2").is_ok());
        assert!(check_plugin_compat(">=2.7.0", "2.44.2").is_ok());
    }

    #[test]
    fn compat_refuses_outside_the_declared_range() {
        let err = check_plugin_compat(">=3.0.0, <4.0.0", "2.44.2").unwrap_err();
        assert!(err.contains("does not match"), "{err}");
        assert!(check_plugin_compat(">=3.0.0, <4.0.0", "4.0.0").is_err());
    }

    #[test]
    fn an_unparseable_range_is_its_own_error() {
        // Distinguished from a mismatch by the caller: an unparseable range is
        // a malformed manifest (a violation), a mismatch is a plugin that has
        // not been re-declared for this major (a warning until it is used).
        let err = check_plugin_compat("not a range", "3.0.0").unwrap_err();
        assert!(err.contains("not a valid semver range"), "{err}");
    }

    #[test]
    fn a_missing_compat_is_refused_rather_than_assumed_compatible() {
        // `#[serde(default)]` in the resolvers yields an empty string; it must
        // not read as "compatible with everything".
        assert!(check_plugin_compat("", "3.0.0").is_err());
    }

    #[test]
    fn unbounded_ranges_are_recognised() {
        assert!(compat_range_is_unbounded(">=3.0.0"));
        assert!(compat_range_is_unbounded(">2.0.0"));
        assert!(!compat_range_is_unbounded(">=3.0.0, <4.0.0"));
        assert!(!compat_range_is_unbounded("^3.0.0"));
        assert!(!compat_range_is_unbounded("~3.1"));
        assert!(!compat_range_is_unbounded("=3.0.0"));
        // Unparseable is a louder, separate finding — not reported as unbounded.
        assert!(!compat_range_is_unbounded("garbage"));
    }

    #[test]
    fn the_framework_version_is_parseable_semver() {
        // If this ever fails, every compat check in the binary fails with it.
        assert!(
            semver::Version::parse(FRAMEWORK_VERSION).is_ok(),
            "FRAMEWORK_VERSION = {FRAMEWORK_VERSION:?}"
        );
    }
}
