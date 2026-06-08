//! Worker result envelope (HV3-3b) — the structured outcome a Saṅgha worker
//! leaves in its worktree for the lead to collect, in place of a bare exit
//! code.
//!
//! A worker is a `bwoc-harness` subprocess in its own git worktree (see
//! [`crate::worker`]); it cannot return a value to the lead in-process. So at
//! session end the worker **writes** [`WorkerResult`] to
//! `<worktree>/.bwoc/worker-result.json`, and the lead **reads** it after the
//! child exits — before tearing the worktree down. The envelope carries what
//! the lead can't otherwise see (turns, the model that answered, a one-line
//! summary) plus a [`DiffSummary`] of what the worker changed.
//!
//! Best-effort throughout: a worker that predates this envelope (or fails to
//! write it) simply leaves none, and the lead falls back to the exit code it
//! already has. This is the seam HV3-3c's peer-review gate taps — the reviewer
//! reads the same envelope before the lead runs gates.

use std::path::Path;
// `PathBuf` only feeds the `#[cfg(unix)]` FS-jail path (`git_common_dir`); gate
// the import so Windows — where the jail is absent by design — has no unused use.
#[cfg(unix)]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Path, relative to a worktree, where the worker writes its result envelope
/// and the lead reads it.
pub const RESULT_FILE: &str = ".bwoc/worker-result.json";

/// Phase 5 t7a / C7 — config keys forced on every `git` the PARENT runs over a
/// (possibly child-touched) worktree. Command-line `-c` overrides outrank any
/// repo-local `.git/config`, so a worktree that planted a `core.hooksPath`,
/// `core.fsmonitor`, `diff.external`, `core.pager`, or `core.sshCommand` to get
/// code executed when the parent shells out to git is neutralized. Defence in
/// depth on top of the FS jail (the spawned git is also Landlock/sandbox-exec
/// confined to the worktree + its git dir).
const GIT_HARDENING: &[&str] = &[
    "core.hooksPath=/dev/null",
    "core.fsmonitor=false",
    "core.pager=cat",
    "core.sshCommand=/bin/false",
    "diff.external=",
    "protocol.ext.allow=never",
];

/// Final assistant messages can be long; the envelope keeps a bounded preview
/// so the lead's log and any downstream reviewer stay readable.
const SUMMARY_MAX: usize = 600;

/// A worktree's net changes versus the base `HEAD` it branched off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

impl DiffSummary {
    /// Summarise `<worktree>` against its base `HEAD`: tracked edits from
    /// `git diff --numstat HEAD` plus any untracked files. Best-effort — a
    /// non-repo path or a git failure yields an all-zero summary rather than an
    /// error (the envelope is advisory, never load-bearing for correctness).
    pub fn from_worktree(worktree: &Path) -> Self {
        let numstat = git_output(worktree, &["diff", "--numstat", "HEAD"]).unwrap_or_default();
        let (mut files, ins, del) = parse_numstat(&numstat);
        // Untracked files don't appear in `diff` — count them so a worker that
        // only adds new files isn't reported as a no-op.
        if let Some(out) = git_output(worktree, &["ls-files", "--others", "--exclude-standard"]) {
            files += out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
        }
        DiffSummary {
            files_changed: files,
            insertions: ins,
            deletions: del,
        }
    }
}

/// Parse `git diff --numstat` output into `(files, insertions, deletions)`.
/// Binary files render as `-\t-\t<path>`; they count toward `files` but
/// contribute no line deltas. Pure so it is testable without a repo.
fn parse_numstat(numstat: &str) -> (u32, u32, u32) {
    let (mut files, mut ins, mut del) = (0u32, 0u32, 0u32);
    for line in numstat.lines() {
        let mut cols = line.split('\t');
        let (Some(a), Some(b)) = (cols.next(), cols.next()) else {
            continue;
        };
        files += 1;
        ins += a.trim().parse::<u32>().unwrap_or(0);
        del += b.trim().parse::<u32>().unwrap_or(0);
    }
    (files, ins, del)
}

