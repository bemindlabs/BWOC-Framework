//! Tool registry and core tool implementations.
//!
//! Each tool:
//!   1. Declares its JSON schema (used to populate the `tools` array in the
//!      chat completion request).
//!   2. Implements [`ToolImpl`]: `name()` → `execute(args, ctx)`.
//!
//! Path confinement is enforced here (P1 minimal): any path that escapes
//! the working directory is rejected with [`HarnessError::PathEscape`].
//! Full sandbox (OS-level, allowlist, env scrub) is P2.

pub mod auth;
pub mod computer;
pub mod extra_tools;
pub mod impls;
pub mod registry;
pub mod session;
pub mod webfetch;

pub use auth::{CredentialBroker, CredentialRequest, InMemoryCredentialStore, ResolvedCredentials};
pub use extra_tools::{
    BwocSend, BwocTask, EditFile, Git, Glob, Grep, MemoryRead, MemoryWrite, MultiEdit, RunGates,
};
pub use impls::{ListDir, ReadFile, RunCommand, WriteFile};
pub use registry::ToolRegistry;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::error::HarnessError;

// ---------------------------------------------------------------------------
// Tool execution context (carries the working directory for path confinement)
// ---------------------------------------------------------------------------

/// Runtime context passed to every tool invocation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// The absolute path of the worktree / working directory. Relative tool
    /// paths resolve against this root, and — when [`confine`] is set — file
    /// operations are confined to it.
    ///
    /// [`confine`]: ToolContext::confine
    pub workdir: PathBuf,
    /// Whether file operations are confined to [`workdir`]. `true` (the default,
    /// via [`new`]) rejects any path that escapes the root — the path-traversal
    /// sandbox. `false` (via [`unconfined`], the harness `--unrestricted` flag)
    /// lets a tool touch any absolute path; the safety gate then shifts entirely
    /// to the permission policy (every write/edit/run is `ask`-gated). The
    /// workdir still serves as the base for *relative* paths either way.
    ///
    /// [`workdir`]: ToolContext::workdir
    /// [`new`]: ToolContext::new
    /// [`unconfined`]: ToolContext::unconfined
    pub confine: bool,
    /// The agent's tier-1 memory directory (`memory_read`/`memory_write` operate
    /// here). Defaults to `workdir/memories`; callers honor the manifest's
    /// `memoryPath` via [`with_memory_dir`] instead of hardcoding `memories/`.
    /// Expected to be `workdir`-rooted — the memory tools still run the
    /// path-confinement check (`starts_with(workdir)`) on every access, so a
    /// stray relative or escaping value is rejected at use, not trusted here.
    ///
    /// [`with_memory_dir`]: ToolContext::with_memory_dir
    pub memory_dir: PathBuf,
}

impl ToolContext {
    /// A confined context (the path-traversal sandbox) — the safe default.
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        let workdir = workdir.into();
        Self {
            memory_dir: workdir.join("memories"),
            workdir,
            confine: true,
        }
    }

    /// An unconfined context: file tools may touch any absolute path. Relative
    /// paths still resolve against `workdir`. The permission policy is the only
    /// remaining gate — use only where every action is `ask`/operator-reviewed.
    pub fn unconfined(workdir: impl Into<PathBuf>) -> Self {
        let workdir = workdir.into();
        Self {
            memory_dir: workdir.join("memories"),
            workdir,
            confine: false,
        }
    }

    /// Override the tier-1 memory directory (from the manifest's `memoryPath`).
    /// Builder-style so real run paths can honor the configured path while tests
    /// and eval keep the `workdir/memories` default.
    pub fn with_memory_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.memory_dir = dir.into();
        self
    }

    /// Resolve `raw` against the workdir, enforcing confinement when enabled.
    ///
    /// Returns the lexically-normalized absolute path. Does NOT require the path
    /// to exist yet (for write_file on new files). Confinement is two checks: a
    /// lexical `starts_with` on the normalized path, then a symlink-safe one —
    /// the deepest existing ancestor is canonicalized and must still sit inside
    /// the canonicalized workdir ([`crate::sandbox::is_confined`]), so a
    /// symlink inside the workdir that points outside is rejected. When
    /// [`confine`] is false both checks are skipped — any path is allowed.
    ///
    /// [`confine`]: ToolContext::confine
    pub fn resolve_path(&self, raw: &str) -> Result<PathBuf, HarnessError> {
        let p = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workdir.join(raw)
        };

        // Lexical normalisation: collapse `..` and `.` components.
        let normalized = normalize_path(&p);

        if self.confine
            && (!normalized.starts_with(&self.workdir)
                || !crate::sandbox::is_confined(&normalized, &self.workdir))
        {
            return Err(HarnessError::PathEscape(raw.to_string()));
        }

        Ok(normalized)
    }
}

