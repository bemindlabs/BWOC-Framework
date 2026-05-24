//! Workspace document-kind registry.
//!
//! A `DocKind` describes a first-class document category that lives under a
//! well-known directory and follows the `YYYY-MM-DD_<slug>.md` naming
//! convention.  All kinds are committed to the repository (`committed: true`).
//!
//! The built-in lookup covers three kinds: `notes`, `retrospectives`, and
//! `research`.  One generic engine (`bwoc-cli::doc_cmd`) consumes the
//! descriptor — there are no per-kind code paths.
//!
//! # Extension points
//!
//! ## Workspace-declared custom kinds (TODO #12 — implemented)
//!
//! A workspace may declare extra kinds in `.bwoc/doc-kinds.toml`:
//!
//! ```toml
//! [[kind]]
//! name = "decision"
//! dir = "decisions"
//! committed = true
//! template = "# {date} — <Decision>\n\n## Context\n\n## Decision\n\n## Consequences\n"
//! # OR: template_file = ".bwoc/templates/decision.md"
//! ```
//!
//! Resolution order: built-in first, then `.bwoc/doc-kinds.toml`.
//! Unknown kind → error listing all available kinds.

use std::path::Path;

// ---------------------------------------------------------------------------
// DocKind — unified built-in + custom descriptor
// ---------------------------------------------------------------------------

/// Descriptor for a workspace document kind.
///
/// Represents both built-in (static) and workspace-declared custom kinds.
/// Two `DocKind` values are equal when they have the same `name`.
#[derive(Debug, Clone)]
pub struct DocKind {
    /// Short name used in CLI commands (e.g. `"notes"`).
    pub name: String,
    /// Workspace-relative directory where documents of this kind are stored.
    pub dir: String,
    /// Whether documents of this kind are tracked in version control.
    pub committed: bool,
    /// Template source — resolved lazily.
    template_source: TemplateSource,
}

/// How the template for a custom kind is stored.
#[derive(Debug, Clone)]
enum TemplateSource {
    /// Static function — used by built-ins.
    Fn(fn() -> String),
    /// Inline string from `[[kind]].template`.
    Inline(String),
    /// Path relative to the workspace root from `[[kind]].template_file`.
    File(String),
}

impl PartialEq for DocKind {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for DocKind {}

impl DocKind {
    /// Return the starter template for a new document of this kind.
    ///
    /// For `File` sources, the workspace root must be passed; if absent or
    /// the file cannot be read the inline/fn fallback is used gracefully.
    pub fn template(&self) -> String {
        self.template_with_root(None)
    }

    /// Return the starter template, optionally reading from the workspace root
    /// for `template_file`-based custom kinds.
    pub fn template_with_root(&self, workspace_root: Option<&Path>) -> String {
        match &self.template_source {
            TemplateSource::Fn(f) => f(),
            TemplateSource::Inline(s) => s.clone(),
            TemplateSource::File(rel_path) => {
                if let Some(root) = workspace_root {
                    let full = root.join(rel_path);
                    if let Ok(content) = std::fs::read_to_string(&full) {
                        return content;
                    }
                }
                // Graceful fallback: generic template with the file path hinted.
                format!("# YYYY-MM-DD — <Title>\n\n<!-- template_file: {rel_path} not found -->\n")
            }
        }
    }

    // -- Private constructors ------------------------------------------------

