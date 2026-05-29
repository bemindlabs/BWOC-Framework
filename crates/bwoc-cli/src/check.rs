//! `bwoc check` — backend-neutrality audit.
//!
//! Rust port of `modules/agent-template/scripts/check-agent-neutrality.sh`
//! with feature parity. Pure-data audit + separate printer for testability.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::i18n;

/// Result of a single audit run. Each finding is a human-readable line.
pub struct AuditReport {
    pub target: String,
    pub passes: Vec<String>,
    pub warnings: Vec<String>,
    pub violations: Vec<String>,
}

const HARDCODED_MODELS: &[&str] = &[
    "claude-opus",
    "claude-sonnet",
    "claude-haiku",
    "claude-3",
    "claude-4",
    "gemini-3",
    "gemini-2",
    "gemini-1",
    "gemini-pro",
    "gpt-4",
    "gpt-3",
    "gpt-oss",
    "o3-",
    "o4-",
    "codex-",
    "kimi-k2",
];

const HARDCODED_TOOLS: &[&str] = &["mempalace", "chromadb", "pinecone", "pgvector", "weaviate"];

const BACKEND_PHRASES: &[&str] = &[
    "Claude will",
    "Claude can",
    "Antigravity will",
    "Antigravity can",
    "Codex will",
    "Kimi will",
];

const REQUIRED_PLACEHOLDERS: &[&str] = &[
    "{{agentId}}",
    "{{memoryPath}}",
    "{{taskId}}",
    "{{deepMemoryCmd}}",
];

/// Placeholders that legitimately stay literal in AGENTS.md *after*
/// incarnation. Per `interconnect/trust.md`-adjacent §Appendix A of
/// AGENTS.md, `{{taskId}}` is resolved by the agent at task-assignment
/// time, not at incarnation — so finding it in an incarnated doc is
/// expected, not a violation. All other placeholders must be substituted.
const RUNTIME_PLACEHOLDERS: &[&str] = &["{{taskId}}"];

/// Which mode the audit runs in. Template mode asserts placeholders
/// *exist* (the template must remain parseable as a scaffold). Incarnation
/// mode asserts placeholders are *gone* (the agent has been personalized).
/// Detection key: `manifest.name`. `{{name}}` ≡ template, anything else
/// ≡ incarnation. Missing manifest defaults to template (safer — won't
/// false-positive a half-built agent into "incarnated and broken").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditMode {
    Template,
    Incarnation,
}

/// Decide audit mode from a parsed `config.manifest.json` value.
fn detect_mode(manifest: Option<&serde_json::Value>) -> AuditMode {
    let name = manifest
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    // Template signals: literal `{{name}}` placeholder OR missing/empty
    // manifest (incarnation-in-progress reads as template to avoid
    // false-positives on half-built agents).
    let looks_like_placeholder = name.starts_with("{{") && name.ends_with("}}");
    if looks_like_placeholder || name.is_empty() {
        AuditMode::Template
    } else {
        AuditMode::Incarnation
    }
}

/// Extract every `{{identifier}}` placeholder found in `content`. Identifier
/// = ASCII alphanumeric + underscore (matches the AGENTS.md spec, which
/// uses camelCase keys). Duplicates collapse — each placeholder reported
/// at most once. Used by the incarnation-mode check and by `new.rs` tests.
pub(crate) fn extract_placeholders(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = content;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            break;
        };
        let key = &after_open[..close];
        if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            let ph = format!("{{{{{key}}}}}");
            if !out.contains(&ph) {
                out.push(ph);
            }
        }
        rest = &after_open[close + 2..];
    }
    out
}

/// Run all neutrality checks against `target` and return the structured report.
pub fn audit(target: &Path) -> AuditReport {
    let mut report = AuditReport {
        target: target.display().to_string(),
        passes: Vec::new(),
        warnings: Vec::new(),
        violations: Vec::new(),
    };

    let agents_md = target.join("AGENTS.md");

    // 1. AGENTS.md exists and is a regular file
    if agents_md.is_file() {
        report.passes.push("AGENTS.md exists".to_string());
    } else {
        report
            .violations
            .push("AGENTS.md not found — this is the single source of truth".to_string());
    }

    // 2. Backend symlinks (AGY, CODEX, KIMI, OLLAMA must symlink to AGENTS.md)
    for backend in &["AGY.md", "CODEX.md", "KIMI.md", "OLLAMA.md"] {
        let p = target.join(backend);
        check_symlink_to_agents(&p, backend, &mut report);
    }

    // 3. CLAUDE.md — can be a symlink or a standalone guidance file
    let claude_md = target.join("CLAUDE.md");
    if claude_md.is_symlink() {
        match fs::read_link(&claude_md) {
            Ok(t) if t == Path::new("AGENTS.md") => {
                report.passes.push("CLAUDE.md -> AGENTS.md".to_string());
            }
            Ok(t) => report.violations.push(format!(
                "CLAUDE.md points to '{}' instead of AGENTS.md",
                t.display()
            )),
            Err(_) => report
                .warnings
                .push("CLAUDE.md unreadable symlink".to_string()),
        }
    } else if claude_md.is_file() {
        report
            .passes
            .push("CLAUDE.md exists (standalone guidance file)".to_string());
    } else {
        report.warnings.push("CLAUDE.md missing".to_string());
    }

    // 4. config.manifest.json — exists and is valid JSON
    let manifest = target.join("config.manifest.json");
    let mut manifest_value: Option<serde_json::Value> = None;
    if manifest.is_file() {
        match fs::read_to_string(&manifest) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(v) => {
                    report
                        .passes
                        .push("config.manifest.json is valid JSON".to_string());
                    manifest_value = Some(v);
                }
                Err(_) => report
                    .violations
                    .push("config.manifest.json is not valid JSON".to_string()),
            },
            Err(_) => report
                .violations
                .push("config.manifest.json unreadable".to_string()),
        }
    } else {
        report
            .warnings
            .push("config.manifest.json missing (recommended for cloning readiness)".to_string());
    }
    let mode = detect_mode(manifest_value.as_ref());

    // Content-based checks on AGENTS.md
    if let Ok(content) = fs::read_to_string(&agents_md) {
        // 5. Placeholders — template asserts existence; incarnation
        //    asserts substitution.
        match mode {
            AuditMode::Template => {
                for ph in REQUIRED_PLACEHOLDERS {
                    if content.contains(ph) {
                        report.passes.push(format!("AGENTS.md contains {ph}"));
                    } else {
                        report
                            .warnings
                            .push(format!("AGENTS.md missing recommended placeholder {ph}"));
                    }
                }
            }
            AuditMode::Incarnation => {
                let found = extract_placeholders(&content);
                let unsubstituted: Vec<&String> = found
                    .iter()
                    .filter(|ph| !RUNTIME_PLACEHOLDERS.contains(&ph.as_str()))
                    .collect();
                if unsubstituted.is_empty() {
                    report
                        .passes
                        .push("AGENTS.md has no unsubstituted placeholders".to_string());
                } else {
                    for ph in unsubstituted {
                        report.violations.push(format!(
                            "AGENTS.md has unsubstituted placeholder {ph} \
                             — agent is incarnated but not personalized"
                        ));
                    }
                }
            }
        }

        // 6. No YAML frontmatter
        if content.starts_with("---\n") || content.starts_with("---\r\n") || content == "---" {
            report.violations.push(
                "AGENTS.md has YAML frontmatter — instruction files must use plain Markdown"
                    .to_string(),
            );
        } else {
            report
                .passes
                .push("AGENTS.md has no YAML frontmatter".to_string());
        }

        // 7. No wikilinks (Obsidian [[...]] syntax)
        if contains_wikilink(&content) {
            report.violations.push(
                "AGENTS.md contains wikilinks — instruction files must use plain Markdown"
                    .to_string(),
            );
        } else {
            report.passes.push("AGENTS.md has no wikilinks".to_string());
        }

        // Checks 8-10 are template-only. An incarnated agent has committed
        // to a model + tools + backend voice — the neutrality rules guard
        // the SCAFFOLD, not the per-agent instance. Running them in
        // incarnation mode would false-positive every legitimately
        // personalized agent (`primaryModel = claude-opus-4-7` after
        // substitution would match HARDCODED_MODELS).
        if matches!(mode, AuditMode::Template) {
            // 8. No hardcoded model IDs (case-insensitive substring match)
            let lower = content.to_lowercase();
            let mut model_ok = true;
            for model in HARDCODED_MODELS {
                if lower.contains(model) {
                    report
                        .violations
                        .push(format!("AGENTS.md contains hardcoded model ID '{model}'"));
                    model_ok = false;
                }
            }
            if model_ok {
                report
                    .passes
                    .push("No hardcoded model IDs in AGENTS.md".to_string());
            }

            // 9. No hardcoded tool names
            let mut tool_ok = true;
            for tool in HARDCODED_TOOLS {
                if lower.contains(tool) {
                    report
                        .violations
                        .push(format!("AGENTS.md contains hardcoded tool name '{tool}'"));
                    tool_ok = false;
                }
            }
            if tool_ok {
                report
                    .passes
                    .push("No hardcoded tool names in AGENTS.md".to_string());
            }

            // 10. No backend-specific phrasing
            let mut lang_ok = true;
            for phrase in BACKEND_PHRASES {
                if content.contains(phrase) {
                    report.violations.push(format!(
                        "AGENTS.md contains backend-specific phrase '{phrase}'"
                    ));
                    lang_ok = false;
                }
            }
            if lang_ok {
                report
                    .passes
                    .push("No backend-specific language in AGENTS.md".to_string());
            }
        }
    }

    // 11. Trust evidence — Kalyāṇamitta 7. Each `true` declaration in
    //     config.manifest.json's `trust.declared` block must have the
    //     evidence documented in `interconnect/trust.md`. A claim without
    //     evidence is a violation. Skipped silently if no trust block.
    check_trust_evidence(target, &mut report);

    report
}

/// Verify the Kalyāṇamitta 7 evidence rules from
/// `modules/agent-template/interconnect/trust.md`. For each quality
/// the agent's manifest declares `true`, this checks the corresponding
/// structural evidence. `false` declarations are always valid (no
/// evidence needed). A missing `trust` block skips the check entirely.
fn check_trust_evidence(target: &Path, report: &mut AuditReport) {
    use bwoc_core::manifest::Manifest;
    let manifest_path = target.join("config.manifest.json");
    let Ok(m) = Manifest::load_from_path(&manifest_path) else {
        return; // no manifest or unparseable — handled by earlier check
    };
    let Some(trust) = m.trust.as_ref() else {
        return; // no trust block — nothing to verify
    };
    let d = &trust.declared;

    if d.piyo {
        check_piyo(target, report);
    }
    if d.garu {
        check_garu(target, report);
    }
    if d.bhavaniyo {
        check_bhavaniyo(target, report);
    }
    if d.vatta {
        check_vatta(target, report);
    }
    if d.vacanakkhamo {
        check_vacanakkhamo(target, report);
    }
    if d.gambhira {
        check_gambhira(target, report);
    }
    if d.no_catthana {
        check_no_catthana(target, report);
    }
}

/// Extract a section body from a Markdown doc. Looks for `## <heading>`
/// (case-insensitive), returns the lines between it and the next
/// same-level heading. Returns None if heading isn't found.
fn extract_section(content: &str, heading: &str) -> Option<String> {
    let lower_heading = heading.to_lowercase();
    let mut collecting = false;
    let mut body = String::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            if collecting {
                // Reached the next ## — stop.
                break;
            }
            if rest.trim().to_lowercase() == lower_heading {
                collecting = true;
                continue;
            }
        } else if collecting {
            body.push_str(line);
            body.push('\n');
        }
    }
    if collecting { Some(body) } else { None }
}

/// Section body is "filled in" if it contains at least one non-empty,
/// non-placeholder line. A line with only `{{placeholder}}` doesn't
/// count — that's an un-resolved scaffold, not content.
fn section_is_filled(body: &str) -> bool {
    body.lines().any(|l| {
        let t = l.trim();
        // Skip empty, blockquote, raw placeholder, and template labels —
        // anything else counts as filled content.
        !(t.is_empty()
            || t.starts_with('>')
            || (t.starts_with("{{") && t.ends_with("}}"))
            || t.starts_with("**Does:**")
            || t.starts_with("**Does not:**"))
    })
}

/// Piyo — persona/README.md "Scope" section filled with concrete content.
fn check_piyo(target: &Path, report: &mut AuditReport) {
    let p = target.join("persona/README.md");
    let Ok(content) = fs::read_to_string(&p) else {
        report.violations.push(
            "trust.piyo=true but persona/README.md is missing — scope cannot be declared".into(),
        );
        return;
    };
    match extract_section(&content, "Scope") {
        Some(body) if section_is_filled(&body) => report
            .passes
            .push("trust.piyo: Scope section filled".into()),
        Some(_) => report.violations.push(
            "trust.piyo=true but persona/README.md Scope section is empty / unresolved placeholder"
                .into(),
        ),
        None => report
            .violations
            .push("trust.piyo=true but persona/README.md has no Scope section".into()),
    }
}

/// Garu — at least one user-authored .md (not README.md) under
/// skills/ OR mindsets/. Respectability needs a demonstrated surface.
fn check_garu(target: &Path, report: &mut AuditReport) {
    let mut count = 0;
    for sub in &["skills", "mindsets"] {
        let dir = target.join(sub);
        if let Ok(read) = fs::read_dir(&dir) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && name != "README.md" {
                    count += 1;
                }
            }
        }
    }
    if count > 0 {
        report
            .passes
            .push(format!("trust.garu: {count} skill/mindset stub(s) present"));
    } else {
        report
            .violations
            .push("trust.garu=true but no .md files under skills/ or mindsets/ (only README.md doesn't count)".into());
    }
}

/// Bhāvanīyo — mindsets/ has a file whose name or content references
/// improvement / verification / yoniso / mattaññutā.
fn check_bhavaniyo(target: &Path, report: &mut AuditReport) {
    let dir = target.join("mindsets");
    let Ok(read) = fs::read_dir(&dir) else {
        report
            .violations
            .push("trust.bhavaniyo=true but mindsets/ directory is missing".into());
        return;
    };
    const KEYWORDS: &[&str] = &[
        "improve",
        "improvement",
        "verify",
        "verification",
        "yoniso",
        "manasikara",
        "manasikāra",
        "mattaññutā",
        "mattanutata",
        "right amount",
    ];
    let mut hit = false;
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !name.ends_with(".md") || name == "readme.md" {
            continue;
        }
        // name match
        if KEYWORDS.iter().any(|k| name.contains(k)) {
            hit = true;
            break;
        }
        // content match
        if let Ok(c) = fs::read_to_string(entry.path()) {
            let lc = c.to_lowercase();
            if KEYWORDS.iter().any(|k| lc.contains(k)) {
                hit = true;
                break;
            }
        }
    }
    if hit {
        report.passes.push(
            "trust.bhavaniyo: mindsets/ references improvement/verify/yoniso/mattaññutā".into(),
        );
    } else {
        report.violations.push(
            "trust.bhavaniyo=true but no mindset references improvement/verify/yoniso/mattaññutā keywords".into(),
        );
    }
}

/// Vattā — persona/README.md "Anti-scope" / "Out-of-scope" section filled.
/// "Speaks beneficial truth" needs an honest declaration of what the
/// agent DOES NOT do.
fn check_vatta(target: &Path, report: &mut AuditReport) {
    let p = target.join("persona/README.md");
    let Ok(content) = fs::read_to_string(&p) else {
        report
            .violations
            .push("trust.vatta=true but persona/README.md is missing".into());
        return;
    };
    // Accept either "Anti-scope" or "Out-of-scope" heading.
    let body = extract_section(&content, "Anti-scope")
        .or_else(|| extract_section(&content, "Out-of-scope"))
        .or_else(|| extract_section(&content, "Scope")); // fallback: Scope section may include "Does not:" line
    match body {
        Some(b) if section_is_filled(&b) || b.to_lowercase().contains("does not:") => report
            .passes
            .push("trust.vatta: anti-scope declared".into()),
        Some(_) => report
            .violations
            .push("trust.vatta=true but anti-scope section is empty".into()),
        None => report
            .violations
            .push("trust.vatta=true but no Anti-scope / Out-of-scope section found".into()),
    }
}

/// Vacanakkhamo — agent has exercised inbox listening, OR has a
/// `interconnect/feedback.md` documenting how it handles feedback.
fn check_vacanakkhamo(target: &Path, report: &mut AuditReport) {
    let inbox = target.join(".bwoc/inbox.jsonl");
    let inbox_used = fs::metadata(&inbox).map(|m| m.len() > 0).unwrap_or(false);
    let feedback_doc = target.join("interconnect/feedback.md").is_file();
    if inbox_used || feedback_doc {
        report
            .passes
            .push("trust.vacanakkhamo: inbox used OR interconnect/feedback.md present".into());
    } else {
        report.violations.push(
            "trust.vacanakkhamo=true but inbox.jsonl is empty AND interconnect/feedback.md is missing".into(),
        );
    }
}

/// Gambhīra — at least one doc under the agent root is ≥50 lines AND
/// contains a `[[PHILOSOPHY.en.md]]` or `[[PHILOSOPHY.th.md]]` wikilink.
/// Pi's review: backlink-to-canon is harder to fake than keyword sniff.
fn check_gambhira(target: &Path, report: &mut AuditReport) {
    let mut found = None;
    visit_md_files(target, 0, &mut |path, content| {
        let line_count = content.lines().count();
        if line_count >= 50
            && (content.contains("[[PHILOSOPHY.en.md]]")
                || content.contains("[[PHILOSOPHY.th.md]]"))
        {
            found = Some(path.display().to_string());
        }
    });
    match found {
        Some(p) => report
            .passes
            .push(format!("trust.gambhira: depth doc anchored to PHILOSOPHY at {p}")),
        None => report.violations.push(
            "trust.gambhira=true but no doc has ≥50 lines AND a [[PHILOSOPHY.en.md]] wikilink — backlink to canon is required (anti-padding rule from Pi review)".into(),
        ),
    }
}

/// No-caṭṭhāne — persona Anti-scope section exists AND contains at
/// least one explicit "will refuse" entry (or similar refusal verb).
fn check_no_catthana(target: &Path, report: &mut AuditReport) {
    let p = target.join("persona/README.md");
    let Ok(content) = fs::read_to_string(&p) else {
        report
            .violations
            .push("trust.noCatthana=true but persona/README.md is missing".into());
        return;
    };
    let body = extract_section(&content, "Anti-scope")
        .or_else(|| extract_section(&content, "Out-of-scope"))
        .or_else(|| extract_section(&content, "Scope"));
    let Some(body) = body else {
        report.violations.push(
            "trust.noCatthana=true but no Anti-scope / Out-of-scope section in persona/README.md"
                .into(),
        );
        return;
    };
    let lc = body.to_lowercase();
    const REFUSAL_VERBS: &[&str] = &[
        "will refuse",
        "refuses",
        "will not",
        "does not",
        "never ",
        "must not",
        "refuse to",
        "decline",
    ];
    if REFUSAL_VERBS.iter().any(|v| lc.contains(v)) {
        report
            .passes
            .push("trust.noCatthana: anti-scope contains explicit refusal entry".into());
    } else {
        report.violations.push(
            "trust.noCatthana=true but anti-scope has no explicit refusal verb (will refuse / does not / never / must not / ...)".into(),
        );
    }
}

/// Walk all .md files under `target` (skipping `node_modules`, `target/`,
/// `.git/`, `.bwoc/`) up to a small depth, calling `visit` per file. Used
/// by `check_gambhira` to find the backlinked-to-canon evidence doc.
fn visit_md_files<F: FnMut(&Path, &str)>(target: &Path, depth: usize, visit: &mut F) {
    if depth > 4 {
        return;
    }
    let Ok(read) = fs::read_dir(target) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(name.as_str(), ".git" | ".bwoc" | "node_modules" | "target") {
                continue;
            }
            visit_md_files(&path, depth + 1, visit);
        } else if name.ends_with(".md") {
            if let Ok(c) = fs::read_to_string(&path) {
                visit(&path, &c);
            }
        }
    }
}

/// Verify the backend entry file (`AGY.md`, `CODEX.md`, `KIMI.md`) is
/// a symlink pointing at `AGENTS.md`. Missing files are warnings, not
/// violations — an agent may not declare every backend. Symlinks
/// pointing elsewhere are violations.
fn check_symlink_to_agents(path: &Path, backend: &str, report: &mut AuditReport) {
    if path.is_symlink() {
        match fs::read_link(path) {
            Ok(t) if t == Path::new("AGENTS.md") => {
                report.passes.push(format!("{backend} -> AGENTS.md"));
            }
            Ok(t) => report.violations.push(format!(
                "{backend} points to '{}' instead of AGENTS.md",
                t.display()
            )),
            Err(_) => report
                .warnings
                .push(format!("{backend} unreadable symlink")),
        }
    } else {
        report.warnings.push(format!(
            "{backend} missing (create with: ln -s AGENTS.md {backend})"
        ));
    }
}

/// Conservative wikilink detector: look for `[[` followed by `]]` on the same
/// or nearby lines. Avoids a regex dep; deliberately matches the shell script's
/// `grep -qE '\[\[.*\]\]'` behavior (anything between `[[` and `]]` on one line).
fn contains_wikilink(content: &str) -> bool {
    for line in content.lines() {
        if let Some(open) = line.find("[[") {
            if line[open + 2..].contains("]]") {
                return true;
            }
        }
    }
    false
}

/// Print the report. Header / labels / summary are localized via Fluent;
/// finding descriptions stay English (rule-specific; deferred per Mattaññutā).
pub fn print_report(
    report: &AuditReport,
    bundle: &fluent_bundle::FluentBundle<fluent_bundle::FluentResource>,
) {
    let pass_label = i18n::t(bundle, "check-label-pass");
    let warn_label = i18n::t(bundle, "check-label-warn");
    let fail_label = i18n::t(bundle, "check-label-fail");
    let warnings_count = report.warnings.len().to_string();
    let violations_count = report.violations.len().to_string();

    println!();
    println!("{}", i18n::t(bundle, "check-header"));
    println!("============================");
    println!(
        "{}",
        i18n::t_with(bundle, "check-target", &[("target", &report.target)])
    );
    println!();
    for p in &report.passes {
        println!("{pass_label}  {p}");
    }
    for w in &report.warnings {
        println!("{warn_label}  {w}");
    }
    for v in &report.violations {
        println!("{fail_label}  {v}");
    }
    println!();
    println!("==============================");
    if !report.violations.is_empty() {
        println!(
            "{}",
            i18n::t_with(
                bundle,
                "check-summary-failure",
                &[
                    ("violations", &violations_count),
                    ("warnings", &warnings_count),
                ],
            )
        );
        println!("{}", i18n::t(bundle, "check-summary-failure-tail"));
    } else {
        println!(
            "{}",
            i18n::t_with(
                bundle,
                "check-summary-success",
                &[("warnings", &warnings_count)],
            )
        );
        println!("{}", i18n::t(bundle, "check-summary-success-tail"));
    }
}

/// Entry point. Returns the process exit code (0 = ok, 1 = violations).
pub fn run(target: &Path, lang: &str, json: bool) -> i32 {
    let report = audit(target);
    if json {
        let value = serde_json::json!({
            "target": report.target,
            "passes": report.passes,
            "warnings": report.warnings,
            "violations": report.violations,
            "summary": {
                "passes": report.passes.len(),
                "warnings": report.warnings.len(),
                "violations": report.violations.len(),
            },
        });
        match serde_json::to_string_pretty(&value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("bwoc check: failed to serialize JSON: {e}");
                return 1;
            }
        }
    } else {
        let bundle = i18n::bundle_for(lang);
        print_report(&report, &bundle);
    }
    if report.violations.is_empty() { 0 } else { 1 }
}