/// Lexically normalize a path (collapse `..`/`.`) without hitting the
/// filesystem (so it works for paths that don't exist yet).
///
/// Public alias used by `extra_tools` for memory-path confinement.
pub fn normalize_path_pub(p: &Path) -> PathBuf {
    normalize_path(p)
}

fn normalize_path(p: &Path) -> PathBuf {
    use std::path::Component;
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
// Tool trait
// ---------------------------------------------------------------------------

/// Maximum bytes of tool-output **text** fed back to the model in one result.
///
/// A side-effect-free read (`read_file`, `grep`) is permitted on an Untrusted
/// turn precisely because it's cheap — so unbounded output is a context/cost DoS
/// the Layer-0 capability gate deliberately allows. The only prior cap in the
/// tool path was `grep`'s match count; a whole-file `read_file` or a chatty
/// `run_command` had none. Clamp once at the dispatch seam (see
/// [`clamp_tool_output`]) so every tool — and every MCP tool — inherits the
/// budget; on unix that seam runs inside the isolated child, so it also bounds
/// the IPC frame (#478).
pub const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;

/// Reserve, within [`MAX_TOOL_OUTPUT_BYTES`], for the truncation notice — so the
/// *final* returned string (kept content **plus** notice) stays within the
/// budget rather than overshooting it by the notice length. The notice is ~180
/// bytes plus a few digits; 256 is a safe ceiling.
const TRUNCATION_NOTICE_RESERVE: usize = 256;

/// Clamp tool output so the returned string is at most [`MAX_TOOL_OUTPUT_BYTES`],
/// truncating on a UTF-8 **char boundary** and appending an *actionable* notice.
///
/// The notice counts against the budget (via [`TRUNCATION_NOTICE_RESERVE`]), so
/// the total never exceeds the cap. Never a silent cut: a truncated read that
/// looks complete is how an agent writes an edit against content it never saw.
/// The notice names the next move (`read_file` with `offset`/`limit`; a tighter
/// `grep`). A naive byte slice would panic once `sandbox`/providers run the text
/// through `from_utf8_lossy`, hence the boundary walk.
pub fn clamp_tool_output(content: String) -> String {
    if content.len() <= MAX_TOOL_OUTPUT_BYTES {
        return content;
    }
    let total = content.len();
    let budget = MAX_TOOL_OUTPUT_BYTES.saturating_sub(TRUNCATION_NOTICE_RESERVE);
    let mut n = budget;
    while n > 0 && !content.is_char_boundary(n) {
        n -= 1;
    }
    let kept = &content[..n];
    format!(
        "{kept}\n\n[bwoc: tool output truncated — showed {n} of {total} bytes \
         (limit {MAX_TOOL_OUTPUT_BYTES}). Narrow the request: read a file with \
         `read_file` using `offset`/`limit`, or find a match with `grep` using a \
         tighter pattern or a narrower path.]"
    )
}

/// A tool's result: the text fed back to the model, plus any multimodal images
/// (e.g. a screenshot) to attach to the `tool_result` message. The common case
/// is text-only ([`ToolOutput::text`]); only visual tools populate `images`.
#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub content: String,
    pub images: Vec<crate::provider::types::ImageBlock>,
}

impl ToolOutput {
    /// A text-only result (no images) — the common case.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            images: Vec::new(),
        }
    }
}

/// A single callable tool.
#[async_trait]
pub trait ToolImpl: Send + Sync {
    /// Machine name used in `function.name`.
    fn name(&self) -> &'static str;