/// Run `git -C <worktree> <args…>`, returning trimmed stdout on exit 0.
///
/// Phase 5 t7a / C7 — the worktree may have been mutated by an isolated child
/// turn, so it is **untrusted** here. Two layers protect the parent:
///
/// 1. [`GIT_HARDENING`] `-c` overrides neutralize config-driven RCE vectors;
/// 2. the git process runs inside the same FS jail (Landlock on Linux,
///    sandbox-exec write-confinement on macOS), rw-restricted to the worktree +
///    its git common dir, so even an unforeseen vector cannot read secrets or
///    write outside the worktree.
fn git_output(worktree: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(worktree);
    for kv in GIT_HARDENING {
        cmd.arg("-c").arg(kv);
    }
    cmd.args(args);
    // C7 — point global/system config at /dev/null. Two wins: git never reads
    // `~/.gitconfig` / `/etc/gitconfig` (which the FS jail would `EACCES`, so a
    // diff/ls-files would otherwise fail), and a malicious global config cannot
    // reintroduce a hook/pager/fsmonitor vector. The repo-local `.git/config`
    // (inside the jail's rw set) is still read but is fully overridden by the
    // `-c` flags above.
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");

    // C7 — confine git to the worktree + its git dir. Best-effort: on a kernel
    // without Landlock (or a non-jail platform) the `-c` hardening above still
    // stands; `jail_command` emits a LOUD warning, never a silent pass.
    //
    // The process FS jail (`jail::jail_command`: Landlock on Linux, sandbox-exec
    // write-confine on macOS) is `#[cfg(unix)]` by design. On Windows it is
    // simply ABSENT — git here runs with the cross-platform `-c` config
    // hardening above but no FS jail. Gate the call so the crate compiles on
    // Windows; do NOT stub a no-op jail that would falsely claim confinement.
    #[cfg(unix)]
    {
        let spec = crate::jail::JailSpec::for_git(worktree, git_common_dir(worktree).as_deref());
        let _ = crate::jail::jail_command(&mut cmd, &spec);
    }

    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Resolve the git **common dir** for `worktree` so the C7 jail can grant git
/// rw there (a linked worktree's gitdir lives outside the worktree). Returns
/// `None` for a standard repo (`.git` is a dir under the worktree, already in
/// the jail's rw set) or when resolution fails (best-effort — the jail then
/// covers only the worktree, and the diff summary degrades to zero rather than
/// breaking).
///
/// `#[cfg(unix)]`: its sole caller is the FS-jail spec in [`git_output`], which
/// is itself unix-only. Windows has no jail, so it needs no common-dir lookup.
#[cfg(unix)]
fn git_common_dir(worktree: &Path) -> Option<PathBuf> {
    let dot_git = worktree.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return None;
    }
    // Linked worktree: `.git` is a file `gitdir: <path>`.
    let content = std::fs::read_to_string(&dot_git).ok()?;
    let raw = content.lines().find_map(|l| l.strip_prefix("gitdir:"))?;
    let mut gitdir = PathBuf::from(raw.trim());
    if gitdir.is_relative() {
        gitdir = worktree.join(gitdir);
    }
    // gitdir = <repo>/.git/worktrees/<name>; the common dir is <repo>/.git,
    // which holds both this worktree's gitdir and the shared objects/refs.
    gitdir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or(Some(gitdir))
}

/// The structured outcome of one worker run, written to [`RESULT_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerResult {
    /// The task prompt the worker ran (its title) — self-description for a
    /// reader that only has the worktree.
    pub task: String,
    /// Whether the agentic loop completed, as opposed to aborting on a budget /
    /// max-iterations / models-exhausted guard.
    pub success: bool,
    /// Agentic turns taken.
    pub turns: u32,
    /// Context compactions performed.
    pub compactions: u32,
    /// Token-pressure model switches performed.
    pub token_pressure_switches: u32,
    /// Model that produced the final answer (may differ from the requested one
    /// after fallback).
    pub active_model: String,
    /// Net working-tree changes versus the worktree's base `HEAD`.
    pub diff: DiffSummary,
    /// Bounded preview of the final assistant message (or the abort reason on
    /// failure).
    pub summary: String,
}

impl WorkerResult {
    /// Envelope for a run that completed — metrics drawn from its
    /// [`LoopResult`], `summary` the (truncated) final response.
    pub fn completed(
        task: impl Into<String>,
        res: &crate::agent_loop::LoopResult,
        diff: DiffSummary,
    ) -> Self {
        Self {
            task: task.into(),
            success: true,
            turns: res.turns,
            compactions: res.compactions,
            token_pressure_switches: res.token_pressure_switches,
            active_model: res.active_model.clone(),
            diff,
            summary: truncate(&res.final_response, SUMMARY_MAX),
        }
    }