/// Fleet-wide audit. Iterates the workspace's `agents.toml`, runs
/// `audit()` per agent, aggregates findings. Exit 0 only if every
/// agent passes; 1 if any has violations; 2 if the workspace itself
/// can't be located.
pub fn run_all(workspace_path: Option<&Path>, lang: &str, json: bool) -> i32 {
    use bwoc_core::workspace::AgentsRegistry;

    // Resolve workspace root: explicit path > BWOC_WORKSPACE env > ancestor walk.
    let root = match workspace_path {
        Some(p) => p.to_path_buf(),
        None => {
            if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
                if !env_path.is_empty() {
                    std::path::PathBuf::from(env_path)
                } else {
                    let Some(p) = find_workspace_root_local() else {
                        eprintln!(
                            "bwoc check --all: no workspace found. Pass --workspace, set \
                             BWOC_WORKSPACE, or run from a workspace directory."
                        );
                        return 2;
                    };
                    p
                }
            } else {
                let Some(p) = find_workspace_root_local() else {
                    eprintln!(
                        "bwoc check --all: no workspace found. Pass --workspace, set \
                         BWOC_WORKSPACE, or run from a workspace directory."
                    );
                    return 2;
                };
                p
            }
        }
    };
    let registry = match AgentsRegistry::load(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc check --all: failed to read agents.toml: {e}");
            return 1;
        }
    };
    if registry.agents.is_empty() {
        eprintln!(
            "bwoc check --all: no agents registered in {}. \
             Run `bwoc new <name>` to incarnate one.",
            root.display()
        );
        return 0;
    }

    let mut total_violations = 0u32;
    let mut total_warnings = 0u32;
    let mut total_passes = 0u32;
    let mut per_agent_reports: Vec<(String, AuditReport)> = Vec::new();
    for entry in &registry.agents {
        let path = root.join(&entry.path);
        let report = audit(&path);
        total_violations += report.violations.len() as u32;
        total_warnings += report.warnings.len() as u32;
        total_passes += report.passes.len() as u32;
        per_agent_reports.push((entry.id.clone(), report));
    }

    // Skill + plugin manifest audits (BWOC-8) — extend the fleet tally with
    // any installed `modules/skills/<name>/manifest.toml` and
    // `modules/plugins/<name>/manifest.toml`. Spec source of truth:
    // docs/en/SKILLS.en.md and docs/en/PLUGINS.en.md.
    let mut per_skill_reports: Vec<(String, AuditReport)> = Vec::new();
    for dir in discover_skill_dirs(&root) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let report = audit_skill_manifest(&dir);
        total_violations += report.violations.len() as u32;
        total_warnings += report.warnings.len() as u32;
        total_passes += report.passes.len() as u32;
        per_skill_reports.push((name, report));
    }
    let mut per_plugin_reports: Vec<(String, AuditReport)> = Vec::new();
    for dir in discover_plugin_dirs(&root) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let report = audit_plugin_manifest(&dir);
        total_violations += report.violations.len() as u32;
        total_warnings += report.warnings.len() as u32;
        total_passes += report.passes.len() as u32;
        per_plugin_reports.push((name, report));
    }

    if json {
        let agents: Vec<serde_json::Value> = per_agent_reports
            .iter()
            .map(|(id, r)| {
                serde_json::json!({
                    "agent": id,
                    "target": r.target,
                    "passes": r.passes,
                    "warnings": r.warnings,
                    "violations": r.violations,
                    "summary": {
                        "passes": r.passes.len(),
                        "warnings": r.warnings.len(),
                        "violations": r.violations.len(),
                    },
                })
            })
            .collect();
        let skills: Vec<serde_json::Value> = per_skill_reports
            .iter()
            .map(|(name, r)| {
                serde_json::json!({
                    "skill": name,
                    "target": r.target,
                    "passes": r.passes,
                    "warnings": r.warnings,
                    "violations": r.violations,
                    "summary": {
                        "passes": r.passes.len(),
                        "warnings": r.warnings.len(),
                        "violations": r.violations.len(),
                    },
                })
            })
            .collect();
        let plugins: Vec<serde_json::Value> = per_plugin_reports
            .iter()
            .map(|(name, r)| {
                serde_json::json!({
                    "plugin": name,
                    "target": r.target,
                    "passes": r.passes,
                    "warnings": r.warnings,
                    "violations": r.violations,
                    "summary": {
                        "passes": r.passes.len(),
                        "warnings": r.warnings.len(),
                        "violations": r.violations.len(),
                    },
                })
            })
            .collect();
        let value = serde_json::json!({
            "workspace": root.display().to_string(),
            "agents": agents,
            "skills": skills,
            "plugins": plugins,
            "summary": {
                "agents_checked": per_agent_reports.len(),
                "skills_checked": per_skill_reports.len(),
                "plugins_checked": per_plugin_reports.len(),
                "total_passes": total_passes,
                "total_warnings": total_warnings,
                "total_violations": total_violations,
            },
        });
        match serde_json::to_string_pretty(&value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("bwoc check --all: failed to serialize JSON: {e}");
                return 1;
            }
        }
    } else {
        let bundle = i18n::bundle_for(lang);
        for (id, report) in &per_agent_reports {
            println!();
            println!("=== {id} ===");
            print_report(report, &bundle);
        }
        for (name, report) in &per_skill_reports {
            println!();
            println!("=== skill: {name} ===");
            print_report(report, &bundle);
        }
        for (name, report) in &per_plugin_reports {
            println!();
            println!("=== plugin: {name} ===");
            print_report(report, &bundle);
        }
        println!();
        println!(
            "=== Fleet summary ===\n  {} agent(s) + {} skill(s) + {} plugin(s): {} pass, {} warn, {} violation(s)",
            per_agent_reports.len(),
            per_skill_reports.len(),
            per_plugin_reports.len(),
            total_passes,
            total_warnings,
            total_violations,
        );
        println!();
    }

    if total_violations > 0 { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// Skill + plugin manifest audits (BWOC-8).
//
// Each installed skill (`modules/skills/<name>/manifest.toml`) and plugin
// (`modules/plugins/<name>/manifest.toml`) gets its own AuditReport so the
// fleet-wide `bwoc check --all` tally surfaces manifest violations alongside
// agent neutrality findings. Source of truth for these checks:
//   - docs/en/SKILLS.en.md §"Manifest" + §"Verification"
//   - docs/en/PLUGINS.en.md §"Manifest" + §"Verification"
// ---------------------------------------------------------------------------

/// Backend identifiers — the five declared backends from ARCHITECTURE.en.md.
/// A skill manifest naming any of these is backend-specific and belongs as
/// that backend's integration plugin, not as a framework skill (Samānattatā).
/// Whole-word match: substring is too loose (e.g. "claude" would trip on
/// any name containing those letters); too-strict matching is fine here
/// because manifest values are short, kebab-case-or-sentence text.
const BACKEND_NAMES: &[&str] = &["claude", "antigravity", "codex", "kimi", "ollama"];

/// Plugin kinds accepted by the framework. The task brief enumerates
/// `audit (future)` as a forward-compatible value — accept it now so
/// the EPIC-2 ISO compliance plugins land without a v2 audit bump.
/// `jira` (BWOC-43) is the first write-capable integration kind; it is
/// already declared in the PLUGINS.en.md enum (BWOC-41), so the validator
/// recognizes it here too — otherwise the reference jira plugin would fail
/// its own `bwoc check`. `okr` (BWOC-47) is the third reporting kind,
/// declared in the PLUGINS.en.md enum; recognized here so the reference
/// `okr/workspace-okrs` plugin (BWOC-49) passes basic well-formedness. The
/// okr-specific manifest + Progress Schema validation lands in BWOC-50.
/// `council` (BWOC-57) is the first coordination kind, declared in the
/// PLUGINS.en.md enum; recognized here so the reference
/// `council/council-sangha-7` plugin (BWOC-59) passes basic well-formedness.
/// The council-specific manifest (voting_model / quorum) + Decision Schema
/// validation lands in BWOC-60 (`audit_council`).
/// `figma` (BWOC-62) is the eighth kind — a read-mostly design→dev
/// integration, declared in the PLUGINS.en.md enum; recognized here so the
/// reference `figma/figma-rest` plugin (BWOC-64) passes basic well-formedness.
/// The figma `auth.toml` secret-leak guard + Asset Mapping Schema validation
/// land in BWOC-65 (`audit_figma_auth` / `audit_figma_assets`).
/// `gws` (BWOC-73) is the ninth kind — a read-mostly Google Workspace
/// integration, declared in the PLUGINS.en.md enum; recognized here so the
/// reference `gws/gws-auth` + `gws/gws-drive` plugins (BWOC-75) pass basic
/// well-formedness. The gws `auth.toml` secret-leak guard + Workspace Resource
/// Schema validation land in BWOC-77 (`audit_gws_auth`).
const PLUGIN_KINDS: &[&str] = &[
    "memory-backend",
    "llm-backend",
    "workflow",
    "audit",
    "jira",
    "okr",
    "council",
    "figma",
    "gws",
];

/// Closed severity enum for declared criteria. Source of truth:
/// PLUGINS.en.md §"Audit Findings Schema" — five-level scale matching
/// the published ISO/NIST risk vocabulary. Diverges from the looser
/// `{error, warn, info}` triplet some operator-facing tooling uses;
/// `bwoc check` honors the spec, not the surface vocabulary.
const AUDIT_SEVERITY_LEVELS: &[&str] = &["info", "low", "medium", "high", "critical"];

/// Closed evidence-kind enum for declared criteria. Source of truth:
/// PLUGINS.en.md §"Evidence kinds" (extended by BWOC-27 with
/// `attestation` and `sample`). A criterion's optional
/// `expected_evidence_kind` field declares which kind the runtime intends
/// to emit; BWOC-29 enforces the kind name is valid and (for kinds that
/// carry spec-mandated sub-fields) the per-kind contract is declared.
const EVIDENCE_KINDS: &[&str] = &[
    "file",
    "content",
    "command",
    "attestation",
    "sample",
    "none",
];

/// Sub-field names valid under `[criterion.<id>.attestation].required`.
/// `signer` and `signed_at` are the spec floor (always required by
/// PLUGINS.en.md when kind = "attestation"); `valid_through` and `as_of`
/// are time-bounded fields any criterion may elevate to required.
const ATTESTATION_FIELDS: &[&str] = &["signer", "signed_at", "valid_through", "as_of"];
const ATTESTATION_FLOOR: &[&str] = &["signer", "signed_at"];

/// Sub-field names valid under `[criterion.<id>.sample].required`.
/// `sampled_count` and `sampled_of` are the spec floor; `window`,
/// `valid_through`, and `as_of` may be elevated per criterion.
const SAMPLE_FIELDS: &[&str] = &[
    "sampled_count",
    "sampled_of",
    "window",
    "valid_through",
    "as_of",
];
const SAMPLE_FLOOR: &[&str] = &["sampled_count", "sampled_of"];

/// Closed `confidence` enum for an OKR key result. Source of truth:
/// PLUGINS.en.md §"OKR Progress Schema" — a qualitative trajectory read,
/// deliberately an enum rather than a numeric score (BWOC-46 §5): attainment
/// carries the quantitative signal, `confidence` the qualitative one.
const OKR_CONFIDENCE_LEVELS: &[&str] = &["high", "medium", "low"];

/// Closed `unit` enum for an OKR key result. Source of truth:
/// PLUGINS.en.md §"OKR Progress Schema" — how `target` / `current` are read
/// (`boolean` uses `0`/`1`).
const OKR_UNITS: &[&str] = &["count", "percent", "currency", "ratio", "boolean"];

/// Maturity values accepted in a skill manifest (Ariya-dhana 7 scale).
const MATURITY_LEVELS: &[&str] = &["L1", "L2", "L3", "L4", "L5", "L6", "L7"];

/// Closed `status` enum for a Council Decision entry. Source of truth:
/// PLUGINS.en.md §"Council Decision Schema" — the protocol states
/// `proposed → discussing → voting → resolved` (or `abandoned` if quorum fails).
/// Mirrors the runtime `STATUS_*` constants in `council.rs`.
const COUNCIL_STATUSES: &[&str] = &["proposed", "discussing", "voting", "resolved", "abandoned"];

/// Audit one skill installed at `<workspace>/modules/skills/<name>/`. Required
/// fields, types, neutrality, and the spec's non-empty-`exposes` rule are all
/// checked. Returns a report keyed by the manifest path so fleet output
/// disambiguates skills from agents and plugins.
pub fn audit_skill_manifest(skill_dir: &Path) -> AuditReport {
    let manifest_path = skill_dir.join("manifest.toml");
    let mut report = AuditReport {
        target: manifest_path.display().to_string(),
        passes: Vec::new(),
        warnings: Vec::new(),
        violations: Vec::new(),
    };

    let body = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            report
                .violations
                .push(format!("manifest.toml unreadable: {e}"));
            return report;
        }
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report
                .passes
                .push("manifest.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("manifest.toml is not valid TOML: {e}"));
            return report;
        }
    };

    let skill_table = match raw.get("skill").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            report
                .violations
                .push("[skill] table missing — required per SKILLS.en.md".to_string());
            return report;
        }
    };
    report.passes.push("[skill] table present".to_string());

    // Required string fields under [skill].
    for field in &["name", "version", "description", "maturity"] {
        match skill_table.get(*field) {
            Some(v) if v.is_str() => report
                .passes
                .push(format!("[skill].{field} present (string)")),
            Some(_) => report
                .violations
                .push(format!("[skill].{field} has wrong type — expected string")),
            None => report
                .violations
                .push(format!("[skill].{field} missing — required field")),
        }
    }

    // Name matches directory basename.
    let dir_name = skill_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(name) = skill_table.get("name").and_then(|v| v.as_str()) {
        if name == dir_name {
            report
                .passes
                .push(format!("[skill].name matches directory '{dir_name}'"));
        } else {
            report.violations.push(format!(
                "[skill].name '{name}' does not match directory '{dir_name}'"
            ));
        }
    }

    // Maturity in L1..L7.
    if let Some(m) = skill_table.get("maturity").and_then(|v| v.as_str()) {
        if MATURITY_LEVELS.contains(&m) {
            report
                .passes
                .push(format!("[skill].maturity '{m}' in L1..L7"));
        } else {
            report
                .violations
                .push(format!("[skill].maturity '{m}' not in L1..L7"));
        }
    }

    // [contract].exposes — required, array of strings, non-empty.
    let contract = raw.get("contract").and_then(|v| v.as_table());
    match contract.and_then(|c| c.get("exposes")) {
        Some(toml::Value::Array(arr)) => {
            if arr.is_empty() {
                report.violations.push(
                    "[contract].exposes is empty — must be non-empty per spec (a skill that exposes nothing should not exist)"
                        .to_string(),
                );
            } else if !arr.iter().all(|v| v.is_str()) {
                report
                    .violations
                    .push("[contract].exposes contains non-string entries".to_string());
            } else {
                report.passes.push(format!(
                    "[contract].exposes is non-empty ({} operation(s))",
                    arr.len()
                ));
            }
        }
        Some(_) => report
            .violations
            .push("[contract].exposes has wrong type — expected array of strings".to_string()),
        None => report
            .violations
            .push("[contract].exposes missing — required field".to_string()),
    }

    // [contract].requires (optional) — when present must be array of strings.
    if let Some(req) = contract.and_then(|c| c.get("requires")) {
        match req {
            toml::Value::Array(arr) if arr.iter().all(|v| v.is_str()) => {
                report.passes.push(format!(
                    "[contract].requires is array of strings (length {})",
                    arr.len()
                ));
            }
            _ => report
                .violations
                .push("[contract].requires has wrong type — expected array of strings".to_string()),
        }
    }

    // [contract].requires_plugins (optional, BWOC-44/45) — plugin KINDS this
    // skill needs enabled. STATIC check only, per SKILLS.en.md §Verification
    // (line 379): every value must be a valid plugin-kind enum. Whether a
    // matching plugin is actually enabled is a spawn-time / `bwoc skill verify`
    // concern (see skill::run_verify), NOT a manifest-shape concern — kind
    // validity is the manifest's job, enablement is the workspace's.
    if let Some(req) = contract.and_then(|c| c.get("requires_plugins")) {
        match req {
            toml::Value::Array(arr) if arr.iter().all(|v| v.is_str()) => {
                for kind in arr.iter().filter_map(|v| v.as_str()) {
                    if PLUGIN_KINDS.contains(&kind) {
                        report.passes.push(format!(
                            "[contract].requires_plugins '{kind}' is a valid plugin kind"
                        ));
                    } else {
                        report.violations.push(format!(
                            "[contract].requires_plugins '{kind}' is not a valid plugin kind \
                             (expected one of {{memory-backend, llm-backend, workflow, audit, jira, okr, council, figma}})"
                        ));
                    }
                }
            }
            _ => report.violations.push(
                "[contract].requires_plugins has wrong type — expected array of strings"
                    .to_string(),
            ),
        }
    }

    // Neutrality — no backend / model names anywhere in manifest values.
    check_manifest_neutrality_skill(&raw, &mut report);

    report
}

/// Audit one plugin installed at `<workspace>/modules/plugins/<name>/`.
/// Same shape as the skill audit, with plugin-specific rules: kind ∈ the
/// declared enum, neutrality lets vendor names appear in `description` only.
pub fn audit_plugin_manifest(plugin_dir: &Path) -> AuditReport {
    let manifest_path = plugin_dir.join("manifest.toml");
    let mut report = AuditReport {
        target: manifest_path.display().to_string(),
        passes: Vec::new(),
        warnings: Vec::new(),
        violations: Vec::new(),
    };

    let body = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            report
                .violations
                .push(format!("manifest.toml unreadable: {e}"));
            return report;
        }
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report
                .passes
                .push("manifest.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("manifest.toml is not valid TOML: {e}"));
            return report;
        }
    };

    let plugin_table = match raw.get("plugin").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            report
                .violations
                .push("[plugin] table missing — required per PLUGINS.en.md".to_string());
            return report;
        }
    };
    report.passes.push("[plugin] table present".to_string());

    // Required string fields under [plugin].
    for field in &["name", "kind", "version", "description", "compat", "entry"] {
        match plugin_table.get(*field) {
            Some(v) if v.is_str() => report
                .passes
                .push(format!("[plugin].{field} present (string)")),
            Some(_) => report
                .violations
                .push(format!("[plugin].{field} has wrong type — expected string")),
            None => report
                .violations
                .push(format!("[plugin].{field} missing — required field")),
        }
    }

    // BWOC-36: entry must be well-formed, not just present. A traversal or
    // absolute entry would let `bwoc audit run` execute an arbitrary host
    // binary, so reject it statically here too (same rule as the runtime guard).
    if let Some(entry) = plugin_table.get("entry").and_then(|v| v.as_str()) {
        match crate::util::validate_plugin_entry(entry) {
            Ok(()) => report
                .passes
                .push("[plugin].entry is a contained path (no traversal)".to_string()),
            Err(e) => report.violations.push(e),
        }
    }

    // Name matches directory basename.
    let dir_name = plugin_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(name) = plugin_table.get("name").and_then(|v| v.as_str()) {
        if name == dir_name {
            report
                .passes
                .push(format!("[plugin].name matches directory '{dir_name}'"));
        } else {
            report.violations.push(format!(
                "[plugin].name '{name}' does not match directory '{dir_name}'"
            ));
        }
    }

    // Kind ∈ {memory-backend, llm-backend, workflow, audit, jira, okr, council, figma, gws}.
    if let Some(kind) = plugin_table.get("kind").and_then(|v| v.as_str()) {
        if PLUGIN_KINDS.contains(&kind) {
            report
                .passes
                .push(format!("[plugin].kind '{kind}' in supported set"));
        } else {
            report.violations.push(format!(
                "[plugin].kind '{kind}' not in {{memory-backend, llm-backend, workflow, audit, jira, okr, council, figma, gws}}"
            ));
        }
    }

    // Neutrality — vendor names tolerated in description only.
    check_manifest_neutrality_plugin(&raw, &mut report);

    // Criteria audit (BWOC-17). Audit-kind plugins declare their criteria
    // in a sibling `criteria.toml`; validate the static shape so the report
    // surfaces malformed declarations before any audit run.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("audit") {
        audit_audit_criteria(plugin_dir, &mut report);
    }

    // Auth contract audit (BWOC-45). Jira-kind plugins carry credentials; the
    // sibling `auth.toml` declares the credential SHAPE and must never hold a
    // real value. Validate it here so a leaked secret is caught statically.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("jira") {
        audit_jira_auth(plugin_dir, &mut report);
    }

    // Auth contract audit (BWOC-55). Workflow-kind plugins (the `gcloud-*`
    // reference plugins) declare a credential-RESOLUTION shape in a sibling
    // `auth.toml` — file paths and env-var NAMES, never a value. Validate it
    // with the same fail-closed secret-leak guard jira uses, adapted to the
    // `[sources]` shape (notes/2026-05-28_gcloud-workflow-plugin-architecture.md
    // §Decision 3). Workflow plugins without an auth.toml are not audited.
    //
    // Write-verb gate metadata audit (BWOC-70). A write-capable workflow plugin
    // (the EPIC-9 `gcloud-compute` lifecycle slice) declares one `[[verb]]` table
    // per verb with its `write` classification + the operator-confirm gate, so
    // the CLI gate (BWOC-68) and this audit can tell a remote-mutating verb from
    // a free read. Validate the metadata is declared + well-formed. Read-only
    // workflow plugins declare no `[[verb]]` array and are not audited here.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("workflow") {
        audit_workflow_auth(plugin_dir, &mut report);
        audit_workflow_verbs(&raw, &mut report);
    }

    // OKR data audit (BWOC-50). okr-kind plugins (the reference `workspace-okrs`)
    // author their Objectives + Key Results in sibling `objectives.toml` /
    // `key_results.toml`. Validate them against the OKR Progress Schema
    // (PLUGINS.en.md §OKR Progress Schema): referential integrity (every
    // key_result.objective_id resolves), the `unit` / `confidence` closed enums,
    // and the reused audit Evidence-kind vocabulary — caught statically before
    // any `bwoc okr` run.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("okr") {
        audit_okr_data(plugin_dir, &mut report);
    }

    // Council contract audit (BWOC-60). council-kind plugins (the reference
    // `council-sangha-7`) declare a `[council]` table (voting_model + quorum),
    // seed issue templates in a sibling `decisions.toml`, and emit decision
    // records conforming to the Council Decision Schema (PLUGINS.en.md). Validate
    // the manifest table, the templates, and any plugin-local decision records —
    // the deep validation deferred from BWOC-59.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("council") {
        audit_council(&raw, plugin_dir, &mut report);
    }

    // Figma asset-mapping audit (BWOC-65). figma-kind plugins (the reference
    // `figma-rest`) carry an `auth.toml` credential contract (a personal access
    // token, SHAPE only) and emit asset entries conforming to the Figma Asset
    // Mapping Schema (PLUGINS.en.md). Validate the auth.toml with the same
    // fail-closed secret-leak guard jira (BWOC-45) / gcloud (BWOC-55) use, and
    // validate any plugin-local captured asset entries against the schema. The
    // `figma.sh` entry path-traversal safety is already covered by the base
    // `validate_plugin_entry` check above (BWOC-36) — not re-done here.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("figma") {
        audit_figma_auth(plugin_dir, &mut report);
        audit_figma_assets(plugin_dir, &mut report);
    }

    // GWS resource audit (BWOC-77). gws-kind plugins (the read-mostly Google
    // Workspace adapters) carry an OAuth2 credential contract in a sibling
    // `auth.toml` — only the `gws-auth` foundation ships one; the drive/gmail/
    // calendar siblings source the token from it — and emit resource entries
    // conforming to the Workspace Resource Schema (PLUGINS.en.md). Validate the
    // auth.toml with the same fail-closed secret-leak guard jira (BWOC-45) /
    // gcloud (BWOC-55) / figma (BWOC-65) use, and validate any plugin-local
    // captured resource entries against the per-service shape. The `gws.sh` entry
    // path-traversal safety is already covered by the base `validate_plugin_entry`
    // check above (BWOC-36) — not re-done here.
    if plugin_table.get("kind").and_then(|v| v.as_str()) == Some("gws") {
        audit_gws_auth(plugin_dir, &mut report);
        audit_gws_resources(plugin_dir, &mut report);
    }

    report
}

/// Validate the operator-authored OKR data files shipped next to an `okr`-kind
/// plugin's manifest (BWOC-50). Source of truth: PLUGINS.en.md §"OKR Progress
/// Schema" + the `workspace-okrs` SPEC.md data-shape tables.
///
/// `bwoc check` is a static validator — it does not run the plugin. The `report`
/// verb emits one progress entry per key result, derived field-for-field from
/// `key_results.toml`, so validating the rows against the schema is equivalent
/// to validating the emitted Progress entries, caught before any `bwoc okr` run
/// rather than at emit time.
///
/// Two files, validated in dependency order:
///   1. `objectives.toml` — the declared Objectives; collect their ids so the
///      key-result referential check has a resolution set.
///   2. `key_results.toml` — each `[[key_result]]` validated against the OKR
///      Progress Schema: required fields + types, the `unit` / `confidence`
///      closed enums, the reused audit Evidence-kind vocabulary, a unique
///      `key_result_id`, and an `objective_id` that resolves to a declared
///      objective (a dangling reference is a plugin bug, not operator state).
///
/// v1 okr plugins read these two siblings directly — there is no `data_dir`
/// indirection yet (workspace-okrs/manifest.toml header) — so both files are the
/// plugin's core contract and their absence is a violation.
fn audit_okr_data(plugin_dir: &Path, report: &mut AuditReport) {
    let objective_ids = audit_okr_objectives(plugin_dir, report);
    audit_okr_key_results(plugin_dir, report, &objective_ids);
}

/// Validate `objectives.toml` and return the set of declared `objective_id`s
/// for the key-result referential check. Each `[[objective]]` must carry the
/// required string fields (`objective_id`, `title`, `owner`, `period`); the
/// optional `parent` (objective-tree rollup is deferred — SPEC §Status) is not
/// resolved here. Duplicate ids are a violation. On any structural failure the
/// returned set is whatever resolved so far (an empty set cascades into
/// referential violations on every key result, which is the correct signal).
fn audit_okr_objectives(plugin_dir: &Path, report: &mut AuditReport) -> HashSet<String> {
    let mut ids = HashSet::new();
    let path = plugin_dir.join("objectives.toml");
    let body = match fs::read_to_string(&path) {
        Ok(s) => {
            report.passes.push("objectives.toml present".to_string());
            s
        }
        Err(e) => {
            report.violations.push(format!(
                "objectives.toml missing or unreadable: {e} — an okr plugin must author its Objectives"
            ));
            return ids;
        }
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report
                .passes
                .push("objectives.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("objectives.toml is not valid TOML: {e}"));
            return ids;
        }
    };

    let objectives = match raw.get("objective").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => {
            report
                .passes
                .push(format!("objectives.toml declares {} objective(s)", a.len()));
            a
        }
        Some(_) => {
            report.violations.push(
                "objectives.toml [[objective]] array is empty — declare at least one Objective"
                    .to_string(),
            );
            return ids;
        }
        None => {
            report.violations.push(
                "objectives.toml declares no [[objective]] entries — okr plugins must declare at least one"
                    .to_string(),
            );
            return ids;
        }
    };

    for (i, obj) in objectives.iter().enumerate() {
        let pos = i + 1;
        let table = match obj.as_table() {
            Some(t) => t,
            None => {
                report.violations.push(format!(
                    "objective #{pos} is not a table — expected [[objective]] with scalar fields"
                ));
                continue;
            }
        };
        for field in &["objective_id", "title", "owner", "period"] {
            match table.get(*field) {
                Some(toml::Value::String(s)) if !s.is_empty() => {}
                Some(toml::Value::String(_)) => report.violations.push(format!(
                    "objective #{pos} {field} is empty — required field"
                )),
                Some(_) => report.violations.push(format!(
                    "objective #{pos} {field} has wrong type — expected string"
                )),
                None => report
                    .violations
                    .push(format!("objective #{pos} missing required '{field}'")),
            }
        }
        if let Some(id) = table.get("objective_id").and_then(|v| v.as_str()) {
            if !id.is_empty() && !ids.insert(id.to_string()) {
                report.violations.push(format!(
                    "objective_id '{id}' declared more than once — ids must be unique"
                ));
            }
        }
    }
    ids
}

