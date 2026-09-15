//! Thin shell-out layer over `git worktree` and `git branch`.
//!
//! **Design constraint**: no `git2` / `gitoxide` dependency — all operations
//! go through `std::process::Command`, matching the existing process-spawn
//! style in `spawn.rs`. This keeps the binary small and dependency-free on
//! any platform where `git` is on PATH.
//!
//! # Scope (Phase 3 Track B)
//!
//! Only the operations needed by B2 (worktree creation at `task-claimed`)
//! and the future converge step (`bwoc retire` Step 3 cleanup) are
//! implemented. Do not add operations that have no consumer yet
//! (Mattaññutā).
//!
//! # Conventions
//!
//! Worktrees created by BWOC follow the path convention:
//! `<worktreeBase>/<agentId>/<taskId>`
//!
//! Branches follow the naming convention:
//! `agent/<agentId>/feat/<taskId>`
//!
//! Both conventions are filesystem-deterministic — retire Step 3 can
//! reconstruct the worktree path and branch name from the registry
//! (`agentId`) and the Saṅgha task id, without parsing any agent-written
//! log. This keeps the Saṅgha task list (coordination) and the per-agent
//! `task-log.jsonl` (execution log) as separate systems (Anattā).
//!
//! `bwoc retire` Step 3 is now wired: worktree cleanup + branch release
//! are called from `retire.rs`. Dead-code suppression removed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Error type for git shell-out failures.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// `git` is not on PATH, or the OS refused to exec it.
    #[error("git not found or could not be exec'd: {0}")]
    NotFound(#[source] std::io::Error),
    /// `git` ran but exited non-zero. Contains the combined stderr output.
    #[error("git command failed (exit {code}): {stderr}")]
    Failed { code: i32, stderr: String },
}

// --- internal helper -------------------------------------------------------

/// Run a `git` sub-command with the given args; return stdout on success,
/// `GitError` on exec failure or non-zero exit.
fn git(args: &[&str]) -> Result<String, GitError> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(GitError::NotFound)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(GitError::Failed {
            code: out.status.code().unwrap_or(-1),
            stderr,
        })
    }
}

// --- worktree operations ---------------------------------------------------

/// List all registered worktrees.
///
/// Equivalent to: `git worktree list --porcelain`
///
/// Returns one [`WorktreeEntry`] per worktree (including the main one).
pub fn worktree_list() -> Result<Vec<WorktreeEntry>, GitError> {
    let raw = git(&["worktree", "list", "--porcelain"])?;
    Ok(parse_worktree_list(&raw))
}

/// Remove a registered worktree.
///
/// Equivalent to: `git worktree remove <path>`
///
/// Note: `git worktree remove` refuses to remove a worktree with uncommitted
/// changes unless `--force` is passed. We never pass `--force` here — the
/// caller must clean up or commit first (Sīla: no silent destruction).
pub fn worktree_remove(path: &Path) -> Result<(), GitError> {
    let path_str = path.to_string_lossy();
    git(&["worktree", "remove", &path_str])?;
    Ok(())
}

/// A single entry from `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: Option<String>, // None for detached HEAD
    pub head: String,           // commit hash
    pub is_bare: bool,
}

/// Parse the `--porcelain` output of `git worktree list`.
///
/// Each worktree stanza is separated by a blank line:
/// ```text
/// worktree /abs/path
/// HEAD <sha>
/// branch refs/heads/<branch>   (or "detached")
/// bare                          (optional — only if bare)
/// ```
fn parse_worktree_list(raw: &str) -> Vec<WorktreeEntry> {
    let mut result = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut head = String::new();
    let mut branch: Option<String> = None;
    let mut is_bare = false;

    for line in raw.lines() {
        if line.is_empty() {
            // Stanza boundary — flush current entry.
            if let Some(p) = path.take() {
                result.push(WorktreeEntry {
                    path: p,
                    branch: branch.take(),
                    head: std::mem::take(&mut head),
                    is_bare,
                });
            }
            is_bare = false;
        } else if let Some(rest) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            head = rest.to_owned();
        } else if let Some(rest) = line.strip_prefix("branch ") {
            // "refs/heads/main" → "main" (strip the refs/heads/ prefix)
            branch = Some(rest.strip_prefix("refs/heads/").unwrap_or(rest).to_owned());
        } else if line == "bare" {
            is_bare = true;
        }
        // "detached" line → branch stays None
    }
    // Flush last stanza (no trailing blank line in git output).
    if let Some(p) = path {
        result.push(WorktreeEntry {
            path: p,
            branch,
            head,
            is_bare,
        });
    }
    result
}

