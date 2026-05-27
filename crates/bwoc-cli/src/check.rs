//! `bwoc check` — backend-neutrality audit.
//!
//! Rust port of `modules/agent-template/scripts/check-agent-neutrality.sh`
//! with feature parity. Pure-data audit + separate printer for testability.

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
/// its own `bwoc check`.
const PLUGIN_KINDS: &[&str] = &["memory-backend", "llm-backend", "workflow", "audit", "jira"];

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

/// Maturity values accepted in a skill manifest (Ariya-dhana 7 scale).
const MATURITY_LEVELS: &[&str] = &["L1", "L2", "L3", "L4", "L5", "L6", "L7"];

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
                             (expected one of {{memory-backend, llm-backend, workflow, audit, jira}})"
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

    // Kind ∈ {memory-backend, llm-backend, workflow, audit, jira}.
    if let Some(kind) = plugin_table.get("kind").and_then(|v| v.as_str()) {
        if PLUGIN_KINDS.contains(&kind) {
            report
                .passes
                .push(format!("[plugin].kind '{kind}' in supported set"));
        } else {
            report.violations.push(format!(
                "[plugin].kind '{kind}' not in {{memory-backend, llm-backend, workflow, audit, jira}}"
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

    report
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

fn discover_module_dirs(root: &Path, sub: &str) -> Vec<std::path::PathBuf> {
    let dir = root.join(sub);
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<std::path::PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("manifest.toml").is_file())
        .collect();
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