/// Validate `key_results.toml` against the OKR Progress Schema (PLUGINS.en.md
/// §"OKR Progress Schema"). Each `[[key_result]]` is one progress entry: a
/// unique `key_result_id`; an `objective_id` that resolves to a declared
/// objective (referential integrity); a non-empty `description`; numeric
/// `target` / `current`; the closed `unit` / `confidence` enums; an `evidence`
/// inline table over the reused audit Evidence-kind vocabulary; and an optional
/// ISO-8601 `as_of`.
fn audit_okr_key_results(
    plugin_dir: &Path,
    report: &mut AuditReport,
    objective_ids: &HashSet<String>,
) {
    let path = plugin_dir.join("key_results.toml");
    let body = match fs::read_to_string(&path) {
        Ok(s) => {
            report.passes.push("key_results.toml present".to_string());
            s
        }
        Err(e) => {
            report.violations.push(format!(
                "key_results.toml missing or unreadable: {e} — an okr plugin must author its Key Results"
            ));
            return;
        }
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report
                .passes
                .push("key_results.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("key_results.toml is not valid TOML: {e}"));
            return;
        }
    };

    let krs = match raw.get("key_result").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => {
            report.passes.push(format!(
                "key_results.toml declares {} key result(s)",
                a.len()
            ));
            a
        }
        Some(_) => {
            report.violations.push(
                "key_results.toml [[key_result]] array is empty — declare at least one Key Result"
                    .to_string(),
            );
            return;
        }
        None => {
            report.violations.push(
                "key_results.toml declares no [[key_result]] entries — okr plugins must declare at least one"
                    .to_string(),
            );
            return;
        }
    };

    let mut seen_ids: HashSet<String> = HashSet::new();
    for (i, kr) in krs.iter().enumerate() {
        let pos = i + 1;
        let table = match kr.as_table() {
            Some(t) => t,
            None => {
                report.violations.push(format!(
                    "key result #{pos} is not a table — expected [[key_result]] with scalar fields"
                ));
                continue;
            }
        };
        // A stable label for messages: the id when present, else the position.
        let label = table
            .get("key_result_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| format!("'{s}'"))
            .unwrap_or_else(|| format!("#{pos}"));

        // key_result_id — required, non-empty, unique within the plugin.
        match table.get("key_result_id") {
            Some(toml::Value::String(s)) if !s.is_empty() => {
                if !seen_ids.insert(s.clone()) {
                    report.violations.push(format!(
                        "key_result_id '{s}' declared more than once — ids must be unique within the plugin"
                    ));
                }
            }
            Some(toml::Value::String(_)) => report.violations.push(format!(
                "key result {label} key_result_id is empty — required field"
            )),
            Some(_) => report.violations.push(format!(
                "key result {label} key_result_id has wrong type — expected string"
            )),
            None => report.violations.push(format!(
                "key result {label} missing required 'key_result_id'"
            )),
        }

        // objective_id — required, non-empty, referential.
        match table.get("objective_id").and_then(|v| v.as_str()) {
            Some(oid) if !oid.is_empty() => {
                if objective_ids.contains(oid) {
                    report
                        .passes
                        .push(format!("key result {label} objective_id '{oid}' resolves"));
                } else {
                    report.violations.push(format!(
                        "key result {label} objective_id '{oid}' does not resolve to a declared objective — dangling reference"
                    ));
                }
            }
            _ => report.violations.push(format!(
                "key result {label} missing required 'objective_id' (string)"
            )),
        }

        // description — required authoring field (SPEC data shape).
        match table.get("description") {
            Some(toml::Value::String(s)) if !s.is_empty() => {}
            Some(_) => report.violations.push(format!(
                "key result {label} description has wrong type or is empty — expected non-empty string"
            )),
            None => report
                .violations
                .push(format!("key result {label} missing required 'description'")),
        }

        // target / current — required numbers (integer or float).
        for field in &["target", "current"] {
            match table.get(*field) {
                Some(v) if v.is_integer() || v.is_float() => {}
                Some(_) => report.violations.push(format!(
                    "key result {label} {field} has wrong type — expected a number"
                )),
                None => report.violations.push(format!(
                    "key result {label} missing required '{field}' (number)"
                )),
            }
        }

        // unit — required, closed enum.
        match table.get("unit") {
            Some(toml::Value::String(s)) => {
                if OKR_UNITS.contains(&s.as_str()) {
                    report
                        .passes
                        .push(format!("key result {label} unit '{s}' in supported set"));
                } else {
                    report.violations.push(format!(
                        "key result {label} unit '{s}' not in {{count, percent, currency, ratio, boolean}}"
                    ));
                }
            }
            Some(_) => report.violations.push(format!(
                "key result {label} unit has wrong type — expected string"
            )),
            None => report
                .violations
                .push(format!("key result {label} missing required 'unit'")),
        }

        // confidence — required, closed enum.
        match table.get("confidence") {
            Some(toml::Value::String(s)) => {
                if OKR_CONFIDENCE_LEVELS.contains(&s.as_str()) {
                    report.passes.push(format!(
                        "key result {label} confidence '{s}' in supported set"
                    ));
                } else {
                    report.violations.push(format!(
                        "key result {label} confidence '{s}' not in {{high, medium, low}}"
                    ));
                }
            }
            Some(_) => report.violations.push(format!(
                "key result {label} confidence has wrong type — expected string"
            )),
            None => report
                .violations
                .push(format!("key result {label} missing required 'confidence'")),
        }

        // evidence — required inline table over the reused audit Evidence kinds.
        check_okr_evidence(&label, table, report);

        // as_of — optional ISO-8601 date; when present must be a string.
        if let Some(v) = table.get("as_of") {
            if !v.is_str() {
                report.violations.push(format!(
                    "key result {label} as_of has wrong type — expected an ISO-8601 date string"
                ));
            }
        }
    }
}

/// Validate an OKR key result's `evidence` inline table against the reused audit
/// Evidence-kind vocabulary (PLUGINS.en.md §"Evidence kinds"; the okr kind
/// introduces none of its own). The `report` verb emits `{ kind, value }`, so
/// that is the shape validated: `kind` is the closed enum, `value` is a string,
/// and the Musāvāda guard requires a non-empty referent for any kind but `none`
/// (which conversely must carry an empty value — no claim without a referent).
fn check_okr_evidence(
    label: &str,
    table: &toml::map::Map<String, toml::Value>,
    report: &mut AuditReport,
) {
    let evidence = match table.get("evidence") {
        Some(toml::Value::Table(t)) => t,
        Some(_) => {
            report.violations.push(format!(
                "key result {label} evidence has wrong type — expected an inline table {{ kind, value }}"
            ));
            return;
        }
        None => {
            report.violations.push(format!(
                "key result {label} missing required 'evidence' {{ kind, value }}"
            ));
            return;
        }
    };
    let kind = match evidence.get("kind") {
        Some(toml::Value::String(s)) => s.as_str(),
        Some(_) => {
            report.violations.push(format!(
                "key result {label} evidence.kind has wrong type — expected string"
            ));
            return;
        }
        None => {
            report.violations.push(format!(
                "key result {label} evidence.kind missing — required"
            ));
            return;
        }
    };
    if !EVIDENCE_KINDS.contains(&kind) {
        report.violations.push(format!(
            "key result {label} evidence.kind '{kind}' not in {{file, content, command, attestation, sample, none}}"
        ));
        return;
    }
    match evidence.get("value") {
        Some(toml::Value::String(s)) => {
            if kind == "none" {
                if s.is_empty() {
                    report.passes.push(format!(
                        "key result {label} evidence kind 'none' (no referent)"
                    ));
                } else {
                    report.violations.push(format!(
                        "key result {label} evidence.kind='none' but value is non-empty — 'none' carries no referent"
                    ));
                }
            } else if s.is_empty() {
                report.violations.push(format!(
                    "key result {label} evidence.kind='{kind}' but value is empty — a tracked value must carry a reproducible referent (Musāvāda)"
                ));
            } else {
                report.passes.push(format!(
                    "key result {label} evidence '{kind}' carries a referent"
                ));
            }
        }
        Some(_) => report.violations.push(format!(
            "key result {label} evidence.value has wrong type — expected string"
        )),
        None => report.violations.push(format!(
            "key result {label} evidence.value missing — required (empty string when kind='none')"
        )),
    }
}

/// Validate a `council`-kind plugin's council-specific contract (BWOC-60).
/// Source of truth: PLUGINS.en.md §"Council Decision Schema" + the
/// `council-sangha-7` SPEC.md §Configuration + the BWOC-56 design note (§3
/// voting models, §4 quorum). `bwoc check` (BWOC-59) already accepts the
/// `council` kind at basic well-formedness; this is the deep validation that
/// story deferred here.
///
/// Three concerns:
///   1. the manifest `[council]` table — `voting_model` ∈ the four models and
///      `quorum` (an integer count or `"n/m"` fraction) are validated through
///      the SAME parsers the runtime tally uses (`council::validate_*`), so the
///      static audit and the runtime cannot drift on what is well-formed.
///   2. `decisions.toml` — the issue templates that seed a decision's question +
///      options on `propose --template`; each `[[template]]` must be well-formed.
///   3. any plugin-local `records/*.json` decision entries — validated against
///      the Council Decision Schema. Decision records are runtime state (the
///      normal store is `<workspace>/.bwoc/council/`); the SPEC documents a
///      plugin-local `records/` fallback, so when that dir is present each entry
///      is validated, and when absent (the shipped reference plugin carries
///      templates, not records) there is nothing to audit.
fn audit_council(raw: &toml::Value, plugin_dir: &Path, report: &mut AuditReport) {
    audit_council_manifest(raw, report);
    audit_council_templates(plugin_dir, report);
    audit_council_records(plugin_dir, report);
}

/// Validate the `[council]` table in a council plugin's manifest: `voting_model`
/// in the closed four-model set and `quorum` present + well-formed. Both route
/// through the runtime `council::validate_*` parsers so a value the runtime would
/// reject cannot pass `bwoc check`, and vice versa (anti-drift, BWOC-60).
fn audit_council_manifest(raw: &toml::Value, report: &mut AuditReport) {
    let council = match raw.get("council").and_then(|v| v.as_table()) {
        Some(t) => {
            report.passes.push("[council] table present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[council] table missing — a council plugin must declare voting_model + quorum"
                    .to_string(),
            );
            return;
        }
    };

    // voting_model — required string; the runtime VotingModel parser is the
    // authority on the accepted set, so accept/reject can never drift from it.
    match council.get("voting_model") {
        Some(toml::Value::String(vm)) => {
            if crate::council::validate_voting_model(vm).is_ok() {
                report
                    .passes
                    .push(format!("[council].voting_model '{vm}' in supported set"));
            } else {
                report.violations.push(format!(
                    "[council].voting_model '{vm}' not in {{simple-majority, consensus, weighted, sangha}}"
                ));
            }
        }
        Some(_) => report
            .violations
            .push("[council].voting_model has wrong type — expected string".to_string()),
        None => report
            .violations
            .push("[council].voting_model missing — required field".to_string()),
    }

    // quorum — required; a positive integer count or an "n/m" fraction string.
    // Validated through the same parser the runtime tally uses.
    match council.get("quorum") {
        Some(q) => match crate::council::validate_quorum(q) {
            Ok(()) => report
                .passes
                .push("[council].quorum is well-formed (count or fraction)".to_string()),
            Err(e) => report
                .violations
                .push(format!("[council].quorum is malformed — {e}")),
        },
        None => report
            .violations
            .push("[council].quorum missing — required field".to_string()),
    }
}

/// Validate the `decisions.toml` issue templates shipped next to a council
/// plugin's manifest. Each `[[template]]` seeds a decision's `question` +
/// `options` on `propose --template <id>` (SPEC.md §How it runs), so the static
/// shape is validated before any `bwoc council` run: a unique non-empty
/// `template_id`, an integer `condition`, a non-empty `name`, a non-empty
/// `question`, and `options` — a string array of ≥2 choices (the Council
/// Decision Schema fixes `options` ≥2 at propose time).
///
/// An absent `decisions.toml` is a violation: the reference council plugin's
/// templates are its seed contract (SPEC.md §Configuration — "v1 reads templates
/// from decisions.toml").
fn audit_council_templates(plugin_dir: &Path, report: &mut AuditReport) {
    let path = plugin_dir.join("decisions.toml");
    let body = match fs::read_to_string(&path) {
        Ok(s) => {
            report.passes.push("decisions.toml present".to_string());
            s
        }
        Err(e) => {
            report.violations.push(format!(
                "decisions.toml missing or unreadable: {e} — a council plugin must seed its issue templates"
            ));
            return;
        }
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report
                .passes
                .push("decisions.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("decisions.toml is not valid TOML: {e}"));
            return;
        }
    };

    let templates = match raw.get("template").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => {
            report
                .passes
                .push(format!("decisions.toml declares {} template(s)", a.len()));
            a
        }
        Some(_) => {
            report.violations.push(
                "decisions.toml [[template]] array is empty — declare at least one issue template"
                    .to_string(),
            );
            return;
        }
        None => {
            report.violations.push(
                "decisions.toml declares no [[template]] entries — a council plugin must seed at least one"
                    .to_string(),
            );
            return;
        }
    };

    let mut seen_ids: HashSet<String> = HashSet::new();
    for (i, tmpl) in templates.iter().enumerate() {
        let pos = i + 1;
        let table = match tmpl.as_table() {
            Some(t) => t,
            None => {
                report.violations.push(format!(
                    "template #{pos} is not a table — expected [[template]] with scalar fields"
                ));
                continue;
            }
        };
        let label = table
            .get("template_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| format!("'{s}'"))
            .unwrap_or_else(|| format!("#{pos}"));

        // template_id — required, non-empty, unique within the plugin.
        match table.get("template_id") {
            Some(toml::Value::String(s)) if !s.is_empty() => {
                if !seen_ids.insert(s.clone()) {
                    report.violations.push(format!(
                        "template_id '{s}' declared more than once — ids must be unique"
                    ));
                }
            }
            Some(toml::Value::String(_)) => report.violations.push(format!(
                "template {label} template_id is empty — required field"
            )),
            Some(_) => report.violations.push(format!(
                "template {label} template_id has wrong type — expected string"
            )),
            None => report
                .violations
                .push(format!("template {label} missing required 'template_id'")),
        }

        // condition — required integer (which Aparihaniya-dhamma signal, 1..7).
        match table.get("condition") {
            Some(v) if v.is_integer() => {}
            Some(_) => report.violations.push(format!(
                "template {label} condition has wrong type — expected an integer"
            )),
            None => report.violations.push(format!(
                "template {label} missing required 'condition' (integer)"
            )),
        }

        // name / question — required, non-empty strings.
        for field in &["name", "question"] {
            match table.get(*field) {
                Some(toml::Value::String(s)) if !s.is_empty() => {}
                Some(_) => report.violations.push(format!(
                    "template {label} {field} has wrong type or is empty — expected non-empty string"
                )),
                None => report
                    .violations
                    .push(format!("template {label} missing required '{field}'")),
            }
        }

        // options — required string array of ≥2 choices.
        match table.get("options") {
            Some(toml::Value::Array(opts)) if opts.iter().all(|o| o.is_str()) => {
                if opts.len() >= 2 {
                    report
                        .passes
                        .push(format!("template {label} declares {} options", opts.len()));
                } else {
                    report.violations.push(format!(
                        "template {label} options must declare ≥2 choices, found {}",
                        opts.len()
                    ));
                }
            }
            Some(toml::Value::Array(_)) => report.violations.push(format!(
                "template {label} options must be an array of strings"
            )),
            Some(_) => report.violations.push(format!(
                "template {label} options has wrong type — expected an array of strings"
            )),
            None => report.violations.push(format!(
                "template {label} missing required 'options' (array of ≥2 strings)"
            )),
        }
    }
}

/// Validate any plugin-local decision records against the Council Decision
/// Schema. Decision records persist as one JSON file per decision; the normal
/// store is `<workspace>/.bwoc/council/`, but the SPEC documents a plugin-local
/// `records/` fallback for hand-invocation / smoke tests. When that directory is
/// present, every `*.json` in it is validated; when absent (the shipped
/// reference plugin ships templates, not records), there is nothing to audit.
fn audit_council_records(plugin_dir: &Path, report: &mut AuditReport) {
    let records_dir = plugin_dir.join("records");
    let read = match fs::read_dir(&records_dir) {
        Ok(r) => r,
        // No plugin-local records — decision records live under the workspace.
        Err(_) => return,
    };
    let mut json_paths: Vec<std::path::PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    json_paths.sort();
    for path in json_paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                report
                    .violations
                    .push(format!("records/{name} unreadable: {e}"));
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                report
                    .violations
                    .push(format!("records/{name} is not valid JSON: {e}"));
                continue;
            }
        };
        validate_council_decision(&name, &value, report);
    }
}

/// Validate one decision entry against the Council Decision Schema (PLUGINS.en.md
/// §"Council Decision Schema"). `label` identifies the record in messages. The
/// schema is the `council` kind's contract — the reference plugin's verbs and the
/// `bwoc council` CLI emit entries of this shape, and `bwoc check` validates it
/// (BWOC-60). The append-only fields (`rounds`, `votes`) are validated by
/// per-element shape; append-only itself is a cross-snapshot protocol invariant,
/// not observable in one record. Operational fields beyond the schema's named set
/// (`question`, `effect`, a vote `rationale`, `cast_at`) are ignored, not
/// rejected — the validator checks the contract, additive fields are allowed.
fn validate_council_decision(label: &str, value: &serde_json::Value, report: &mut AuditReport) {
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            report.violations.push(format!(
                "decision {label} is not a JSON object — expected a Council Decision entry"
            ));
            return;
        }
    };

    // decision_id — required, non-empty string (the stable key).
    match obj.get("decision_id") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => report
            .passes
            .push(format!("decision {label} decision_id present")),
        _ => report.violations.push(format!(
            "decision {label} missing required 'decision_id' (non-empty string)"
        )),
    }

    // status — required, closed protocol enum.
    match obj.get("status").and_then(|v| v.as_str()) {
        Some(s) if COUNCIL_STATUSES.contains(&s) => report
            .passes
            .push(format!("decision {label} status '{s}' in supported set")),
        Some(s) => report.violations.push(format!(
            "decision {label} status '{s}' not in {{proposed, discussing, voting, resolved, abandoned}}"
        )),
        None => report
            .violations
            .push(format!("decision {label} missing required 'status' (string)")),
    }

    // participants — required array of agent-id strings (may be empty for an
    // "open" council where quorum counts whoever votes).
    match obj.get("participants") {
        Some(serde_json::Value::Array(a)) => {
            if a.iter().all(|p| p.is_string()) {
                report.passes.push(format!(
                    "decision {label} participants is a string array ({} member(s))",
                    a.len()
                ));
            } else {
                report.violations.push(format!(
                    "decision {label} participants must be an array of agent-id strings"
                ));
            }
        }
        _ => report.violations.push(format!(
            "decision {label} missing required 'participants' (array of strings)"
        )),
    }

    // options — required string array of ≥2 choices (fixed at propose time).
    match obj.get("options") {
        Some(serde_json::Value::Array(a)) if a.iter().all(|o| o.is_string()) => {
            if a.len() >= 2 {
                report
                    .passes
                    .push(format!("decision {label} declares {} options", a.len()));
            } else {
                report.violations.push(format!(
                    "decision {label} options must declare ≥2 choices, found {}",
                    a.len()
                ));
            }
        }
        Some(serde_json::Value::Array(_)) => report.violations.push(format!(
            "decision {label} options must be an array of strings"
        )),
        _ => report.violations.push(format!(
            "decision {label} missing required 'options' (array of ≥2 strings)"
        )),
    }

    validate_council_rounds(label, obj.get("rounds"), report);
    validate_council_votes(label, obj.get("votes"), report);

    // dissent / evidence_links — optional; validated only when present (omitted,
    // never null, when absent — per the schema's omit-don't-null convention).
    if let Some(d) = obj.get("dissent") {
        validate_council_dissent(label, d, report);
    }
    if let Some(e) = obj.get("evidence_links") {
        validate_council_evidence_links(label, e, report);
    }

    // opened_at — required ISO-8601 datetime string.
    match obj.get("opened_at") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => {}
        _ => report.violations.push(format!(
            "decision {label} missing required 'opened_at' (ISO-8601 datetime string)"
        )),
    }
}

/// Validate a decision's `rounds`: a required array where each round carries a
/// `turns` array of `{ participant, message_ref }` (Council Decision Schema).
fn validate_council_rounds(
    label: &str,
    rounds: Option<&serde_json::Value>,
    report: &mut AuditReport,
) {
    let arr = match rounds {
        Some(serde_json::Value::Array(a)) => a,
        Some(_) => {
            report.violations.push(format!(
                "decision {label} rounds has wrong type — expected an array"
            ));
            return;
        }
        None => {
            report.violations.push(format!(
                "decision {label} missing required 'rounds' (array)"
            ));
            return;
        }
    };
    let mut ok = true;
    for (i, round) in arr.iter().enumerate() {
        let rpos = i + 1;
        let turns = match round.as_object().and_then(|o| o.get("turns")) {
            Some(serde_json::Value::Array(t)) => t,
            _ => {
                report.violations.push(format!(
                    "decision {label} round #{rpos} missing 'turns' (array of {{ participant, message_ref }})"
                ));
                ok = false;
                continue;
            }
        };
        for (j, turn) in turns.iter().enumerate() {
            let tpos = j + 1;
            let to = turn.as_object();
            let has_participant = matches!(
                to.and_then(|o| o.get("participant")),
                Some(serde_json::Value::String(s)) if !s.is_empty()
            );
            let has_ref = matches!(
                to.and_then(|o| o.get("message_ref")),
                Some(serde_json::Value::String(s)) if !s.is_empty()
            );
            if !has_participant || !has_ref {
                report.violations.push(format!(
                    "decision {label} round #{rpos} turn #{tpos} must carry non-empty 'participant' + 'message_ref' strings"
                ));
                ok = false;
            }
        }
    }
    if ok {
        report.passes.push(format!(
            "decision {label} rounds well-formed ({} round(s))",
            arr.len()
        ));
    }
}

/// Validate a decision's `votes`: a required append-only array where each cast is
/// `{ participant, option, abstain }`. A non-abstaining vote names a non-empty
/// `option`; an abstention carries no `option` (Council Decision Schema + the
/// runtime `VoteRecord` shape).
fn validate_council_votes(
    label: &str,
    votes: Option<&serde_json::Value>,
    report: &mut AuditReport,
) {
    let arr = match votes {
        Some(serde_json::Value::Array(a)) => a,
        Some(_) => {
            report.violations.push(format!(
                "decision {label} votes has wrong type — expected an array"
            ));
            return;
        }
        None => {
            report
                .violations
                .push(format!("decision {label} missing required 'votes' (array)"));
            return;
        }
    };
    let mut ok = true;
    for (i, vote) in arr.iter().enumerate() {
        let vpos = i + 1;
        let vo = match vote.as_object() {
            Some(o) => o,
            None => {
                report.violations.push(format!(
                    "decision {label} vote #{vpos} is not an object — expected {{ participant, option, abstain }}"
                ));
                ok = false;
                continue;
            }
        };
        if !matches!(vo.get("participant"), Some(serde_json::Value::String(s)) if !s.is_empty()) {
            report.violations.push(format!(
                "decision {label} vote #{vpos} missing 'participant' (non-empty string)"
            ));
            ok = false;
        }
        let abstain = match vo.get("abstain") {
            Some(serde_json::Value::Bool(b)) => *b,
            _ => {
                report.violations.push(format!(
                    "decision {label} vote #{vpos} missing 'abstain' (boolean)"
                ));
                ok = false;
                continue;
            }
        };
        match vo.get("option") {
            Some(serde_json::Value::String(s)) => {
                if !abstain && s.is_empty() {
                    report.violations.push(format!(
                        "decision {label} vote #{vpos} has an empty 'option' — a non-abstaining vote names a choice"
                    ));
                    ok = false;
                }
            }
            None | Some(serde_json::Value::Null) => {
                if !abstain {
                    report.violations.push(format!(
                        "decision {label} vote #{vpos} is not an abstention but names no 'option'"
                    ));
                    ok = false;
                }
            }
            Some(_) => {
                report.violations.push(format!(
                    "decision {label} vote #{vpos} option has wrong type — expected a string"
                ));
                ok = false;
            }
        }
    }
    if ok {
        report.passes.push(format!(
            "decision {label} votes well-formed ({} cast)",
            arr.len()
        ));
    }
}

/// Validate a decision's optional `dissent` array. Each entry is
/// `{ participant, option, rationale? }` — recorded minority positions preserved
/// on resolve (Council Decision Schema + the runtime `Dissent` shape).
fn validate_council_dissent(label: &str, dissent: &serde_json::Value, report: &mut AuditReport) {
    let arr = match dissent {
        serde_json::Value::Array(a) => a,
        _ => {
            report.violations.push(format!(
                "decision {label} dissent has wrong type — expected an array of {{ participant, option, rationale }}"
            ));
            return;
        }
    };
    let mut ok = true;
    for (i, d) in arr.iter().enumerate() {
        let dpos = i + 1;
        let dobj = match d.as_object() {
            Some(o) => o,
            None => {
                report
                    .violations
                    .push(format!("decision {label} dissent #{dpos} is not an object"));
                ok = false;
                continue;
            }
        };
        let has_participant = matches!(
            dobj.get("participant"),
            Some(serde_json::Value::String(s)) if !s.is_empty()
        );
        let has_option =
            matches!(dobj.get("option"), Some(serde_json::Value::String(s)) if !s.is_empty());
        if !has_participant || !has_option {
            report.violations.push(format!(
                "decision {label} dissent #{dpos} must carry non-empty 'participant' + 'option' strings"
            ));
            ok = false;
        }
        if let Some(r) = dobj.get("rationale") {
            if !r.is_string() && !r.is_null() {
                report.violations.push(format!(
                    "decision {label} dissent #{dpos} rationale has wrong type — expected a string"
                ));
                ok = false;
            }
        }
    }
    if ok && !arr.is_empty() {
        report.passes.push(format!(
            "decision {label} dissent well-formed ({} entry/entries)",
            arr.len()
        ));
    }
}

/// Validate a decision's optional `evidence_links` array. It REUSES the audit
/// [Evidence kinds] — `{ kind, value }` over the closed `EVIDENCE_KINDS`
/// vocabulary; the council kind introduces none of its own (Council Decision
/// Schema).
fn validate_council_evidence_links(
    label: &str,
    links: &serde_json::Value,
    report: &mut AuditReport,
) {
    let arr = match links {
        serde_json::Value::Array(a) => a,
        _ => {
            report.violations.push(format!(
                "decision {label} evidence_links has wrong type — expected an array of {{ kind, value }}"
            ));
            return;
        }
    };
    let mut ok = true;
    for (i, link) in arr.iter().enumerate() {
        let lpos = i + 1;
        let lobj = match link.as_object() {
            Some(o) => o,
            None => {
                report.violations.push(format!(
                    "decision {label} evidence_link #{lpos} is not an object"
                ));
                ok = false;
                continue;
            }
        };
        match lobj.get("kind").and_then(|v| v.as_str()) {
            Some(k) if EVIDENCE_KINDS.contains(&k) => {}
            Some(k) => {
                report.violations.push(format!(
                    "decision {label} evidence_link #{lpos} kind '{k}' not in {{file, content, command, attestation, sample, none}}"
                ));
                ok = false;
            }
            None => {
                report.violations.push(format!(
                    "decision {label} evidence_link #{lpos} missing 'kind' (string)"
                ));
                ok = false;
            }
        }
        if !lobj.get("value").map(|v| v.is_string()).unwrap_or(false) {
            report.violations.push(format!(
                "decision {label} evidence_link #{lpos} missing 'value' (string)"
            ));
            ok = false;
        }
    }
    if ok && !arr.is_empty() {
        report.passes.push(format!(
            "decision {label} evidence_links reuse the audit Evidence kinds"
        ));
    }
}

