//! Durable run checkpoints (HV2-2).
//!
//! Persists agent-loop state after each completed turn so a long run survives a
//! crash or restart.  The turn boundary (after the assistant reply and any tool
//! results have been applied to `history`) is the only consistent seam — tools
//! mutate the worktree mid-turn, so a mid-turn snapshot could disagree with what
//! is on disk.  The worktree itself already persists, so resuming is *reload +
//! re-attach to the existing worktree* — there is **no replay** of past turns.
//!
//! Writes are atomic: a sibling temp file is written then renamed over the
//! target.  POSIX rename is atomic, so a reader (or a `--resume`) never observes
//! a partially-written checkpoint even if the process is killed mid-write.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::provider::ChatMessage;

/// Serializable snapshot of [`run_loop`](crate::agent_loop::run_loop) state.
///
/// Mirrors the loop's durable locals; transient per-turn scratch (retry
/// counters, the provider-limit cache) is intentionally omitted — it is cheap
/// to rebuild and not needed to continue correctly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    /// Unique id for this run; names the directory under the runs root.
    pub run_id: String,
    /// The originating task prompt (informational; survives resume).
    pub task: String,
    /// Full message history, including the system prompt — replayed verbatim,
    /// not regenerated, on resume.
    pub history: Vec<ChatMessage>,
    /// Completed turns so far.
    pub turns: u32,
    /// Context compactions performed.
    pub compactions: u32,
    /// Token-pressure–driven model switches performed.
    pub token_pressure_switches: u32,
    /// Model active at checkpoint time (may differ from the configured primary
    /// after a fallback or token-pressure switch).
    pub active_model: String,
    /// Canonical worktree path this run executed in.  Validated on resume — the
    /// re-attached `--workdir` must match, or the replayed `history` would
    /// describe files in a directory the resumed run isn't pointed at.  Empty
    /// (the `serde(default)`) on checkpoints written before this field existed,
    /// in which case the resume guard is skipped.
    #[serde(default)]
    pub workdir: PathBuf,
}

impl RunState {
    /// Atomically write this state as pretty JSON to `path` (temp-write +
    /// rename).  Creates the parent directory if missing.  Uses only `std` —
    /// no extra runtime dependency — to keep the dep-quarantine honest: the
    /// temp file is a sibling in the same directory, so the rename is atomic
    /// and never crosses a filesystem boundary.
    pub fn save_atomic(&self, path: &Path) -> io::Result<()> {
        let dir = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "checkpoint path has no parent")
        })?;
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = dir.join(format!(".checkpoint.{}.{nanos}.tmp", std::process::id()));

        // Write + fsync the temp file, then rename over the target.  Clean up
        // the temp on any failure so a crashed write leaves no litter.
        let write = (|| -> io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&json)?;
            f.sync_all()
        })();
        if let Err(e) = write {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }

    /// Read a checkpoint from `path`.
    pub fn load_from(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }
}

/// Per-run checkpoint wiring carried on [`LoopConfig`](crate::agent_loop::LoopConfig).
///
/// `None` on the config means durability is disabled (the historical
/// behaviour, preserved for tests and embedders).  `Some` enables a per-turn
/// save under `<root>/<run_id>/checkpoint.json`; a non-`None` [`resume`] seeds
/// the loop from a prior run.
#[derive(Debug, Clone)]
pub struct CheckpointConfig {
    /// Run id — names the directory under `root`.
    pub run_id: String,
    /// Root directory for run checkpoints.  Normally [`runs_root`]; tests
    /// override it to a temp dir for isolation (no process-global env races).
    pub root: PathBuf,
    /// When resuming, the previously-saved state to seed the loop from.
    pub resume: Option<RunState>,
}

impl CheckpointConfig {
    /// A fresh run rooted at the default runs directory ([`runs_root`]).
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            root: runs_root(),
            resume: None,
        }
    }

    /// Resume `run_id` by loading its checkpoint from the default runs dir.
    pub fn resume(run_id: impl Into<String>) -> io::Result<Self> {
        let run_id = run_id.into();
        let root = runs_root();
        let resume = RunState::load_from(&checkpoint_path(&root, &run_id))?;
        Ok(Self {
            run_id,
            root,
            resume: Some(resume),
        })
    }

    /// Path to this run's `checkpoint.json`.
    pub fn path(&self) -> PathBuf {
        checkpoint_path(&self.root, &self.run_id)
    }

    /// Atomically persist `state` to this run's checkpoint path.
    pub fn save(&self, state: &RunState) -> io::Result<()> {
        state.save_atomic(&self.path())
    }

    /// Remove this run's checkpoint directory.  Called on successful
    /// completion — a finished run has nothing to resume (Anattā: no clinging
    /// to completed state).  Best-effort; an absent directory is success.
    pub fn delete(&self) -> io::Result<()> {
        let dir = self.root.join(&self.run_id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

fn checkpoint_path(root: &Path, run_id: &str) -> PathBuf {
    root.join(run_id).join("checkpoint.json")
}

/// Default root for run checkpoints: `$BWOC_HOME/runs` when `BWOC_HOME` is set,
/// else `$HOME/.bwoc/runs`, else `./.bwoc/runs` (never panics in a headless
/// environment with neither variable set).
pub fn runs_root() -> PathBuf {
    if let Some(h) = std::env::var_os("BWOC_HOME") {
        return PathBuf::from(h).join("runs");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".bwoc").join("runs");
    }
    PathBuf::from(".bwoc").join("runs")
}

/// Generate a fresh run id: `run-<unix-secs>-<pid>` (no extra dep for a UUID).
pub fn new_run_id() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("run-{secs}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_state(run_id: &str) -> RunState {
        RunState {
            run_id: run_id.to_string(),
            task: "do the thing".to_string(),
            history: vec![
                ChatMessage::system("you are a bwoc agent"),
                ChatMessage::user("do the thing"),
                ChatMessage::assistant(Some("on it".to_string()), None),
            ],
            turns: 3,
            compactions: 1,
            token_pressure_switches: 0,
            active_model: "mock".to_string(),
            workdir: PathBuf::from("/tmp/bwoc-wt"),
        }
    }

    #[test]
    fn save_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cfg = CheckpointConfig {
            run_id: "r1".to_string(),
            root: tmp.path().to_path_buf(),
            resume: None,
        };
        let state = sample_state("r1");
        cfg.save(&state).unwrap();

        let loaded = RunState::load_from(&cfg.path()).unwrap();
        // No PartialEq on the wire types; compare via canonical JSON.
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::to_value(&loaded).unwrap()
        );
    }

    #[test]
    fn save_is_atomic_no_partial_file() {
        // After a save the target exists and parses fully; the temp sibling is
        // gone (persist renamed it). A directory listing holds exactly one file.
        let tmp = TempDir::new().unwrap();
        let cfg = CheckpointConfig {
            run_id: "r2".to_string(),
            root: tmp.path().to_path_buf(),
            resume: None,
        };
        cfg.save(&sample_state("r2")).unwrap();

        let dir = tmp.path().join("r2");
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "no leftover temp file beside checkpoint");
        assert!(RunState::load_from(&cfg.path()).is_ok());
    }

    #[test]
    fn delete_removes_run_dir_and_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let cfg = CheckpointConfig {
            run_id: "r3".to_string(),
            root: tmp.path().to_path_buf(),
            resume: None,
        };
        cfg.save(&sample_state("r3")).unwrap();
        assert!(cfg.path().exists());

        cfg.delete().unwrap();
        assert!(!cfg.path().exists());
        // Idempotent: deleting an already-absent run is Ok.
        cfg.delete().unwrap();
    }
}
