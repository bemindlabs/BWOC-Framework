//! Schema versioning seam — one convention every on-disk BWOC format shares.
//!
//! # Why this exists
//!
//! Through 2.x, exactly one artifact on disk could say which revision it was
//! written against: the manifest's `trust` block ([`crate::manifest::TrustBlock`],
//! whose `schemaVersion` is required precisely because trust semantics ride on
//! the block being well-formed). Every other format — `workspace.toml`,
//! `agents.toml`, `routes.toml`, `peers.toml`, `harness-policy.toml`,
//! `doc-kinds.toml` — was unversioned, which means a future revision of any of
//! them could only fail *silently*: an old binary reading a new file, or a new
//! binary reading an old file, with no way to tell the two apart.
//!
//! 3.0 closes that. Every format BWOC owns carries a top-level marker, and this
//! module is the single place that decides what the numbers mean.
//!
//! # Spelling
//!
//! The key follows the casing its own file already uses: `schema_version` in
//! TOML (which is snake_case throughout — `agents_dir`, `default_mode`) and
//! `schemaVersion` in JSON (camelCase throughout, the spelling `TrustBlock`
//! already uses). Same marker, read the way a reader of that file expects.
//!
//! # What is deliberately *not* versioned
//!
//! `.bwoc/doc-kinds.toml` (additive, never written by BWOC, unparseable already
//! degrades to "no custom kinds"), `.bwoc/secrets.toml` (a secret store),
//! `.bwoc/installed-sources.toml` and `.bwoc/teams/*.toml` (derived from
//! commands that rewrite them wholesale), and every `*.jsonl` append-only
//! stream. A marker earns its place only where a reader could act on it.
//!
//! # The Anicca seam
//!
//! Formats change; the *reading* of a format must survive the change. Two rules
//! give that, and they are opposite ends of the same seam:
//!
//! - **Absent ⇒ [`SchemaVersion::LEGACY`].** A file written by 2.x carries no
//!   marker, so `#[serde(default)]` resolves it to v2 rather than failing. 3.0
//!   reads it, warns, and points at `bwoc migrate`. This is what makes the
//!   3.0 upgrade non-destructive.
//! - **Unknown fields are ignored** (serde's default; the workspace sets
//!   `deny_unknown_fields` nowhere). So a 2.x binary reading a file this
//!   version wrote skips the marker and keeps working — downgrade stays
//!   possible for as long as the *content* is compatible.
//!
//! A version *ahead* of [`SchemaVersion::CURRENT`] is the one case that is not
//! forgiving: it was written by a newer BWOC whose semantics this build cannot
//! know, so callers should refuse rather than guess. See [`SchemaStatus`].
//!
//! # Support window
//!
//! 3.x reads both v2 and v3 (`bwoc migrate` upgrades in place). v2 support is
//! removed in 4.0 — one major version of overlap, per
//! `docs/en/COMPATIBILITY.en.md`.

use serde::{Deserialize, Serialize};

/// The schema revision an on-disk artifact conforms to.
///
/// Serialized transparently, so a field of this type is written as a bare
/// integer (`schema_version = 3`) and reads back from one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaVersion(pub u32);

impl SchemaVersion {
    /// What this build writes, and the only revision it fully understands.
    pub const CURRENT: Self = Self(3);

    /// What an unmarked file is assumed to be: everything BWOC 2.x wrote.
    ///
    /// Readable through the 3.x line; removed in 4.0.
    pub const LEGACY: Self = Self(2);

    /// How this build should treat the artifact.
    pub fn status(self) -> SchemaStatus {
        match self.0 {
            v if v == Self::CURRENT.0 => SchemaStatus::Current,
            v if v < Self::CURRENT.0 => SchemaStatus::Legacy,
            _ => SchemaStatus::Future,
        }
    }

    /// Written against this revision — nothing to do.
    pub fn is_current(self) -> bool {
        matches!(self.status(), SchemaStatus::Current)
    }

    /// Readable, but `bwoc migrate` should be run before 4.0 lands.
    pub fn is_legacy(self) -> bool {
        matches!(self.status(), SchemaStatus::Legacy)
    }

    /// Written by a newer BWOC. Refuse — guessing at semantics this build does
    /// not have is how a control-plane file gets misread.
    pub fn is_future(self) -> bool {
        matches!(self.status(), SchemaStatus::Future)
    }
}

/// An unmarked file is a 2.x file. This is the `#[serde(default)]` value for
/// every marker field, and the reason 3.0 can read a 2.x workspace.
impl Default for SchemaVersion {
    fn default() -> Self {
        Self::LEGACY
    }
}

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// What a [`SchemaVersion`] means to *this* build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaStatus {
    /// Matches [`SchemaVersion::CURRENT`].
    Current,
    /// Older than current — readable, migratable, deprecated.
    Legacy,
    /// Newer than current — this build must not interpret it.
    Future,
}