    fn from_static(
        name: &'static str,
        dir: &'static str,
        committed: bool,
        template_fn: fn() -> String,
    ) -> Self {
        Self {
            name: name.to_string(),
            dir: dir.to_string(),
            committed,
            template_source: TemplateSource::Fn(template_fn),
        }
    }
}

// ---------------------------------------------------------------------------
// Templates for built-in kinds
// ---------------------------------------------------------------------------

fn notes_template() -> String {
    // Mirrors the note skeleton in CLAUDE.md §"Implementation Logs".
    "# YYYY-MM-DD — <Title>\n\
     \n\
     <one-paragraph summary>\n\
     \n\
     ## What changed\n\
     \n\
     ## Decisions\n\
     \n\
     ## Alternatives considered\n\
     \n\
     ## Bugs surfaced and fixed\n\
     \n\
     ## Status / deferred\n\
     \n\
     ## Related (links)\n"
        .to_string()
}

fn retrospectives_template() -> String {
    // Paññā-3 structure from AGENTS.md §11 Self-Improvement.
    "# YYYY-MM-DD — <Sprint / Session Title>\n\
     \n\
     ## Sutamayā (what the data / docs say)\n\
     \n\
     ## Cintāmayā (synthesis — what pattern emerged)\n\
     \n\
     ## Bhāvanāmayā (action — feedback / memory saved)\n\
     \n\
     ## Metrics\n\
     \n\
     | Metric | Value |\n\
     |---|---|\n\
     | tasks_attempted | |\n\
     | tasks_completed | |\n\
     | gate_pass_rate  | |\n"
        .to_string()
}

fn research_template() -> String {
    "# YYYY-MM-DD — <Research Title>\n\
     \n\
     ## Question\n\
     \n\
     ## Scope\n\
     \n\
     ## Sources (Sutamayā)\n\
     \n\
     ## Findings\n\
     \n\
     ## Recommendation\n"
        .to_string()
}

// ---------------------------------------------------------------------------
// Built-in kinds
// ---------------------------------------------------------------------------

fn builtin_notes() -> DocKind {
    DocKind::from_static("notes", "notes/", true, notes_template)
}

fn builtin_retrospectives() -> DocKind {
    DocKind::from_static(
        "retrospectives",
        "retrospectives/",
        true,
        retrospectives_template,
    )
}

fn builtin_research() -> DocKind {
    DocKind::from_static("research", "research/", true, research_template)
}

/// All built-in kinds, in stable order.
fn builtin_kinds() -> [DocKind; 3] {
    [
        builtin_notes(),
        builtin_retrospectives(),
        builtin_research(),
    ]
}

/// Look up a built-in `DocKind` by name.  Returns `None` for unknown names.
///
/// # Examples
/// ```
/// use bwoc_core::doc_kind::kind;
/// assert!(kind("notes").is_some());
/// assert!(kind("unknown").is_none());
/// ```
pub fn kind(name: &str) -> Option<DocKind> {
    builtin_kinds().into_iter().find(|k| k.name == name)
}

// ---------------------------------------------------------------------------
// Workspace-declared custom kinds  (Feature A — TODO #12)
// ---------------------------------------------------------------------------

/// TOML shape for a single `[[kind]]` entry in `.bwoc/doc-kinds.toml`.
#[derive(serde::Deserialize)]
struct RawKindEntry {
    name: String,
    dir: String,
    #[serde(default = "default_committed")]
    committed: bool,
    /// Inline template string.
    template: Option<String>,
    /// Path relative to workspace root of a file containing the template.
    template_file: Option<String>,
}

fn default_committed() -> bool {
    true
}

/// Top-level shape of `.bwoc/doc-kinds.toml`.
#[derive(serde::Deserialize)]
struct RawDocKindsFile {
    #[serde(default, rename = "kind")]
    kinds: Vec<RawKindEntry>,
}

/// Load custom kinds declared in `<workspace_root>/.bwoc/doc-kinds.toml`.
///
/// Returns an empty `Vec` (never errors) when the file is absent or
/// unparseable — custom kinds are additive, not required.
pub fn load_custom_kinds(workspace_root: &Path) -> Vec<DocKind> {
    let path = workspace_root.join(".bwoc/doc-kinds.toml");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let raw: RawDocKindsFile = match toml::from_str(&content) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    raw.kinds
        .into_iter()
        .map(|entry| {
            let template_source = match (entry.template, entry.template_file) {
                (Some(inline), _) => TemplateSource::Inline(inline),
                (None, Some(file_path)) => TemplateSource::File(file_path),
                (None, None) => {
                    // Fallback: generic template for kinds with no template specified.
                    TemplateSource::Inline(format!(
                        "# YYYY-MM-DD — <Title>\n\n<!-- {} document -->\n",
                        entry.name
                    ))
                }
            };
            DocKind {
                name: entry.name,
                dir: entry.dir,
                committed: entry.committed,
                template_source,
            }
        })
        .collect()
}

/// Resolve a kind by name against both built-ins and workspace-declared custom
/// kinds.  Resolution order: built-in first, then custom.
///
/// Returns `Ok(DocKind)` on success, or `Err(String)` listing all available
/// kind names for a clear error message.
pub fn resolve_kind(name: &str, workspace_root: &Path) -> Result<DocKind, String> {
    // 1. Built-in?
    if let Some(k) = kind(name) {
        return Ok(k);
    }

    // 2. Custom?
    let custom = load_custom_kinds(workspace_root);
    if let Some(k) = custom.iter().find(|k| k.name == name) {
        return Ok(k.clone());
    }

    // 3. Build a helpful error message listing all available kinds.
    let mut available: Vec<String> = builtin_kinds().iter().map(|k| k.name.clone()).collect();
    for k in &custom {
        available.push(k.name.clone());
    }
    Err(format!(
        "unknown document kind '{}'. Available: {}",
        name,
        available.join(", ")
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    // ---- built-in resolution -----------------------------------------------

    #[test]
    fn known_kinds_resolve() {
        for name in ["notes", "retrospectives", "research"] {
            let k = kind(name).unwrap_or_else(|| panic!("kind '{name}' not found"));
            assert_eq!(k.name, name);
            assert!(k.committed);
            let tmpl = k.template();
            assert!(!tmpl.is_empty(), "template for '{name}' is empty");
            assert!(tmpl.contains("##"), "template for '{name}' has no sections");
        }
    }

    #[test]
    fn unknown_kind_returns_none() {
        assert!(kind("unknown").is_none());
        assert!(kind("").is_none());
        assert!(kind("NOTES").is_none()); // case-sensitive
    }

    #[test]
    fn notes_dir() {
        assert_eq!(kind("notes").unwrap().dir, "notes/");
    }

    #[test]
    fn retrospectives_dir() {
        assert_eq!(kind("retrospectives").unwrap().dir, "retrospectives/");
    }

    #[test]
    fn research_dir() {
        assert_eq!(kind("research").unwrap().dir, "research/");
    }

    // ---- custom kinds loading -----------------------------------------------

    #[test]
    fn custom_kind_inline_template() {
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        fs::create_dir_all(&bwoc_dir).unwrap();
        // Use TOML multi-line literal string to avoid escape-sequence issues.
        let toml_content = "[[kind]]\nname = \"decision\"\ndir = \"decisions\"\ncommitted = true\ntemplate = \"Decision template content\"\n";
        fs::write(bwoc_dir.join("doc-kinds.toml"), toml_content).unwrap();

        let custom = load_custom_kinds(dir.path());
        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].name, "decision");
        assert_eq!(custom[0].dir, "decisions");
        assert!(custom[0].committed);
        let tmpl = custom[0].template();
        assert!(tmpl.contains("Decision"));
    }

    #[test]
    fn custom_kind_template_file() {
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        let tmpl_dir = bwoc_dir.join("templates");
        fs::create_dir_all(&tmpl_dir).unwrap();
        fs::write(tmpl_dir.join("adr.md"), "# ADR\n\n## Status\n").unwrap();
        fs::write(
            bwoc_dir.join("doc-kinds.toml"),
            r#"
[[kind]]
name = "adr"
dir = "adrs"
committed = true
template_file = ".bwoc/templates/adr.md"
"#,
        )
        .unwrap();

        let custom = load_custom_kinds(dir.path());
        assert_eq!(custom.len(), 1);
        let tmpl = custom[0].template_with_root(Some(dir.path()));
        assert!(tmpl.contains("ADR"));
    }

    #[test]
    fn custom_kind_template_file_missing_falls_back() {
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        fs::create_dir_all(&bwoc_dir).unwrap();
        fs::write(
            bwoc_dir.join("doc-kinds.toml"),
            r#"
[[kind]]
name = "adr"
dir = "adrs"
template_file = ".bwoc/templates/missing.md"
"#,
        )
        .unwrap();

        let custom = load_custom_kinds(dir.path());
        // Should not panic; fallback template contains the path hint.
        let tmpl = custom[0].template_with_root(Some(dir.path()));
        assert!(tmpl.contains("missing.md"));
    }

    #[test]
    fn load_custom_kinds_absent_file_returns_empty() {
        let dir = tmp();
        let custom = load_custom_kinds(dir.path());
        assert!(custom.is_empty());
    }

    #[test]
    fn load_custom_kinds_malformed_toml_returns_empty() {
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        fs::create_dir_all(&bwoc_dir).unwrap();
        fs::write(bwoc_dir.join("doc-kinds.toml"), "not valid toml [[[").unwrap();
        let custom = load_custom_kinds(dir.path());
        assert!(custom.is_empty());
    }

    // ---- resolve_kind -------------------------------------------------------

    #[test]
    fn resolve_builtin_kind() {
        let dir = tmp();
        let k = resolve_kind("notes", dir.path()).unwrap();
        assert_eq!(k.name, "notes");
    }

    #[test]
    fn resolve_custom_kind() {
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        fs::create_dir_all(&bwoc_dir).unwrap();
        let toml = "[[kind]]\nname = \"decision\"\ndir = \"decisions\"\n";
        fs::write(bwoc_dir.join("doc-kinds.toml"), toml).unwrap();

        let k = resolve_kind("decision", dir.path()).unwrap();
        assert_eq!(k.name, "decision");
    }

    #[test]
    fn resolve_unknown_kind_error_lists_available() {
        let dir = tmp();
        let err = resolve_kind("nonexistent", dir.path()).unwrap_err();
        assert!(err.contains("nonexistent"));
        assert!(err.contains("notes"));
        assert!(err.contains("retrospectives"));
        assert!(err.contains("research"));
    }

    #[test]
    fn builtin_wins_over_custom_same_name() {
        // If a custom kind has the same name as a built-in, the built-in wins.
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        fs::create_dir_all(&bwoc_dir).unwrap();
        let toml = "[[kind]]\nname = \"notes\"\ndir = \"custom-notes\"\n";
        fs::write(bwoc_dir.join("doc-kinds.toml"), toml).unwrap();

        let k = resolve_kind("notes", dir.path()).unwrap();
        // Built-in dir wins.
        assert_eq!(k.dir, "notes/");
    }
}
