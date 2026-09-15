//! Extra tool implementations added in the "complete tool set" increment:
//! edit_file, grep, git, run_gates, bwoc_task, bwoc_send, memory_read,
//! memory_write.
//!
//! Every tool routes through the caller's safety pipeline
//! (guardrails → permission → sandbox) via `ToolContext` path confinement.
//! The tools themselves do not call the pipeline — that is the agent_loop's
//! responsibility — but they enforce worktree confinement via
//! `ToolContext::resolve_path` on every filesystem operation.

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{ToolContext, ToolImpl};
use crate::error::HarnessError;
use crate::sandbox::shell_command;

// ---------------------------------------------------------------------------
// edit_file — targeted string replacement (unique-match patch)
// ---------------------------------------------------------------------------

/// Replace exactly one occurrence of `old_string` with `new_string` in the
/// file at `path`.  The replacement is rejected if `old_string` matches zero
/// or more than one time (unique-match invariant, same as Claude Code's Edit
/// tool).
///
/// When an exact match fails, a **whitespace-tolerant fallback** is tried: the
/// file is matched line-by-line against `old_string` ignoring each line's
/// leading/trailing whitespace, and the replacement is re-indented to the
/// file's actual indentation. This rescues the most common small-model failure
/// — an `old_string` whose indentation is slightly off — while still refusing
/// an ambiguous (multi-site) match. See [`apply_edit`].
///
/// Confined to the worktree; passes through the safety pipeline before
/// reaching here.
pub struct EditFile;

/// Outcome of an [`apply_edit`] attempt.
#[derive(Debug, PartialEq)]
enum EditOutcome {
    /// Replacement applied. Carries the new file content and how it matched
    /// (`"exact"` or `"whitespace-tolerant"`), for the result message.
    Replaced { content: String, how: &'static str },
    /// `old_string` matched nowhere (exact nor trimmed).
    NotFound,
    /// `old_string` matched in more than one place (`count` sites); the caller
    /// must ask for more surrounding context rather than guess.
    Ambiguous { count: usize },
}

/// Leading-whitespace prefix of a line (spaces/tabs).
fn leading_ws(line: &str) -> &str {
    let end = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..end]
}

/// Compute the replacement for `old` → `new` in `content`.
///
/// 1. **Exact**: if `old` occurs exactly once, replace it verbatim. More than
///    once → [`EditOutcome::Ambiguous`].
/// 2. **Whitespace-tolerant fallback** (only when there is no exact match):
///    match `old`'s lines against the file's lines comparing `str::trim`-med
///    content, so indentation/trailing-space differences don't matter. A unique
///    block match is replaced; the `new` lines are re-indented by the delta
///    between the matched file block's indent and `old`'s indent, so the patch
///    adopts the file's real indentation. Multiple block matches → ambiguous.
///
/// Operates on `\n`-split lines; a final `\n` is preserved by the split/join
/// round-trip. CRLF files match (trim drops the `\r`) but inserted lines use
/// `\n` — acceptable for the LF-dominant files these agents edit.
fn apply_edit(content: &str, old: &str, new: &str) -> EditOutcome {
    // ── 1. Exact ─────────────────────────────────────────────────────────
    match content.matches(old).count() {
        1 => {
            return EditOutcome::Replaced {
                content: content.replacen(old, new, 1),
                how: "exact",
            };
        }
        n if n > 1 => return EditOutcome::Ambiguous { count: n },
        _ => {} // 0 → fall through to the tolerant pass
    }

    // ── 2. Whitespace-tolerant, line-block fallback ──────────────────────
    let file_lines: Vec<&str> = content.split('\n').collect();
    // Drop a single trailing empty element so a trailing newline in `old`
    // doesn't force the window to include the file's final empty line.
    let mut old_lines: Vec<&str> = old.split('\n').collect();
    if old_lines.len() > 1 && old_lines.last() == Some(&"") {
        old_lines.pop();
    }
    let n = old_lines.len();
    if n == 0 || n > file_lines.len() {
        return EditOutcome::NotFound;
    }

    let trimmed_old: Vec<&str> = old_lines.iter().map(|l| l.trim()).collect();
    let starts: Vec<usize> = (0..=file_lines.len() - n)
        .filter(|&i| (0..n).all(|j| file_lines[i + j].trim() == trimmed_old[j]))
        .collect();

    match starts.as_slice() {
        [] => EditOutcome::NotFound,
        [start] => {
            let start = *start;
            // Re-indent `new` by (file block indent − old indent).
            let file_indent = leading_ws(file_lines[start]);
            let old_indent = leading_ws(old_lines[0]);
            let mut new_lines: Vec<String> = new
                .split('\n')
                .map(|line| {
                    if line.trim().is_empty() {
                        String::new()
                    } else {
                        let body = line.strip_prefix(old_indent).unwrap_or(line);
                        format!("{file_indent}{body}")
                    }
                })
                .collect();
            // The replaced block is a span of inter-newline lines; the
            // surrounding file already supplies the boundary newline after it.
            // A `new_string` ending in `\n` would otherwise splice an extra
            // blank line, so drop a single trailing empty element (mirrors the
            // `old_lines` trim above).
            if new_lines.len() > 1 && new_lines.last().map(|s| s.is_empty()) == Some(true) {
                new_lines.pop();
            }
            let mut out: Vec<&str> = Vec::with_capacity(file_lines.len() - n + new_lines.len());
            out.extend_from_slice(&file_lines[..start]);
            out.extend(new_lines.iter().map(|s| s.as_str()));
            out.extend_from_slice(&file_lines[start + n..]);
            EditOutcome::Replaced {
                content: out.join("\n"),
                how: "whitespace-tolerant",
            }
        }
        many => EditOutcome::Ambiguous { count: many.len() },
    }
}

/// Apply one edit to in-memory `content`. Returns the new content and a short
/// "replaced …" summary, or the refusal reason. `replace_all` replaces every
/// exact occurrence (no whitespace-tolerant pass: a bulk rewrite must not
/// guess); otherwise [`apply_edit`]'s unique-match rules apply.
fn apply_one(
    content: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<(String, String), String> {
    if old.is_empty() {
        return Err("`old_string` must not be empty".to_string());
    }
    if replace_all {
        let count = content.matches(old).count();
        if count == 0 {
            return Err(
                "`old_string` not found (`replace_all` matches exactly). Read the file \
                 first and ensure the text matches."
                    .to_string(),
            );
        }
        return Ok((
            content.replace(old, new),
            format!("replaced {count} occurrence(s) (exact match, replace_all)"),
        ));
    }
    match apply_edit(content, old, new) {
        EditOutcome::Replaced { content, how } => {
            Ok((content, format!("replaced 1 occurrence ({how} match)")))
        }
        EditOutcome::NotFound => Err(
            "`old_string` not found (tried exact and whitespace-tolerant matching). \
             Read the file first and ensure the lines match."
                .to_string(),
        ),
        EditOutcome::Ambiguous { count } => Err(format!(
            "`old_string` matches {count} places. Provide more surrounding context so \
             the match is unique, or set `replace_all`."
        )),
    }
}

fn tool_error(tool: &str, reason: impl Into<String>) -> HarnessError {
    HarnessError::ToolExecution {
        tool: tool.to_string(),
        reason: reason.into(),
    }
}

#[async_trait]
impl ToolImpl for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Targeted string replacement in a file. Replaces one occurrence of \
         `old_string` with `new_string`. Tries an exact match first, then a \
         whitespace-tolerant line match (indentation/trailing-space differences \
         are forgiven and the replacement is re-indented to the file). Fails if \
         `old_string` is not found, or matches more than one place (provide more \
         surrounding context to disambiguate). Set `replace_all` to replace every \
         exact occurrence instead. The file must already exist."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative to working directory)."
                },
                "old_string": {
                    "type": "string",
                    "description": "The string to find. Matched exactly first; if that fails, matched line-by-line ignoring each line's leading/trailing whitespace. Must identify a unique location (an ambiguous match is rejected — add surrounding context)."
                },
                "new_string": {
                    "type": "string",
                    "description": "The replacement string."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace every exact occurrence of `old_string` (e.g. a rename). Default: false."
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let arg = |key: &str| {
            args[key]
                .as_str()
                .ok_or_else(|| tool_error(self.name(), format!("missing `{key}` argument")))
        };
        let raw = arg("path")?;
        let old = arg("old_string")?;
        let new = arg("new_string")?;
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);

        let path = ctx.resolve_path(raw)?;
        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            tool_error(
                self.name(),
                format!("cannot read `{}`: {e}", path.display()),
            )
        })?;

        let (updated, summary) = apply_one(&content, old, new, replace_all).map_err(|reason| {
            tool_error(self.name(), format!("{reason} (`{}`)", path.display()))
        })?;

        tokio::fs::write(&path, &updated).await.map_err(|e| {
            tool_error(
                self.name(),
                format!("cannot write `{}`: {e}", path.display()),
            )
        })?;

        Ok(format!("edited `{}`: {summary}", path.display()))
    }
}

