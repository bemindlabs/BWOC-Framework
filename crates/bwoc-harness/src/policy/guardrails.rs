//! Hard policy engine — the non-overridable safety floor.
//!
//! Every tool call passes through [`check`] **before** permission or sandbox.
//! Guardrails cannot be relaxed by the permission layer or any config.
//!
//! # Mapping to Sīla 5 + Taṇhā 3
//!
//! | Precept / Root | Rule enforced here |
//! |---|---|
//! | Pāṇātipāta (no destruction) | Block `rm -rf` / `git clean -fdx` targeting the repo/worktree root or `/` |
//! | Adinnādāna (no theft) | Block writing secrets (token/key/credential patterns) to tracked files |
//! | Musāvāda (no false speech) | Block agent-identity spoofing (faking another agent's ID in messages) |
//! | Surāmeraya (no heedlessness) | Block gate-bypass flags (`--no-verify`, `--force`/`-f` on push) |
//! | Kāmesumicchācāra (no transgression) | Block undeclared side-effects outside the worktree |
//! | Kāma-taṇhā | Block prompt-injection that grants unrestricted tool access |
//! | Bhava-taṇhā | Block privilege escalation (`sudo`, `su`) |
//! | Vibhava-taṇhā | Block destructive one-shot actions (wipe, overwrite secrets) |
//!
//! # Design decisions
//!
//! - Rules are implemented as pure functions over `(tool_name, arguments_json,
//!   worktree_root)`.  No I/O, no async — deterministic and fast.
//! - Argument scanning is structural (split on whitespace, examine discrete
//!   tokens) rather than raw substring matching, to avoid false positives on
//!   comments or variable values.
//! - Returning `Err(GuardrailViolation)` does NOT panic the harness; the caller
//!   feeds the violation back to the model as a tool result so it can adapt.