// --- branch operations -----------------------------------------------------

/// List branches whose name matches a glob pattern.
///
/// Equivalent to: `git branch --list '<glob>'`
///
/// Returns bare branch names (no `refs/heads/` prefix). The glob is
/// passed directly to git — use `*` as the wildcard.
///
/// # Example
///
/// ```no_run
/// use bwoc_cli::git_worktree::branch_list_glob;
/// let branches = branch_list_glob("agent/agent-pi/*").unwrap();
/// ```
pub fn branch_list_glob(glob: &str) -> Result<Vec<String>, GitError> {
    let raw = git(&["branch", "--list", glob])?;
    Ok(raw
        .lines()
        .map(|l| l.trim_start_matches("* ").trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Delete a local branch by name.
///
/// Equivalent to: `git branch -d <branch>`
///
/// Uses `-d` (safe delete — refuses if not fully merged). Returns
/// `GitError::Failed` when the branch has unmerged commits; the caller
/// should surface that and offer the user a chance to review before
/// escalating to [`branch_delete_force`].
pub fn branch_delete(branch: &str) -> Result<(), GitError> {
    git(&["branch", "-d", branch])?;
    Ok(())
}

/// Force-delete a local branch by name, even if it has unmerged commits.
///
/// Equivalent to: `git branch -D <branch>`
///
/// This is the explicit escalation path. Callers MUST surface a warning
/// to the user before calling this (Sīla: no silent destruction of work).
/// `bwoc retire` uses this only when `-d` fails and surfaces the branch
/// name + a "forced" label in its output so the operator is informed.
pub fn branch_delete_force(branch: &str) -> Result<(), GitError> {
    git(&["branch", "-D", branch])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_worktree_list -----------------------------------------------

    #[test]
    fn parse_empty_output() {
        assert!(parse_worktree_list("").is_empty());
    }

    #[test]
    fn parse_single_worktree() {
        let raw = "worktree /home/user/proj\nHEAD abc123\nbranch refs/heads/main\n\n";
        let entries = parse_worktree_list(raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, PathBuf::from("/home/user/proj"));
        assert_eq!(entries[0].head, "abc123");
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(!entries[0].is_bare);
    }

    #[test]
    fn parse_detached_head() {
        let raw = "worktree /tmp/wt\nHEAD deadbeef\ndetached\n";
        let entries = parse_worktree_list(raw);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].branch.is_none());
    }

    #[test]
    fn parse_multiple_worktrees() {
        let raw = concat!(
            "worktree /main\nHEAD aaa\nbranch refs/heads/main\n\n",
            "worktree /tmp/task1\nHEAD bbb\nbranch refs/heads/agent/pi/feat/t1\n\n",
            "worktree /tmp/task2\nHEAD ccc\ndetached\n",
        );
        let entries = parse_worktree_list(raw);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1].branch.as_deref(), Some("agent/pi/feat/t1"));
        assert!(entries[2].branch.is_none());
    }

    #[test]
    fn parse_bare_worktree() {
        let raw = "worktree /srv/bare.git\nHEAD 000000\nbare\n";
        let entries = parse_worktree_list(raw);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_bare);
    }

    // ---- branch_list_glob (unit: output parsing via workaround) -----------
    // We can't shell out to git in unit tests without a real repo, so the
    // live git commands (worktree_list, worktree_remove,
    // branch_list_glob, branch_delete) are integration-verified by the
    // overall `cargo build --workspace` + live workspace exercise.
    // The parser above is the pure-logic surface.
}