// ---------------------------------------------------------------------------
// multi_edit — several replacements in one file, all or nothing
// ---------------------------------------------------------------------------

/// Apply an ordered list of `edit_file`-style replacements to one file. Every
/// edit runs against the result of the previous one, in memory; the file is
/// written once, and only if every edit succeeded.
pub struct MultiEdit;

/// Upper bound on edits per call — a runaway list is a model fault, not a patch.
const MULTI_EDIT_MAX: usize = 100;

#[async_trait]
impl ToolImpl for MultiEdit {
    fn name(&self) -> &'static str {
        "multi_edit"
    }

    fn description(&self) -> &'static str {
        "Apply several string replacements to one file, in order, all or nothing. \
         Each entry in `edits` has `old_string`, `new_string` and optional \
         `replace_all`, with the same matching rules as `edit_file`, and sees the \
         result of the edits before it. If any edit fails, nothing is written. \
         The file must already exist."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (relative to working directory)."
                },
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "description": "Replacements applied in order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" },
                            "replace_all": { "type": "boolean" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let raw = args["path"]
            .as_str()
            .ok_or_else(|| tool_error(self.name(), "missing `path` argument"))?;
        let edits = args["edits"]
            .as_array()
            .filter(|e| !e.is_empty())
            .ok_or_else(|| tool_error(self.name(), "`edits` must be a non-empty array"))?;
        if edits.len() > MULTI_EDIT_MAX {
            return Err(tool_error(
                self.name(),
                format!("too many edits ({}; max {MULTI_EDIT_MAX})", edits.len()),
            ));
        }

        let path = ctx.resolve_path(raw)?;
        let mut content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            tool_error(
                self.name(),
                format!("cannot read `{}`: {e}", path.display()),
            )
        })?;

        let mut summaries = Vec::with_capacity(edits.len());
        for (i, edit) in edits.iter().enumerate() {
            let n = i + 1;
            let (Some(old), Some(new)) = (edit["old_string"].as_str(), edit["new_string"].as_str())
            else {
                return Err(tool_error(
                    self.name(),
                    format!("edit #{n}: needs `old_string` and `new_string`; no changes written"),
                ));
            };
            let replace_all = edit["replace_all"].as_bool().unwrap_or(false);
            let (next, summary) = apply_one(&content, old, new, replace_all).map_err(|reason| {
                tool_error(
                    self.name(),
                    format!(
                        "edit #{n}: {reason} (`{}`); no changes written",
                        path.display()
                    ),
                )
            })?;
            content = next;
            summaries.push(format!("#{n}: {summary}"));
        }

        tokio::fs::write(&path, &content).await.map_err(|e| {
            tool_error(
                self.name(),
                format!("cannot write `{}`: {e}", path.display()),
            )
        })?;

        Ok(format!(
            "edited `{}`: applied {} edit(s)\n{}",
            path.display(),
            summaries.len(),
            summaries.join("\n")
        ))
    }
}

// ---------------------------------------------------------------------------
// grep — search file contents by pattern under the worktree
// ---------------------------------------------------------------------------

/// Walk the worktree (or a sub-path) and search for lines matching a regex.
///
/// Pure-Rust walk + the `regex` crate, so no external binary is required.
/// `fixed_strings` searches the pattern literally. A pattern that is not a
/// valid regex falls back to a literal search (with a note), so a caller that
/// relied on the earlier substring-only behaviour (`foo(`) keeps working.
/// Binary files (a NUL byte in the first 8 KB, or non-UTF-8) are skipped.
///
/// Output: matching lines in `<relative-path>:<line-no>:<line>` format,
/// capped at 1000 matches to prevent context overflow.
pub struct Grep;

const GREP_MAX_MATCHES: usize = 1_000;

/// Bytes inspected for a NUL when deciding a file is binary (git's heuristic).
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

#[async_trait]
impl ToolImpl for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search file contents under the working directory for lines matching a \
         regular expression (Rust regex syntax). Returns matching lines as \
         `<path>:<line>:<content>`. Set `fixed_strings` to search the pattern \
         literally, `case_insensitive` to ignore case, and `glob` (e.g. `*.rs`) to \
         restrict which files are searched. Binary files and hidden directories are \
         skipped. Results are capped at 1000 matches. Confined to the working directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression to search for (a literal string when `fixed_strings` is true)."
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search (relative to working directory). Defaults to working directory root."
                },
                "glob": {
                    "type": "string",
                    "description": "Only search files matching this glob (same syntax as the `glob` tool), e.g. `*.rs`."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "If true, matching is case-insensitive. Default: false."
                },
                "fixed_strings": {
                    "type": "boolean",
                    "description": "If true, `pattern` is a literal string, not a regex. Default: false."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `pattern` argument".to_string(),
            })?;
        let raw = args["path"].as_str().unwrap_or(".");
        let case_insensitive = args["case_insensitive"].as_bool().unwrap_or(false);
        let fixed = args["fixed_strings"].as_bool().unwrap_or(false);

        let search_root = ctx.resolve_path(raw)?;
        let (matcher, fallback_note) = grep_matcher(pattern, fixed, case_insensitive)?;
        let include = match args["glob"].as_str() {
            Some(g) => Some(glob_to_regex(g).map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("invalid glob `{g}`: {e}"),
            })?),
            None => None,
        };

        // Walking is blocking I/O; keep it off the async runtime.
        let workdir = ctx.workdir.clone();
        let results = tokio::task::spawn_blocking(move || {
            grep_walk(&search_root, &workdir, &matcher, include.as_ref())
        })
        .await
        .map_err(|e| HarnessError::ToolExecution {
            tool: "grep".to_string(),
            reason: format!("grep task panicked: {e}"),
        })?;

        let mut out = if results.is_empty() {
            format!("no matches for `{pattern}` in `{raw}`")
        } else {
            let truncated = results.len() >= GREP_MAX_MATCHES;
            let mut out = results.join("\n");
            if truncated {
                out.push_str(&format!(
                    "\n[truncated at {GREP_MAX_MATCHES} matches — narrow your search path or pattern]"
                ));
            }
            out
        };
        if let Some(note) = fallback_note {
            out.push_str(&note);
        }
        Ok(out)
    }
}

/// Build the line matcher. An invalid regex (when `fixed` is false) degrades to
/// a literal search and returns a note saying so, rather than failing the call.
fn grep_matcher(
    pattern: &str,
    fixed: bool,
    case_insensitive: bool,
) -> Result<(regex::Regex, Option<String>), HarnessError> {
    let build = |p: &str| {
        regex::RegexBuilder::new(p)
            .case_insensitive(case_insensitive)
            .build()
    };
    let literal = regex::escape(pattern);
    if fixed {
        return build(&literal).map(|r| (r, None)).map_err(grep_regex_error);
    }
    match build(pattern) {
        Ok(r) => Ok((r, None)),
        Err(e) => {
            let note = format!(
                "\n[note: `pattern` is not a valid regex ({}); searched it as a literal string]",
                e.to_string().lines().last().unwrap_or("parse error").trim()
            );
            build(&literal)
                .map(|r| (r, Some(note)))
                .map_err(grep_regex_error)
        }
    }
}

fn grep_regex_error(e: regex::Error) -> HarnessError {
    HarnessError::ToolExecution {
        tool: "grep".to_string(),
        reason: format!("cannot compile pattern: {e}"),
    }
}

/// Synchronous recursive walk + grep (runs in spawn_blocking).
fn grep_walk(
    root: &std::path::Path,
    workdir: &std::path::Path,
    matcher: &regex::Regex,
    include: Option<&regex::Regex>,
) -> Vec<String> {
    let mut matches = Vec::new();

    walk_files(root, workdir, &mut |p| {
        if let Some(include) = include {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !include.is_match(&name) && !include.is_match(&rel_slash(p, root)) {
                return true;
            }
        }
        let Ok(bytes) = std::fs::read(p) else {
            return true;
        };
        if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
            return true;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            return true;
        };
        let rel = rel_slash(p, workdir);
        for (lineno, line) in content.lines().enumerate() {
            if matcher.is_match(line) {
                matches.push(format!("{}:{}:{}", rel, lineno + 1, line));
                if matches.len() >= GREP_MAX_MATCHES {
                    return false;
                }
            }
        }
        true
    });

    matches
}

