//! Generic workspace document-kind engine.
//!
//! All three CLI aliases (`bwoc notes`, `bwoc retro`, `bwoc research`) dispatch
//! into `run()` here — one code path for all kinds.  The `DocKind` descriptor
//! from `bwoc-core::doc_kind` drives every decision.
//!
//! # Retro metrics-prefill (Feature B — TODO #10)
//!
//! When `bwoc retro new` scaffolds a retrospective, `cmd_new` best-effort reads
//! `session-metrics.jsonl` from `metrics/` and/or `agents/*/metrics/` under the
//! workspace root and injects a summary into the `## Metrics` table.  Absent or
//! unparseable JSONL leaves the placeholder row unchanged — it never fails the
//! scaffold.

use std::fs;
use std::path::{Path, PathBuf};

use bwoc_core::doc_kind::DocKind;
use bwoc_core::time::utc_now_iso8601;

/// Actions available on every document kind.
pub enum DocAction {
    /// Create a new document with the given title.
    New { title: String },
    /// List documents in the kind's directory (newest first).
    List,
    /// Print a document that matches the given date prefix or exact filename.
    View { name: String },
}

/// Run a document-kind action relative to `workspace_root`.
///
/// Returns a UNIX-style exit code: 0 = success, 1 = user error, 2 = I/O error.
pub fn run(kind: DocKind, action: DocAction, workspace_root: &Path) -> i32 {
    match action {
        DocAction::New { title } => cmd_new(kind, title, workspace_root),
        DocAction::List => cmd_list(kind, workspace_root),
        DocAction::View { name } => cmd_view(kind, name, workspace_root),
    }
}

// ---------------------------------------------------------------------------
// New
// ---------------------------------------------------------------------------

fn cmd_new(kind: DocKind, title: String, root: &Path) -> i32 {
    let slug = slugify(&title);
    let date = date_prefix();
    let filename = format!("{date}_{slug}.md");

    let dir = root.join(kind.dir.trim_end_matches('/'));
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!(
            "bwoc {}: could not create directory {}: {e}",
            kind.name,
            dir.display()
        );
        return 2;
    }

    let path = dir.join(&filename);
    if path.exists() {
        eprintln!(
            "bwoc {}: file already exists — refusing to clobber: {}",
            kind.name,
            path.display()
        );
        return 1;
    }

    // Obtain base template, passing workspace root so template_file kinds can
    // read their template from disk.
    let mut body = kind.template_with_root(Some(root));

    // Feature B: for retrospectives, best-effort prefill the Metrics section.
    if kind.name == "retrospectives" {
        body = prefill_retro_metrics(body, root);
    }

    if let Err(e) = fs::write(&path, body) {
        eprintln!(
            "bwoc {}: failed to write {}: {e}",
            kind.name,
            path.display()
        );
        return 2;
    }

    println!("{}", path.display());
    0
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

