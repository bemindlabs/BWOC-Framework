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

pub use auth::{CredentialBroker, CredentialRequest, InMemoryCredentialStore, ResolvedCredentials};
pub use extra_tools::{BwocSend, BwocTask, EditFile, Git, Grep, MemoryRead, MemoryWrite, RunGates};
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
}

impl ToolContext {
    /// A confined context (the path-traversal sandbox) — the safe default.
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            confine: true,
        }
    }

    /// An unconfined context: file tools may touch any absolute path. Relative
    /// paths still resolve against `workdir`. The permission policy is the only
    /// remaining gate — use only where every action is `ask`/operator-reviewed.
    pub fn unconfined(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            confine: false,
        }
    }

    /// Resolve `raw` against the workdir, enforcing confinement when enabled.
    ///
    /// Returns the lexically-normalized absolute path. Does NOT require the path
    /// to exist yet (for write_file on new files), so we use `Path::starts_with`
    /// on the normalized path rather than `fs::canonicalize`. When [`confine`] is
    /// false the escape check is skipped — any absolute path is allowed.
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

        if self.confine && !normalized.starts_with(&self.workdir) {
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

    #[test]
    fn unconfined_relative_still_resolves_against_workdir() {
        let tmp = TempDir::new().unwrap();
        let ctx = ToolContext::unconfined(tmp.path().to_path_buf());
        let p = ctx.resolve_path("src/main.rs").unwrap();
        assert_eq!(p, tmp.path().join("src/main.rs"));
    }
}