/// Visit every regular file under `root` (or `root` itself when it is a file),
/// calling `visit` until it returns `false`. Shared by `grep` and `glob`.
///
/// - Confined: a directory outside `workdir` is never read, and a symlink is
///   followed only when its target stays inside the workdir (canonical check).
/// - Hidden directories (`.git`, `.bwoc`, …) are not descended into.
/// - `.gitignore` is **not** honoured — no ignore-matcher dependency is carried;
///   callers narrow with a `path` (or `glob`) instead.
fn walk_files(
    root: &std::path::Path,
    workdir: &std::path::Path,
    visit: &mut dyn FnMut(&std::path::Path) -> bool,
) {
    if root.is_file() {
        visit(root);
        return;
    }
    let mut dirs: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if !dir.starts_with(workdir) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            if !p.starts_with(workdir)
                || (entry.file_type().is_ok_and(|t| t.is_symlink())
                    && !crate::sandbox::is_confined(&p, workdir))
            {
                continue;
            }
            if p.is_dir() {
                let hidden = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'));
                if !hidden {
                    dirs.push(p);
                }
            } else if p.is_file() && !visit(&p) {
                return;
            }
        }
    }
}

/// `p` relative to `base`, with `/` separators on every platform.
fn rel_slash(p: &std::path::Path, base: &std::path::Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// glob — find paths by glob pattern under the worktree
// ---------------------------------------------------------------------------

/// List files whose path matches a glob pattern. Read-only; shares the confined
/// walker with `grep` (hidden dirs skipped, `.gitignore` not honoured).
pub struct Glob;

const GLOB_MAX_RESULTS: usize = 1_000;

#[async_trait]
impl ToolImpl for Glob {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> &'static str {
        "Find files by glob pattern under the working directory. `*` matches within \
         one path segment, `**` across segments, `?` one character, `[abc]` a class, \
         `{a,b}` alternatives. A pattern without `/` matches the file name at any \
         depth (`*.rs`); a pattern with `/` matches the path relative to `path` \
         (`src/**/*.rs`). Returns sorted paths relative to the working directory, \
         capped at 1000. Hidden directories are skipped; `.gitignore` is not read."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern, e.g. `*.toml` or `crates/**/src/*.rs`."
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search (relative to working directory). Defaults to the working directory root."
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `pattern` argument".to_string(),
            })?;
        let raw = args["path"].as_str().unwrap_or(".");
        let base = ctx.resolve_path(raw)?;
        let matcher = glob_to_regex(pattern).map_err(|e| HarnessError::ToolExecution {
            tool: self.name().to_string(),
            reason: format!("invalid glob `{pattern}`: {e}"),
        })?;
        let by_name = !pattern.contains('/');

        let workdir = ctx.workdir.clone();
        let (paths, truncated) =
            tokio::task::spawn_blocking(move || glob_walk(&base, &workdir, &matcher, by_name))
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: "glob".to_string(),
                    reason: format!("glob task panicked: {e}"),
                })?;

        if paths.is_empty() {
            return Ok(format!("no files match `{pattern}` in `{raw}`"));
        }
        let mut out = paths.join("\n");
        if truncated {
            out.push_str(&format!(
                "\n[truncated at {GLOB_MAX_RESULTS} paths — narrow the pattern or path]"
            ));
        }
        Ok(out)
    }
}

/// Synchronous glob walk (runs in spawn_blocking). Returns sorted
/// workdir-relative paths and whether the result cap was hit.
fn glob_walk(
    base: &std::path::Path,
    workdir: &std::path::Path,
    matcher: &regex::Regex,
    by_name: bool,
) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut truncated = false;
    walk_files(base, workdir, &mut |p| {
        let subject = if by_name {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            rel_slash(p, base)
        };
        if matcher.is_match(&subject) {
            if paths.len() >= GLOB_MAX_RESULTS {
                truncated = true;
                return false;
            }
            paths.push(rel_slash(p, workdir));
        }
        true
    });
    paths.sort();
    (paths, truncated)
}

/// Compile a glob into an anchored regex. `**/` spans zero or more directories,
/// `**` anything, `*` / `?` stay within one segment, `[..]` (with `!` negation)
/// is a character class, `{a,b}` an alternation; everything else is literal.
fn glob_to_regex(glob: &str) -> Result<regex::Regex, regex::Error> {
    let chars: Vec<char> = glob.chars().collect();
    let mut re = String::from("^");
    let mut braces = 0usize;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    re.push_str("(?:[^/]*/)*");
                    i += 3;
                } else {
                    re.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '{' => {
                braces += 1;
                re.push_str("(?:");
            }
            '}' if braces > 0 => {
                braces -= 1;
                re.push(')');
            }
            ',' if braces > 0 => re.push('|'),
            '[' => {
                if let Some(len) = chars[i + 1..].iter().position(|&c| c == ']') {
                    let body: String = chars[i + 1..i + 1 + len].iter().collect();
                    let body = match body.strip_prefix('!') {
                        Some(rest) => format!("^{rest}"),
                        None => body,
                    };
                    re.push('[');
                    re.push_str(&body.replace('\\', "\\\\"));
                    re.push(']');
                    i += len + 2;
                    continue;
                }
                re.push_str("\\[");
            }
            c => re.push_str(&regex::escape(c.encode_utf8(&mut [0u8; 4]))),
        }
        i += 1;
    }
    re.push('$');
    regex::Regex::new(&re)
}

// ---------------------------------------------------------------------------
// git — scoped git ops in the worktree
// ---------------------------------------------------------------------------

/// Run a git subcommand inside the worktree.
///
/// The current working directory is forced to the worktree root.
/// Force-push (`--force`, `-f`) and history-rewrite (`--no-verify`) remain
/// blocked — the guardrails layer catches them *before* this tool executes,
/// so we do not need to duplicate the check here (defence-in-depth is the
/// guardrails + sandbox layers).
///
/// Allowed subcommands: status, diff, add, commit, branch, worktree, log,
/// show, stash (list only), fetch, pull, push (non-force).
///
/// Credentials for push are injected by the P3 auth broker; git reads them
/// from the environment.
pub struct Git;

/// Git subcommands explicitly allowed.
const GIT_ALLOWED_SUBS: &[&str] = &[
    "status",
    "diff",
    "add",
    "commit",
    "branch",
    "worktree",
    "log",
    "show",
    "stash",
    "fetch",
    "pull",
    "push",
    "checkout",
    "switch",
    "restore",
    "merge-base",
    "rev-parse",
    "ls-files",
    "tag",
];

/// Git subcommands that are never allowed (history-rewrite / destructive).
const GIT_BLOCKED_SUBS: &[&str] = &[
    "rebase",
    "filter-branch",
    "filter-repo",
    "reset",
    "clean",
    "gc",
    "reflog",
    "update-ref",
    "am",
    "apply",
];

#[async_trait]
impl ToolImpl for Git {
    fn name(&self) -> &'static str {
        "git"
    }

    fn description(&self) -> &'static str {
        "Run a scoped git operation inside the worktree. \
         `subcommand` is the git subcommand (e.g. `status`, `diff`, `add`, `commit`, `push`). \
         `args` is an optional list of arguments. \
         Force-push, `--no-verify`, history-rewrite, and destructive subcommands are blocked. \
         The working directory is always the worktree root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "description": "Git subcommand to run (e.g. 'status', 'diff', 'add', 'commit', 'push')."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of arguments to pass after the subcommand."
                }
            },
            "required": ["subcommand"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let sub = args["subcommand"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `subcommand` argument".to_string(),
            })?;

        // Tool-level allow/block list (guardrails also check at a higher level).
        if GIT_BLOCKED_SUBS.contains(&sub) {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "git subcommand `{sub}` is blocked (history-rewrite / destructive). \
                     Use the worktree model: work on feature branches, rebase manually if needed."
                ),
            });
        }
        if !GIT_ALLOWED_SUBS.contains(&sub) {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "git subcommand `{sub}` is not in the allowed list: {}",
                    GIT_ALLOWED_SUBS.join(", ")
                ),
            });
        }

        // Collect extra args.
        let extra: Vec<String> = match args["args"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            None => Vec::new(),
        };

        // Defence-in-depth: block --no-verify and force-push flags
        // (guardrails catch these first, but being explicit here is safer).
        for arg in &extra {
            if arg == "--no-verify" {
                return Err(HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: "`--no-verify` is not permitted (Surāmeraya guardrail).".to_string(),
                });
            }
            if sub == "push"
                && (arg == "--force"
                    || arg == "-f"
                    || arg == "--force-with-lease"
                    || arg == "--force-if-includes")
            {
                return Err(HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!(
                        "`git push {arg}` is not permitted (Surāmeraya guardrail). \
                         Only non-force pushes are allowed."
                    ),
                });
            }
        }

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg(sub);
        cmd.args(&extra);
        cmd.current_dir(&ctx.workdir);

        // Scrub the environment (same as sandbox.rs) — git only needs PATH +
        // identity vars.  The P3 auth broker injects GITHUB_TOKEN at exec time.
        cmd.env_clear();
        for var in &[
            "PATH",
            "HOME",
            "USER",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "SSH_AUTH_SOCK",
            "GIT_SSH_COMMAND",
            "TMPDIR",
            "TMP",
            "TEMP",
        ] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn git: {e}"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            result.push_str(&format!("\n[exit code: {code}]"));
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// run_gates — run the verification gates declared in the agent manifest
// ---------------------------------------------------------------------------