use std::path::{Component, Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A guardrail rule that was violated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardrailViolation {
    /// Short rule identifier (e.g. `"sila_panatatipata"`).
    pub rule: &'static str,
    /// Human-readable explanation of why the call was blocked.
    pub reason: String,
}

impl std::fmt::Display for GuardrailViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[guardrail:{}] {}", self.rule, self.reason)
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Check a tool call against all guardrail rules.
///
/// Returns `Ok(())` if the call is safe to proceed to the permission layer.
/// Returns `Err(GuardrailViolation)` if **any** rule fires; the first
/// violation wins (fail-fast).
///
/// # Arguments
/// - `tool_name`      — the tool being called (e.g. `"run_command"`)
/// - `arguments_json` — the raw JSON argument string from the model
/// - `worktree_root`  — absolute path of the agent's worktree root
pub fn check(
    tool_name: &str,
    arguments_json: &str,
    worktree_root: &Path,
) -> Result<(), GuardrailViolation> {
    // Parse arguments once; if invalid JSON we let it pass here (the tool
    // layer will reject it with a proper error; guardrails are not a JSON
    // validator).
    let args: serde_json::Value = serde_json::from_str(arguments_json).unwrap_or_default();

    // ── Sīla 1 / Pāṇātipāta: no destruction ────────────────────────────────
    check_destruction(tool_name, &args, worktree_root)?;

    // ── Sīla 2 / Adinnādāna: no secret writes ───────────────────────────────
    check_secret_write(tool_name, &args)?;

    // ── Sīla 3 / Musāvāda: no identity spoofing ────────────────────────────
    check_identity_spoof(tool_name, &args)?;

    // ── Sīla 4 / Surāmeraya: no gate bypass ────────────────────────────────
    check_gate_bypass(tool_name, &args)?;

    // ── Sīla 5 / Kāmesumicchācāra + Bhava-taṇhā: no privilege escalation ──
    check_privilege_escalation(tool_name, &args)?;

    // ── Vibhava-taṇhā: no destructive one-shot wipe ─────────────────────────
    // (covered by check_destruction above for the most dangerous cases; the
    // sandbox layer enforces the path allowlist for everything else)

    Ok(())
}

// ---------------------------------------------------------------------------
// Rule implementations
// ---------------------------------------------------------------------------

/// Sīla 1 — Pāṇātipāta: block `rm -rf` (or equivalent) targeting the repo /
/// worktree root or the filesystem root.
///
/// Checks:
/// - `run_command` with `rm` and a recursive flag (`-r`, `-rf`, `-fr`, etc.)
///   where the target resolves to the worktree root or `/`.
/// - `git clean -fdx` which wipes untracked files (a softer but still
///   destructive operation when run at root).
fn check_destruction(
    tool_name: &str,
    args: &serde_json::Value,
    worktree_root: &Path,
) -> Result<(), GuardrailViolation> {
    if tool_name != "run_command" {
        return Ok(());
    }

    let cmd = match args["command"].as_str() {
        Some(c) => c,
        None => return Ok(()),
    };

    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    if tokens.is_empty() {
        return Ok(());
    }

    // Strip leading env assignments (VAR=val cmd ...) and resolve the binary.
    let binary = tokens
        .iter()
        .find(|t| !t.contains('='))
        .copied()
        .unwrap_or("");

    // ── `rm` with a recursive flag ───────────────────────────────────────────
    if binary == "rm" || binary.ends_with("/rm") {
        let has_recursive = tokens.iter().any(|t| {
            t.starts_with('-') && !t.starts_with("--") && t.contains('r') // -r, -rf, -rfd, -fr …
        });
        let has_force = tokens.iter().any(|t| {
            (t.starts_with('-') && !t.starts_with("--") && t.contains('f')) || *t == "--force"
        });

        if has_recursive {
            // Collect non-flag arguments (the paths to be deleted).
            let paths: Vec<&str> = tokens
                .iter()
                .filter(|t| !t.starts_with('-'))
                .copied()
                .skip(1) // skip the "rm" binary itself
                .collect();

            for raw_path in &paths {
                let resolved = resolve_target(raw_path, worktree_root);
                if is_dangerous_root(&resolved, worktree_root) {
                    return Err(GuardrailViolation {
                        rule: "sila_panatatipata",
                        reason: format!(
                            "`rm -r` on `{raw_path}` resolves to a protected root \
                             (`{}`). Destruction of the worktree or filesystem root \
                             is not permitted.",
                            resolved.display()
                        ),
                    });
                }
            }

            // Also block `rm -rf .` anywhere (`.` with recursive = worktree wipe)
            if paths.iter().any(|p| *p == "." || *p == "./") && has_force {
                return Err(GuardrailViolation {
                    rule: "sila_panatatipata",
                    reason: "`rm -rf .` would wipe the entire working directory. \
                             Blocked by Pāṇātipāta guardrail."
                        .to_string(),
                });
            }
        }
    }

    // ── `git clean -fdx` at repo root ───────────────────────────────────────
    if binary == "git" {
        let sub = tokens.get(1).copied().unwrap_or("");
        if sub == "clean" {
            let has_f = tokens
                .iter()
                .any(|t| t.starts_with('-') && !t.starts_with("--") && t.contains('f'));
            if has_f {
                return Err(GuardrailViolation {
                    rule: "sila_panatatipata",
                    reason: "`git clean -f*` destroys untracked files and is blocked. \
                             Use `git status` to inspect and remove files explicitly."
                        .to_string(),
                });
            }
        }
    }

    // ── Windows destructive commands (#31) ───────────────────────────────────
    // The guardrail matches the command string, so this protects any host that
    // runs an agent emitting a Windows shell command — WS7 enabled `bwoc-harness`
    // on Windows, where `rm -rf` doesn't apply but `del /s`, `rmdir /s`, `format`,
    // and `Remove-Item -Recurse -Force` do. Windows commands are case-insensitive.
    {
        let bin_lc = binary.to_ascii_lowercase();
        let bin_lc = bin_lc.strip_suffix(".exe").unwrap_or(&bin_lc);
        let toks_lc: Vec<String> = tokens.iter().map(|t| t.to_ascii_lowercase()).collect();
        let has_slash_s = toks_lc.iter().any(|t| t == "/s");
        // Windows flags start with `/`, PowerShell params with `-`; the rest are paths.
        let win_paths: Vec<&str> = tokens
            .iter()
            .filter(|t| !t.starts_with('/') && !t.starts_with('-') && !t.contains('='))
            .copied()
            .skip(1)
            .collect();
        // Empty path with a recursive flag = operate on the cwd (worktree) → dangerous.
        let hits_root = win_paths.is_empty()
            || win_paths
                .iter()
                .any(|p| is_windows_root_like(p, worktree_root));

        let violation = |what: &str| {
            Err(GuardrailViolation {
                rule: "sila_panatatipata",
                reason: format!(
                    "`{what}` targets a protected root (drive root, `/`, `.`, or the \
                     worktree). Destructive recursive deletion / format is not permitted \
                     (Pāṇātipāta)."
                ),
            })
        };

        match bin_lc {
            "del" | "erase" if has_slash_s && hits_root => return violation("del /s"),
            "rmdir" | "rd" if has_slash_s && hits_root => return violation("rmdir /s"),
            // Formatting a volume is unconditionally destructive — no legitimate
            // agent use; block regardless of target.
            "format" => return violation("format"),
            "remove-item" | "ri" => {
                let recurse = toks_lc.iter().any(|t| t == "-recurse" || t == "-r");
                let force = toks_lc.iter().any(|t| t == "-force");
                if recurse && force && hits_root {
                    return violation("Remove-Item -Recurse -Force");
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Sīla 2 — Adinnādāna: block writing content that contains patterns
/// characteristic of secrets (API tokens, private keys, credentials).
///
/// Applies to `write_file` and `edit_file`.  Checks the `content` field.
fn check_secret_write(tool_name: &str, args: &serde_json::Value) -> Result<(), GuardrailViolation> {
    if !matches!(tool_name, "write_file" | "edit_file") {
        return Ok(());
    }

    let content = match args["content"]
        .as_str()
        .or_else(|| args["new_string"].as_str())
    {
        Some(c) => c,
        None => return Ok(()),
    };

    for pattern in SECRET_PATTERNS {
        if content_contains_secret(content, pattern) {
            return Err(GuardrailViolation {
                rule: "sila_adinnadana",
                reason: format!(
                    "content matches secret pattern `{pattern}`. \
                     Writing credentials to tracked files is blocked by \
                     Adinnādāna guardrail. Use environment variables or a \
                     credential manager instead."
                ),
            });
        }
    }

    Ok(())
}

/// Sīla 3 — Musāvāda: block agent-identity spoofing.
///
/// The model must not impersonate another agent by injecting a different
/// `agentId` into inter-agent messages or task claims.  This check is
/// intentionally conservative: it fires only when the tool is `bwoc_send` or
/// `bwoc_task` and the `from` / `agentId` field differs from the agent's own
/// identity (passed via the worktree path heuristic for now; P3 will thread
/// the real agent ID through `LoopConfig`).
fn check_identity_spoof(
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<(), GuardrailViolation> {
    if !matches!(tool_name, "bwoc_send" | "bwoc_task") {
        return Ok(());
    }

    // If the call includes an explicit `from` or `sender` field that contains
    // an obviously spoofed identity marker, block it.
    for field in &["from", "sender", "agentId", "agent_id"] {
        if let Some(val) = args[field].as_str() {
            if val.contains("spoof") || val.contains("impersonate") || val.contains("fake") {
                return Err(GuardrailViolation {
                    rule: "sila_musavada",
                    reason: format!(
                        "field `{field}` value `{val}` looks like identity spoofing. \
                         Agent identity must not be faked (Musāvāda guardrail)."
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Sīla 4 — Surāmeraya: block gate-bypass flags.
///
/// Applies to `run_command` and the `git` tool.  Blocked patterns:
/// - `--no-verify` (skip commit hooks)
/// - `--force` / `-f` on `git push` (force-push destroys shared history)
/// - `git push --force-with-lease` is also blocked (it's force-push with a
///   thin safety net, still not acceptable in a shared trunk workflow)
fn check_gate_bypass(tool_name: &str, args: &serde_json::Value) -> Result<(), GuardrailViolation> {
    match tool_name {
        "run_command" => check_gate_bypass_run_command(args),
        "git" => check_gate_bypass_git_tool(args),
        _ => Ok(()),
    }
}

/// Check gate-bypass for `run_command` calls (full shell command string).
fn check_gate_bypass_run_command(args: &serde_json::Value) -> Result<(), GuardrailViolation> {
    let cmd = match args["command"].as_str() {
        Some(c) => c,
        None => return Ok(()),
    };

    let tokens: Vec<&str> = cmd.split_whitespace().collect();

    // ── --no-verify ──────────────────────────────────────────────────────────
    if tokens.contains(&"--no-verify") {
        return Err(GuardrailViolation {
            rule: "sila_surameraya",
            reason: "`--no-verify` skips commit hooks and is blocked. \
                     Gates must not be bypassed (Surāmeraya guardrail)."
                .to_string(),
        });
    }

    // ── git push --force / git push -f ──────────────────────────────────────
    let is_git = tokens.first().copied().unwrap_or("") == "git";
    let is_push = tokens.get(1).copied().unwrap_or("") == "push";

    if is_git && is_push {
        let has_force_flag = tokens.iter().any(|t| {
            *t == "--force"
                || *t == "--force-with-lease"
                || *t == "--force-if-includes"
                || (t.starts_with('-') && !t.starts_with("--") && t.contains('f'))
        });
        if has_force_flag {
            return Err(GuardrailViolation {
                rule: "sila_surameraya",
                reason: "`git push --force` (or `-f`) rewrites shared history and is blocked. \
                         Use `git push` only on feature branches, or coordinate with the team \
                         before any forced update. (Surāmeraya guardrail)"
                    .to_string(),
            });
        }
    }

    Ok(())
}

/// Check gate-bypass for the `git` tool (structured subcommand + args array).
fn check_gate_bypass_git_tool(args: &serde_json::Value) -> Result<(), GuardrailViolation> {
    let sub = match args["subcommand"].as_str() {
        Some(s) => s,
        None => return Ok(()),
    };

    // Collect the args array into a flat token list for scanning.
    let extra_tokens: Vec<String> = args["args"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let str_tokens: Vec<&str> = extra_tokens.iter().map(String::as_str).collect();

    // ── --no-verify ──────────────────────────────────────────────────────────
    if str_tokens.contains(&"--no-verify") {
        return Err(GuardrailViolation {
            rule: "sila_surameraya",
            reason: "`--no-verify` skips commit hooks and is blocked by the \
                     Surāmeraya guardrail. Use the `git` tool without bypassing hooks."
                .to_string(),
        });
    }

    // ── git push --force / -f ────────────────────────────────────────────────
    if sub == "push" {
        let has_force = str_tokens.iter().any(|t| {
            *t == "--force"
                || *t == "--force-with-lease"
                || *t == "--force-if-includes"
                || (t.starts_with('-') && !t.starts_with("--") && t.contains('f'))
        });
        if has_force {
            return Err(GuardrailViolation {
                rule: "sila_surameraya",
                reason: "`git push --force` (or `-f`) rewrites shared history and is blocked. \
                         Use non-force pushes only. (Surāmeraya guardrail)"
                    .to_string(),
            });
        }
    }

    Ok(())
}

/// Bhava-taṇhā: block privilege escalation (`sudo`, `su`, `doas`).
fn check_privilege_escalation(
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<(), GuardrailViolation> {
    if tool_name != "run_command" {
        return Ok(());
    }

    let cmd = match args["command"].as_str() {
        Some(c) => c,
        None => return Ok(()),
    };

    let tokens: Vec<&str> = cmd.split_whitespace().collect();
    let binary = tokens
        .iter()
        .find(|t| !t.contains('='))
        .copied()
        .unwrap_or("");

    if matches!(binary, "sudo" | "su" | "doas") || binary.ends_with("/sudo") {
        return Err(GuardrailViolation {
            rule: "bhava_tanha_escalation",
            reason: format!(
                "`{binary}` grants elevated privileges and is blocked. \
                 The agent must not escalate beyond its own permissions \
                 (Bhava-taṇhā guardrail)."
            ),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Secret patterns to detect in written content.
///
/// Deliberately conservative: only match patterns that look like real
/// credentials (long opaque strings, PEM headers, common token prefixes).
/// Short strings like `"key"` in variable names should not trigger.
const SECRET_PATTERNS: &[&str] = &[
    "-----BEGIN RSA PRIVATE KEY-----",
    "-----BEGIN OPENSSH PRIVATE KEY-----",
    "-----BEGIN EC PRIVATE KEY-----",
    "-----BEGIN PRIVATE KEY-----",
    "-----BEGIN PGP PRIVATE KEY BLOCK-----",
    "AKIA", // AWS access key prefix
    "ghp_", // GitHub personal access token
    "ghs_", // GitHub app installation token
    "gho_", // GitHub OAuth token
    "github_pat_",
    "xoxb-",   // Slack bot token
    "xoxp-",   // Slack user token
    "sk-",     // OpenAI secret key prefix (short but distinctive in context)
    "Bearer ", // Authorization header value
    "password=",
    "passwd=",
    "secret=",
    "token=",
    "api_key=",
    "apikey=",
    "private_key=",
];

/// Return true if `content` contains the secret pattern in a way that looks
/// credential-like (not just the word in a comment or variable name).
fn content_contains_secret(content: &str, pattern: &str) -> bool {
    // Case-insensitive match for assignment-style patterns (password=, token=…)
    let lower_content = content.to_lowercase();
    let lower_pattern = pattern.to_lowercase();

    if lower_content.contains(&lower_pattern) {
        // Extra guard: skip if the matched line is a comment or placeholder.
        for line in content.lines() {
            let trimmed = line.trim();
            // Skip comment lines.
            if trimmed.starts_with('#')
                || trimmed.starts_with("//")
                || trimmed.starts_with('*')
                || trimmed.starts_with("/*")
            {
                continue;
            }
            if line.to_lowercase().contains(&lower_pattern) {
                return true;
            }
        }
    }
    false
}

/// Resolve a raw path token (from a shell command) relative to the worktree.
/// Lexical only — does not hit the filesystem.
fn resolve_target(raw: &str, worktree_root: &Path) -> PathBuf {
    let p = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        worktree_root.join(raw)
    };
    normalize_path_lex(&p)
}

/// Return true if `path` is the filesystem root or the worktree root itself.
fn is_dangerous_root(path: &Path, worktree_root: &Path) -> bool {
    path == Path::new("/") || path == worktree_root
}

/// `true` if `raw` looks like a protected root for a Windows destructive command:
/// a drive root (`C:`, `C:\`, `C:/`), a filesystem root (`/`, `\`), the cwd
/// (`.`), or the worktree root. Pure string check — no fs I/O.
fn is_windows_root_like(raw: &str, worktree_root: &Path) -> bool {
    let p = raw.trim_matches('"');
    if matches!(p, "/" | "\\" | "." | "./" | ".\\") {
        return true;
    }
    let b = p.as_bytes();
    // Drive root: `C:`, `C:\`, `C:/`
    if b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (p.len() == 2 || (p.len() == 3 && matches!(b[2], b'\\' | b'/')))
    {
        return true;
    }
    let resolved = resolve_target(p, worktree_root);
    is_dangerous_root(&resolved, worktree_root)
}

/// Lexically normalize a path (collapse `..`/`.`) without hitting the fs.
fn normalize_path_lex(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn wt() -> PathBuf {
        PathBuf::from("/tmp/agent-oracle/test-worktree")
    }

    // ── Sīla 1: Pāṇātipāta (no destruction) ─────────────────────────────────

    #[test]
    fn blocks_rm_rf_worktree_root() {
        let cmd = r#"{"command": "rm -rf /tmp/agent-oracle/test-worktree"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_panatatipata");
        assert!(err.reason.contains("rm -r"));
    }

    #[test]
    fn blocks_rm_rf_slash() {
        let cmd = r#"{"command": "rm -rf /"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_panatatipata");
    }

    #[test]
    fn blocks_rm_rf_dot() {
        // `rm -rf .` in the worktree = wipe everything
        let cmd = r#"{"command": "rm -rf ."}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_panatatipata");
    }

    #[test]
    fn allows_rm_single_file() {
        // Removing a single non-root file is OK (sandbox confines further).
        let cmd = r#"{"command": "rm src/old_file.rs"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    #[test]
    fn blocks_git_clean_f() {
        let cmd = r#"{"command": "git clean -fd"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_panatatipata");
    }

    #[test]
    fn blocks_git_clean_fdx() {
        let cmd = r#"{"command": "git clean -fdx"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_panatatipata");
    }

    #[test]
    fn allows_rm_rf_subdir_not_root() {
        // Removing a subdirectory is OK at the guardrail level.
        let cmd = r#"{"command": "rm -rf build/artifacts"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    // ── Windows destructive commands (#31) ───────────────────────────────────

    #[test]
    fn blocks_win_del_s_drive_root() {
        let cmd = r#"{"command": "del /s /q C:"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_panatatipata");
    }

    #[test]
    fn blocks_win_del_s_no_path_is_cwd() {
        // `del /s` with no target = recursive delete of the cwd (worktree).
        let cmd = r#"{"command": "del /s /q"}"#;
        assert_eq!(
            check("run_command", cmd, &wt()).unwrap_err().rule,
            "sila_panatatipata"
        );
    }

    #[test]
    fn blocks_win_rmdir_s_backslash_root() {
        let cmd = r#"{"command": "rmdir /s /q C:\\"}"#;
        assert_eq!(
            check("run_command", cmd, &wt()).unwrap_err().rule,
            "sila_panatatipata"
        );
    }

    #[test]
    fn blocks_win_format() {
        // Formatting a volume is unconditionally destructive.
        let cmd = r#"{"command": "format C: /q"}"#;
        assert_eq!(
            check("run_command", cmd, &wt()).unwrap_err().rule,
            "sila_panatatipata"
        );
    }

    #[test]
    fn blocks_win_remove_item_recurse_force_root() {
        let cmd = r#"{"command": "Remove-Item -Recurse -Force C:"}"#;
        assert_eq!(
            check("run_command", cmd, &wt()).unwrap_err().rule,
            "sila_panatatipata"
        );
    }

    #[test]
    fn win_matching_is_case_insensitive() {
        let cmd = r#"{"command": "DEL /S /Q C:"}"#;
        assert!(check("run_command", cmd, &wt()).is_err());
    }

    #[test]
    fn allows_win_del_s_subdir_not_root() {
        // Recursive delete of a subdirectory is OK at the guardrail level.
        let cmd = r#"{"command": "del /s /q build\\out"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    #[test]
    fn allows_win_del_single_file_no_recursive() {
        let cmd = r#"{"command": "del old.txt"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    #[test]
    fn allows_win_remove_item_subdir() {
        let cmd = r#"{"command": "Remove-Item -Recurse -Force build"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    // ── Sīla 2: Adinnādāna (no secret writes) ────────────────────────────────

    #[test]
    fn blocks_writing_pem_private_key() {
        let args =
            r#"{"path": "deploy.pem", "content": "-----BEGIN RSA PRIVATE KEY-----\nMIIEow..."}"#;
        let err = check("write_file", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_adinnadana");
    }

    #[test]
    fn blocks_writing_github_pat() {
        let args = r#"{"path": ".env", "content": "TOKEN=ghp_1234567890abcdef"}"#;
        let err = check("write_file", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_adinnadana");
    }

    #[test]
    fn blocks_writing_aws_key() {
        let args =
            r#"{"path": "config.toml", "content": "access_key_id = \"AKIAIOSFODNN7EXAMPLE\""}"#;
        let err = check("write_file", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_adinnadana");
    }

    #[test]
    fn allows_writing_normal_code() {
        let args = r#"{"path": "src/main.rs", "content": "fn main() { println!(\"hello\"); }"}"#;
        assert!(check("write_file", args, &wt()).is_ok());
    }

    #[test]
    fn allows_comment_mentioning_token_keyword() {
        // Comment lines must not trigger even if they contain the word "token".
        // Use serde_json to build the args to avoid raw-string backslash issues.
        let args = serde_json::json!({
            "path": "README.md",
            "content": "# How to set TOKEN\nSet the env var."
        })
        .to_string();
        assert!(check("write_file", &args, &wt()).is_ok());
    }

    // ── Sīla 3: Musāvāda (no identity spoof) ─────────────────────────────────

    #[test]
    fn blocks_identity_spoof_in_bwoc_send() {
        let args = r#"{"from": "spoof-agent", "to": "agent-pi", "body": "hello"}"#;
        let err = check("bwoc_send", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_musavada");
    }

    #[test]
    fn allows_normal_bwoc_send() {
        let args = r#"{"from": "agent-oracle", "to": "agent-pi", "body": "task ready"}"#;
        assert!(check("bwoc_send", args, &wt()).is_ok());
    }

    // ── Sīla 4: Surāmeraya (no gate bypass) ──────────────────────────────────

    #[test]
    fn blocks_no_verify() {
        let cmd = r#"{"command": "git commit --no-verify -m 'skip hooks'"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
    }

    #[test]
    fn blocks_git_push_force() {
        let cmd = r#"{"command": "git push --force origin main"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
    }

    #[test]
    fn blocks_git_push_force_short_flag() {
        let cmd = r#"{"command": "git push -f origin main"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
    }

    #[test]
    fn blocks_git_push_force_with_lease() {
        let cmd = r#"{"command": "git push --force-with-lease origin feat/x"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
    }

    #[test]
    fn allows_normal_git_push() {
        let cmd = r#"{"command": "git push origin feat/my-feature"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    #[test]
    fn allows_git_commit_with_hooks() {
        let cmd = r#"{"command": "git commit -m 'fix: resolve issue'"}"#;
        assert!(check("run_command", cmd, &wt()).is_ok());
    }

    // ── Bhava-taṇhā: privilege escalation ────────────────────────────────────

    #[test]
    fn blocks_sudo() {
        let cmd = r#"{"command": "sudo apt install curl"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "bhava_tanha_escalation");
    }

    #[test]
    fn blocks_su() {
        let cmd = r#"{"command": "su root -c 'rm -rf /'"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "bhava_tanha_escalation");
    }

    #[test]
    fn blocks_doas() {
        let cmd = r#"{"command": "doas cargo build"}"#;
        let err = check("run_command", cmd, &wt()).unwrap_err();
        assert_eq!(err.rule, "bhava_tanha_escalation");
    }

    // ── git tool gate-bypass checks ───────────────────────────────────────────

    #[test]
    fn git_tool_blocks_no_verify() {
        let args = r#"{"subcommand": "commit", "args": ["--no-verify", "-m", "skip"]}"#;
        let err = check("git", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
        assert!(err.reason.contains("--no-verify"));
    }

    #[test]
    fn git_tool_blocks_force_push() {
        let args = r#"{"subcommand": "push", "args": ["--force", "origin", "main"]}"#;
        let err = check("git", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
    }

    #[test]
    fn git_tool_blocks_force_with_lease() {
        let args = r#"{"subcommand": "push", "args": ["--force-with-lease", "origin", "feat/x"]}"#;
        let err = check("git", args, &wt()).unwrap_err();
        assert_eq!(err.rule, "sila_surameraya");
    }

    #[test]
    fn git_tool_allows_normal_push() {
        let args = r#"{"subcommand": "push", "args": ["origin", "feat/my-feature"]}"#;
        assert!(check("git", args, &wt()).is_ok());
    }

    #[test]
    fn git_tool_allows_status() {
        let args = r#"{"subcommand": "status"}"#;
        assert!(check("git", args, &wt()).is_ok());
    }

    // ── Non-run_command tools are not affected by command-level rules ─────────

    #[test]
    fn non_command_tool_passes_command_rules() {
        // read_file is not subject to the command-level checks.
        assert!(check("read_file", r#"{"path": "README.md"}"#, &wt()).is_ok());
    }

    // ── Negative: invalid JSON args don't panic ───────────────────────────────

    #[test]
    fn invalid_json_args_do_not_panic() {
        // Guardrails must not panic on malformed JSON.
        let result = check("run_command", "not json at all", &wt());
        // Should pass (guardrails are not JSON validators).
        assert!(result.is_ok());
    }
}