fn cmd_list(kind: DocKind, root: &Path) -> i32 {
    let dir = root.join(kind.dir.trim_end_matches('/'));
    if !dir.exists() {
        println!("No {} documents yet.", kind.name);
        return 0;
    }

    let mut entries = match collect_md_files(&dir) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bwoc {}: could not read {}: {e}", kind.name, dir.display());
            return 2;
        }
    };

    if entries.is_empty() {
        println!("No {} documents yet.", kind.name);
        return 0;
    }

    // Newest first: lexicographic descending (YYYY-MM-DD prefix guarantees this).
    entries.sort_by(|a, b| b.cmp(a));

    for name in &entries {
        println!("{name}");
    }
    0
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn cmd_view(kind: DocKind, name: String, root: &Path) -> i32 {
    let dir = root.join(kind.dir.trim_end_matches('/'));

    // Resolve: try exact filename first, then prefix match.
    let path = resolve_name(&dir, &name);
    match path {
        Some(p) => match fs::read_to_string(&p) {
            Ok(content) => {
                print!("{content}");
                0
            }
            Err(e) => {
                eprintln!("bwoc {}: failed to read {}: {e}", kind.name, p.display());
                2
            }
        },
        None => {
            eprintln!(
                "bwoc {}: no document matching {:?} in {}",
                kind.name,
                name,
                dir.display()
            );
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Feature B — retro metrics prefill
// ---------------------------------------------------------------------------

/// Summarise all `session-metrics.jsonl` files found under `workspace_root`
/// and inject the totals into the `## Metrics` table in the retro template.
///
/// Searches `<root>/metrics/session-metrics.jsonl` and
/// `<root>/agents/*/metrics/session-metrics.jsonl`.
///
/// Never panics / never returns an error — on any failure, the original
/// `body` is returned unchanged.
fn prefill_retro_metrics(body: String, root: &Path) -> String {
    let candidates = collect_metrics_files(root);
    if candidates.is_empty() {
        return body;
    }

    let mut total_attempted: u64 = 0;
    let mut total_completed: u64 = 0;
    let mut total_gates_passed: u64 = 0;
    let mut total_gates_failed: u64 = 0;

    for path in &candidates {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(metrics) = val.get("metrics") {
                    total_attempted += metrics
                        .get("tasksAttempted")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    total_completed += metrics
                        .get("tasksCompleted")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    total_gates_passed += metrics
                        .get("gatesPassed")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    total_gates_failed += metrics
                        .get("gatesFailed")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                }
            }
        }
    }

    // Compute gate pass rate as percentage string.
    let total_gates = total_gates_passed + total_gates_failed;
    let gate_pass_rate = (100 * total_gates_passed)
        .checked_div(total_gates)
        .map_or_else(|| "n/a".to_string(), |pct| format!("{pct}%"));

    // Replace empty-cell placeholder rows in the Metrics table.
    // Pattern: `| <metric-col> | |`  →  `| <metric-col> | <value> |`
    // The `gate_pass_rate` row has an extra space for column alignment.
    let body = replace_metric_cell(body, "tasks_attempted", &total_attempted.to_string());
    let body = replace_metric_cell(body, "tasks_completed", &total_completed.to_string());
    replace_metric_cell(body, "gate_pass_rate ", &gate_pass_rate)
}

/// Replace an empty markdown table cell `| <label> | |` with `| <label> | <value> |`.
///
/// `label` is the exact left-column text as it appears in the template,
/// including any trailing alignment spaces.  The right column is the empty
/// placeholder `|` which becomes `| <value> |`.
fn replace_metric_cell(body: String, label: &str, value: &str) -> String {
    // The template rows look like:
    //   | tasks_attempted | |
    //   | gate_pass_rate  | |   ← extra space for visual alignment
    //
    // We match `| <label> | |` and replace the trailing ` | |` with ` | <value> |`.
    let old = format!("| {label} | |");
    let new = format!("| {label} | {value} |");
    if body.contains(&old) {
        body.replace(&old, &new)
    } else {
        body
    }
}

/// Collect candidate `session-metrics.jsonl` paths under the workspace root.
fn collect_metrics_files(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. <workspace>/metrics/session-metrics.jsonl
    let p = root.join("metrics/session-metrics.jsonl");
    if p.is_file() {
        paths.push(p);
    }

    // 2. <workspace>/agents/*/metrics/session-metrics.jsonl
    let agents_dir = root.join("agents");
    if let Ok(entries) = fs::read_dir(&agents_dir) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("metrics/session-metrics.jsonl");
            if candidate.is_file() {
                paths.push(candidate);
            }
        }
    }

    paths
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return `YYYY-MM-DD` derived from the current UTC timestamp.
fn date_prefix() -> String {
    // utc_now_iso8601() returns "YYYY-MM-DDTHH:MM:SSZ"
    utc_now_iso8601()[..10].to_string()
}

/// Turn a human title into a lowercase-hyphen slug suitable for filenames.
/// Collapses runs of non-alphanumeric characters into single hyphens and
/// strips leading/trailing hyphens.
fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    let mut prev_hyphen = false;
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen && !slug.is_empty() {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    // Strip trailing hyphen that may result from trailing non-alnum chars.
    slug.trim_end_matches('-').to_string()
}

/// Collect all `*.md` filenames (stem + extension, not full paths) from `dir`.
pub(crate) fn collect_md_files(dir: &Path) -> Result<Vec<String>, std::io::Error> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                out.push(name.to_string());
            }
        }
    }
    Ok(out)
}