/// Read `lintCmd`, `formatCmd`, `testCmd`, `buildCmd` from the agent's
/// `config.manifest.json` and run them in order, capturing pass/fail and
/// output.  Returns a structured report.
///
/// The manifest is located by searching upward from `ctx.workdir` for
/// `config.manifest.json`.  The commands are run via `sh -c` with the
/// worktree root as `cwd`.
///
/// Source of gate definitions: `bwoc-core::manifest::Manifest` (already a dep).
pub struct RunGates;

#[async_trait]
impl ToolImpl for RunGates {
    fn name(&self) -> &'static str {
        "run_gates"
    }

    fn description(&self) -> &'static str {
        "Run the verification gates declared in the agent manifest \
         (lint/format/test/build commands). Returns pass/fail status and output \
         for each gate. Gates are read from `config.manifest.json` in the \
         working directory. Feeds gate-pass/fail counts into telemetry."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "gates": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["lint", "format", "test", "build"]
                    },
                    "description": "Which gates to run. Defaults to all four: [\"lint\", \"format\", \"test\", \"build\"]."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        // Load the manifest.
        let manifest_path = ctx.workdir.join("config.manifest.json");
        if !manifest_path.exists() {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!(
                    "config.manifest.json not found in `{}`. \
                     run_gates requires the agent manifest.",
                    ctx.workdir.display()
                ),
            });
        }

        let manifest =
            bwoc_core::manifest::Manifest::load_from_path(&manifest_path).map_err(|e| {
                HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("cannot load manifest: {e}"),
                }
            })?;

        // Decide which gates to run.
        let requested: Vec<String> = match args["gates"].as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            None => vec![
                "lint".to_string(),
                "format".to_string(),
                "test".to_string(),
                "build".to_string(),
            ],
        };

        let gate_cmds: Vec<(&str, &str)> = requested
            .iter()
            .filter_map(|g| match g.as_str() {
                "lint" => Some(("lint", manifest.lint_cmd.as_str())),
                "format" => Some(("format", manifest.format_cmd.as_str())),
                "test" => Some(("test", manifest.test_cmd.as_str())),
                "build" => Some(("build", manifest.build_cmd.as_str())),
                _ => None,
            })
            .collect();

        if gate_cmds.is_empty() {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "no valid gates specified (accepted: lint, format, test, build)"
                    .to_string(),
            });
        }

        let mut report = String::new();
        let mut all_passed = true;

        for (gate_name, cmd) in &gate_cmds {
            let output = shell_command(cmd)
                .current_dir(&ctx.workdir)
                .output()
                .await
                .map_err(|e| HarnessError::ToolExecution {
                    tool: self.name().to_string(),
                    reason: format!("failed to run gate `{gate_name}` (`{cmd}`): {e}"),
                })?;

            let passed = output.status.success();
            if !passed {
                all_passed = false;
            }

            let status_str = if passed { "PASS" } else { "FAIL" };
            report.push_str(&format!("[{gate_name}] {status_str} — `{cmd}`\n"));

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                report.push_str(&format!("  stdout: {}\n", stdout.trim()));
            }
            if !stderr.trim().is_empty() {
                report.push_str(&format!("  stderr: {}\n", stderr.trim()));
            }
            let code = output.status.code().unwrap_or(-1);
            if !passed {
                report.push_str(&format!("  exit code: {code}\n"));
            }
        }

        let summary = if all_passed {
            "ALL GATES PASSED"
        } else {
            "ONE OR MORE GATES FAILED"
        };
        report.push_str(&format!("\n{summary}\n"));

        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// bwoc_task — claim / complete / list Saṅgha tasks
// ---------------------------------------------------------------------------

/// Claim, complete, or list tasks on the team's shared task list.
///
/// **Implementation choice: shell out to `bwoc` binary.**
/// Rationale: `bwoc-core::team` owns pure data-model + state-transition
/// logic, but the file locking, JSONL persistence, and team discovery all
/// live in `bwoc-cli`.  Reimplementing that here would duplicate non-trivial
/// I/O logic *and* risk race conditions that the CLI's advisory lock prevents.
/// Shelling out to `bwoc` is the cleaner path, adds zero deps, and keeps
/// the harness decoupled from CLI internals.
///
/// The `--from` identity is the agent's own ID; the guardrails' Musāvāda
/// check (identity spoof) fires first if the model tries to inject a
/// different agent ID.
pub struct BwocTask;