/// Read a top-level `schema_version` out of already-parsed TOML.
///
/// For the hand-authored control-plane files BWOC reads but never rewrites
/// (`peers.toml`, `harness-policy.toml`), where the marker has to be pulled
/// from a loosely-typed value rather than a `#[serde(default)]` field. An
/// absent or non-integer marker reads as [`SchemaVersion::LEGACY`], same as
/// everywhere else.
pub fn marker_from_toml(value: &toml::Value) -> SchemaVersion {
    value
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .map(SchemaVersion)
        .unwrap_or_default()
}

/// The formats that carry a marker, and how they get one.
///
/// Paths are relative to the artifact's own root — a workspace root for the
/// first three, an agent directory for the last two. The split matters to
/// `bwoc migrate`: BWOC owns the first group and rewrites it wholesale, but
/// only *reads* the second, so migrating those means prepending the marker line
/// rather than reserializing someone's hand-written file and losing their
/// comments.
///
/// `.bwoc/doc-kinds.toml` is deliberately absent: nothing writes it, nothing
/// validates it, and an unparseable one already degrades to "no custom kinds".
/// A marker on a file with no reader that could act on it earns nothing
/// (Mattaññutā).
pub const FRAMEWORK_OWNED_FORMATS: &[&str] = &[
    ".bwoc/workspace.toml",
    ".bwoc/agents.toml",
    ".bwoc/interconnect/routes.toml",
];

/// Control-plane files BWOC reads but must never reserialize — see
/// [`FRAMEWORK_OWNED_FORMATS`].
pub const HAND_AUTHORED_FORMATS: &[&str] = &[".bwoc/peers.toml", ".bwoc/harness-policy.toml"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_marker_reads_as_legacy() {
        #[derive(Deserialize)]
        struct Doc {
            #[serde(default)]
            schema_version: SchemaVersion,
        }
        let doc: Doc = toml::from_str("name = 'x'\n").unwrap();
        assert_eq!(doc.schema_version, SchemaVersion::LEGACY);
        assert!(doc.schema_version.is_legacy());
    }

    #[test]
    fn marker_roundtrips_as_bare_integer() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Doc {
            #[serde(default)]
            schema_version: SchemaVersion,
        }
        let doc = Doc {
            schema_version: SchemaVersion::CURRENT,
        };
        let out = toml::to_string(&doc).unwrap();
        assert_eq!(out.trim(), "schema_version = 3");
        let back: Doc = toml::from_str(&out).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn status_partitions_the_three_cases() {
        assert_eq!(SchemaVersion(2).status(), SchemaStatus::Legacy);
        assert_eq!(SchemaVersion(3).status(), SchemaStatus::Current);
        assert_eq!(SchemaVersion(4).status(), SchemaStatus::Future);
        assert!(SchemaVersion(99).is_future());
    }

    #[test]
    fn current_is_ahead_of_legacy() {
        assert!(SchemaVersion::CURRENT > SchemaVersion::LEGACY);
        assert_eq!(SchemaVersion::default(), SchemaVersion::LEGACY);
    }

    #[test]
    fn display_is_v_prefixed() {
        assert_eq!(SchemaVersion::CURRENT.to_string(), "v3");
    }

    #[test]
    fn marker_read_from_loose_toml() {
        let v: toml::Value = toml::from_str("schema_version = 3\n[[peer]]\nid = 'a'\n").unwrap();
        assert_eq!(marker_from_toml(&v), SchemaVersion::CURRENT);
    }

    #[test]
    fn absent_or_bogus_marker_in_loose_toml_is_legacy() {
        let absent: toml::Value = toml::from_str("[[peer]]\nid = 'a'\n").unwrap();
        assert_eq!(marker_from_toml(&absent), SchemaVersion::LEGACY);

        // A string, a float, a negative — none of them are a revision.
        for bad in [
            "schema_version = '3'",
            "schema_version = 3.5",
            "schema_version = -1",
        ] {
            let v: toml::Value = toml::from_str(bad).unwrap();
            assert_eq!(marker_from_toml(&v), SchemaVersion::LEGACY, "{bad}");
        }
    }

    #[test]
    fn format_lists_are_disjoint_and_bwoc_scoped() {
        for p in FRAMEWORK_OWNED_FORMATS.iter().chain(HAND_AUTHORED_FORMATS) {
            assert!(p.starts_with(".bwoc/"), "{p} is outside the control plane");
        }
        for p in HAND_AUTHORED_FORMATS {
            assert!(!FRAMEWORK_OWNED_FORMATS.contains(p), "{p} in both lists");
        }
    }
}