/// Try to find a file in `dir` matching `name`:
/// 1. Exact filename match (with or without `.md` extension).
/// 2. Any file whose name starts with `name` (date-prefix lookup).
fn resolve_name(dir: &Path, name: &str) -> Option<PathBuf> {
    // Normalise: strip trailing ".md" so both "2026-05-24_foo" and
    // "2026-05-24_foo.md" work as lookup keys.
    let stem = name.strip_suffix(".md").unwrap_or(name);

    let exact_md = dir.join(format!("{stem}.md"));
    if exact_md.exists() {
        return Some(exact_md);
    }

    // Prefix scan — pick the first lexicographic match.
    if let Ok(mut candidates) = collect_md_files(dir) {
        candidates.sort();
        for candidate in candidates {
            let cand_stem = candidate.strip_suffix(".md").unwrap_or(&candidate);
            if cand_stem.starts_with(stem) {
                return Some(dir.join(candidate));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bwoc_core::doc_kind::{kind, load_custom_kinds, resolve_kind};
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    // ---- kind lookup -------------------------------------------------------

    #[test]
    fn kind_lookup_notes() {
        assert!(kind("notes").is_some());
    }

    #[test]
    fn kind_lookup_retrospectives() {
        assert!(kind("retrospectives").is_some());
    }

    #[test]
    fn kind_lookup_research() {
        assert!(kind("research").is_some());
    }

    #[test]
    fn kind_lookup_unknown() {
        assert!(kind("unknown").is_none());
    }

    // ---- new ---------------------------------------------------------------

    #[test]
    fn new_creates_file_with_template() {
        let dir = tmp();
        let k = kind("notes").unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "my first note".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);

        let notes_dir = dir.path().join("notes");
        let files: Vec<_> = fs::read_dir(&notes_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1, "expected exactly one file");

        let content = fs::read_to_string(files[0].path()).unwrap();
        // Template has section headings
        assert!(content.contains("##"));
    }

    #[test]
    fn new_path_contains_slug() {
        let dir = tmp();
        let k = kind("notes").unwrap();
        run(
            k,
            DocAction::New {
                title: "Workspace Design".into(),
            },
            dir.path(),
        );

        let notes_dir = dir.path().join("notes");
        let names: Vec<_> = fs::read_dir(&notes_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(
            names[0].contains("workspace-design"),
            "slug not found in {:?}",
            names[0]
        );
    }

    #[test]
    fn new_refuses_clobber() {
        let dir = tmp();
        let k = kind("notes").unwrap();

        // Create file once.
        let code1 = run(
            k.clone(),
            DocAction::New {
                title: "same title".into(),
            },
            dir.path(),
        );
        assert_eq!(code1, 0);

        // Attempt to create again with the same date → same filename → should fail.
        let code2 = run(
            k,
            DocAction::New {
                title: "same title".into(),
            },
            dir.path(),
        );
        assert_eq!(code2, 1, "expected clobber protection to return exit 1");
    }

    #[test]
    fn new_retro_creates_correct_dir() {
        let dir = tmp();
        let k = kind("retrospectives").unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "sprint 1".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);
        assert!(dir.path().join("retrospectives").exists());
    }

    #[test]
    fn new_research_creates_correct_dir() {
        let dir = tmp();
        let k = kind("research").unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "llm caching".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);
        assert!(dir.path().join("research").exists());
    }

    // ---- custom kinds via bwoc doc engine -----------------------------------

    #[test]
    fn custom_kind_new_creates_right_dir_and_template() {
        let dir = tmp();
        let bwoc_dir = dir.path().join(".bwoc");
        fs::create_dir_all(&bwoc_dir).unwrap();
        // Use basic template without markdown ## to avoid Rust 2024 ## token issues.
        fs::write(
            bwoc_dir.join("doc-kinds.toml"),
            "[[kind]]\nname = \"decision\"\ndir = \"decisions\"\ntemplate = \"Decision Context section\"\n",
        )
        .unwrap();

        let custom = load_custom_kinds(dir.path());
        assert_eq!(custom.len(), 1);

        let k = custom.into_iter().next().unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "use postgres".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);
        assert!(dir.path().join("decisions").exists());

        let files: Vec<_> = fs::read_dir(dir.path().join("decisions"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);
        let content = fs::read_to_string(files[0].path()).unwrap();
        assert!(
            content.contains("Context"),
            "expected 'Context' in:\n{content}"
        );
    }

    #[test]
    fn resolve_kind_unknown_error_message() {
        let dir = tmp();
        let err = resolve_kind("doesnotexist", dir.path()).unwrap_err();
        assert!(err.contains("doesnotexist"));
        assert!(err.contains("notes"));
    }

    // ---- list --------------------------------------------------------------

    #[test]
    fn list_empty_dir_is_friendly() {
        let dir = tmp();
        let k = kind("notes").unwrap();
        let code = run(k, DocAction::List, dir.path());
        assert_eq!(code, 0);
    }

    #[test]
    fn list_returns_newest_first() {
        let dir = tmp();
        let notes_dir = dir.path().join("notes");
        fs::create_dir_all(&notes_dir).unwrap();

        for name in [
            "2026-01-01_alpha.md",
            "2026-03-15_gamma.md",
            "2026-02-10_beta.md",
        ] {
            fs::write(notes_dir.join(name), "# content").unwrap();
        }

        let mut files = collect_md_files(&notes_dir).unwrap();
        files.sort_by(|a, b| b.cmp(a));
        assert_eq!(files[0], "2026-03-15_gamma.md");
        assert_eq!(files[1], "2026-02-10_beta.md");
        assert_eq!(files[2], "2026-01-01_alpha.md");
    }

    // ---- view --------------------------------------------------------------

    #[test]
    fn view_not_found_returns_1() {
        let dir = tmp();
        let k = kind("notes").unwrap();
        let code = run(
            k,
            DocAction::View {
                name: "nonexistent".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 1);
    }

    #[test]
    fn view_finds_by_prefix() {
        let dir = tmp();
        let notes_dir = dir.path().join("notes");
        fs::create_dir_all(&notes_dir).unwrap();
        let filename = "2026-05-24_doc-kinds.md";
        fs::write(notes_dir.join(filename), "# hello").unwrap();

        let k = kind("notes").unwrap();
        let code = run(
            k,
            DocAction::View {
                name: "2026-05-24".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn view_finds_by_exact_stem() {
        let dir = tmp();
        let notes_dir = dir.path().join("notes");
        fs::create_dir_all(&notes_dir).unwrap();
        fs::write(notes_dir.join("2026-05-24_exact.md"), "# exact").unwrap();

        let k = kind("notes").unwrap();
        // With .md extension
        let code = run(
            k.clone(),
            DocAction::View {
                name: "2026-05-24_exact.md".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);
        // Without extension
        let code2 = run(
            k,
            DocAction::View {
                name: "2026-05-24_exact".into(),
            },
            dir.path(),
        );
        assert_eq!(code2, 0);
    }

    // ---- slugify -----------------------------------------------------------

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("My First Note!"), "my-first-note");
        assert_eq!(slugify("  spaces  "), "spaces");
        assert_eq!(slugify("A--B"), "a-b");
    }

    // ---- retro metrics prefill (Feature B) ---------------------------------

    #[test]
    fn retro_prefill_with_metrics_file() {
        let dir = tmp();
        let metrics_dir = dir.path().join("metrics");
        fs::create_dir_all(&metrics_dir).unwrap();

        // Two session records.
        let jsonl = concat!(
            r#"{"sessionId":"s1","agentId":"agent-oracle","startedAt":"2026-05-24T10:00:00Z","endedAt":"2026-05-24T11:00:00Z","metrics":{"tasksAttempted":3,"tasksCompleted":2,"tasksFailed":1,"gatesPassed":8,"gatesFailed":2,"revisionCycles":1,"memoriesCreated":0,"memoriesUpdated":0,"memoriesRemoved":0},"discoveries":[]}"#,
            "\n",
            r#"{"sessionId":"s2","agentId":"agent-oracle","startedAt":"2026-05-24T12:00:00Z","endedAt":"2026-05-24T13:00:00Z","metrics":{"tasksAttempted":2,"tasksCompleted":2,"tasksFailed":0,"gatesPassed":6,"gatesFailed":0,"revisionCycles":0,"memoriesCreated":1,"memoriesUpdated":0,"memoriesRemoved":0},"discoveries":[]}"#,
            "\n",
        );
        fs::write(metrics_dir.join("session-metrics.jsonl"), jsonl).unwrap();

        let k = kind("retrospectives").unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "sprint 1".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);

        let retro_dir = dir.path().join("retrospectives");
        let files: Vec<_> = fs::read_dir(&retro_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let content = fs::read_to_string(files[0].path()).unwrap();

        // tasks_attempted = 3+2 = 5
        assert!(
            content.contains("| tasks_attempted | 5 |"),
            "expected tasks_attempted=5 in:\n{content}"
        );
        // tasks_completed = 2+2 = 4
        assert!(
            content.contains("| tasks_completed | 4 |"),
            "expected tasks_completed=4 in:\n{content}"
        );
        // gate_pass_rate = (8+6)/(8+2+6+0) = 14/16 = 87%
        assert!(
            content.contains("| gate_pass_rate  | 87% |"),
            "expected gate_pass_rate=87% in:\n{content}"
        );
    }

    #[test]
    fn retro_prefill_absent_metrics_leaves_placeholder() {
        let dir = tmp();
        // No metrics/ dir exists.
        let k = kind("retrospectives").unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "sprint no metrics".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);

        let retro_dir = dir.path().join("retrospectives");
        let files: Vec<_> = fs::read_dir(&retro_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let content = fs::read_to_string(files[0].path()).unwrap();

        // Placeholder rows should remain unchanged.
        assert!(
            content.contains("| tasks_attempted | |"),
            "placeholder should be unchanged:\n{content}"
        );
        assert!(
            content.contains("| gate_pass_rate  | |"),
            "placeholder should be unchanged:\n{content}"
        );
    }

    #[test]
    fn retro_prefill_agents_subdir_metrics() {
        let dir = tmp();
        // Put metrics under agents/agent-oracle/metrics/ — the secondary search path.
        let metrics_dir = dir.path().join("agents/agent-oracle/metrics");
        fs::create_dir_all(&metrics_dir).unwrap();

        let jsonl = r#"{"sessionId":"s1","agentId":"agent-oracle","startedAt":"2026-05-24T10:00:00Z","endedAt":"2026-05-24T11:00:00Z","metrics":{"tasksAttempted":1,"tasksCompleted":1,"tasksFailed":0,"gatesPassed":4,"gatesFailed":0,"revisionCycles":0,"memoriesCreated":0,"memoriesUpdated":0,"memoriesRemoved":0},"discoveries":[]}"#;
        fs::write(metrics_dir.join("session-metrics.jsonl"), jsonl).unwrap();

        let k = kind("retrospectives").unwrap();
        run(
            k,
            DocAction::New {
                title: "agent subdir".into(),
            },
            dir.path(),
        );

        let retro_dir = dir.path().join("retrospectives");
        let files: Vec<_> = fs::read_dir(&retro_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let content = fs::read_to_string(files[0].path()).unwrap();

        assert!(
            content.contains("| tasks_attempted | 1 |"),
            "expected prefill from agents/ subdir in:\n{content}"
        );
    }

    #[test]
    fn retro_prefill_malformed_jsonl_ignored() {
        let dir = tmp();
        let metrics_dir = dir.path().join("metrics");
        fs::create_dir_all(&metrics_dir).unwrap();
        // Mix of valid + garbage lines.
        let jsonl = "not json at all\n{\"sessionId\":\"s1\",\"agentId\":\"a\",\"startedAt\":\"2026\",\"endedAt\":\"2026\",\"metrics\":{\"tasksAttempted\":2,\"tasksCompleted\":1,\"tasksFailed\":1,\"gatesPassed\":3,\"gatesFailed\":1,\"revisionCycles\":0,\"memoriesCreated\":0,\"memoriesUpdated\":0,\"memoriesRemoved\":0},\"discoveries\":[]}\n";
        fs::write(metrics_dir.join("session-metrics.jsonl"), jsonl).unwrap();

        let k = kind("retrospectives").unwrap();
        let code = run(
            k,
            DocAction::New {
                title: "mixed jsonl".into(),
            },
            dir.path(),
        );
        assert_eq!(code, 0);

        let retro_dir = dir.path().join("retrospectives");
        let files: Vec<_> = fs::read_dir(&retro_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let content = fs::read_to_string(files[0].path()).unwrap();
        // Valid line is parsed; tasks_attempted = 2.
        assert!(
            content.contains("| tasks_attempted | 2 |"),
            "expected valid line to be parsed in:\n{content}"
        );
    }
}