#[async_trait]
impl ToolImpl for BwocTask {
    fn name(&self) -> &'static str {
        "bwoc_task"
    }

    fn description(&self) -> &'static str {
        "Claim, complete, or list Saṅgha tasks on the shared task list. \
         `action` is one of: `list`, `claim`, `complete`. \
         `task_id` is required for `claim` and `complete`. \
         `team_id` identifies the team (defaults to the first team if omitted). \
         The agent identity is verified by the safety guardrails (Musāvāda)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "claim", "complete"],
                    "description": "Task action to perform."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID to claim or complete. Required for `claim` and `complete`."
                },
                "team_id": {
                    "type": "string",
                    "description": "Team ID. If omitted, the first available team is used."
                },
                "agent_id": {
                    "type": "string",
                    "description": "The agent's own ID. Must match the harness agent identity."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let action = args["action"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `action` argument (list | claim | complete)".to_string(),
            })?;

        // Build the bwoc command: `bwoc task <action> [--task <id>] [--team <id>] [--agent <id>]`
        // Resolve `bwoc` as a sibling of the running harness, not a stale PATH copy.
        let mut cmd = tokio::process::Command::new(bwoc_core::exec::binary_or_name("bwoc"));
        cmd.arg("task").arg(action);

        if let Some(task_id) = args["task_id"].as_str() {
            cmd.arg("--task").arg(task_id);
        } else if matches!(action, "claim" | "complete") {
            return Err(HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("`task_id` is required for action `{action}`"),
            });
        }

        if let Some(team_id) = args["team_id"].as_str() {
            cmd.arg("--team").arg(team_id);
        }

        if let Some(agent_id) = args["agent_id"].as_str() {
            // The guardrails (Musāvāda) check fires before this point if
            // the model injects a spoofed identity.
            cmd.arg("--agent").arg(agent_id);
        }

        cmd.current_dir(&ctx.workdir);
        // Minimal clean env — bwoc reads its workspace from cwd.
        cmd.env_clear();
        for var in &["PATH", "HOME", "USER", "LANG", "TMPDIR"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn `bwoc`: {e}. Is `bwoc` on PATH?"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            result.push_str(&format!("\n[exit code: {code}]"));
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// bwoc_send — send an inter-agent message via the bwoc send channel
// ---------------------------------------------------------------------------

/// Send an inter-agent message.
///
/// **Implementation choice: shell out to `bwoc send`.**
/// Same rationale as `bwoc_task`: the send channel's routing, inbox locking,
/// and routing-table resolution live in `bwoc-cli`.  Shelling out is the
/// clean path and avoids duplicating that logic.
///
/// The `--from` field MUST be the agent's own ID.  The guardrails' Musāvāda
/// check fires before execute() if the model tries to inject a spoofed `from`
/// / `sender` value.
pub struct BwocSend;

#[async_trait]
impl ToolImpl for BwocSend {
    fn name(&self) -> &'static str {
        "bwoc_send"
    }

    fn description(&self) -> &'static str {
        "Send an inter-agent message to another agent via the bwoc send channel. \
         `to` is the target agent ID. `body` is the message text. \
         `from` must be this agent's own ID — identity spoofing is blocked by \
         the Musāvāda safety guardrail."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Target agent ID (e.g. 'agent-pi')."
                },
                "body": {
                    "type": "string",
                    "description": "Message body text."
                },
                "from": {
                    "type": "string",
                    "description": "Sender agent ID. Must be this agent's own identity."
                },
                "subject": {
                    "type": "string",
                    "description": "Optional message subject / title."
                }
            },
            "required": ["to", "body", "from"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let to = args["to"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `to` argument".to_string(),
            })?;
        let body = args["body"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `body` argument".to_string(),
            })?;
        let from = args["from"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `from` argument".to_string(),
            })?;

        // Sibling-of-running-harness resolution (see bwoc_core::exec).
        let mut cmd = tokio::process::Command::new(bwoc_core::exec::binary_or_name("bwoc"));
        cmd.arg("send")
            .arg("--to")
            .arg(to)
            .arg("--from")
            .arg(from)
            .arg("--body")
            .arg(body);

        if let Some(subject) = args["subject"].as_str() {
            cmd.arg("--subject").arg(subject);
        }

        cmd.current_dir(&ctx.workdir);
        cmd.env_clear();
        for var in &["PATH", "HOME", "USER", "LANG", "TMPDIR"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn `bwoc`: {e}. Is `bwoc` on PATH?"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            result.push_str(&format!("\n[exit code: {code}]"));
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// bwoc_run — launch (delegate to) another BWOC agent on a task
// ---------------------------------------------------------------------------

/// Launch another BWOC agent on a task and return its captured result.
///
/// **"ollama launches bwoc"** — the model running this harness loop can hand a
/// subtask to a *different* agent by shelling out to
/// `bwoc run <agent> --task <task> --json --timeout <n>` (headless, captured).
/// The launched agent runs its own full safety pipeline in its own process.
///
/// Same shell-out rationale as [`BwocTask`]/[`BwocSend`]: agent resolution,
/// backend selection, and the run lifecycle live in `bwoc-cli`.
///
/// **Safety posture.** Like every tool, `bwoc_run` is gated by the permission
/// layer and is **denied by default** (the policy's fail-safe `default_mode`),
/// so it only runs when an operator opts in via `.bwoc/harness-policy.toml`.
/// That same gate bounds recursion: a launched agent can only launch further
/// agents if *its* policy also allows `bwoc_run`. Each launch is additionally
/// time-bounded (`timeout_secs`, default 300) so a delegate can't hang the
/// caller indefinitely.
pub struct BwocRun;

#[async_trait]
impl ToolImpl for BwocRun {
    fn name(&self) -> &'static str {
        "bwoc_run"
    }

    fn description(&self) -> &'static str {
        "Launch (delegate to) another BWOC agent on a task and return its \
         captured result. Runs `bwoc run <agent> --task <task>` headless. \
         Use to hand a self-contained subtask to a specialist agent. \
         `agent` and `task` are required; `timeout_secs` bounds the run \
         (default 300). Requires operator opt-in (denied by default policy)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Target agent: id (`agent-foo`) or bare name (`foo`)."
                },
                "task": {
                    "type": "string",
                    "description": "The self-contained task prompt to deliver to that agent."
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Kill the launched agent and report a timeout after this many seconds. Default 300."
                }
            },
            "required": ["agent", "task"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let agent = args["agent"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `agent` argument".to_string(),
            })?;
        let task = args["task"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `task` argument".to_string(),
            })?;
        // Bounded by default so a delegate can never hang the caller forever.
        let timeout = args["timeout_secs"].as_u64().unwrap_or(300).max(1);

        // `bwoc run <agent> --task <task> --json --timeout <n>` — sibling-resolved
        // bwoc, not a stale PATH copy (see bwoc_core::exec).
        let mut cmd = tokio::process::Command::new(bwoc_core::exec::binary_or_name("bwoc"));
        cmd.arg("run")
            .arg(agent)
            .arg("--task")
            .arg(task)
            .arg("--json")
            .arg("--timeout")
            .arg(timeout.to_string());

        cmd.current_dir(&ctx.workdir);
        // Scrubbed env via the shared core scrubber (allowlist + credential
        // filter, cross-platform TMP/TEMP) — the same rule the sandbox uses;
        // bwoc reads its workspace from cwd, not the env.
        cmd.env_clear();
        cmd.envs(crate::sandbox::scrub_env());
        // Reap the child if this tool future is dropped (harness shutdown /
        // queue cancellation) so a delegated run never becomes an orphan —
        // same reason `worker.rs` sets it on spawned workers.
        cmd.kill_on_drop(true);

        let output = cmd
            .output()
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("failed to spawn `bwoc`: {e}. Is `bwoc` on PATH?"),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let mut result = stdout;
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("[stderr] ");
            result.push_str(&stderr);
        }
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            result.push_str(&format!("\n[exit code: {code}]"));
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// memory_read — read from the agent's tier-1 file-based memory store
// ---------------------------------------------------------------------------

/// Read a memory file from the agent's `memories/` directory.
///
/// If `name` is omitted, reads `MEMORY.md` (the index).
/// All paths are confined to the memories sub-directory inside the worktree.
pub struct MemoryRead;

#[async_trait]
impl ToolImpl for MemoryRead {
    fn name(&self) -> &'static str {
        "memory_read"
    }

    fn description(&self) -> &'static str {
        "Read from the agent's tier-1 file-based memory store (the configured memory \
         directory, `memoryPath` — default `memories/`). \
         If `name` is omitted, reads `MEMORY.md` (the index). \
         Paths are confined to that directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory file name within the memory directory (e.g. `feedback_over_engineering.md`). Defaults to `MEMORY.md`."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let name = args["name"].as_str().unwrap_or("MEMORY.md");
        // Tier-1 memory lives in the manifest-configured `memoryPath` (default
        // `<workdir>/memories`) — see ToolContext::memory_dir.
        let memories_dir = ctx.memory_dir.clone();
        let mem_path = memories_dir.join(name);

        // Confinement: must stay inside memories/ which is inside workdir.
        let canonical_mem_dir = super::normalize_path_pub(&memories_dir);
        let canonical_path = super::normalize_path_pub(&mem_path);
        if !canonical_path.starts_with(&canonical_mem_dir) {
            return Err(HarnessError::PathEscape(name.to_string()));
        }
        // Also ensure memories/ is inside workdir.
        if !canonical_mem_dir.starts_with(&ctx.workdir) {
            return Err(HarnessError::PathEscape("memories/".to_string()));
        }
        // Symlink-safe: a symlinked memories/ (or file) must not leave the workdir.
        if !crate::sandbox::is_confined(&canonical_path, &ctx.workdir) {
            return Err(HarnessError::PathEscape(name.to_string()));
        }

        tokio::fs::read_to_string(&mem_path)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot read memory `{name}`: {e}"),
            })
    }
}

// ---------------------------------------------------------------------------
// memory_write — write to the agent's tier-1 file-based memory store
// ---------------------------------------------------------------------------

/// Write a memory file to the agent's `memories/` directory.
///
/// Creates the file if it does not exist.  All paths are confined to the
/// memory directory (`ToolContext::memory_dir`).  The 200-line cap on
/// `MEMORY.md` is a convention (Mattaññutā): this tool does not truncate, but it
/// appends a soft warning to its result when a `MEMORY.md` write exceeds the cap
/// so the agent prunes.
pub struct MemoryWrite;

#[async_trait]
impl ToolImpl for MemoryWrite {
    fn name(&self) -> &'static str {
        "memory_write"
    }

    fn description(&self) -> &'static str {
        "Write to the agent's tier-1 file-based memory store (the configured memory \
         directory, `memoryPath` — default `memories/`). \
         Creates the file if it does not exist. Confined to that directory. \
         `MEMORY.md` is the index (capped at 200 lines by convention — Mattaññutā)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Memory file name within `memories/` (e.g. `feedback_foo.md`)."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write."
                }
            },
            "required": ["name", "content"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError> {
        let name = args["name"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `name` argument".to_string(),
            })?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: "missing `content` argument".to_string(),
            })?;

        let memories_dir = ctx.memory_dir.clone();
        let mem_path = memories_dir.join(name);

        // Confinement check.
        let canonical_mem_dir = super::normalize_path_pub(&memories_dir);
        let canonical_path = super::normalize_path_pub(&mem_path);
        if !canonical_path.starts_with(&canonical_mem_dir) {
            return Err(HarnessError::PathEscape(name.to_string()));
        }
        if !canonical_mem_dir.starts_with(&ctx.workdir) {
            return Err(HarnessError::PathEscape("memories/".to_string()));
        }
        // Symlink-safe: a symlinked memories/ (or file) must not leave the workdir.
        if !crate::sandbox::is_confined(&canonical_path, &ctx.workdir) {
            return Err(HarnessError::PathEscape(name.to_string()));
        }

        // Create the memories directory if needed.
        tokio::fs::create_dir_all(&memories_dir)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot create memories/ dir: {e}"),
            })?;

        tokio::fs::write(&mem_path, content)
            .await
            .map_err(|e| HarnessError::ToolExecution {
                tool: self.name().to_string(),
                reason: format!("cannot write memory `{name}`: {e}"),
            })?;

        // Surface the MEMORY.md 200-line cap (Mattaññutā) as a soft warning in the
        // tool result so the agent prunes — never truncate (that would silently
        // lose curated content). The audit (`bwoc check`) warns too; this makes it
        // visible at write time, where the agent can act on it.
        let mut msg = format!("memory `{name}` written ({} bytes)", content.len());
        if name == "MEMORY.md" {
            let lines = content.lines().count();
            if lines > MEMORY_MD_MAX_LINES {
                msg.push_str(&format!(
                    " — WARNING: {lines} lines exceed the {MEMORY_MD_MAX_LINES}-line cap; \
                     prune the index (Mattaññutā)"
                ));
            }
        }
        Ok(msg)
    }
}