/// Validate the `auth.toml` credential CONTRACT shipped next to a
/// `workflow`-kind plugin's manifest (BWOC-55). Source of truth: the
/// `gcloud-auth` / `gcloud-project` SPEC.md §Authentication, the `auth.toml`
/// header, and notes/2026-05-28_gcloud-workflow-plugin-architecture.md
/// §Decision 3. The file declares the credential SHAPE only — which sources are
/// consulted, the file paths, and the env-var NAMES — and carries NO secret
/// value. The plugin never reads a credential value, so a malformed `auth.toml`
/// cannot leak a token; this check makes that invariant enforceable statically.
///
/// Two concerns, in order of severity (mirrors `audit_jira_auth`, BWOC-45):
///   1. SECURITY (fail-closed) — each `[sources.<src>]` table may carry ONLY
///      its declared shape keys (`path`/`priority` for the file sources,
///      `vars`/`priority` for the env source). ANY other key is treated as an
///      inline credential value — the single worst outcome this check exists to
///      prevent — and is a hard violation. The value is NEVER echoed back.
///   2. SHAPE — `[sources]` declares the three precedence-ordered sources
///      `adc` / `service_account` / `env`, each well-typed, so the runtime
///      resolution contract is present and well-formed.
///
/// An absent `auth.toml` is not audited — a workflow plugin need not carry
/// credentials (the contract is validated only when the file exists, mirroring
/// the BWOC-45 jira scope).
fn audit_workflow_auth(plugin_dir: &Path, report: &mut AuditReport) {
    let auth_path = plugin_dir.join("auth.toml");
    let body = match fs::read_to_string(&auth_path) {
        Ok(s) => {
            report.passes.push("auth.toml present".to_string());
            s
        }
        // No auth.toml → nothing to validate here.
        Err(_) => return,
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report.passes.push("auth.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("auth.toml is not valid TOML: {e}"));
            return;
        }
    };

    // [sources] — the credential-resolution contract table.
    let sources = match raw.get("sources").and_then(|s| s.as_table()) {
        Some(t) => {
            report.passes.push("[sources] table present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[sources] table missing — auth.toml must declare the credential-resolution \
                 contract (adc / service_account / env)"
                    .to_string(),
            );
            return;
        }
    };

    // Each source: present, a table, carrying ONLY its declared shape keys.
    // `path`/`vars` are the resolution descriptors; `priority` orders precedence.
    // Any other key is treated as an inline credential value — fail closed and
    // never echo it (echoing would re-leak the secret into `bwoc check` output).
    for (src, allowed) in [
        ("adc", &["path", "priority"][..]),
        ("service_account", &["path", "priority"][..]),
        ("env", &["vars", "priority"][..]),
    ] {
        let table = match sources.get(src) {
            Some(toml::Value::Table(t)) => {
                report.passes.push(format!("[sources].{src} declared"));
                t
            }
            Some(_) => {
                report.violations.push(format!(
                    "[sources].{src} has wrong type — expected an inline table of shape descriptors"
                ));
                continue;
            }
            None => {
                report.violations.push(format!(
                    "[sources].{src} missing — required credential-source key"
                ));
                continue;
            }
        };

        // Fail-closed secret-leak guard: reject any key outside the shape set.
        for key in table.keys() {
            if !allowed.contains(&key.as_str()) {
                report.violations.push(format!(
                    "[sources].{src}.{key} is not a declared shape key — auth.toml carries SHAPE \
                     only; an inline credential value MUST NOT be committed (value redacted)"
                ));
            }
        }

        // Shape type checks for the declared descriptors.
        if src == "env" {
            match table.get("vars") {
                Some(toml::Value::Array(a)) if !a.is_empty() && a.iter().all(|v| v.is_str()) => {
                    report
                        .passes
                        .push("[sources].env.vars names the resolving env vars".to_string());
                }
                Some(_) => report.violations.push(
                    "[sources].env.vars has wrong type — expected a non-empty array of \
                     env-var-name strings"
                        .to_string(),
                ),
                None => report.violations.push(
                    "[sources].env.vars missing — the env source must name the variables it \
                     resolves from"
                        .to_string(),
                ),
            }
        } else {
            match table.get("path") {
                Some(toml::Value::String(s)) if !s.is_empty() => report
                    .passes
                    .push(format!("[sources].{src}.path declares a resolution path")),
                Some(_) => report.violations.push(format!(
                    "[sources].{src}.path has wrong type — expected a non-empty path string"
                )),
                None => report.violations.push(format!(
                    "[sources].{src}.path missing — a file source must declare its path"
                )),
            }
        }

        // `priority`, when present, orders source precedence — must be an integer.
        if let Some(p) = table.get("priority") {
            if !p.is_integer() {
                report.violations.push(format!(
                    "[sources].{src}.priority has wrong type — expected an integer precedence rank"
                ));
            }
        }
    }
}

/// Validate the write-verb gate metadata declared in a `workflow`-kind plugin's
/// manifest (BWOC-70). Source of truth: PLUGINS.en.md §"Write verbs — the
/// operator-confirm gate" (BWOC-67) + the BWOC-66 gcloud-compute risk matrix
/// (notes/2026-05-28_gcloud-compute-write-verbs.md).
///
/// A write-capable workflow plugin (the EPIC-9 `gcloud-compute` lifecycle slice)
/// declares one `[[verb]]` table per verb so the `bwoc <cli>` gate (BWOC-68) and
/// `bwoc check` (this audit) can see which verbs mutate external state. The
/// metadata is the static contract; this audit confirms it is declared and
/// well-formed before any `invoke`:
///
///   1. `name`  — non-empty string, unique across the verb set.
///   2. `write` — boolean; the write classification itself. Its absence is the
///      gap this audit closes — an undeclared classification means the CLI gate
///      cannot tell a remote-mutating verb from a free read.
///   3. `confirm` — required `"operator"` on every `write = true` verb (the
///      normative operator-confirm gate). A write verb missing it would be
///      reachable without the documented confirmation. Read verbs (`write =
///      false`) carry no gate — a `confirm` on one is contradictory metadata
///      (warned, not failed; read verbs are free).
///
/// Read-only workflow plugins (the EPIC-8 `gcloud-auth` / `gcloud-project`)
/// declare no `[[verb]]` array; their absence is not audited (mirrors
/// `audit_workflow_auth`'s treatment of a missing `auth.toml`).
fn audit_workflow_verbs(raw: &toml::Value, report: &mut AuditReport) {
    let verbs = match raw.get("verb") {
        Some(toml::Value::Array(a)) => a,
        // A workflow plugin may be read-only — no verb metadata to validate.
        None => return,
        Some(_) => {
            report
                .violations
                .push("[[verb]] has wrong type — expected an array of verb tables".to_string());
            return;
        }
    };
    if verbs.is_empty() {
        report.violations.push(
            "[[verb]] declared but empty — verb metadata must name at least one verb".to_string(),
        );
        return;
    }
    report.passes.push(format!(
        "[[verb]] write-gate metadata declared ({} verbs)",
        verbs.len()
    ));

    let mut seen: HashSet<String> = HashSet::new();
    for (i, verb) in verbs.iter().enumerate() {
        let table = match verb.as_table() {
            Some(t) => t,
            None => {
                report
                    .violations
                    .push(format!("[[verb]] entry #{} is not a table", i + 1));
                continue;
            }
        };

        // name — non-empty string, unique across the verb set.
        let name = match table.get("name") {
            Some(toml::Value::String(s)) if !s.is_empty() => {
                if !seen.insert(s.clone()) {
                    report
                        .violations
                        .push(format!("[[verb]].name '{s}' is declared more than once"));
                }
                s.clone()
            }
            Some(_) => {
                report.violations.push(format!(
                    "[[verb]] entry #{} has an empty or non-string 'name'",
                    i + 1
                ));
                continue;
            }
            None => {
                report
                    .violations
                    .push(format!("[[verb]] entry #{} missing required 'name'", i + 1));
                continue;
            }
        };

        // write — the classification boolean. Its presence is the whole point of
        // the gate metadata; without it the CLI cannot gate the verb.
        let is_write = match table.get("write") {
            Some(toml::Value::Boolean(b)) => {
                report
                    .passes
                    .push(format!("[[verb]] '{name}' declares write = {b}"));
                *b
            }
            Some(_) => {
                report.violations.push(format!(
                    "[[verb]] '{name}'.write has wrong type — expected a boolean write classification"
                ));
                continue;
            }
            None => {
                report.violations.push(format!(
                    "[[verb]] '{name}' missing 'write' — every workflow verb must declare its write \
                     classification (PLUGINS.en.md §Write verbs)"
                ));
                continue;
            }
        };

        // confirm — required "operator" on writes; forbidden (redundant) on reads.
        match (is_write, table.get("confirm")) {
            (true, Some(toml::Value::String(s))) if s == "operator" => report.passes.push(format!(
                "[[verb]] '{name}' write carries the operator-confirm gate"
            )),
            (true, Some(toml::Value::String(s))) => report.violations.push(format!(
                "[[verb]] '{name}'.confirm '{s}' is not 'operator' — a write verb's only gate mode \
                 is operator-confirm (PLUGINS.en.md §Write verbs)"
            )),
            (true, Some(_)) => report.violations.push(format!(
                "[[verb]] '{name}'.confirm has wrong type — expected the string \"operator\""
            )),
            (true, None) => report.violations.push(format!(
                "[[verb]] '{name}' is a write but declares no confirm gate — write verbs MUST carry \
                 confirm = \"operator\" (PLUGINS.en.md §Write verbs)"
            )),
            (false, Some(_)) => report.warnings.push(format!(
                "[[verb]] '{name}' is a read (write = false) but declares a confirm gate — read \
                 verbs are free; the gate is redundant"
            )),
            // Read verb, no gate — correct and frictionless.
            (false, None) => {}
        }
    }
}

/// Validate the `auth.toml` credential CONTRACT shipped next to a `jira`-kind
/// plugin's manifest (BWOC-45). Source of truth: the jira-cloud-rest SPEC.md
/// §Authentication + the `auth.toml` header — the file declares the SHAPE only.
/// Real credentials resolve at runtime from `BWOC_JIRA_*` env (or a gitignored
/// secrets file) and MUST NEVER appear in this tracked file.
///
/// Two concerns, in order of severity:
///   1. SECURITY (fail-closed) — the `[jira.auth]` placeholders `email` /
///      `token` / `base_url` must be EMPTY strings. A non-empty value is a
///      committed credential, the single worst outcome this check exists to
///      prevent, so it is a hard violation. The value is NEVER echoed back.
///   2. SHAPE — `[jira.auth]` declares the three placeholder keys, and
///      `[jira.auth.env]` binds each to a non-empty `var`, so the runtime
///      resolution map is present and well-formed.
///
/// An absent `auth.toml` is not audited (the contract is validated only when
/// the file exists, per the BWOC-45 scope "when a jira/* plugin has auth.toml").
fn audit_jira_auth(plugin_dir: &Path, report: &mut AuditReport) {
    let auth_path = plugin_dir.join("auth.toml");
    let body = match fs::read_to_string(&auth_path) {
        Ok(s) => {
            report.passes.push("auth.toml present".to_string());
            s
        }
        // No auth.toml → nothing to validate here.
        Err(_) => return,
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report.passes.push("auth.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("auth.toml is not valid TOML: {e}"));
            return;
        }
    };

    // [jira.auth] — the placeholder contract table.
    let auth = match raw
        .get("jira")
        .and_then(|j| j.get("auth"))
        .and_then(|a| a.as_table())
    {
        Some(t) => {
            report.passes.push("[jira.auth] table present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[jira.auth] table missing — auth.toml must declare the credential contract"
                    .to_string(),
            );
            return;
        }
    };

    // Each of email/token/base_url: present, a string, and EMPTY. A non-empty
    // value is a committed secret — fail closed, and never echo the value.
    for field in &["email", "token", "base_url"] {
        match auth.get(*field) {
            Some(toml::Value::String(s)) if s.is_empty() => report
                .passes
                .push(format!("[jira.auth].{field} is an empty placeholder")),
            Some(toml::Value::String(_)) => report.violations.push(format!(
                "[jira.auth].{field} has a non-empty value — a credential MUST NOT be committed; \
                 leave it empty and set the matching BWOC_JIRA_* env var (value redacted)"
            )),
            Some(_) => report.violations.push(format!(
                "[jira.auth].{field} has wrong type — expected an (empty) string placeholder"
            )),
            None => report.violations.push(format!(
                "[jira.auth].{field} missing — required placeholder key"
            )),
        }
    }

    // [jira.auth.env] — the runtime env-var binding map. Each credential must
    // name the environment variable it resolves from.
    let env = match auth.get("env").and_then(|e| e.as_table()) {
        Some(t) => {
            report
                .passes
                .push("[jira.auth.env] binding map present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[jira.auth.env] binding map missing — auth.toml must declare how each \
                 credential resolves from the environment"
                    .to_string(),
            );
            return;
        }
    };
    for field in &["email", "token", "base_url"] {
        match env.get(*field).and_then(|v| v.as_table()) {
            Some(binding) => match binding.get("var").and_then(|v| v.as_str()) {
                Some(var) if !var.is_empty() => report
                    .passes
                    .push(format!("[jira.auth.env].{field} binds to ${var}")),
                _ => report.violations.push(format!(
                    "[jira.auth.env].{field} missing a non-empty 'var' — each binding must name \
                     its environment variable"
                )),
            },
            None => report.violations.push(format!(
                "[jira.auth.env].{field} missing or not a table — expected \
                 {{ var = \"BWOC_JIRA_…\", required = true }}"
            )),
        }
    }
}

/// Validate the `auth.toml` credential CONTRACT shipped next to a `figma`-kind
/// plugin's manifest (BWOC-65). Source of truth: the figma-rest SPEC.md
/// §Authentication + the `auth.toml` header — the file declares the credential
/// SHAPE only. The real personal access token (PAT) resolves at runtime from
/// `BWOC_FIGMA_TOKEN` env (or a gitignored secrets file) and MUST NEVER appear
/// in this tracked file.
///
/// Two concerns, in order of severity (mirrors `audit_jira_auth`, BWOC-45, and
/// `audit_workflow_auth`, BWOC-55):
///   1. SECURITY (fail-closed) — the `[figma.auth]` `token` placeholder must be
///      an EMPTY string. A non-empty value is a committed PAT, the single worst
///      outcome this check exists to prevent, so it is a hard violation. The
///      value is NEVER echoed back.
///   2. SHAPE — `[figma.auth.env].token` binds to a non-empty `var`, so the
///      runtime resolution map is present and well-formed.
///
/// The `[figma.auth.secrets_file]` and `[figma.auth.scopes]` sub-tables carry
/// only path / table / key / scope NAMES (never a credential value), so they are
/// not secret-leak surfaces and are not policed here. An absent `auth.toml` is
/// not audited (the contract is validated only when the file exists — same scope
/// as jira/gcloud).
fn audit_figma_auth(plugin_dir: &Path, report: &mut AuditReport) {
    let auth_path = plugin_dir.join("auth.toml");
    let body = match fs::read_to_string(&auth_path) {
        Ok(s) => {
            report.passes.push("auth.toml present".to_string());
            s
        }
        // No auth.toml → nothing to validate here.
        Err(_) => return,
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report.passes.push("auth.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("auth.toml is not valid TOML: {e}"));
            return;
        }
    };

    // [figma.auth] — the placeholder contract table.
    let auth = match raw
        .get("figma")
        .and_then(|f| f.get("auth"))
        .and_then(|a| a.as_table())
    {
        Some(t) => {
            report.passes.push("[figma.auth] table present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[figma.auth] table missing — auth.toml must declare the credential contract"
                    .to_string(),
            );
            return;
        }
    };

    // token: present, a string, and EMPTY. A non-empty value is a committed PAT —
    // fail closed, and never echo the value.
    match auth.get("token") {
        Some(toml::Value::String(s)) if s.is_empty() => report
            .passes
            .push("[figma.auth].token is an empty placeholder".to_string()),
        Some(toml::Value::String(_)) => report.violations.push(
            "[figma.auth].token has a non-empty value — a personal access token MUST NOT be \
             committed; leave it empty and set BWOC_FIGMA_TOKEN (value redacted)"
                .to_string(),
        ),
        Some(_) => report.violations.push(
            "[figma.auth].token has wrong type — expected an (empty) string placeholder"
                .to_string(),
        ),
        None => report
            .violations
            .push("[figma.auth].token missing — required placeholder key".to_string()),
    }

    // [figma.auth.env].token — the runtime env-var binding. The PAT must name the
    // environment variable it resolves from.
    let env = match auth.get("env").and_then(|e| e.as_table()) {
        Some(t) => {
            report
                .passes
                .push("[figma.auth.env] binding map present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[figma.auth.env] binding map missing — auth.toml must declare how the token \
                 resolves from the environment"
                    .to_string(),
            );
            return;
        }
    };
    match env.get("token").and_then(|v| v.as_table()) {
        Some(binding) => match binding.get("var").and_then(|v| v.as_str()) {
            Some(var) if !var.is_empty() => report
                .passes
                .push(format!("[figma.auth.env].token binds to ${var}")),
            _ => report.violations.push(
                "[figma.auth.env].token missing a non-empty 'var' — the binding must name its \
                 environment variable"
                    .to_string(),
            ),
        },
        None => report.violations.push(
            "[figma.auth.env].token missing or not a table — expected \
             { var = \"BWOC_FIGMA_TOKEN\", required = true, secret = true }"
                .to_string(),
        ),
    }
}

/// Validate any plugin-local Figma asset mappings against the Figma Asset
/// Mapping Schema (BWOC-65). The `figma` kind is read-mostly: the `fetch` /
/// `tokens` / `export` verbs emit asset entries at runtime — they are never
/// persisted as tracked plugin files (the only on-disk artifact is the
/// content-addressable image cache under `figma/exports/`, which is gitignored).
/// So like council's `records/` fallback (BWOC-60), an optional plugin-local
/// `mappings/` directory holds captured `bwoc figma` output for hand-invocation
/// / smoke tests; when present every `*.json` in it is validated, when absent
/// (the shipped reference plugin ships none) there is nothing to audit.
///
/// Each file may be a captured verb envelope — `{ "assets": [ … ] }` (fetch /
/// tokens) or `{ "asset": { … } }` (export) — a bare array of entries, or a
/// single entry; every entry it carries is validated against the schema.
fn audit_figma_assets(plugin_dir: &Path, report: &mut AuditReport) {
    let mappings_dir = plugin_dir.join("mappings");
    let read = match fs::read_dir(&mappings_dir) {
        Ok(r) => r,
        // No plugin-local mappings — asset entries are emitted at runtime.
        Err(_) => return,
    };
    let mut json_paths: Vec<std::path::PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    json_paths.sort();
    for path in json_paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                report
                    .violations
                    .push(format!("mappings/{name} unreadable: {e}"));
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                report
                    .violations
                    .push(format!("mappings/{name} is not valid JSON: {e}"));
                continue;
            }
        };
        // Unwrap the verb envelope to the asset entries it carries: a `fetch` /
        // `tokens` body nests them under `assets`, an `export` body under `asset`;
        // a bare array or single object is taken as the entry set directly.
        let entries: Vec<&serde_json::Value> =
            if let Some(arr) = value.get("assets").and_then(|a| a.as_array()) {
                arr.iter().collect()
            } else if let Some(asset) = value.get("asset") {
                vec![asset]
            } else if let Some(arr) = value.as_array() {
                arr.iter().collect()
            } else {
                vec![&value]
            };
        for (i, entry) in entries.iter().enumerate() {
            let label = format!("mappings/{name}[{i}]");
            validate_figma_asset(&label, entry, report);
        }
    }
}

/// Validate one asset entry against the Figma Asset Mapping Schema (PLUGINS.en.md
/// §"Figma Asset Mapping Schema", BWOC-62). `label` identifies the entry in
/// messages. The schema is the `figma` kind's contract — the reference plugin's
/// verbs and the `bwoc figma` CLI emit entries of this shape, and `bwoc check`
/// validates it (BWOC-65). `file_key` + `node_id` is the stable key; the other
/// required fields are mutable projections of Figma state. Optional fields are
/// validated only when present and MUST be omitted (never serialized as `null`)
/// when absent — per the schema's omit-don't-null convention. Additive fields
/// beyond the schema's named set are ignored, not rejected.
fn validate_figma_asset(label: &str, value: &serde_json::Value, report: &mut AuditReport) {
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            report.violations.push(format!(
                "asset {label} is not a JSON object — expected a Figma Asset Mapping entry"
            ));
            return;
        }
    };

    // Required non-empty string fields. `file_key` + `node_id` are the stable
    // key; `name` / `type` / `last_modified` are required projections of Figma
    // state (`last_modified` is the export cache-invalidation signal).
    for field in &["file_key", "node_id", "name", "type", "last_modified"] {
        match obj.get(*field) {
            Some(serde_json::Value::String(s)) if !s.is_empty() => {
                report.passes.push(format!("asset {label} {field} present"))
            }
            Some(serde_json::Value::String(_)) => report.violations.push(format!(
                "asset {label} '{field}' is empty — required non-empty string"
            )),
            _ => report.violations.push(format!(
                "asset {label} missing required '{field}' (non-empty string)"
            )),
        }
    }

    // Optional string fields: validated only when present; an explicit null is a
    // violation (the schema omits absent fields, never serializes them as null).
    for field in &["exported_path", "image_url"] {
        match obj.get(*field) {
            None => {}
            Some(serde_json::Value::String(s)) if !s.is_empty() => report
                .passes
                .push(format!("asset {label} {field} well-formed")),
            Some(serde_json::Value::Null) => report.violations.push(format!(
                "asset {label} '{field}' is null — optional fields MUST be omitted, not null"
            )),
            Some(_) => report.violations.push(format!(
                "asset {label} '{field}' has wrong type — expected a non-empty string when present"
            )),
        }
    }

    // design_tokens: optional `{ name: value }` object; never null. Each value is
    // a scalar token value (the SPEC's color / spacing / type extraction yields
    // string or number scalars) — a nested object or array is not a token value.
    match obj.get("design_tokens") {
        None => {}
        Some(serde_json::Value::Object(tokens)) => {
            if tokens
                .values()
                .all(|v| v.is_string() || v.is_number() || v.is_boolean())
            {
                report
                    .passes
                    .push(format!("asset {label} design_tokens is a scalar map"));
            } else {
                report.violations.push(format!(
                    "asset {label} design_tokens values must be scalar token values \
                     (string / number), not nested objects or arrays"
                ));
            }
        }
        Some(serde_json::Value::Null) => report.violations.push(format!(
            "asset {label} 'design_tokens' is null — optional fields MUST be omitted, not null"
        )),
        Some(_) => report.violations.push(format!(
            "asset {label} 'design_tokens' has wrong type — expected a {{ name: value }} object"
        )),
    }
}

/// Validate the `auth.toml` credential CONTRACT shipped next to a `gws`-kind
/// plugin's manifest (BWOC-77). Source of truth: the gws-auth SPEC.md
/// §Authentication + the shipped `auth.toml` header — the file declares the
/// credential SHAPE only. The real OAuth2 access token resolves at runtime from
/// `BWOC_GWS_TOKEN` env (or a gitignored, owner-only `.bwoc/secrets/gws-token.json`)
/// and MUST NEVER appear in this tracked file.
///
/// Only the `gws-auth` foundation ships an `auth.toml`; the drive/gmail/calendar
/// siblings source the token from it and carry no `auth.toml` of their own, so an
/// absent file is not audited (same scope as jira/gcloud/figma).
///
/// Two concerns, in order of severity (mirrors `audit_figma_auth`, BWOC-65):
///   1. SECURITY (fail-closed) — the `[gws.auth]` `token` placeholder must be an
///      EMPTY string. A non-empty value is a committed OAuth token, the single
///      worst outcome this check exists to prevent, so it is a hard violation.
///      The value is NEVER echoed back.
///   2. SHAPE — `[gws.auth.env].token` binds to a non-empty `var`, so the runtime
///      resolution map is present and well-formed.
///
/// The `[gws.auth.secrets_file]` and `[gws.auth.scopes]` sub-tables carry only
/// path / field / scope NAMES (never a credential value), so they are not
/// secret-leak surfaces and are not policed here.
fn audit_gws_auth(plugin_dir: &Path, report: &mut AuditReport) {
    let auth_path = plugin_dir.join("auth.toml");
    let body = match fs::read_to_string(&auth_path) {
        Ok(s) => {
            report.passes.push("auth.toml present".to_string());
            s
        }
        // No auth.toml → nothing to validate here (the credential-less siblings).
        Err(_) => return,
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report.passes.push("auth.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("auth.toml is not valid TOML: {e}"));
            return;
        }
    };

    // [gws.auth] — the placeholder contract table.
    let auth = match raw
        .get("gws")
        .and_then(|g| g.get("auth"))
        .and_then(|a| a.as_table())
    {
        Some(t) => {
            report.passes.push("[gws.auth] table present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[gws.auth] table missing — auth.toml must declare the credential contract"
                    .to_string(),
            );
            return;
        }
    };

    // token: present, a string, and EMPTY. A non-empty value is a committed OAuth
    // token — fail closed, and never echo the value.
    match auth.get("token") {
        Some(toml::Value::String(s)) if s.is_empty() => report
            .passes
            .push("[gws.auth].token is an empty placeholder".to_string()),
        Some(toml::Value::String(_)) => report.violations.push(
            "[gws.auth].token has a non-empty value — an OAuth token MUST NOT be committed; \
             leave it empty and set BWOC_GWS_TOKEN (value redacted)"
                .to_string(),
        ),
        Some(_) => report.violations.push(
            "[gws.auth].token has wrong type — expected an (empty) string placeholder".to_string(),
        ),
        None => report
            .violations
            .push("[gws.auth].token missing — required placeholder key".to_string()),
    }

    // [gws.auth.env].token — the runtime env-var binding. The token must name the
    // environment variable it resolves from.
    let env = match auth.get("env").and_then(|e| e.as_table()) {
        Some(t) => {
            report
                .passes
                .push("[gws.auth.env] binding map present".to_string());
            t
        }
        None => {
            report.violations.push(
                "[gws.auth.env] binding map missing — auth.toml must declare how the token \
                 resolves from the environment"
                    .to_string(),
            );
            return;
        }
    };
    match env.get("token").and_then(|v| v.as_table()) {
        Some(binding) => match binding.get("var").and_then(|v| v.as_str()) {
            Some(var) if !var.is_empty() => report
                .passes
                .push(format!("[gws.auth.env].token binds to ${var}")),
            _ => report.violations.push(
                "[gws.auth.env].token missing a non-empty 'var' — the binding must name its \
                 environment variable"
                    .to_string(),
            ),
        },
        None => report.violations.push(
            "[gws.auth.env].token missing or not a table — expected \
             { var = \"BWOC_GWS_TOKEN\", required = true, secret = true }"
                .to_string(),
        ),
    }
}

/// The three Google Workspace services a `gws` plugin can surface, each with its
/// own normative resource shape under the Workspace Resource Schema (PLUGINS.en.md).
#[derive(Clone, Copy)]
enum GwsService {
    Drive,
    Gmail,
    Calendar,
}

impl GwsService {
    /// Resolve the resource shape a gws plugin emits from its directory basename.
    /// The reference plugins are 1:1 with a Google service; `gws-auth` is the
    /// credential foundation and emits only `status` metadata (no resource entry),
    /// so it — and any gws plugin not bound to a known service — has no shape.
    fn from_plugin_name(name: &str) -> Option<Self> {
        match name {
            "gws-drive" => Some(Self::Drive),
            "gws-gmail" => Some(Self::Gmail),
            "gws-calendar" => Some(Self::Calendar),
            _ => None,
        }
    }

    /// The envelope key under which the `list` / `search` / `events` verbs nest
    /// their resource-entry array.
    fn array_key(self) -> &'static str {
        match self {
            Self::Drive => "files",
            Self::Gmail => "threads",
            Self::Calendar => "events",
        }
    }
}