    /// Human-readable description for the model.
    fn description(&self) -> &'static str;

    /// JSON Schema for the `parameters` field.
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given parsed arguments.
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, HarnessError>;

    /// Execute and return a rich result that may carry images (screenshots).
    /// Defaults to text-only, wrapping [`Self::execute`]; visual tools override
    /// it. The isolated turn-executor calls this so images survive the IPC.
    async fn execute_rich(
        &self,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, HarnessError> {
        Ok(ToolOutput::text(self.execute(args, ctx).await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ctx(dir: &Path) -> ToolContext {
        ToolContext::new(dir.to_path_buf())
    }

    // Relative-path confinement tests use real temp dirs so they work on
    // both Unix (/tmp/…) and Windows (C:\Users\…\AppData\Local\Temp\…).

    #[test]
    fn resolve_path_relative_ok() {
        let tmp = TempDir::new().unwrap();
        let ctx = ctx(tmp.path());
        let p = ctx.resolve_path("src/main.rs").unwrap();
        assert_eq!(p, tmp.path().join("src/main.rs"));
    }

    #[test]
    fn clamp_leaves_small_output_untouched() {
        let s = "small output".to_string();
        assert_eq!(clamp_tool_output(s.clone()), s);
    }

    #[test]
    fn clamp_truncates_and_appends_actionable_notice() {
        let big = "a".repeat(MAX_TOOL_OUTPUT_BYTES * 2);
        let out = clamp_tool_output(big);
        assert!(out.len() < MAX_TOOL_OUTPUT_BYTES * 2, "must shrink");
        assert!(
            out.contains("truncated"),
            "must announce the cut: {out:.80}"
        );
        assert!(out.contains("offset"), "must name the next move");
    }

    #[test]
    fn clamp_total_stays_within_budget() {
        // The notice counts against the cap: kept + notice must not exceed it.
        let big = "a".repeat(MAX_TOOL_OUTPUT_BYTES * 2);
        let out = clamp_tool_output(big);
        assert!(
            out.len() <= MAX_TOOL_OUTPUT_BYTES,
            "clamped output {} exceeds the budget {MAX_TOOL_OUTPUT_BYTES}",
            out.len()
        );
    }

    #[test]
    fn clamp_never_splits_a_multibyte_char() {
        // A wall of 3-byte chars whose length lands the cut mid-char must not
        // panic and must stay valid UTF-8 (sandbox/providers run from_utf8_lossy).
        let big = "あ".repeat(MAX_TOOL_OUTPUT_BYTES); // 3 bytes each
        let out = clamp_tool_output(big);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.contains("truncated"));
    }

    #[test]
    fn resolve_path_dotdot_escape_rejected() {
        let tmp = TempDir::new().unwrap();
        let ctx = ctx(tmp.path());
        let err = ctx.resolve_path("../../etc/passwd").unwrap_err();
        assert!(matches!(err, HarnessError::PathEscape(_)));
    }

    #[test]
    fn resolve_path_absolute_inside_ok() {
        let tmp = TempDir::new().unwrap();
        let ctx = ctx(tmp.path());
        let inside = tmp.path().join("README.md");
        let p = ctx.resolve_path(inside.to_str().unwrap()).unwrap();
        assert_eq!(p, inside);
    }

    #[test]
    fn resolve_path_absolute_outside_rejected() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let ctx = ctx(tmp.path());
        let outside_path = outside.path().join("passwd");
        let err = ctx
            .resolve_path(outside_path.to_str().unwrap())
            .unwrap_err();
        assert!(matches!(err, HarnessError::PathEscape(_)));
    }

    #[test]
    fn unconfined_allows_outside_paths() {
        // `--unrestricted`: an absolute path outside the workdir is allowed
        // (the policy, not the sandbox, is the gate now).
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let ctx = ToolContext::unconfined(tmp.path().to_path_buf());
        let outside_path = outside.path().join("notes.txt");
        let p = ctx.resolve_path(outside_path.to_str().unwrap()).unwrap();
        assert_eq!(p, outside_path);
        // `..` escaping the workdir is likewise permitted when unconfined.
        assert!(ctx.resolve_path("../sibling/x").is_ok());
    }

    /// A workdir holding `inner/` (real dir), `inside_link -> inner`, and
    /// `escape_link -> <outside dir>` with `secret.txt` in the outside dir.
    #[cfg(unix)]
    fn symlinked_workdir() -> (TempDir, TempDir) {
        let wd = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "top secret").unwrap();
        std::fs::create_dir(wd.path().join("inner")).unwrap();
        std::fs::write(wd.path().join("inner/ok.txt"), "fine").unwrap();
        std::os::unix::fs::symlink(outside.path(), wd.path().join("escape_link")).unwrap();
        std::os::unix::fs::symlink(wd.path().join("inner"), wd.path().join("inside_link")).unwrap();
        (wd, outside)
    }

    #[cfg(unix)]
    #[test]
    fn resolve_path_rejects_symlink_escaping_the_workdir() {
        let (wd, _outside) = symlinked_workdir();
        let ctx = ctx(wd.path());
        for raw in [
            "escape_link",
            "escape_link/secret.txt",
            "escape_link/new.txt",
        ] {
            assert!(
                matches!(ctx.resolve_path(raw), Err(HarnessError::PathEscape(_))),
                "{raw} must be rejected"
            );
        }
        // A normal nested path and a symlink that stays inside still resolve.
        assert!(ctx.resolve_path("inner/ok.txt").is_ok());
        assert!(ctx.resolve_path("inside_link/ok.txt").is_ok());
        assert!(ctx.resolve_path("inner/not_yet.txt").is_ok());
        // `--unrestricted` is unchanged: the symlink is followed.
        let open = ToolContext::unconfined(wd.path().to_path_buf());
        assert!(open.resolve_path("escape_link/secret.txt").is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn file_tools_refuse_a_symlink_escape() {
        use serde_json::json;
        let (wd, outside) = symlinked_workdir();
        let ctx = ctx(wd.path());
        let read = ReadFile
            .execute(json!({"path": "escape_link/secret.txt"}), &ctx)
            .await;
        assert!(matches!(read, Err(HarnessError::PathEscape(_))), "{read:?}");
        let list = ListDir.execute(json!({"path": "escape_link"}), &ctx).await;
        assert!(matches!(list, Err(HarnessError::PathEscape(_))), "{list:?}");
        let write = WriteFile
            .execute(
                json!({"path": "escape_link/planted.txt", "content": "x"}),
                &ctx,
            )
            .await;
        assert!(
            matches!(write, Err(HarnessError::PathEscape(_))),
            "{write:?}"
        );
        assert!(!outside.path().join("planted.txt").exists());
        // grep never descends through the escaping link.
        let hits = Grep
            .execute(json!({"pattern": "top secret"}), &ctx)
            .await
            .unwrap();
        assert!(hits.starts_with("no matches"), "{hits}");
        // In-workdir reads through an inside symlink still work.
        let ok = ReadFile
            .execute(json!({"path": "inside_link/ok.txt"}), &ctx)
            .await
            .unwrap();
        assert_eq!(ok, "fine");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn memory_tools_refuse_a_symlinked_memory_dir() {
        use serde_json::json;
        let wd = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("MEMORY.md"), "leak").unwrap();
        std::os::unix::fs::symlink(outside.path(), wd.path().join("memories")).unwrap();
        let ctx = ctx(wd.path());
        let read = MemoryRead.execute(json!({}), &ctx).await;
        assert!(matches!(read, Err(HarnessError::PathEscape(_))), "{read:?}");
        let write = MemoryWrite
            .execute(json!({"name": "x.md", "content": "x"}), &ctx)
            .await;
        assert!(
            matches!(write, Err(HarnessError::PathEscape(_))),
            "{write:?}"
        );
        assert!(!outside.path().join("x.md").exists());
    }

    #[test]
    fn unconfined_relative_still_resolves_against_workdir() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::unconfined(tmp.path().to_path_buf());
        let p = ctx.resolve_path("src/main.rs").unwrap();
        assert_eq!(p, tmp.path().join("src/main.rs"));
    }
}