/// Soft cap on the `MEMORY.md` index (Mattaññutā — right amount). Mirrors
/// `bwoc check`'s constant. A convention, not a hard limit: `memory_write` warns
/// past it but still writes (truncating would lose curated content).
const MEMORY_MD_MAX_LINES: usize = 200;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    fn ctx_for(dir: &TempDir) -> ToolContext {
        ToolContext::new(dir.path().to_path_buf())
    }

    // ── apply_edit (exact + whitespace-tolerant matching) ──────────────────────

    #[test]
    fn apply_edit_exact_single() {
        let out = apply_edit("a = 1\nb = 2\n", "b = 2", "b = 3");
        assert_eq!(
            out,
            EditOutcome::Replaced {
                content: "a = 1\nb = 3\n".to_string(),
                how: "exact",
            }
        );
    }

    #[test]
    fn apply_edit_exact_multi_is_ambiguous() {
        let out = apply_edit("x\nx\n", "x", "y");
        assert_eq!(out, EditOutcome::Ambiguous { count: 2 });
    }

    #[test]
    fn apply_edit_whitespace_tolerant_reindents() {
        // File uses a tab; old_string uses 4 spaces — exact match fails, the
        // tolerant pass matches by trimmed content and re-indents `new` to the
        // file's tab.
        let file = "fn f() {\n\tlet x = 1;\n}\n";
        let out = apply_edit(file, "    let x = 1;", "    let x = 2;");
        assert_eq!(
            out,
            EditOutcome::Replaced {
                content: "fn f() {\n\tlet x = 2;\n}\n".to_string(),
                how: "whitespace-tolerant",
            }
        );
    }

    #[test]
    fn apply_edit_tolerant_multiline_block() {
        let file = "a\n  foo(\n    bar,\n  )\nz\n";
        // old block with different (zero) indentation; trimmed lines still match.
        let old = "foo(\nbar,\n)";
        let new = "foo(\nbaz,\n)";
        let out = apply_edit(file, old, new);
        match out {
            EditOutcome::Replaced { content, how } => {
                assert_eq!(how, "whitespace-tolerant");
                // First matched line's indent ("  ") is applied to the new block.
                assert_eq!(content, "a\n  foo(\n  baz,\n  )\nz\n");
            }
            other => panic!("expected Replaced, got {other:?}"),
        }
    }

    #[test]
    fn apply_edit_tolerant_ambiguous_block() {
        let file = "  foo\nx\n    foo\n";
        let out = apply_edit(file, "foo", "bar");
        // Two trimmed-equal single-line blocks → ambiguous, never guesses.
        assert_eq!(out, EditOutcome::Ambiguous { count: 2 });
    }

    #[test]
    fn apply_edit_not_found() {
        assert_eq!(apply_edit("a\nb\n", "nope", "x"), EditOutcome::NotFound);
    }

    #[test]
    fn apply_edit_tolerant_trailing_newline_no_extra_blank() {
        // old/new both end in `\n`; the tolerant path must not splice an extra
        // blank line (the surrounding file already supplies the boundary).
        // old/new indented differently from the file so the exact pass misses
        // and the tolerant pass handles the trailing newline.
        let file = "a\n  foo\nz\n";
        let out = apply_edit(file, "    foo\n", "    bar\n");
        match out {
            EditOutcome::Replaced { content, how } => {
                assert_eq!(how, "whitespace-tolerant");
                assert_eq!(content, "a\n  bar\nz\n", "no blank line should appear");
            }
            other => panic!("expected Replaced, got {other:?}"),
        }

        // Multi-line new block with a trailing newline behaves the same.
        let file2 = "x\n  a\n  b\ny\n";
        let out2 = apply_edit(file2, "a\nb\n", "p\nq\n");
        match out2 {
            EditOutcome::Replaced { content, .. } => {
                assert_eq!(content, "x\n  p\n  q\ny\n");
            }
            other => panic!("expected Replaced, got {other:?}"),
        }
    }

    // ── edit_file ─────────────────────────────────────────────────────────────

    #[test]
    fn edit_file_happy_path() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            // Create a file to edit.
            tokio::fs::write(
                tmp.path().join("hello.rs"),
                "fn main() { println!(\"hello\"); }\n",
            )
            .await
            .unwrap();

            let tool = EditFile;
            let args = json!({
                "path": "hello.rs",
                "old_string": "hello",
                "new_string": "world"
            });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("replaced 1 occurrence"));

            let content = tokio::fs::read_to_string(tmp.path().join("hello.rs"))
                .await
                .unwrap();
            assert!(content.contains("world"));
            assert!(!content.contains("println!(\"hello\")"));
        });
    }

    #[test]
    fn edit_file_not_found_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = EditFile;
            let args = json!({
                "path": "missing.rs",
                "old_string": "foo",
                "new_string": "bar"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::ToolExecution { .. }));
        });
    }

    #[test]
    fn edit_file_zero_matches_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "line one\nline two\n")
                .await
                .unwrap();
            let tool = EditFile;
            let args = json!({
                "path": "f.txt",
                "old_string": "nonexistent",
                "new_string": "replacement"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not found"), "expected 'not found' in: {msg}");
        });
    }

    #[test]
    fn edit_file_multiple_matches_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "foo\nfoo\n")
                .await
                .unwrap();
            let tool = EditFile;
            let args = json!({
                "path": "f.txt",
                "old_string": "foo",
                "new_string": "bar"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("2 places"), "expected '2 places' in: {msg}");
        });
    }

    #[test]
    fn edit_file_path_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = EditFile;
            let args = json!({
                "path": "../../etc/passwd",
                "old_string": "root",
                "new_string": "hacked"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    #[tokio::test]
    async fn edit_file_replace_all() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.rs"), "old();\nold();\nkeep();\n").unwrap();
        let ctx = ctx_for(&tmp);
        let msg = EditFile
            .execute(
                json!({"path": "f.rs", "old_string": "old", "new_string": "new", "replace_all": true}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(msg.contains("replaced 2 occurrence"), "{msg}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("f.rs")).unwrap(),
            "new();\nnew();\nkeep();\n"
        );
        // replace_all is exact-only: nothing matches → error, file untouched.
        let err = EditFile
            .execute(
                json!({"path": "f.rs", "old_string": "absent", "new_string": "x", "replace_all": true}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
        // An empty old_string is refused (it would match everywhere).
        let err = EditFile
            .execute(
                json!({"path": "f.rs", "old_string": "", "new_string": "x", "replace_all": true}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
    }

    // ── multi_edit ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn multi_edit_applies_in_order() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("m.txt"), "a b\nc\n").unwrap();
        let ctx = ctx_for(&tmp);
        let msg = MultiEdit
            .execute(
                json!({"path": "m.txt", "edits": [
                    {"old_string": "a", "new_string": "x"},
                    // Sees the first edit's result.
                    {"old_string": "x b", "new_string": "y"},
                    {"old_string": "c", "new_string": "z", "replace_all": true}
                ]}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(msg.contains("applied 3 edit(s)"), "{msg}");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("m.txt")).unwrap(),
            "y\nz\n"
        );
    }

    #[tokio::test]
    async fn multi_edit_is_all_or_nothing() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("m.txt"), "one\ntwo\ntwo\n").unwrap();
        let ctx = ctx_for(&tmp);
        for edits in [
            json!([{"old_string": "one", "new_string": "1"}, {"old_string": "missing", "new_string": "x"}]),
            json!([{"old_string": "one", "new_string": "1"}, {"old_string": "two", "new_string": "2"}]),
            json!([{"old_string": "one"}]),
            json!([]),
        ] {
            let err = MultiEdit
                .execute(json!({"path": "m.txt", "edits": edits}), &ctx)
                .await
                .unwrap_err();
            assert!(matches!(err, HarnessError::ToolExecution { .. }), "{err}");
            assert_eq!(
                std::fs::read_to_string(tmp.path().join("m.txt")).unwrap(),
                "one\ntwo\ntwo\n",
                "a failed batch must leave the file untouched"
            );
        }
    }

    #[tokio::test]
    async fn multi_edit_path_escape_rejected() {
        let tmp = TempDir::new().unwrap();
        let err = MultiEdit
            .execute(
                json!({"path": "../x.txt", "edits": [{"old_string": "a", "new_string": "b"}]}),
                &ctx_for(&tmp),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, HarnessError::PathEscape(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn multi_edit_refuses_a_symlink_escape() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("t.txt"), "a").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();
        let err = MultiEdit
            .execute(
                json!({"path": "link/t.txt", "edits": [{"old_string": "a", "new_string": "b"}]}),
                &ctx_for(&tmp),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, HarnessError::PathEscape(_)));
        assert_eq!(
            std::fs::read_to_string(outside.path().join("t.txt")).unwrap(),
            "a"
        );
    }

    // ── grep ──────────────────────────────────────────────────────────────────

    #[test]
    fn grep_finds_pattern() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            tokio::fs::write(tmp.path().join("code.rs"), "fn hello() {}\nfn world() {}\n")
                .await
                .unwrap();

            let tool = Grep;
            let args = json!({ "pattern": "hello" });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("code.rs"));
            assert!(result.contains("hello"));
            assert!(!result.contains("world() {}"));
        });
    }

    #[test]
    fn grep_no_match_returns_no_matches_message() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "line one\nline two\n")
                .await
                .unwrap();
            let tool = Grep;
            let args = json!({ "pattern": "ZZZNOMATCH" });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("no matches"));
        });
    }

    #[test]
    fn grep_case_insensitive() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            tokio::fs::write(tmp.path().join("f.txt"), "Hello World\n")
                .await
                .unwrap();
            let tool = Grep;
            let args = json!({ "pattern": "hello", "case_insensitive": true });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("Hello World"));
        });
    }

    #[test]
    fn grep_path_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Grep;
            let args = json!({ "pattern": "root", "path": "../../etc" });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    #[tokio::test]
    async fn grep_accepts_a_file_path() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("one.txt"), "needle\n").unwrap();
        let out = Grep
            .execute(
                json!({"pattern": "needle", "path": "one.txt"}),
                &ctx_for(&tmp),
            )
            .await
            .unwrap();
        assert_eq!(out, "one.txt:1:needle");
    }

    #[tokio::test]
    async fn grep_regex_fixed_strings_and_invalid_fallback() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn alpha() {}\nlet x = foo(1);\n").unwrap();
        let ctx = ctx_for(&tmp);
        let out = Grep
            .execute(json!({"pattern": r"fn \w+\(\)"}), &ctx)
            .await
            .unwrap();
        assert_eq!(out, "a.rs:1:fn alpha() {}");
        // `.` is literal under fixed_strings — no line contains "o.o".
        let out = Grep
            .execute(json!({"pattern": "o.o", "fixed_strings": true}), &ctx)
            .await
            .unwrap();
        assert!(out.starts_with("no matches"), "{out}");
        // An invalid regex degrades to a literal search, with a note.
        let out = Grep
            .execute(json!({"pattern": "foo("}), &ctx)
            .await
            .unwrap();
        assert!(out.starts_with("a.rs:2:let x = foo(1);"), "{out}");
        assert!(out.contains("not a valid regex"), "{out}");
    }

    #[tokio::test]
    async fn grep_glob_filter_and_binary_skip() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "needle\n").unwrap();
        std::fs::write(tmp.path().join("a.md"), "needle\n").unwrap();
        std::fs::write(tmp.path().join("blob.bin"), b"needle\0\x01\x02").unwrap();
        let ctx = ctx_for(&tmp);
        let out = Grep
            .execute(json!({"pattern": "needle", "glob": "*.rs"}), &ctx)
            .await
            .unwrap();
        assert_eq!(out, "a.rs:1:needle");
        let out = Grep
            .execute(json!({"pattern": "needle"}), &ctx)
            .await
            .unwrap();
        assert!(!out.contains("blob.bin"), "binary skipped: {out}");
        assert!(out.contains("a.md") && out.contains("a.rs"), "{out}");
    }

    // ── glob ──────────────────────────────────────────────────────────────────

    #[test]
    fn glob_to_regex_semantics() {
        let m = |g: &str, s: &str| glob_to_regex(g).unwrap().is_match(s);
        assert!(m("*.rs", "main.rs"));
        assert!(!m("*.rs", "src/main.rs"), "`*` stays in one segment");
        assert!(m("src/**/*.rs", "src/main.rs"), "`**/` spans zero dirs");
        assert!(m("src/**/*.rs", "src/a/b/lib.rs"));
        assert!(m("?.md", "a.md") && !m("?.md", "ab.md"));
        assert!(m("*.{toml,json}", "x.json") && !m("*.{toml,json}", "x.yaml"));
        assert!(m("[ab].txt", "a.txt") && !m("[!ab].txt", "a.txt"));
        assert!(m("a+b(1).txt", "a+b(1).txt"), "regex metachars are literal");
        assert!(glob_to_regex("[]").is_err());
    }

    fn glob_fixture() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src/nested")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join("Cargo.toml"), "").unwrap();
        std::fs::write(tmp.path().join("src/main.rs"), "").unwrap();
        std::fs::write(tmp.path().join("src/nested/lib.rs"), "").unwrap();
        std::fs::write(tmp.path().join(".git/config.rs"), "").unwrap();
        tmp
    }

    #[tokio::test]
    async fn glob_by_name_and_by_path() {
        let tmp = glob_fixture();
        let ctx = ctx_for(&tmp);
        let out = Glob
            .execute(json!({"pattern": "*.rs"}), &ctx)
            .await
            .unwrap();
        assert_eq!(out, "src/main.rs\nsrc/nested/lib.rs", "hidden dir skipped");
        let out = Glob
            .execute(json!({"pattern": "nested/*.rs", "path": "src"}), &ctx)
            .await
            .unwrap();
        assert_eq!(out, "src/nested/lib.rs");
        let out = Glob
            .execute(json!({"pattern": "*.none"}), &ctx)
            .await
            .unwrap();
        assert!(out.starts_with("no files match"), "{out}");
    }

    #[tokio::test]
    async fn glob_rejects_escape_and_bad_pattern() {
        let tmp = glob_fixture();
        let ctx = ctx_for(&tmp);
        let err = Glob
            .execute(json!({"pattern": "*", "path": "../"}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, HarnessError::PathEscape(_)));
        let err = Glob
            .execute(json!({"pattern": "[]"}), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid glob"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn glob_never_lists_through_an_escaping_symlink() {
        let tmp = glob_fixture();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("secret.rs"), "").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("link")).unwrap();
        let out = Glob
            .execute(json!({"pattern": "*.rs"}), &ctx_for(&tmp))
            .await
            .unwrap();
        assert!(!out.contains("secret"), "{out}");
    }

    // ── git ───────────────────────────────────────────────────────────────────

    #[test]
    fn git_status_in_real_repo() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            // Initialise a real git repo so `git status` works.
            // HOME (Unix) / USERPROFILE (Windows) let git find its global config.
            let home_val = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_default();
            tokio::process::Command::new("git")
                .args(["init", "-b", "main"])
                .current_dir(tmp.path())
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap_or_default())
                .env("HOME", &home_val)
                .env("USERPROFILE", &home_val)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .await
                .unwrap();

            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "status" });
            let result = tool.execute(args, &ctx).await.unwrap();
            // `git status` in an empty repo should mention branch or nothing to commit.
            assert!(
                result.contains("branch")
                    || result.contains("commit")
                    || result.contains("No commits"),
                "unexpected git status output: {result}"
            );
        });
    }

    #[test]
    fn git_blocked_subcommand_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "rebase", "args": ["-i", "HEAD~3"] });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::ToolExecution { .. }));
        });
    }

    #[test]
    fn git_unknown_subcommand_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "hack" });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("not in the allowed list"), "got: {msg}");
        });
    }

    #[test]
    fn git_no_verify_rejected_by_tool_layer() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "commit", "args": ["--no-verify", "-m", "skip"] });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("--no-verify"), "got: {msg}");
        });
    }

    #[test]
    fn git_force_push_rejected_by_tool_layer() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = Git;
            let args = json!({ "subcommand": "push", "args": ["--force", "origin", "main"] });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("--force"), "got: {msg}");
        });
    }

    // ── run_gates ─────────────────────────────────────────────────────────────

    #[test]
    fn run_gates_no_manifest_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = RunGates;
            let args = json!({});
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("config.manifest.json"), "got: {msg}");
        });
    }

    #[test]
    fn run_gates_with_passing_manifest() {
        // `true` (Unix) / `exit 0` (Windows CMD) — always-passing gate command.
        #[cfg(unix)]
        let pass_cmd = "true";
        #[cfg(windows)]
        let pass_cmd = "exit 0";

        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            // Write a minimal manifest where all gate commands always pass.
            let manifest_json = serde_json::json!({
                "name": "test-agent",
                "agentId": "agent-test",
                "agentRole": "test",
                "primaryModel": "model-x",
                "memoryPath": "memories/",
                "lintCmd": pass_cmd,
                "formatCmd": pass_cmd,
                "testCmd": pass_cmd,
                "buildCmd": pass_cmd,
                "version": "2.0"
            });
            tokio::fs::write(
                tmp.path().join("config.manifest.json"),
                serde_json::to_string_pretty(&manifest_json).unwrap(),
            )
            .await
            .unwrap();

            let tool = RunGates;
            let args = json!({ "gates": ["lint", "format"] });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("PASS"), "expected PASS in: {result}");
            assert!(
                result.contains("ALL GATES PASSED"),
                "expected all passed in: {result}"
            );
        });
    }

    #[test]
    fn run_gates_with_failing_gate() {
        // `false` (Unix) / `exit 1` (Windows CMD) — always-failing gate command.
        #[cfg(unix)]
        let fail_cmd = "false";
        #[cfg(windows)]
        let fail_cmd = "exit 1";
        // pass_cmd: same as above
        #[cfg(unix)]
        let pass_cmd = "true";
        #[cfg(windows)]
        let pass_cmd = "exit 0";

        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            let manifest_json = serde_json::json!({
                "name": "test-agent",
                "agentId": "agent-test",
                "agentRole": "test",
                "primaryModel": "model-x",
                "memoryPath": "memories/",
                "lintCmd": fail_cmd,
                "formatCmd": pass_cmd,
                "testCmd": pass_cmd,
                "buildCmd": pass_cmd,
                "version": "2.0"
            });
            tokio::fs::write(
                tmp.path().join("config.manifest.json"),
                serde_json::to_string_pretty(&manifest_json).unwrap(),
            )
            .await
            .unwrap();

            let tool = RunGates;
            let args = json!({ "gates": ["lint"] });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("FAIL"), "expected FAIL in: {result}");
            assert!(result.contains("ONE OR MORE GATES FAILED"), "got: {result}");
        });
    }

    // ── bwoc_task ─────────────────────────────────────────────────────────────

    /// bwoc_task shells out to `bwoc`; in tests we verify schema + arg
    /// handling without requiring `bwoc` to be on PATH.  If bwoc is absent,
    /// the tool returns an error — that is expected behaviour.
    #[test]
    fn bwoc_task_claim_requires_task_id() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = BwocTask;
            let args = json!({ "action": "claim" }); // no task_id
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("task_id"), "got: {msg}");
        });
    }

    #[test]
    fn bwoc_task_list_does_not_require_task_id() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            // Initialise a dummy bwoc workspace so list doesn't error on missing dirs.
            // (If bwoc is on PATH this gives a real result; if not, we just get a
            //  spawn error which is the expected fallback behaviour.)
            let ctx = ctx_for(&tmp);
            let tool = BwocTask;
            let args = json!({ "action": "list" });
            // We accept either success or a spawn error (bwoc not on PATH).
            // The important invariant is that the *argument parsing* succeeds —
            // no `task_id` is required for `list`.
            let _ = tool.execute(args, &ctx).await;
        });
    }

    // ── bwoc_send ─────────────────────────────────────────────────────────────

    #[test]
    fn bwoc_send_missing_to_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = BwocSend;
            let args = json!({ "body": "hello", "from": "agent-oracle" }); // no `to`
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("missing `to`"), "got: {msg}");
        });
    }

    #[test]
    fn bwoc_send_missing_from_errors() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = BwocSend;
            let args = json!({ "to": "agent-pi", "body": "hello" }); // no `from`
            let err = tool.execute(args, &ctx).await.unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("missing `from`"), "got: {msg}");
        });
    }

    // ── memory_read ───────────────────────────────────────────────────────────

    #[test]
    fn memory_read_returns_content() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);

            // Create memories/ dir and write a memory file.
            tokio::fs::create_dir(tmp.path().join("memories"))
                .await
                .unwrap();
            tokio::fs::write(
                tmp.path().join("memories").join("MEMORY.md"),
                "# Memory Index\n",
            )
            .await
            .unwrap();

            let tool = MemoryRead;
            let args = json!({});
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("Memory Index"));
        });
    }

    #[test]
    fn memory_read_dotdot_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = MemoryRead;
            let args = json!({ "name": "../../etc/passwd" });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    // ── memory_write ──────────────────────────────────────────────────────────

    #[test]
    fn memory_write_creates_file() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = MemoryWrite;
            let args = json!({
                "name": "project_foo.md",
                "content": "# Foo\nSome project memory.\n"
            });
            let result = tool.execute(args, &ctx).await.unwrap();
            assert!(result.contains("written"));

            let content =
                tokio::fs::read_to_string(tmp.path().join("memories").join("project_foo.md"))
                    .await
                    .unwrap();
            assert!(content.contains("Foo"));
        });
    }

    #[test]
    fn memory_write_dotdot_escape_rejected() {
        Runtime::new().unwrap().block_on(async {
            let tmp = TempDir::new().unwrap();
            let ctx = ctx_for(&tmp);
            let tool = MemoryWrite;
            let args = json!({
                "name": "../../evil.sh",
                "content": "#!/bin/sh\nrm -rf /\n"
            });
            let err = tool.execute(args, &ctx).await.unwrap_err();
            assert!(matches!(err, HarnessError::PathEscape(_)));
        });
    }

    // ── Schema tests (all tools have valid JSON schema) ───────────────────────

    #[test]
    fn all_new_tools_have_schemas() {
        let tools: Vec<Box<dyn ToolImpl + Send + Sync>> = vec![
            Box::new(EditFile),
            Box::new(MultiEdit),
            Box::new(Grep),
            Box::new(Glob),
            Box::new(Git),
            Box::new(RunGates),
            Box::new(BwocTask),
            Box::new(BwocSend),
            Box::new(BwocRun),
            Box::new(MemoryRead),
            Box::new(MemoryWrite),
        ];
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "schema for `{}` is not an object",
                tool.name()
            );
            assert!(
                schema["type"].as_str() == Some("object"),
                "schema for `{}` missing type:object",
                tool.name()
            );
        }
    }

    #[tokio::test]
    async fn bwoc_run_requires_agent_and_task() {
        let ctx = ToolContext::new(std::env::temp_dir());
        // missing `agent`
        let err = BwocRun
            .execute(json!({ "task": "do the thing" }), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("agent"), "got: {err}");
        // missing `task`
        let err = BwocRun
            .execute(json!({ "agent": "agent-foo" }), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("task"), "got: {err}");
    }

    // ── tier-1 memory: honor memory_dir + cap warning ──────────────────────────

    #[tokio::test]
    async fn memory_tools_honor_configured_memory_dir() {
        // A manifest `memoryPath` other than `memories/` must be used by both
        // write and read (previously hardcoded to `memories/`).
        let tmp = TempDir::new().unwrap();
        let ctx =
            ToolContext::new(tmp.path().to_path_buf()).with_memory_dir(tmp.path().join("brain"));

        MemoryWrite
            .execute(json!({ "name": "note.md", "content": "hi" }), &ctx)
            .await
            .unwrap();
        // Written under the configured dir, not the default `memories/`.
        assert!(tmp.path().join("brain/note.md").is_file());
        assert!(!tmp.path().join("memories/note.md").exists());

        let got = MemoryRead
            .execute(json!({ "name": "note.md" }), &ctx)
            .await
            .unwrap();
        assert_eq!(got, "hi");
    }

    #[tokio::test]
    async fn memory_write_warns_over_the_memory_md_cap() {
        let tmp = TempDir::new().unwrap();
        let ctx = ctx_for(&tmp);

        // Under cap → no warning.
        let small = "line\n".repeat(10);
        let msg = MemoryWrite
            .execute(json!({ "name": "MEMORY.md", "content": small }), &ctx)
            .await
            .unwrap();
        assert!(!msg.contains("WARNING"), "got: {msg}");

        // Over cap → warning, but still written.
        let big = "line\n".repeat(MEMORY_MD_MAX_LINES + 5);
        let msg = MemoryWrite
            .execute(json!({ "name": "MEMORY.md", "content": big }), &ctx)
            .await
            .unwrap();
        assert!(msg.contains("WARNING") && msg.contains("cap"), "got: {msg}");
        assert!(tmp.path().join("memories/MEMORY.md").is_file());

        // A non-index file over 200 lines is fine (cap is MEMORY.md-only).
        let big_other = "x\n".repeat(MEMORY_MD_MAX_LINES + 5);
        let msg = MemoryWrite
            .execute(json!({ "name": "big.md", "content": big_other }), &ctx)
            .await
            .unwrap();
        assert!(!msg.contains("WARNING"), "got: {msg}");
    }
}