/// Validate any plugin-local captured Workspace resource entries against the
/// Workspace Resource Schema (BWOC-77). The `gws` kind is read-mostly: the
/// `list` / `get` / `search` / `show` / `events` verbs emit resource entries at
/// runtime — they are never persisted as tracked plugin files. So like figma's
/// `mappings/` (BWOC-65) and council's `records/` (BWOC-60), an optional
/// plugin-local `resources/` directory holds captured `bwoc gws` output for
/// hand-invocation / smoke tests; when present every `*.json` in it is validated,
/// when absent (the shipped reference plugins ship none) there is nothing to
/// audit.
///
/// The shape to validate against is fixed by the plugin's service: a file under
/// `gws-drive/resources/` is a Drive file, `gws-gmail/` a Gmail thread,
/// `gws-calendar/` a Calendar event. `gws-auth` emits no resource entry, so its
/// `resources/` (if any) is not policed. Only the resource-bearing verbs' output
/// belongs here — the `labels` / `calendars` list verbs emit label / calendar
/// objects that are not part of the schema.
///
/// Each file may be a captured verb envelope — `{ "files": [ … ] }` (list),
/// `{ "file": { … } }` (get) and likewise for threads/events, the `show` /
/// `get` envelope that spreads a single entry into itself, a bare array of
/// entries, or a single entry; every entry it carries is validated.
fn audit_gws_resources(plugin_dir: &Path, report: &mut AuditReport) {
    let service = match plugin_dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(GwsService::from_plugin_name)
    {
        Some(s) => s,
        // gws-auth (credential foundation) or any gws plugin not bound to a known
        // service emits no Workspace resource entry — nothing to validate here.
        None => return,
    };

    let resources_dir = plugin_dir.join("resources");
    let read = match fs::read_dir(&resources_dir) {
        Ok(r) => r,
        // No plugin-local captures — resource entries are emitted at runtime.
        Err(_) => return,
    };
    let mut json_paths: Vec<std::path::PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    json_paths.sort();
    for path in json_paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                report
                    .violations
                    .push(format!("resources/{name} unreadable: {e}"));
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                report
                    .violations
                    .push(format!("resources/{name} is not valid JSON: {e}"));
                continue;
            }
        };
        // Unwrap the verb envelope to the resource entries it carries: a
        // list / search / events body nests them under the service array key; a
        // get / show body spreads a single entry into the envelope (the extra
        // envelope fields are additive and ignored by the per-field validator); a
        // bare array or single object is taken directly.
        let entries: Vec<&serde_json::Value> =
            if let Some(arr) = value.get(service.array_key()).and_then(|a| a.as_array()) {
                arr.iter().collect()
            } else if let Some(arr) = value.as_array() {
                arr.iter().collect()
            } else {
                vec![&value]
            };
        for (i, entry) in entries.iter().enumerate() {
            let label = format!("resources/{name}[{i}]");
            validate_gws_resource(&label, service, entry, report);
        }
    }
}

/// Validate one resource entry against its service's shape in the Workspace
/// Resource Schema (PLUGINS.en.md §"Workspace Resource Schema", BWOC-73). `label`
/// identifies the entry in messages. Each shape has a stable id key plus mutable
/// projections (all required, non-empty strings) and a few optional fields.
/// Optional fields are validated only when present and MUST be omitted (never
/// serialized as `null`) when absent — per the schema's omit-don't-null
/// convention. Additive fields beyond the schema's named set (e.g. the `ok` /
/// `plugin` / `operation` envelope keys on a spread entry) are ignored, not
/// rejected.
fn validate_gws_resource(
    label: &str,
    service: GwsService,
    value: &serde_json::Value,
    report: &mut AuditReport,
) {
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            report.violations.push(format!(
                "resource {label} is not a JSON object — expected a Workspace resource entry"
            ));
            return;
        }
    };

    // Per-service field sets: required non-empty strings, optional strings,
    // optional string-arrays, optional numbers.
    let (required, opt_strings, opt_string_arrays, opt_numbers): (
        &[&str],
        &[&str],
        &[&str],
        &[&str],
    ) = match service {
        GwsService::Drive => (
            &["file_id", "name", "mime_type", "modified_time"],
            &["web_view_link"],
            &["owners"],
            &[],
        ),
        GwsService::Gmail => (
            &["thread_id", "subject", "from", "last_message_time"],
            &["snippet"],
            &["labels"],
            &[],
        ),
        GwsService::Calendar => (
            &["event_id", "calendar_id", "summary", "start", "end"],
            &[],
            &[],
            &["attendees_count"],
        ),
    };

    // Required non-empty string fields — the stable key + the mutable projections.
    for field in required {
        match obj.get(*field) {
            Some(serde_json::Value::String(s)) if !s.is_empty() => report
                .passes
                .push(format!("resource {label} {field} present")),
            Some(serde_json::Value::String(_)) => report.violations.push(format!(
                "resource {label} '{field}' is empty — required non-empty string"
            )),
            _ => report.violations.push(format!(
                "resource {label} missing required '{field}' (non-empty string)"
            )),
        }
    }

    // Optional string fields: validated only when present; explicit null is a
    // violation (the schema omits absent fields, never serializes them as null).
    for field in opt_strings {
        match obj.get(*field) {
            None => {}
            Some(serde_json::Value::String(s)) if !s.is_empty() => report
                .passes
                .push(format!("resource {label} {field} well-formed")),
            Some(serde_json::Value::Null) => report.violations.push(format!(
                "resource {label} '{field}' is null — optional fields MUST be omitted, not null"
            )),
            Some(_) => report.violations.push(format!(
                "resource {label} '{field}' has wrong type — expected a non-empty string when present"
            )),
        }
    }

    // Optional string-array fields (owners / labels): each element a non-empty
    // string.
    for field in opt_string_arrays {
        match obj.get(*field) {
            None => {}
            Some(serde_json::Value::Array(arr)) => {
                if arr
                    .iter()
                    .all(|v| matches!(v, serde_json::Value::String(s) if !s.is_empty()))
                {
                    report
                        .passes
                        .push(format!("resource {label} {field} well-formed"));
                } else {
                    report.violations.push(format!(
                        "resource {label} '{field}' must be an array of non-empty strings"
                    ));
                }
            }
            Some(serde_json::Value::Null) => report.violations.push(format!(
                "resource {label} '{field}' is null — optional fields MUST be omitted, not null"
            )),
            Some(_) => report.violations.push(format!(
                "resource {label} '{field}' has wrong type — expected an array of strings when present"
            )),
        }
    }

    // Optional number fields (attendees_count): a number when present.
    for field in opt_numbers {
        match obj.get(*field) {
            None => {}
            Some(serde_json::Value::Number(_)) => report
                .passes
                .push(format!("resource {label} {field} well-formed")),
            Some(serde_json::Value::Null) => report.violations.push(format!(
                "resource {label} '{field}' is null — optional fields MUST be omitted, not null"
            )),
            Some(_) => report.violations.push(format!(
                "resource {label} '{field}' has wrong type — expected a number when present"
            )),
        }
    }
}

/// Validate the `criteria.toml` declaration that ships next to an
/// audit-kind plugin's manifest. Source of truth: PLUGINS.en.md
/// §"Audit Findings Schema" — `criterion_id` is kebab-case and
/// plugin-scoped, `severity` is the closed `{info, low, medium,
/// high, critical}` enum. Per-criterion findings are appended to the
/// plugin's existing report so each audit-kind plugin still produces
/// exactly one row in the fleet output (consistent with BWOC-8).
fn audit_audit_criteria(plugin_dir: &Path, report: &mut AuditReport) {
    let criteria_path = plugin_dir.join("criteria.toml");
    let body = match fs::read_to_string(&criteria_path) {
        Ok(s) => {
            report.passes.push("criteria.toml present".to_string());
            s
        }
        Err(e) => {
            report
                .violations
                .push(format!("criteria.toml missing or unreadable: {e}"));
            return;
        }
    };
    let raw: toml::Value = match toml::from_str(&body) {
        Ok(v) => {
            report
                .passes
                .push("criteria.toml is valid TOML".to_string());
            v
        }
        Err(e) => {
            report
                .violations
                .push(format!("criteria.toml is not valid TOML: {e}"));
            return;
        }
    };

    let criterion_table = match raw.get("criterion").and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            report.violations.push(
                "criteria.toml declares no [criterion.*] entries — audit-kind plugins must declare at least one"
                    .to_string(),
            );
            return;
        }
    };
    if criterion_table.is_empty() {
        report.violations.push(
            "criteria.toml [criterion] table is empty — at least one [criterion.<id>] required"
                .to_string(),
        );
        return;
    }
    report.passes.push(format!(
        "criteria.toml declares {} criterion entries",
        criterion_table.len()
    ));

    for (id, entry) in criterion_table.iter() {
        // criterion_id is the table key; check kebab-case shape.
        if is_kebab_case(id) {
            report
                .passes
                .push(format!("criterion '{id}' id is kebab-case"));
        } else {
            report.violations.push(format!(
                "criterion '{id}' id is not kebab-case (lowercase ASCII letters/digits separated by single '-')"
            ));
        }

        // severity field — required, closed enum.
        let table = match entry.as_table() {
            Some(t) => t,
            None => {
                report.violations.push(format!(
                    "criterion '{id}' is not a table — expected [criterion.{id}] = {{ severity = ..., ... }}"
                ));
                continue;
            }
        };
        match table.get("severity") {
            Some(toml::Value::String(s)) => {
                if AUDIT_SEVERITY_LEVELS.contains(&s.as_str()) {
                    report
                        .passes
                        .push(format!("criterion '{id}' severity '{s}' in supported set"));
                } else {
                    report.violations.push(format!(
                        "criterion '{id}' severity '{s}' not in {{info, low, medium, high, critical}}"
                    ));
                }
            }
            Some(_) => report.violations.push(format!(
                "criterion '{id}' severity has wrong type — expected string"
            )),
            None => report.violations.push(format!(
                "criterion '{id}' missing required 'severity' field"
            )),
        }

        // expected_evidence_kind (BWOC-29) — optional. When declared,
        // verify the enum value and (for attestation / sample) check the
        // per-kind required-sub-fields contract.
        check_expected_evidence_kind(id, table, report);
    }
}

/// Validate a criterion's optional `expected_evidence_kind` field plus its
/// per-kind `required` sub-fields contract. Absent field = silent pass; the
/// runtime is free to choose at invoke time. Source of truth:
/// PLUGINS.en.md §"Evidence kinds" + notes/2026-05-27_check-evidence-kinds-extension.md.
fn check_expected_evidence_kind(
    id: &str,
    table: &toml::map::Map<String, toml::Value>,
    report: &mut AuditReport,
) {
    let kind = match table.get("expected_evidence_kind") {
        Some(toml::Value::String(s)) => s.as_str(),
        Some(_) => {
            report.violations.push(format!(
                "criterion '{id}' expected_evidence_kind has wrong type — expected string"
            ));
            return;
        }
        None => return, // optional field — absence is fine.
    };
    if !EVIDENCE_KINDS.contains(&kind) {
        report.violations.push(format!(
            "criterion '{id}' expected_evidence_kind '{kind}' not in \
             {{file, content, command, attestation, sample, none}}"
        ));
        return;
    }
    report.passes.push(format!(
        "criterion '{id}' expected_evidence_kind '{kind}' in supported set"
    ));

    match kind {
        "attestation" => {
            check_required_sub_fields(
                id,
                table,
                "attestation",
                ATTESTATION_FIELDS,
                ATTESTATION_FLOOR,
                report,
            );
        }
        "sample" => {
            check_required_sub_fields(id, table, "sample", SAMPLE_FIELDS, SAMPLE_FLOOR, report);
        }
        // file / content / command / none have no spec-mandated sub-fields.
        _ => {}
    }
}

/// Validate the `[criterion.<id>.<kind>] required = [...]` subtable. Three
/// gates: the subtable exists; `required` is an array of strings; the array
/// satisfies the per-kind spec floor and contains only valid sub-field names.
fn check_required_sub_fields(
    id: &str,
    table: &toml::map::Map<String, toml::Value>,
    kind: &str,
    valid_fields: &[&str],
    floor: &[&str],
    report: &mut AuditReport,
) {
    let subtable = match table.get(kind).and_then(|v| v.as_table()) {
        Some(t) => t,
        None => {
            report.violations.push(format!(
                "criterion '{id}' expected_evidence_kind='{kind}' but no \
                 [criterion.{id}.{kind}] subtable declaring required sub-fields"
            ));
            return;
        }
    };
    let required = match subtable.get("required") {
        Some(toml::Value::Array(arr)) => arr,
        Some(_) => {
            report.violations.push(format!(
                "criterion '{id}' {kind}.required has wrong type — expected array of strings"
            ));
            return;
        }
        None => {
            report.violations.push(format!(
                "criterion '{id}' [criterion.{id}.{kind}] missing 'required' field — \
                 declare which sub-fields the runtime must emit"
            ));
            return;
        }
    };

    let mut names: Vec<&str> = Vec::with_capacity(required.len());
    let mut all_strings = true;
    for v in required {
        match v.as_str() {
            Some(s) => names.push(s),
            None => {
                all_strings = false;
                break;
            }
        }
    }
    if !all_strings {
        report.violations.push(format!(
            "criterion '{id}' {kind}.required contains non-string entries — \
             expected array of sub-field names"
        ));
        return;
    }

    // Spec floor — every minimum field per kind must be in `required`.
    let mut floor_ok = true;
    for &must in floor {
        if !names.contains(&must) {
            report.violations.push(format!(
                "criterion '{id}' {kind}.required must include '{must}' (spec floor)"
            ));
            floor_ok = false;
        }
    }

    // Unknown sub-field names — anything outside the valid set is a violation.
    let mut unknown_ok = true;
    for name in &names {
        if !valid_fields.contains(name) {
            report.violations.push(format!(
                "criterion '{id}' {kind}.required contains unknown sub-field '{name}' — \
                 valid: {}",
                valid_fields.join(", ")
            ));
            unknown_ok = false;
        }
    }

    if floor_ok && unknown_ok {
        report.passes.push(format!(
            "criterion '{id}' {kind}.required satisfies spec floor ({} field(s))",
            names.len()
        ));
    }
}

/// Kebab-case predicate: lowercase ASCII letters/digits separated by
/// single '-'. Must start and end with an alphanumeric character; no
/// consecutive hyphens. Empty strings are rejected. Mirrors the
/// PLUGINS.en.md and SKILLS.en.md grammar for plugin/skill/criterion ids.
fn is_kebab_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    let valid_char = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !valid_char(bytes[0]) || !valid_char(bytes[bytes.len() - 1]) {
        return false;
    }
    let mut prev_hyphen = false;
    for &b in bytes {
        if b == b'-' {
            if prev_hyphen {
                return false;
            }
            prev_hyphen = true;
        } else if valid_char(b) {
            prev_hyphen = false;
        } else {
            return false;
        }
    }
    true
}

/// Recursively scan every string value in `raw`, refusing any that name a
/// declared backend or hardcoded model. Skill manifests have no exempt
/// field — Samānattatā is total at this layer.
fn check_manifest_neutrality_skill(raw: &toml::Value, report: &mut AuditReport) {
    let mut findings: Vec<String> = Vec::new();
    walk_strings(raw, "", &mut |path, val| {
        for vendor in BACKEND_NAMES {
            if contains_word(val, vendor) {
                findings.push(format!(
                    "{path} contains backend name '{vendor}' — skills must be backend-neutral"
                ));
            }
        }
        for model in HARDCODED_MODELS {
            if val.to_lowercase().contains(model) {
                findings.push(format!("{path} contains hardcoded model id '{model}'"));
            }
        }
    });
    if findings.is_empty() {
        report
            .passes
            .push("manifest values are backend-neutral".to_string());
    } else {
        report.violations.extend(findings);
    }
}

/// Same as the skill check, with one exemption: `[plugin].description` is
/// the only manifest value where a vendor name is tolerated (the description
/// often names the integration target). Everywhere else still rejects.
fn check_manifest_neutrality_plugin(raw: &toml::Value, report: &mut AuditReport) {
    let mut findings: Vec<String> = Vec::new();
    walk_strings(raw, "", &mut |path, val| {
        if path == "[plugin].description" {
            return;
        }
        for vendor in BACKEND_NAMES {
            if contains_word(val, vendor) {
                findings.push(format!(
                    "{path} contains backend name '{vendor}' — only [plugin].description may name a vendor"
                ));
            }
        }
        for model in HARDCODED_MODELS {
            if val.to_lowercase().contains(model) {
                findings.push(format!("{path} contains hardcoded model id '{model}'"));
            }
        }
    });
    if findings.is_empty() {
        report
            .passes
            .push("manifest values are backend-neutral (description exempt)".to_string());
    } else {
        report.violations.extend(findings);
    }
}

/// Walk every string leaf in a TOML value, invoking `visit(path, value)` per
/// leaf. `path` is a dotted breadcrumb like `[skill].description` or
/// `[contract].exposes[0]`, sufficient to localize a finding.
fn walk_strings<F: FnMut(&str, &str)>(value: &toml::Value, path: &str, visit: &mut F) {
    match value {
        toml::Value::String(s) => visit(path, s),
        toml::Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let sub = format!("{path}[{i}]");
                walk_strings(v, &sub, visit);
            }
        }
        toml::Value::Table(t) => {
            for (k, v) in t.iter() {
                let sub = if path.is_empty() {
                    format!("[{k}]")
                } else {
                    format!("{path}.{k}")
                };
                walk_strings(v, &sub, visit);
            }
        }
        _ => {}
    }
}

/// Word-boundary substring match (case-insensitive). Avoids the false
/// positives a raw `lower.contains("kimi")` would emit on otherwise-fine
/// strings like "Kimimaro" or "claudette" (none in practice today, but
/// the neutrality rule is brittle without it). Word boundary = anything
/// that is NOT an ASCII alphanumeric or `_`.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let lower = haystack.to_lowercase();
    let n = needle.to_lowercase();
    let mut start = 0;
    while let Some(idx) = lower[start..].find(&n) {
        let abs = start + idx;
        let before_ok = abs == 0
            || !lower
                .as_bytes()
                .get(abs - 1)
                .map(|b| (*b as char).is_ascii_alphanumeric() || *b == b'_')
                .unwrap_or(false);
        let after = abs + n.len();
        let after_ok = after >= lower.len()
            || !lower
                .as_bytes()
                .get(after)
                .map(|b| (*b as char).is_ascii_alphanumeric() || *b == b'_')
                .unwrap_or(false);
        if before_ok && after_ok {
            return true;
        }
        start = abs + n.len();
    }
    false
}

/// Discover every installed skill manifest under `<root>/modules/skills/*/manifest.toml`.
/// Returns the per-skill directory paths sorted by directory name. Missing
/// `modules/skills/` dir is not an error — workspaces may have no skills yet.
pub fn discover_skill_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    discover_module_dirs(root, "modules/skills")
}

/// Discover every installed plugin manifest under `<root>/modules/plugins/*/manifest.toml`.
pub fn discover_plugin_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    discover_module_dirs(root, "modules/plugins")
}

/// Discover module dirs under `<root>/<sub>` across BOTH layouts:
///   - flat            `<sub>/<name>/manifest.toml`
///   - kind-namespaced `<sub>/<kind>/<name>/manifest.toml`
///
/// A directory with no `manifest.toml` of its own is treated as a kind-group
/// (e.g. `modules/plugins/workflow/`, which holds the `gcloud-*` plugins shipped
/// by BWOC-53) and descended exactly ONE level. A real module dir always owns a
/// manifest, so we never recurse into one — and one level matches the only
/// namespacing the framework uses (mirrors `bwoc gcloud`'s two-layout discovery
/// in gcloud.rs::candidate_plugin_dirs).
fn discover_module_dirs(root: &Path, sub: &str) -> Vec<std::path::PathBuf> {
    let dir = root.join(sub);
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for path in read.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        if path.join("manifest.toml").is_file() {
            out.push(path);
        } else if let Ok(inner) = fs::read_dir(&path) {
            // Kind-group dir → collect its manifest-bearing children only.
            out.extend(
                inner
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && p.join("manifest.toml").is_file()),
            );
        }
    }
    out.sort();
    out
}