    /// Envelope for a run that aborted (budget / max-iterations / models
    /// exhausted) — no `LoopResult`, so metrics are zero and `reason` is the
    /// abort message; `model` is the requested one (no answer was produced).
    pub fn aborted(
        task: impl Into<String>,
        model: impl Into<String>,
        diff: DiffSummary,
        reason: &str,
    ) -> Self {
        Self {
            task: task.into(),
            success: false,
            turns: 0,
            compactions: 0,
            token_pressure_switches: 0,
            active_model: model.into(),
            diff,
            summary: truncate(reason, SUMMARY_MAX),
        }
    }

    /// Bare constructor for tests — full control over every field.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task: impl Into<String>,
        success: bool,
        turns: u32,
        compactions: u32,
        token_pressure_switches: u32,
        active_model: impl Into<String>,
        diff: DiffSummary,
        summary: &str,
    ) -> Self {
        Self {
            task: task.into(),
            success,
            turns,
            compactions,
            token_pressure_switches,
            active_model: active_model.into(),
            diff,
            summary: truncate(summary, SUMMARY_MAX),
        }
    }

    /// Write the envelope to `<worktree>/.bwoc/worker-result.json`, creating
    /// `.bwoc/` if needed. Pretty JSON so a human can read a stranded result.
    pub fn write(&self, worktree: &Path) -> std::io::Result<()> {
        let path = worktree.join(RESULT_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }

    /// Read the envelope from `<worktree>/.bwoc/worker-result.json`. `None` when
    /// it is absent or unparseable — the lead degrades to the exit code.
    pub fn read(worktree: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(worktree.join(RESULT_FILE)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// One-line summary for the lead's completion log.
    pub fn one_line(&self) -> String {
        let d = &self.diff;
        format!(
            "{} turn(s), +{}/-{} across {} file(s), model={}",
            self.turns, d.insertions, d.deletions, d.files_changed, self.active_model
        )
    }
}

/// Truncate to `max` chars on a char boundary, appending an ellipsis marker
/// when cut so a reader knows the preview is partial.
fn truncate(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let cut: String = trimmed.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn parse_numstat_sums_files_and_lines() {
        let s = "3\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n";
        assert_eq!(parse_numstat(s), (2, 13, 1));
    }

    #[test]
    fn parse_numstat_counts_binary_as_file_no_lines() {
        let s = "-\t-\tassets/logo.png\n5\t2\tsrc/c.rs\n";
        assert_eq!(parse_numstat(s), (2, 5, 2));
    }

    #[test]
    fn parse_numstat_empty_is_zero() {
        assert_eq!(parse_numstat(""), (0, 0, 0));
    }

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("  hi  ", 10), "hi");
    }

    #[test]
    fn truncate_cuts_long_strings_on_char_boundary() {
        let out = truncate(&"ก".repeat(50), 10);
        assert_eq!(out.chars().count(), 11); // 10 + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn write_then_read_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let wr = WorkerResult::new(
            "do the thing",
            true,
            3,
            1,
            0,
            "gemma4",
            DiffSummary {
                files_changed: 2,
                insertions: 12,
                deletions: 4,
            },
            "all done",
        );
        wr.write(tmp.path()).unwrap();
        let back = WorkerResult::read(tmp.path()).expect("envelope reads back");
        assert_eq!(wr, back);
        assert_eq!(
            back.one_line(),
            "3 turn(s), +12/-4 across 2 file(s), model=gemma4"
        );
    }

    #[test]
    fn read_absent_envelope_is_none() {
        let tmp = TempDir::new().unwrap();
        assert!(WorkerResult::read(tmp.path()).is_none());
    }

    // Git is assumed present on every CI leg (the worker worktree tests already
    // shell out to it unconditionally), so this integration test is not gated.
    #[test]
    fn diff_summary_counts_tracked_edits_and_untracked_files() {
        let repo = TempDir::new().unwrap();
        let r = repo.path();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(r)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t.t"]);
        git(&["config", "user.name", "t"]);
        std::fs::write(r.join("tracked.txt"), "one\ntwo\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "seed"]);

        // Edit a tracked file (+1 line) and add an untracked one.
        std::fs::write(r.join("tracked.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(r.join("new.txt"), "x\n").unwrap();

        let d = DiffSummary::from_worktree(r);
        assert_eq!(d.files_changed, 2, "1 tracked edit + 1 untracked: {d:?}");
        assert_eq!(d.insertions, 1);
        assert_eq!(d.deletions, 0);
    }

    #[test]
    fn diff_summary_non_repo_is_zero() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(
            DiffSummary::from_worktree(tmp.path()),
            DiffSummary::default()
        );
    }
}