/// Local ancestor-walk helper (kept here to avoid pulling in
/// workspace.rs::find_workspace_root which threads Fluent bundles).
fn find_workspace_root_local() -> Option<std::path::PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikilink_detection() {
        assert!(contains_wikilink("see [[neutrality|Neutrality]]"));
        assert!(contains_wikilink("[[plain]]"));
        assert!(!contains_wikilink("plain markdown link [text](url)"));
        assert!(!contains_wikilink("[[ on one line\n]] on another"));
        assert!(!contains_wikilink(""));
    }

    #[test]
    fn audit_missing_directory_reports_violation() {
        let report = audit(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("AGENTS.md not found"))
        );
    }

    // ---- Dual-mode detection ------------------------------------------------

    #[test]
    fn detect_mode_template_for_placeholder_name() {
        let m = serde_json::json!({ "name": "{{name}}" });
        assert_eq!(detect_mode(Some(&m)), AuditMode::Template);
    }

    #[test]
    fn detect_mode_incarnation_for_real_name() {
        let m = serde_json::json!({ "name": "pi" });
        assert_eq!(detect_mode(Some(&m)), AuditMode::Incarnation);
    }

    #[test]
    fn detect_mode_template_for_missing_manifest() {
        // Safer default: half-built agents (no manifest yet) read as
        // template, not as a broken incarnation.
        assert_eq!(detect_mode(None), AuditMode::Template);
    }

    #[test]
    fn detect_mode_template_for_empty_name() {
        let m = serde_json::json!({ "name": "" });
        assert_eq!(detect_mode(Some(&m)), AuditMode::Template);
    }

    // ---- Placeholder extraction ---------------------------------------------

    #[test]
    fn extract_placeholders_finds_each_unique() {
        let content = "Hello {{agentId}}, role: {{agentRole}}. Again: {{agentId}}.";
        let found = extract_placeholders(content);
        assert_eq!(found, vec!["{{agentId}}", "{{agentRole}}"]);
    }

    #[test]
    fn extract_placeholders_ignores_non_identifier_content() {
        // `{{ ... }}` with spaces or punctuation between is not an
        // identifier — skip it. Catches false-positives in code blocks.
        let content = "{{ not an id }} but {{realOne}} yes";
        let found = extract_placeholders(content);
        assert_eq!(found, vec!["{{realOne}}"]);
    }

    #[test]
    fn extract_placeholders_handles_empty_string() {
        assert!(extract_placeholders("").is_empty());
    }

    #[test]
    fn extract_placeholders_handles_no_close() {
        // Unclosed `{{` — return cleanly, don't loop.
        assert!(extract_placeholders("ragged {{open").is_empty());
    }

    // ---- Incarnation-mode audit end-to-end ----------------------------------
    // Unix-only: exercises real backend symlinks → AGENTS.md. Windows symlink
    // support is deferred to Phase 2 (see new.rs::create_symlinks), so these
    // end-to-end audits don't apply there.

    #[cfg(unix)]
    fn write_temp_agent(label: &str, manifest_name: &str, agents_body: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("bwoc-check-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("AGENTS.md"), agents_body).unwrap();
        for backend in ["AGY.md", "CODEX.md", "KIMI.md", "CLAUDE.md", "OLLAMA.md"] {
            std::os::unix::fs::symlink("AGENTS.md", root.join(backend)).unwrap();
        }
        let manifest = serde_json::json!({
            "name": manifest_name,
            "agentId": format!("agent-{manifest_name}"),
            "agentRole": "demo",
            "primaryModel": "m",
            "memoryPath": "memories/",
            "lintCmd": "true",
            "formatCmd": "true",
            "testCmd": "true",
            "buildCmd": "true",
            "version": "2.0",
        });
        fs::write(
            root.join("config.manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        root
    }

    #[cfg(unix)]
    #[test]
    fn incarnated_with_unsubstituted_placeholder_fails() {
        let root = write_temp_agent(
            "unsub",
            "alpha",
            "You are {{agentId}}. The role is {{agentRole}}.",
        );
        let report = audit(&root);
        let has_violation = report.violations.iter().any(|v| {
            v.contains("unsubstituted placeholder")
                && (v.contains("{{agentId}}") || v.contains("{{agentRole}}"))
        });
        assert!(
            has_violation,
            "expected incarnation-mode violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn incarnated_clean_doc_passes() {
        let root = write_temp_agent(
            "clean",
            "alpha",
            "You are agent-alpha. Use {{taskId}} per task. No other placeholders.",
        );
        let report = audit(&root);
        // taskId is whitelisted as runtime — the only remaining check should pass.
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("no unsubstituted placeholders")),
            "expected the no-placeholders pass line, got passes: {:?}",
            report.passes
        );
        // No placeholder-related violations.
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.contains("unsubstituted")),
            "got unexpected violations: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(&root);
    }

    // ---- Skill + plugin manifest audits (BWOC-8) ----------------------------

    fn write_skill_manifest(label: &str, name: &str, body: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bwoc-skill-{label}-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("modules/skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.toml"), body).unwrap();
        dir
    }

    fn write_plugin_manifest(label: &str, name: &str, body: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bwoc-plugin-{label}-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("modules/plugins").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.toml"), body).unwrap();
        dir
    }

    #[test]
    fn audit_skill_manifest_reference_passes() {
        // The reference manifest from modules/skills/worktree-discipline/.
        let dir = write_skill_manifest(
            "ref",
            "worktree-discipline",
            r#"[skill]
name        = "worktree-discipline"
version     = "0.1.0"
description = "Create, isolate, and cleanup task worktrees per Anattā."
maturity    = "L1"

[contract]
requires    = []
exposes     = ["claim_task", "release_task"]

[gates]
verify      = "bwoc skill verify worktree-discipline"
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected reference skill manifest to pass, got: {:?}",
            report.violations
        );
        assert!(report.passes.iter().any(|p| p.contains("non-empty")));
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_skill_manifest_empty_exposes_fails() {
        let dir = write_skill_manifest(
            "empty-exposes",
            "no-ops",
            r#"[skill]
name        = "no-ops"
version     = "0.1.0"
description = "Skill that exposes nothing."
maturity    = "L1"

[contract]
exposes     = []
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[contract].exposes is empty")),
            "expected non-empty-exposes violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_skill_manifest_missing_required_field_fails() {
        let dir = write_skill_manifest(
            "missing-version",
            "broken",
            r#"[skill]
name        = "broken"
description = "Missing version."
maturity    = "L1"

[contract]
exposes     = ["op"]
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[skill].version missing")),
            "expected missing-version violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_skill_manifest_name_mismatch_fails() {
        let dir = write_skill_manifest(
            "mismatch",
            "directory-name",
            r#"[skill]
name        = "different-name"
version     = "0.1.0"
description = "Name mismatch."
maturity    = "L1"

[contract]
exposes     = ["op"]
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("does not match directory")),
            "expected name-mismatch violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_skill_manifest_bad_maturity_fails() {
        let dir = write_skill_manifest(
            "bad-maturity",
            "wrong-level",
            r#"[skill]
name        = "wrong-level"
version     = "0.1.0"
description = "Bad maturity."
maturity    = "L9"

[contract]
exposes     = ["op"]
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("maturity 'L9' not in L1..L7")),
            "expected bad-maturity violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_skill_manifest_vendor_name_fails() {
        let dir = write_skill_manifest(
            "vendor",
            "claude-only",
            r#"[skill]
name        = "claude-only"
version     = "0.1.0"
description = "Skill that names Claude — should fail neutrality."
maturity    = "L1"

[contract]
exposes     = ["op"]
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("backend name 'claude'")),
            "expected vendor-name violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_reference_passes() {
        // The reference manifest from modules/plugins/memory-tier2-noop/.
        let dir = write_plugin_manifest(
            "ref",
            "memory-tier2-noop",
            r#"[plugin]
name        = "memory-tier2-noop"
kind        = "memory-backend"
version     = "0.1.0"
description = "No-op Tier 2 memory backend that forwards to Tier 1."
compat      = ">=2.5.0"
entry       = "bwoc-plugin-memory-tier2-noop"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected reference plugin manifest to pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_bad_kind_fails() {
        let dir = write_plugin_manifest(
            "bad-kind",
            "weird-kind",
            r#"[plugin]
name        = "weird-kind"
kind        = "frobnicator"
version     = "0.1.0"
description = "Plugin with unknown kind."
compat      = ">=2.5.0"
entry       = "bin"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("kind 'frobnicator' not in")),
            "expected bad-kind violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_audit_kind_accepted() {
        // EPIC-2 forward-compat: 'audit' kind must be accepted today.
        // Per BWOC-17, audit-kind plugins also need a sibling criteria.toml,
        // so this fixture provides a minimal valid one.
        let dir = write_plugin_manifest(
            "audit-kind",
            "iso-29110",
            r#"[plugin]
name        = "iso-29110"
kind        = "audit"
version     = "0.1.0"
description = "ISO/IEC 29110 compliance audit."
compat      = ">=2.5.0"
entry       = "bwoc-plugin-iso-29110"
"#,
        );
        fs::write(
            dir.join("criteria.toml"),
            r#"[criterion.iso-29110-smoke]
severity    = "info"
description = "Minimal smoke criterion for the test fixture."
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected 'audit' kind to be accepted, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_jira_kind_accepted() {
        // BWOC-43: the 'jira' kind is declared in the PLUGINS.en.md enum
        // (BWOC-41); the validator must accept it so the reference jira plugin
        // passes its own `bwoc check`. 'jira' takes no criteria.toml (that is
        // audit-kind-specific), so the bare manifest must pass clean.
        let dir = write_plugin_manifest(
            "jira-kind",
            "jira-cloud-rest",
            r#"[plugin]
name        = "jira-cloud-rest"
kind        = "jira"
version     = "0.1.0"
description = "Jira Cloud REST v3 integration adapter."
compat      = ">=2.7.0"
entry       = "jira.sh"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected 'jira' kind to be accepted, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_council_kind_accepted() {
        // BWOC-59: the 'council' kind is declared in the PLUGINS.en.md enum
        // (BWOC-57); the validator must accept it so the reference
        // council-sangha-7 plugin passes its own `bwoc check`. BWOC-60 added the
        // deep council validation (the [council] table + the decisions.toml
        // templates), so a clean pass now requires both — this fixture supplies a
        // well-formed [council] table and decisions.toml. The granular
        // [council]/templates/Decision-Schema checks have their own tests below.
        let dir = write_plugin_manifest(
            "council-kind",
            "council-sangha-7",
            r#"[plugin]
name        = "council-sangha-7"
kind        = "council"
version     = "0.1.0"
description = "Aparihaniya-dhamma 7 consensus council."
compat      = ">=2.9.0"
entry       = "protocol.sh"

[council]
voting_model = "sangha"
quorum       = "2/3"
"#,
        );
        fs::write(dir.join("decisions.toml"), COUNCIL_TEMPLATES_OK).unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected 'council' kind to be accepted, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_figma_kind_accepted() {
        // BWOC-64: the 'figma' kind (the eighth) is declared in the
        // PLUGINS.en.md enum (BWOC-62); the validator must accept it so the
        // reference figma-rest plugin passes its own `bwoc check`. This fixture
        // ships no auth.toml / mappings, so the BWOC-65 figma audit is a no-op
        // here, and the optional [config.schema] table is ignored, not rejected —
        // the manifest passes clean.
        let dir = write_plugin_manifest(
            "figma-kind",
            "figma-rest",
            r#"[plugin]
name        = "figma-rest"
kind        = "figma"
version     = "0.1.0"
description = "Read-mostly Figma REST adapter."
compat      = ">=2.10.0"
entry       = "figma.sh"

[config.schema]
export_dir = { type = "string", required = false, default = "figma/exports" }
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected 'figma' kind to be accepted, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_real_figma_reference_passes() {
        // End-to-end (BWOC-65): audit the actual shipped reference plugin
        // (modules/plugins/figma/figma-rest/) — manifest + auth.toml secret-leak
        // guard — exactly as `bwoc check --all` does in an operator workspace.
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/plugins/figma/figma-rest");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the plugin — nothing to assert.
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real figma-rest manifest + auth.toml must pass bwoc check, got: {:?}",
            report.violations
        );
        // Confirm the figma auth guard actually ran (not silently skipped) — the
        // shipped auth.toml carries an empty token placeholder + env binding.
        assert!(
            report
                .passes
                .iter()
                .any(|p| p == "[figma.auth].token is an empty placeholder"),
            "expected the figma auth.toml token placeholder to be validated, got: {:?}",
            report.passes
        );
    }

    // ---- BWOC-65: figma auth.toml + Asset Mapping Schema -------------------

    /// Body of a minimal valid figma-kind plugin manifest, reused by the
    /// auth.toml / mappings fixtures below.
    const FIGMA_MANIFEST: &str = r#"[plugin]
name        = "figma-rest"
kind        = "figma"
version     = "0.1.0"
description = "Read-mostly Figma REST adapter."
compat      = ">=2.10.0"
entry       = "figma.sh"
"#;

    /// The shipped auth.toml contract shape: an EMPTY token placeholder plus the
    /// env binding map. Mirrors modules/plugins/figma/figma-rest/auth.toml.
    const FIGMA_AUTH_OK: &str = r#"[figma.auth]
token = ""

[figma.auth.env]
token = { var = "BWOC_FIGMA_TOKEN", required = true, secret = true }

[figma.auth.secrets_file]
path  = ".bwoc/secrets.toml"
table = "figma"
key   = "token"

[figma.auth.scopes]
required = ["file_content"]
optional = ["library_content"]
"#;

    #[test]
    fn audit_figma_auth_empty_placeholder_passes() {
        // A figma plugin whose auth.toml holds only an EMPTY token placeholder
        // plus a complete env binding map is the shipped contract shape — and the
        // secrets_file / scopes sub-tables (path / table / key / scope NAMES, not
        // values) must NOT be policed as leaks. Passes clean.
        let dir = write_plugin_manifest("figma-auth-ok", "figma-rest", FIGMA_MANIFEST);
        fs::write(dir.join("auth.toml"), FIGMA_AUTH_OK).unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "empty-placeholder figma auth.toml must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_figma_auth_real_token_fails_and_redacts() {
        // SECURITY: a committed PAT is the worst outcome this check exists to
        // prevent. A non-empty token MUST be a hard violation, and the leaked
        // value MUST NOT be echoed into the report (that would re-leak the secret
        // into whatever consumes `bwoc check` output).
        let leaked = "figd_super-secret-personal-access-token";
        let dir = write_plugin_manifest("figma-auth-leak", "figma-rest", FIGMA_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            format!(
                r#"[figma.auth]
token = "{leaked}"

[figma.auth.env]
token = {{ var = "BWOC_FIGMA_TOKEN", required = true, secret = true }}
"#
            ),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[figma.auth].token") && v.contains("MUST NOT be committed")),
            "a committed token must be a violation, got: {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().all(|v| !v.contains(leaked)),
            "the secret value must be redacted from the report, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_figma_auth_missing_env_binding_fails() {
        // The token placeholder is empty (good) but the [figma.auth.env] binding
        // map is absent — the runtime resolution contract is incomplete.
        let dir = write_plugin_manifest("figma-auth-noenv", "figma-rest", FIGMA_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            r#"[figma.auth]
token = ""
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[figma.auth.env] binding map missing")),
            "a missing env binding map must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    /// A schema-conforming asset entry (the PLUGINS.en.md §Figma Asset Mapping
    /// Schema example), with every optional field populated.
    fn figma_asset_ok() -> serde_json::Value {
        serde_json::json!({
            "file_key": "AbC123dEf456",
            "node_id": "12:345",
            "name": "Primary Button",
            "type": "COMPONENT",
            "last_modified": "2026-05-27T09:00:00Z",
            "exported_path": "figma/exports/9f86d081884c7d65.png",
            "image_url": "https://figma-alpha-api.s3.amazonaws.com/render/9f86.png",
            "design_tokens": { "color/primary": "#2D7FF9", "radius/sm": "4px" }
        })
    }

    fn figma_asset_violations(value: &serde_json::Value) -> Vec<String> {
        let mut report = AuditReport {
            target: "test".to_string(),
            passes: Vec::new(),
            warnings: Vec::new(),
            violations: Vec::new(),
        };
        validate_figma_asset("asset.json", value, &mut report);
        report.violations
    }

    #[test]
    fn figma_asset_well_formed_passes() {
        assert!(
            figma_asset_violations(&figma_asset_ok()).is_empty(),
            "a schema-conforming asset entry must pass"
        );
    }

    #[test]
    fn figma_asset_minimal_required_only_passes() {
        // Optional fields omitted (never-exported, no tokens) — the entry is still
        // schema-conforming on its required-field floor.
        let entry = serde_json::json!({
            "file_key": "AbC123",
            "node_id": "1:2",
            "name": "Frame",
            "type": "FRAME",
            "last_modified": "2026-05-27T09:00:00Z"
        });
        assert!(
            figma_asset_violations(&entry).is_empty(),
            "a required-only asset entry must pass"
        );
    }

    #[test]
    fn figma_asset_missing_required_field_fails() {
        let mut entry = figma_asset_ok();
        entry.as_object_mut().unwrap().remove("node_id");
        assert!(
            figma_asset_violations(&entry)
                .iter()
                .any(|v| v.contains("missing required 'node_id'")),
            "a missing stable-key field must fail"
        );
    }

    #[test]
    fn figma_asset_empty_required_field_fails() {
        let mut entry = figma_asset_ok();
        entry.as_object_mut().unwrap()["file_key"] = serde_json::json!("");
        assert!(
            figma_asset_violations(&entry)
                .iter()
                .any(|v| v.contains("'file_key' is empty")),
            "an empty stable-key field must fail"
        );
    }

    #[test]
    fn figma_asset_null_optional_field_fails() {
        // The schema omits absent optional fields — it never serializes them as
        // null. An explicit null is a violation.
        let mut entry = figma_asset_ok();
        entry.as_object_mut().unwrap()["exported_path"] = serde_json::Value::Null;
        assert!(
            figma_asset_violations(&entry)
                .iter()
                .any(|v| v.contains("'exported_path' is null") && v.contains("MUST be omitted")),
            "a null optional field must fail"
        );
    }

    #[test]
    fn figma_asset_nested_design_tokens_fails() {
        // design_tokens is a flat { name: value } scalar map — a nested object as
        // a value is not a token value.
        let mut entry = figma_asset_ok();
        entry.as_object_mut().unwrap()["design_tokens"] =
            serde_json::json!({ "color": { "primary": "#2D7FF9" } });
        assert!(
            figma_asset_violations(&entry)
                .iter()
                .any(|v| v.contains("design_tokens values must be scalar")),
            "a nested design_tokens value must fail"
        );
    }

    #[test]
    fn audit_figma_assets_fetch_envelope_validates_entries() {
        // A captured `fetch` body nests entries under `assets`; a malformed entry
        // in the array must surface as a violation when the plugin is audited.
        let dir = write_plugin_manifest("figma-map-fetch", "figma-rest", FIGMA_MANIFEST);
        fs::write(dir.join("auth.toml"), FIGMA_AUTH_OK).unwrap();
        fs::create_dir_all(dir.join("mappings")).unwrap();
        let bad = serde_json::json!({
            "ok": true,
            "file_key": "AbC123",
            "assets": [
                figma_asset_ok(),
                { "file_key": "AbC123", "name": "No Node Id", "type": "FRAME",
                  "last_modified": "2026-05-27T09:00:00Z" }
            ]
        });
        fs::write(
            dir.join("mappings/fetch.json"),
            serde_json::to_string_pretty(&bad).unwrap(),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("mappings/fetch.json[1]") && v.contains("'node_id'")),
            "a malformed entry in the fetch envelope must be flagged, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_figma_assets_export_envelope_passes() {
        // A captured `export` body nests one entry under `asset`; a well-formed
        // entry passes clean.
        let dir = write_plugin_manifest("figma-map-export", "figma-rest", FIGMA_MANIFEST);
        fs::write(dir.join("auth.toml"), FIGMA_AUTH_OK).unwrap();
        fs::create_dir_all(dir.join("mappings")).unwrap();
        let body = serde_json::json!({ "ok": true, "cached": true, "asset": figma_asset_ok() });
        fs::write(
            dir.join("mappings/export.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a well-formed export envelope must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- BWOC-77: gws auth.toml + Workspace Resource Schema ----------------

    /// Body of a minimal valid gws-kind plugin manifest, reused by the auth.toml
    /// / resources fixtures below.
    const GWS_MANIFEST: &str = r#"[plugin]
name        = "gws-auth"
kind        = "gws"
version     = "0.1.0"
description = "Google Workspace OAuth2 credential foundation."
compat      = ">=2.10.0"
entry       = "gws.sh"
"#;

    /// A minimal valid gws-kind plugin manifest for `name`, so a resources/
    /// fixture written into the `name` directory satisfies the base name-matches-
    /// directory check.
    fn gws_manifest(name: &str) -> String {
        format!(
            r#"[plugin]
name        = "{name}"
kind        = "gws"
version     = "0.1.0"
description = "Read-mostly Google Workspace adapter."
compat      = ">=2.10.0"
entry       = "gws.sh"
"#
        )
    }

    /// The shipped auth.toml contract shape: an EMPTY token placeholder plus the
    /// env binding map and the (non-secret) secrets_file / scopes sub-tables.
    /// Mirrors modules/plugins/gws/gws-auth/auth.toml.
    const GWS_AUTH_OK: &str = r#"[gws.auth]
token = ""

[gws.auth.env]
token = { var = "BWOC_GWS_TOKEN", required = true, secret = true }

[gws.auth.secrets_file]
path = ".bwoc/secrets/gws-token.json"
fields = ["access_token", "refresh_token"]

[gws.auth.scopes]
drive    = ["https://www.googleapis.com/auth/drive.readonly"]
gmail    = ["https://www.googleapis.com/auth/gmail.readonly"]
calendar = ["https://www.googleapis.com/auth/calendar.readonly"]
"#;

    #[test]
    fn audit_plugin_manifest_gws_kind_accepted() {
        // The 'gws' kind (the ninth) is declared in the PLUGINS.en.md enum
        // (BWOC-73); the validator must accept it so the reference gws plugins
        // pass their own `bwoc check`. This fixture ships no auth.toml / resources,
        // so the BWOC-77 gws audit is a no-op here — the manifest passes clean.
        let dir = write_plugin_manifest(
            "gws-kind",
            "gws-drive",
            r#"[plugin]
name        = "gws-drive"
kind        = "gws"
version     = "0.1.0"
description = "Read-mostly Google Drive adapter."
compat      = ">=2.10.0"
entry       = "gws.sh"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected 'gws' kind to be accepted, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_real_gws_references_pass() {
        // End-to-end (BWOC-77): audit the actual shipped reference plugins
        // (modules/plugins/gws/{gws-auth,gws-drive,gws-gmail,gws-calendar}/) —
        // manifest + auth.toml secret-leak guard + resource schema — exactly as
        // `bwoc check --all` does in an operator workspace.
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/plugins/gws");
        for name in &["gws-auth", "gws-drive", "gws-gmail", "gws-calendar"] {
            let dir = base.join(name);
            if !dir.join("manifest.toml").is_file() {
                continue; // partial checkout without the plugin — nothing to assert.
            }
            let report = audit_plugin_manifest(&dir);
            assert!(
                report.violations.is_empty(),
                "real {name} manifest must pass bwoc check, got: {:?}",
                report.violations
            );
        }
        // Confirm the gws auth guard actually ran on the foundation (not silently
        // skipped) — only gws-auth ships an auth.toml with the empty placeholder.
        let auth_dir = base.join("gws-auth");
        if auth_dir.join("auth.toml").is_file() {
            let report = audit_plugin_manifest(&auth_dir);
            assert!(
                report
                    .passes
                    .iter()
                    .any(|p| p == "[gws.auth].token is an empty placeholder"),
                "expected the gws auth.toml token placeholder to be validated, got: {:?}",
                report.passes
            );
        }
    }

    #[test]
    fn audit_gws_auth_empty_placeholder_passes() {
        // A gws plugin whose auth.toml holds only an EMPTY token placeholder plus
        // the env binding map is the shipped contract shape — and the secrets_file
        // / scopes sub-tables (path / field / scope NAMES, not values) must NOT be
        // policed as leaks. Passes clean.
        let dir = write_plugin_manifest("gws-auth-ok", "gws-auth", GWS_MANIFEST);
        fs::write(dir.join("auth.toml"), GWS_AUTH_OK).unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "empty-placeholder gws auth.toml must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_gws_auth_real_token_fails_and_redacts() {
        // SECURITY: a committed OAuth token is the worst outcome this check exists
        // to prevent. A non-empty token MUST be a hard violation, and the leaked
        // value MUST NOT be echoed into the report (that would re-leak the secret
        // into whatever consumes `bwoc check` output).
        let leaked = "ya29.super-secret-google-oauth-access-token";
        let dir = write_plugin_manifest("gws-auth-leak", "gws-auth", GWS_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            format!(
                r#"[gws.auth]
token = "{leaked}"

[gws.auth.env]
token = {{ var = "BWOC_GWS_TOKEN", required = true, secret = true }}
"#
            ),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[gws.auth].token") && v.contains("MUST NOT be committed")),
            "a committed token must be a violation, got: {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().all(|v| !v.contains(leaked)),
            "the secret value must be redacted from the report, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_gws_auth_missing_env_binding_fails() {
        // The token placeholder is empty (good) but the [gws.auth.env] binding map
        // is absent — the runtime resolution contract is incomplete.
        let dir = write_plugin_manifest("gws-auth-noenv", "gws-auth", GWS_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            r#"[gws.auth]
token = ""
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[gws.auth.env] binding map missing")),
            "a missing env binding map must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- BWOC-77: Workspace Resource Schema (Drive / Gmail / Calendar) ------

    /// A schema-conforming Drive file entry, with every optional field populated.
    fn gws_drive_ok() -> serde_json::Value {
        serde_json::json!({
            "file_id": "1AbC_dEfGhIjKlMnOpQrStUvWxYz",
            "name": "BWOC Architecture.gdoc",
            "mime_type": "application/vnd.google-apps.document",
            "modified_time": "2026-05-27T09:00:00Z",
            "owners": ["me@example.com"],
            "web_view_link": "https://docs.google.com/document/d/1AbC/edit"
        })
    }

    /// A schema-conforming Gmail thread entry, every optional field populated.
    fn gws_gmail_ok() -> serde_json::Value {
        serde_json::json!({
            "thread_id": "18ab12cd34ef5678",
            "subject": "Sprint 13 review",
            "from": "jisoo@example.com",
            "snippet": "Closing EPIC-13…",
            "labels": ["INBOX", "IMPORTANT"],
            "last_message_time": "2026-05-28T09:00:00Z"
        })
    }

    /// A schema-conforming Calendar event entry, every optional field populated.
    fn gws_calendar_ok() -> serde_json::Value {
        serde_json::json!({
            "event_id": "abc123def456",
            "calendar_id": "primary",
            "summary": "Sprint 13 review",
            "start": "2026-05-28T09:00:00Z",
            "end": "2026-05-28T10:00:00Z",
            "attendees_count": 4
        })
    }

    fn gws_resource_violations(service: GwsService, value: &serde_json::Value) -> Vec<String> {
        let mut report = AuditReport {
            target: "test".to_string(),
            passes: Vec::new(),
            warnings: Vec::new(),
            violations: Vec::new(),
        };
        validate_gws_resource("resource.json", service, value, &mut report);
        report.violations
    }

    #[test]
    fn gws_resources_well_formed_pass() {
        assert!(
            gws_resource_violations(GwsService::Drive, &gws_drive_ok()).is_empty(),
            "a schema-conforming Drive file must pass"
        );
        assert!(
            gws_resource_violations(GwsService::Gmail, &gws_gmail_ok()).is_empty(),
            "a schema-conforming Gmail thread must pass"
        );
        assert!(
            gws_resource_violations(GwsService::Calendar, &gws_calendar_ok()).is_empty(),
            "a schema-conforming Calendar event must pass"
        );
    }

    #[test]
    fn gws_resource_minimal_required_only_passes() {
        // Optional fields omitted across all three shapes — still schema-conforming
        // on the required-field floor.
        let drive = serde_json::json!({
            "file_id": "1A", "name": "f", "mime_type": "application/pdf",
            "modified_time": "2026-05-27T09:00:00Z"
        });
        let gmail = serde_json::json!({
            "thread_id": "18", "subject": "s", "from": "a@b.c",
            "last_message_time": "2026-05-28T09:00:00Z"
        });
        let cal = serde_json::json!({
            "event_id": "e", "calendar_id": "primary", "summary": "s",
            "start": "2026-06-01", "end": "2026-06-02"
        });
        assert!(gws_resource_violations(GwsService::Drive, &drive).is_empty());
        assert!(gws_resource_violations(GwsService::Gmail, &gmail).is_empty());
        assert!(gws_resource_violations(GwsService::Calendar, &cal).is_empty());
    }

    #[test]
    fn gws_resource_missing_required_field_fails() {
        let mut drive = gws_drive_ok();
        drive.as_object_mut().unwrap().remove("file_id");
        assert!(
            gws_resource_violations(GwsService::Drive, &drive)
                .iter()
                .any(|v| v.contains("missing required 'file_id'")),
            "a Drive file without the stable key must fail"
        );
    }

    #[test]
    fn gws_resource_empty_required_field_fails() {
        let mut gmail = gws_gmail_ok();
        gmail.as_object_mut().unwrap()["subject"] = serde_json::json!("");
        assert!(
            gws_resource_violations(GwsService::Gmail, &gmail)
                .iter()
                .any(|v| v.contains("'subject'") && v.contains("empty")),
            "an empty required string must fail"
        );
    }

    #[test]
    fn gws_resource_null_optional_field_fails() {
        // owners present-but-null violates the omit-don't-null convention.
        let mut drive = gws_drive_ok();
        drive.as_object_mut().unwrap()["owners"] = serde_json::Value::Null;
        assert!(
            gws_resource_violations(GwsService::Drive, &drive)
                .iter()
                .any(|v| v.contains("'owners'") && v.contains("null")),
            "a null optional field must fail"
        );
    }

    #[test]
    fn gws_resource_wrong_type_optional_fails() {
        // attendees_count must be a number, not a string.
        let mut cal = gws_calendar_ok();
        cal.as_object_mut().unwrap()["attendees_count"] = serde_json::json!("four");
        assert!(
            gws_resource_violations(GwsService::Calendar, &cal)
                .iter()
                .any(|v| v.contains("'attendees_count'") && v.contains("number")),
            "a non-number attendees_count must fail"
        );
        // labels must be an array of non-empty strings.
        let mut gmail = gws_gmail_ok();
        gmail.as_object_mut().unwrap()["labels"] = serde_json::json!(["INBOX", ""]);
        assert!(
            gws_resource_violations(GwsService::Gmail, &gmail)
                .iter()
                .any(|v| v.contains("'labels'") && v.contains("non-empty strings")),
            "a labels array with an empty element must fail"
        );
    }

    #[test]
    fn gws_resource_non_object_fails() {
        assert!(
            gws_resource_violations(GwsService::Drive, &serde_json::json!("not-an-object"))
                .iter()
                .any(|v| v.contains("not a JSON object")),
            "a non-object entry must fail"
        );
    }

    #[test]
    fn audit_gws_resources_list_envelope_validates_entries() {
        // A captured `list` body nests entries under the service array key
        // (`files`); every entry is unwrapped and validated.
        let dir = write_plugin_manifest("gws-res-list", "gws-drive", &gws_manifest("gws-drive"));
        fs::create_dir_all(dir.join("resources")).unwrap();
        let body = serde_json::json!({
            "ok": true, "plugin": "gws-drive", "operation": "list", "total": 1,
            "files": [ gws_drive_ok() ]
        });
        fs::write(
            dir.join("resources/list.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a well-formed list envelope must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_gws_resources_show_spread_envelope_passes() {
        // The Gmail `show` / Drive `get` envelope spreads a single entry into
        // itself alongside `ok` / `plugin` / `operation`; those additive envelope
        // keys are ignored and the spread entry validates clean.
        let dir = write_plugin_manifest("gws-res-show", "gws-gmail", &gws_manifest("gws-gmail"));
        fs::create_dir_all(dir.join("resources")).unwrap();
        let mut body = gws_gmail_ok();
        let obj = body.as_object_mut().unwrap();
        obj.insert("ok".to_string(), serde_json::json!(true));
        obj.insert("plugin".to_string(), serde_json::json!("gws-gmail"));
        obj.insert("operation".to_string(), serde_json::json!("show"));
        fs::write(
            dir.join("resources/show.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a well-formed show spread envelope must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_gws_resources_bad_entry_in_envelope_fails() {
        // A malformed entry inside a captured `events` envelope is surfaced with
        // its file + index label.
        let dir =
            write_plugin_manifest("gws-res-bad", "gws-calendar", &gws_manifest("gws-calendar"));
        fs::create_dir_all(dir.join("resources")).unwrap();
        let mut bad = gws_calendar_ok();
        bad.as_object_mut().unwrap().remove("event_id");
        let body = serde_json::json!({
            "ok": true, "operation": "events", "events": [ gws_calendar_ok(), bad ]
        });
        fs::write(
            dir.join("resources/events.json"),
            serde_json::to_string_pretty(&body).unwrap(),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("resources/events.json[1]") && v.contains("event_id")),
            "the malformed entry must be flagged by file + index, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_real_jira_reference_passes() {
        // End-to-end: audit the actual shipped reference plugin
        // (modules/plugins/jira-cloud-rest/), not a fixture — this is the file
        // `bwoc check --all` will validate in an operator workspace.
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/plugins/jira-cloud-rest");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the plugin — nothing to assert.
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real jira-cloud-rest manifest must pass bwoc check, got: {:?}",
            report.violations
        );
    }

    #[test]
    fn audit_skill_manifest_real_scrum_via_jira_reference_passes() {
        // End-to-end (BWOC-45): audit the actual shipped scrum-via-jira skill —
        // the framework's first skill-on-plugin dependency — exactly as
        // `bwoc check --all` does per-skill. Its requires_plugins = ["jira"]
        // must validate as a real kind enum and the manifest must pass clean.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/skills/scrum-via-jira");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the skill — nothing to assert.
        }
        let report = audit_skill_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real scrum-via-jira manifest must pass bwoc check, got: {:?}",
            report.violations
        );
    }

    // ---- BWOC-45: jira auth.toml contract validation -----------------------

    /// Body of a minimal valid jira-kind plugin manifest, reused by the
    /// auth.toml fixtures below.
    const JIRA_MANIFEST: &str = r#"[plugin]
name        = "jira-cloud-rest"
kind        = "jira"
version     = "0.1.0"
description = "Jira Cloud REST v3 integration adapter."
compat      = ">=2.7.0"
entry       = "jira.sh"
"#;

    #[test]
    fn audit_jira_auth_empty_placeholders_passes() {
        // A jira plugin whose auth.toml holds only EMPTY placeholders plus a
        // complete env binding map is the shipped contract shape — passes clean.
        let dir = write_plugin_manifest("jira-auth-ok", "jira-cloud-rest", JIRA_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            r#"[jira.auth]
email    = ""
token    = ""
base_url = ""

[jira.auth.env]
email    = { var = "BWOC_JIRA_EMAIL",    required = true }
token    = { var = "BWOC_JIRA_TOKEN",    required = true, secret = true }
base_url = { var = "BWOC_JIRA_BASE_URL", required = true }
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "empty-placeholder auth.toml must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_jira_auth_real_token_fails_and_redacts() {
        // SECURITY: a committed credential is the worst outcome this check
        // exists to prevent. A non-empty token MUST be a hard violation, and
        // the leaked value MUST NOT be echoed into the report (that would
        // re-leak the secret into whatever consumes `bwoc check` output).
        let leaked = "ATATT-super-secret-token-value";
        let dir = write_plugin_manifest("jira-auth-leak", "jira-cloud-rest", JIRA_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            format!(
                r#"[jira.auth]
email    = ""
token    = "{leaked}"
base_url = ""

[jira.auth.env]
email    = {{ var = "BWOC_JIRA_EMAIL",    required = true }}
token    = {{ var = "BWOC_JIRA_TOKEN",    required = true, secret = true }}
base_url = {{ var = "BWOC_JIRA_BASE_URL", required = true }}
"#
            ),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[jira.auth].token") && v.contains("MUST NOT be committed")),
            "a committed token must be a violation, got: {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().all(|v| !v.contains(leaked)),
            "the secret value must be redacted from the report, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_jira_auth_missing_env_binding_fails() {
        // The placeholders alone are not enough — the [jira.auth.env] map is
        // how each credential resolves at runtime. A missing map is a shape
        // violation, not a security leak.
        let dir = write_plugin_manifest("jira-auth-noenv", "jira-cloud-rest", JIRA_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            r#"[jira.auth]
email    = ""
token    = ""
base_url = ""
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[jira.auth.env] binding map missing")),
            "missing env binding map must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_skill_requires_plugins_valid_kind_passes() {
        // BWOC-45: `bwoc check` validates that requires_plugins names a valid
        // plugin KIND enum ("jira"). It does NOT require the plugin to be
        // enabled — that is a spawn-time / `bwoc skill verify` concern.
        let dir = write_skill_manifest(
            "reqplug-ok",
            "scrum-via-jira",
            r#"[skill]
name        = "scrum-via-jira"
version     = "0.1.0"
description = "Scrum operations over a jira-kind plugin."
maturity    = "L1"

[contract]
requires         = []
requires_plugins = ["jira"]
exposes          = ["propose-sprint"]
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "requires_plugins with a valid kind must pass, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("requires_plugins 'jira' is a valid plugin kind")),
            "expected a pass note for the valid kind, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_skill_requires_plugins_invalid_kind_fails() {
        let dir = write_skill_manifest(
            "reqplug-bad",
            "bogus-dep",
            r#"[skill]
name        = "bogus-dep"
version     = "0.1.0"
description = "Skill depending on a kind that does not exist."
maturity    = "L1"

[contract]
requires_plugins = ["not-a-real-kind"]
exposes          = ["do-thing"]
"#,
        );
        let report = audit_skill_manifest(&dir);
        assert!(
            report.violations.iter().any(
                |v| v.contains("requires_plugins 'not-a-real-kind' is not a valid plugin kind")
            ),
            "invalid requires_plugins kind must be a violation, got: {:?}",
            report.violations
        );
    }

    #[test]
    fn audit_plugin_manifest_vendor_in_description_allowed() {
        // Vendor names ARE allowed in description (per PLUGINS.en.md §"Neutrality").
        let dir = write_plugin_manifest(
            "vendor-desc",
            "kimi-bridge",
            r#"[plugin]
name        = "kimi-bridge"
kind        = "llm-backend"
version     = "0.1.0"
description = "Bridge to the kimi backend (vendor name allowed here only)."
compat      = ">=2.5.0"
entry       = "bin"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        // We still expect the name itself to trip neutrality (it contains 'kimi'),
        // but the description should not — so any violations should not name description.
        for v in &report.violations {
            assert!(
                !v.contains("[plugin].description contains"),
                "description should be exempt, got: {v}"
            );
        }
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_vendor_outside_description_fails() {
        let dir = write_plugin_manifest(
            "vendor-entry",
            "neutral-name",
            r#"[plugin]
name        = "neutral-name"
kind        = "llm-backend"
version     = "0.1.0"
description = "A plugin."
compat      = ">=2.5.0"
entry       = "claude-cli-wrapper"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[plugin].entry") && v.contains("'claude'")),
            "expected vendor-in-entry violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_missing_required_field_fails() {
        let dir = write_plugin_manifest(
            "no-compat",
            "broken-plugin",
            r#"[plugin]
name        = "broken-plugin"
kind        = "workflow"
version     = "0.1.0"
description = "Missing compat."
entry       = "bin"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[plugin].compat missing")),
            "expected missing-compat violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- Entry path-traversal guard (BWOC-36) ------------------------------

    // Build a fully-valid workflow manifest whose only variable is `entry`, so
    // a non-empty `violations` isolates the entry-guard verdict.
    fn write_entry_manifest(label: &str, name: &str, entry: &str) -> std::path::PathBuf {
        write_plugin_manifest(
            label,
            name,
            &format!(
                r#"[plugin]
name        = "{name}"
kind        = "workflow"
version     = "0.1.0"
description = "Path-traversal guard test."
compat      = ">=2.5.0"
entry       = "{entry}"
"#
            ),
        )
    }

    #[test]
    fn plugin_entry_bare_name_ok() {
        let dir = write_entry_manifest("entry-bare", "trav-bare", "audit.sh");
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected clean manifest, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("[plugin].entry is a contained path"))
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn plugin_entry_contained_relative_ok() {
        let dir = write_entry_manifest("entry-rel", "trav-rel", "bin/audit.sh");
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected clean manifest, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn plugin_entry_parent_escape_rejected() {
        let dir = write_entry_manifest("entry-esc", "trav-esc", "../escape");
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("../escape") && v.contains("..")),
            "expected traversal violation naming the entry, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn plugin_entry_absolute_rejected() {
        let dir = write_entry_manifest("entry-abs", "trav-abs", "/tmp/evil");
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("/tmp/evil") && v.contains("absolute")),
            "expected absolute-path violation naming the entry, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn plugin_entry_parent_anywhere_rejected() {
        let dir = write_entry_manifest("entry-mid", "trav-mid", "nested/../evil");
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.iter().any(|v| v.contains("'..'")),
            "expected '..'-component violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- Audit-kind criteria.toml validation (BWOC-17) ---------------------

    fn write_audit_plugin(
        label: &str,
        name: &str,
        manifest_body: &str,
        criteria_body: Option<&str>,
    ) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "bwoc-audit-{label}-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("modules/plugins").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.toml"), manifest_body).unwrap();
        if let Some(c) = criteria_body {
            fs::write(dir.join("criteria.toml"), c).unwrap();
        }
        dir
    }

    const AUDIT_MANIFEST_REF: &str = r#"[plugin]
name        = "audit-iso-ref"
kind        = "audit"
version     = "0.1.0"
description = "Reference audit plugin used in tests."
compat      = ">=2.5.0"
entry       = "audit.sh"
"#;

    #[test]
    fn audit_criteria_reference_passes() {
        let dir = write_audit_plugin(
            "ref",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-good-criterion]
severity    = "high"
description = "A valid criterion."

[criterion.ref-another-one]
severity    = "low"
description = "Second valid criterion."
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected reference audit plugin to pass, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("declares 2 criterion entries"))
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_missing_file_fails() {
        let dir = write_audit_plugin("no-file", "audit-iso-ref", AUDIT_MANIFEST_REF, None);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("criteria.toml missing")),
            "expected missing-criteria violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_invalid_toml_fails() {
        let dir = write_audit_plugin(
            "bad-toml",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some("this is not = valid [toml\n"),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("criteria.toml is not valid TOML")),
            "expected invalid-TOML violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_empty_table_fails() {
        let dir = write_audit_plugin(
            "no-entries",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some("# criteria file with no [criterion.*] entries\n"),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("declares no [criterion.*] entries")),
            "expected no-criteria violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_bad_severity_fails() {
        let dir = write_audit_plugin(
            "bad-severity",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-bad]
severity    = "warn"
description = "Severity outside the closed enum."
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("severity 'warn' not in {info, low, medium, high, critical}")),
            "expected bad-severity violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_missing_severity_fails() {
        let dir = write_audit_plugin(
            "no-severity",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-no-sev]
description = "Severity field omitted."
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("missing required 'severity' field")),
            "expected missing-severity violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_non_kebab_id_fails() {
        let dir = write_audit_plugin(
            "non-kebab",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            // Bare key cannot contain underscores per the standard;
            // use a quoted key for the non-kebab form.
            Some(
                r#"["criterion"."Ref_NonKebab"]
severity    = "high"
description = "criterion_id is not kebab-case."
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'Ref_NonKebab' id is not kebab-case")),
            "expected non-kebab violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- expected_evidence_kind validation (BWOC-29) -----------------------

    #[test]
    fn audit_criteria_evidence_kind_file_passes() {
        // kind=file has no spec-mandated sub-fields — no subtable required.
        let dir = write_audit_plugin(
            "kind-file",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-file-kind]
severity                = "medium"
expected_evidence_kind  = "file"
description             = "Criterion that points at a workspace file."
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected file-kind criterion to pass, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("expected_evidence_kind 'file' in supported set")),
            "expected pass line for file kind, got: {:?}",
            report.passes
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_content_passes() {
        let dir = write_audit_plugin(
            "kind-content",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-content-kind]
severity                = "medium"
expected_evidence_kind  = "content"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected content-kind criterion to pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_command_passes() {
        let dir = write_audit_plugin(
            "kind-command",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-command-kind]
severity                = "info"
expected_evidence_kind  = "command"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected command-kind criterion to pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_none_passes() {
        let dir = write_audit_plugin(
            "kind-none",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-none-kind]
severity                = "info"
expected_evidence_kind  = "none"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected none-kind criterion to pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_attestation_passes() {
        let dir = write_audit_plugin(
            "kind-attestation",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-attestation-kind]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-attestation-kind.attestation]
required = ["signer", "signed_at"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected attestation-kind criterion to pass, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("attestation.required satisfies spec floor")),
            "expected floor-satisfied pass line, got: {:?}",
            report.passes
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_sample_passes() {
        let dir = write_audit_plugin(
            "kind-sample",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-sample-kind]
severity                = "high"
expected_evidence_kind  = "sample"

[criterion.ref-sample-kind.sample]
required = ["sampled_count", "sampled_of"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected sample-kind criterion to pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_attestation_can_tighten_required() {
        // Operators MAY elevate optional spec fields (valid_through, as_of)
        // to required for a specific criterion — useful when a clause
        // demands explicit expiry tracking.
        let dir = write_audit_plugin(
            "tighten",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-tightened]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-tightened.attestation]
required = ["signer", "signed_at", "valid_through"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected tightened-required to pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_omitted_passes() {
        // Backward compat — criteria.toml without expected_evidence_kind
        // declared must keep working (audit-iso-29110, stubs).
        let dir = write_audit_plugin(
            "kind-omitted",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-no-kind]
severity    = "medium"
description = "No expected_evidence_kind declared — runtime free to choose."
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "expected no-kind criterion to pass, got: {:?}",
            report.violations
        );
        // And no expected_evidence_kind pass / fail lines either.
        assert!(
            !report
                .passes
                .iter()
                .any(|p| p.contains("expected_evidence_kind")),
            "should not emit any expected_evidence_kind line for omitted field, got: {:?}",
            report.passes
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_unknown_enum_fails() {
        let dir = write_audit_plugin(
            "kind-typo",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-bad-kind]
severity                = "high"
expected_evidence_kind  = "attestion"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("expected_evidence_kind 'attestion' not in")),
            "expected unknown-enum violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_wrong_type_fails() {
        let dir = write_audit_plugin(
            "kind-type",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-bad-type]
severity                = "high"
expected_evidence_kind  = 42
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("expected_evidence_kind has wrong type")),
            "expected wrong-type violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_attestation_missing_subtable_fails() {
        let dir = write_audit_plugin(
            "att-no-subtable",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-att-bare]
severity                = "high"
expected_evidence_kind  = "attestation"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.iter().any(|v| v.contains(
                "expected_evidence_kind='attestation' but no \
                 [criterion.ref-att-bare.attestation] subtable"
            )),
            "expected missing-subtable violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_attestation_missing_signer_fails() {
        let dir = write_audit_plugin(
            "att-no-signer",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-att-floor]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-att-floor.attestation]
required = ["signed_at"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("attestation.required must include 'signer' (spec floor)")),
            "expected missing-signer violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_attestation_missing_signed_at_fails() {
        let dir = write_audit_plugin(
            "att-no-signed-at",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-att-no-date]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-att-no-date.attestation]
required = ["signer"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("attestation.required must include 'signed_at' (spec floor)")),
            "expected missing-signed_at violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_attestation_unknown_subfield_fails() {
        let dir = write_audit_plugin(
            "att-unknown",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-att-unknown]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-att-unknown.attestation]
required = ["signer", "signed_at", "frobnicator"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.iter().any(
                |v| v.contains("attestation.required contains unknown sub-field 'frobnicator'")
            ),
            "expected unknown-subfield violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_sample_missing_subtable_fails() {
        let dir = write_audit_plugin(
            "sample-no-subtable",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-sample-bare]
severity                = "high"
expected_evidence_kind  = "sample"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.iter().any(|v| v.contains(
                "expected_evidence_kind='sample' but no \
                 [criterion.ref-sample-bare.sample] subtable"
            )),
            "expected missing-subtable violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_sample_missing_sampled_count_fails() {
        let dir = write_audit_plugin(
            "sample-no-count",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-sample-no-n]
severity                = "high"
expected_evidence_kind  = "sample"

[criterion.ref-sample-no-n.sample]
required = ["sampled_of"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("sample.required must include 'sampled_count' (spec floor)")),
            "expected missing-sampled_count violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_sample_missing_sampled_of_fails() {
        let dir = write_audit_plugin(
            "sample-no-of",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-sample-no-m]
severity                = "high"
expected_evidence_kind  = "sample"

[criterion.ref-sample-no-m.sample]
required = ["sampled_count"]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("sample.required must include 'sampled_of' (spec floor)")),
            "expected missing-sampled_of violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_required_wrong_type_fails() {
        let dir = write_audit_plugin(
            "required-not-array",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-required-string]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-required-string.attestation]
required = "signer,signed_at"
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v
                    .contains("attestation.required has wrong type — expected array of strings")),
            "expected wrong-type violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_required_missing_field_fails() {
        let dir = write_audit_plugin(
            "required-missing",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-no-required]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-no-required.attestation]
# subtable exists but `required` field is absent.
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v
                    .contains("[criterion.ref-no-required.attestation] missing 'required' field")),
            "expected missing-required violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_required_non_string_entry_fails() {
        let dir = write_audit_plugin(
            "required-int",
            "audit-iso-ref",
            AUDIT_MANIFEST_REF,
            Some(
                r#"[criterion.ref-required-int]
severity                = "high"
expected_evidence_kind  = "attestation"

[criterion.ref-required-int.attestation]
required = ["signer", 42]
"#,
            ),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("attestation.required contains non-string entries")),
            "expected non-string-entry violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_evidence_kind_skipped_for_non_audit_kind() {
        // Plugin with kind != "audit" should not trigger any
        // expected_evidence_kind checks even if criteria.toml were present.
        let dir = write_plugin_manifest(
            "non-audit-evidence",
            "memory-tier2-noop",
            r#"[plugin]
name        = "memory-tier2-noop"
kind        = "memory-backend"
version     = "0.1.0"
description = "Non-audit kind — evidence-kind checks must not fire."
compat      = ">=2.5.0"
entry       = "bin"
"#,
        );
        // Drop a criteria.toml alongside that WOULD violate the BWOC-29 check
        // if it ran — proves the check is gated on plugin kind.
        fs::write(
            dir.join("criteria.toml"),
            r#"[criterion.would-fail]
severity                = "high"
expected_evidence_kind  = "frobnicator"
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.contains("expected_evidence_kind")),
            "non-audit kind must not emit expected_evidence_kind violations, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_criteria_skipped_for_non_audit_kind() {
        // Plugin with kind != "audit" should NOT trigger the criteria check
        // — criteria.toml is an audit-kind-only contract.
        let dir = write_plugin_manifest(
            "non-audit",
            "memory-tier2-noop",
            r#"[plugin]
name        = "memory-tier2-noop"
kind        = "memory-backend"
version     = "0.1.0"
description = "Non-audit kind."
compat      = ">=2.5.0"
entry       = "bin"
"#,
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            !report.passes.iter().any(|p| p.contains("criteria.toml")),
            "non-audit kind should not emit criteria.toml checks, got: {:?}",
            report.passes
        );
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.contains("criteria.toml")),
            "non-audit kind should not emit criteria.toml violations, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn is_kebab_case_rules() {
        assert!(is_kebab_case("foo"));
        assert!(is_kebab_case("foo-bar"));
        assert!(is_kebab_case("foo-bar-baz"));
        assert!(is_kebab_case("iso-29110-bp-project-plan"));
        assert!(is_kebab_case("a1"));
        assert!(!is_kebab_case(""));
        assert!(!is_kebab_case("Foo")); // uppercase
        assert!(!is_kebab_case("foo_bar")); // underscore
        assert!(!is_kebab_case("-foo")); // leading hyphen
        assert!(!is_kebab_case("foo-")); // trailing hyphen
        assert!(!is_kebab_case("foo--bar")); // double hyphen
        assert!(!is_kebab_case("foo bar")); // space
        assert!(!is_kebab_case("foo.bar")); // dot
    }

    #[test]
    fn audit_skill_manifest_invalid_toml_fails() {
        let dir = write_skill_manifest("bad-toml", "broken", "this is not valid TOML = [\n");
        let report = audit_skill_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("not valid TOML")),
            "expected invalid-TOML violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn contains_word_does_not_match_substrings() {
        // Word-boundary match: 'claude' should NOT trip on 'claudette',
        // but should trip on 'claude' as a standalone token.
        assert!(contains_word("uses claude backend", "claude"));
        assert!(contains_word("claude", "claude"));
        assert!(contains_word("CLAUDE", "claude"));
        assert!(contains_word("with-claude-suffix", "claude")); // hyphen is a boundary
        assert!(!contains_word("claudette", "claude"));
        assert!(!contains_word("claude_marketing", "claude")); // underscore is NOT a boundary
    }

    #[test]
    fn discover_skill_dirs_handles_missing_modules() {
        let root = std::env::temp_dir().join(format!("bwoc-discover-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(discover_skill_dirs(&root).is_empty());
        assert!(discover_plugin_dirs(&root).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_module_dirs_skips_dirs_without_manifest() {
        let root = std::env::temp_dir().join(format!("bwoc-discover-mixed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let skills = root.join("modules/skills");
        fs::create_dir_all(skills.join("with-manifest")).unwrap();
        fs::create_dir_all(skills.join("without-manifest")).unwrap();
        fs::write(
            skills.join("with-manifest/manifest.toml"),
            "[skill]\nname = \"with-manifest\"\n",
        )
        .unwrap();
        let found = discover_skill_dirs(&root);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("with-manifest"));
        let _ = fs::remove_dir_all(&root);
    }

    // ---- BWOC-55: kind-namespaced (workflow/) plugin discovery -------------

    #[test]
    fn discover_plugin_dirs_finds_kind_namespaced_layout() {
        // BWOC-53 ships the gcloud plugins under modules/plugins/workflow/<name>/.
        // Discovery must descend ONE level into a manifest-less kind-group dir so
        // `bwoc check --all` audits them and the fleet tally grows.
        let root =
            std::env::temp_dir().join(format!("bwoc-discover-nested-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let plugins = root.join("modules/plugins");
        // A flat plugin alongside the kind-group.
        fs::create_dir_all(plugins.join("jira-cloud-rest")).unwrap();
        fs::write(
            plugins.join("jira-cloud-rest/manifest.toml"),
            "[plugin]\nname = \"jira-cloud-rest\"\n",
        )
        .unwrap();
        // Kind-namespaced plugins under workflow/ — the group dir has NO manifest.
        fs::create_dir_all(plugins.join("workflow/gcloud-auth")).unwrap();
        fs::create_dir_all(plugins.join("workflow/gcloud-project")).unwrap();
        fs::write(
            plugins.join("workflow/gcloud-auth/manifest.toml"),
            "[plugin]\nname = \"gcloud-auth\"\n",
        )
        .unwrap();
        fs::write(
            plugins.join("workflow/gcloud-project/manifest.toml"),
            "[plugin]\nname = \"gcloud-project\"\n",
        )
        .unwrap();
        let names: Vec<String> = discover_plugin_dirs(&root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains(&"jira-cloud-rest".to_string()),
            "flat layout must still resolve: {names:?}"
        );
        assert!(
            names.contains(&"gcloud-auth".to_string()),
            "kind-namespaced plugin not discovered: {names:?}"
        );
        assert!(
            names.contains(&"gcloud-project".to_string()),
            "kind-namespaced plugin not discovered: {names:?}"
        );
        assert!(
            !names.contains(&"workflow".to_string()),
            "the kind-group dir is not itself a plugin: {names:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn audit_plugin_manifest_real_gcloud_auth_reference_passes() {
        // End-to-end (BWOC-55): audit the actual shipped workflow/gcloud-auth
        // reference plugin — manifest + auth.toml — exactly as `bwoc check --all`
        // does in an operator workspace.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/plugins/workflow/gcloud-auth");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the plugin — nothing to assert.
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-auth manifest + auth.toml must pass bwoc check, got: {:?}",
            report.violations
        );
        // Confirm the workflow auth contract was actually exercised, not skipped.
        assert!(
            report.passes.iter().any(|p| p == "[sources] table present"),
            "expected the workflow auth.toml shape to be validated, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_plugin_manifest_real_gcloud_project_reference_passes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/plugins/workflow/gcloud-project");
        if !dir.join("manifest.toml").is_file() {
            return;
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-project manifest + auth.toml must pass bwoc check, got: {:?}",
            report.violations
        );
    }

    #[test]
    fn audit_plugin_manifest_real_gcloud_compute_reference_passes() {
        // End-to-end (BWOC-70): audit the actual shipped workflow/gcloud-compute
        // reference plugin — the first write-capable gcloud slice (EPIC-9) —
        // exactly as `bwoc check --all` does in an operator workspace.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/plugins/workflow/gcloud-compute");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the plugin — nothing to assert.
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-compute manifest must pass bwoc check, got: {:?}",
            report.violations
        );
        // The [[verb]] write-gate metadata is a fork-side enhancement (BWOC-69
        // detail) that upstream's #103 squash chose not to ship — the upstream
        // gcloud-compute manifest is intentionally minimal (no [[verb]] blocks).
        // Assert the richer verb-metadata passes only when the manifest actually
        // declares them, so this test passes on both manifest shapes.
        let has_verb_metadata = report
            .passes
            .iter()
            .any(|p| p.starts_with("[[verb]] write-gate metadata declared"));
        if has_verb_metadata {
            for verb in ["start", "stop"] {
                assert!(
                    report.passes.iter().any(|p| p
                        == &format!("[[verb]] '{verb}' write carries the operator-confirm gate")),
                    "expected write verb '{verb}' to carry the operator-confirm gate, got: {:?}",
                    report.passes
                );
            }
            assert!(
                report
                    .passes
                    .iter()
                    .any(|p| p == "[[verb]] 'list' declares write = false"),
                "expected read verb 'list' to declare write = false, got: {:?}",
                report.passes
            );
        }
    }

    #[test]
    fn audit_plugin_manifest_real_gcloud_storage_reference_passes() {
        // EPIC-10: the second write-capable slice (first irreversible write).
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/plugins/workflow/gcloud-storage");
        if !dir.join("manifest.toml").is_file() {
            return;
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-storage manifest + auth.toml must pass bwoc check, got: {:?}",
            report.violations
        );
        assert!(
            report.passes.iter().any(|p| p == "[sources] table present"),
            "expected the workflow auth.toml shape to be validated, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_plugin_manifest_real_gcloud_run_reference_passes() {
        // EPIC-11: the third write-capable slice (Cloud Run deploy, T2).
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/plugins/workflow/gcloud-run");
        if !dir.join("manifest.toml").is_file() {
            return;
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-run manifest + auth.toml must pass bwoc check, got: {:?}",
            report.violations
        );
        assert!(
            report.passes.iter().any(|p| p == "[sources] table present"),
            "expected the workflow auth.toml shape to be validated, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_plugin_manifest_real_gcloud_iam_reference_passes() {
        // EPIC-12: the fourth + LAST write-capable slice (IAM bindings, T4).
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/plugins/workflow/gcloud-iam");
        if !dir.join("manifest.toml").is_file() {
            return;
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-iam manifest + auth.toml must pass bwoc check, got: {:?}",
            report.violations
        );
        assert!(
            report.passes.iter().any(|p| p == "[sources] table present"),
            "expected the workflow auth.toml shape to be validated, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_skill_manifest_real_gcloud_ops_reference_passes() {
        // The gcloud-ops skill is the framework's first skill-on-MULTIPLE-plugins
        // (requires_plugins = ["workflow"], kind-level). Its manifest must pass
        // and the workflow kind must validate as a real enum.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/skills/gcloud-ops");
        if !dir.join("manifest.toml").is_file() {
            return;
        }
        let report = audit_skill_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real gcloud-ops manifest must pass bwoc check, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("requires_plugins 'workflow' is a valid plugin kind")),
            "expected requires_plugins=[workflow] to validate as a real kind, got: {:?}",
            report.passes
        );
    }

    // ---- BWOC-55: workflow auth.toml contract validation -------------------

    /// Minimal valid workflow-kind plugin manifest, reused by the auth.toml
    /// fixtures below.
    const WORKFLOW_MANIFEST: &str = r#"[plugin]
name        = "gcloud-auth"
kind        = "workflow"
version     = "0.1.0"
description = "gcloud credential-state adapter."
compat      = ">=2.9.0"
entry       = "gcloud.sh"
"#;

    /// The shipped [sources] shape — file paths + env-var NAMES, no values.
    const WORKFLOW_AUTH_OK: &str = r#"[sources]
adc             = { path = "~/.config/gcloud/application_default_credentials.json", priority = 1 }
service_account = { path = ".bwoc/secrets/gcloud-sa.json", priority = 2 }
env             = { vars = ["BWOC_GCLOUD_ACCOUNT", "BWOC_GCLOUD_PROJECT"], priority = 3 }
"#;

    #[test]
    fn audit_workflow_auth_shape_passes() {
        let dir = write_plugin_manifest("wf-auth-ok", "gcloud-auth", WORKFLOW_MANIFEST);
        fs::write(dir.join("auth.toml"), WORKFLOW_AUTH_OK).unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "well-formed [sources] auth.toml must pass, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_auth_inline_secret_fails_and_redacts() {
        // SECURITY: an inline credential value under a source is the worst
        // outcome this guard prevents. An undeclared key MUST be a hard
        // violation, and the value MUST NOT be echoed back into the report.
        let leaked = "ya29.A0ARrda-super-secret-access-token";
        let dir = write_plugin_manifest("wf-auth-leak", "gcloud-auth", WORKFLOW_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            format!(
                r#"[sources]
adc             = {{ path = "~/.config/gcloud/adc.json", priority = 1, token = "{leaked}" }}
service_account = {{ path = ".bwoc/secrets/gcloud-sa.json", priority = 2 }}
env             = {{ vars = ["BWOC_GCLOUD_ACCOUNT"], priority = 3 }}
"#
            ),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[sources].adc.token") && v.contains("redacted")),
            "an inline credential under a source must be a violation, got: {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().all(|v| !v.contains(leaked)),
            "the secret value must be redacted from the report, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_auth_missing_sources_fails() {
        let dir = write_plugin_manifest("wf-auth-nosrc", "gcloud-auth", WORKFLOW_MANIFEST);
        fs::write(dir.join("auth.toml"), "# shape doc only, no [sources]\n").unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[sources] table missing")),
            "missing [sources] must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_auth_missing_source_key_fails() {
        // Drop the `env` source — the precedence contract is incomplete.
        let dir = write_plugin_manifest("wf-auth-noenv", "gcloud-auth", WORKFLOW_MANIFEST);
        fs::write(
            dir.join("auth.toml"),
            r#"[sources]
adc             = { path = "~/.config/gcloud/adc.json", priority = 1 }
service_account = { path = ".bwoc/secrets/gcloud-sa.json", priority = 2 }
"#,
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[sources].env missing")),
            "a missing source key must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_auth_absent_file_not_audited() {
        // A workflow plugin need not carry credentials — no auth.toml means the
        // contract is simply not audited (mirrors the BWOC-45 jira scope).
        let dir = write_plugin_manifest("wf-auth-none", "gcloud-auth", WORKFLOW_MANIFEST);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a workflow plugin without auth.toml must still pass, got: {:?}",
            report.violations
        );
        assert!(
            !report.passes.iter().any(|p| p == "auth.toml present"),
            "absent auth.toml must not be reported as present"
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- BWOC-70: workflow write-verb gate metadata validation -------------

    /// A write-capable workflow plugin manifest header (the gcloud-compute
    /// shape), reused by the verb-metadata fixtures below. Tests append their
    /// own `[[verb]]` tables.
    const COMPUTE_MANIFEST_HEADER: &str = r#"[plugin]
name        = "gcloud-compute"
kind        = "workflow"
version     = "0.1.0"
description = "gcloud Compute Engine instance-lifecycle adapter."
compat      = ">=2.9.0"
entry       = "gcloud.sh"
"#;

    fn compute_manifest_with(verbs: &str) -> String {
        format!("{COMPUTE_MANIFEST_HEADER}{verbs}")
    }

    #[test]
    fn audit_workflow_verbs_well_formed_passes() {
        let body = compute_manifest_with(
            r#"
[[verb]]
name  = "list"
write = false

[[verb]]
name    = "start"
write   = true
confirm = "operator"

[[verb]]
name    = "stop"
write   = true
confirm = "operator"
"#,
        );
        let dir = write_plugin_manifest("wf-verbs-ok", "gcloud-compute", &body);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "well-formed verb metadata must pass, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.starts_with("[[verb]] write-gate metadata declared")),
            "expected the verb metadata to be reported as declared, got: {:?}",
            report.passes
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_verbs_missing_write_classification_fails() {
        // The whole point of the gate metadata: a verb that omits its write
        // classification cannot be gated by the CLI — fail closed.
        let body = compute_manifest_with(
            r#"
[[verb]]
name = "start"
"#,
        );
        let dir = write_plugin_manifest("wf-verbs-no-write", "gcloud-compute", &body);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'start' missing 'write'")),
            "a verb without a write classification must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_verbs_write_without_confirm_gate_fails() {
        // A write verb that declares no operator-confirm gate would be reachable
        // without the documented confirmation — the core BWOC-67 violation.
        let body = compute_manifest_with(
            r#"
[[verb]]
name  = "start"
write = true
"#,
        );
        let dir = write_plugin_manifest("wf-verbs-no-confirm", "gcloud-compute", &body);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'start' is a write but declares no confirm gate")),
            "a write verb without a confirm gate must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_verbs_non_operator_confirm_fails() {
        let body = compute_manifest_with(
            r#"
[[verb]]
name    = "start"
write   = true
confirm = "auto"
"#,
        );
        let dir = write_plugin_manifest("wf-verbs-bad-confirm", "gcloud-compute", &body);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'start'.confirm 'auto' is not 'operator'")),
            "a non-operator confirm mode on a write verb must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_verbs_read_with_confirm_warns_not_fails() {
        // Read verbs are free; a confirm gate on one is contradictory metadata
        // but not a security risk — warn, do not fail.
        let body = compute_manifest_with(
            r#"
[[verb]]
name    = "list"
write   = false
confirm = "operator"
"#,
        );
        let dir = write_plugin_manifest("wf-verbs-read-confirm", "gcloud-compute", &body);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a read verb with a redundant gate must not fail, got: {:?}",
            report.violations
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("'list' is a read") && w.contains("redundant")),
            "expected a redundant-gate warning on the read verb, got: {:?}",
            report.warnings
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_verbs_duplicate_name_fails() {
        let body = compute_manifest_with(
            r#"
[[verb]]
name  = "list"
write = false

[[verb]]
name  = "list"
write = false
"#,
        );
        let dir = write_plugin_manifest("wf-verbs-dup", "gcloud-compute", &body);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'list' is declared more than once")),
            "a duplicate verb name must be a violation, got: {:?}",
            report.violations
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_workflow_verbs_absent_array_not_audited() {
        // A read-only workflow plugin (gcloud-auth / gcloud-project) declares no
        // verb metadata — its absence is not a violation (mirrors absent auth.toml).
        let dir = write_plugin_manifest("wf-verbs-none", "gcloud-auth", WORKFLOW_MANIFEST);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a workflow plugin without verb metadata must still pass, got: {:?}",
            report.violations
        );
        assert!(
            !report
                .passes
                .iter()
                .any(|p| p.starts_with("[[verb]] write-gate metadata declared")),
            "absent verb metadata must not be reported as declared"
        );
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    // ---- BWOC-50: okr data validation (objectives + key_results) ----------

    /// Minimal valid okr-kind plugin manifest, reused by the data fixtures.
    const OKR_MANIFEST: &str = r#"[plugin]
name        = "workspace-okrs"
kind        = "okr"
version     = "0.1.0"
description = "Reference okr plugin tracking Objectives + Key Results."
compat      = ">=2.9.0"
entry       = "okr.sh"
"#;

    /// A well-formed objectives.toml — one top-level objective.
    const OKR_OBJECTIVES_OK: &str = r#"[[objective]]
objective_id = "O1"
title        = "Ship the OKR plugin kind"
owner        = "agent-jisoo"
period       = "2026-Q2"
parent       = ""
"#;

    /// A well-formed key_results.toml — two KRs, one tracked (file evidence),
    /// one never tracked (none evidence, no as_of). Both reference O1.
    const OKR_KEY_RESULTS_OK: &str = r#"[[key_result]]
key_result_id = "O1-KR1"
objective_id  = "O1"
description   = "PLUGINS spec declares the okr kind"
target        = 1
current       = 1
unit          = "count"
confidence    = "high"
evidence      = { kind = "file", value = "docs/en/PLUGINS.en.md" }
as_of         = "2026-05-28"

[[key_result]]
key_result_id = "O1-KR2"
objective_id  = "O1"
description   = "bwoc okr CLI surface ships"
target        = 4
current       = 0
unit          = "count"
confidence    = "medium"
evidence      = { kind = "none", value = "" }
"#;

    /// Write an okr-kind plugin with its two sibling data files. Either data
    /// body may be `None` to exercise the missing-file paths.
    fn write_okr_plugin(
        label: &str,
        objectives: Option<&str>,
        key_results: Option<&str>,
    ) -> std::path::PathBuf {
        let dir = write_plugin_manifest(label, "workspace-okrs", OKR_MANIFEST);
        if let Some(o) = objectives {
            fs::write(dir.join("objectives.toml"), o).unwrap();
        }
        if let Some(k) = key_results {
            fs::write(dir.join("key_results.toml"), k).unwrap();
        }
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn audit_plugin_manifest_real_workspace_okrs_reference_passes() {
        // End-to-end (BWOC-50): audit the actual shipped okr/workspace-okrs
        // reference plugin — manifest + objectives.toml + key_results.toml —
        // exactly as `bwoc check --all` does in an operator workspace.
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/plugins/okr/workspace-okrs");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the plugin — nothing to assert.
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real workspace-okrs manifest + data files must pass bwoc check, got: {:?}",
            report.violations
        );
        // Confirm the okr data audit actually ran (not silently skipped) and that
        // referential integrity was exercised against the real seed data.
        assert!(
            report.passes.iter().any(|p| p == "objectives.toml present"),
            "expected the okr objectives.toml to be validated, got: {:?}",
            report.passes
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("objective_id 'O1' resolves")),
            "expected a real key_result.objective_id to resolve, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_okr_well_formed_data_passes() {
        let dir = write_okr_plugin("okr-ok", Some(OKR_OBJECTIVES_OK), Some(OKR_KEY_RESULTS_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "well-formed okr data must pass, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_dangling_objective_id_fails() {
        // Referential integrity: a key_result.objective_id with no matching
        // objective is a plugin bug, not operator state.
        let krs = OKR_KEY_RESULTS_OK.replace(r#"objective_id  = "O1""#, r#"objective_id  = "O9""#);
        let dir = write_okr_plugin("okr-dangling", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'O9' does not resolve") && v.contains("dangling")),
            "a dangling objective_id must fail referential integrity, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_bad_confidence_enum_fails() {
        let krs =
            OKR_KEY_RESULTS_OK.replace(r#"confidence    = "high""#, r#"confidence    = "certain""#);
        let dir = write_okr_plugin("okr-conf", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("confidence 'certain' not in {high, medium, low}")),
            "an out-of-enum confidence must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_bad_unit_enum_fails() {
        let krs = OKR_KEY_RESULTS_OK
            .replace(r#"unit          = "count""#, r#"unit          = "widgets""#);
        let dir = write_okr_plugin("okr-unit", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("unit 'widgets' not in")),
            "an out-of-enum unit must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_bad_evidence_kind_fails() {
        let krs = OKR_KEY_RESULTS_OK.replace(
            r#"evidence      = { kind = "file", value = "docs/en/PLUGINS.en.md" }"#,
            r#"evidence      = { kind = "screenshot", value = "x.png" }"#,
        );
        let dir = write_okr_plugin("okr-evk", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("evidence.kind 'screenshot' not in")),
            "an out-of-vocabulary evidence kind must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_evidence_none_with_value_fails() {
        // Musāvāda: kind='none' carries no referent — a non-empty value is a lie.
        let krs = OKR_KEY_RESULTS_OK.replace(
            r#"evidence      = { kind = "none", value = "" }"#,
            r#"evidence      = { kind = "none", value = "something" }"#,
        );
        let dir = write_okr_plugin("okr-none-val", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("kind='none' but value is non-empty")),
            "kind=none with a value must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_evidence_referent_required_for_non_none() {
        // Musāvāda: a file/content/command/etc. claim must carry a referent.
        let krs = OKR_KEY_RESULTS_OK.replace(
            r#"evidence      = { kind = "file", value = "docs/en/PLUGINS.en.md" }"#,
            r#"evidence      = { kind = "file", value = "" }"#,
        );
        let dir = write_okr_plugin("okr-empty-ref", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("value is empty") && v.contains("Musāvāda")),
            "a non-none evidence with an empty value must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_duplicate_key_result_id_fails() {
        let krs = format!(
            "{OKR_KEY_RESULTS_OK}\n[[key_result]]\nkey_result_id = \"O1-KR1\"\nobjective_id  = \"O1\"\ndescription   = \"dup\"\ntarget        = 1\ncurrent       = 0\nunit          = \"count\"\nconfidence    = \"low\"\nevidence      = {{ kind = \"none\", value = \"\" }}\n"
        );
        let dir = write_okr_plugin("okr-dup", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("'O1-KR1' declared more than once")),
            "a duplicate key_result_id must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_non_numeric_target_fails() {
        let krs = OKR_KEY_RESULTS_OK.replace("target        = 1", r#"target        = "one""#);
        let dir = write_okr_plugin("okr-target", Some(OKR_OBJECTIVES_OK), Some(&krs));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("target has wrong type — expected a number")),
            "a non-numeric target must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_missing_objectives_fails_and_cascades() {
        // No objectives.toml → the file is a violation AND every key_result's
        // objective_id dangles (empty resolution set).
        let dir = write_okr_plugin("okr-noobj", None, Some(OKR_KEY_RESULTS_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("objectives.toml missing or unreadable")),
            "a missing objectives.toml must fail, got: {:?}",
            report.violations
        );
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("does not resolve")),
            "an empty objective set must cascade into referential failures, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_missing_key_results_fails() {
        let dir = write_okr_plugin("okr-nokr", Some(OKR_OBJECTIVES_OK), None);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("key_results.toml missing or unreadable")),
            "a missing key_results.toml must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_okr_never_tracked_omits_as_of_and_passes() {
        // The schema says as_of is omitted (not null) when never tracked; the
        // OK fixture's KR2 exercises exactly that — confirm it does not warn.
        let dir = write_okr_plugin(
            "okr-noas",
            Some(OKR_OBJECTIVES_OK),
            Some(OKR_KEY_RESULTS_OK),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            !report.violations.iter().any(|v| v.contains("as_of")),
            "a never-tracked KR (no as_of) must not raise an as_of violation, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    // ---- BWOC-60: council manifest + templates + Decision Schema ----------

    /// Minimal valid council-kind plugin manifest (matches the dir basename
    /// `council-sangha-7` so the name check passes), reused by the fixtures.
    const COUNCIL_MANIFEST_OK: &str = r#"[plugin]
name        = "council-sangha-7"
kind        = "council"
version     = "0.1.0"
description = "Aparihaniya-dhamma 7 consensus council reference plugin."
compat      = ">=2.9.0"
entry       = "protocol.sh"

[council]
voting_model = "sangha"
quorum       = "2/3"
"#;

    /// A well-formed decisions.toml — two issue templates, each with ≥2 options.
    const COUNCIL_TEMPLATES_OK: &str = r#"[[template]]
template_id = "ap1-regular-meetings"
condition   = 1
name        = "Regular meetings"
question    = "Shall the fleet hold standups on a fixed, frequent cadence?"
options     = ["affirm-cadence", "revise-cadence"]

[[template]]
template_id = "ap2-coordinated-start-end"
condition   = 2
name        = "Coordinated start/end"
question    = "Shall the fleet begin and end sprints in concord?"
options     = ["affirm-concord", "revise-concord"]
"#;

    /// Write a council-kind plugin with an optional sibling decisions.toml.
    fn write_council_plugin(
        label: &str,
        manifest: &str,
        templates: Option<&str>,
    ) -> std::path::PathBuf {
        let dir = write_plugin_manifest(label, "council-sangha-7", manifest);
        if let Some(t) = templates {
            fs::write(dir.join("decisions.toml"), t).unwrap();
        }
        dir
    }

    /// A schema-conformant decision record (resolved, with an abstention, dissent,
    /// and an evidence link) — the base the Decision Schema tests mutate.
    fn good_decision() -> serde_json::Value {
        serde_json::json!({
            "decision_id": "D1",
            "status": "resolved",
            "participants": ["agent-jisoo", "agent-jennie", "agent-lisa", "agent-rose"],
            "options": ["adopt", "defer"],
            "rounds": [{
                "round": 1,
                "turns": [{ "participant": "agent-jisoo", "message_ref": "msg-20260528T120000Z-a1b2c" }]
            }],
            "votes": [
                { "participant": "agent-jisoo", "option": "adopt", "abstain": false, "cast_at": "2026-05-28T12:10:00Z" },
                { "participant": "agent-rose",  "abstain": true,                      "cast_at": "2026-05-28T12:11:00Z" }
            ],
            "outcome": "adopt",
            "dissent": [{ "participant": "agent-lisa", "option": "defer", "rationale": "prefers to wait" }],
            "evidence_links": [{ "kind": "file", "value": "notes/2026-05-28_council-plugin-architecture.md" }],
            "opened_at": "2026-05-28T12:00:00Z",
            "closed_at": "2026-05-28T12:30:00Z"
        })
    }

    fn decision_violations(value: &serde_json::Value) -> Vec<String> {
        let mut report = AuditReport {
            target: "test".to_string(),
            passes: Vec::new(),
            warnings: Vec::new(),
            violations: Vec::new(),
        };
        validate_council_decision("D1.json", value, &mut report);
        report.violations
    }

    #[test]
    fn audit_plugin_manifest_real_council_sangha7_reference_passes() {
        // End-to-end (BWOC-60): audit the actual shipped council/council-sangha-7
        // reference plugin — manifest [council] table + decisions.toml templates —
        // exactly as `bwoc check --all` does in an operator workspace. The plugin
        // ships templates, not records, so the records audit finds nothing.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../modules/plugins/council/council-sangha-7");
        if !dir.join("manifest.toml").is_file() {
            return; // partial checkout without the plugin — nothing to assert.
        }
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "real council-sangha-7 manifest + decisions.toml must pass bwoc check, got: {:?}",
            report.violations
        );
        // Confirm the council-specific audit actually ran (not silently skipped).
        assert!(
            report
                .passes
                .iter()
                .any(|p| p == "[council].voting_model 'sangha' in supported set"),
            "expected the council voting_model to be validated, got: {:?}",
            report.passes
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.starts_with("decisions.toml declares") && p.contains("template")),
            "expected the council decisions.toml templates to be validated, got: {:?}",
            report.passes
        );
    }

    #[test]
    fn audit_council_well_formed_manifest_passes() {
        let dir = write_council_plugin(
            "council-ok",
            COUNCIL_MANIFEST_OK,
            Some(COUNCIL_TEMPLATES_OK),
        );
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "a well-formed council plugin must pass, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_integer_quorum_passes() {
        let manifest = COUNCIL_MANIFEST_OK.replace(r#"quorum       = "2/3""#, "quorum       = 3");
        let dir = write_council_plugin("council-intq", &manifest, Some(COUNCIL_TEMPLATES_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report.violations.is_empty(),
            "an integer quorum must be well-formed, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_bad_voting_model_fails() {
        let manifest = COUNCIL_MANIFEST_OK
            .replace(r#"voting_model = "sangha""#, r#"voting_model = "dictator""#);
        let dir = write_council_plugin("council-badvm", &manifest, Some(COUNCIL_TEMPLATES_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("voting_model 'dictator' not in")),
            "an out-of-set voting_model must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_missing_voting_model_fails() {
        let manifest = COUNCIL_MANIFEST_OK.replace("voting_model = \"sangha\"\n", "");
        let dir = write_council_plugin("council-novm", &manifest, Some(COUNCIL_TEMPLATES_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[council].voting_model missing")),
            "a missing voting_model must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_bad_quorum_fails() {
        // A zero-denominator fraction is rejected by the same parser the runtime
        // tally uses (anti-drift) — the static check must reject it too.
        let manifest =
            COUNCIL_MANIFEST_OK.replace(r#"quorum       = "2/3""#, r#"quorum       = "1/0""#);
        let dir = write_council_plugin("council-badq", &manifest, Some(COUNCIL_TEMPLATES_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[council].quorum is malformed")),
            "a malformed quorum must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_missing_council_table_fails() {
        // Strip the whole [council] table — a council plugin must declare it.
        let manifest = &COUNCIL_MANIFEST_OK[..COUNCIL_MANIFEST_OK.find("[council]").unwrap()];
        let dir = write_council_plugin("council-notable", manifest, Some(COUNCIL_TEMPLATES_OK));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("[council] table missing")),
            "a missing [council] table must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_missing_decisions_toml_fails() {
        let dir = write_council_plugin("council-notmpl", COUNCIL_MANIFEST_OK, None);
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("decisions.toml missing or unreadable")),
            "a missing decisions.toml must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_template_options_below_two_fails() {
        let templates = COUNCIL_TEMPLATES_OK.replace(
            r#"options     = ["affirm-cadence", "revise-cadence"]"#,
            r#"options     = ["affirm-cadence"]"#,
        );
        let dir = write_council_plugin("council-1opt", COUNCIL_MANIFEST_OK, Some(&templates));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("options must declare ≥2 choices")),
            "a template with <2 options must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_template_duplicate_id_fails() {
        let templates = COUNCIL_TEMPLATES_OK.replace(
            r#"template_id = "ap2-coordinated-start-end""#,
            r#"template_id = "ap1-regular-meetings""#,
        );
        let dir = write_council_plugin("council-duptmpl", COUNCIL_MANIFEST_OK, Some(&templates));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("declared more than once")),
            "a duplicate template_id must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn audit_council_template_non_integer_condition_fails() {
        let templates = COUNCIL_TEMPLATES_OK.replace("condition   = 1", r#"condition   = "one""#);
        let dir = write_council_plugin("council-strcond", COUNCIL_MANIFEST_OK, Some(&templates));
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("condition has wrong type")),
            "a non-integer condition must fail, got: {:?}",
            report.violations
        );
        cleanup(&dir);
    }

    #[test]
    fn council_decision_well_formed_passes() {
        assert!(
            decision_violations(&good_decision()).is_empty(),
            "a schema-conformant decision must pass, got: {:?}",
            decision_violations(&good_decision())
        );
    }

    #[test]
    fn council_decision_bad_status_fails() {
        let mut d = good_decision();
        d["status"] = serde_json::json!("frozen");
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("status 'frozen' not in")),
            "an out-of-enum status must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_missing_decision_id_fails() {
        let mut d = good_decision();
        d.as_object_mut().unwrap().remove("decision_id");
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("missing required 'decision_id'")),
            "a missing decision_id must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_options_below_two_fails() {
        let mut d = good_decision();
        d["options"] = serde_json::json!(["adopt"]);
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("options must declare ≥2 choices")),
            "fewer than two options must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_round_turn_missing_message_ref_fails() {
        let mut d = good_decision();
        d["rounds"] = serde_json::json!([{
            "round": 1,
            "turns": [{ "participant": "agent-jisoo" }]
        }]);
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("must carry non-empty 'participant' + 'message_ref'")),
            "a turn missing message_ref must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_vote_missing_abstain_fails() {
        let mut d = good_decision();
        d["votes"] = serde_json::json!([{ "participant": "agent-jisoo", "option": "adopt" }]);
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("missing 'abstain'")),
            "a vote missing the abstain flag must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_non_abstain_without_option_fails() {
        let mut d = good_decision();
        d["votes"] = serde_json::json!([{ "participant": "agent-jisoo", "abstain": false }]);
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("names no 'option'")),
            "a non-abstaining vote with no option must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_abstention_without_option_passes() {
        // The mirror of the above: an abstention legitimately carries no option.
        let mut d = good_decision();
        d["votes"] = serde_json::json!([
            { "participant": "agent-jisoo", "option": "adopt", "abstain": false },
            { "participant": "agent-rose",  "abstain": true }
        ]);
        assert!(
            !decision_violations(&d).iter().any(|v| v.contains("vote")),
            "an abstention with no option must be accepted, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_dissent_missing_option_fails() {
        let mut d = good_decision();
        d["dissent"] = serde_json::json!([{ "participant": "agent-lisa", "rationale": "x" }]);
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("dissent #1 must carry non-empty 'participant' + 'option'")),
            "a dissent entry missing its option must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_bad_evidence_kind_fails() {
        let mut d = good_decision();
        d["evidence_links"] = serde_json::json!([{ "kind": "screenshot", "value": "x.png" }]);
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("kind 'screenshot' not in")),
            "an out-of-vocabulary evidence kind must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn council_decision_missing_opened_at_fails() {
        let mut d = good_decision();
        d.as_object_mut().unwrap().remove("opened_at");
        assert!(
            decision_violations(&d)
                .iter()
                .any(|v| v.contains("missing required 'opened_at'")),
            "a missing opened_at must fail, got: {:?}",
            decision_violations(&d)
        );
    }

    #[test]
    fn audit_council_records_dir_validates_each_entry() {
        // A plugin-local records/ dir (the SPEC's hand-invocation fallback) is
        // audited per JSON entry against the Council Decision Schema.
        let dir = write_council_plugin(
            "council-recs",
            COUNCIL_MANIFEST_OK,
            Some(COUNCIL_TEMPLATES_OK),
        );
        let records = dir.join("records");
        fs::create_dir_all(&records).unwrap();
        // One conformant record, one malformed (bad status).
        fs::write(
            records.join("D1.json"),
            serde_json::to_string_pretty(&good_decision()).unwrap(),
        )
        .unwrap();
        let mut bad = good_decision();
        bad["status"] = serde_json::json!("frozen");
        fs::write(
            records.join("D2.json"),
            serde_json::to_string_pretty(&bad).unwrap(),
        )
        .unwrap();
        let report = audit_plugin_manifest(&dir);
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.contains("status 'frozen' not in")),
            "a malformed plugin-local record must surface a violation, got: {:?}",
            report.violations
        );
        assert!(
            report
                .passes
                .iter()
                .any(|p| p.contains("D1.json") && p.contains("decision_id present")),
            "the conformant record must produce passes, got: {:?}",
            report.passes
        );
        cleanup(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn template_mode_still_checks_placeholders_exist() {
        let root = write_temp_agent(
            "tmpl",
            "{{name}}", // template-mode trigger
            "AGENTS.md without any placeholders at all.",
        );
        let report = audit(&root);
        // In template mode, MISSING recommended placeholders become warnings.
        let warned = report
            .warnings
            .iter()
            .any(|w| w.contains("missing recommended placeholder {{agentId}}"));
        assert!(
            warned,
            "expected template-mode warning, got: {:?}",
            report.warnings
        );
        let _ = fs::remove_dir_all(&root);
    }
}
